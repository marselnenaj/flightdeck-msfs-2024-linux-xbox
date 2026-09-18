# SPDX-License-Identifier: MIT
"""Scope-bound download cache; every use still reads the live index twice.

Only complete validated snapshots become cache candidates. A hit needs an exact
container revision tuple, not a timestamp/TTL or atom GUID guess. Cache failures
are misses, never a substitute for successful authentication or a complete live
inventory. Written containers can be excluded from reuse so commit readback
always verifies their actual remote payloads, even if an ETag did not change.
"""
from contextlib import contextmanager
from dataclasses import replace
import hashlib
import json
import os
from pathlib import Path
import re
import time
import uuid

from . import cloud_import as ci, cloud_storage as cs, save_state

_KEYS = {"schema", "scope_binding", "snapshot_id", "manifest_sha256",
         "container_count", "blob_count", "total_bytes"}


def _require(condition):
    if not condition:
        raise cs.CloudStorageError("local_storage")


def _check(cancel, deadline=None):
    if cancel is not None and cancel.is_set():
        raise cs.CloudStorageError("cancelled")
    if deadline is not None:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise cs.CloudStorageError("deadline")
        return remaining


@contextmanager
def _folder(runtime, *, create=False):
    owned = []
    try:
        root = ci._open_path(runtime, private=False); owned.append(root)
        private = ci._child(root, "private"); owned.append(private)
        folder = ci._child(private, "cloud-cache", create=create); owned.append(folder)
        yield folder
        ci._linked(private, "cloud-cache", folder)
        ci._linked(root, "private", private)
    finally:
        for fd in reversed(owned):
            os.close(fd)


def _validated(runtime, scope, snapshot, expected=None):
    # Native decoding, no-follow descriptors, file counts and all stored hashes
    # are checked by the existing importer. Metadata below must be the very
    # same bytes whose digest that validation returned, including across races.
    state, digest = ci._snapshot(runtime, scope, snapshot)
    if expected is not None:
        _require(digest == expected)
    fd = ci._open_path(snapshot.path)
    try:
        raw = ci._read(fd, "manifest.json", save_state.METADATA_LIMIT)
    finally:
        os.close(fd)
    _require(hashlib.sha256(raw).hexdigest() == digest)
    manifest = cs._json(raw)
    containers = []
    for row in manifest["containers"]:
        name = row["name"][:-len(",savedgame")]
        item = cs.Container(row["name"], row["display_name"], row["etag"], row["client_file_time"], row["size"])
        blobs = tuple(cs._CachedBlob(cs.Atom(blob["atom"], blob["name"], blob["size"]),
                     state.containers[name].blobs[blob["name"]], blob["sha256"]) for blob in row["blobs"])
        containers.append(cs._CachedContainer(item, blobs))
    return cs._SnapshotCache(scope.binding, tuple(containers)), digest


def _load(runtime, scope):
    try:
        with _folder(runtime) as folder:
            raw = ci._read(folder, scope.binding + ".json", 4096)
        value = cs._json(raw)
        _require(isinstance(value, dict) and set(value) == _KEYS
                 and type(value["schema"]) is int and value["schema"] == 1
                 and value["scope_binding"] == scope.binding
                 and isinstance(value["snapshot_id"], str)
                 and re.fullmatch(r"snapshot-[0-9a-f]{32}", value["snapshot_id"])
                 and isinstance(value["manifest_sha256"], str)
                 and re.fullmatch(r"[0-9a-f]{64}", value["manifest_sha256"]))
        snapshot = cs.Snapshot(Path(runtime) / "private/cloud-saves" / value["snapshot_id"],
                              scope.binding, value["container_count"], value["blob_count"], value["total_bytes"])
        return _validated(runtime, scope, snapshot, value["manifest_sha256"])[0]
    except (OSError, ValueError, TypeError, KeyError, ci.CloudImportError, cs.CloudStorageError):
        return None


def remember(runtime, scope, snapshot):
    """Remember a fully verified network snapshot; return False on cache I/O failure.

    No baseline, remote receipt or active save is changed. Invalid snapshots
    still raise; optional cache publication cannot invalidate a verified upload.
    Callers must not use this as evidence that any cloud write completed.
    """
    _, digest = _validated(runtime, scope, snapshot)
    value = {"schema": 1, "scope_binding": scope.binding,
             "snapshot_id": snapshot.path.name, "manifest_sha256": digest,
             "container_count": snapshot.container_count, "blob_count": snapshot.blob_count,
             "total_bytes": snapshot.total_bytes}
    payload = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    temporary = ".cache-" + uuid.uuid4().hex
    try:
        with _folder(runtime, create=True) as folder:
            try:
                cs._write(folder, temporary, payload)
                os.replace(temporary, scope.binding + ".json", src_dir_fd=folder, dst_dir_fd=folder)
                os.fsync(folder)
            finally:
                cs._unlink(folder, temporary)
        return True
    except (OSError, ci.CloudImportError, cs.CloudStorageError):
        return False


def _snapshot_parent(runtime):
    owned = []
    try:
        root = ci._open_path(runtime, private=False); owned.append(root)
        private = ci._child(root, "private"); owned.append(private)
        snapshots = ci._child(private, "cloud-saves", create=True); owned.append(snapshots)
        ci._linked(private, "cloud-saves", snapshots)
        ci._linked(root, "private", private)
        return Path(runtime) / "private/cloud-saves"
    except (OSError, ci.CloudImportError):
        raise cs.CloudStorageError("local_storage") from None
    finally:
        for fd in reversed(owned):
            os.close(fd)


def download(runtime, client, *, cancel=None, force_containers=frozenset()):
    """Read current cloud state; force_containers contains logical save names.

    Every explicitly named container bypasses reuse, including on an unchanged
    ETag. Missing names are confirmed absent by both complete live indexes.
    """
    if type(force_containers) is not frozenset or len(force_containers) > client.limits.max_containers * 2:
        raise ValueError("Invalid forced container selection.")
    try:
        forced = {save_state._name(name, container=True) + ",savedgame" for name in force_containers}
    except save_state.SaveStateError:
        raise ValueError("Invalid forced container selection.") from None
    deadline = time.monotonic() + client.limits.deadline_seconds
    _check(cancel, deadline)
    cached = _load(runtime, client.scope)
    if cached is not None and forced:
        cached = replace(cached, containers=tuple(row for row in cached.containers
                                                  if row.container.name not in forced))
    _check(cancel, deadline)
    parent = _snapshot_parent(runtime)
    remaining = _check(cancel, deadline)
    limited = cs.CloudStorageClient(client.scope, client.transport,
        limits=replace(client.limits, deadline_seconds=remaining))
    snapshot = limited.download_snapshot(parent, cancel=cancel, _cached=cached)
    _check(cancel, deadline)
    try:
        remember(runtime, client.scope, snapshot)
    except (OSError, ValueError, TypeError, KeyError, ci.CloudImportError, cs.CloudStorageError):
        # The caller still validates this returned snapshot before import. A
        # failed optional cache refresh cannot turn a real read into a failure.
        pass
    _check(cancel, deadline)
    return snapshot
