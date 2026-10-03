# SPDX-License-Identifier: MIT
"""Reserve the selected runtime while preparing or running the FSDT installer."""
import os
import subprocess
import threading
import time
import uuid

from .backend import LauncherError
from ._fenix import core
from . import gsx_core, gsx_processes


class GSXManager:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.job = None
        self.job_runtime = None
        self.worker = None
        self.closing = False
        self.stop_requested = threading.Event()

    def snapshot(self):
        with self.launcher.lock:
            root = self.launcher.runtime
            busy = self.launcher.setup_busy or self.launcher.desktop_closing
            playing = self.launcher.process is not None
            external = root is not None and self.launcher._external(create_lock=False)
        with self.lock:
            job = dict(self.job) if self.job and self.job_runtime == root else None
        value = gsx_core.snapshot(root)
        running, game = gsx_processes.status(root)
        interactive = bool(job and job["state"] == "running" and job["operation"] == "open" and not job.get("stopping"))
        value.update(runtime_path=str(root) if root else "", job=job, busy=bool(busy or playing or external),
                     manager_running=running, can_change=bool(root and value["idle"] and not busy and not playing and not external),
                     can_stop=bool(root and running and not playing and not game and
                                   (interactive or (not busy and not external)) and not self.closing))
        return value

    def start(self, operation, data):
        if operation not in {"prepare", "open", "configure", "disable", "recover", "stop"}:
            raise LauncherError("Unbekannter GSX-Vorgang.")
        with self.launcher.lock:
            self.launcher.require_open()
            root = self.launcher.runtime
            if root is None or data.get("runtime_path") != str(root):
                raise LauncherError("Die ausgewählte Runtime hat sich geändert. GSX-Status neu laden.")
            if operation != "stop":
                error = gsx_core.proton_error(root, recovery=operation == "recover")
                if error:
                    raise LauncherError(error)
            with self.lock:
                if self.closing:
                    raise LauncherError("Flightdeck wird beendet.")
                if operation == "stop":
                    if not self.snapshot()["can_stop"]:
                        raise LauncherError("Beende MSFS und laufende Installationen, bevor du FSDT schließt.")
                    if self.job and self.job["state"] == "running":
                        self.job.update(stopping=True)
                        self.stop_requested.set()
                        return {"ok": True, "job_id": self.job["id"]}
                self.launcher.reserve_setup()
                self.job_runtime = root
                self.stop_requested = threading.Event()
                self.job = {"id": uuid.uuid4().hex, "operation": operation, "state": "running", "message": "GSX-Einrichtung läuft …"}
                self.worker = threading.Thread(target=self._run, args=(root, operation), daemon=False)
                self.worker.start()
                return {"ok": True, "job_id": self.job["id"]}

    def _progress(self, message):
        with self.lock:
            self.job["message"] = message[:1500]

    def _open(self, root):
        value = gsx_core.snapshot(root)
        if not value["prepared"] or not gsx_core.setup_complete(root):
            raise LauncherError("Bereite zuerst den FSDT-Installer vor.")
        directory = gsx_core.manager_directory(root / "local/msfs-prefix")
        runner = (root / "runner").resolve()
        fd = os.open(root / "private/gsx-manager.log", os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            child = subprocess.Popen([str(runner / "files/bin/wine"), str(directory / "Couatl_Updater.exe"),
                                      "/SILENT", "/INSTALLMODE=TRUE"], cwd=directory,
                                     env=gsx_core.wine_environment(root / "local/msfs-prefix", runner),
                                     stdin=subprocess.DEVNULL, stdout=fd, stderr=subprocess.STDOUT)
            self._progress("Installiere und aktiviere GSX im FSDT-Installer. Schließe ihn danach vollständig.")
            quiet_since = None
            try:
                while True:
                    if self.stop_requested.is_set():
                        gsx_processes.stop(root)
                        if child.poll() is None:
                            child.terminate()
                        child.wait(timeout=5)
                        return
                    result = child.poll()
                    running, _ = gsx_processes.status(root)
                    if result is not None and not running:
                        quiet_since = quiet_since or time.monotonic()
                        if time.monotonic() - quiet_since >= 1.5:
                            if result:
                                raise LauncherError("Der FSDT-Installer wurde unerwartet beendet. Details stehen im GSX-Protokoll.")
                            gsx_processes.stop(root)
                            return
                    else:
                        quiet_since = None
                    self.stop_requested.wait(.25)
            finally:
                if child.poll() is not None:
                    child.wait()
        finally:
            os.close(fd)

    def _run(self, root, operation):
        try:
            with core.locked(root, idle=operation != "stop", recovery=operation == "recover"):
                if operation == "prepare":
                    gsx_core.prepare(root, self.launcher.state_dir / "gsx-downloads", self._progress)
                elif operation == "open":
                    self._open(root)
                elif operation == "recover":
                    gsx_core.recover(root)
                elif operation == "stop":
                    gsx_processes.stop(root)
                else:
                    if not gsx_core.setup_complete(root):
                        raise LauncherError("Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.")
                    gsx_core.configure(root, operation == "configure")
            messages = {"prepare": "FSDT ist vorbereitet. Öffne den Installer, um GSX zu installieren und zu aktivieren.",
                        "open": "FSDT wurde geschlossen. Der GSX-Status wird neu geprüft.", "stop": "FSDT wurde geschlossen. Der GSX-Status wird neu geprüft.",
                        "configure": "GSX startet mit MSFS. Prüfe Menü und Bodendienste im Simulator; Linux-Kompatibilität ist noch unbestätigt.",
                        "disable": "Der automatische GSX-Start ist ausgeschaltet.", "recover": "Das Windows-Profil vor der unterbrochenen GSX-Einrichtung ist wieder aktiv."}
            with self.lock:
                self.job.update(state="complete", message=messages[operation], stopping=False)
        except Exception as error:
            with self.lock:
                message = str(error).replace("private Fenix setup log", "private GSX setup log")
                self.job.update(state="failed", message=message[:1500], stopping=False)
        finally:
            self.launcher.release_setup()

    def close(self):
        with self.lock:
            self.closing = True
