# SPDX-License-Identifier: MIT
"""Read-only Xbox ConnectedStorage transport and isolated download snapshots.

Protocol reference: xodus-gaming/xodus, revision
0670e25aeb0e0e9f800f8f2f4968ae3b681842a7, docs/xbox/titlestorage.md.
Read-route/contract-107 schema evidence: billynothingelse/xbcsmgr, revision
efa52379e533e567c42af3a7dd5719a57c8211fe, TitleStorageService and its models.
Pagination evidence: ProjectSparkDev/xbcsmgr, revision
ead66358e5ef60808cf932434578b906e2d25fae, TitleStorageService.GetBlobMetadataPartial.
This is an independent implementation of observed wire formats, not copied
implementation code. Fenced writes are implemented separately in cloud_write;
this reader never synthesizes conditional-write headers.

A snapshot is an optimistically rechecked download, NOT a locked cloud-sync
baseline. This module never imports into GameSave, acquires a lock or uploads.
Account IDs, save names, bodies and authorization must never be logged.
"""
from __future__ import annotations

import ctypes
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import time
from typing import Mapping, Protocol
from urllib.parse import parse_qsl, quote, urlencode, urlsplit
import uuid


HOST = "titlestorage.xboxlive.com"
ORIGIN = "https://" + HOST
_GUID = re.compile(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}\Z")
_ERRORS = {
    "invalid_scope": "Cloud-save account and title binding is invalid.",
    "invalid_request": "The cloud-save request is not an allowed read operation.",
    "invalid_response": "The cloud-save service returned an invalid response.",
    "authentication": "Cloud-save authentication is required or was rejected.",
    "not_found": "Cloud-save storage was not found; this is not an empty inventory.",
    "http_error": "The cloud-save service did not complete the read request.",
    "transport": "The cloud-save connection failed.",
    "bounds": "The cloud-save response exceeds this reader's supported limits.",
    "unsupported_paging": "Cloud-save pagination is not yet supported by this reader.",
    "changed": "Cloud saves changed during download. No snapshot was published.",
    "cancelled": "The cloud-save read was cancelled.",
    "deadline": "The cloud-save read exceeded its time limit.",
    "local_storage": "The private cloud-save snapshot could not be written safely.",
    "durability_unknown": "A complete snapshot was published, but its disk flush failed.",
}


class CloudStorageError(Exception):
    """Static error codes/messages: never expose response bodies or private URLs."""

    def __init__(self, code: str):
        self.code = code
        super().__init__(_ERRORS[code])


@dataclass(frozen=True, repr=False)
class Scope:
    xuid: str
    scid: str
    package_family_name: str
    title_id: int

    def __post_init__(self):
        if (not isinstance(self.xuid, str)
                or not re.fullmatch(r"[0-9]{1,20}", self.xuid)
                or int(self.xuid) == 0 or int(self.xuid) > 2**64 - 1
                or not isinstance(self.scid, str) or not _GUID.fullmatch(self.scid)
                or not isinstance(self.package_family_name, str)
                or not re.fullmatch(r"[A-Za-z0-9_.-]{1,255}", self.package_family_name)
                or type(self.title_id) is not int or not 0 < self.title_id < 2**32):
            raise CloudStorageError("invalid_scope")
        object.__setattr__(self, "scid", self.scid.lower())

    @property
    def binding(self) -> str:
        digest = hashlib.sha256(b"Flightdeck ConnectedStorage read snapshot v1\0")
        for value in (self.xuid, self.scid, self.package_family_name, str(self.title_id)):
            data = value.encode("utf-8")
            digest.update(len(data).to_bytes(4, "big"))
            digest.update(data)
        return digest.hexdigest()

    @property
    def base_url(self) -> str:
        return f"{ORIGIN}/connectedstorage/users/xuid({self.xuid})/scids/{self.scid}"


@dataclass(frozen=True, repr=False)
class Request:
    url: str
    headers: Mapping[str, str] = field(default_factory=dict)
    method: str = "GET"


@dataclass(frozen=True, repr=False)
class Response:
    status: int
    headers: Mapping[str, str]
    body: bytes


class Transport(Protocol):
    # Production uses cloud_runtime.HelperTransport: a fixed-operation native
    # child owns Xbox authentication. Python never receives its credentials.
    # Implementations must bound the entire call by timeout, interrupt on the
    # session's cancellation event and never follow redirects.
    def __call__(self, request: Request, *, timeout: float, max_bytes: int) -> Response: ...


def _allowed_url(url: str):
    try:
        parts = urlsplit(url)
        if (parts.scheme != "https" or parts.netloc != HOST or parts.fragment
                or not parts.path.startswith("/connectedstorage/users/xuid(")
                or any(ord(c) <= 32 or ord(c) >= 127 for c in url)):
            raise ValueError()
        if parts.query:
            # Continuations are values on the same index route, never URLs or
            # arbitrary query parameters on a container/atom endpoint.
            if not re.fullmatch(r"/connectedstorage/users/xuid\([0-9]{1,20}\)/scids/[0-9a-f-]{36}", parts.path):
                raise ValueError()
            parse_index_query(parts.query)
        return parts
    except (ValueError, TypeError):
        raise CloudStorageError("invalid_request") from None


def index_query(skip_items: int, continuation_token: str) -> str:
    """Canonical fixed-route continuation; opaque values are percent encoded."""
    if type(skip_items) is not int or not 0 < skip_items <= 4096:
        raise CloudStorageError("invalid_request")
    try:
        _text(continuation_token, maximum=8192)
    except CloudStorageError:
        raise CloudStorageError("invalid_request") from None
    return urlencode({"skipItems": str(skip_items), "continuationToken": continuation_token},
                     quote_via=quote, safe="")


def parse_index_query(query: str) -> tuple[int, str]:
    """Used by the native pipe adapter to accept only the canonical query."""
    try:
        pairs = parse_qsl(query, keep_blank_values=True, strict_parsing=True,
                          encoding="utf-8", errors="strict", max_num_fields=2)
        if len(pairs) != 2 or [key for key, _ in pairs] != ["skipItems", "continuationToken"]:
            raise ValueError()
        raw_skip, token = pairs[0][1], pairs[1][1]
        if not re.fullmatch(r"[1-9][0-9]{0,3}", raw_skip):
            raise ValueError()
        skip = int(raw_skip)
        if index_query(skip, token) != query:
            raise ValueError()
        return skip, token
    except (ValueError, TypeError, UnicodeError):
        raise CloudStorageError("invalid_request") from None


def _headers(values: Mapping[str, str]) -> dict[str, str]:
    result = {}
    try:
        for name, value in values.items():
            if (not isinstance(name, str) or not isinstance(value, str)
                    or not re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+", name)
                    or any(ord(c) < 32 or ord(c) == 127 for c in value)
                    or name.lower() in result):
                raise ValueError()
            result[name.lower()] = value
    except (ValueError, TypeError, AttributeError):
        raise CloudStorageError("invalid_response") from None
    return result


@dataclass(frozen=True)
class Limits:
    max_pages: int = 64
    max_containers: int = 4096
    max_blobs: int = 65536
    max_blob_bytes: int = 64 * 1024 * 1024
    max_total_bytes: int = 256 * 1024 * 1024
    max_json_bytes: int = 4 * 1024 * 1024
    timeout_seconds: float = 20
    deadline_seconds: float = 180

    def __post_init__(self):
        for value in (self.max_pages, self.max_containers, self.max_blobs, self.max_blob_bytes,
                      self.max_total_bytes, self.max_json_bytes):
            if type(value) is not int or value <= 0:
                raise ValueError("Limits must be positive integers.")
        for value in (self.timeout_seconds, self.deadline_seconds):
            if not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
                raise ValueError("Timeouts must be positive finite numbers.")


@dataclass(frozen=True, repr=False)
class Container:
    # Exact index wire filename, not an inferred native GameSave container name.
    name: str
    display_name: str
    etag: str
    client_file_time: int
    size: int


@dataclass(frozen=True, repr=False)
class Inventory:
    scope_binding: str
    containers: tuple[Container, ...]

    @property
    def container_count(self):
        return len(self.containers)

    @property
    def total_bytes(self):
        return sum(item.size for item in self.containers)


@dataclass(frozen=True, repr=False)
class Atom:
    atom: str
    name: str
    # The contract-107 dictionary has no per-atom size. Container totals still
    # bound downloads, which must sum exactly to the advertised container size.
    size: int | None


@dataclass(frozen=True, repr=False)
class Snapshot:
    path: Path
    scope_binding: str
    container_count: int
    blob_count: int
    total_bytes: int
    consistency: str = "rechecked-unlocked"


@dataclass(frozen=True, repr=False)
class _CachedBlob:
    atom: Atom
    data: bytes
    sha256: str


@dataclass(frozen=True, repr=False)
class _CachedContainer:
    container: Container
    blobs: tuple[_CachedBlob, ...]


@dataclass(frozen=True, repr=False)
class _SnapshotCache:
    # Internal only: cloud_cache constructs this from a scope-bound snapshot
    # whose manifest seal and every original payload hash have been verified.
    scope_binding: str
    containers: tuple[_CachedContainer, ...]


def _cache_rows(cache, scope, limits, check):
    if cache is None:
        return {}
    if type(cache) is not _SnapshotCache or cache.scope_binding != scope.binding:
        raise CloudStorageError("invalid_scope")
    rows, total, blobs = {}, 0, 0
    for row in cache.containers:
        check()
        if type(row) is not _CachedContainer or type(row.container) is not Container:
            raise CloudStorageError("invalid_response")
        item = row.container
        _text(item.name, maximum=1024)
        _text(item.display_name, empty=True)
        _text(item.etag, maximum=1024)
        _number(item.client_file_time, 2**64 - 1)
        _number(item.size, limits.max_total_bytes)
        if item.name in rows:
            raise CloudStorageError("invalid_response")
        names, atoms, size = set(), set(), 0
        for blob in row.blobs:
            if (type(blob) is not _CachedBlob or type(blob.atom) is not Atom
                    or type(blob.data) is not bytes or blob.atom.size != len(blob.data)
                    or not isinstance(blob.atom.atom, str) or not _GUID.fullmatch(blob.atom.atom)
                    or blob.sha256 != hashlib.sha256(blob.data).hexdigest()):
                raise CloudStorageError("invalid_response")
            _text(blob.atom.name)
            _number(len(blob.data), limits.max_blob_bytes)
            if blob.atom.name in names or blob.atom.atom.lower() in atoms:
                raise CloudStorageError("invalid_response")
            names.add(blob.atom.name); atoms.add(blob.atom.atom.lower())
            size += len(blob.data); blobs += 1
        if size != item.size:
            raise CloudStorageError("invalid_response")
        rows[item.name] = row
        total += size
        if len(rows) > limits.max_containers or blobs > limits.max_blobs or total > limits.max_total_bytes:
            raise CloudStorageError("bounds")
    return rows


def _representation(headers: Mapping[str, str], maximum: int):
    if headers.get("content-encoding", "identity").lower() != "identity":
        raise CloudStorageError("invalid_response")
    length = headers.get("content-length")
    if length is not None:
        if not re.fullmatch(r"[0-9]{1,20}", length):
            raise CloudStorageError("invalid_response")
        if int(length) > maximum:
            raise CloudStorageError("bounds")


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError()
        result[key] = value
    return result


def _json(body: bytes):
    try:
        value = json.loads(body.decode("utf-8"), object_pairs_hook=_object,
                           parse_constant=lambda _: (_ for _ in ()).throw(ValueError()))
        if not isinstance(value, dict):
            raise ValueError()
        return value
    except (ValueError, UnicodeError, RecursionError):
        raise CloudStorageError("invalid_response") from None


def _text(value, *, maximum=256, empty=False):
    if (not isinstance(value, str) or (not empty and not value)
            or any(ord(c) < 32 or ord(c) == 127 for c in value)):
        raise CloudStorageError("invalid_response")
    try:
        if len(value.encode("utf-8")) > maximum:
            raise CloudStorageError("bounds")
    except UnicodeError:
        raise CloudStorageError("invalid_response") from None
    return value


def _number(value, maximum):
    if type(value) is not int or not 0 <= value <= maximum:
        raise CloudStorageError("invalid_response")
    return value


def _file_time(value):
    """Normalize RFC3339/.NET DateTimeOffset to exact 100 ns Windows ticks."""
    if type(value) is int:
        return _number(value, 2**64 - 1)
    if not isinstance(value, str):
        raise CloudStorageError("invalid_response")
    match = re.fullmatch(r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.([0-9]{1,7}))?(Z|[+-][0-9]{2}:[0-9]{2})", value)
    if not match:
        raise CloudStorageError("invalid_response")
    try:
        stamp, fraction, zone = match.groups()
        offset = 0
        if zone != "Z":
            hours, minutes = map(int, zone[1:].split(":"))
            if hours > 14 or minutes > 59 or (hours == 14 and minutes):
                raise ValueError()
            offset = (hours * 60 + minutes) * (1 if zone[0] == "+" else -1)
        date = datetime.strptime(stamp, "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
        delta = date - timedelta(minutes=offset) - datetime(1601, 1, 1, tzinfo=timezone.utc)
        ticks = (delta.days * 86400 + delta.seconds) * 10_000_000 + int((fraction or "0").ljust(7, "0"))
        return _number(ticks, 2**64 - 1)
    except (ValueError, OverflowError):
        raise CloudStorageError("invalid_response") from None


class _Read:
    def __init__(self, client, cancel):
        self.client = client
        self.cancel = cancel
        self.deadline = time.monotonic() + client.limits.deadline_seconds

    def check(self):
        if self.cancel is not None and self.cancel.is_set():
            raise CloudStorageError("cancelled")
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise CloudStorageError("deadline")
        return remaining

    def get(self, suffix: str, maximum: int) -> bytes:
        timeout = min(self.check(), self.client.limits.timeout_seconds)
        request = Request(self.client.scope.base_url + suffix, {
            "x-xbl-pfn": self.client.scope.package_family_name,
            "x-xbl-contract-version": "107",
            "Accept-Encoding": "identity",
        })
        _allowed_url(request.url)
        try:
            response = self.client.transport(request, timeout=timeout, max_bytes=maximum)
        except CloudStorageError:
            raise
        except Exception:
            raise CloudStorageError("transport") from None
        self.check()
        if not isinstance(response, Response):
            raise CloudStorageError("invalid_response")
        if type(response.status) is not int:
            raise CloudStorageError("invalid_response")
        if response.status in (401, 403):
            raise CloudStorageError("authentication")
        if response.status == 404:
            raise CloudStorageError("not_found")
        if response.status != 200:
            raise CloudStorageError("http_error")
        headers = _headers(response.headers)
        _representation(headers, maximum)
        if type(response.body) is not bytes:
            raise CloudStorageError("invalid_response")
        if len(response.body) > maximum:
            raise CloudStorageError("bounds")
        if "content-length" in headers and int(headers["content-length"]) != len(response.body):
            raise CloudStorageError("invalid_response")
        return response.body

    def inventory(self) -> Inventory:
        containers = []
        names, continuations = set(), set()
        expected_total = None
        size = 0
        suffix = ""
        try:
            for _page in range(self.client.limits.max_pages):
                obj = _json(self.get(suffix, self.client.limits.max_json_bytes))
                rows, paging = obj["blobs"], obj["pagingInfo"]
                if not isinstance(rows, list) or not isinstance(paging, dict):
                    raise ValueError()
                continuation = paging["continuationToken"]
                if continuation is not None:
                    _text(continuation, maximum=8192)
                total = _number(paging["totalItems"], self.client.limits.max_containers)
                if expected_total is None:
                    expected_total = total
                elif total != expected_total:
                    raise CloudStorageError("changed")
                if len(containers) + len(rows) > total:
                    raise ValueError()
                for row in rows:
                    name = _text(row["fileName"], maximum=1024)
                    # Duplicate names across pages are not another container.
                    if name in names or name in (".", ".."):
                        raise ValueError()
                    names.add(name)
                    item = Container(name, _text(row["displayName"], empty=True),
                                     _text(row["etag"], maximum=1024),
                                     _file_time(row["clientFileTime"]),
                                     _number(row["size"], self.client.limits.max_total_bytes))
                    containers.append(item)
                    size += item.size
                    if size > self.client.limits.max_total_bytes:
                        raise CloudStorageError("bounds")
                if continuation is None:
                    if len(containers) != expected_total:
                        raise ValueError()
                    return Inventory(self.client.scope.binding, tuple(sorted(containers, key=lambda item: item.name)))
                if not rows or len(containers) >= expected_total or continuation in continuations:
                    raise ValueError()
                continuations.add(continuation)
                suffix = "?" + index_query(len(containers), continuation)
            raise CloudStorageError("bounds")
        except (KeyError, TypeError, ValueError):
            raise CloudStorageError("invalid_response") from None

    def atoms(self, container: Container) -> tuple[Atom, ...]:
        # Read the exact filename supplied by the index. /savedgames/<name> is
        # the write route in contract 107, not a prefix to invent for reads.
        obj = _json(self.get("/" + quote(container.name, safe=""), self.client.limits.max_json_bytes))
        try:
            rows = obj["atoms"]
            dictionary = isinstance(rows, dict)
            if dictionary:
                converted = []
                for name, wire_atom in rows.items():
                    if not isinstance(wire_atom, str) or not wire_atom.endswith(",binary"):
                        raise ValueError()
                    converted.append({"name": name, "atom": wire_atom[:-7], "size": None})
                rows = converted
            elif not isinstance(rows, list):
                raise ValueError()
            if len(rows) > self.client.limits.max_blobs:
                raise CloudStorageError("bounds")
            atoms = []
            names, identifiers = set(), set()
            for row in rows:
                name = _text(row["name"])
                atom = row["atom"]
                if not isinstance(atom, str) or not _GUID.fullmatch(atom):
                    raise ValueError()
                # An atom is an opaque wire name despite its UUID syntax.
                # Preserve its case for the GET; compare UUIDs only for dedupe.
                identity = atom.lower()
                if name in names or identity in identifiers:
                    raise ValueError()
                names.add(name)
                identifiers.add(identity)
                size = None if dictionary else _number(row["size"], self.client.limits.max_blob_bytes)
                atoms.append(Atom(atom, name, size))
            if not dictionary and sum(item.size for item in atoms) != container.size:
                raise CloudStorageError("changed")
            return tuple(sorted(atoms, key=lambda item: item.name))
        except (KeyError, TypeError, ValueError):
            raise CloudStorageError("invalid_response") from None


class CloudStorageClient:
    def __init__(self, scope: Scope, transport: Transport, *, limits: Limits | None = None):
        self.scope = scope
        self.transport = transport
        self.limits = limits or Limits()

    def read_inventory(self, *, cancel=None) -> Inventory:
        """Read a complete index. Errors or partial pages never become empty."""
        return _Read(self, cancel).inventory()

    def download_snapshot(self, parent_directory: Path, *, cancel=None, _cached=None) -> Snapshot:
        """Publish one private generation; never overwrite or import a save.

        Both container versions and downloaded atom lists are re-read afterwards.
        An internal validated cache reuses a whole container only when every
        current inventory field, including its opaque nonempty ETag, matches.
        Remote names remain private manifest metadata; disk paths are generated
        ordinals. A failed or cancelled operation removes only its own staging.
        """
        read = _Read(self, cancel)
        try:
            cached = _cache_rows(_cached, self.scope, self.limits, read.check)
        except CloudStorageError as error:
            if error.code in {"cancelled", "deadline"}:
                raise
            cached = {}  # A corrupt/incompatible cache never weakens a live read.
        root_fd = stage_fd = blobs_fd = None
        staging = ".snapshot-" + uuid.uuid4().hex
        destination = "snapshot-" + uuid.uuid4().hex
        created, published = False, False
        files = []
        try:
            root_fd = _private_directory(parent_directory)
            inventory = read.inventory()
            os.mkdir(staging, mode=0o700, dir_fd=root_fd)
            created = True
            stage_fd = os.open(staging, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root_fd)
            os.mkdir("blobs", mode=0o700, dir_fd=stage_fd)
            blobs_fd = os.open("blobs", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=stage_fd)
            manifest = {"schema": 1, "scope_binding": self.scope.binding,
                        "consistency": "rechecked-unlocked", "containers": []}
            before_atoms = {}
            total_bytes = 0
            for container in inventory.containers:
                read.check()
                previous = cached.get(container.name)
                reusable = previous is not None and previous.container == container
                if reusable:
                    atoms = tuple(blob.atom for blob in previous.blobs)
                else:
                    atoms = read.atoms(container)
                    before_atoms[container.name] = atoms
                if len(files) + len(atoms) > self.limits.max_blobs:
                    raise CloudStorageError("bounds")
                saved = {"name": container.name, "display_name": container.display_name,
                         "etag": container.etag, "client_file_time": container.client_file_time,
                         "size": container.size, "blobs": []}
                container_bytes = 0
                for position, atom in enumerate(atoms):
                    read.check()
                    maximum = atom.size if atom.size is not None else min(
                        self.limits.max_blob_bytes, container.size - container_bytes)
                    data = previous.blobs[position].data if reusable else read.get("/" + atom.atom + ",binary", maximum)
                    if atom.size is not None and len(data) != atom.size:
                        raise CloudStorageError("invalid_response")
                    filename = f"{len(files):08x}.bin"
                    files.append(filename)
                    _write(blobs_fd, filename, data)
                    total_bytes += len(data)
                    container_bytes += len(data)
                    saved["blobs"].append({"name": atom.name, "atom": atom.atom,
                                           "file": "blobs/" + filename, "size": len(data),
                                           "sha256": hashlib.sha256(data).hexdigest()})
                if container_bytes != container.size:
                    raise CloudStorageError("changed")
                manifest["containers"].append(saved)
            for container in inventory.containers:
                if container.name in before_atoms and read.atoms(container) != before_atoms[container.name]:
                    raise CloudStorageError("changed")
            if read.inventory() != inventory:
                raise CloudStorageError("changed")
            read.check()
            encoded = (json.dumps(manifest, ensure_ascii=True, separators=(",", ":")) + "\n").encode("utf-8")
            _write(stage_fd, "manifest.json", encoded)
            os.fsync(blobs_fd)
            os.fsync(stage_fd)
            read.check()
            _same_directory(root_fd, parent_directory)
            linked = os.stat(staging, dir_fd=root_fd, follow_symlinks=False)
            opened = os.fstat(stage_fd)
            if not stat.S_ISDIR(linked.st_mode) or (linked.st_dev, linked.st_ino) != (opened.st_dev, opened.st_ino):
                raise CloudStorageError("local_storage")
            _rename_noreplace(root_fd, staging, destination)
            published = True
            try:
                os.fsync(root_fd)
            except OSError:
                raise CloudStorageError("durability_unknown") from None
            return Snapshot(Path(parent_directory) / destination, self.scope.binding,
                            inventory.container_count, len(files), total_bytes)
        except OSError:
            raise CloudStorageError("local_storage") from None
        finally:
            # Anchored descriptors ensure cleanup cannot follow an exchanged
            # parent path or a save-name symlink. Never delete existing targets.
            if created and not published:
                if blobs_fd is not None:
                    for name in files:
                        _unlink(blobs_fd, name)
                if stage_fd is not None:
                    _unlink(stage_fd, "manifest.json")
                    _unlink(stage_fd, "blobs", directory=True)
                if root_fd is not None:
                    _unlink(root_fd, staging, directory=True)
            for fd in (blobs_fd, stage_fd, root_fd):
                if fd is not None:
                    os.close(fd)


def _private_directory(path: Path):
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        raise CloudStorageError("local_storage")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        info = os.fstat(fd)
        if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077:
            raise CloudStorageError("local_storage")
        result, fd = fd, None
        return result
    finally:
        if fd is not None:
            os.close(fd)


def _write(directory_fd, name, data):
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory_fd)
    with os.fdopen(fd, "wb") as target:
        target.write(data)
        target.flush()
        os.fsync(target.fileno())


def _same_directory(directory_fd, path):
    current = _private_directory(path)
    try:
        before, after = os.fstat(directory_fd), os.fstat(current)
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
            raise CloudStorageError("local_storage")
    finally:
        os.close(current)


def _rename_noreplace(directory_fd, source, destination):
    rename = getattr(ctypes.CDLL(None, use_errno=True), "renameat2", None)
    if rename is None:
        raise CloudStorageError("local_storage")
    rename.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    rename.restype = ctypes.c_int
    if rename(directory_fd, os.fsencode(source), directory_fd, os.fsencode(destination), 1):
        raise CloudStorageError("local_storage")


def _unlink(directory_fd, name, *, directory=False):
    try:
        if directory:
            os.rmdir(name, dir_fd=directory_fd)
        else:
            os.unlink(name, dir_fd=directory_fd)
    except OSError:
        pass
