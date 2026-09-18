# SPDX-License-Identifier: MIT
"""Cloud-first game sessions; network work never runs in a status request.

One worker owns the runtime from the initial comparison through the exact game
child's exit and final upload. Durable, account-bound journals survive a lost
launcher, and an uncertain upload is reconciled before another game starts.
"""
from __future__ import annotations

from contextlib import contextmanager
from dataclasses import replace
import os
import stat
import threading
import time
import uuid

from .backend import LauncherError
from . import cloud_cache, cloud_import, cloud_policy, cloud_process_guard, cloud_session, cloud_storage, cloud_write
from .cloud_sync import _snapshot_directory, now


MESSAGES = {
    "before": "Cloud-Spielstände werden geladen. Der Simulator startet anschließend automatisch.",
    "after": "Deine Spielstände werden in der Xbox-Cloud gespeichert. Eine lokale Sicherung bleibt erhalten.",
    "playing": "Cloud-Spielstände geladen. Änderungen werden nach dem Beenden synchronisiert.",
    "synced": "Deine Spielstände sind mit der Xbox-Cloud synchronisiert.",
    "conflict": "Auf diesem Rechner und in der Cloud gibt es unterschiedliche Änderungen. Welchen Stand möchtest du verwenden?",
    "authentication": "Die Xbox-Anmeldung ist noch nicht verfügbar. Versuche es erneut oder starte das Spiel zur Anmeldung mit lokalen Spielständen.",
    "failed_before": "Der Cloud-Abgleich konnte nicht abgeschlossen werden. Versuche es erneut oder spiele mit dem gesicherten lokalen Stand.",
    "failed_after": "Deine Spielstände sind lokal gesichert. Der Cloud-Upload ist noch nicht abgeschlossen; Flightdeck prüft ihn vor dem nächsten Start erneut.",
    "local": "Diese Sitzung verwendet lokale Spielstände. Der Cloud-Abgleich wird beim nächsten Start erneut versucht.",
    "cancelled": "Der Start wurde abgebrochen. Gesicherte Spielstände bleiben erhalten.",
    "cleanup": "Die Spielstände wurden synchronisiert. Die Verbindung konnte nicht vollständig geschlossen werden.",
    "unsafe_session": "Die vorherige Spielsitzung wurde unerwartet unterbrochen. Bitte Linux neu starten, bevor Flightdeck die Spielstände erneut verwendet.",
}
_OFFLINE = "cloud-offline.pending"
_OFFLINE_BODY = b"Flightdeck local session pending\n"


class Attention(Exception):
    def __init__(self, code, plan=None):
        self.code, self.plan = code, plan
        super().__init__(code)


def _private(runtime):
    return cloud_storage._private_directory(runtime / "private")


def _offline(runtime, lock_fd, action=None):
    """Remember sessions that may create progress under a newly selected account."""
    parent = _private(runtime)
    try:
        cloud_import._lease(parent, lock_fd)
        if action == "set":
            name = ".offline-" + uuid.uuid4().hex
            cloud_storage._write(parent, name, _OFFLINE_BODY)
            try:
                os.replace(name, _OFFLINE, src_dir_fd=parent, dst_dir_fd=parent)
                os.fsync(parent)
            finally:
                try: os.unlink(name, dir_fd=parent)
                except FileNotFoundError: pass
            return True
        try:
            fd = os.open(_OFFLINE, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        except FileNotFoundError:
            return False
        try:
            info = os.fstat(fd)
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_nlink != 1 or info.st_mode & 0o077
                    or os.read(fd, 128) != _OFFLINE_BODY):
                raise Attention("local_storage")
            return True
        finally:
            os.close(fd)
    finally:
        os.close(parent)


def _enable_local(runtime, lock_fd):
    parent = _private(runtime)
    folder = None
    try:
        cloud_import._lease(parent, lock_fd)
        try: os.mkdir("local-saves", 0o700, dir_fd=parent)
        except FileExistsError: pass
        folder = os.open("local-saves", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        info = os.fstat(folder)
        if info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise Attention("local_storage")
        try:
            fd = os.open("local-saves.enabled", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        except FileNotFoundError:
            cloud_storage._write(parent, "local-saves.enabled", b"enabled\n")
        else:
            try:
                info = os.fstat(fd)
                if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
                    raise Attention("local_storage")
            finally: os.close(fd)
        os.fsync(parent)
    finally:
        if folder is not None: os.close(folder)
        os.close(parent)


class CloudAutomation:
    def __init__(self, manager):
        self.manager, self.launcher = manager, manager.launcher
        self.lock = threading.RLock()
        self.worker = None
        self.cancel = threading.Event()
        self.runtime = None
        self.closed = False
        self.request_id = None
        self.phase = None
        self.state = "idle"
        self.message = "Cloud-Spielstände werden vor dem Start und nach dem Beenden automatisch abgeglichen."
        self.error_code = None
        self.review = None
        self.review_deadline = 0
        self.last_synced_at = None
        self.scope_binding = None
        self.timings = {}

    def available(self):
        launcher = self.launcher
        try:
            return bool(launcher.runtime and self.manager._api().available(
                launcher.runtime, launcher.setup.source_root, verify=False, write=True))
        except (ImportError, OSError, ValueError):
            return False

    def snapshot(self):
        with self.launcher.lock:
            runtime = self.launcher.runtime
            enabled = self.available()
            with self.lock:
                current = self.runtime == runtime
                state = self.state if current else "idle"
                active = bool(current and self.worker and self.worker.is_alive())
                attention = enabled and current and state == "attention" and not active and not self.closed
                return {"enabled": enabled, "state": state,
                    "phase": self.phase if current else None,
                    "message": self.message if current else "Cloud-Spielstände werden vor dem Start und nach dem Beenden automatisch abgeglichen.",
                    "error_code": self.error_code if current else None,
                    "can_retry": attention,
                    "can_play_local": attention and self.phase == "before_start" and self.error_code != "unsafe_session",
                    "can_cancel": active and state == "syncing" and self.phase == "before_start",
                    "request_id": self.request_id if current else None,
                    "last_synced_at": self.last_synced_at if current else None,
                    "timings": dict(self.timings) if current else {},
                    "conflict": bool(attention and self.review is not None),
                    "summary": self.review.summary() if attention and self.review is not None else None}

    def launch(self):
        with self.launcher.lock:
            if not self.available():
                return None
            return self._start("before_start")

    def action(self, action, request_id, choice=None):
        with self.launcher.lock:
            with self.lock:
                status = self.snapshot()
                if not request_id or request_id != status["request_id"]:
                    raise LauncherError("Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.")
                if action == "cancel-auto" and status["can_cancel"]:
                    self.cancel.set()
                    return {"ok": True}
                if not status["can_retry"]:
                    raise LauncherError("Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.")
                phase = self.phase
                review = None
                if action == "resolve":
                    if (choice not in {"cloud", "local"} or self.review is None
                            or time.monotonic() >= self.review_deadline):
                        raise LauncherError("Der Spielstandvergleich ist abgelaufen. Bitte erneut versuchen.")
                    review = self.review
                elif action == "play-local":
                    if not status["can_play_local"]:
                        raise LauncherError("Diese lokale Sitzung kann nicht gestartet werden.")
                elif action != "retry":
                    raise LauncherError("Diese Cloud-Aktion ist nicht verfügbar.")
            return self._start(phase, local_only=action == "play-local", review=review, choice=choice)

    def _start(self, phase, *, local_only=False, review=None, choice=None):
        launcher = self.launcher
        launcher.require_open()
        with self.lock:
            if self.closed or (self.worker and self.worker.is_alive()):
                raise LauncherError("Eine Cloud-Abfrage läuft bereits.")
            if not launcher.status()["runtime"]["ready"]:
                raise LauncherError("Die Runtime ist nicht startbereit oder das Spiel läuft bereits.")
            launcher.reserve_setup()
            runtime = launcher.runtime
            expected_binding = self.scope_binding if phase == "after_exit" and runtime == self.runtime else None
            if phase == "before_start": self.scope_binding = None
            session_id = uuid.uuid4().hex
            launcher.managed_session = session_id
            self.runtime, self.request_id, self.phase = runtime, session_id, phase
            self.timings = {}
            self.state, self.error_code, self.review = "syncing", None, None
            self.message = MESSAGES["before" if phase == "before_start" else "after"]
            self.cancel = threading.Event()
            self.worker = threading.Thread(target=self._run,
                args=(runtime, session_id, phase, local_only, review, choice, expected_binding),
                name="flightdeck-cloud-session", daemon=True)
            try: self.worker.start()
            except BaseException:
                launcher.managed_session = None
                launcher.release_setup()
                self.state = "attention"
                raise
            return {"ok": True, "cloud_sync": True, "request_id": session_id}

    def _set(self, state, message, **values):
        with self.lock:
            self.state, self.message = state, message
            for name, value in values.items(): setattr(self, name, value)

    def _check(self):
        if self.cancel.is_set(): raise Attention("cancelled")

    def _elapsed(self, stage, started):
        # In-memory numeric diagnostics only: no account, save name or payload.
        with self.lock:
            self.timings[stage] = round(time.monotonic() - started, 3)

    @contextmanager
    def _timed(self, stage):
        started = time.monotonic()
        try:
            yield
        finally:
            self._elapsed(stage, started)

    @contextmanager
    def _client(self, api, runtime, phase):
        opened, started = False, time.monotonic()
        closing = started
        try:
            with api.open_client(runtime, self.launcher.setup.source_root, cancel=self.cancel) as client:
                opened = True
                self._elapsed(phase + "_connect_seconds", started)
                try:
                    yield client
                finally:
                    closing = time.monotonic()
        finally:
            self._elapsed(phase + ("_cleanup_seconds" if opened else "_connect_seconds"),
                          closing if opened else started)

    def _download(self, runtime, client):
        snapshot = cloud_cache.download(runtime, client, cancel=self.cancel)
        remote = cloud_import.read_snapshot(runtime, client.scope, snapshot)
        baseline = cloud_import.load_baseline(runtime, client.scope)
        plan = cloud_import.prepare(runtime, client.scope, snapshot, baseline=baseline)
        return snapshot, remote, baseline, plan

    def _readback(self, runtime, client):
        def read(*, timeout, container_names=None):
            limited = cloud_storage.CloudStorageClient(client.scope, client.transport,
                limits=replace(client.limits, deadline_seconds=min(timeout, client.limits.deadline_seconds)))
            if container_names is not None:
                snapshot = cloud_cache.download(runtime, limited, cancel=self.cancel,
                                                force_containers=container_names)
                return cloud_import.read_snapshot(runtime, client.scope, snapshot)
            snapshot = limited.download_snapshot(_snapshot_directory(runtime), cancel=self.cancel)
            remote = cloud_import.read_snapshot(runtime, client.scope, snapshot)
            # Readback proves remote writes with actual downloaded bytes. Only
            # then may those bytes accelerate a future ordinary comparison.
            try:
                cloud_cache.remember(runtime, client.scope, snapshot)
            except (OSError, ValueError, TypeError, KeyError,
                    cloud_import.CloudImportError, cloud_storage.CloudStorageError):
                # The independently validated State above is already the
                # readback proof; a failed cache reread must not discard it.
                pass
            return remote
        return read

    def _finish_transfer(self, runtime, client, lock_fd, record, expected, *, playing):
        with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd, cancel=self.cancel) as local:
            record = cloud_session.update(runtime, client.scope, record, phase="uploading", export=local)
            readback = self._readback(runtime, client)
            receipt = cloud_write.upload(client.scope, client.transport,
                lambda *, timeout: readback(timeout=timeout, container_names=frozenset()), local.state,
                expected_remote_digest=expected, assert_local_unchanged=local.assert_unchanged, cancel=self.cancel,
                read_committed=readback)
            # Leave the journal in place if durable baseline bookkeeping fails.
            cloud_import.record_common(local, receipt)
            if not receipt.lease_released:
                raise Attention("lease_lost")
            if playing:
                record = cloud_session.checkpoint(runtime, client.scope, record, export=local, receipt=receipt, phase="playing")
            else:
                cloud_session.complete(runtime, client.scope, record, export=local, receipt=receipt)
                record = None
        # Retain the runtime-wide local-session guard: a successful account B
        # transfer must not remove protection for account A's unsynced progress.
        # This account's verified baseline makes the marker harmless for it.
        return record

    def _before(self, runtime, client, lock_fd, review=None, choice=None):
        snapshot, remote, baseline, plan = self._download(runtime, client)
        record = cloud_session.load(runtime, client.scope)
        with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd, cancel=self.cancel) as export:
            local = export.state
            local_digest = export.content_digest
        if review is not None:
            if (review.scope_binding != plan.scope_binding or review.remote_digest != plan.remote_digest
                    or review.local_digest != plan.local_digest):
                raise Attention("conflict", plan)
            if choice == "cloud":
                imported = cloud_import.apply(runtime, client.scope, plan, choice="cloud", runtime_lock_fd=lock_fd, cancel=self.cancel)
                if not imported.durability_confirmed: raise Attention("local_storage")
            with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd, cancel=self.cancel) as export:
                if record is None:
                    record = cloud_session.begin(runtime, client.scope, snapshot=snapshot, export=export)
            return self._finish_transfer(runtime, client, lock_fd, record, plan.remote_digest, playing=True)
        if record is not None:
            # Read the server again; a pending transaction is never replayed.
            if plan.remote_digest not in {record["before_remote_digest"], record["target_digest"]}:
                raise Attention("conflict", plan)
            if record["phase"] != "playing" and local_digest != record["target_digest"]:
                raise Attention("conflict", plan)
            if plan.remote_digest == record["target_digest"] and plan.remote_digest != record["before_remote_digest"]:
                if local_digest != record["target_digest"]:
                    raise Attention("conflict", plan)
                return self._finish_transfer(runtime, client, lock_fd, record, plan.remote_digest, playing=True)
            with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd, cancel=self.cancel) as export:
                return cloud_session.update(runtime, client.scope, record, phase="playing", export=export)
        if (_offline(runtime, lock_fd) and baseline is None and local.containers
                and local_digest != plan.remote_digest and remote.containers):
            raise Attention("conflict", plan)
        binding = client.scope.binding
        policy = cloud_policy.decide(scope_binding=binding,
            local=cloud_policy.SaveSet.from_state(binding, local),
            remote=cloud_policy.SaveSet.from_state(binding, remote),
            baseline=cloud_policy.SaveSet(binding, baseline.containers) if baseline is not None else None)
        if policy.action in {"conflict", "blocked"}:
            raise Attention("conflict", plan)
        if policy.action in {"import_cloud", "merge"}:
            # The first cloud-first import is deliberate; later merges preserve
            # independently changed containers and never select by timestamp.
            imported = cloud_import.apply(runtime, client.scope, plan,
                choice="cloud" if policy.action == "import_cloud" else {},
                runtime_lock_fd=lock_fd, cancel=self.cancel)
            if not imported.durability_confirmed: raise Attention("local_storage")
        with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd, cancel=self.cancel) as export:
            if cloud_policy.SaveSet.from_state(binding, export.state) != policy.target:
                raise Attention("changed")
            record = cloud_session.begin(runtime, client.scope, snapshot=snapshot, export=export)
            return cloud_session.update(runtime, client.scope, record, phase="playing", export=export)

    def _after(self, runtime, client, lock_fd, expected_binding=None, review=None, choice=None):
        if expected_binding is not None and client.scope.binding != expected_binding:
            raise Attention("invalid_scope")
        snapshot, remote, baseline, plan = self._download(runtime, client)
        record = cloud_session.load(runtime, client.scope)
        if record is None:
            # A retry cannot invent ownership of another account's session.
            raise Attention("invalid_scope")
        with cloud_import.export_local(runtime, client.scope, runtime_lock_fd=lock_fd, cancel=self.cancel) as export:
            local_digest = export.content_digest
        if review is not None:
            if (review.scope_binding != plan.scope_binding or review.remote_digest != plan.remote_digest
                    or review.local_digest != plan.local_digest):
                raise Attention("conflict", plan)
            if choice == "cloud":
                imported = cloud_import.apply(runtime, client.scope, plan, choice="cloud", runtime_lock_fd=lock_fd, cancel=self.cancel)
                if not imported.durability_confirmed: raise Attention("local_storage")
        elif plan.remote_digest != record["before_remote_digest"] and plan.remote_digest != local_digest:
            raise Attention("conflict", plan)
        self._finish_transfer(runtime, client, lock_fd, record, plan.remote_digest, playing=False)

    def _run(self, runtime, session_id, phase, local_only, review, choice, expected_binding=None):
        launcher = self.launcher
        active_phase = phase
        transferred = False
        phase_started = time.monotonic()
        try:
            api = self.manager._api()
            with launcher.runtime_lock(operation="cloud_session") as lock_fd:
                cloud_process_guard.check(runtime, lock_fd)
                _enable_local(runtime, lock_fd)
                binding = expected_binding
                if phase == "before_start":
                    with self._timed("before_backup_seconds"):
                        launcher._backup_reserved(lock_fd)
                    if not local_only:
                        with self._client(api, runtime, "before") as client:
                            with self._timed("before_compare_seconds"):
                                self._before(runtime, client, lock_fd, review, choice)
                            binding = client.scope.binding
                            with self.lock: self.scope_binding = binding
                    self._check()
                    # The game can switch accounts after our preflight. Keep
                    # first-cloud preference from hiding that other account's
                    # newly created progress on a later launch.
                    if not _offline(runtime, lock_fd):
                        _offline(runtime, lock_fd, "set")
                    self._set("playing", MESSAGES["local" if local_only else "playing"])
                    with launcher.lock:
                        launcher.require_open()
                        if launcher.managed_session != session_id or launcher.runtime != runtime:
                            raise Attention("changed")
                        cloud_process_guard.mark(runtime, lock_fd)
                        try:
                            process = launcher._spawn_reserved(lock_fd)
                        except Exception:
                            if launcher.process is None:
                                cloud_process_guard.clear(runtime, lock_fd)
                            raise
                    self._elapsed("before_total_seconds", phase_started)
                    code = process.wait()
                    phase_started = time.monotonic()
                    with launcher.lock:
                        if launcher.process is not process or launcher.managed_session != session_id:
                            raise Attention("changed")
                        launcher.exit_code, launcher.process, launcher.stopping = code, None, False
                    active_phase = "after_exit"
                    if code < 0:
                        raise Attention("unsafe_session")
                    cloud_process_guard.clear(runtime, lock_fd)
                    self._set("syncing", MESSAGES["after"], phase=active_phase)
                    with self._timed("after_backup_seconds"):
                        launcher._backup_reserved(lock_fd)
                    if local_only:
                        self._set("local", MESSAGES["local"], phase=None)
                        return
                    review = choice = None
                self._check()
                with self._client(api, runtime, "after") as client:
                    with self._timed("after_transfer_seconds"):
                        self._after(runtime, client, lock_fd, binding, review, choice)
                    transferred = True
                self._set("synced", MESSAGES["synced"], phase=None, last_synced_at=now())
        except Exception as error:
            if transferred:
                self._set("synced", MESSAGES["cleanup"], phase=None, last_synced_at=now())
            else:
                code = getattr(error, "code", "failed")
                plan = error.plan if isinstance(error, Attention) else None
                if self.cancel.is_set(): code = "cancelled"
                if code not in {"conflict", "authentication", "auth_required", "unauthorized", "forbidden", "cancelled", "invalid_scope", "local_storage", "lease_lost", "changed", "unsafe_session"}:
                    code = "failed"
                key = "unsafe_session" if code == "unsafe_session" else "conflict" if plan is not None else "authentication" if code in {"authentication", "auth_required", "unauthorized", "forbidden"} else "cancelled" if code == "cancelled" and active_phase == "before_start" else "failed_before" if active_phase == "before_start" else "failed_after"
                self._set("idle" if code == "cancelled" and active_phase == "before_start" else "attention",
                    MESSAGES[key], phase=active_phase, error_code=code, review=plan,
                    review_deadline=time.monotonic() + 15 * 60)
        finally:
            key = "before_total_seconds" if active_phase == "before_start" else "after_total_seconds"
            if key not in self.timings:
                self._elapsed(key, phase_started)
            with launcher.lock:
                if launcher.managed_session == session_id:
                    launcher.managed_session = None
                    launcher.release_setup()

    def close(self):
        with self.lock:
            self.closed = True
            self.cancel.set()
            worker = self.worker
        if worker and worker is not threading.current_thread(): worker.join(timeout=5)
