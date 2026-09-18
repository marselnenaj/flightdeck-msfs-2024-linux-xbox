# SPDX-License-Identifier: MIT
"""Prepare free compatibility components and a fresh prefix before game login.

Only pinned archives and explicitly packaged files are executed. Downloads are
verified before extraction, shared caches are locked, and each installation owns
a separate private workspace. No Microsoft credentials or game data live here.
"""
from __future__ import annotations

import ctypes.util
from dataclasses import dataclass
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

from .i18n import message
from .setup import (ARTIFACTS, CONNECTED_STORAGE_FEATURE, artifact_names,
                    SetupError, SetupCancelled, data_home, digest,
                    interrupted, path_input, _copy_file, _publish)


@dataclass
class BootstrapPlan:
    inputs: dict
    destination: Path
    lock: dict
    source: Path
    native: Path | None


def paths(source_root=None):
    source = Path(source_root) if source_root else Path(__file__).resolve().parents[1]
    packaged = Path(__file__).resolve().parent / "resources"
    lock = source / "compat/bootstrap.lock.json"
    return source, lock if lock.is_file() else packaged / "bootstrap.lock.json"


def cached_native_path(lock):
    """One fixed, content-bound component directory; never search user runtimes."""
    checksum = hashlib.sha256(json.dumps(lock["native"]["files"], sort_keys=True,
                                        separators=(",", ":")).encode()).hexdigest()
    return data_home() / "flightdeck/components" / ("native-" + checksum)


def native_path(source, lock=None, *, verify=True):
    # Private developer builds are reusable only after matching the release pin.
    candidates = (Path(__file__).resolve().parent / "resources/native",
                      source / "flightdeck/resources/native", source / "build/compat/artifacts",
                      source / "build/compat-marketplace-run23/artifacts")
    required = artifact_names(lock["native"]) if lock is not None else ARTIFACTS
    if lock is not None:
        candidates += (cached_native_path(lock),)
    for candidate in candidates:
        if not candidate.is_symlink() and all((candidate / name).is_file() for name in required):
            if lock is not None and verify:
                try:
                    verify_native(candidate, lock)
                except (OSError, SetupError, KeyError, TypeError):
                    continue
            return candidate
    return None


def availability(source_root=None):
    source, lockfile = paths(source_root)
    try:
        lock = json.loads(lockfile.read_text())
        if platform.system() != "Linux" or platform.machine() not in ("x86_64", "amd64"):
            raise SetupError("Die automatische Installation unterstützt derzeit Linux auf x86-64.")
        if native_path(source, lock) is None and not lock["native"].get("archive_sha256"):
            raise SetupError("Bitte das vollständige Flightdeck-Linux-Paket installieren. In diesem Quellpaket fehlen die geprüften Laufzeitkomponenten.")
        if not hasattr(tarfile, "data_filter"):
            raise SetupError("Für sichere Downloads wird Python 3.10.12 oder neuer benötigt.")
        return {"available": True, "reason": ""}
    except (OSError, ValueError, KeyError, TypeError) as error:
        return {"available": False, "reason": error.args[0] if isinstance(error, SetupError) else
                "Die Beschreibung der Installationskomponenten fehlt. Flightdeck bitte erneut installieren."}


def preflight(data, *, source_root=None, notify=None, cancel=None):
    from .setup import regular
    interrupted(cancel)
    capability = availability(source_root)
    if not capability["available"]:
        raise SetupError(capability["reason"])
    source, lockfile = paths(source_root)
    lock = json.loads(lockfile.read_text())
    destination = path_input(data.get("destination_path") or str(data_home() / "flightdeck/runtimes/msfs2024"), "Zielordner", exists=False)
    market = data.get("market", "US")
    if not isinstance(market, str) or not re.fullmatch("[A-Z]{2}", market):
        raise SetupError("Bitte einen Ländercode mit zwei Großbuchstaben wählen, zum Beispiel AT.")
    if not isinstance(data.get("local_saves", False), bool):
        raise SetupError("Die Auswahl für lokale Spielstände muss Ja oder Nein sein.")
    parent = destination.parent
    while not parent.exists():
        parent = parent.parent
    if not parent.is_dir() or not os.access(parent, os.W_OK | os.X_OK):
        raise SetupError("Der Zielordner kann hier nicht angelegt werden. Bitte Schreibrechte oder Speicherort ändern.")
    if shutil.disk_usage(parent).free < 100 * 1024 ** 3:
        raise SetupError("Für die Erstinstallation werden mindestens 100 GiB freier Speicherplatz benötigt. Bitte einen anderen Speicherort wählen.")
    family, version = platform.libc_ver()
    minimum = tuple(map(int, lock["native"]["minimum_glibc"].split(".")))
    if family != "glibc" or tuple(map(int, version.split("."))) < minimum:
        raise SetupError(message("Dieses Laufzeitpaket benötigt glibc {version} oder neuer. Bitte ein passendes Linux-System verwenden.", version=lock["native"]["minimum_glibc"]))
    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")) or not os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        raise SetupError("Flightdeck bitte aus der grafischen Linux-Sitzung starten, damit die Microsoft-Anmeldung und der Schlüsselbund verfügbar sind.")
    needed = ("webkit2gtk-4.1", "gtk-3", "ssl", "vulkan")
    missing = [name for name in needed if not ctypes.util.find_library(name)]
    if missing:
        raise SetupError(message("Für die Anmeldung oder Grafik fehlen Linux-Bibliotheken: {names}. Bitte über die Softwareverwaltung installieren.", names=", ".join(missing)))
    check_media(cancel=cancel)
    native = native_path(source, lock)
    if native:
        verify_native(native, lock, cancel)
        # Fixed, verified --help does not read an account or open a window.
        try:
            status = subprocess.run([str(native / "bin/xodus-cli"), "--help"],
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, timeout=15).returncode
        except (OSError, subprocess.TimeoutExpired):
            status = 1
        if status:
            raise SetupError("Xodus kann auf diesem Linux nicht starten. Bitte die GTK-, WebKitGTK- und OpenSSL-Laufzeitbibliotheken prüfen.")
    if notify:
        for key, label in (("platform", "Linux und Anmeldefenster"), ("space", "Mindestens 100 GiB freier Speicher"),
                           ("components", "Geprüfte Installationskomponenten"), ("media", "Videowiedergabe (MP4/H.264)")):
            notify("bootstrap", label, None, {"id": key, "label": label, "ok": True, "detail": "Geprüft"})
    inputs = {"mode": "install", "market": market, "local_saves": data.get("local_saves", False),
              "destination_path": str(destination)}
    return BootstrapPlan(inputs, destination, lock, source, native)


def check_media(*, cancel=None):
    """Check the known startup-video dependencies, not every possible codec.

    Fresh installs rely on host plugins. Do not let an experimental custom
    plugin path mask a missing host dependency; existing prepared runtimes can
    keep their separately selected media modules. The probe uses no game data
    and writes its scanner cache only to its own temporary directory.
    """
    interrupted(cancel)
    command = shutil.which("gst-inspect-1.0")
    if not command:
        raise SetupError("Für die Videoprüfung fehlt gst-inspect-1.0. Bitte die GStreamer-Werkzeuge über die Softwareverwaltung installieren.")
    missing = []
    env = {key: value for key, value in os.environ.items() if not key.startswith(("GST_PLUGIN_", "GST_REGISTRY"))}
    with tempfile.TemporaryDirectory(prefix="flightdeck-media-check-") as temporary:
        env.update(GST_PLUGIN_PATH="", GST_PLUGIN_PATH_1_0="", GST_REGISTRY_1_0=str(Path(temporary) / "registry.bin"), GST_DEBUG="0")
        for plugin in ("qtdemux", "h264parse", "avdec_h264"):
            interrupted(cancel)
            try:
                result = subprocess.run([command, plugin], env=env, stdin=subprocess.DEVNULL,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
                if result.returncode:
                    missing.append(plugin)
            except (OSError, subprocess.TimeoutExpired):
                missing.append(plugin)
        interrupted(cancel)
    if missing:
        raise SetupError(message("Für die Videowiedergabe fehlen GStreamer-Module: {names}. Bitte GStreamer Good, Bad und Libav über die Softwareverwaltung installieren.", names=", ".join(missing)))


def verify_native(root, lock, cancel=None):
    for name in artifact_names(lock["native"]):
        path = root / name
        if path.is_symlink() or not path.is_file() or digest(path, cancel) != lock["native"]["files"].get(name):
            raise SetupError("Die Prüfsumme der Laufzeitkomponenten stimmt nicht. Flightdeck bitte erneut installieren.")
        if name.startswith("bin/") and not os.access(path, os.X_OK):
            raise SetupError("Die Laufzeitkomponenten sind nicht ausführbar. Flightdeck bitte erneut installieren.")


def cache_native(root, lock, cancel=None):
    """Called with the components lock held; publish only a complete verified copy."""
    target = cached_native_path(lock)
    if target.is_symlink():
        raise SetupError("Der Komponentenordner muss ein privater Ordner des aktuellen Benutzers sein.")
    if target.exists():
        verify_native(target, lock, cancel)
        return target
    temporary = Path(tempfile.mkdtemp(prefix=".native-", dir=target.parent))
    try:
        for name in artifact_names(lock["native"]):
            interrupted(cancel)
            _copy_file(root / name, temporary / name)
        verify_native(temporary, lock, cancel)
        interrupted(cancel)
        _publish(temporary, target)
    finally:
        shutil.rmtree(temporary, ignore_errors=True)
    return target


class HTTPSOnlyRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, newurl):
        if not newurl.lower().startswith("https://"):
            raise SetupError("Der Komponentendownload wurde auf eine unsichere Adresse umgeleitet.")
        return super().redirect_request(request, response, code, message, headers, newurl)


_MAX_COMPONENT_BYTES = 2 * 1024 ** 3


def _content_length(response):
    """Use only one bounded decimal HTTP length; otherwise stay indeterminate."""
    headers = getattr(response, "headers", None)
    if headers is None or headers.get("Transfer-Encoding"):
        return None
    if hasattr(headers, "get_all"):
        values = headers.get_all("Content-Length", [])
    else:
        value = headers.get("Content-Length")
        values = [] if value is None else [value]
    if (len(values) != 1 or not isinstance(values[0], str)
            or not re.fullmatch(r"[0-9]{1,10}", values[0])):
        return None
    length = int(values[0])
    return length if 0 < length <= _MAX_COMPONENT_BYTES else None


def _component_transfer(received, total, *, verified=0):
    return {"kind": "components", "received_bytes": received, "verified_bytes": verified, "total_bytes": total,
            "completed_files": None, "total_files": None}


def download(url, expected, target, *, cancel=None, progress=None):
    interrupted(cancel)
    if target.is_file() and not target.is_symlink() and digest(target, cancel) == expected:
        if progress:
            size = target.stat().st_size
            progress(_component_transfer(size, size, verified=size))
        return target
    if not isinstance(expected, str) or not re.fullmatch("[a-f0-9]{64}", expected) or not url.startswith("https://"):
        raise SetupError("Für den Komponentendownload fehlt eine gültige Prüfsumme.")
    fd, temporary = tempfile.mkstemp(prefix=".download-", dir=target.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            request = urllib.request.Request(url, headers={"User-Agent": "Flightdeck/0.1.0"})
            # No cookies, credentials, shell or user-supplied URL.
            with urllib.request.build_opener(HTTPSOnlyRedirect()).open(request, timeout=30) as response:
                if not response.url.startswith("https://"):
                    raise SetupError("Der Komponentendownload wurde auf eine unsichere Adresse umgeleitet.")
                count, total = 0, _content_length(response)
                if progress:
                    progress(_component_transfer(count, total))
                last_report = time.monotonic()
                while block := response.read(1024 * 1024):
                    interrupted(cancel)
                    count += len(block)
                    if count > _MAX_COMPONENT_BYTES:
                        raise SetupError("Der Komponentendownload ist unerwartet groß.")
                    output.write(block)
                    if total is not None and count > total:
                        total = None
                    now = time.monotonic()
                    # Withhold a known 100% until the archive has passed its
                    # pinned hash check. Progress never authorizes extraction.
                    if progress and count != total and now - last_report >= .5:
                        progress(_component_transfer(count, total))
                        last_report = now
        if digest(Path(temporary), cancel) != expected:
            raise SetupError("Die Prüfsumme des Downloads stimmt nicht. Die Datei wurde nicht ausgeführt.")
        os.replace(temporary, target)
        if progress:
            progress(_component_transfer(count, total if count == total else None, verified=count))
        return target
    except (urllib.error.URLError, TimeoutError):
        raise SetupError("Die Laufzeitkomponenten konnten nicht heruntergeladen werden. Verbindung prüfen oder das vollständige Flightdeck-Paket verwenden.") from None
    finally:
        Path(temporary).unlink(missing_ok=True)


def extract(archive_path, destination, cancel=None):
    with tarfile.open(archive_path) as archive:
        members = archive.getmembers()
        if len(members) > 30000 or sum(m.size for m in members) > 6 * 1024 ** 3:
            raise SetupError("Das Komponentenarchiv überschreitet die erwartete Größe.")
        for member in members:
            interrupted(cancel)
            if member.isdev() or member.isfifo():
                raise SetupError("Das Komponentenarchiv enthält unzulässige Dateien.")
            archive.extract(member, destination, filter="data")


def _command(arguments, *, env, cancel, timeout=300):
    from .game_install import _stop_owned
    interrupted(cancel)
    process = subprocess.Popen([str(value) for value in arguments], env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               start_new_session=True, umask=0o077)
    start = time.monotonic()
    try:
        while process.poll() is None:
            interrupted(cancel)
            if time.monotonic() - start > timeout:
                raise SetupError("Die Linux-Spielumgebung konnte nicht rechtzeitig eingerichtet werden. Bitte erneut versuchen.")
            time.sleep(.1)
        if process.returncode:
            raise SetupError("Die Linux-Spielumgebung konnte nicht eingerichtet werden. Bitte die Grafiktreiber und Wine-Abhängigkeiten prüfen.")
    finally:
        _stop_owned(process)


def prepare_prefix(runner, prefix, *, cancel=None):
    if prefix.exists() or prefix.is_symlink():
        raise SetupError("Die neue Spielumgebung existiert bereits und wird nicht überschrieben.")
    env = dict(os.environ, WINEPREFIX=str(prefix), WINEARCH="win64", WINEESYNC="0", WINEFSYNC="0", WINEDEBUG="-all")
    env.pop("WINEDLLPATH", None)
    env.pop("WINEDLLOVERRIDES", None)
    wine = runner / "files/bin/wine"
    try:
        _command([wine, "wineboot", "-u"], env=env, cancel=cancel)
        _command([runner / "files/bin/wineserver", "-w"], env=env, cancel=cancel)
        from .setup import prefix_system32
        prefix_system32(prefix)
        for architecture, target in (("x86_64-windows", "system32"), ("i386-windows", "syswow64")):
            folder = prefix / "drive_c/windows" / target
            if folder.is_symlink() or not folder.is_dir():
                raise SetupError("Die neue Spielumgebung enthält ungültige Systemordner.")
            for library, names in (("dxvk", ("dxgi", "d3d11", "d3d10core")), ("vkd3d-proton", ("d3d12", "d3d12core"))):
                for name in names:
                    _copy_file(runner / "files/lib/wine" / library / architecture / (name + ".dll"), folder / (name + ".dll"))
        registry = prefix.parent / "graphics.reg"
        registry.write_text('Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\DllOverrides]\n' +
                            ''.join('"' + name + '"="native"\n' for name in ("dxgi", "d3d11", "d3d10core", "d3d12", "d3d12core")))
        _command([wine, "regedit", "/S", registry], env=env, cancel=cancel)
        _command([runner / "files/bin/wineserver", "-w"], env=env, cancel=cancel)
    finally:
        # This environment names only this job's new prefix, never a live game.
        subprocess.run([str(runner / "files/bin/wineserver"), "-k"], env=env,
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
    for name in ("system.reg", "user.reg"):
        if not (prefix / name).is_file():
            raise SetupError("Die neue Spielumgebung wurde nicht vollständig eingerichtet.")


def bootstrap(plan, *, notify=None, cancel=None, transfer=None):
    def event(phase, text):
        interrupted(cancel)
        if notify:
            notify(phase, text, None, None)
    def fetch(url, expected, target):
        try:
            return download(url, expected, target, cancel=cancel, progress=transfer)
        finally:
            if transfer:
                transfer(None)
    if transfer:
        transfer(None)
    plan.destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    workspace = Path(tempfile.mkdtemp(prefix=".flightdeck-install-", dir=plan.destination.parent))
    cache = data_home() / "flightdeck/components"
    cache.mkdir(parents=True, exist_ok=True, mode=0o700)
    if cache.is_symlink() or cache.stat().st_uid != os.getuid() or cache.stat().st_mode & 0o077:
        raise SetupError("Der Komponentenordner muss ein privater Ordner des aktuellen Benutzers sein.")
    fd = os.open(cache / ".lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise SetupError("Die Komponentensperre ist ungültig.")
        event("bootstrap", "Linux-Komponenten werden vorbereitet …")
        while True:
            interrupted(cancel)
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                time.sleep(.1)
        artifacts = workspace / "artifacts"
        if plan.native:
            verify_native(plan.native, plan.lock, cancel)
            artifacts.mkdir(mode=0o700)
            for name in artifact_names(plan.lock["native"]):
                _copy_file(plan.native / name, artifacts / name)
        else:
            native = plan.lock["native"]
            archive = fetch(native["archive_url"], native["archive_sha256"], cache / "native.tar.gz")
            artifacts.mkdir(mode=0o700)
            extract(archive, artifacts, cancel)
        verify_native(artifacts, plan.lock, cancel)
        if CONNECTED_STORAGE_FEATURE in plan.lock["native"].get("features", []):
            cache_native(artifacts, plan.lock, cancel)
        (artifacts / "manifest.json").write_text(json.dumps({"format": 1, "files": plan.lock["native"]["files"],
            "features": plan.lock["native"].get("features", [])}) + "\n")
        runner_lock = plan.lock["runner"]
        event("bootstrap", "Der geprüfte Proton-Runner wird heruntergeladen …")
        archive_zip = fetch(runner_lock["url"], runner_lock["zip_sha256"], cache / "runner.zip")
        runner_archive = cache / "runner.tar.xz"
        if not runner_archive.is_file() or runner_archive.is_symlink() or digest(runner_archive, cancel) != runner_lock["archive_sha256"]:
            with zipfile.ZipFile(archive_zip) as archive:
                files = [m for m in archive.infolist() if m.filename.endswith(".tar.xz")]
                if len(files) != 1 or files[0].file_size > 1024 ** 3:
                    raise SetupError("Das Runner-Archiv enthält keine eindeutige Installation.")
                fd_archive, temporary = tempfile.mkstemp(prefix=".runner-", dir=cache)
                try:
                    with archive.open(files[0]) as source, os.fdopen(fd_archive, "wb") as target:
                        while block := source.read(1024 * 1024):
                            interrupted(cancel)
                            target.write(block)
                    if digest(Path(temporary), cancel) != runner_lock["archive_sha256"]:
                        raise SetupError("Die Prüfsumme des Proton-Runners stimmt nicht.")
                    os.replace(temporary, runner_archive)
                finally:
                    Path(temporary).unlink(missing_ok=True)
        event("bootstrap", "Proton wird entpackt und die Grafik eingerichtet …")
        extract(runner_archive, workspace, cancel)
        runner = workspace / runner_lock["directory"]
        if digest(runner / "files/lib/wine/x86_64-windows/xgameruntime.dll", cancel) != runner_lock["original_runtime_sha256"]:
            raise SetupError("Der entpackte Proton-Runner passt nicht zur geprüften Runtime.")
    finally:
        os.close(fd)
    prefix = workspace / "prefix"
    prepare_prefix(runner, prefix, cancel=cancel)
    return {"cli": artifacts / "bin/xodus-cli", "cli_sha256": plan.lock["native"]["files"]["bin/xodus-cli"],
            "cli_features": plan.lock["native"].get("cli_features", []),
            "artifacts_path": artifacts, "runner_path": runner, "prefix_path": prefix, "workspace": workspace}
