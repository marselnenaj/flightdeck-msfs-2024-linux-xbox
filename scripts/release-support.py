# SPDX-License-Identifier: MIT
"""Bounded validation of compatibility components used by native release tooling."""
import hashlib
import io
import json
import os
from pathlib import PurePosixPath
import re
import stat
import tarfile

NATIVE_FEATURES = ("connected-storage-read-v1", "connected-storage-sync-v1")
NATIVE_FILES = frozenset(("bin/xodus-cli", "bin/xodus-service", "bin/flightdeck-connected-storage.exe",
                          "runtime/xgameruntime.dll",
                          "builtin/x86_64-windows/xodus_store_test.dll",
                          "builtin/x86_64-unix/xodus_store_test.so"))
NATIVE_MEMBERS = NATIVE_FILES | {"manifest.json", "THIRD-PARTY-NOTICES.txt"}
NATIVE_ARCHIVE_MAX = 256 * 1024 * 1024
NATIVE_FILE_MAX = 128 * 1024 * 1024
NATIVE_NOTICE_MAX = 16 * 1024 * 1024
NATIVE_TOTAL_MAX = 256 * 1024 * 1024
JSON_MAX = 1024 * 1024
HASH = re.compile(r"[0-9a-f]{64}\Z")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate JSON key")
        result[key] = value
    return result


def reject_constant(_value):
    raise ValueError("Non-finite JSON number")


def read_json(data, label):
    if len(data) > JSON_MAX:
        raise ValueError(label + " exceeds the JSON size limit")
    try:
        value = json.loads(data, object_pairs_hook=unique_object,
                           parse_constant=reject_constant)
    except (ValueError, UnicodeError):
        raise ValueError(label + " is invalid JSON") from None
    if not isinstance(value, dict):
        raise ValueError(label + " must be an object")
    return value


def safe_name(name):
    if (not isinstance(name, str) or not name or len(name) > 1024
            or "\\" in name or ":" in name
            or any(ord(character) < 32 or ord(character) == 127 for character in name)
            or name.startswith("/") or any(part in ("", ".", "..") for part in name.split("/"))):
        raise ValueError("Archive path must be a unique canonical relative file path")
    return name


def read_regular(path, maximum):
    # O_NONBLOCK prevents a supplied FIFO from blocking before fstat rejects it.
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > maximum:
            raise ValueError("Input is not a regular archive within the size limit")
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise ValueError("Archive exceeds the compressed size limit")
    return data


def reject_parent_files(contents):
    for name in contents:
        if any(str(parent) in contents for parent in PurePosixPath(name).parents):
            raise ValueError("A regular file cannot also be a parent directory")


def hash_map(value, label):
    if not isinstance(value, dict) or not value:
        raise ValueError(label + " must contain file hashes")
    for name, checksum in value.items():
        safe_name(name)
        if not isinstance(checksum, str) or not HASH.fullmatch(checksum):
            raise ValueError(label + " contains an invalid SHA256")
    return value


def verify_native(contents, lock):
    validate_native_lock(lock)
    if set(contents) != NATIVE_MEMBERS:
        raise ValueError("Native archive must contain exactly six binaries, manifest and notices")
    manifest = read_json(contents["manifest.json"], "Native manifest")
    if (set(manifest) != {"format", "files"} or type(manifest.get("format")) is not int
            or manifest["format"] != 1 or manifest.get("files") != lock["files"]):
        raise ValueError("Native manifest differs from the verified source lock")
    for name, checksum in {**lock["files"], "THIRD-PARTY-NOTICES.txt": lock["notice_sha256"]}.items():
        if digest(contents[name]) != checksum:
            raise ValueError("Native binary or notice hash differs from the verified source lock")


def read_archive(data):
    contents = {}
    total = 0
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r|gz") as archive:
            for member in archive:
                if len(contents) >= len(NATIVE_MEMBERS):
                    raise ValueError("Archive contains too many members")
                name = safe_name(member.name)
                if name in contents or name not in NATIVE_MEMBERS:
                    raise ValueError("Unexpected or duplicate native archive file")
                if (member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE)
                        or member.sparse is not None
                        or any(key.startswith("GNU.sparse") for key in member.pax_headers)):
                    raise ValueError("Archives may contain only regular files")
                maximum = (JSON_MAX if name == "manifest.json" else
                           NATIVE_NOTICE_MAX if name == "THIRD-PARTY-NOTICES.txt" else NATIVE_FILE_MAX)
                total += member.size
                if member.size < 0 or member.size > maximum or total > NATIVE_TOTAL_MAX:
                    raise ValueError("Archive exceeds its expanded size limit")
                with archive.extractfile(member) as extracted:
                    payload = extracted.read(member.size + 1)
                if len(payload) != member.size:
                    raise ValueError("Archive member is truncated")
                contents[name] = payload
    except (tarfile.TarError, EOFError, OSError):
        raise ValueError("Archive is malformed or truncated") from None
    reject_parent_files(contents)
    return contents



def validate_native_lock(lock):
    features = lock.get("features")
    if (not isinstance(features, list) or any(not isinstance(item, str) for item in features)
            or not set(NATIVE_FEATURES).issubset(features)):
        raise ValueError("Native lock is missing a required ConnectedStorage capability")
    if set(hash_map(lock.get("files"), "Native lock")) != NATIVE_FILES:
        raise ValueError("Native lock must name exactly the six supported binaries")
    for field in ("archive_sha256", "notice_sha256"):
        if not isinstance(lock.get(field), str) or not HASH.fullmatch(lock[field]):
            raise ValueError("Native lock has an invalid archive or notice SHA256")



def graphics_files(lock_bytes, directory):
    if directory is None:
        return {}
    lock = read_json(lock_bytes, "Graphics lock")
    files = hash_map(lock.get("files"), "Graphics lock")
    base = hash_map(lock.get("base"), "Graphics base")
    if lock.get("schema") != 2 or set(files) != {"d3d12.dll", "d3d12core.dll", "dxgi.dll", "d3d11.dll", "d3d10core.dll"} or set(base) != set(files):
        raise ValueError("Invalid graphics lock")
    expected = {**files, "LICENSE": lock.get("license_sha256")}
    if not isinstance(expected["LICENSE"], str) or not HASH.fullmatch(expected["LICENSE"]):
        raise ValueError("Invalid graphics license hash")
    if directory.is_symlink() or set(p.name for p in directory.iterdir()) != set(expected) | {"manifest.json"}:
        raise ValueError("Unexpected graphics bundle contents")
    values = {name: read_regular(directory / name, NATIVE_FILE_MAX) for name in expected}
    if any(digest(values[name]) != checksum for name, checksum in expected.items()):
        raise ValueError("Graphics bundle differs from source lock")
    manifest = read_regular(directory / "manifest.json", JSON_MAX)
    if read_json(manifest, "Graphics manifest") != {"schema": 2, "base": base, "files": files}:
        raise ValueError("Graphics manifest differs from source lock")
    values["manifest.json"] = manifest
    return values
