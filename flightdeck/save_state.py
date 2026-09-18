# SPDX-License-Identifier: MIT
"""Bounded reader/writer for Flightdeck's XDLOCAL1 save interchange format.

This module performs no filesystem or cloud operations. A caller must acquire
the runtime lock, bind the actual game identity and retain a backup before
installing serialized data. Serialization alone is never cloud synchronization.
"""
from __future__ import annotations

from dataclasses import dataclass, field
import hashlib
import hmac
import re
import struct
import uuid

QUOTA = 256 * 1024 * 1024
METADATA_LIMIT = 32 * 1024 * 1024
CONTAINER_LIMIT = 4096
BLOB_LIMIT = 65536
MAGIC = b"XDLOCAL1"


class SaveStateError(ValueError):
    """Malformed or unsupported local data; intentionally omits save contents."""


@dataclass(repr=False)
class Container:
    display_name: str
    modified: int
    blobs: dict[str, bytes] = field(default_factory=dict)


@dataclass(repr=False)
class State:
    generation: int
    containers: dict[str, Container] = field(default_factory=dict)


def _integer(value, maximum):
    if type(value) is not int or not 0 <= value <= maximum:
        raise SaveStateError("Invalid save metadata.")
    return value


def _name(value, *, container=False):
    pattern = r"(?:[A-Za-z0-9_]+/)*[A-Za-z0-9_.-]+" if container else r"[A-Za-z0-9_.-]+"
    if (not isinstance(value, str) or not 0 < len(value) <= 256
            or not re.fullmatch(pattern, value) or ".." in value or value.endswith(".")):
        raise SaveStateError("Unsupported save name.")
    return value


def namespace_key(title_id, scid, xuid):
    """Match the native provider's title/SCID/actual-user binding exactly."""
    _integer(title_id, 0xffffffff)
    _integer(xuid, 0xffffffffffffffff)
    if not title_id or not xuid:
        raise SaveStateError("A game profile is required.")
    try:
        canonical = str(uuid.UUID(scid))
    except (ValueError, AttributeError, TypeError):
        raise SaveStateError("Invalid game identity.") from None
    if not isinstance(scid, str) or scid.lower() != canonical:
        raise SaveStateError("Invalid game identity.")
    data = (b"xodus.localgamesave.namespace.v1\0" + title_id.to_bytes(4, "big")
            + canonical.encode("ascii") + b"\x01" + xuid.to_bytes(8, "big"))
    return hashlib.sha256(data).hexdigest()


def decode(data: bytes) -> State:
    if (not isinstance(data, bytes) or not 56 <= len(data) <= QUOTA + METADATA_LIMIT + 32
            or data[:8] != MAGIC):
        raise SaveStateError("Invalid save file.")
    if not hmac.compare_digest(hashlib.sha256(data[:-32]).digest(), data[-32:]):
        raise SaveStateError("Save checksum mismatch.")
    end, cursor = len(data) - 32, 8

    def take(size):
        nonlocal cursor
        if size < 0 or size > end - cursor:
            raise SaveStateError("Incomplete save file.")
        result = data[cursor:cursor + size]
        cursor += size
        return result

    def number(size):
        return int.from_bytes(take(size), "little")

    def text(limit):
        size = number(4)
        if size > limit:
            raise SaveStateError("Save metadata exceeds its limit.")
        try:
            value = take(size).decode("utf-8")
        except UnicodeError:
            raise SaveStateError("Invalid save text.") from None
        if "\0" in value:
            raise SaveStateError("Invalid save text.")
        return value

    if number(4) != 1:
        raise SaveStateError("Unsupported save format.")
    state = State(number(8))
    count = _integer(number(4), CONTAINER_LIMIT)
    used = total_blobs = 0
    for _ in range(count):
        name = _name(text(256), container=True)
        if name in state.containers:
            raise SaveStateError("Duplicate save container.")
        display = text(4096)
        modified = _integer(number(8), 0x7fffffffffffffff)
        blob_count = _integer(number(4), BLOB_LIMIT - total_blobs)
        total_blobs += blob_count
        entry = Container(display, modified)
        for _ in range(blob_count):
            blob = _name(text(256))
            if blob in entry.blobs:
                raise SaveStateError("Duplicate save blob.")
            size = _integer(number(4), QUOTA - used)
            used += size
            entry.blobs[blob] = take(size)
        state.containers[name] = entry
    if cursor != end:
        raise SaveStateError("Invalid save metadata size.")
    return state


def encode(state: State) -> bytes:
    if not isinstance(state, State) or not isinstance(state.containers, dict):
        raise SaveStateError("Invalid save state.")
    _integer(state.generation, 0xffffffffffffffff)
    _integer(len(state.containers), CONTAINER_LIMIT)
    output = bytearray(MAGIC + struct.pack("<IQI", 1, state.generation, len(state.containers)))
    used = total_blobs = 0

    def text(value, limit):
        if not isinstance(value, str) or "\0" in value:
            raise SaveStateError("Invalid save text.")
        try:
            payload = value.encode("utf-8")
        except UnicodeError:
            raise SaveStateError("Invalid save text.") from None
        if len(payload) > limit:
            raise SaveStateError("Save text exceeds its limit.")
        output.extend(struct.pack("<I", len(payload)))
        output.extend(payload)

    # Validate keys before sorting, so malformed inputs also fail predictably.
    names = [_name(name, container=True) for name in state.containers]
    for name in sorted(names):
        entry = state.containers[name]
        if not isinstance(entry, Container) or not isinstance(entry.blobs, dict):
            raise SaveStateError("Invalid save container.")
        _integer(entry.modified, 0x7fffffffffffffff)
        total_blobs += len(entry.blobs)
        _integer(total_blobs, BLOB_LIMIT)
        text(name, 256)
        text(entry.display_name, 4096)
        output.extend(struct.pack("<QI", entry.modified, len(entry.blobs)))
        blobs = [_name(blob) for blob in entry.blobs]
        for blob in sorted(blobs):
            payload = entry.blobs[blob]
            if not isinstance(payload, bytes) or len(payload) > QUOTA - used:
                raise SaveStateError("Save payload exceeds its limit.")
            used += len(payload)
            text(blob, 256)
            output.extend(struct.pack("<I", len(payload)))
            output.extend(payload)
        # Native caps the whole body at quota plus the metadata allowance;
        # the allowance is not an independent limit on metadata bytes.
        if len(output) > QUOTA + METADATA_LIMIT:
            raise SaveStateError("Save metadata exceeds its limit.")
    output.extend(hashlib.sha256(output).digest())
    return bytes(output)


def content_digest(state: State) -> str:
    """Compare logical data independently of generation and local timestamps."""
    canonical = State(0, {name: Container(entry.display_name, 0, entry.blobs)
                          for name, entry in state.containers.items()})
    return hashlib.sha256(encode(canonical)).hexdigest()
