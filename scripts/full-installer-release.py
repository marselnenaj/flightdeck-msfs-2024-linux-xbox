#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Combine verified source/native archives into a deterministic full installer.

Inputs are inspected in memory, never extracted or executed. The source archive
supplies the native lock; no mutable checkout lock or network lookup is used.
"""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE_ROOT = "flightdeck-linux/"
NATIVE_ROOT = SOURCE_ROOT + "flightdeck/resources/native/"
NATIVE_FEATURES = ("connected-storage-read-v1", "connected-storage-sync-v1")
NATIVE_FILES = frozenset(("bin/xodus-cli", "bin/xodus-service", "bin/flightdeck-connected-storage.exe",
                          "runtime/xgameruntime.dll",
                          "builtin/x86_64-windows/xodus_store_test.dll",
                          "builtin/x86_64-unix/xodus_store_test.so"))
NATIVE_MEMBERS = NATIVE_FILES | {"manifest.json", "THIRD-PARTY-NOTICES.txt"}
EXECUTABLE_NATIVE = frozenset(name for name in NATIVE_FILES if name.startswith("bin/"))
SOURCE_ARCHIVE_MAX = 64 * 1024 * 1024
SOURCE_FILE_MAX = 16 * 1024 * 1024
SOURCE_TOTAL_MAX = 64 * 1024 * 1024
NATIVE_ARCHIVE_MAX = 256 * 1024 * 1024
NATIVE_FILE_MAX = 128 * 1024 * 1024
NATIVE_NOTICE_MAX = 16 * 1024 * 1024
NATIVE_TOTAL_MAX = 256 * 1024 * 1024
JSON_MAX = 1024 * 1024
MEMBER_MAX = 4096
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


def read_archive(data, *, native=False):
    contents = {}
    total = 0
    try:
        # Streaming traversal avoids retaining attacker-controlled header lists.
        with tarfile.open(fileobj=io.BytesIO(data), mode="r|gz") as archive:
            for member in archive:
                if len(contents) >= (len(NATIVE_MEMBERS) if native else MEMBER_MAX):
                    raise ValueError("Archive contains too many members")
                name = safe_name(member.name)
                if name in contents:
                    raise ValueError("Duplicate archive member")
                if (member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE)
                        or member.sparse is not None
                        or any(key.startswith("GNU.sparse") for key in member.pax_headers)):
                    raise ValueError("Archives may contain only regular files, not links or special members")
                if native:
                    if name not in NATIVE_MEMBERS:
                        raise ValueError("Unexpected native archive file")
                    maximum = (JSON_MAX if name == "manifest.json" else
                               NATIVE_NOTICE_MAX if name == "THIRD-PARTY-NOTICES.txt" else NATIVE_FILE_MAX)
                else:
                    if not name.startswith(SOURCE_ROOT) or name == SOURCE_ROOT.rstrip("/"):
                        raise ValueError("Source archive must have the flightdeck-linux root")
                    if name.startswith(NATIVE_ROOT) or name == NATIVE_ROOT.rstrip("/"):
                        raise ValueError("Source archive already contains the reserved native path")
                    maximum = SOURCE_FILE_MAX
                total += member.size
                if member.size < 0 or member.size > maximum:
                    raise ValueError("Archive member exceeds its size limit")
                if total > (NATIVE_TOTAL_MAX if native else SOURCE_TOTAL_MAX):
                    raise ValueError("Archive exceeds the expanded total size limit")
                extracted = archive.extractfile(member)
                with extracted:
                    payload = extracted.read(member.size + 1)
                if len(payload) != member.size:
                    raise ValueError("Archive member is truncated")
                contents[name] = payload
    except (tarfile.TarError, EOFError, OSError):
        raise ValueError("Archive is malformed or truncated") from None
    reject_parent_files(contents)
    return contents


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


def verify_sources(contents):
    name = SOURCE_ROOT + "SOURCE-MANIFEST.json"
    if name not in contents:
        raise ValueError("Source archive is missing SOURCE-MANIFEST.json")
    manifest = read_json(contents[name], "Source manifest")
    if type(manifest.get("format")) is not int or manifest["format"] != 1 or manifest.get("status") != "PASS":
        raise ValueError("Source manifest is not a verified format-1 manifest")
    hashes = hash_map(manifest.get("files"), "Source manifest")
    expected = {SOURCE_ROOT + entry for entry in hashes}
    if set(contents) != expected | {name} or name in expected:
        raise ValueError("Source archive files differ from SOURCE-MANIFEST.json")
    for relative, checksum in hashes.items():
        if digest(contents[SOURCE_ROOT + relative]) != checksum:
            raise ValueError("Source file hash differs from SOURCE-MANIFEST.json")
    lock_name = SOURCE_ROOT + "compat/bootstrap.lock.json"
    if lock_name not in expected:
        raise ValueError("Verified source archive is missing compat/bootstrap.lock.json")
    lock = read_json(contents[lock_name], "Bootstrap lock")
    if type(lock.get("format")) is not int or lock["format"] != 1 or not isinstance(lock.get("native"), dict):
        raise ValueError("Bootstrap lock is invalid")
    native = lock["native"]
    features = native.get("features")
    if (not isinstance(features, list) or any(not isinstance(item, str) for item in features)
            or not set(NATIVE_FEATURES).issubset(features)):
        raise ValueError("Native lock is missing a required ConnectedStorage capability")
    native_hashes = hash_map(native.get("files"), "Native lock")
    if set(native_hashes) != NATIVE_FILES:
        raise ValueError("Native lock must name exactly the six supported binaries")
    for field in ("archive_sha256", "notice_sha256"):
        if not isinstance(native.get(field), str) or not HASH.fullmatch(native[field]):
            raise ValueError("Native lock has an invalid archive or notice SHA256")
    return native, len(hashes)


def verify_native(contents, lock):
    if set(contents) != NATIVE_MEMBERS:
        raise ValueError("Native archive must contain exactly six binaries, manifest and notices")
    manifest = read_json(contents["manifest.json"], "Native manifest")
    if (set(manifest) != {"format", "files"} or type(manifest.get("format")) is not int
            or manifest["format"] != 1 or manifest.get("files") != lock["files"]):
        raise ValueError("Native manifest differs from the verified source lock")
    for name, checksum in {**lock["files"], "THIRD-PARTY-NOTICES.txt": lock["notice_sha256"]}.items():
        if digest(contents[name]) != checksum:
            raise ValueError("Native binary or notice hash differs from the verified source lock")


def write_archive(output, contents):
    output = Path(output)
    if output.is_symlink() or output.exists():
        raise ValueError("Output already exists; choose a new archive path")
    output = output.resolve()
    build = ROOT.resolve() / "build"
    if not output.is_relative_to(build) or output == build:
        raise ValueError("Full installers must be written under the ignored root build directory")
    output.parent.mkdir(parents=True, exist_ok=True)
    # Build the whole archive privately, then atomically publish without replacing
    # an existing/racing destination. No archive member is extracted to disk.
    descriptor, temporary = tempfile.mkstemp(prefix=".flightdeck-full-", dir=output.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            os.fchmod(stream.fileno(), 0o644)
            with gzip.GzipFile(fileobj=stream, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                    for name, data in sorted(contents.items()):
                        entry = tarfile.TarInfo(name)
                        entry.size = len(data)
                        entry.mtime = entry.uid = entry.gid = 0
                        entry.uname = entry.gname = ""
                        executable = (name[len(NATIVE_ROOT):] in EXECUTABLE_NATIVE if name.startswith(NATIVE_ROOT)
                                      else name.endswith(".desktop") or data.startswith(b"#!"))
                        entry.mode = 0o755 if executable else 0o644
                        archive.addfile(entry, io.BytesIO(data))
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, output)
    finally:
        os.unlink(temporary)
    return output


def create(source, native, output):
    source_bytes = read_regular(source, SOURCE_ARCHIVE_MAX)
    sources = read_archive(source_bytes)
    lock, source_count = verify_sources(sources)
    native_bytes = read_regular(native, NATIVE_ARCHIVE_MAX)
    if digest(native_bytes) != lock["archive_sha256"]:
        raise ValueError("Native archive hash differs from the verified source lock")
    natives = read_archive(native_bytes, native=True)
    verify_native(natives, lock)
    combined = {**sources, **{NATIVE_ROOT + name: data for name, data in natives.items()}}
    reject_parent_files(combined)
    destination = write_archive(output, combined)
    report = {"status": "PASS", "source_files": source_count, "native_files": len(natives),
              "archive": str(destination), "sha256": digest(destination.read_bytes())}
    print(json.dumps(report))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        create(arguments.source, arguments.native, arguments.output)
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + "\n")
