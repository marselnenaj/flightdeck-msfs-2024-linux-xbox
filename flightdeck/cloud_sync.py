# SPDX-License-Identifier: MIT
"""Account-bound cloud saves with explicit import and fenced cloud uploads.

The game and both save providers are locked during a transfer. A reviewed
comparison never grants permission to overwrite a later remote revision.
"""
from __future__ import annotations

import copy
from datetime import datetime, timezone
from dataclasses import replace
import threading
import uuid
import os
import stat
import time

from .backend import LauncherError


def now():
    return datetime.now(timezone.utc).isoformat()


ERRORS = {
    "authentication": "Der Xbox-Spielstandsdienst hat die Anmeldung abgelehnt. Bitte erneut im Spiel anmelden.",
    "auth_required": "Bitte zuerst mit deinem Xbox-Spielprofil anmelden und die Cloud-Abfrage erneut starten.",
    "unauthorized": "Der Xbox-Spielstandsdienst hat die Anmeldung abgelehnt. Bitte erneut im Spiel anmelden.",
    "forbidden": "Der Xbox-Spielstandsdienst erlaubt diesem Spielprofil keinen Zugriff.",
    "changed": "Die Spielstände haben sich seit dem Vergleich geändert. Bitte erneut vergleichen.",
    "cancelled": "Die Cloud-Abfrage wurde abgebrochen.",
    "unsupported": "Dieser Cloud-Datenbestand wird noch nicht unterstützt. Es wurde nichts übernommen.",
    "unsupported_paging": "Dieser Cloud-Datenbestand benötigt eine noch nicht unterstützte Abfrage. Es wurde nichts übernommen.",
    "invalid_scope": "Das Xbox-Spielprofil hat sich geändert. Bitte den Vergleich erneut erstellen.",
    "invalid_snapshot": "Die Cloud-Kopie ist ungültig. Bitte erneut herunterladen.",
    "invalid_plan": "Dieser Spielstandvergleich ist nicht mehr gültig. Bitte erneut vergleichen.",
    "invalid_lock": "Die Spielstandübernahme konnte nicht exklusiv gesperrt werden. Bitte erneut versuchen.",
    "busy": "Die Spielstände werden gerade verwendet. Bitte den Simulator beenden.",
    "conflict": "Die Spielstände unterscheiden sich. Bitte ausdrücklich einen Stand auswählen.",
    "local_storage": "Die Spielstandsicherung oder Übernahme konnte nicht sicher abgeschlossen werden.",
    "not_found": "Der Xbox-Spielstandsdienst konnte diesen Datenbestand nicht bestätigen. Bitte erneut versuchen.",
}
WRITE_ERRORS = {
    "conflict": "Die Cloud-Spielstände haben sich seit dem Vergleich geändert. Bitte erneut vergleichen.",
    "local_changed": "Die lokalen Spielstände haben sich seit dem Vergleich geändert. Bitte erneut vergleichen.",
    "lease_lost": "Die Xbox-Cloud wird auf einem anderen Gerät verwendet oder die Sperre ist abgelaufen. Bitte später erneut vergleichen.",
    "authentication": "Der Xbox-Spielstandsdienst hat die Anmeldung abgelehnt. Bitte erneut im Spiel anmelden.",
    "quota": "Für diese Spielstände reicht der verfügbare Xbox-Cloud-Speicher nicht aus.",
    "cancelled": "Der Upload wurde abgebrochen, bevor ein Cloud-Spielstand ersetzt wurde.",
}
FAILED = "Die Cloud-Abfrage konnte nicht abgeschlossen werden. Deine Spielstände wurden nicht verändert."


def _snapshot_directory(runtime):
    from .cloud_storage import _private_directory, _same_directory
    directory = runtime / "private/cloud-saves"
    parent = _private_directory(runtime / "private")
    child = None
    try:
        try:
            os.mkdir("cloud-saves", 0o700, dir_fd=parent)
            os.fsync(parent)
        except FileExistsError:
            pass
        child = os.open("cloud-saves", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        info = os.fstat(child)
        if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077:
            raise OSError("Unsafe snapshot directory")
        _same_directory(child, directory)
    finally:
        if child is not None:
            os.close(child)
        os.close(parent)
    return directory


class CloudSaveManager:
    def __init__(self, launcher, runtime_api=None):
        self.launcher = launcher
        self.runtime_api = runtime_api
        self.lock = threading.RLock()
        self.job = None
        self.job_runtime = None
        self.worker = None
        self.cancel_event = threading.Event()
        self.closed = False
        self.plan = None
        self.plan_runtime = None
        self.plan_id = None
        self.plan_deadline = 0.0
        self.restore_id = None
        self.restore_runtime = None
        from .cloud_auto import CloudAutomation
        self.automation = CloudAutomation(self)

    def launch(self):
        return self.automation.launch()

    def _api(self):
        if self.runtime_api is not None:
            return self.runtime_api
        from . import cloud_runtime
        return cloud_runtime

    def snapshot(self):
        launcher = self.launcher
        with launcher.lock:
            runtime = launcher.runtime
            launcher._poll()
            busy = launcher.setup_busy or launcher.process is not None or launcher.desktop_closing
            if runtime is not None and not busy:
                busy = launcher._external(create_lock=False)
            source = launcher.setup.source_root
            local_enabled = launcher.saves(False)["available"]
        available = writable = False
        if runtime is not None:
            try:
                available = self._api().available(runtime, source, verify=False)
                writable = self._api().available(runtime, source, verify=False, write=True)
            except (ImportError, OSError, ValueError):
                pass
        with self.lock:
            job = copy.deepcopy(self.job) if self.job_runtime == runtime else None
            closed = self.closed
            plan = self._plan_summary(runtime)
            restore_id = self.restore_id if self.restore_runtime == runtime else None
        return {"available": bool(available), "mode": "automatic_sync" if writable else "download_and_import",
                "sync_supported": bool(writable), "automatic_sync": bool(writable),
                "automatic": self.automation.snapshot(),
                "can_upload": bool(writable and local_enabled and plan and not busy and not closed),
                "can_restore": bool(available and local_enabled and restore_id and not busy and not closed),
                "restore_id": restore_id,
                "plan": plan,
                "can_prepare_import": bool(available and local_enabled and not busy and not closed),
                "can_import": bool(available and local_enabled and plan and not busy and not closed),
                "can_check": bool(available and not busy and not closed),
                "can_download": bool(available and not busy and not closed),
                "can_cancel": bool(job and job["state"] == "running"), "job": job}

    def _plan_summary(self, runtime):
        if (self.plan is None or self.plan_runtime != runtime
                or time.monotonic() >= self.plan_deadline):
            return None
        return {"id": self.plan_id, **self.plan.summary()}

    def start(self, operation, *, plan_id=None, choice=None, backup_id=None):
        if operation not in {"check", "download", "prepare-import", "import", "upload", "restore"}:
            raise LauncherError("Diese Cloud-Aktion ist nicht verfügbar.")
        launcher = self.launcher
        # Shared order with Launcher mutations: launcher lock before this lock.
        with launcher.lock:
            launcher.require_open()
            with self.lock:
                if self.closed or (self.worker is not None and self.worker.is_alive()):
                    raise LauncherError("Eine Cloud-Abfrage läuft bereits.")
                if launcher.runtime is None:
                    raise LauncherError("Zuerst eine Runtime auswählen.")
                try:
                    api = self._api()
                    usable = api.available(launcher.runtime, launcher.setup.source_root, verify=True, **({"write": True} if operation == "upload" else {}))
                except (ImportError, OSError, ValueError):
                    usable = False
                if not usable:
                    raise LauncherError("Die Cloud-Komponente fehlt. Bitte das aktuelle vollständige Flightdeck-Paket installieren.")
                if operation in {"prepare-import", "import", "upload", "restore"} and not launcher.saves(False)["available"]:
                    raise LauncherError("Für die Übernahme müssen lokale Spielstände in dieser Runtime aktiviert sein.")
                selected_plan = None
                if operation in {"import", "upload"}:
                    if (not self._plan_summary(launcher.runtime) or plan_id != self.plan_id
                            or choice != ("cloud" if operation == "import" else "local")):
                        raise LauncherError("Dieser Spielstandvergleich ist nicht mehr gültig. Bitte erneut vergleichen.")
                    selected_plan = self.plan
                if operation == "restore" and (not backup_id or backup_id != self.restore_id or self.restore_runtime != launcher.runtime):
                    raise LauncherError("Diese Spielstandsicherung ist nicht mehr für eine Rücknahme verfügbar.")
                launcher.reserve_setup()
                if operation in {"prepare-import", "import", "upload", "restore"}:
                    self.plan = None
                    self.plan_id = None
                    self.plan_runtime = None
                runtime = launcher.runtime
                self.cancel_event = threading.Event()
                self.job_runtime = runtime
                self.job = {"id": uuid.uuid4().hex, "operation": operation, "state": "running",
                            "started_at": now(), "finished_at": None, "result": None,
                            "error_code": None, "message": "Cloud-Spielstände werden abgefragt …"}
                self.worker = threading.Thread(target=self._run, args=(runtime, api, operation, selected_plan, backup_id), daemon=True,
                                               name="flightdeck-cloud-saves")
                try:
                    self.worker.start()
                except BaseException:
                    self.job = None
                    launcher.release_setup()
                    raise
                return {"ok": True, "job_id": self.job["id"]}

    def _run(self, runtime, api, operation, selected_plan=None, backup_id=None):
        result = None
        prepared = None
        try:
            with self.launcher.runtime_lock(operation="cloud_saves") as lock_fd:
                with api.open_client(runtime, self.launcher.setup.source_root, cancel=self.cancel_event) as client:
                    if self.cancel_event.is_set():
                        raise InterruptedError()
                    if operation == "upload":
                        result = self._upload(runtime, client, selected_plan, lock_fd)
                    elif operation == "restore":
                        from . import cloud_import
                        restored = cloud_import.restore(runtime, client.scope, backup_id, runtime_lock_fd=lock_fd, cancel=self.cancel_event)
                        result = {**restored.summary(), "restored": True, "downloaded": False, "rechecked": False}
                    elif operation == "check":
                        inventory = client.read_inventory(cancel=self.cancel_event)
                        result = {"container_count": inventory.container_count,
                                  "blob_count": None, "total_bytes": inventory.total_bytes,
                                  "downloaded": False, "rechecked": False}
                    else:
                        snapshot = client.download_snapshot(_snapshot_directory(runtime),
                                                            cancel=self.cancel_event)
                        result = {"container_count": snapshot.container_count,
                                  "blob_count": snapshot.blob_count, "total_bytes": snapshot.total_bytes,
                                  "downloaded": True, "rechecked": True,
                                  "snapshot_id": snapshot.path.name,
                                  "consistency": snapshot.consistency}
                        if operation in {"prepare-import", "import"}:
                            from . import cloud_import
                            prepared = cloud_import.prepare(runtime, client.scope, snapshot,
                                baseline=cloud_import.load_baseline(runtime, client.scope))
                            if operation == "prepare-import":
                                result["prepared_for_import"] = True
                            else:
                                if selected_plan.remote_digest != prepared.remote_digest:
                                    raise cloud_import.CloudImportError("changed")
                                # The new read confirms cloud content; apply also checks
                                # this fresh account and the original local fingerprint.
                                imported = cloud_import.apply(runtime, client.scope, selected_plan,
                                    choice="cloud", runtime_lock_fd=lock_fd, cancel=self.cancel_event)
                                result.update(imported.summary())
                                prepared = None
                    if operation in {"check", "prepare-import"} and self.cancel_event.is_set():
                        raise InterruptedError()
            with self.lock:
                if prepared is not None:
                    self.plan, self.plan_runtime = prepared, runtime
                    self.plan_id = uuid.uuid4().hex
                    self.plan_deadline = time.monotonic() + 15 * 60
                if operation == "import" and result.get("imported"):
                    self.restore_id, self.restore_runtime = result.get("backup_id"), runtime
                elif operation == "restore":
                    self.restore_id = self.restore_runtime = None
                message = ("Lokale Spielstände wiederhergestellt." if result.get("restored") else
                           "Lokale Spielstände hochgeladen und in der Xbox-Cloud geprüft." if result.get("uploaded") else
                           "Cloud-Spielstände übernommen. Der vorherige lokale Stand wurde gesichert."
                           if result.get("imported") else
                           "Die lokalen Spielstände stimmen bereits mit der Cloud überein." if operation == "import" else
                           "Spielstände verglichen. Wähle den Stand aus, den MSFS verwenden soll."
                           if operation == "prepare-import" else
                           "Cloud-Kopie heruntergeladen. Lokale Spielstände wurden nicht ersetzt."
                           if operation == "download" else "Cloud-Spielstände wurden gefunden."
                           if result["container_count"] else "Für dieses Spielprofil sind keine Cloud-Spielstände vorhanden.")
                self.job.update(state="succeeded", result=result, finished_at=now(), message=message)
                if result.get("imported") and not result.get("durability_confirmed", True):
                    self.job.update(warning="durability_unknown",
                        message="Die Cloud-Spielstände wurden übernommen. Die endgültige Speicherung konnte nicht bestätigt werden; die Sicherung bleibt erhalten.")
                if result.get("uploaded") and (not result["lease_released"] or not result["baseline_saved"] or result.get("local_cleanup_failed")):
                    self.job.update(warning="sync_cleanup",
                        message="Der Upload wurde durch erneutes Lesen bestätigt. Das Speichern des Vergleichsstands oder die Freigabe der Verbindung konnte nicht bestätigt werden. Bitte vor weiteren Änderungen erneut vergleichen.")
        except Exception as error:
            # A completed atomic download remains a valid independent archive
            # even when the helper's later cleanup is interrupted or fails.
            if result is not None and (result.get("uploaded") or result.get("restored") or result.get("imported") or
                    (operation == "download" and result.get("downloaded"))):
                with self.lock:
                    if operation == "import" and result.get("imported"):
                        self.restore_id, self.restore_runtime = result.get("backup_id"), runtime
                    elif operation == "restore":
                        self.restore_id = self.restore_runtime = None
                    self.job.update(state="succeeded", result=result, finished_at=now(),
                                    message="Upload geprüft. Die Verbindung konnte nicht vollständig geschlossen werden."
                                    if result.get("uploaded") else "Spielstände wiederhergestellt. Die Verbindung konnte nicht vollständig geschlossen werden."
                                    if result.get("restored") else "Spielstandübernahme abgeschlossen. Die Verbindung konnte nicht vollständig geschlossen werden."
                                    if result.get("imported") else "Cloud-Kopie gespeichert. Die Verbindung konnte nicht vollständig geschlossen werden.",
                                    warning="connection_cleanup")
                return
            from .cloud_storage import CloudStorageError
            from .cloud_import import CloudImportError
            from .cloud_write import CloudWriteError
            if isinstance(error, CloudWriteError):
                with self.lock:
                    self.job.update(state="failed" if error.recovery_required else "cancelled" if error.code == "cancelled" else "failed",
                        error_code=error.code, recovery_required=error.recovery_required,
                        committed_containers=error.committed_containers, result=None, finished_at=now(),
                        message="Der Upload wurde nicht vollständig bestätigt. Teile der Cloud können bereits aktualisiert sein. Vergleiche die Spielstände erneut; die heruntergeladenen Kopien bleiben erhalten."
                        if error.recovery_required else WRITE_ERRORS.get(error.code, "Der Upload konnte nicht gestartet oder bestätigt werden. Bitte erneut vergleichen."))
                return
            code = getattr(error, "code", None) if isinstance(error, (CloudStorageError, CloudImportError)) else None
            if self.cancel_event.is_set() or isinstance(error, InterruptedError):
                code = "cancelled"
            if code not in ERRORS:
                code = "failed"
            with self.lock:
                if operation == "upload":
                    # An unclassified failure cannot prove that no remote
                    # commit occurred, including failures in context cleanup.
                    self.job.update(state="failed", error_code=code, recovery_required=True,
                        committed_containers=None, result=None, finished_at=now(),
                        message="Der Upload wurde nicht vollständig bestätigt. Teile der Cloud können bereits aktualisiert sein. Vergleiche die Spielstände erneut; die heruntergeladenen Kopien bleiben erhalten.")
                    return
                self.job.update(state="cancelled" if code == "cancelled" else "failed", error_code=code,
                                message=ERRORS.get(code, FAILED), result=None, finished_at=now())
        finally:
            self.launcher.release_setup()

    def _upload(self, runtime, client, plan, lock_fd):
        from . import cloud_import, cloud_storage, cloud_write
        result = None
        try:
            with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd,
                                           cancel=self.cancel_event) as local:
                if local.scope_binding != plan.scope_binding:
                    raise cloud_import.CloudImportError("invalid_scope")
                if local.local_digest != plan.local_digest:
                    raise cloud_import.CloudImportError("changed")
                def read_remote(*, timeout):
                    limited = cloud_storage.CloudStorageClient(client.scope, client.transport,
                        limits=replace(client.limits, deadline_seconds=min(timeout, client.limits.deadline_seconds)))
                    snapshot = limited.download_snapshot(_snapshot_directory(runtime), cancel=self.cancel_event)
                    return cloud_import.read_snapshot(runtime, client.scope, snapshot)
                receipt = cloud_write.upload(client.scope, client.transport, read_remote, local.state,
                    expected_remote_digest=plan.remote_digest, assert_local_unchanged=local.assert_unchanged,
                    cancel=self.cancel_event)
                result = {"uploaded": True, "downloaded": False, "rechecked": True,
                          "container_count": receipt.container_count, "blob_count": receipt.blob_count,
                          "total_bytes": receipt.total_bytes, "changed_containers": receipt.changed_containers,
                          "lease_released": receipt.lease_released, "baseline_saved": False}
                try:
                    cloud_import.record_common(local, receipt)
                    result["baseline_saved"] = True
                except Exception:
                    pass  # Verified remote success survives a local receipt failure.
                return result
        except Exception:
            if result is None:
                raise
            result["local_cleanup_failed"] = True
            return result

    def cancel(self, job_id):
        with self.lock:
            if not self.job or self.job["id"] != job_id or self.job["state"] != "running":
                raise LauncherError("Diese Cloud-Abfrage läuft nicht mehr.")
            self.cancel_event.set()
        return {"ok": True}

    def discard_plan(self, plan_id):
        with self.lock:
            if plan_id != self.plan_id or not self.plan_id:
                raise LauncherError("Dieser Spielstandvergleich ist nicht mehr gültig. Bitte erneut vergleichen.")
            self.plan = self.plan_id = self.plan_runtime = None
        return {"ok": True}

    def close(self):
        self.automation.close()
        with self.lock:
            self.closed = True
            self.cancel_event.set()
            worker = self.worker
        if worker is not None and worker is not threading.current_thread():
            worker.join(timeout=5)
