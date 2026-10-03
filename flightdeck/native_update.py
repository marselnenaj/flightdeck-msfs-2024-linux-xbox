# SPDX-License-Identifier: MIT
"""Recognize native packages without importing code from the incoming release."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import stat

PACKAGE = "FLIGHTDECK-PACKAGE.json"
MAX_BINARY = 128 * 1024 * 1024
INVALID = "Das Updatepaket enthält ungültige Dateien."


class PackageError(ValueError):
    pass


def no_links(path):
    if any(item.is_symlink() for item in (path, *path.parents)):
        raise PackageError(INVALID)


def regular(path, maximum):
    no_links(path)
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > maximum:
            raise PackageError(INVALID)
        result = stream.read(maximum + 1)
    if len(result) > maximum:
        raise PackageError(INVALID)
    return result


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise PackageError(INVALID)
        result[key] = value
    return result


def invalid_constant(_value):
    raise PackageError(INVALID)


def executable(source, expected_version=None, *, extracted=False):
    """None means legacy; a present but invalid native manifest is always fatal.

    The downloaded archive has already passed the whole-file GitHub digest.
    This binds its executable to its native manifest before execution. That
    release's installer subsequently verifies its own complete payload format.
    Only private, freshly extracted files have their execute bit restored.
    """
    source = Path(os.path.abspath(source))
    manifest = source / PACKAGE
    no_links(manifest)
    if not manifest.exists():
        return None
    try:
        package = json.loads(regular(manifest, 2 * 1024 * 1024),
                             object_pairs_hook=unique, parse_constant=invalid_constant)
        if (not isinstance(package, dict) or type(package.get("schema")) is not int
                or package["schema"] != 1 or package.get("kind") != "rust-launcher"
                or not isinstance(package.get("version"), str) or not 1 <= len(package["version"]) <= 128
                or not isinstance(package.get("files"), dict) or not 1 <= len(package["files"]) <= 4096):
            raise PackageError(INVALID)
        if expected_version is not None and package["version"] != expected_version:
            raise PackageError("Die Version im Updatepaket stimmt nicht mit GitHub überein.")
        for name, checksum in package["files"].items():
            if (not name or len(name) > 1024 or "\\" in name or ":" in name
                    or any(ord(c) < 32 or ord(c) == 127 for c in name)
                    or any(part in {"", ".", ".."} for part in name.split("/"))
                    or not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum)):
                raise PackageError(INVALID)
        target = source / "bin/flightdeck"
        data = regular(target, MAX_BINARY)
        if (len(data) < 64 or data[:6] != b"\x7fELF\x02\x01" or data[16:18] not in (b"\x02\0", b"\x03\0")
                or data[18:20] != b"\x3e\0" or hashlib.sha256(data).hexdigest() != package["files"].get("bin/flightdeck")):
            raise PackageError(INVALID)
        if extracted:
            # Extraction never creates hard links. Refuse one before chmod.
            if target.stat().st_nlink != 1:
                raise PackageError(INVALID)
            target.chmod(0o700)
        if not os.access(target, os.X_OK):
            raise PackageError(INVALID)
        return target
    except PackageError:
        raise
    except (OSError, ValueError, TypeError):
        raise PackageError(INVALID) from None
