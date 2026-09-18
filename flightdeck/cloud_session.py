# SPDX-License-Identifier: MIT
"""Private restart journal for automatic saves; no network or game mutation.

Journal phases are intentions, never evidence that an HTTP operation completed.
Only a fresh writer receipt can checkpoint common content or finish a session.
The coordinator owns play.lock; active exports retain their native writer lock.
"""
from contextlib import contextmanager
import copy
import json
import os
from pathlib import Path
import re
import threading
import uuid

from . import cloud_import as ci, cloud_write, save_state
from .cloud_storage import Snapshot, _json, _rename_noreplace

_LOCK = threading.RLock()
_PHASES = {"prepared", "playing", "uploading", "pending"}
_KEYS = {"schema", "scope_binding", "namespace", "session_id", "revision", "phase",
         "snapshot", "original_remote_digest", "before_remote_digest", "pre_local_digest",
         "pre_local_sha256", "initial_backup_id", "target_digest", "target_sha256",
         "target_local_sha256", "target_backup_id", "generation", "confirmed_backup_id"}


class CloudSessionError(Exception):
    def __init__(self, code):
        self.code = code
        super().__init__({"invalid_session": "The saved sync session could not be verified.",
                          "invalid_scope": "The saved sync session belongs to a different scope.",
                          "invalid_receipt": "The sync result has not been verified.",
                          "changed": "The saved sync session changed.",
                          "busy": "A saved sync session already needs recovery.",
                          "local_storage": "The private sync journal could not be saved safely."}[code])


def _require(value, code="invalid_session"):
    if not value:
        raise CloudSessionError(code)


def _hex(value, size=64):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{%d}" % size, value) is not None


@contextmanager
def _access(runtime, scope, *, export=None, runtime_lock_fd=None, mutate=False):
    """Never open writer.lock, especially while an export already owns it."""
    owned = []
    with _LOCK:
        try:
            binding, namespace = ci._scope(scope)
            if export is not None:
                _require(isinstance(export, ci.LocalExport) and export._runtime == str(Path(runtime))
                         and export.scope_binding == binding and export.namespace == namespace, "invalid_scope")
                export.assert_unchanged()
                root, private = export._root, export._private
            else:
                root = ci._open_path(runtime, private=False); owned.append(root)
                private = ci._child(root, "private"); owned.append(private)
                if mutate:
                    ci._lease(private, runtime_lock_fd)
            try:
                folder = ci._child(private, "cloud-sessions", create=mutate)
                owned.append(folder)
            except FileNotFoundError:
                folder = None
            yield binding, namespace, private, folder
            ci._linked(root, "private", private)
            if folder is not None:
                ci._linked(private, "cloud-sessions", folder)
            if export is not None:
                export.assert_unchanged()
        except CloudSessionError:
            raise
        except Exception:
            raise CloudSessionError("local_storage") from None
        finally:
            for fd in reversed(owned):
                os.close(fd)


def _backup(private, binding, namespace, backup_id):
    _require(isinstance(backup_id, str) and re.fullmatch(r"backup-[0-9a-f]{32}", backup_id))
    return ci._backup_data(private, binding, namespace, backup_id)


def _validated(runtime, scope, private, value):
    binding, namespace = ci._scope(scope)
    _require(isinstance(value, dict) and set(value) == _KEYS)
    _require(type(value["schema"]) is int and value["schema"] == 1
             and value["scope_binding"] == binding and value["namespace"] == namespace)
    _require(_hex(value["session_id"], 32) and type(value["revision"]) is int
             and 0 <= value["revision"] < 2**63 and value["phase"] in _PHASES)
    for key in ("original_remote_digest", "before_remote_digest", "pre_local_digest",
                "target_digest", "target_sha256"):
        _require(_hex(value[key]))
    for key in ("pre_local_sha256", "target_local_sha256"):
        _require(value[key] == "missing" or _hex(value[key]))
    _require(type(value["generation"]) is int and 0 <= value["generation"] < 2**64)
    reference = value["snapshot"]
    _require(isinstance(reference, dict) and set(reference) ==
             {"id", "manifest_sha256", "container_count", "blob_count", "total_bytes"})
    _require(isinstance(reference["id"], str) and re.fullmatch(r"snapshot-[0-9a-f]{32}", reference["id"])
             and _hex(reference["manifest_sha256"]))
    snapshot = Snapshot(Path(runtime) / "private/cloud-saves" / reference["id"], binding,
                        reference["container_count"], reference["blob_count"], reference["total_bytes"])
    remote, snapshot_hash = ci._snapshot(runtime, scope, snapshot)
    _require(snapshot_hash == reference["manifest_sha256"]
             and save_state.content_digest(remote) == value["original_remote_digest"])
    initial, raw, _ = _backup(private, binding, namespace, value["initial_backup_id"])
    initial_state = save_state.decode(raw) if raw is not None else save_state.State(0)
    _require((ci._hash(raw) if raw is not None else "missing") == value["pre_local_sha256"]
             and save_state.content_digest(initial_state) == value["pre_local_digest"])
    metadata, _, encoded = _backup(private, binding, namespace, value["target_backup_id"])
    target = save_state.decode(encoded)
    _require(ci._hash(encoded) == value["target_sha256"] and target.generation == value["generation"]
             and save_state.content_digest(target) == value["target_digest"]
             and (metadata["state_sha256"] if metadata["existed"] else "missing") == value["target_local_sha256"])
    if value["confirmed_backup_id"] is None:
        _require(value["before_remote_digest"] == value["original_remote_digest"])
    else:
        _, _, confirmed = _backup(private, binding, namespace, value["confirmed_backup_id"])
        _require(save_state.content_digest(save_state.decode(confirmed)) == value["before_remote_digest"])
    return value


def _load(runtime, scope, private, folder, namespace):
    if folder is None:
        return None
    raw = ci._read(folder, namespace + ".json", 32768, missing=True)
    return None if raw is None else _validated(runtime, scope, private, _json(raw))


def load(runtime, scope):
    """Return verified private metadata; absence differs from corrupt data.

    Read-only and usable while the game owns writer.lock. Never expose the
    returned record directly in the public API: it contains private digests.
    """
    with _access(runtime, scope) as (_, namespace, private, folder):
        return _load(runtime, scope, private, folder, namespace)


def _store(private, folder, namespace, value, *, new=False):
    payload = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    _require(len(payload) <= 32768)
    temporary = ".session-" + uuid.uuid4().hex
    try:
        ci._write(folder, temporary, payload)
        ci._linked(private, "cloud-sessions", folder)
        if new:
            _rename_noreplace(folder, temporary, namespace + ".json")
        else:
            os.replace(temporary, namespace + ".json", src_dir_fd=folder, dst_dir_fd=folder)
        os.fsync(folder)
    finally:
        try:
            os.unlink(temporary, dir_fd=folder)
        except FileNotFoundError:
            pass


def _capture(export):
    export.assert_unchanged()
    identity = ci._BackupIdentity(export.scope_binding, export.namespace, export.generation)
    backup = ci._backup(export._private, identity,
                        None if export.local_digest == "missing" else export.encoded, export.encoded)
    export.assert_unchanged()
    return {"target_backup_id": backup, "target_sha256": ci._hash(export.encoded),
            "target_local_sha256": export.local_digest, "target_digest": export.content_digest,
            "generation": export.generation}


def begin(runtime, scope, *, snapshot, export, phase="prepared"):
    """Durably back up the current target before any upload; never replace pending work."""
    _require(phase in {"prepared", "playing"})
    with _access(runtime, scope, export=export, mutate=True) as (binding, namespace, private, folder):
        _require(_load(runtime, scope, private, folder, namespace) is None, "busy")
        remote, snapshot_hash = ci._snapshot(runtime, scope, snapshot)
        target = _capture(export)
        value = {"schema": 1, "scope_binding": binding, "namespace": namespace,
                 "session_id": uuid.uuid4().hex, "revision": 0, "phase": phase,
                 "snapshot": {"id": snapshot.path.name, "manifest_sha256": snapshot_hash,
                              "container_count": snapshot.container_count, "blob_count": snapshot.blob_count,
                              "total_bytes": snapshot.total_bytes},
                 "original_remote_digest": save_state.content_digest(remote),
                 "before_remote_digest": save_state.content_digest(remote),
                 "pre_local_digest": export.content_digest, "pre_local_sha256": export.local_digest,
                 "initial_backup_id": target["target_backup_id"], "confirmed_backup_id": None, **target}
        _store(private, folder, namespace, value, new=True)
        return copy.deepcopy(value)


def _current(runtime, scope, private, folder, namespace, record):
    current = _load(runtime, scope, private, folder, namespace)
    _require(current is not None and current == record, "changed")
    _require(current["revision"] < 2**63 - 1)
    return copy.deepcopy(current)


def update(runtime, scope, record, *, phase, runtime_lock_fd=None, export=None):
    """Persist intention; uploading always requires a freshly backed-up export."""
    _require(phase in _PHASES and (phase != "uploading" or export is not None))
    with _access(runtime, scope, export=export, runtime_lock_fd=runtime_lock_fd, mutate=True) as (_, namespace, private, folder):
        value = _current(runtime, scope, private, folder, namespace, record)
        if export is not None:
            value.update(_capture(export))
        value.update(phase=phase, revision=value["revision"] + 1)
        _store(private, folder, namespace, value)
        return copy.deepcopy(value)


def _confirmed(value, export, receipt):
    _require(export.content_digest == value["target_digest"]
             and export.local_digest == value["target_local_sha256"]
             and ci._hash(export.encoded) == value["target_sha256"], "changed")
    _require(cloud_write.verify_receipt(receipt, scope_binding=export.scope_binding,
                                       source_content_digest=value["target_digest"]), "invalid_receipt")


def checkpoint(runtime, scope, record, *, export, receipt, phase="playing"):
    """Keep the session after verified pre-game sync; advance only its confirmed base."""
    _require(phase == "playing")
    with _access(runtime, scope, export=export, mutate=True) as (_, namespace, private, folder):
        value = _current(runtime, scope, private, folder, namespace, record)
        _confirmed(value, export, receipt)
        value.update(before_remote_digest=value["target_digest"],
                     confirmed_backup_id=value["target_backup_id"], phase=phase,
                     revision=value["revision"] + 1)
        _store(private, folder, namespace, value)
        return copy.deepcopy(value)


def complete(runtime, scope, record, *, export, receipt):
    """Remove only the exact verified journal, retaining all immutable backups."""
    with _access(runtime, scope, export=export, mutate=True) as (_, namespace, private, folder):
        value = _current(runtime, scope, private, folder, namespace, record)
        _confirmed(value, export, receipt)
        ci._linked(private, "cloud-sessions", folder)
        os.unlink(namespace + ".json", dir_fd=folder)
        os.fsync(folder)
