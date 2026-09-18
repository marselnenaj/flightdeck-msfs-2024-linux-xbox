# SPDX-License-Identifier: MIT
"""Fence a supervised game lifetime without guessing which processes remain.

The coordinator marks before Popen and clears only after that exact supervisor
has completed its normal cleanup (or Popen failed without creating a child).
After an unobserved/unclean exit, only a reboot proves orphaned processes gone.
The coordinator holds its runtime flock during every operation. No process is
inspected or signalled, and no save or account data is recorded.
"""
from contextlib import contextmanager
import json
import os
import re
import threading
import uuid

from . import cloud_import as ci
from .cloud_storage import _json, _rename_noreplace

_NAME = "cloud-interrupted-process.json"
_BOOT_ID = "/proc/sys/kernel/random/boot_id"
_LOCK = threading.RLock()
_GUID = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}")


class UnsafeSessionError(Exception):
    code = "unsafe_session"

    def __init__(self):
        super().__init__("A previous game session could not be confirmed as stopped. Restart Linux before playing again.")


def _boot_id():
    fd = os.open(_BOOT_ID, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        data = source.read(80)
    value = data.removesuffix(b"\n").decode("ascii")
    if not _GUID.fullmatch(value) or value == "00000000-0000-0000-0000-000000000000":
        raise UnsafeSessionError()
    return value


@contextmanager
def _access(runtime, runtime_lock_fd):
    root = private = None
    with _LOCK:
        try:
            root = ci._open_path(runtime, private=False)
            private = ci._child(root, "private")
            ci._lease(private, runtime_lock_fd)
            yield root, private
            ci._lease(private, runtime_lock_fd)
            _linked(runtime, root, private)
        except UnsafeSessionError:
            raise
        except Exception:
            raise UnsafeSessionError() from None
        finally:
            if private is not None:
                os.close(private)
            if root is not None:
                os.close(root)


def _linked(runtime, root, private):
    current = ci._open_path(runtime, private=False)
    try:
        before, after = os.fstat(root), os.fstat(current)
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
            raise UnsafeSessionError()
    finally:
        os.close(current)
    ci._linked(root, "private", private)


def _read(private):
    raw = ci._read(private, _NAME, 512, missing=True)
    if raw is None:
        return None
    value = _json(raw)
    if (not isinstance(value, dict) or set(value) != {"schema", "boot_id"}
            or type(value["schema"]) is not int or value["schema"] != 1
            or not isinstance(value["boot_id"], str) or not _GUID.fullmatch(value["boot_id"])
            or value["boot_id"] == "00000000-0000-0000-0000-000000000000"):
        raise UnsafeSessionError()
    return value


def check(runtime, runtime_lock_fd):
    """Reject interrupted sessions from this boot; clear a verified older marker."""
    with _access(runtime, runtime_lock_fd) as (root, private):
        boot = _boot_id()
        value = _read(private)
        if value is None:
            return
        if value["boot_id"] == boot:
            raise UnsafeSessionError()
        _linked(runtime, root, private)
        if _read(private) != value:
            raise UnsafeSessionError()
        os.unlink(_NAME, dir_fd=private)
        os.fsync(private)


def mark(runtime, runtime_lock_fd):
    """Durably fence a start before Popen, including a possible launcher crash."""
    with _access(runtime, runtime_lock_fd) as (root, private):
        boot = _boot_id()
        previous = _read(private)
        if previous is not None and previous["boot_id"] == boot:
            os.fsync(private)
            return
        value = {"schema": 1, "boot_id": boot}
        temporary = ".interrupted-process-" + uuid.uuid4().hex
        try:
            ci._write(private, temporary, (json.dumps(value, sort_keys=True) + "\n").encode())
            _linked(runtime, root, private)
            if _read(private) != previous:
                raise UnsafeSessionError()
            if previous is None:
                _rename_noreplace(private, temporary, _NAME)
            else:
                os.replace(temporary, _NAME, src_dir_fd=private, dst_dir_fd=private)
            os.fsync(private)
        finally:
            try:
                os.unlink(temporary, dir_fd=private)
            except FileNotFoundError:
                pass


def clear(runtime, runtime_lock_fd):
    """Clear after confirmed supervisor cleanup, or a failed Popen with no child.

    The caller must have waited for its exact child and seen a nonnegative
    return code. Never use this on timeout, unobserved exit, or signal death.
    """
    with _access(runtime, runtime_lock_fd) as (root, private):
        value = _read(private)
        if value is None or value["boot_id"] != _boot_id():
            raise UnsafeSessionError()
        _linked(runtime, root, private)
        if _read(private) != value:
            raise UnsafeSessionError()
        os.unlink(_NAME, dir_fd=private)
        os.fsync(private)
