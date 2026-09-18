# SPDX-License-Identifier: MIT
"""Original, bounded ConnectedStorage write coordination.

The native adapter owns authentication and the fixed HTTP operations. This
module owns conservative lease fencing, conflict detection and readback. The
protocol observations are documented in docs/connected-storage-protocol.md;
no Microsoft implementation code is incorporated here.

Only a container commit is atomic. Failure after one of several container
commits is explicitly partial/uncertain and requires a fresh comparison.
"""
from __future__ import annotations

from dataclasses import dataclass, field
import hashlib
import hmac
import json
import math
import os
import re
import time
from typing import Callable, Protocol

from . import save_state
from .cloud_storage import CloudStorageError, Scope

_SEAL_KEY = os.urandom(32)
_PROTOCOL = "connected-storage-lock-v1-readback"
_ERRORS = {
    "invalid_input": "The cloud-save write input is invalid.",
    "invalid_scope": "The cloud-save write account or title does not match.",
    "invalid_response": "The cloud-save write response is invalid.",
    "conflict": "Cloud saves changed. A fresh comparison is required.",
    "local_changed": "Local saves changed. A fresh comparison is required.",
    "lease_lost": "The exclusive cloud-save lock is no longer owned.",
    "authentication": "Cloud-save authentication was rejected.",
    "transport": "The cloud-save write could not be confirmed.",
    "bounds": "The cloud-save write exceeds its supported limits.",
    "quota": "The selected saves exceed the cloud-save quota.",
    "cancelled": "The cloud-save write was cancelled.",
    "deadline": "The cloud-save write exceeded its time limit.",
    "readback": "The cloud-save commit could not be verified by reading it back.",
}


class CloudWriteError(Exception):
    """No save names, response text, tokens or paths appear in an error."""
    def __init__(self, code, *, committed_containers=0, recovery_required=False):
        self.code = code
        self.committed_containers = committed_containers
        self.recovery_required = recovery_required
        super().__init__(_ERRORS[code])


@dataclass(frozen=True, repr=False)
class LeaseReply:
    status: int
    owner_change_id: str
    quota_bytes: int


class WriteOperations(Protocol):
    scope: Scope
    def acquire(self, *, timeout: float) -> LeaseReply: ...
    def renew(self, *, timeout: float) -> LeaseReply: ...
    # Release is cleanup, including after cancellation: the adapter must use a
    # separate bounded cleanup budget, never re-acquire or break another lock.
    def release(self, *, timeout: float) -> int: ...
    def upload_atom(self, payload: bytes, *, timeout: float) -> str: ...
    def put_container(self, name: str, display_name: str, modified: int,
                      atoms: dict[str, str], *, timeout: float) -> int: ...
    def delete_container(self, name: str, *, timeout: float) -> int: ...


@dataclass(frozen=True)
class Limits:
    max_operations: int = 8192
    max_blob_bytes: int = 64 * 1024 * 1024
    max_total_bytes: int = save_state.QUOTA
    timeout_seconds: float = 30
    deadline_seconds: float = 600
    cleanup_seconds: float = 10

    def __post_init__(self):
        for value in (self.max_operations, self.max_blob_bytes, self.max_total_bytes):
            if type(value) is not int or value <= 0:
                raise ValueError("Invalid cloud write limits.")
        for value in (self.timeout_seconds, self.deadline_seconds, self.cleanup_seconds):
            if type(value) not in (int, float) or not math.isfinite(value) or value <= 0:
                raise ValueError("Invalid cloud write time limit.")


@dataclass(frozen=True, repr=False)
class CommitReceipt:
    scope_binding: str
    source_content_digest: str
    remote_before_digest: str
    committed: bool
    changed_containers: int
    container_count: int
    blob_count: int
    total_bytes: int
    lease_released: bool
    protocolproof: str = _PROTOCOL
    _seal_value: str = field(default="", repr=False)


def _receipt_payload(receipt):
    return [getattr(receipt, key) for key in (
        "scope_binding", "source_content_digest", "remote_before_digest", "committed",
        "changed_containers", "container_count", "blob_count", "total_bytes",
        "lease_released", "protocolproof")]


def _seal(receipt):
    return hmac.new(_SEAL_KEY, json.dumps(_receipt_payload(receipt),
                    separators=(",", ":")).encode(), hashlib.sha256).hexdigest()


def verify_receipt(receipt, *, scope_binding, source_content_digest):
    """Only this process's successful, exact-readback receipts form baselines."""
    try:
        return (type(receipt) is CommitReceipt and receipt.committed is True
                and receipt.scope_binding == scope_binding
                and receipt.source_content_digest == source_content_digest
                and receipt.protocolproof == _PROTOCOL
                and hmac.compare_digest(receipt._seal_value, _seal(receipt)))
    except (ValueError, TypeError, AttributeError):
        return False


def _copy(state):
    try:
        return save_state.decode(save_state.encode(state))
    except (save_state.SaveStateError, ValueError, TypeError, AttributeError):
        raise CloudWriteError("invalid_input") from None


def _entry_digest(name, entry):
    return save_state.content_digest(save_state.State(0, {name: entry}))


class _Write:
    def __init__(self, scope, operations, read_remote, assert_local_unchanged, cancel, limits,
                 read_committed=None):
        self.scope, self.ops, self.read_remote = scope, operations, read_remote
        self.read_committed = read_committed
        self.assert_local = assert_local_unchanged
        self.cancel, self.limits = cancel, limits
        self.deadline = time.monotonic() + limits.deadline_seconds
        self.operations = self.committed = 0
        self.mutation_attempted = self.acquired = False
        self.mutated_names = set()
        self.owner = None

    def fail(self, code):
        raise CloudWriteError(code, committed_containers=self.committed,
                              recovery_required=self.mutation_attempted)

    def check(self):
        if self.cancel is not None and self.cancel.is_set():
            self.fail("cancelled")
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            self.fail("deadline")
        try:
            self.assert_local()
        except Exception:
            self.fail("local_changed")
        return remaining

    def call(self, method, *args):
        timeout = min(self.check(), self.limits.timeout_seconds)
        self.operations += 1
        if self.operations > self.limits.max_operations:
            self.fail("bounds")
        return method(*args, timeout=timeout)

    def remote(self, container_names=frozenset()):
        # The callback must apply this absolute remaining budget to its reader;
        # checking again also prevents a late callback from producing success.
        # Selective readback still returns the COMPLETE verified remote state.
        # Written containers must bypass revision caching even if their ETag
        # did not change; untouched containers may use exact revision proofs.
        timeout = self.check()
        if container_names and self.read_committed is not None:
            state = _copy(self.read_committed(timeout=timeout,
                                            container_names=frozenset(container_names)))
        else:
            state = _copy(self.read_remote(timeout=timeout))
        self.check()
        return state

    def lease(self, reply, *, first=False):
        if not isinstance(reply, LeaseReply) or type(reply.status) is not int:
            self.fail("invalid_response")
        if reply.status in (401, 403):
            self.acquired = False
            self.fail("authentication")
        if reply.status == 409:
            self.acquired = False
            self.fail("lease_lost")
        if reply.status not in (200, 201):
            self.fail("transport")
        if (not isinstance(reply.owner_change_id, str) or not reply.owner_change_id
                or len(reply.owner_change_id.encode("utf-8")) > 1024
                or any(ord(c) < 32 or ord(c) == 127 for c in reply.owner_change_id)
                or type(reply.quota_bytes) is not int or not 0 < reply.quota_bytes < 2**64):
            self.fail("invalid_response")
        if first:
            self.owner = reply.owner_change_id
            self.acquired = True
        elif reply.status == 201:
            # This is confirmed new ownership, safe to release but never to
            # continue an earlier transaction. Mirrors the reference client.
            self.acquired = True
            self.fail("lease_lost")
        elif reply.owner_change_id != self.owner:
            self.acquired = False
            self.fail("lease_lost")
        else:
            self.acquired = True
        self.check()

    def renew(self):
        def dispatch(*, timeout):
            # Once dispatched, an unconfirmed renewal also makes cleanup
            # ownership uncertain. A local preflight failure does not do so.
            self.acquired = False
            return self.ops.renew(timeout=timeout)
        self.lease(self.call(dispatch))

    def confirm_container(self, name, expected):
        self.renew()
        actual = self.remote(frozenset({name})).containers.get(name)
        if ((actual is None) != (expected is None)
                or actual is not None and _entry_digest(name, actual) != _entry_digest(name, expected)):
            self.fail("readback")
        self.renew()
        self.committed += 1

    def commit(self, name, entry, atoms=None):
        self.renew()
        self.check()
        self.mutation_attempted = True
        self.mutated_names.add(name)
        try:
            if entry is None:
                status = self.call(self.ops.delete_container, name + ",savedgame")
            else:
                status = self.call(self.ops.put_container, name + ",savedgame",
                                   entry.display_name, entry.modified, atoms)
        except (CloudStorageError, OSError, TimeoutError):
            # A lost HTTP response is not evidence of failure. Read it back
            # under the same owner; never replay a possibly successful commit.
            self.confirm_container(name, entry)
            return
        if type(status) is not int:
            self.fail("invalid_response")
        if status == 409:
            self.acquired = False
            self.fail("lease_lost")
        if status in (401, 403):
            self.acquired = False
            self.fail("authentication")
        if status not in (200, 201, 204):
            # Non-success can follow a server-side commit, too. The same
            # readback rule provides certainty without a blind HTTP retry.
            self.confirm_container(name, entry)
            return
        self.confirm_container(name, entry)


def upload(scope: Scope, operations: WriteOperations, read_remote: Callable,
           local_state: save_state.State, *, expected_remote_digest: str,
           assert_local_unchanged: Callable, cancel=None, limits: Limits | None = None,
           read_committed: Callable | None = None) -> CommitReceipt:
    """Explicitly replace cloud content with an already selected local state.

    The caller holds launcher/runtime/native-writer locks and retains a backup.
    `read_remote(timeout=seconds)` returns a fresh fully validated State from
    the same scope/helper session and must honor that total time budget. The
    expected digest comes from the user's reviewed remote snapshot, never an
    implicitly accepted fresh baseline. No remote container is silently merged.

    Optional `read_committed(timeout=seconds, container_names=frozenset(...))`
    also returns the COMPLETE verified remote State. The names are logical
    save-container names, without the wire `,savedgame` suffix. Every named
    container must be downloaded anew, regardless of matching ETags; deletion
    requires its absence from a complete live inventory. Other containers may
    reuse validated bytes only with exact live revision matches. Both callbacks
    must recheck the complete live inventory before and after their read.
    Without this callback, all commit verification uses `read_remote` as before.
    """
    if (not isinstance(scope, Scope) or not isinstance(getattr(operations, "scope", None), Scope)
            or operations.scope != scope):
        raise CloudWriteError("invalid_scope")
    if (not isinstance(expected_remote_digest, str)
            or not re.fullmatch(r"[a-f0-9]{64}", expected_remote_digest)
            or not callable(read_remote) or not callable(assert_local_unchanged)
            or read_committed is not None and not callable(read_committed)):
        raise CloudWriteError("invalid_input")
    local = _copy(local_state)
    source_digest = save_state.content_digest(local)
    limits = limits or Limits()
    blobs = [payload for entry in local.containers.values() for payload in entry.blobs.values()]
    total = sum(map(len, blobs))
    if total > limits.max_total_bytes or any(len(b) > limits.max_blob_bytes for b in blobs):
        raise CloudWriteError("bounds")
    run = _Write(scope, operations, read_remote, assert_local_unchanged, cancel, limits,
                 read_committed)
    released = False
    try:
        run.check()
        lease = run.call(operations.acquire)
        run.lease(lease, first=True)
        if total > lease.quota_bytes:
            run.fail("quota")
        remote = run.remote()
        if save_state.content_digest(remote) != expected_remote_digest:
            run.fail("conflict")
        run.renew()
        names = sorted(set(local.containers) | set(remote.containers))
        for name in names:
            target, old = local.containers.get(name), remote.containers.get(name)
            if target is not None and old is not None and _entry_digest(name, target) == _entry_digest(name, old):
                continue
            atoms = {}
            if target is not None:
                run.renew()
                for blob, payload in sorted(target.blobs.items()):
                    atom = run.call(operations.upload_atom, payload)
                    if (not isinstance(atom, str)
                            or not re.fullmatch(r"[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}", atom)
                            or atom.lower() in {value.lower() for value in atoms.values()}):
                        run.fail("invalid_response")
                    atoms[blob] = atom
                    run.check()
            run.commit(name, target, atoms)
        run.renew()
        if save_state.content_digest(run.remote(frozenset(run.mutated_names))) != source_digest:
            run.fail("readback")
        run.renew()
    except CloudWriteError as error:
        # Rewrap errors from pure input/reader helpers with transaction facts.
        raise CloudWriteError(error.code, committed_containers=run.committed,
                              recovery_required=run.mutation_attempted) from None
    except CloudStorageError as error:
        code = error.code if error.code in _ERRORS else "transport"
        raise CloudWriteError(code, committed_containers=run.committed,
                              recovery_required=run.mutation_attempted) from None
    except Exception:
        raise CloudWriteError("transport", committed_containers=run.committed,
                              recovery_required=run.mutation_attempted) from None
    finally:
        if run.acquired:
            try:
                status = operations.release(timeout=limits.cleanup_seconds)
                released = type(status) is int and status in (200, 204)
            except Exception:
                pass  # Never hide the original failure or fabricate release.
    receipt = CommitReceipt(scope.binding, source_digest, expected_remote_digest, True,
                            run.committed, len(local.containers), len(blobs), total, released)
    return CommitReceipt(**{**receipt.__dict__, "_seal_value": _seal(receipt)})
