# SPDX-License-Identifier: MIT
"""Read only the downloader's sealed, complete file set. Never learn a baseline.

This detects local damage, not malicious edits by the account that owns both
files and receipts. Symlinks inside the package are not traversed. No account
or file names leave the checker; it returns aggregate counts only.
"""
from __future__ import annotations

from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat

from .setup import SetupError, SetupCancelled, interrupted
from . import games

FEATURE = "streaming-integrity-index-v1"
UNAVAILABLE = "Für diese Installation fehlt ein vollständiger Download-Prüfnachweis. Eine vollständige Reparatur erstellt ihn; vorhandene Dateien werden nicht als fehlerfreie Vorlage übernommen."
MAX_INDEX = 32 * 1024 * 1024
MAX_FILES = 100_001
HEX = re.compile(r"[0-9a-f]{64}")


def _open(parent, name, *, directory=False):
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    if directory:
        flags |= os.O_DIRECTORY
    fd = os.open(name, flags, dir_fd=parent)
    info = os.fstat(fd)
    valid = stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode) and info.st_nlink == 1
    if not valid or info.st_uid != os.getuid():
        os.close(fd)
        raise ValueError()
    return fd


def _json(parent, name, limit):
    fd = _open(parent, name)
    with os.fdopen(fd, "rb") as stream:
        if os.fstat(stream.fileno()).st_size > limit:
            raise ValueError()
        data = stream.read(limit + 1)
        if len(data) > limit:
            raise ValueError()
        return json.loads(data)


def _name(value):
    return (isinstance(value, str) and 0 < len(value.encode("utf-8")) <= 4096
            and (not value.startswith(".xodus-") or value == ".xodus-streaming.msixvc") and not any(x in value for x in ("\\", ":", "\0"))
            and all(part not in {"", ".", ".."} for part in value.split("/")))


@contextmanager
def baseline(game):
    # The configured game pointer may be a symlink; every component below its
    # resolved root uses anchored descriptors and O_NOFOLLOW.
    root = _open(None, game.resolve(strict=True), directory=True)
    journal = lock = None
    try:
        journal = _open(root, ".xodus-resume", directory=True)
        if os.fstat(journal).st_mode & 0o077:
            raise ValueError()
        lock = _open(journal, "lock")
        fcntl.flock(lock, fcntl.LOCK_SH | fcntl.LOCK_NB)
        value = _json(journal, "integrity.json", MAX_INDEX)
        package = _json(journal, "package.json", 8192)
        if (not isinstance(value, dict) or set(value) != {"format", "source", "package_sha256", "files"}
                or type(value["format"]) is not int or value["format"] != 1
                or value["source"] != "xodus-completed-download-v1"
                or not isinstance(value["package_sha256"], str) or not HEX.fullmatch(value["package_sha256"])
                or package != {"format": 1, "package_sha256": value["package_sha256"]}
                or not isinstance(value["files"], list) or not 0 < len(value["files"]) <= MAX_FILES):
            raise ValueError()
        seen = set()
        for row in value["files"]:
            if (not isinstance(row, dict) or set(row) != {"name", "length", "sha256"}
                    or not _name(row["name"]) or row["name"] in seen
                    or type(row["length"]) is not int or not 0 <= row["length"] < 8 * 1024**4
                    or not isinstance(row["sha256"], str) or not HEX.fullmatch(row["sha256"])):
                raise ValueError()
            seen.add(row["name"])
        if ".xodus-streaming.msixvc" not in seen:
            raise ValueError()
        yield root, value
    finally:
        for fd in (lock, journal, root):
            if fd is not None:
                os.close(fd)


def available(game):
    try:
        with baseline(game):
            return True
    except (OSError, ValueError, TypeError, UnicodeError):
        return False


def _file(root, name):
    parent = os.dup(root)
    try:
        parts = name.split("/")
        for part in parts[:-1]:
            child = _open(parent, part, directory=True)
            os.close(parent)
            parent = child
        return _open(parent, parts[-1])
    finally:
        os.close(parent)


def verify(launcher, *, notify, cancel):
    runtime = launcher.runtime
    if runtime is None:
        raise SetupError("Zuerst eine Runtime auswählen.")
    game = games.path(runtime)
    with launcher.runtime_lock(operation="game_integrity"):
        try:
            with baseline(game) as (root, value):
                result = {"checked": 0, "missing": 0, "changed": 0, "unreadable": 0, "total": len(value["files"]), "healthy": False}
                for row in value["files"]:
                    interrupted(cancel)
                    try:
                        fd = _file(root, row["name"])
                        with os.fdopen(fd, "rb") as stream:
                            before = os.fstat(stream.fileno())
                            digest = hashlib.sha256()
                            length = 0
                            if before.st_size == row["length"]:
                                while chunk := stream.read(1024 * 1024):
                                    interrupted(cancel)
                                    length += len(chunk)
                                    if length > row["length"]:
                                        break
                                    digest.update(chunk)
                            after = os.fstat(stream.fileno())
                            if (before.st_size != row["length"] or length != row["length"] or digest.hexdigest() != row["sha256"]
                                    or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns)):
                                result["changed"] += 1
                    except SetupCancelled:
                        raise
                    except FileNotFoundError:
                        result["missing"] += 1
                    except (OSError, ValueError):
                        result["unreadable"] += 1
                    result["checked"] += 1
                    notify("integrity_check", "Installierte Dateien werden mit den ursprünglichen Download-Prüfsummen verglichen …", result["checked"] * 100 // result["total"])
                interrupted(cancel)
                result["healthy"] = not (result["missing"] or result["changed"] or result["unreadable"])
                return result
        except SetupCancelled:
            raise
        except (OSError, ValueError, TypeError, UnicodeError):
            raise SetupError(UNAVAILABLE) from None


def record_installation(game, *, game_id="msfs2024"):
    """Called only at successful download completion, before runtime publication.

    Preserve package identity separately so damage to MicrosoftGame.Config can
    still be repaired. This never invents digests for existing game files.
    """
    from .game_update import installed
    with baseline(game) as (root, index):
        configs = [row for row in index["files"] if row["name"] in {"MicrosoftGame.Config", "MicrosoftGame.config"}]
        if len(configs) != 1 or configs[0]["length"] > 1024 * 1024:
            raise ValueError()
        row = configs[0]
        with os.fdopen(_file(root, row["name"]), "rb") as stream:
            data = stream.read(1024 * 1024 + 1)
        if len(data) != row["length"] or hashlib.sha256(data).hexdigest() != row["sha256"]:
            raise ValueError()
        identity = installed(game, data=data, game_id=game_id)
        encoded = json.dumps(index, sort_keys=True, separators=(",", ":")).encode()
        value = {"format": 1, "index_sha256": hashlib.sha256(encoded).hexdigest(), "identity": identity}
        if game_id != "msfs2024":
            value.update(format=2, game_id=game_id)
        directory = _open(root, ".xodus-resume", directory=True)
        temporary = "installed.json.new"
        try:
            # Keep the write anchored too; never follow a replaced journal path.
            try:
                os.unlink(temporary, dir_fd=directory)
            except FileNotFoundError:
                pass
            fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
            with os.fdopen(fd, "w") as stream:
                json.dump(value, stream)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, "installed.json", src_dir_fd=directory, dst_dir_fd=directory)
            os.fsync(directory)
        finally:
            os.close(directory)


def installed_identity(game, *, game_id="msfs2024"):
    from .game_update import installed, version
    try:
        return installed(game, game_id=game_id)
    except ValueError as original:
        try:
            with baseline(game) as (root, index):
                journal = _open(root, ".xodus-resume", directory=True)
                try:
                    saved = _json(journal, "installed.json", 8192)
                finally:
                    os.close(journal)
                digest = hashlib.sha256(json.dumps(index, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
                identity = saved["identity"]
                receipt_bound = (set(saved) == {"format", "index_sha256", "identity"} and saved["format"] == 1 and game_id == "msfs2024") or (
                    set(saved) == {"format", "index_sha256", "identity", "game_id"} and saved["format"] == 2 and saved["game_id"] == game_id)
                if (not receipt_bound
                        or saved["index_sha256"] != digest or not isinstance(identity, dict)
                        or set(identity) != {"name", "publisher", "version"}
                        or not all(isinstance(v, str) and v for v in identity.values())):
                    raise ValueError()
                version(identity["version"])
                return identity
        except (OSError, ValueError, KeyError, TypeError):
            raise original from None
