# SPDX-License-Identifier: MIT
"""Explicit, cancellable Store checks. No checkout or order submission."""
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import threading
import time
import uuid
import xml.etree.ElementTree as ET

from . import bootstrap, games, store_diagnostics
from .game_install import run_cli
from .backend import LauncherError, utc_now

STAGES = ("runtime", "account", "catalog", "license", "library", "window")
CODES = {"checking", "available", "verified", "local_session", "visible", "sign_in_required", "expired",
         "not_licensed", "unsupported", "invalid_config", "connection", "timeout", "account_changed",
         "keyring", "error", "cancelled", "runtime_update", "incomplete"}
CODES.add("signing_in")
STATES = {"running", "passed", "failed", "skipped", "cancelled"}


def config(runtime):
    from .cloud_runtime import _config, _read
    info = _config(runtime)  # Same validated public title and publisher family as the game.
    tree = ET.fromstring(info["config"])
    apps = [n.text for n in tree.iter() if n.tag.rsplit("}", 1)[-1] == "MSAAppId"]
    settings = json.loads(_read(runtime / "private/runtime.json", 65536))
    market = settings.get("market")
    if len(apps) != 1 or not isinstance(apps[0], str) or not re.fullmatch(r"(?:[0-9a-fA-F]{16}|[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12})", apps[0]) or not isinstance(market, str) or not re.fullmatch(r"[A-Z]{2}", market):
        raise ValueError("invalid public title config")
    return {"store_id": games.for_runtime(runtime).store_id, "title_id": info["title_id"],
            "msa_app_id": apps[0], "package_family_name": info["pfn"], "market": market}


def verify_runtime(runtime, source_root):
    from .runtime_components import _targets, _regular
    from .setup import digest
    _, lockpath = bootstrap.paths(source_root)
    lock = json.loads(lockpath.read_text())
    for name, paths in _targets(runtime).items():
        expected = lock["native"]["files"][name]
        for path in paths:
            _regular(runtime, path)
            if digest(path) != expected:
                raise ValueError("runtime update required")
    return store_diagnostics.launch_record(runtime)


class StoreCheck:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.thread = None
        self.cancelled = threading.Event()
        self.job = None
        self.runtime = None

    def snapshot(self):
        with self.lock:
            return {"job": copy.deepcopy(self.job) if self.runtime == self.launcher.runtime else None}

    def report(self):
        job = self.snapshot()["job"]
        return {key: job[key] for key in ("state", "started_at", "finished_at", "steps", "components")} if job else None

    def start(self, language="en", *, recover=False, job_id=None, cloud_request_id=None):
        with self.launcher.lock, self.lock:
            if self.thread and self.thread.is_alive():
                raise LauncherError("Eine Store-Prüfung läuft bereits.")
            if self.launcher.runtime is None:
                raise LauncherError("Zuerst eine Runtime auswählen.")
            if job_id is not None and (not self.job or job_id != self.job["id"] or self.runtime != self.launcher.runtime):
                raise LauncherError("Diese Store-Prüfung ist nicht mehr aktuell. Bitte den Status neu laden.")
            if cloud_request_id is not None:
                cloud = self.launcher.cloud_saves.automation.snapshot()
                if not recover or not cloud["can_sign_in"] or cloud_request_id != cloud["request_id"]:
                    raise LauncherError("Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.")
            self.launcher._require_cloud_idle(allow_attention=True)
            self.launcher.reserve_setup()
            self.runtime = self.launcher.runtime
            self.cancelled.clear()
            stages = ("runtime", "sign_in", "account", "catalog", "license", "library") if recover else STAGES
            self.job = {"id": uuid.uuid4().hex, "state": "running", "operation": "recover" if recover else "check",
                        "started_at": utc_now(), "finished_at": None,
                        "steps": [{"stage": stage, "state": "pending", "code": "checking"} for stage in stages], "components": None}
            self.thread = threading.Thread(target=self._run, args=(self.runtime, language, recover, cloud_request_id), daemon=False)
            try:
                self.thread.start()
            except Exception:
                self.launcher.release_setup()
                self.job.update(state="failed", finished_at=utc_now())
                raise
            return {"ok": True, **self.snapshot()}

    def cancel(self, job_id):
        with self.lock:
            if not self.job or job_id != self.job["id"] or self.job["state"] != "running":
                raise LauncherError("Diese Store-Prüfung ist nicht mehr aktiv.")
            self.cancelled.set()
            return {"ok": True, **self.snapshot()}

    def _step(self, stage, state, code):
        with self.lock:
            row = next(r for r in self.job["steps"] if r["stage"] == stage)
            row.update(state=state, code=code)

    def _event(self, raw, allowed):
        try:
            v = json.loads(raw)
            if v in ({"stage":"check", "state":"failed", "code":"timeout"}, {"stage":"check", "state":"failed", "code":"error"}):
                self._finish_pending(allowed, v["code"])
                return True
            if (not isinstance(v, dict) or set(v) != {"stage", "state", "code"} or v["stage"] not in allowed
                    or v["state"] not in STATES or v["code"] not in CODES):
                return False
            successes = {"account":"local_session", "catalog":"available", "license":"verified", "library":"available", "window":"visible"}
            if v["state"] == "passed" and v["code"] != successes.get(v["stage"]):
                return False
            if v["state"] == "running" and v["code"] != "checking":
                return False
            with self.lock:
                row = next(r for r in self.job["steps"] if r["stage"] == v["stage"])
                # A terminal result cannot be replaced by subsequent messages.
                if row["state"] not in {"pending", "running"}:
                    return False
                self._step(v["stage"], v["state"], v["code"])
            return True
        except (ValueError, TypeError, KeyError, StopIteration):
            return False

    def _process(self, command, payload, allowed, timeout):
        env = os.environ.copy()
        env.update(XODUS_LOG="off", RUST_LOG="off", RUST_BACKTRACE="0")
        # Diagnostics never reuse the launch marker of a previous game.
        env.pop("FLIGHTDECK_STORE_LAUNCH", None)
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                 cwd=self.runtime, env=env, start_new_session=True, close_fds=True, umask=0o077)
        deadline = time.monotonic() + timeout
        pending = b""
        total = 0
        try:
            child.stdin.write(payload)
            child.stdin.close()
            with selectors.DefaultSelector() as selector:
                selector.register(child.stdout, selectors.EVENT_READ)
                while selector.get_map():
                    if self.cancelled.is_set():
                        return "cancelled"
                    if time.monotonic() >= deadline:
                        return "timeout"
                    for key, _ in selector.select(0.1):
                        data = os.read(key.fileobj.fileno(), 4096)
                        if not data:
                            selector.unregister(key.fileobj)
                            continue
                        total += len(data)
                        if total > 65536:
                            return "error"
                        pending += data
                        while b"\n" in pending:
                            line, pending = pending.split(b"\n", 1)
                            if len(line) > 1024 or not self._event(line, allowed):
                                return "error"
                while child.poll() is None:
                    if self.cancelled.wait(0.1):
                        return "cancelled"
                    if time.monotonic() >= deadline:
                        return "timeout"
                return "available" if child.returncode == 0 and not pending else "error"
        finally:
            # Stop this exact process group, including a native test window.
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=2)
            child.stdout.close()
            if not child.stdin.closed:
                child.stdin.close()

    def _finish_pending(self, stages, code):
        with self.lock:
            for row in self.job["steps"]:
                if row["stage"] in stages and row["state"] in {"pending", "running"}:
                    row.update(state="cancelled" if code == "cancelled" else "failed", code=code)

    def _run(self, runtime, language, recover=False, cloud_request_id=None):
        try:
            with self.launcher.runtime_lock(operation="store-check"):
                self._step("runtime", "running", "checking")
                try:
                    components = verify_runtime(runtime, self.launcher.setup.source_root)
                    if components is None:
                        raise ValueError()
                    with self.lock:
                        self.job["components"] = components
                except (OSError, ValueError, KeyError, TypeError):
                    self._step("runtime", "failed", "runtime_update")
                    return
                self._step("runtime", "passed", "verified")
                if recover:
                    self._step("sign_in", "running", "signing_in")
                    result = run_cli(runtime / "bin/xodus-cli", ["login", "--same-account"],
                                     cwd=runtime / "private", cancel=self.cancelled, timeout=900,
                                     xdg_root=runtime / "private/xdg")
                    if result:
                        self._step("sign_in", "failed", "sign_in_required")
                        return
                    self._step("sign_in", "passed", "verified")
                online = {"account", "catalog", "license", "library"}
                try:
                    payload = json.dumps(config(runtime)).encode()
                except Exception:
                    self._finish_pending(online, "invalid_config")
                else:
                    code = self._process([str(runtime / "bin/xodus-service"), "--store-check"], payload, online, 125)
                    self._finish_pending(online, code if code != "available" else "incomplete")
                if not recover and not self.cancelled.is_set():
                    self._step("window", "running", "checking")
                    code = self._process([str(runtime / "bin/xodus-cli"), "store-window-check", "--language", "de" if language == "de" else "en"], b"", {"window"}, 95)
                    self._finish_pending({"window"}, code if code != "available" else "incomplete")
        except Exception:
            self._finish_pending({r["stage"] for r in self.job["steps"]}, "cancelled" if self.cancelled.is_set() else "error")
        finally:
            with self.lock:
                for row in self.job["steps"]:
                    if row["state"] in {"pending", "running"}:
                        row.update(state="cancelled" if self.cancelled.is_set() else "skipped", code="cancelled" if self.cancelled.is_set() else "incomplete")
                states = {r["state"] for r in self.job["steps"]}
                self.job.update(state="passed" if states == {"passed"} else "cancelled" if self.cancelled.is_set() else "failed" if "failed" in states else "incomplete", finished_at=utc_now())
            # Release the lease before resuming precisely the cloud operation
            # which requested login. A changed runtime/request can never resume.
            with self.launcher.lock:
                self.launcher.release_setup()
                if (cloud_request_id and self.job["state"] == "passed" and not self.cancelled.is_set()
                        and self.launcher.runtime == runtime):
                    try:
                        self.launcher.cloud_saves.automation.action("retry", cloud_request_id)
                    except LauncherError:
                        pass  # The original cloud attention remains actionable.

    def close(self):
        self.cancelled.set()
        if self.thread and self.thread is not threading.current_thread():
            self.thread.join(timeout=6)
