# SPDX-License-Identifier: MIT
"""Explicit, account-bound local import of verified ConnectedStorage archives.

The caller owns the Launcher reservation and runtime lease. This module also
takes the native provider's byte-range lock and never accesses another save
namespace. It makes no cloud request, acquires no cloud lease and never uploads.
An observed common-content baseline is not a claim of locked cloud consistency.
"""
from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass, field, replace
import fcntl
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import stat
import threading
import uuid

from . import save_state
from .cloud_storage import CloudStorageError, Scope, Snapshot, _json, _rename_noreplace

_SEAL_KEY = os.urandom(32)
_MUTEX = threading.RLock()  # POSIX record locks alone do not exclude threads.
_ACTIVE_EXPORTS = set()
_ACTIVE_NAMESPACES = set()
_MAX_STATE = save_state.QUOTA + save_state.METADATA_LIMIT + 32
_ERRORS = {name: text for name, text in (
    ("invalid_scope", "The cloud-save account or title does not match."),
    ("invalid_snapshot", "The cloud-save archive could not be verified."),
    ("invalid_plan", "This cloud-save import plan is no longer valid."),
    ("changed", "Save data changed. Prepare a new import preview."),
    ("busy", "The native save provider is still in use."),
    ("invalid_lock", "An exclusive launcher runtime lease is required."),
    ("conflict", "Conflicting saves require an explicit choice."),
    ("unsupported", "This local save configuration or format is not supported."),
    ("local_storage", "The save backup or local import could not be completed safely."),
    ("cancelled", "The local save import was cancelled."),
)}


class CloudImportError(Exception):
    def __init__(self, code):
        self.code = code
        super().__init__(_ERRORS[code])


def _fail(code):
    raise CloudImportError(code)


def _hash(data):
    return hashlib.sha256(data).hexdigest()


def _seal(value):
    return hmac.new(_SEAL_KEY, json.dumps(value, ensure_ascii=True, sort_keys=True,
                    separators=(",", ":")).encode(), hashlib.sha256).hexdigest()


def _cancel(cancel):
    if cancel is not None and cancel.is_set():
        _fail("cancelled")


@dataclass(frozen=True, repr=False)
class Baseline:
    """Verified historical common content, sealed while held in this process.

    No arbitrary archive can claim to be a baseline. A successful import that
    exactly matches its remote input creates a private durable receipt.
    ``load_baseline`` verifies that receipt and its immutable data before
    creating a new process-local seal. A plain cloud archive is insufficient.
    """
    scope_binding: str
    containers: tuple[tuple[str, str], ...]
    _seal_value: str = field(repr=False)


@dataclass(frozen=True, repr=False)
class Decision:
    name: str
    action: str
    local_digest: str | None
    remote_digest: str | None
    baseline_digest: str | None


@dataclass(frozen=True, repr=False)
class ImportPlan:
    scope_binding: str
    namespace: str
    local_digest: str
    remote_digest: str
    snapshot_digest: str
    generation: int
    decisions: tuple[Decision, ...]
    _runtime: str
    _snapshot: Snapshot
    _baseline: Baseline | None
    _counts: tuple[int, int, int, int]
    _seal_value: str = field(repr=False)

    def summary(self):
        containers, blobs, size, local_count = self._counts
        return {"container_count": containers, "blob_count": blobs, "total_bytes": size,
                "local_exists": self.local_digest != "missing", "local_container_count": local_count,
                "add_count": sum(d.local_digest is None and d.remote_digest is not None for d in self.decisions),
                "replace_count": sum(d.local_digest is not None and d.remote_digest is not None
                                     and d.local_digest != d.remote_digest for d in self.decisions),
                "delete_count": sum(d.local_digest is not None and d.remote_digest is None for d in self.decisions),
                "unchanged_count": sum(d.local_digest == d.remote_digest for d in self.decisions),
                "conflict_count": sum(d.action == "conflict" for d in self.decisions)}


@dataclass(frozen=True, repr=False)
class ImportResult:
    imported: bool
    backup_id: str | None
    generation: int
    container_count: int
    blob_count: int
    total_bytes: int
    durability_confirmed: bool
    baseline: Baseline | None = field(default=None, repr=False)

    def summary(self):
        return {key: getattr(self, key) for key in ("imported", "backup_id", "generation",
                "container_count", "blob_count", "total_bytes", "durability_confirmed")}


def _scope(scope):
    try:
        if not isinstance(scope, Scope):
            _fail("invalid_scope")
        verified = Scope(scope.xuid, scope.scid, scope.package_family_name, scope.title_id)
        return verified.binding, save_state.namespace_key(verified.title_id, verified.scid, int(verified.xuid))
    except (ValueError, TypeError, CloudStorageError):
        _fail("invalid_scope")


def _directory_info(fd, *, private=True):
    info = os.fstat(fd)
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()
            or private and stat.S_IMODE(info.st_mode) & 0o077):
        _fail("local_storage")
    return info


def _open_path(path, *, private=True):
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        _fail("local_storage")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for name in path.parts[1:]:
            new = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = new
        _directory_info(fd, private=private)
        return fd
    except BaseException:
        os.close(fd)
        raise


def _child(parent, name, *, create=False):
    if create:
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
            os.fsync(parent)
        except FileExistsError:
            pass
    fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
    try:
        _directory_info(fd)
        return fd
    except BaseException:
        os.close(fd)
        raise


def _file_info(fd):
    info = os.fstat(fd)
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) & 0o077 or info.st_nlink != 1):
        _fail("local_storage")
    return info


def _read(parent, name, maximum, *, missing=False):
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    except FileNotFoundError:
        if missing:
            return None
        raise
    with os.fdopen(fd, "rb") as stream:
        before = _file_info(stream.fileno())
        if before.st_size > maximum:
            _fail("unsupported")
        data = stream.read(maximum + 1)
        after = _file_info(stream.fileno())
        if (len(data) != before.st_size or len(data) > maximum
                or (before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                != (after.st_size, after.st_mtime_ns, after.st_ctime_ns)):
            _fail("changed")
        return data


def _linked(parent, name, fd):
    opened = os.fstat(fd)
    linked = os.stat(name, dir_fd=parent, follow_symlinks=False)
    if not stat.S_ISDIR(linked.st_mode) or (opened.st_dev, opened.st_ino) != (linked.st_dev, linked.st_ino):
        _fail("changed")


def _lease(private_fd, fd):
    if type(fd) is not int:
        _fail("invalid_lock")
    try:
        given = _file_info(fd)
        probe = os.open("play.lock", os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=private_fd)
        try:
            actual = _file_info(probe)
            if (given.st_dev, given.st_ino) != (actual.st_dev, actual.st_ino):
                _fail("invalid_lock")
            try:
                fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                pass
            else:
                fcntl.flock(probe, fcntl.LOCK_UN)
                _fail("invalid_lock")
            # Another process's lease is insufficient: this exact open file
            # description must own it. Never unlock the caller's descriptor.
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        finally:
            os.close(probe)
    except (OSError, CloudImportError):
        _fail("invalid_lock")


@contextmanager
def _local(runtime, namespace, *, create=False, lease=None):
    fds = []
    active_key = None
    with _MUTEX:
        try:
            root = _open_path(runtime, private=False); fds.append(root)
            private = _child(root, "private"); fds.append(private)
            if _read(private, "local-saves.enabled", 4096, missing=True) is None:
                _fail("unsupported")
            if create or lease is not None:
                _lease(private, lease)
            saves = _child(private, "local-saves"); fds.append(saves)
            identity = os.fstat(saves)
            key = identity.st_dev, identity.st_ino, namespace
            if key in _ACTIVE_NAMESPACES:
                _fail("busy")
            _ACTIVE_NAMESPACES.add(key)
            active_key = key
            try:
                folder = _child(saves, namespace, create=create); fds.append(folder)
            except FileNotFoundError:
                yield root, private, saves, None
                return
            lock = os.open("writer.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK,
                           0o600, dir_fd=folder)
            fds.append(lock)
            _file_info(lock)
            try:
                fcntl.lockf(lock, fcntl.LOCK_EX | fcntl.LOCK_NB, 1, 0, os.SEEK_SET)
            except BlockingIOError:
                _fail("busy")
            yield root, private, saves, folder
        finally:
            close_error = None
            for fd in reversed(fds):
                try:
                    os.close(fd)
                except OSError as error:
                    close_error = error
            if active_key is not None:
                _ACTIVE_NAMESPACES.discard(active_key)
            if close_error is not None:
                raise close_error


def _state(folder):
    raw = _read(folder, "state.bin", _MAX_STATE, missing=True) if folder is not None else None
    return raw, save_state.decode(raw) if raw is not None else save_state.State(0)


def _containers(state):
    return {name: save_state.content_digest(save_state.State(0, {name: value}))
            for name, value in state.containers.items()}


def _baseline(scope_binding, state):
    items = tuple(sorted(_containers(state).items()))
    return Baseline(scope_binding, items, _seal([scope_binding, items]))


def _baseline_items(baseline, binding):
    if baseline is None:
        return None
    if (not isinstance(baseline, Baseline) or baseline.scope_binding != binding
            or not hmac.compare_digest(baseline._seal_value, _seal([binding, baseline.containers]))):
        _fail("invalid_plan")
    return dict(baseline.containers)


def _decisions(local, remote, baseline):
    left, right = _containers(local), _containers(remote)
    result = []
    for name in sorted(left.keys() | right.keys() | (baseline.keys() if baseline is not None else set())):
        a, b, old = left.get(name), right.get(name), baseline.get(name) if baseline is not None else None
        if a == b:
            action = "unchanged"
        elif baseline is not None and a == old:
            action = "cloud"
        elif baseline is not None and b == old:
            action = "local"
        elif baseline is None and a is None:
            action = "cloud"
        else:
            action = "conflict"
        result.append(Decision(name, action, a, b, old))
    return tuple(result)


def _snapshot(runtime, scope, snapshot):
    binding, _ = _scope(scope)
    if (not isinstance(snapshot, Snapshot) or snapshot.scope_binding != binding
            or snapshot.consistency != "rechecked-unlocked"):
        _fail("invalid_scope")
    path = Path(snapshot.path)
    if (path.parent != Path(runtime) / "private/cloud-saves"
            or not re.fullmatch(r"snapshot-[0-9a-f]{32}", path.name)):
        _fail("invalid_snapshot")
    root = blobs_fd = None
    try:
        root = _open_path(path)
        encoded = _read(root, "manifest.json", save_state.METADATA_LIMIT)
        manifest = _json(encoded)
        if (set(manifest) != {"schema", "scope_binding", "consistency", "containers"}
                or type(manifest["schema"]) is not int or manifest["schema"] != 1
                or manifest["scope_binding"] != binding or manifest["consistency"] != "rechecked-unlocked"
                or not isinstance(manifest["containers"], list)
                or len(manifest["containers"]) > save_state.CONTAINER_LIMIT):
            _fail("invalid_snapshot")
        blobs_fd = _child(root, "blobs")
        state = save_state.State(0)
        count = size = 0
        for row in manifest["containers"]:
            if (not isinstance(row, dict) or set(row) != {"name", "display_name", "etag", "client_file_time", "size", "blobs"}
                    or not isinstance(row["name"], str) or not row["name"].endswith(",savedgame")
                    or not isinstance(row["etag"], str) or not 0 < len(row["etag"].encode()) <= 1024
                    or not isinstance(row["blobs"], list) or type(row["client_file_time"]) is not int
                    or not 0 <= row["client_file_time"] < 2**64
                    or type(row["size"]) is not int or not 0 <= row["size"] <= save_state.QUOTA):
                _fail("invalid_snapshot")
            name = row["name"][:-len(",savedgame")]
            save_state._name(name, container=True)
            if name in state.containers:
                _fail("invalid_snapshot")
            modified = row["client_file_time"] // 10_000_000 - 11644473600
            if modified < 0:
                _fail("unsupported")
            entry = save_state.Container(row["display_name"], modified)
            atoms = set()
            container_size = 0
            for blob in row["blobs"]:
                if (not isinstance(blob, dict) or set(blob) != {"name", "atom", "file", "size", "sha256"}
                        or type(blob["size"]) is not int or not 0 <= blob["size"] <= 64 * 1024 * 1024
                        or not isinstance(blob["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", blob["sha256"])
                        or blob["file"] != f"blobs/{count:08x}.bin"
                        or not isinstance(blob["atom"], str)
                        or not re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", blob["atom"])):
                    _fail("invalid_snapshot")
                save_state._name(blob["name"])
                if blob["name"] in entry.blobs or blob["atom"].lower() in atoms:
                    _fail("invalid_snapshot")
                count += 1
                size += blob["size"]
                if count > save_state.BLOB_LIMIT or size > save_state.QUOTA:
                    _fail("unsupported")
                payload = _read(blobs_fd, Path(blob["file"]).name, blob["size"])
                if len(payload) != blob["size"] or not hmac.compare_digest(_hash(payload), blob["sha256"]):
                    _fail("invalid_snapshot")
                entry.blobs[blob["name"]] = payload
                atoms.add(blob["atom"].lower())
                container_size += len(payload)
            if container_size != row["size"]:
                _fail("invalid_snapshot")
            state.containers[name] = entry
        if ((snapshot.container_count, snapshot.blob_count, snapshot.total_bytes)
                != (len(state.containers), count, size)
                or any(type(n) is not int for n in (snapshot.container_count, snapshot.blob_count, snapshot.total_bytes))):
            _fail("invalid_snapshot")
        if set(os.listdir(root)) != {"manifest.json", "blobs"} or set(os.listdir(blobs_fd)) != {f"{i:08x}.bin" for i in range(count)}:
            _fail("invalid_snapshot")
        save_state.encode(state)  # Check all native format/metadata bounds too.
        return state, _hash(encoded)
    except CloudImportError:
        raise
    except Exception:
        _fail("invalid_snapshot")
    finally:
        for fd in (blobs_fd, root):
            if fd is not None:
                os.close(fd)


def _plan_data(plan):
    s = plan._snapshot
    return [plan.scope_binding, plan.namespace, plan.local_digest, plan.remote_digest,
            plan.snapshot_digest, plan.generation, plan._runtime, plan._counts,
            [str(s.path), s.scope_binding, s.container_count, s.blob_count, s.total_bytes, s.consistency],
            [[d.name, d.action, d.local_digest, d.remote_digest, d.baseline_digest] for d in plan.decisions],
            None if plan._baseline is None else [plan._baseline.scope_binding, plan._baseline.containers, plan._baseline._seal_value]]


def prepare(runtime, scope, snapshot, *, baseline=None):
    """Validate exact inputs and plan per-container changes without importing."""
    try:
        runtime = Path(runtime)
        binding, namespace = _scope(scope)
        old = _baseline_items(baseline, binding)
        remote, snapshot_digest = _snapshot(runtime, scope, snapshot)
        with _local(runtime, namespace) as (_, _, _, folder):
            raw, local = _state(folder)
        plan = ImportPlan(binding, namespace, _hash(raw) if raw is not None else "missing",
                          save_state.content_digest(remote), snapshot_digest, local.generation,
                          _decisions(local, remote, old), str(runtime), snapshot, baseline,
                          (snapshot.container_count, snapshot.blob_count, snapshot.total_bytes, len(local.containers)), "")
        return ImportPlan(**{**plan.__dict__, "_seal_value": _seal(_plan_data(plan))})
    except CloudImportError:
        raise
    except save_state.SaveStateError:
        _fail("unsupported")
    except (OSError, ValueError, TypeError):
        _fail("local_storage")


def read_snapshot(runtime, scope, snapshot):
    """Return validated remote content without opening any local native lock."""
    return _snapshot(runtime, scope, snapshot)[0]


def _write(parent, name, payload, *, mode=0o600):
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode, dir_fd=parent)
    with os.fdopen(fd, "wb") as output:
        output.write(payload)
        output.flush()
        os.fsync(output.fileno())


def _backup(private, plan, raw, replacement):
    parent = _child(private, "cloud-import-backups", create=True)
    name, temporary = "backup-" + uuid.uuid4().hex, ".backup-" + uuid.uuid4().hex
    folder = None
    committed = False
    try:
        os.mkdir(temporary, 0o700, dir_fd=parent)
        folder = _child(parent, temporary)
        metadata = {"schema": 1, "scope_binding": plan.scope_binding, "namespace": plan.namespace,
                    "existed": raw is not None, "state_sha256": _hash(raw) if raw is not None else None,
                    "generation": plan.generation, "replacement_sha256": _hash(replacement),
                    "snapshot_sha256": plan.snapshot_digest}
        if raw is not None:
            _write(folder, "state.bin", raw, mode=0o400)
        _write(folder, "imported.bin", replacement, mode=0o400)
        _write(folder, "manifest.json", (json.dumps(metadata, sort_keys=True) + "\n").encode(), mode=0o400)
        os.fchmod(folder, 0o500)
        os.fsync(folder)
        _rename_noreplace(parent, temporary, name)
        committed = True
        os.fsync(parent)
        return name
    finally:
        if folder is not None:
            if not committed:
                os.fchmod(folder, 0o700)
                for child in ("state.bin", "imported.bin", "manifest.json"):
                    try:
                        os.unlink(child, dir_fd=folder)
                    except FileNotFoundError:
                        pass
            os.close(folder)
        if not committed:
            try:
                os.rmdir(temporary, dir_fd=parent)
            except FileNotFoundError:
                pass
        os.close(parent)


def _select(local, remote, plan, choice):
    if choice == "cloud":
        return save_state.State(local.generation, dict(remote.containers))
    if choice == "local":
        return local
    if choice is not None and not isinstance(choice, dict):
        _fail("conflict")
    choices = {} if choice is None else choice
    conflicts = {d.name for d in plan.decisions if d.action == "conflict"}
    if set(choices) != conflicts or any(value not in {"cloud", "local"} for value in choices.values()):
        _fail("conflict")
    output = {}
    for decision in plan.decisions:
        side = choices.get(decision.name, decision.action)
        entry = remote.containers.get(decision.name) if side == "cloud" else local.containers.get(decision.name)
        if entry is not None:
            output[decision.name] = entry
    return save_state.State(local.generation, output)


def _backup_data(private, binding, namespace, backup_id):
    if not isinstance(backup_id, str) or not re.fullmatch(r"backup-[0-9a-f]{32}", backup_id):
        _fail("invalid_plan")
    parent = folder = None
    try:
        parent = _child(private, "cloud-import-backups")
        folder = _child(parent, backup_id)
        metadata = _json(_read(folder, "manifest.json", 16384))
        if (set(metadata) != {"schema", "scope_binding", "namespace", "existed", "state_sha256",
                              "generation", "replacement_sha256", "snapshot_sha256"}
                or type(metadata["schema"]) is not int or metadata["schema"] != 1
                or metadata["scope_binding"] != binding or metadata["namespace"] != namespace
                or type(metadata["existed"]) is not bool
                or type(metadata["generation"]) is not int or not 0 <= metadata["generation"] < 2**64):
            _fail("invalid_plan")
        files = {"manifest.json", "imported.bin"} | ({"state.bin"} if metadata["existed"] else set())
        if set(os.listdir(folder)) != files:
            _fail("invalid_plan")
        original = _read(folder, "state.bin", _MAX_STATE) if metadata["existed"] else None
        imported = _read(folder, "imported.bin", _MAX_STATE)
        if ((_hash(original) if original is not None else None) != metadata["state_sha256"]
                or _hash(imported) != metadata["replacement_sha256"]):
            _fail("invalid_plan")
        old = save_state.decode(original) if original is not None else save_state.State(0)
        if old.generation != metadata["generation"]:
            _fail("invalid_plan")
        save_state.decode(imported)
        return metadata, original, imported
    finally:
        for fd in (folder, parent):
            if fd is not None:
                os.close(fd)


def _publish_common(private, folder, binding, namespace, backup_id, encoded, *, kind):
    # The caller invokes this only after a successful local import or verified
    # remote commit. Data and old-state backups were already fsynced privately.
    receipts = _child(private, "cloud-import-receipts", create=True)
    temporary = ".receipt-" + uuid.uuid4().hex
    pointer_temp = ".baseline-" + uuid.uuid4().hex
    try:
        digest = save_state.content_digest(save_state.decode(encoded))
        value = {"schema": 1, "kind": kind, "scope_binding": binding, "namespace": namespace,
                 "backup_id": backup_id, "imported_sha256": _hash(encoded), "content_digest": digest}
        data = (json.dumps(value, sort_keys=True) + "\n").encode()
        filename = backup_id + ".json"
        _write(receipts, temporary, data, mode=0o400)
        _rename_noreplace(receipts, temporary, filename)
        os.fsync(receipts)
        pointer = {"schema": 1, "scope_binding": binding, "namespace": namespace,
                   "receipt": filename, "sha256": _hash(data)}
        _write(folder, pointer_temp, (json.dumps(pointer, sort_keys=True) + "\n").encode())
        os.replace(pointer_temp, "cloud-baseline.json", src_dir_fd=folder, dst_dir_fd=folder)
        os.fsync(folder)
    finally:
        for fd, name in ((receipts, temporary), (folder, pointer_temp)):
            try:
                os.unlink(name, dir_fd=fd)
            except FileNotFoundError:
                pass
        os.close(receipts)


def load_baseline(runtime, scope):
    """Load only a hash-bound receipt from a previously successful operation.

    Absent receipts return None. Corrupt or mismatched receipts fail explicitly;
    arbitrary downloaded archives can never be promoted to a baseline here.
    """
    try:
        binding, namespace = _scope(scope)
        with _local(runtime, namespace) as (_, private, _, folder):
            if folder is None:
                return None
            data = _read(folder, "cloud-baseline.json", 16384, missing=True)
            if data is None:
                return None
            pointer = _json(data)
            if (set(pointer) != {"schema", "scope_binding", "namespace", "receipt", "sha256"}
                    or type(pointer["schema"]) is not int or pointer["schema"] != 1
                    or pointer["scope_binding"] != binding or pointer["namespace"] != namespace
                    or not isinstance(pointer["receipt"], str)
                    or not re.fullmatch(r"backup-[0-9a-f]{32}\.json", pointer["receipt"])):
                _fail("invalid_plan")
            receipts = _child(private, "cloud-import-receipts")
            try:
                encoded = _read(receipts, pointer["receipt"], 16384)
            finally:
                os.close(receipts)
            if _hash(encoded) != pointer["sha256"]:
                _fail("invalid_plan")
            receipt = _json(encoded)
            if (set(receipt) != {"schema", "kind", "scope_binding", "namespace", "backup_id", "imported_sha256", "content_digest"}
                    or type(receipt["schema"]) is not int or receipt["schema"] != 1
                    or receipt["kind"] not in {"import", "sync"}
                    or receipt["scope_binding"] != binding or receipt["namespace"] != namespace
                    or receipt["backup_id"] + ".json" != pointer["receipt"]):
                _fail("invalid_plan")
            metadata, _, imported = _backup_data(private, binding, namespace, receipt["backup_id"])
            state = save_state.decode(imported)
            if receipt["imported_sha256"] != metadata["replacement_sha256"] or receipt["content_digest"] != save_state.content_digest(state):
                _fail("invalid_plan")
            return _baseline(binding, state)
    except CloudImportError:
        raise
    except Exception:
        _fail("invalid_plan")


def _commit_file(runtime, root, private, saves, folder, namespace, local_digest, encoded, lease, cancel):
    temporary = ".cloud-import-" + uuid.uuid4().hex
    try:
        _write(folder, temporary, encoded)
        _cancel(cancel)
        fresh, _ = _state(folder)
        if (_hash(fresh) if fresh is not None else "missing") != local_digest:
            _fail("changed")
        _lease(private, lease)
        reopened = _open_path(Path(runtime), private=False)
        try:
            if (os.fstat(reopened).st_dev, os.fstat(reopened).st_ino) != (os.fstat(root).st_dev, os.fstat(root).st_ino):
                _fail("changed")
        finally:
            os.close(reopened)
        _linked(root, "private", private)
        _linked(private, "local-saves", saves)
        _linked(saves, namespace, folder)
        os.replace(temporary, "state.bin", src_dir_fd=folder, dst_dir_fd=folder)
        temporary = None
        try:
            os.fsync(folder)
        except OSError:
            return False
        return True
    finally:
        if temporary is not None:
            try:
                os.unlink(temporary, dir_fd=folder)
            except FileNotFoundError:
                pass


def apply(runtime, scope, plan, *, choice=None, runtime_lock_fd, cancel=None):
    """Apply an exact private plan with backup, fresh scope and caller's lease.

    ``cloud`` is an explicit full replacement of this namespace, including an
    empty cloud state. A mapping resolves exactly the named conflicts and keeps
    the three-way decisions elsewhere. No timestamp selects a winner.
    """
    committed_result = None
    try:
        binding, namespace = _scope(scope)
        if (not isinstance(plan, ImportPlan) or plan._runtime != str(Path(runtime))
                or plan.scope_binding != binding or plan.namespace != namespace):
            _fail("invalid_scope")
        if not hmac.compare_digest(plan._seal_value, _seal(_plan_data(plan))):
            _fail("invalid_plan")
        _cancel(cancel)
        remote, snapshot_digest = _snapshot(runtime, scope, plan._snapshot)
        if snapshot_digest != plan.snapshot_digest or save_state.content_digest(remote) != plan.remote_digest:
            _fail("changed")
        with _local(runtime, namespace, create=True, lease=runtime_lock_fd) as (root, private, saves, folder):
            raw, local = _state(folder)
            if (_hash(raw) if raw is not None else "missing") != plan.local_digest:
                _fail("changed")
            selected = _select(local, remote, plan, choice)
            total = sum(len(blob) for entry in selected.containers.values() for blob in entry.blobs.values())
            blobs = sum(len(entry.blobs) for entry in selected.containers.values())
            common = _baseline(binding, selected) if save_state.content_digest(selected) == plan.remote_digest else plan._baseline
            if save_state.content_digest(selected) == save_state.content_digest(local):
                return ImportResult(False, None, local.generation, len(selected.containers), blobs, total, True, plan._baseline)
            if local.generation == 2**64 - 1:
                _fail("unsupported")
            selected.generation += 1
            encoded = save_state.encode(selected)
            _cancel(cancel)
            backup = _backup(private, plan, raw, encoded)
            durable = _commit_file(runtime, root, private, saves, folder, namespace,
                                   plan.local_digest, encoded, runtime_lock_fd, cancel)
            committed_result = ImportResult(True, backup, selected.generation, len(selected.containers), blobs, total, durable, None)
            if common is not None and save_state.content_digest(selected) == plan.remote_digest:
                try:
                    _publish_common(private, folder, binding, namespace, backup, encoded, kind="import")
                    committed_result = replace(committed_result, baseline=common)
                except Exception:
                    committed_result = replace(committed_result, durability_confirmed=False)
            return committed_result
    except CloudImportError:
        if committed_result is not None:
            return replace(committed_result, durability_confirmed=False)
        raise
    except save_state.SaveStateError:
        _fail("unsupported")
    except Exception:
        if committed_result is not None:
            return replace(committed_result, durability_confirmed=False)
        _fail("local_storage")


@dataclass(frozen=True, repr=False)
class _BackupIdentity:
    scope_binding: str
    namespace: str
    generation: int
    snapshot_digest: str | None = None


def restore(runtime, scope, backup_id, *, runtime_lock_fd, cancel=None):
    """Restore a private backup only while the exact imported revision remains.

    This is a local rollback, not a rollback of any Xbox cloud mutation. A new
    backup precedes it, and the native generation increases rather than rewinds.
    """
    result = None
    try:
        binding, namespace = _scope(scope)
        _cancel(cancel)
        with _local(runtime, namespace, create=True, lease=runtime_lock_fd) as (root, private, saves, folder):
            metadata, original, _ = _backup_data(private, binding, namespace, backup_id)
            raw, local = _state(folder)
            if raw is None or _hash(raw) != metadata["replacement_sha256"]:
                _fail("changed")
            if local.generation == 2**64 - 1:
                _fail("unsupported")
            restored = save_state.decode(original) if original is not None else save_state.State(0)
            restored.generation = local.generation + 1
            encoded = save_state.encode(restored)
            backup = _backup(private, _BackupIdentity(binding, namespace, local.generation), raw, encoded)
            durable = _commit_file(runtime, root, private, saves, folder, namespace, _hash(raw), encoded, runtime_lock_fd, cancel)
            result = ImportResult(True, backup, restored.generation, len(restored.containers),
                                  sum(len(c.blobs) for c in restored.containers.values()),
                                  sum(len(v) for c in restored.containers.values() for v in c.blobs.values()), durable)
            return result
    except CloudImportError:
        if result is not None:
            return replace(result, durability_confirmed=False)
        raise
    except Exception:
        if result is not None:
            return replace(result, durability_confirmed=False)
        _fail("local_storage")


@dataclass(frozen=True, repr=False)
class LocalExport:
    scope_binding: str
    namespace: str
    local_digest: str
    content_digest: str
    generation: int
    encoded: bytes
    _private: int
    _folder: int
    _lease_fd: int
    _root: int
    _saves: int
    _runtime: str
    _active: bool = True

    @property
    def state(self):
        return save_state.decode(self.encoded)

    def assert_unchanged(self):
        if not self._active or id(self) not in _ACTIVE_EXPORTS:
            _fail("invalid_plan")
        _lease(self._private, self._lease_fd)
        _linked(self._root, "private", self._private)
        _linked(self._private, "local-saves", self._saves)
        _linked(self._saves, self.namespace, self._folder)
        current = _open_path(self._runtime, private=False)
        try:
            if (os.fstat(current).st_dev, os.fstat(current).st_ino) != (os.fstat(self._root).st_dev, os.fstat(self._root).st_ino):
                _fail("changed")
        finally:
            os.close(current)
        raw, _ = _state(self._folder)
        if (_hash(raw) if raw is not None else "missing") != self.local_digest:
            _fail("changed")


@contextmanager
def export_local(runtime, scope, *, runtime_lock_fd, cancel=None):
    """Hold the native lock through the caller's complete remote transaction."""
    entered = False
    try:
        binding, namespace = _scope(scope)
        _cancel(cancel)
        with _local(runtime, namespace, create=True, lease=runtime_lock_fd) as (root, private, saves, folder):
            raw, state = _state(folder)
            export = LocalExport(binding, namespace, _hash(raw) if raw is not None else "missing",
                                 save_state.content_digest(state), state.generation,
                                 raw if raw is not None else save_state.encode(state), private, folder, runtime_lock_fd,
                                 root, saves, str(Path(runtime)))
            _ACTIVE_EXPORTS.add(id(export))
            try:
                entered = True
                yield export
            finally:
                _ACTIVE_EXPORTS.discard(id(export))
                object.__setattr__(export, "_active", False)
    except CloudImportError:
        raise
    except Exception:
        if entered:
            raise  # Preserve the caller's remote transaction error and receipt.
        _fail("local_storage")


def record_common(export, receipt):
    """Persist common content only after a verified remote commit and readback.

    This stays inside export_local's existing lock lifetime: opening another
    writer.lock descriptor could release this process's POSIX record lock.
    """
    from .cloud_write import verify_receipt
    if not isinstance(export, LocalExport):
        _fail("invalid_plan")
    export.assert_unchanged()
    if not verify_receipt(receipt, scope_binding=export.scope_binding,
                          source_content_digest=export.content_digest):
        _fail("invalid_plan")
    try:
        raw, state = _state(export._folder)
        backup = _backup(export._private, _BackupIdentity(export.scope_binding, export.namespace, state.generation),
                         raw, export.encoded)
        export.assert_unchanged()
        _publish_common(export._private, export._folder, export.scope_binding, export.namespace,
                        backup, export.encoded, kind="sync")
        return _baseline(export.scope_binding, state)
    except CloudImportError:
        raise
    except Exception:
        # Remote success is represented by the caller's receipt, not this local
        # bookkeeping result; the manager must retain that success on failure.
        _fail("local_storage")
