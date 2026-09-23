# SPDX-License-Identifier: MIT
"""Optional Fenix installer jobs; reserve the same runtime as game/setup jobs.

The installer engine is vendored from fenix-a320-linux-patch. Only fixed payload
files are extracted; downloaded Python/shell code is never imported by Flightdeck.
"""
from __future__ import annotations
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import threading
import uuid
import zipfile

from .backend import LauncherError
from ._fenix import core

packaged = Path(__file__).resolve().parent / "resources/fenix"
core.ROOT = packaged if packaged.is_dir() else Path(__file__).resolve().parents[1] / "compat/fenix"


def obtain_bundle(cache, supplied, progress):
    if supplied:
        if not isinstance(supplied, str) or len(supplied) > 4096 or not Path(supplied).expanduser().is_absolute():
            raise core.PatchError("Select the extracted Fenix patch release directory.")
        return core.verify_bundle(supplied)
    release = core.read_json(core.ROOT / "release.json")
    lock = core.manifest()
    cache.mkdir(parents=True, exist_ok=True, mode=0o700)
    target = cache / lock["version"]
    if target.exists():
        return core.verify_bundle(target)
    progress("Das geprüfte Fenix-Paket wird heruntergeladen …")
    archive = core.download(release["url"], cache / (lock["version"] + ".zip"), release["sha256"], progress)
    wanted = {"bundle.json": None}
    wanted.update({"payload/" + key: value for key, value in lock["files"].items()})
    wanted.update({"integration/" + key: value for key, value in lock["integration"].items()})
    with tempfile.TemporaryDirectory(prefix=".fenix-extract-", dir=cache) as temporary:
        stage = Path(temporary)
        with zipfile.ZipFile(archive) as zipped:
            seen = set()
            for info in zipped.infolist():
                name = info.filename.removeprefix(release["archive_root"] + "/")
                if name not in wanted:
                    continue
                if name in seen or info.file_size > 32 * 1024 * 1024 or stat.S_ISLNK(info.external_attr >> 16):
                    raise core.PatchError("Invalid file in the Fenix release archive")
                seen.add(name)
                core.atomic(stage / name, zipped.read(info))
            if seen != set(wanted):
                raise core.PatchError("Incomplete Fenix release archive")
        core.verify_bundle(stage)
        os.rename(stage, target)
    return target


class FenixManager:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.job = None
        self.worker = None
        self.closing = False

    def snapshot(self):
        with self.launcher.lock:
            root = self.launcher.runtime
            busy = self.launcher.setup_busy or self.launcher.desktop_closing
            running = self.launcher.process is not None or (root is not None and self.launcher._external(create_lock=False))
        with self.lock:
            job = dict(self.job) if self.job else None
        value = core.snapshot(root) if root else {"state": "unavailable", "message": "Zuerst MSFS 2024 in Flightdeck einrichten."}
        # These files establish that display configuration can proceed, not
        # that the user's account or license has been verified by Fenix.
        value["settings_ready"] = bool(root and value.get("fenix_installed") and all(
            (root / "local/msfs-prefix" / core.CONFIG / name).is_file()
            for name in ("fenixConfig.xml", "persistancy.xml")))
        value.update(job=job, runtime_path=str(root) if root else "", busy=busy or running,
                     can_change=bool(root and not busy and not running and value.get("idle")),
                     project="https://github.com/marselnenaj/fenix-a320-linux-patch")
        return value

    def start(self, operation, data):
        if operation not in {"install", "configure", "restore", "installer", "open", "manager"}:
            raise LauncherError("Unbekannter Fenix-Vorgang.")
        with self.launcher.lock:
            self.launcher.require_open()
            self.launcher.reserve_setup()
            root = self.launcher.runtime
            if root is None:
                self.launcher.release_setup()
                raise LauncherError("Zuerst MSFS 2024 in Flightdeck einrichten.")
            with self.lock:
                if self.closing:
                    self.launcher.release_setup()
                    raise LauncherError("Flightdeck wird beendet.")
                self.job = {"id": uuid.uuid4().hex, "operation": operation, "state": "running", "message": "Fenix-Einrichtung wird vorbereitet …"}
                self.worker = threading.Thread(target=self._run, args=(root, operation, dict(data)), daemon=False)
                self.worker.start()
                return {"ok": True, "job_id": self.job["id"]}

    def _progress(self, text):
        with self.lock:
            self.job["message"] = text[:1500]

    def _run(self, root, operation, data):
        try:
            if operation == "install":
                bundle = obtain_bundle(self.launcher.state_dir / "fenix-bundles", data.get("bundle_path"), self._progress)
                core.install(root, bundle, self._progress)
            elif operation == "restore":
                core.restore(root, self._progress)
            elif operation == "configure":
                core.configure(root, self._progress)
            else:
                executable = data.get("installer_path") if operation == "installer" else None
                if operation == "installer" and (not isinstance(executable, str) or not executable or len(executable) > 4096 or not Path(executable).expanduser().is_absolute()):
                    raise core.PatchError("Bitte die offizielle Fenix-Installer-EXE auswählen.")
                core.windows_app(root, executable, self._progress, manager=operation == "manager")
            with self.lock:
                self.job.update(state="complete", message={
                    "install": "Patch installiert. Installiere jetzt Fenix mit dem offiziellen Installer.",
                    "installer": "Installer beendet. Prüfe die nächsten Schritte oben; die Fenix-Einrichtung ist noch nicht automatisch abgeschlossen.",
                    "open": "Fenix-Fenster geschlossen. Wende jetzt die Anzeige-Einstellungen an.",
                    "configure": "Anzeigen und automatischer Fenix-Start sind eingerichtet.",
                    "manager": "Fenix-Manager geschlossen.",
                    "restore": "Das Profil vor dem Patch wurde wiederhergestellt.",
                }[operation])
        except Exception as error:
            with self.lock:
                self.job.update(state="failed", message=str(error)[:1500])
        finally:
            self.launcher.release_setup()

    def pick(self, kind):
        if kind not in {"installer", "bundle"}:
            raise LauncherError("Ungültige Fenix-Dateiauswahl.")
        picker = self.launcher.setup._picker()
        if not picker:
            raise LauncherError("Kein Dateidialog verfügbar. Bitte den Pfad direkt eingeben.")
        with self.launcher.lock:
            self.launcher.require_open()
            if not self.launcher.setup.picker_lock.acquire(blocking=False):
                raise LauncherError("Ein Dateidialog ist bereits geöffnet.")
        try:
            name, command = picker
            if name == "zenity":
                args = [command, "--file-selection", "--title=Fenix Installer" if kind == "installer" else "--title=Fenix Patch"]
                args += ["--file-filter=Windows Installer | *.exe"] if kind == "installer" else ["--directory"]
            else:
                args = [command, "--getopenfilename" if kind == "installer" else "--getexistingdirectory", str(Path.home())]
            result = subprocess.run(args, capture_output=True, text=True, timeout=120, check=False)
            path = result.stdout.rstrip("\r\n")
            if result.returncode or not path or not Path(path).is_absolute() or len(path) > 4096:
                return {"ok": True, "cancelled": True}
            return {"ok": True, "path": path}
        except subprocess.TimeoutExpired:
            return {"ok": True, "cancelled": True}
        finally:
            self.launcher.setup.picker_lock.release()

    def close(self):
        with self.lock:
            self.closing = True
