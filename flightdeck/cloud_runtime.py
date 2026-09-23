# SPDX-License-Identifier: MIT
"""Private, fixed-operation pipe to the genuine game-XUser signing runtime.

The caller owns the launcher reservation and runtime lock for this context.
No browser parameter becomes an executable, URL, credential or title identity.
Every session starts a fresh helper and broker socket. A verified private Wine
prefix may be reused after confirmed cleanup; no authentication is cached here.
Game files, the normal prefix and local saves are never modified by this reader.
"""
from __future__ import annotations

from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import selectors
import socket
import stat
import struct
import subprocess
import tempfile
import time
from urllib.parse import unquote, quote
import xml.etree.ElementTree as ET

from . import bootstrap, cloud_prefix
from . import games
from .cloud_storage import CloudStorageClient, CloudStorageError, Request, Response, Scope
from .game_install import _stop_owned
from .mods import _read

HELPER = "bin/flightdeck-connected-storage.exe"
FEATURE = "connected-storage-read-v1"
WRITE_FEATURE = "connected-storage-sync-v1"
_FRAME_MAX = 256 * 1024


def _cancel(cancel):
    if cancel is not None and cancel.is_set():
        raise CloudStorageError("cancelled")


def _config(runtime):
    spec = games.for_runtime(runtime)
    game = games.path(runtime)
    path = game / "MicrosoftGame.Config"
    if not path.exists():
        path = game / "MicrosoftGame.config"
    try:
        data = _read(path, 128 * 1024)
        text = data.decode("utf-16" if data.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig")
        if "<!DOCTYPE" in text.upper() or "<!ENTITY" in text.upper():
            raise ValueError()
        root = ET.fromstring(text)
        def one(name):
            nodes = [n for n in root.iter() if n.tag.rsplit("}", 1)[-1] == name]
            if len(nodes) != 1:
                raise ValueError()
            return nodes[0]
        if root.tag.rsplit("}", 1)[-1] != "Game" or one("StoreId").text != spec.store_id:
            raise ValueError()
        title = one("TitleId").text or ""
        if not re.fullmatch(r"[0-9A-Fa-f]{1,8}", title):
            raise ValueError()
        title_id = int(title, 16)
        scids = [n for n in root.iter() if n.tag.rsplit("}", 1)[-1].lower() == "scid"]
        if len(scids) > 1:
            raise ValueError()
        scid = (scids[0].text or "").lower() if scids else ""
        identity = one("Identity")
        name, publisher = identity.attrib["Name"], identity.attrib["Publisher"]
        if not re.fullmatch(r"[A-Za-z0-9.-]{3,50}", name) or not publisher or len(publisher) > 8192:
            raise ValueError()
        # Same Windows publisher-family algorithm as the native title binding:
        # first 64 SHA256 bits of exact UTF-16LE publisher, padded to 65 bits.
        alphabet = "0123456789abcdefghjkmnpqrstvwxyz"
        bits = int.from_bytes(hashlib.sha256(publisher.encode("utf-16le")).digest()[:8], "big") << 1
        suffix = "".join(alphabet[(bits >> shift) & 31] for shift in range(60, -1, -5))
        pfn = name + "_" + suffix
        Scope("1", scid or "00000000-0000-0000-0000-000000000000", pfn, title_id)  # Validate public title fields only.
        return {"op": "init", "config": text, "scid": scid, "pfn": pfn, "title_id": title_id}
    except (OSError, ValueError, KeyError, ET.ParseError, UnicodeError):
        raise CloudStorageError("invalid_scope") from None


def _paths(runtime, source_root=None, *, verify=True, write=False):
    runtime = Path(runtime)
    source, lockfile = bootstrap.paths(source_root)
    lock = json.loads(_read(lockfile, 65536))
    if FEATURE not in lock["native"].get("features", []) or (write and WRITE_FEATURE not in lock["native"].get("features", [])):
        raise CloudStorageError("transport")
    native = bootstrap.native_path(source, lock, verify=verify)
    if native is None:
        raise CloudStorageError("transport")
    helper = native / HELPER
    expected = lock["native"]["files"][HELPER]
    if (helper.is_symlink() or not helper.is_file() or not re.fullmatch(r"[a-f0-9]{64}", expected)
            or (verify and hashlib.sha256(_read(helper, 16 * 1024 * 1024)).hexdigest() != expected)):
        raise CloudStorageError("transport")
    runner = runtime / "runner/files/bin/wine"
    if not runner.is_file():
        # Explicit legacy layout of this pinned runner; never scan arbitrary files.
        directory = lock["runner"]["directory"]
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", directory):
            raise CloudStorageError("transport")
        runner = runtime / "runner-research" / directory / "files/bin/wine"
    if not runner.is_file() or not os.access(runner, os.X_OK):
        raise CloudStorageError("transport")
    original = runner.parent.parent / "lib/wine/x86_64-windows/xgameruntime.dll"
    if not original.is_file():
        original = runtime / "local/msfs-prefix/drive_c/windows/system32/xgameruntime_original.dll"
    if (not original.is_file() or (verify and hashlib.sha256(_read(original, 32 * 1024 * 1024)).hexdigest()
                                 != lock["runner"]["original_runtime_sha256"])):
        raise CloudStorageError("transport")
    return native, helper, runner, original


def available(runtime, source_root=None, *, verify=False, write=False):
    """Only inspect local files; no process, authentication or network traffic."""
    if not runtime:
        return False
    try:
        _paths(runtime, source_root, verify=verify, write=write)
        _config(runtime)
        return True
    except (OSError, ValueError, KeyError, TypeError, CloudStorageError):
        return False


class HelperTransport:
    """Framed pipe transport; caller owns and closes its child process."""
    def __init__(self, process, initial, *, cancel=None):
        self.process, self.cancel = process, cancel
        self.failed = False
        try:
            answer, body = self._exchange(initial, timeout=120, maximum=0)
            scope = answer.get("scope")
            if answer.get("protocol") not in (1, 2) or not isinstance(scope, dict) or body:
                raise CloudStorageError("invalid_response")
            self.can_write = answer.get("protocol") == 2 and WRITE_FEATURE in answer.get("features", [])
            self.scope = Scope(**scope)
            if ((initial["scid"] and self.scope.scid != initial["scid"]) or self.scope.title_id != initial["title_id"]
                    or self.scope.package_family_name != initial["pfn"]):
                raise CloudStorageError("invalid_scope")
        except Exception:
            self.failed = True
            raise

    def _io(self, fd, buffer, *, writing, deadline, ignore_cancel=False):
        with selectors.DefaultSelector() as selector:
            selector.register(fd, selectors.EVENT_WRITE if writing else selectors.EVENT_READ)
            while buffer:
                if not ignore_cancel:
                    _cancel(self.cancel)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise CloudStorageError("deadline")
                if not selector.select(min(.1, remaining)):
                    continue
                try:
                    if writing:
                        n = os.write(fd, buffer)
                    else:
                        data = os.read(fd, len(buffer))
                        n = len(data)
                        buffer[:n] = data
                except (BlockingIOError, InterruptedError):
                    continue
                if not n:
                    raise CloudStorageError("transport")
                buffer = buffer[n:]

    def _exchange(self, request, *, timeout, maximum, input_body=b"", ignore_cancel=False):
        if self.failed or not isinstance(timeout, (int, float)) or not math.isfinite(timeout) or timeout <= 0:
            raise CloudStorageError("transport")
        if not ignore_cancel:
            _cancel(self.cancel)
        try:
            payload = json.dumps(request, ensure_ascii=True, separators=(",", ":")).encode("utf-8")
            if len(payload) > _FRAME_MAX:
                raise CloudStorageError("bounds")
            deadline = time.monotonic() + timeout
            self._io(self.process.stdin.fileno(), memoryview(struct.pack("<I", len(payload)) + payload), writing=True, deadline=deadline, ignore_cancel=ignore_cancel)
            self._io(self.process.stdin.fileno(), memoryview(input_body), writing=True, deadline=deadline, ignore_cancel=ignore_cancel)
            length = bytearray(4)
            self._io(self.process.stdout.fileno(), memoryview(length), writing=False, deadline=deadline, ignore_cancel=ignore_cancel)
            n, = struct.unpack("<I", length)
            if not 0 < n <= _FRAME_MAX:
                raise CloudStorageError("bounds")
            raw = bytearray(n)
            self._io(self.process.stdout.fileno(), memoryview(raw), writing=False, deadline=deadline, ignore_cancel=ignore_cancel)
            from .cloud_storage import _json
            answer = _json(bytes(raw))
            if answer.get("ok") is not True:
                code = answer.get("error")
                error = CloudStorageError("authentication" if code == "authentication" else
                                          "invalid_scope" if code == "title_binding" else "transport")
                # Numeric diagnostics only; never forward native strings/bodies.
                for source, target, maximum in (("hresult", "native_hresult", 2**32 - 1),
                                                ("http_status", "http_status", 599)):
                    value = answer.get(source)
                    if type(value) is int and 0 <= value <= maximum:
                        setattr(error, target, value)
                raise error
            size = answer.get("body_bytes", 0)
            if type(size) is not int or not 0 <= size <= maximum:
                raise CloudStorageError("bounds")
            body = bytearray(size)
            self._io(self.process.stdout.fileno(), memoryview(body), writing=False, deadline=deadline, ignore_cancel=ignore_cancel)
            return answer, bytes(body)
        except CloudStorageError:
            self.failed = True
            raise
        except Exception:
            self.failed = True
            raise CloudStorageError("transport") from None

    def __call__(self, request: Request, *, timeout: float, max_bytes: int) -> Response:
        # Match exact scope then reconstruct each allowed path, not urljoin.
        if request.method != "GET" or type(max_bytes) is not int or not 0 <= max_bytes <= 64 * 1024 * 1024:
            raise CloudStorageError("invalid_request")
        from .cloud_storage import _headers
        headers = _headers(request.headers)
        if any(k not in {"x-xbl-pfn", "accept", "accept-encoding", "x-xbl-contract-version"} for k in headers):
            raise CloudStorageError("invalid_request")
        if (headers.get("x-xbl-pfn", self.scope.package_family_name) != self.scope.package_family_name
                or headers.get("x-xbl-contract-version", "107") != "107"
                or headers.get("accept-encoding", "identity") != "identity"):
            raise CloudStorageError("invalid_request")
        base = self.scope.base_url
        operation = {"max_bytes": max_bytes}
        if request.url == base:
            operation["op"] = "index"
        elif request.url.startswith(base + "?"):
            from .cloud_storage import parse_index_query
            skip, token = parse_index_query(request.url[len(base) + 1:])
            operation.update(op="index", skip_items=skip, continuation_token=token)
        elif request.url.startswith(base + "/"):
            tail = request.url[len(base) + 1:]
            if re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12},binary", tail):
                operation.update(op="atom", atom=tail[:-7])
            else:
                try:
                    name = unquote(tail, encoding="utf-8", errors="strict")
                except UnicodeError:
                    raise CloudStorageError("invalid_request") from None
                if quote(name, safe="") != tail or name in {".", ".."} or not name or len(name.encode()) > 1024:
                    raise CloudStorageError("invalid_request")
                operation.update(op="container", wire_name=name)
        else:
            raise CloudStorageError("invalid_request")
        if operation["op"] != "atom" and max_bytes > 4 * 1024 * 1024:
            raise CloudStorageError("invalid_request")
        answer, body = self._exchange(operation, timeout=timeout, maximum=max_bytes)
        status = answer.get("status")
        if type(status) is not int or not 100 <= status <= 599 or not isinstance(answer.get("headers"), dict):
            self.failed = True
            raise CloudStorageError("invalid_response")
        return Response(status, _headers(answer["headers"]), body)

    def write_operation(self, operation, body=b"", *, timeout, max_bytes=4 * 1024 * 1024, _cleanup=False):
        if (not self.can_write or not isinstance(operation, dict) or type(body) is not bytes
                or len(body) > 64 * 1024 * 1024 or type(max_bytes) is not int
                or not 0 <= max_bytes <= 4 * 1024 * 1024):
            raise CloudStorageError("invalid_request")
        op = operation.get("op")
        fields = {"lease_acquire": {"op"}, "lease_renew": {"op"}, "lease_release": {"op"},
                  "atom_upload": {"op"}, "container_put": {"op", "wire_name", "display_name", "modified", "atoms"},
                  "container_delete": {"op", "wire_name"}}
        if (op not in fields or set(operation) != fields[op] or (body and op != "atom_upload")
                or (_cleanup and op != "lease_release")):
            raise CloudStorageError("invalid_request")
        request = dict(operation, body_bytes=len(body), max_bytes=max_bytes)
        answer, data = self._exchange(request, timeout=timeout, maximum=max_bytes,
                                      input_body=body, ignore_cancel=_cleanup)
        status, headers = answer.get("status"), answer.get("headers")
        if type(status) is not int or not 100 <= status <= 599 or not isinstance(headers, dict):
            self.failed = True
            raise CloudStorageError("invalid_response")
        from .cloud_storage import _headers
        return Response(status, _headers(headers), data)

    def _lease(self, op, timeout):
        from .cloud_write import LeaseReply
        response = self.write_operation({"op": op}, timeout=timeout)
        if response.status not in (200, 201):
            return LeaseReply(response.status, "", 0)
        from .cloud_storage import _json
        data = _json(response.body)
        owner, quota = data.get("owner_change_id"), data.get("quota_bytes")
        if (not isinstance(owner, str) or not 0 < len(owner) <= 256 or any(ord(c) < 32 or ord(c) == 127 for c in owner)
                or type(quota) is not int or not 0 < quota < 2**64):
            self.failed = True
            raise CloudStorageError("invalid_response")
        return LeaseReply(response.status, owner, quota)

    def acquire(self, *, timeout):
        return self._lease("lease_acquire", timeout)

    def renew(self, *, timeout):
        return self._lease("lease_renew", timeout)

    def release(self, *, timeout):
        return self.write_operation({"op": "lease_release"}, timeout=timeout, _cleanup=True).status

    def upload_atom(self, payload, *, timeout):
        response = self.write_operation({"op": "atom_upload"}, payload, timeout=timeout)
        if not 200 <= response.status < 300:
            from .cloud_write import CloudWriteError
            raise CloudWriteError("lease_lost" if response.status == 409 else "authentication" if response.status in (401, 403) else "transport")
        from .cloud_storage import _json
        atom = _json(response.body).get("atom")
        if not isinstance(atom, str) or not re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", atom):
            self.failed = True
            raise CloudStorageError("invalid_response")
        return atom

    def put_container(self, name, display_name, modified, atoms, *, timeout):
        if type(modified) is not int:
            raise CloudStorageError("invalid_request")
        try:
            stamp = datetime.fromtimestamp(modified, timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")
        except (ValueError, OverflowError, OSError):
            raise CloudStorageError("invalid_request") from None
        return self.write_operation({"op": "container_put", "wire_name": name, "display_name": display_name,
                                     "modified": stamp, "atoms": atoms}, timeout=timeout).status

    def delete_container(self, name, *, timeout):
        return self.write_operation({"op": "container_delete", "wire_name": name}, timeout=timeout).status


def _run(argv, *, env, cwd, cancel, timeout=60):
    process = subprocess.Popen([str(x) for x in argv], cwd=cwd, env=env,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, start_new_session=True,
                               close_fds=True, umask=0o077)
    try:
        started = time.monotonic()
        while process.poll() is None:
            _cancel(cancel)
            if time.monotonic() - started > timeout:
                raise CloudStorageError("deadline")
            time.sleep(.05)
        if process.returncode:
            raise CloudStorageError("transport")
    finally:
        _stop_owned(process)


def _prefix_material(native, wine, original, source_root):
    """Bind cache layout to pinned native/runner versions and actual inputs."""
    _, lockfile = bootstrap.paths(source_root)
    lock = json.loads(_read(lockfile, 65536))
    hashes, payloads = {}, {}
    for name, expected in lock["native"]["files"].items():
        payload = _read(native / name, 32 * 1024 * 1024)
        actual = hashlib.sha256(payload).hexdigest()
        if actual != expected:
            raise CloudStorageError("transport")
        hashes[name] = actual
        if name == "runtime/xgameruntime.dll":
            payloads["xgameruntime.dll"] = payload
        elif name == "builtin/x86_64-windows/xodus_store_test.dll":
            payloads["xodus_store_test.dll"] = payload
    payloads["xgameruntime_original.dll"] = _read(original, 32 * 1024 * 1024)
    original_hash = hashlib.sha256(payloads["xgameruntime_original.dll"]).hexdigest()
    if original_hash != lock["runner"]["original_runtime_sha256"]:
        raise CloudStorageError("transport")
    inputs = {"schema": 1, "native": hashes, "runner": lock["runner"],
              "wine_path": str(wine.resolve()), "native_path": str(native.resolve()),
              "original": original_hash,
              "wine": hashlib.sha256(_read(wine, 32 * 1024 * 1024)).hexdigest(),
              "wineserver": hashlib.sha256(_read(wine.parent / "wineserver", 32 * 1024 * 1024)).hexdigest()}
    binding = hashlib.sha256(json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return binding, payloads


def _stop_prefix(wine, *, env, cwd):
    # Wine's -k returns 1 if no server holds this prefix's lock. Only a
    # successful bounded -w proves the private server has finished either way.
    try:
        _run([wine.parent / "wineserver", "-k"], env=env, cwd=cwd, cancel=None, timeout=10)
    except (CloudStorageError, OSError, subprocess.SubprocessError):
        pass
    _run([wine.parent / "wineserver", "-w"], env=env, cwd=cwd, cancel=None, timeout=10)


@contextmanager
def open_client(runtime, source_root=None, *, cancel=None):
    """Open an account-bound reader while the caller holds runtime_lock."""
    runtime = Path(runtime)
    native, helper, wine, original = _paths(runtime, source_root)
    binding, libraries = _prefix_material(native, wine, original, source_root)
    config = _config(runtime)
    private = runtime / "private"
    info = private.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise CloudStorageError("local_storage")
    device = private / "connected-storage-device.seed"
    try:
        st = device.lstat()
        if (not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o077
                or st.st_nlink != 1 or st.st_size != 32):
            raise CloudStorageError("local_storage")
    except FileNotFoundError:
        pass  # Created by the native helper only on an explicit lease acquisition.
    config["device_file"] = "Z:" + str(device.absolute())
    with cloud_prefix.session(runtime, binding, libraries) as prefix:
        work = prefix.work
        # Linux AF_UNIX paths are bounded; don't embed a long runtime path.
        with tempfile.TemporaryDirectory(prefix="flightdeck-cs-", dir="/tmp", ignore_cleanup_errors=True) as sockets:
            socket_dir = Path(sockets)
            env = dict(os.environ)
            for key in ("WINE_DLL_FILE_MAP", "XODUS_KEYRING_FILE", "XODUS_LOCAL_GAMESAVE", "XODUS_LOCAL_GAMESAVE_ROOT"):
                env.pop(key, None)
            env.update(WINEPREFIX=str(prefix.path), WINEARCH="win64", WINEESYNC="0", WINEFSYNC="0",
                       WINEDEBUG="-all", XODUS_LOG="off", RUST_LOG="off", RUST_BACKTRACE="0",
                       XDG_RUNTIME_DIR=str(socket_dir), XODUS_USER_SOCKET_SUFFIX="xodus.sock", XODUS_USER_RUNTIME="1",
                       WINEDLLPATH=str(native / "builtin"),
                       WINEDLLOVERRIDES="winemenubuilder.exe,mscoree,mshtml=d;xgameruntime=n;xgameruntime_original=n,b;xodus_store_test=b")
            for category in ("config", "data", "cache", "state"):
                env["XDG_" + category.upper() + "_HOME"] = str(runtime / "private/xdg" / category)
            broker = child = None
            entered = False
            try:
                _cancel(cancel)
                if not prefix.reusable:
                    _run([wine, "wineboot", "-u"], env=env, cwd=work, cancel=cancel)
                    prefix.install_libraries()
                broker = subprocess.Popen([str(native / "bin/xodus-service")], cwd=work, env=env,
                                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                          start_new_session=True, close_fds=True, umask=0o077)
                started = time.monotonic()
                while not (socket_dir / "xodus.sock").is_socket():
                    _cancel(cancel)
                    if broker.poll() is not None or time.monotonic() - started > 20:
                        raise CloudStorageError("transport")
                    time.sleep(.05)
                child = subprocess.Popen([str(wine), str(helper)], cwd=work, env=env,
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                         start_new_session=True, close_fds=True, bufsize=0, umask=0o077)
                os.set_blocking(child.stdin.fileno(), False)
                os.set_blocking(child.stdout.fileno(), False)
                transport = HelperTransport(child, config, cancel=cancel)
                entered = True
                yield CloudStorageClient(transport.scope, transport)
            except CloudStorageError:
                raise
            except Exception:
                if entered:
                    raise
                raise CloudStorageError("transport") from None
            finally:
                clean = True
                if child is not None:
                    try:
                        if child.stdin:
                            child.stdin.close()
                        try:
                            child.wait(timeout=3)
                        except subprocess.TimeoutExpired:
                            _stop_owned(child)
                    except (OSError, subprocess.SubprocessError):
                        clean = False
                    finally:
                        try:
                            if child.stdout:
                                child.stdout.close()
                        except OSError:
                            clean = False
                if broker is not None:
                    try:
                        _stop_owned(broker)
                    except (OSError, subprocess.SubprocessError):
                        clean = False
                # Never stop the game's server. This exact private helper
                # prefix becomes reusable only after its server has exited.
                try:
                    _stop_prefix(wine, env=env, cwd=work)
                except (CloudStorageError, OSError, subprocess.SubprocessError):
                    clean = False
                if clean:
                    prefix.complete()
