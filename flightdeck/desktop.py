# SPDX-License-Identifier: MIT
"""Own and reuse one verified local UI service per private state directory.

A PID alone is never an identity. Reuse requires its Linux start time, a held
service lock and an HTTP response matching a private per-service random token.
Only a child created by this invocation can be terminated after failed startup.
"""
from __future__ import annotations

from contextlib import contextmanager
import fcntl
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile
import threading
import time
import webbrowser

APP = "flightdeck-desktop-service"
TOKEN = re.compile(r"[A-Za-z0-9_-]{32,128}\Z")
HASH = re.compile(r"[0-9a-f]{64}\Z")
RECORD = "desktop-service.json"
START_LOCK = "desktop-start.lock"
SERVICE_LOCK = "desktop-service.lock"
MAX_RECORD = 4096
COPY = {
    "en": {
        "path": "The launcher state folder must be a real directory owned by your user.",
        "record": "The saved desktop service record is invalid. Check the launcher state folder.",
        "busy": "Another launcher is starting. Try again in a moment.",
        "unverified": "An existing background service could not be verified. It was left untouched.",
        "start": "The background launcher could not start. Check the state folder and try flightdeck --no-browser for details.",
        "browser": "No supported application browser or default browser could be opened.",
        "runtime": "The running launcher could not switch runtimes. Close the game and finish any active setup first.",
        "refresh": "The previous launcher is still closing. Try starting Flightdeck again in a moment.",
        "update": "A launcher update is ready. Finish the game or setup, then open Flightdeck again. The current session remains running.",
    },
    "de": {
        "path": "Der Einstellungsordner muss ein echter Ordner deines Benutzerkontos sein.",
        "record": "Der gespeicherte Hintergrunddienst-Eintrag ist ungültig. Bitte den Einstellungsordner prüfen.",
        "busy": "Ein anderer Launcher startet gerade. Bitte gleich erneut versuchen.",
        "unverified": "Ein vorhandener Hintergrunddienst konnte nicht verifiziert werden. Er wurde nicht verändert.",
        "start": "Der Launcher konnte nicht im Hintergrund starten. Bitte den Einstellungsordner prüfen; flightdeck --no-browser zeigt Details.",
        "browser": "Es konnte weder ein Anwendungsbrowser noch der Standardbrowser geöffnet werden.",
        "runtime": "Der laufende Launcher konnte die Runtime nicht wechseln. Bitte Spiel und laufende Einrichtung zuerst beenden.",
        "refresh": "Der bisherige Launcher wird noch beendet. Bitte Flightdeck gleich erneut öffnen.",
        "update": "Ein Launcher-Update ist bereit. Bitte Spiel oder Einrichtung abschließen und Flightdeck erneut öffnen. Die aktuelle Sitzung läuft weiter.",
    },
}


class DesktopError(Exception):
    def __init__(self, key):
        super().__init__(key)
        self.key = key

    def localized(self, language):
        return COPY[language][self.key]


def release_identity():
    """Fingerprint our installed code/resources, never runtime or account files.

    A service stores this once at startup. An installed release has immutable
    files; a later invocation computes the identity of its newly selected code.
    The version label alone cannot distinguish updates within the same version.
    """
    package = Path(__file__).resolve().parent
    root = package.parent
    paths = list(package.glob("*.py"))
    paths += list((package / "_fenix").glob("*.py"))
    ui = package / "ui" if (package / "ui").is_dir() else root / "ui"
    paths += [ui / name for name in ("index.html", "app.js", "setup.js", "mods.js", "fenix.js", "updates.js", "cloud-saves.js", "i18n.js", "state.js",
                                    "styles.css", "mark.svg", "flight-panorama.png", "flight-panorama-2020.png", "manrope-variable.woff2", "OFL-Manrope.txt")]
    resources = package / "resources"
    if resources.is_dir():
        paths += list((resources / "runtime").glob("*"))
        paths += [resources / "upstreams.lock.json", resources / "bootstrap.lock.json"]
        paths += [resources / "fenix" / name for name in ("bundle.json", "release.json", "LICENSE")]
    else:
        paths += list((root / "scripts/runtime").glob("*"))
        paths += [root / "compat/upstreams.lock.json", root / "compat/bootstrap.lock.json"]
        paths += [root / "compat/fenix" / name for name in ("bundle.json", "release.json", "LICENSE")]
    digest = hashlib.sha256()
    for path in sorted(paths):
        if path.is_file():
            data = path.read_bytes()
            digest.update(str(path.relative_to(root)).encode() + b"\0")
            digest.update(len(data).to_bytes(8, "big"))
            digest.update(data)
    return digest.hexdigest()


def state_directory(value):
    path = Path(os.path.abspath(Path(value).expanduser()))
    for component in (*reversed(path.parents), path):
        if component.is_symlink():
            raise DesktopError("path")
    path.mkdir(parents=True, mode=0o700, exist_ok=True)
    if not path.is_dir() or path.stat().st_uid != os.getuid():
        raise DesktopError("path")
    path.chmod(0o700)
    return path


def open_private(path, *, create=False):
    flags = os.O_RDWR | os.O_NONBLOCK | os.O_NOFOLLOW
    if create:
        flags |= os.O_CREAT
    fd = os.open(path, flags, 0o600)
    info = os.fstat(fd)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        os.close(fd)
        raise DesktopError("record")
    return fd


@contextmanager
def lock_file(path, *, wait=0):
    fd = open_private(path, create=True)
    deadline = time.monotonic() + wait
    try:
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise DesktopError("busy") from None
                time.sleep(0.05)
        yield fd
    finally:
        os.close(fd)


def service_locked(root):
    try:
        with lock_file(root / SERVICE_LOCK):
            return False
    except DesktopError as error:
        if error.key == "busy":
            return True
        raise


def process_start(pid):
    """Return Linux process birth time only for the current user's process."""
    if type(pid) is not int or pid <= 0:
        return None
    try:
        process = Path("/proc") / str(pid)
        if process.stat().st_uid != os.getuid():
            return None
        data = (process / "stat").read_text()
        fields = data[data.rfind(")") + 2:].split()
        if fields[0] in {"Z", "X"}:
            return None
        return int(fields[19])
    except (OSError, ValueError, IndexError):
        return None


def read_record(root):
    try:
        fd = open_private(root / RECORD)
    except FileNotFoundError:
        return None
    with os.fdopen(fd, "rb") as stream:
        data = stream.read(MAX_RECORD + 1)
    try:
        value = json.loads(data) if len(data) <= MAX_RECORD else None
        if (not isinstance(value, dict) or value.get("app") != APP or value.get("schema") != 1 or
                value.get("uid") != os.getuid() or type(value.get("pid")) is not int or value["pid"] <= 0 or
                type(value.get("start")) is not int or value["start"] < 0 or
                type(value.get("port")) is not int or not 1 <= value["port"] <= 65535 or
                not isinstance(value.get("token"), str) or not TOKEN.fullmatch(value["token"]) or
                ("release" in value and (not isinstance(value["release"], str) or not HASH.fullmatch(value["release"])))):
            raise ValueError()
        return value
    except (ValueError, UnicodeError):
        raise DesktopError("record") from None


def write_record(root, value):
    # A foreign/corrupt existing record is never silently overwritten.
    read_record(root)
    fd, temporary = tempfile.mkstemp(prefix=".desktop-", dir=root)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            os.fchmod(stream.fileno(), 0o600)
            json.dump(value, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, root / RECORD)
    finally:
        Path(temporary).unlink(missing_ok=True)


def request(record, path="/api/status", payload=None, *, timeout=1):
    """Connect directly to numeric loopback; ignore proxy environment settings."""
    connection = http.client.HTTPConnection("127.0.0.1", record["port"], timeout=timeout)
    try:
        headers = {}
        data = None
        if payload is not None:
            data = json.dumps(payload).encode()
            headers = {"Content-Type": "application/json", "X-Flightdeck-Token": record["token"],
                       "Origin": f'http://127.0.0.1:{record["port"]}'}
        connection.request("POST" if data is not None else "GET", path, body=data, headers=headers)
        response = connection.getresponse()
        body = response.read(65537)
        if response.status != 200 or len(body) > 65536:
            return None
        return json.loads(body)
    except (OSError, ValueError, http.client.HTTPException):
        return None
    finally:
        connection.close()


def verified_service(root, record=None):
    record = read_record(root) if record is None else record
    if not record or process_start(record["pid"]) != record["start"] or not service_locked(root):
        return None
    reply = request(record)
    if (not isinstance(reply, dict) or not isinstance(reply.get("app"), dict) or
            reply["app"].get("name") != "Flightdeck" or not isinstance(reply.get("csrf_token"), str) or
            not secrets.compare_digest(reply["csrf_token"], record["token"]) or
            ("release" in record and (not isinstance(reply.get("service"), dict) or
                                     reply["service"].get("release") != record["release"])) or
            process_start(record["pid"]) != record["start"]):
        return None
    return record


def ensure_service(state_dir, runtime=None, port=0, *, timeout=60):
    # A new release may verify and replace the native runtime before its
    # loopback service becomes ready. Give that bounded local operation time.
    root = state_directory(state_dir)
    identity = release_identity()
    with lock_file(root / START_LOCK, wait=timeout):
        old = read_record(root)
        running = verified_service(root, old)
        if running and running.get("release") != identity:
            result = request(running, "/api/desktop/refresh", {})
            if result and result.get("ok") is True and result.get("refresh") == "restarting":
                deadline = time.monotonic() + timeout
                while service_locked(root):
                    if time.monotonic() >= deadline:
                        raise DesktopError("refresh")
                    time.sleep(0.05)
                running = None
            else:
                # Busy or older services without the refresh contract retain
                # their game/setup. Never signal a PID from an old record.
                running = {**running, "update_pending": True}
        if running:
            if runtime is not None:
                result = request(running, "/api/config", {"runtime_path": runtime}, timeout=5)
                if not result or result.get("ok") is not True:
                    raise DesktopError("runtime")
            return root, running
        if service_locked(root):
            raise DesktopError("unverified")
        # Reuse the prior loopback port when available, preserving browser local
        # preferences across a normal service restart. Fall back if it is taken.
        preferred = port or (old["port"] if old else 0)
        choices = [preferred, 0] if preferred and not port else [preferred]
        for chosen in choices:
            args = [sys.executable, "-B", "-m", "flightdeck", "--desktop-service", "--state-dir", str(root), "--port", str(chosen)]
            if runtime is not None:
                args += ["--runtime", runtime]
            child = subprocess.Popen(args, cwd=Path(__file__).resolve().parents[1],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                     start_new_session=True, close_fds=True)
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    break
                current = verified_service(root)
                if current and current.get("release") == identity:
                    threading.Thread(target=child.wait, daemon=True).start()
                    return root, current
                time.sleep(0.075)
            if child.poll() is None:
                # This Popen object owns a child from this invocation, never a
                # PID recovered from potentially stale on-disk metadata.
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)
            if service_locked(root):
                raise DesktopError("unverified")
        raise DesktopError("start")


def serve(state_dir, runtime=None, port=0):
    from .backend import Launcher
    from .server import Server
    root = state_directory(state_dir)
    with lock_file(root / SERVICE_LOCK):
        server = Server(Launcher(root, runtime), port)
        server.desktop_service = True
        record = {"app": APP, "schema": 1, "uid": os.getuid(), "pid": os.getpid(),
                  "start": process_start(os.getpid()), "port": server.server_port, "token": server.token,
                  "release": server.release_identity}
        try:
            write_record(root, record)
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()
            # Retain the private stale record to prefer the same browser origin
            # next time. Its dead PID/start time can never authorize reuse.


def open_interface(root, record, language=None):
    url = f'http://127.0.0.1:{record["port"]}' + ("/?lang=" + language if language else "")
    browser = next((path for name in ("chromium", "chromium-browser", "google-chrome-stable", "google-chrome", "brave-browser", "brave")
                    if (path := shutil.which(name))), None)
    if browser:
        profile = state_directory(root / "desktop-browser")
        try:
            subprocess.Popen([browser, "--app=" + url, "--class=Flightdeck", "--user-data-dir=" + str(profile),
                              "--no-first-run", "--no-default-browser-check"], stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, close_fds=True,
                             start_new_session=True)
            return url
        except OSError:
            pass
    if webbrowser.open(url, new=1):
        return url
    raise DesktopError("browser")


def start(state_dir, runtime=None, port=0, language=None):
    root, record = ensure_service(state_dir, runtime, port)
    if record.get("update_pending"):
        print(COPY[language or "en"]["update"], file=sys.stderr)
    return open_interface(root, record, language)
