# SPDX-License-Identifier: MIT
"""Run Xodus' real Microsoft login and licensed MSFS streaming download.

The caller supplies a hash-verified locally built CLI and a new private download
folder. Child output is discarded: existing Xodus errors can contain tokens or
signed URLs, and its terminal progress is not a stable machine-readable API.
Progress uses a separate bounded numeric protocol only when declared by the
hash-pinned CLI. No progress event establishes ownership or install success.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import threading
import time
import xml.etree.ElementTree as ET

from .i18n import message

MSFS_STORE_ID = "9P38D19T7LRV"
RESUME_FEATURE = "streaming-resume-files-v1"
PROGRESS_FEATURE = "streaming-progress-v1"


class DownloadControl:
    """One job's pause request; never signals a recorded or foreign PID.

    The worker acknowledges a pause only after its streaming child is reaped.
    No process, HTTP connection or auth response is suspended with SIGSTOP.
    """
    def __init__(self, changed=None):
        self.lock = threading.RLock()
        self.requested = threading.Event()
        self.changed = changed
        self.state = "unavailable"

    def _set(self, state):
        self.state = state
        if self.changed:
            self.changed(state)

    def downloading(self, supported):
        with self.lock:
            self._set("download" if supported else "unavailable")

    def pause(self):
        with self.lock:
            if self.state != "download":
                raise GameInstallError("Dieser Download kann gerade nicht pausiert werden.")
            self.requested.set()
            self._set("pausing")

    def wait_paused(self, cancel):
        with self.lock:
            self._set("paused")
        while self.requested.is_set():
            _cancelled(cancel)
            if cancel is not None:
                cancel.wait(.1)
            else:
                time.sleep(.1)
        _cancelled(cancel)

    def resume(self):
        with self.lock:
            if self.state != "paused":
                raise GameInstallError("Es gibt keinen pausierten Download zum Fortsetzen.")
            self._set("resuming")
            self.requested.clear()

    def finish(self):
        with self.lock:
            self._set("unavailable")
            self.requested.clear()


class _PauseDownload(Exception):
    pass


class GameInstallError(ValueError):
    pass


class GameInstallCancelled(GameInstallError):
    pass


def _cancelled(cancel):
    if cancel is not None and cancel.is_set():
        raise GameInstallCancelled("Einrichtung abgebrochen.")


def verify_cli(cli, expected_sha256):
    path = Path(cli)
    if not path.is_absolute() or not path.is_file() or not os.access(path, os.X_OK):
        raise GameInstallError("Der verifizierte Xodus-Installer fehlt. Bitte zuerst die Laufzeitkomponenten vorbereiten.")
    if not isinstance(expected_sha256, str) or not re.fullmatch(r"[a-f0-9]{64}", expected_sha256):
        raise GameInstallError("Für den Xodus-Installer fehlt eine gültige Build-Prüfsumme.")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    if digest.hexdigest() != expected_sha256:
        raise GameInstallError("Die Prüfsumme des Xodus-Installers stimmt nicht. Bitte die Laufzeitkomponenten erneut vorbereiten.")
    return path


def _stop_owned(process):
    """Cancel only the process group started by this installation job."""
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()


def run_cli(cli, arguments, *, cwd, cancel, timeout=None, xdg_root=None, control=None, transfer=None):
    """Fixed argv supplied by this module; never a shell or caller command."""
    _cancelled(cancel)
    environment = dict(os.environ)
    environment.update(XODUS_LOG="off", RUST_BACKTRACE="0")
    if xdg_root is not None:
        for category in ("config", "data", "cache", "state"):
            environment["XDG_" + category.upper() + "_HOME"] = str(xdg_root / category)
    started = time.monotonic()
    from .download_progress import ProgressPipe
    progress = ProgressPipe(transfer) if transfer is not None else None
    process = None
    try:
        command = [str(cli), *arguments]
        options = {}
        if progress:
            command += ["--progress-fd", str(progress.write_fd)]
            options["pass_fds"] = (progress.write_fd,)
        process = subprocess.Popen(command, cwd=cwd, env=environment,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True,
                                   close_fds=True, umask=0o077, **options)
        if progress:
            progress.child_started()
        while process.poll() is None:
            _cancelled(cancel)
            if control is not None and control.requested.is_set():
                raise _PauseDownload()
            if timeout is not None and time.monotonic() - started > timeout:
                raise GameInstallError("Die Microsoft-Anmeldung hat zu lange gedauert. Bitte erneut versuchen.")
            if progress:
                progress.poll()
            if cancel is not None:
                cancel.wait(.1)
            else:
                time.sleep(.1)
        _cancelled(cancel)
        return process.returncode
    finally:
        try:
            if process is not None:
                _stop_owned(process)
            if progress:
                progress.poll(final=True)
        finally:
            if progress:
                progress.close()


def validate_download(destination):
    """A zero CLI exit alone is insufficient in the pinned streaming command."""
    root = Path(destination)
    marker = root / ".xodus-streaming.msixvc"
    temporary = root / ".xodus-streaming-tmp.msixvc"
    executable = root / "FlightSimulator2024.exe"
    config = root / "MicrosoftGame.Config"
    if not config.is_file():
        config = root / "MicrosoftGame.config"
    for path in (marker, executable, config):
        if path.is_symlink() or not path.is_file() or not stat.S_ISREG(path.stat().st_mode) or path.stat().st_size == 0:
            raise GameInstallError("Der Spieldownload wurde nicht vollständig abgeschlossen. Bitte Anmeldung, Kaufberechtigung, Speicherplatz und Verbindung prüfen.")
    if temporary.exists() or temporary.is_symlink():
        raise GameInstallError("Der Spieldownload enthält noch unvollständige Daten. Bitte erneut versuchen.")
    # MSIXVC can deliberately keep the executable encrypted on disk. The
    # runtime decrypts it for execution; a plaintext MZ check would reject it.
    if config.stat().st_size > 1024 * 1024:
        raise GameInstallError("Die heruntergeladene Spielkonfiguration ist ungültig.")
    try:
        tree = ET.fromstring(config.read_bytes())
    except (ET.ParseError, OSError):
        raise GameInstallError("Die heruntergeladene Spielkonfiguration ist ungültig.") from None
    if tree.tag.rsplit("}", 1)[-1] != "Game":
        raise GameInstallError("Die heruntergeladene Spielkonfiguration ist ungültig.")
    if not any(node.tag.rsplit("}", 1)[-1] == "Executable" and node.get("Name", "").lower() == "flightsimulator2024.exe" for node in tree.iter()):
        raise GameInstallError("Die heruntergeladene Spielkonfiguration ist ungültig.")
    return root


def download_game(cli, expected_cli_sha256, destination, market, *, cancel=None, notify=None, xdg_root=None,
                  control=None, cli_features=(), sign_in=True, expected_package=None, transfer=None):
    """Explicit action: Microsoft UI login, then the licensed streaming command.

    No CIK-dumping `license` command is used. Xodus obtains the content license
    before decrypting files. A new destination is mandatory. Partial downloads
    stay in this job-owned folder on failure/cancel. Only a hash-pinned CLI whose
    release metadata declares RESUME_FEATURE may restart in this same session.
    Its atomic journal verifies completed files; an incomplete file restarts.
    """
    if not isinstance(market, str) or not re.fullmatch(r"[A-Z]{2}", market):
        raise GameInstallError("Bitte einen Ländercode mit zwei Großbuchstaben wählen, zum Beispiel AT.")
    path = verify_cli(cli, expected_cli_sha256)
    target = Path(destination)
    if not target.is_absolute() or not target.parent.is_dir():
        raise GameInstallError("Der private Downloadordner ist noch nicht vorbereitet.")
    if target.exists() or target.is_symlink():
        raise GameInstallError("Der Downloadordner existiert bereits und wird nicht überschrieben.")
    _cancelled(cancel)
    target.mkdir(mode=0o700)
    xdg_root = Path(xdg_root) if xdg_root is not None else target.parent / "xdg"
    if not xdg_root.is_absolute() or xdg_root.is_symlink():
        raise GameInstallError("Der private Downloadordner ist noch nicht vorbereitet.")
    xdg_root.mkdir(mode=0o700, exist_ok=True)
    for category in ("config", "data", "cache", "state"):
        directory = xdg_root / category
        if directory.is_symlink():
            raise GameInstallError("Der private Downloadordner ist noch nicht vorbereitet.")
        directory.mkdir(mode=0o700, exist_ok=True)

    def event(phase, text):
        _cancelled(cancel)
        if phase == "authentication" and transfer is not None:
            transfer(None)
        if notify:
            notify(phase, text, None, None)

    if sign_in:
        event("authentication", "Bitte im Microsoft-Fenster mit dem Konto anmelden, das MSFS besitzt.")
        result = run_cli(path, ["login"], cwd=target.parent, cancel=cancel, timeout=15 * 60, xdg_root=xdg_root)
        if result:
            raise GameInstallError(message("Die Microsoft-Anmeldung wurde nicht abgeschlossen (Code {code}). Bitte erneut versuchen.", code=result))
    # Recheck the exact executable before a second independent invocation.
    path = verify_cli(cli, expected_cli_sha256)
    event("download", "MSFS wird über Xodus angefordert, die Lizenz geprüft und das Spiel heruntergeladen. Das kann längere Zeit dauern …")
    supported = isinstance(cli_features, (list, tuple)) and RESUME_FEATURE in cli_features
    progress_supported = supported and PROGRESS_FEATURE in cli_features
    arguments = ["streaming", MSFS_STORE_ID, str(target), "--market", market, "--parallel", "4"]
    if supported:
        arguments.append("--resume-files")
    if expected_package is not None:
        if not supported or "package-info-json-v1" not in cli_features or not isinstance(expected_package, str) or not re.fullmatch("[0-9a-f]{64}", expected_package):
            raise GameInstallError("Diese Flightdeck-Komponenten unterstützen noch keine sicheren Spielupdates. Bitte Flightdeck aktualisieren.")
        arguments.extend(["--expect-package", expected_package])
    try:
        if control:
            control.downloading(supported)
        reauthenticated = False
        while True:
            _cancelled(cancel)
            path = verify_cli(cli, expected_cli_sha256)
            try:
                result = run_cli(path, arguments, cwd=target.parent, cancel=cancel, xdg_root=xdg_root,
                                 control=control if supported else None,
                                 transfer=transfer if progress_supported else None)
                if supported and result == 77 and not reauthenticated:
                    if control:
                        control.finish()
                    event("authentication", "Die Anmeldung ist abgelaufen. Bitte erneut bei Microsoft anmelden; vollständige Downloads bleiben erhalten.")
                    login = run_cli(verify_cli(cli, expected_cli_sha256), ["login"], cwd=target.parent,
                                    cancel=cancel, timeout=15 * 60, xdg_root=xdg_root)
                    if login:
                        raise GameInstallError(message("Die Microsoft-Anmeldung wurde nicht abgeschlossen (Code {code}). Bitte erneut versuchen.", code=login))
                    reauthenticated = True
                    event("download", "Der Download wird mit frischen Zugängen fortgesetzt. Vollständige Dateien werden erneut geprüft …")
                    if control:
                        control.downloading(True)
                    continue
                break
            except _PauseDownload:
                # run_cli's finally has fully stopped/reaped the owned child.
                control.wait_paused(cancel)
                reauthenticated = False
                event("download", "Der Download wird mit frischen Zugängen fortgesetzt. Vollständige Dateien werden erneut geprüft …")
                control.downloading(True)
        if result == 78 and expected_package is not None:
            raise GameInstallError("Im Store ist inzwischen eine andere Paketrevision verfügbar. Bitte das Update erneut prüfen; die bisherige Installation bleibt erhalten.")
        if result:
            raise GameInstallError(message("Der Spieldownload wurde nicht abgeschlossen (Code {code}). Bitte Kaufberechtigung, Speicherplatz und Verbindung prüfen.", code=result))
        _cancelled(cancel)
        game = validate_download(target)
        if "streaming-integrity-index-v1" in cli_features:
            from .game_integrity import record_installation
            try:
                record_installation(game)
            except (OSError, ValueError, TypeError):
                raise GameInstallError("Der vollständige Download-Prüfnachweis fehlt oder ist ungültig. Das Paket wurde nicht aktiviert.") from None
        return game
    finally:
        if transfer is not None:
            transfer(None)
        if control:
            control.finish()
