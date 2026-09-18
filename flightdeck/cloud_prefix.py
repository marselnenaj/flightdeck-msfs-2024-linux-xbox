# SPDX-License-Identifier: MIT
"""Optional private Wine-prefix reuse, never cached authentication or processes.

A dirty same-boot prefix is abandoned rather than guessed safe. The caller
always creates a new helper/broker and may publish a ready prefix only after
its dedicated wineserver has stopped. Failure of this cache is a cold start.
"""
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import uuid
import fcntl

from . import cloud_import as ci
from .cloud_process_guard import _boot_id
from .cloud_storage import CloudStorageError, _json

_STATE = "prefix-state.json"
_REGISTRIES = ("system.reg", "user.reg", "userdef.reg")
_MAX_REGISTRY = 32 * 1024 * 1024


def _directory(parent, name):
    fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
    try:
        info = os.fstat(fd)
        if info.st_uid != os.getuid() or info.st_mode & 0o022:
            raise OSError()
        return fd
    except BaseException:
        os.close(fd)
        raise


def _file(parent, name, maximum):
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_nlink != 1 or info.st_mode & 0o022 or info.st_size > maximum):
            raise OSError()
        value = stream.read(maximum + 1)
        if len(value) != info.st_size:
            raise OSError()
        return value


def _remove_tree(parent, name, *, _budget=None, _depth=0):
    """Remove an owned stopped prefix without traversing its drive symlinks.

    Descriptor-relative operations also work on supported Python 3.10, whose
    shutil.rmtree does not yet accept dir_fd. Refuse changed directory entries.
    """
    budget = [100000] if _budget is None else _budget
    if _depth > 64 or budget[0] <= 0:
        raise OSError()
    before = os.stat(name, dir_fd=parent, follow_symlinks=False)
    if not stat.S_ISDIR(before.st_mode) or before.st_uid != os.getuid():
        raise OSError()
    directory = _directory(parent, name)
    try:
        opened = os.fstat(directory)
        if (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino):
            raise OSError()
        with os.scandir(directory) as entries:
            for entry in entries:
                budget[0] -= 1
                if budget[0] < 0:
                    raise OSError()
                info = os.stat(entry.name, dir_fd=directory, follow_symlinks=False)
                if info.st_uid != os.getuid():
                    raise OSError()
                if stat.S_ISDIR(info.st_mode):
                    _remove_tree(directory, entry.name, _budget=budget, _depth=_depth + 1)
                else:
                    os.unlink(entry.name, dir_fd=directory)
        linked = os.stat(name, dir_fd=parent, follow_symlinks=False)
        if (linked.st_dev, linked.st_ino) != (opened.st_dev, opened.st_ino):
            raise OSError()
        os.rmdir(name, dir_fd=parent)
    finally:
        os.close(directory)


@contextmanager
def _system(work_fd):
    owned = []
    try:
        current = work_fd
        for name in ("prefix", "drive_c", "windows", "system32"):
            current = _directory(current, name)
            owned.append(current)
        yield current
    finally:
        for fd in reversed(owned):
            os.close(fd)


def _valid(work_fd, libraries):
    try:
        prefix = _directory(work_fd, "prefix")
        try:
            for name in _REGISTRIES:
                if not _file(prefix, name, _MAX_REGISTRY).startswith(b"WINE REGISTRY Version 2"):
                    return False
            devices = _directory(prefix, "dosdevices")
            try:
                if (os.readlink("c:", dir_fd=devices) != "../drive_c"
                        or os.readlink("z:", dir_fd=devices) != "/"):
                    return False
            finally:
                os.close(devices)
        finally:
            os.close(prefix)
        with _system(work_fd) as system:
            return all(hashlib.sha256(_file(system, name, 32 * 1024 * 1024)).digest()
                       == hashlib.sha256(data).digest() for name, data in libraries.items())
    except (OSError, ValueError):
        return False


def _state(work_fd, binding, boot, phase):
    temporary = ".prefix-state-" + uuid.uuid4().hex
    try:
        ci._write(work_fd, temporary, (json.dumps({"schema": 1, "binding": binding,
                   "boot": boot, "phase": phase}, sort_keys=True) + "\n").encode())
        os.replace(temporary, _STATE, src_dir_fd=work_fd, dst_dir_fd=work_fd)
        os.fsync(work_fd)
    finally:
        try:
            os.unlink(temporary, dir_fd=work_fd)
        except FileNotFoundError:
            pass


class Prefix:
    def __init__(self, work, work_fd, libraries, *, warm=False, binding=None, boot=None):
        self.work, self.path = work, work / "prefix"
        self.reusable = warm
        self._fd, self._libraries = work_fd, libraries
        self._binding, self._boot = binding, boot
        self.stopped = False

    def install_libraries(self):
        """Replace only three named DLLs without following Wine's file links."""
        with _system(self._fd) as system:
            for name, data in self._libraries.items():
                temporary = ".cloud-library-" + uuid.uuid4().hex
                try:
                    ci._write(system, temporary, data)
                    os.replace(temporary, name, src_dir_fd=system, dst_dir_fd=system)
                finally:
                    try:
                        os.unlink(temporary, dir_fd=system)
                    except FileNotFoundError:
                        pass
            os.fsync(system)

    def complete(self):
        """Called only after confirmed helper/broker and prefix-server cleanup.

        Cache publication is optional: never discard a completed cloud result
        because the prefix cannot be reused on the next call.
        """
        self.stopped = True
        if self._binding is not None:
            try:
                if _valid(self._fd, self._libraries):
                    _state(self._fd, self._binding, self._boot, "ready")
            except (OSError, ValueError, ci.CloudImportError, CloudStorageError):
                pass


@contextmanager
def session(runtime, binding, libraries):
    """Exclusive cache lease, falling back to a distinct cold prefix safely."""
    if (not isinstance(binding, str) or not re.fullmatch(r"[0-9a-f]{64}", binding)
            or set(libraries) != {"xgameruntime.dll", "xgameruntime_original.dll", "xodus_store_test.dll"}
            or any(type(data) is not bytes or not 0 < len(data) <= 32 * 1024 * 1024 for data in libraries.values())):
        raise CloudStorageError("local_storage")
    root = private = cache = work_fd = lock_fd = None
    temporary = None
    lease = None
    try:
        root = ci._open_path(runtime, private=False)
        private = ci._child(root, "private")
        try:
            boot = _boot_id()
            cache = ci._child(private, "cloud-helper-prefixes", create=True)
            work_fd = ci._child(cache, binding, create=True)
            lock_fd = os.open("use.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK,
                              0o600, dir_fd=work_fd)
            ci._file_info(lock_fd)
            fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            raw = ci._read(work_fd, _STATE, 512, missing=True)
            previous = _json(raw) if raw is not None else None
            if previous is not None:
                if (not isinstance(previous, dict) or set(previous) != {"schema", "binding", "boot", "phase"}
                        or type(previous["schema"]) is not int or previous["schema"] != 1
                        or previous["binding"] != binding or previous["phase"] not in {"ready", "in_use"}
                        or not isinstance(previous["boot"], str)
                        or not re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", previous["boot"])
                        or previous["boot"] == "00000000-0000-0000-0000-000000000000"):
                    raise OSError()
                if previous["phase"] == "in_use" and previous["boot"] == boot:
                    raise OSError()  # Unknown surviving process: leave this generation alone.
            warm = previous is not None and previous["phase"] == "ready" and _valid(work_fd, libraries)
            if not warm:
                try:
                    os.stat("prefix", dir_fd=work_fd, follow_symlinks=False)
                except FileNotFoundError:
                    pass
                else:
                    if previous is None:
                        raise OSError()  # Never remove an unrecognised preexisting directory.
                    _remove_tree(work_fd, "prefix")
                    os.fsync(work_fd)
            _state(work_fd, binding, boot, "in_use")
            ci._linked(private, "cloud-helper-prefixes", cache)
            ci._linked(cache, binding, work_fd)
            lease = Prefix(Path(runtime) / "private/cloud-helper-prefixes" / binding,
                           work_fd, libraries, warm=warm, binding=binding, boot=boot)
        except Exception:
            # Invalid cache, contention, or uncertain cleanup is only a cache
            # miss. Retain it for diagnosis; do not force-stop unknown children.
            for fd in (lock_fd, work_fd, cache):
                if fd is not None:
                    os.close(fd)
            lock_fd = work_fd = cache = None
            temporary = "cloud-reader-" + uuid.uuid4().hex
            os.mkdir(temporary, 0o700, dir_fd=private)
            work_fd = ci._child(private, temporary)
            lease = Prefix(Path(runtime) / "private" / temporary, work_fd, libraries)
        ci._linked(root, "private", private)
        yield lease
    finally:
        # A failed cleanup must not unlink files still used by an orphan helper.
        if temporary is not None and lease is not None and lease.stopped:
            try:
                _remove_tree(private, temporary)
                os.fsync(private)
            except OSError:
                pass
        for fd in (lock_fd, work_fd, cache, private, root):
            if fd is not None:
                try:
                    os.close(fd)
                except OSError:
                    pass
