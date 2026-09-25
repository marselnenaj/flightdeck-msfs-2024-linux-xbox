# SPDX-License-Identifier: MIT
"""Explicit GitHub launcher updates using the existing atomic release installer."""
from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import Request, build_opener, HTTPRedirectHandler
import uuid

from . import __version__
from .backend import LauncherError, utc_now

REPOSITORY = "marselnenaj/flightdeck-msfs-2024-linux-xbox"
PROJECT = "https://github.com/" + REPOSITORY
API = "https://api.github.com/repos/" + REPOSITORY + "/releases/latest"
ASSET = "Flightdeck-Linux-x86_64.tar.gz"
MAX_ARCHIVE = 256 * 1024 * 1024
MAX_EXPANDED = 384 * 1024 * 1024
MAX_MEMBER = 128 * 1024 * 1024
HASH = re.compile(r"[0-9a-f]{64}\Z")
VERSION = re.compile(r"v?(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\Z")


class UpdateError(LauncherError):
    pass


class Cancelled(Exception):
    pass


def version(value):
    match = VERSION.fullmatch(value) if isinstance(value, str) else None
    if not match:
        raise UpdateError("GitHub meldet eine ungültige Flightdeck-Version.")
    return tuple(map(int, match.groups()))


def safe_url(url):
    try:
        parsed = urlsplit(url)
        return (parsed.scheme == "https" and parsed.hostname in
                {"api.github.com", "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"}
                and parsed.port in {None, 443} and not parsed.username and not parsed.password)
    except (TypeError, ValueError):
        return False


class GitHubRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        if not safe_url(newurl):
            raise UpdateError("Der Update-Download wurde an eine unbekannte Adresse umgeleitet.")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def open_url(url):
    if not safe_url(url):
        raise UpdateError("Ungültige GitHub-Downloadadresse.")
    return build_opener(GitHubRedirects()).open(Request(url, headers={
        "User-Agent": "Flightdeck/" + __version__, "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28"}), timeout=20)


def release_metadata(raw):
    if not isinstance(raw, dict) or raw.get("draft") is not False or raw.get("prerelease") is not False:
        raise UpdateError("GitHub meldet keine veröffentlichte Flightdeck-Version.")
    tag = raw.get("tag_name")
    version(tag)
    assets = raw.get("assets")
    matches = [item for item in assets if isinstance(item, dict) and item.get("name") == ASSET] if isinstance(assets, list) else []
    if len(matches) != 1:
        raise UpdateError("Das vollständige Linux-Updatepaket fehlt in diesem Release.")
    asset = matches[0]
    checksum = asset.get("digest", "")
    size = asset.get("size")
    url = PROJECT + "/releases/download/" + tag + "/" + ASSET
    if (asset.get("browser_download_url") != url or asset.get("state") != "uploaded"
            or type(size) is not int or not 0 < size <= MAX_ARCHIVE
            or not isinstance(checksum, str) or not checksum.startswith("sha256:") or not HASH.fullmatch(checksum[7:])):
        raise UpdateError("Das GitHub-Updatepaket hat keine gültigen Prüfdaten.")
    return {"version": tag.removeprefix("v"), "tag": tag, "url": url,
            "sha256": checksum[7:], "size": size, "release_url": PROJECT + "/releases/tag/" + tag,
            "notes": str(raw.get("body") or "")[:12000]}


def latest_release():
    with open_url(API) as response:
        data = response.read(2 * 1024 * 1024 + 1)
    if len(data) > 2 * 1024 * 1024:
        raise UpdateError("Die GitHub-Antwort ist zu groß.")
    try:
        return release_metadata(json.loads(data))
    except (ValueError, TypeError) as error:
        raise UpdateError("Die GitHub-Antwort konnte nicht gelesen werden.") from error


def download(release, target, cancel, progress):
    checksum = hashlib.sha256()
    received = 0
    with open_url(release["url"]) as response, target.open("xb") as output:
        while True:
            if cancel.is_set():
                raise Cancelled()
            chunk = response.read(256 * 1024)
            if not chunk:
                break
            received += len(chunk)
            if received > release["size"]:
                raise UpdateError("Die Größe des Updatepakets stimmt nicht mit GitHub überein.")
            checksum.update(chunk)
            output.write(chunk)
            progress(received, release["size"])
    if received != release["size"] or checksum.hexdigest() != release["sha256"]:
        raise UpdateError("Die Prüfsumme des Updates stimmt nicht. Bitte erneut herunterladen.")


def extract(archive, destination, expected_version, cancel):
    """No extractall, links, special files, overwritten members or unbounded payloads."""
    seen, total = set(), 0
    with tarfile.open(archive, "r:gz") as source:
        for item in source:
            if cancel.is_set():
                raise Cancelled()
            name = item.name.rstrip("/") if item.isdir() else item.name
            parts = name.split("/")
            if (not name or len(name) > 1024 or parts[0] != "flightdeck-linux"
                    or any(part in {"", ".", ".."} for part in parts)
                    or "\\" in name or ":" in name or any(ord(c) < 32 or ord(c) == 127 for c in name)
                    or name in seen or len(seen) >= 4096 or not (item.isfile() or item.isdir())
                    or item.size < 0 or item.size > MAX_MEMBER):
                raise UpdateError("Das Updatepaket enthält ungültige Dateien.")
            seen.add(name)
            total += item.size
            if total > MAX_EXPANDED:
                raise UpdateError("Das entpackte Updatepaket ist zu groß.")
            target = destination.joinpath(*parts)
            if item.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            stream = source.extractfile(item)
            if stream is None:
                raise UpdateError("Das Updatepaket ist unvollständig.")
            with stream, target.open("xb") as output:
                remaining = item.size
                while remaining:
                    if cancel.is_set():
                        raise Cancelled()
                    data = stream.read(min(256 * 1024, remaining))
                    if not data:
                        raise UpdateError("Das Updatepaket ist unvollständig.")
                    output.write(data)
                    remaining -= len(data)
    root = destination / "flightdeck-linux"
    init = root / "flightdeck/__init__.py"
    if not init.is_file() or init.stat().st_size > 65536:
        raise UpdateError("Die Versionsangabe im Updatepaket fehlt.")
    match = re.search(r'^__version__\s*=\s*[\'"]([^\'"]+)[\'"]\s*$', init.read_text(), re.M)
    if not match or match[1] != expected_version:
        raise UpdateError("Die Version im Updatepaket stimmt nicht mit GitHub überein.")
    if not (root / "scripts/install-launcher.py").is_file():
        raise UpdateError("Das Updatepaket ist unvollständig.")
    return root


def installation_context():
    source = Path(__file__).resolve().parents[1]
    if source.parent.name != "releases" or not HASH.fullmatch(source.name):
        return None
    spec = importlib.util.spec_from_file_location("flightdeck_installed_manager", source / "scripts/install-launcher.py")
    manager = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(manager)
    root = source.parent.parent
    state = manager.load_installation(root)
    if not state or source.name not in {state["current"], state.get("previous")}:
        raise UpdateError("Die installierte Flightdeck-Version konnte nicht zugeordnet werden.")
    return manager, root, source.name


class LauncherUpdateManager:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.thread = None
        self.restart_process = None
        self.cancel_event = threading.Event()
        self.release = None
        self.check_id = None
        self.checked_at = None
        self.job = None
        self.context = None
        self.context_error = ""
        self.last_check_attempt = None
        try:
            self.context = installation_context()
        except Exception:
            self.context_error = "Die installierte Flightdeck-Version konnte nicht zugeordnet werden."

    def snapshot(self):
        with self.launcher.lock, self.lock:
            self.launcher._poll()
            idle = (not self.launcher.setup_busy and self.launcher.process is None
                    and not self.launcher._external(create_lock=False) and not self.launcher.desktop_closing)
            active = bool(self.thread and self.thread.is_alive())
            state = None
            if self.context:
                try:
                    state = self.context[0].load_installation(self.context[1])
                except Exception:
                    self.context_error = "Die installierte Flightdeck-Version konnte nicht zugeordnet werden."
            pending = bool(state and state["current"] != self.context[2])
            restarting = bool(self.restart_process and self.restart_process.poll() is None)
            if self.restart_process and not restarting and pending:
                self.restart_process = None
                self.job = {"id": uuid.uuid4().hex, "operation": "restart", "state": "failed", "phase": "restart",
                            "can_cancel": False, "message": "", "error": "Flightdeck konnte nicht neu geöffnet werden. Bitte beende Spiel und Einrichtung und versuche es erneut."}
            newer = bool(self.release and version(self.release["version"]) > version(__version__))
            return {"installed_version": __version__, "latest_version": self.release["version"] if self.release else None,
                    "update_available": newer if self.release else None, "checked_at": self.checked_at,
                    "check_id": self.check_id, "release_url": self.release["release_url"] if self.release else PROJECT + "/releases",
                    "notes": self.release["notes"] if self.release else "", "download_size": self.release["size"] if self.release else None,
                    "managed": bool(state), "pending_restart": pending, "can_check": not active and not pending,
                    "can_install": bool(state and newer and idle and not active and not pending),
                    "can_rollback": bool(state and state.get("previous") and idle and not active and not pending),
                    "can_restart": bool(pending and idle and not active and not restarting), "busy": not idle,
                    "unavailable_reason": self.context_error or ("Für Updates im Launcher Flightdeck einmal mit dem offiziellen Installer installieren." if not state else ""),
                    "job": copy.deepcopy(self.job)}

    def _change(self, **values):
        with self.lock:
            self.job.update(values)

    def check_on_startup(self):
        """Opening another window must not repeat the public GitHub request."""
        with self.launcher.lock, self.lock:
            if (self.last_check_attempt is not None and time.monotonic() - self.last_check_attempt < 1800
                    or not self.snapshot()["can_check"]):
                return
            self.start("check")

    def start(self, operation, check_id=None):
        with self.launcher.lock, self.lock:
            allowed = self.snapshot()
            permission = {"check": "can_check", "install": "can_install", "rollback": "can_rollback"}.get(operation)
            if not permission or not allowed[permission]:
                raise UpdateError("Dieser Launcher-Updatevorgang ist gerade nicht verfügbar.")
            if operation == "install" and (not check_id or check_id != self.check_id):
                raise UpdateError("Bitte die Flightdeck-Updates zuerst erneut prüfen.")
            if operation != "check":
                self.launcher._require_cloud_idle()
                self.launcher.reserve_setup()
            self.cancel_event.clear()
            if operation == "check":
                self.release = self.check_id = None
                self.last_check_attempt = time.monotonic()
            self.job = {"id": uuid.uuid4().hex, "operation": operation, "state": "running", "phase": "checking" if operation == "check" else "preparing",
                        "message": "GitHub wird nach Flightdeck-Updates gefragt …" if operation == "check" else "Launcher-Update wird vorbereitet …",
                        "progress": None, "received": 0, "total": None, "error": "", "can_cancel": operation != "rollback"}
            self.thread = threading.Thread(target=self._run, args=(operation, copy.deepcopy(self.release)), daemon=True)
            self.thread.start()
            return {"ok": True, "job": copy.deepcopy(self.job)}

    def _run(self, operation, release):
        try:
            if operation == "check":
                result = latest_release()
                if self.cancel_event.is_set():
                    raise Cancelled()
                with self.lock:
                    self.release, self.check_id, self.checked_at = result, uuid.uuid4().hex, utc_now()
                message = "Eine neue Flightdeck-Version ist verfügbar." if version(result["version"]) > version(__version__) else "Flightdeck ist auf dem neuesten Stand."
            else:
                manager, root, running = self.context
                previous = manager.load_installation(root)
                if previous["current"] != running:
                    raise UpdateError("Flightdeck wurde inzwischen geändert. Bitte den Launcher neu öffnen.")
                if operation == "rollback":
                    self._change(phase="installing", can_cancel=False, message="Vorherige Launcher-Version wird wiederhergestellt …")
                    manager.rollback(root, language=previous.get("language", "en"), expected_current=running)
                else:
                    with tempfile.TemporaryDirectory(prefix="flightdeck-update-") as temporary:
                        staging = Path(temporary)
                        archive = staging / ASSET
                        self._change(phase="downloading", message="Flightdeck wird von GitHub heruntergeladen …")
                        download(release, archive, self.cancel_event,
                                 lambda received, total: self._change(received=received, total=total, progress=round(received / total * 100)))
                        self._change(phase="verifying", message="Updatepaket wird geprüft …", progress=None)
                        source = extract(archive, staging / "unpacked", release["version"], self.cancel_event)
                        with self.lock:
                            if self.cancel_event.is_set():
                                raise Cancelled()
                            self.job.update(phase="installing", can_cancel=False, message="Flightdeck wird aktualisiert …")
                        # Execute only the checksum-verified release's installer, which
                        # understands its own new package format. No shell or caller URL.
                        completed = subprocess.run([sys.executable, "-B", str(source / "scripts/install-launcher.py"),
                            "--source", str(source), "--data-dir", str(root), "--no-desktop", "--no-launch",
                            "--language", previous.get("language", "en"), "--expected-current", running],
                            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=180)
                        if completed.returncode:
                            raise UpdateError("Das Launcher-Update konnte nicht installiert werden. Die bisherige Version bleibt verfügbar.")
                message = "Update installiert. Öffne Flightdeck jetzt neu." if operation == "install" else "Vorherige Version bereit. Öffne Flightdeck jetzt neu."
            self._change(state="complete", phase="complete", message=message, progress=100, can_cancel=False)
        except Cancelled:
            self._change(state="cancelled", message="Launcher-Update abgebrochen.", can_cancel=False)
        except Exception as error:
            if isinstance(error, HTTPError):
                message = "GitHub ist gerade nicht verfügbar oder das Abfragelimit ist erreicht. Bitte später erneut versuchen."
            elif isinstance(error, (URLError, TimeoutError)):
                message = "GitHub ist nicht erreichbar. Prüfe deine Internetverbindung und versuche es erneut."
            elif isinstance(error, UpdateError):
                message = str(error)
            else:
                message = "Das Launcher-Update ist fehlgeschlagen. Die bisherige Version bleibt verfügbar."
            self._change(state="failed", error=message, message=message, can_cancel=False)
        finally:
            if operation != "check":
                self.launcher.release_setup()

    def cancel(self, job_id):
        with self.lock:
            if not self.job or self.job["id"] != job_id or not self.job.get("can_cancel") or self.job["state"] != "running":
                raise UpdateError("Dieser Launcher-Updatevorgang kann nicht mehr abgebrochen werden.")
            self.cancel_event.set()
            self.job.update(can_cancel=False, message="Launcher-Update wird abgebrochen …")
        return {"ok": True}

    def restart(self, language):
        with self.launcher.lock, self.lock:
            if not self.snapshot()["can_restart"]:
                raise UpdateError("Bitte beende Spiel und Einrichtung vor dem Launcher-Neustart.")
            self.launcher._require_cloud_idle()
            manager, root, _ = self.context
            state = manager.load_installation(root)
            entry = state["entries"]["launcher"]
            executable = Path(entry["path"])
            if hashlib.sha256(manager.read_regular(executable)).hexdigest() != entry["sha256"]:
                raise UpdateError("Der installierte Launcher wurde verändert. Bitte den Installer erneut ausführen.")
            self.restart_process = subprocess.Popen([sys.executable, "-B", str(executable), "--desktop", "--state-dir", str(self.launcher.state_dir),
                                                     "--language", language], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                                     stderr=subprocess.DEVNULL, start_new_session=True)
            self.job = {"id": uuid.uuid4().hex, "operation": "restart", "state": "running", "phase": "restart",
                        "can_cancel": False, "message": "Flightdeck wird neu geöffnet …", "error": ""}
        return {"ok": True, "job": copy.deepcopy(self.job)}

    def close(self):
        self.cancel_event.set()
        if self.thread:
            self.thread.join(timeout=25)
