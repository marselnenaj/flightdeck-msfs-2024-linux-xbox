#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Check real native installation, service handoff and legacy rollback in isolation."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


class RequestFailure(AssertionError):
    def __init__(self, status, path, value):
        super().__init__((status, path, value))
        self.status = status


def request(record, path, data=None):
    connection = http.client.HTTPConnection("127.0.0.1", record["port"], timeout=5)
    try:
        headers = {"X-Flightdeck-Token": record["token"], "Content-Type": "application/json"}
        connection.request("POST" if data is not None else "GET", path,
                           body=json.dumps(data) if data is not None else None, headers=headers)
        response = connection.getresponse()
        value = json.loads(response.read())
        if response.status != 200:
            raise RequestFailure(response.status, path, value)
        return value
    finally:
        connection.close()


def wait_for(check, timeout=60):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, http.client.HTTPException) as error:
            last = error
        time.sleep(.05)
    raise AssertionError("Timed out waiting for the isolated service") from last


def record_at(state):
    record = json.loads((state / "desktop-service.json").read_text())
    try:
        status = request(record, "/api/status")
    except RequestFailure as error:
        if error.status not in (401, 403):
            raise
        # The replacement may rotate authentication after the record was read.
        # Only bounded discovery polling retries this; mutation errors stay fatal.
        raise ValueError("Service authentication changed during handoff") from error
    if status["csrf_token"] != record["token"] or status["service"]["release"] != record["release"]:
        # The replacement can publish its record between our file read and
        # HTTP request. Retry this observation; never reuse the stale token.
        raise ValueError("Service record changed during handoff")
    assert not status["runtime"]["configured"]
    return record


def main(args):
    package = args.package.resolve(strict=True)
    binary = package / "bin/flightdeck"
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    environment = dict(os.environ)
    environment.update(HOME=str(output), DISPLAY="", WAYLAND_DISPLAY="")
    for name in ("CONFIG", "CACHE", "DATA", "STATE"):
        environment["XDG_" + name + "_HOME"] = str(output / name.lower())
    environment.update(HTTPS_PROXY="http://127.0.0.1:1", HTTP_PROXY="http://127.0.0.1:1", NO_PROXY="127.0.0.1,localhost")
    environment.pop("PYTHONPATH", None)
    environment.pop("PYTHONHOME", None)
    browser_bin = output / "fake-browser"
    browser_bin.mkdir()
    browser = browser_bin / "chromium"
    browser.write_text('#!/bin/sh\nprintf \'%s\\n\' "$@" > "$FLIGHTDECK_TEST_BROWSER_LOG"\n')
    browser.chmod(0o700)
    browser_log = output / "browser-arguments.txt"
    environment["FLIGHTDECK_TEST_BROWSER_LOG"] = str(browser_log)
    environment["PATH"] = str(browser_bin) + os.pathsep + environment.get("PATH", "")
    processes, states, results = [], [], {}

    def run(command, no_python=False):
        env = dict(environment)
        if no_python:
            env["PATH"] = "/no-programs-on-path"
        completed = subprocess.run(command, cwd=output, env=env, capture_output=True, timeout=60)
        assert completed.returncode == 0, completed.stderr.decode(errors="replace")
        return completed

    def install(source, root, legacy=False):
        command = ([sys.executable, str(source / "scripts/install-launcher.py")] if legacy else [str(source / "bin/flightdeck"), "install"])
        run(command + ["--source", str(source), "--data-dir", str(root / "installed"),
            "--bin-dir", str(root / "bin"), "--applications-dir", str(root / "applications"),
            "--no-desktop", "--no-launch", "--language", "en"], no_python=not legacy)

    def start(root, legacy=False):
        state = root / "state"
        states.append(state)
        command = ([sys.executable] if legacy else []) + [str(root / "bin/flightdeck"), "--desktop-service", "--state-dir", str(state)]
        with (root / "service.log").open("wb") as log:
            child = subprocess.Popen(command, cwd=output, env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        processes.append(child)
        return wait_for(lambda: record_at(state))

    def changed(state, previous, expected=None):
        def check():
            record = record_at(state)
            if record["token"] == previous["token"]:
                return None
            assert record["port"] == previous["port"], "Handoff must preserve the local service endpoint"
            assert record["pid"] != previous["pid"]
            if expected:
                assert record["release"] == expected
            return record
        return wait_for(check)

    def rollback(record, state):
        request(record, "/api/launcher-update/rollback", {})
        def finished():
            status = request(record, "/api/launcher-update")
            assert status["job"]["state"] != "failed", status["job"]
            return status if status["job"]["state"] == "complete" else None
        status = wait_for(finished)
        assert status["pending_restart"] and status["can_restart"]
        request(record, "/api/launcher-update/restart", {})
        return changed(state, record)

    try:
        if args.previous_native:
            previous = args.previous_native.resolve(strict=True)
            previous_digest = hashlib.sha256((previous / "bin/flightdeck").read_bytes()).hexdigest()
            assert digest != previous_digest, "Use different builds to exercise the actual handoff"
            root = output / "native"
            root.mkdir()
            install(previous, root)
            before = start(root)
            assert before["release"] == previous_digest
            install(package, root)
            status = request(before, "/api/launcher-update")
            assert status["pending_restart"] and status["can_restart"]
            request(before, "/api/launcher-update/restart", {})
            current = changed(root / "state", before, digest)
            request(current, "/api/preferences", {"language": "en"})
            assert json.loads((root / "state/ui-preferences.json").read_text())["language"] == "en"
            request(current, "/api/preferences", {"language": "de"})
            assert json.loads((root / "state/ui-preferences.json").read_text())["language"] == "de"
            assert not browser_log.exists(), "The native UI must not launch Chromium"
            results["native_language_preferences"] = "PASS"
            reverted = rollback(current, root / "state")
            assert reverted["release"] == previous_digest
            assert request(reverted, "/api/launcher-update")["can_rollback"]
            results["native_update_and_rollback"] = "PASS"
        if args.python_package:
            root = output / "legacy"
            root.mkdir()
            install(args.python_package.resolve(strict=True), root, legacy=True)
            before = start(root, legacy=True)
            legacy_digest = before["release"]
            install(package, root)
            run([str(root / "bin/flightdeck"), "desktop-handoff", "--state-dir", str(root / "state"), "--port", str(before["port"])])
            current = changed(root / "state", before, digest)
            assert request(current, "/api/launcher-update")["can_rollback"]
            reverted = rollback(current, root / "state")
            assert reverted["release"] == legacy_digest
            results["python_upgrade_and_rollback"] = "PASS"
        assert results, "Supply a previous native or Python package"
    finally:
        for state in states:
            try:
                record = wait_for(lambda: record_at(state), timeout=3)
                request(record, "/api/desktop/refresh", {})
            except (OSError, ValueError, AssertionError, http.client.HTTPException):
                pass
        for child in processes:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
            child.wait(timeout=30)
    report = {"status": "PASS", "native_binary_sha256": digest, "checks": results,
              "native_install_without_python": True, "same_service_endpoint": True, "native_window": "covered separately; this test is headless",
              "game_or_account_calls": False}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--previous-native", type=Path)
    parser.add_argument("--python-package", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    main(parser.parse_args())
