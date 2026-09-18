# SPDX-License-Identifier: MIT
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from flightdeck import cloud_runtime as cr
from flightdeck.cloud_storage import CloudStorageError, Request

SCID = "12345678-1234-1234-1234-123456789abc"
INIT = {"op": "init", "config": "<Game/>", "scid": SCID, "pfn": "Test.Game_abc", "title_id": 123}
SCOPE = {"xuid": "123", "scid": SCID, "package_family_name": "Test.Game_abc", "title_id": 123}


def frame(value, body=b""):
    data = json.dumps(value).encode()
    return struct.pack("<I", len(data)) + data + body


class Child:
    def __init__(self, replies):
        self.calls = []
        incoming_r, incoming_w = os.pipe()
        outgoing_r, outgoing_w = os.pipe()
        self.stdin = os.fdopen(incoming_w, "wb", buffering=0)
        self.stdout = os.fdopen(outgoing_r, "rb", buffering=0)
        def worker():
            with os.fdopen(incoming_r, "rb", buffering=0) as source, os.fdopen(outgoing_w, "wb", buffering=0) as sink:
                for reply in replies:
                    header = source.read(4)
                    if not header:
                        break
                    n, = struct.unpack("<I", header)
                    data = b""
                    while len(data) < n:
                        data += source.read(n - len(data))
                    self.calls.append(json.loads(data))
                    sink.write(reply)
        self.thread = threading.Thread(target=worker, daemon=True)
        self.thread.start()
    def close(self):
        self.stdin.close()
        self.thread.join(timeout=2)
        self.stdout.close()


class CloudRuntimeTests(unittest.TestCase):
    def transport(self, replies=(), *, scope=None, cancel=None):
        child = Child([frame({"ok": True, "protocol": 1, "scope": scope or SCOPE}), *replies])
        self.addCleanup(child.close)
        return cr.HelperTransport(child, INIT, cancel=cancel), child

    def test_exact_scope_index_and_private_frame(self):
        t, child = self.transport([frame({"ok": True, "status": 200, "headers": {}, "body_bytes": 2}, b"{}")])
        response = t(Request(t.scope.base_url), timeout=1, max_bytes=100)
        self.assertEqual(response.body, b"{}")
        self.assertEqual(child.calls[1], {"op": "index", "max_bytes": 100})
        self.assertFalse(any("token" in k for k in child.calls[1]))

    def test_exact_wire_filename_encoded_once(self):
        t, child = self.transport([frame({"ok": True, "status": 200, "headers": {}, "body_bytes": 0})])
        t(Request(t.scope.base_url + "/profile%2Csavedgame"), timeout=1, max_bytes=100)
        self.assertEqual(child.calls[1]["wire_name"], "profile,savedgame")

    def test_paging_is_typed_canonical_and_bounded(self):
        t, child = self.transport([frame({"ok": True, "status": 200, "headers": {}, "body_bytes": 0})])
        for suffix in ["?skipItems=01&continuationToken=x", "?skipItems=1&continuationToken=x&extra=y",
                       "?skipItems=1&continuationToken=%0A", "?skipItems=4097&continuationToken=x"]:
            with self.subTest(suffix=suffix), self.assertRaises(CloudStorageError):
                t(Request(t.scope.base_url + suffix), timeout=1, max_bytes=100)
        t(Request(t.scope.base_url + "?skipItems=2&continuationToken=opaque%2F%2B%3D%3D"), timeout=1, max_bytes=100)
        self.assertEqual(child.calls[1], {"op": "index", "max_bytes": 100,
                                        "skip_items": 2, "continuation_token": "opaque/+=="})

    def test_atom_zero_bytes_is_valid(self):
        t, child = self.transport([frame({"ok": True, "status": 200, "headers": {}, "body_bytes": 0})])
        response = t(Request(t.scope.base_url + "/" + SCID + ",binary"), timeout=1, max_bytes=0)
        self.assertEqual(response.body, b"")
        self.assertEqual(child.calls[1]["op"], "atom")

    def test_atom_wire_case_preserved(self):
        t, child = self.transport([frame({"ok": True, "status": 200, "headers": {}, "body_bytes": 0})])
        t(Request(t.scope.base_url + "/" + SCID.upper() + ",binary"), timeout=1, max_bytes=0)
        self.assertEqual(child.calls[1]["atom"], SCID.upper())

    def test_foreign_scope_path_and_mutation_rejected(self):
        t, child = self.transport()
        for request in [Request("https://evil.invalid/"), Request(t.scope.base_url, method="PUT"),
                        Request(t.scope.base_url + "/../lock"), Request(t.scope.base_url + "?token=x"),
                        Request(t.scope.base_url, {"Authorization": "synthetic-secret"}),
                        Request(t.scope.base_url, {"x-xbl-pfn": "Other.Title"})]:
            with self.subTest(request=request), self.assertRaises(CloudStorageError):
                t(request, timeout=1, max_bytes=100)
        self.assertEqual(len(child.calls), 1)

    def test_title_drift_rejected(self):
        with self.assertRaises(CloudStorageError):
            self.transport(scope=dict(SCOPE, title_id=124))

    def test_server_error_has_no_body(self):
        t, _ = self.transport([frame({"ok": True, "status": 403, "headers": {}, "body_bytes": 0})])
        self.assertEqual(t(Request(t.scope.base_url), timeout=1, max_bytes=100).status, 403)

    def test_oversize_reply_poison_session(self):
        t, _ = self.transport([frame({"ok": True, "status": 200, "headers": {}, "body_bytes": 101})])
        with self.assertRaises(CloudStorageError):
            t(Request(t.scope.base_url), timeout=1, max_bytes=100)
        self.assertTrue(t.failed)
        with self.assertRaises(CloudStorageError):
            t(Request(t.scope.base_url), timeout=1, max_bytes=100)

    def test_auth_errors_static_and_no_remote_text(self):
        child = Child([frame({"ok": False, "error": "authentication", "message": "synthetic-token"})])
        self.addCleanup(child.close)
        with self.assertRaises(CloudStorageError) as result:
            cr.HelperTransport(child, INIT)
        self.assertEqual(result.exception.code, "authentication")
        self.assertNotIn("synthetic-token", str(result.exception))

    def test_cancel_before_auth_writes_nothing(self):
        cancel = threading.Event(); cancel.set()
        child = Child([]); self.addCleanup(child.close)
        with self.assertRaises(CloudStorageError) as result:
            cr.HelperTransport(child, INIT, cancel=cancel)
        self.assertEqual(result.exception.code, "cancelled")
        self.assertEqual(child.calls, [])

    def test_config_real_binding_and_no_fallback(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); game = root / "games/MSFS2024"; game.mkdir(parents=True)
            path = game / "MicrosoftGame.Config"
            value = '<Game><Identity Name="Test.Game" Publisher="CN=Test" Version="1.0.0.0"/><StoreId>9P38D19T7LRV</StoreId><TitleId>7B</TitleId><SCId>' + SCID + '</SCId></Game>'
            path.write_text(value)
            config = cr._config(root)
            self.assertEqual(config["title_id"], 123)
            self.assertEqual(config["scid"], SCID)
            self.assertRegex(config["pfn"], r"^Test.Game_[0-9a-z]{13}$")
            path.write_text(value.replace('<SCId>' + SCID + '</SCId>', ''))
            self.assertEqual(cr._config(root)["scid"], "")  # Exact TitleHub lookup required.
            path.write_text('<!DOCTYPE Game [<!ENTITY x "test">]>' + value)
            with self.assertRaises(CloudStorageError):
                cr._config(root)

    def test_availability_no_exec_and_current_feature_required(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(cr.subprocess, "Popen") as process:
            self.assertFalse(cr.available(Path(folder), source_root=Path(folder)))
            process.assert_not_called()

    def test_actual_child_frames_and_timeout_cleanup(self):
        # An actual subprocess writes in small chunks; no shell/auth/network.
        payload = frame({"ok": True, "protocol": 1, "scope": SCOPE}).hex()
        code = "import sys,struct,time\nh=sys.stdin.buffer.read(4);n=struct.unpack('<I',h)[0];sys.stdin.buffer.read(n)\nfor b in bytes.fromhex(sys.argv[1]):\n sys.stdout.buffer.write(bytes([b]));sys.stdout.buffer.flush()\ntime.sleep(5)"
        child = subprocess.Popen([sys.executable, "-c", code, payload], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, start_new_session=True, bufsize=0)
        try:
            t = cr.HelperTransport(child, INIT)
            started = time.monotonic()
            with self.assertRaises(CloudStorageError) as result:
                t(Request(t.scope.base_url), timeout=.1, max_bytes=10)
            self.assertEqual(result.exception.code, "deadline")
            self.assertLess(time.monotonic() - started, 1)
        finally:
            cr._stop_owned(child); child.stdin.close(); child.stdout.close()

    def test_warm_prefix_skips_wineboot_but_fresh_processes_rebind_actual_user(self):
        from unittest.mock import Mock
        from tests.test_cloud_prefix import LIBRARIES, wine_layout
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "private").mkdir(mode=0o700)
            children, brokers, spawned, wine_calls = [], [], [], []
            for user in ("123", "456"):
                child = Child([frame({"ok": True, "protocol": 2, "features": [cr.WRITE_FEATURE],
                                      "scope": dict(SCOPE, xuid=user)})])
                child.wait = lambda **kwargs: 0
                children.append(child)
                broker = Mock(); broker.poll.return_value = None; brokers.append(broker)
            queue = iter([brokers[0], children[0], brokers[1], children[1]])
            def spawn(arguments, **kwargs):
                spawned.append((arguments, kwargs))
                return next(queue)
            def run(arguments, *, cwd, env, **kwargs):
                wine_calls.append((list(map(str, arguments)), env["WINEPREFIX"]))
                if "wineboot" in arguments:
                    wine_layout(Path(env["WINEPREFIX"]))
                elif "-k" in arguments:
                    raise CloudStorageError("transport")  # Server already exited; -w still proves cleanup.
            try:
                with patch.object(cr, "_paths", return_value=(root / "native", root / "helper.exe", root / "wine", root / "original")), \
                     patch.object(cr, "_prefix_material", return_value=("a" * 64, LIBRARIES)), \
                     patch.object(cr, "_config", side_effect=lambda _: dict(INIT)) as config, \
                     patch.object(cr, "_run", side_effect=run), \
                     patch.object(Path, "is_socket", return_value=True), \
                     patch.object(cr.subprocess, "Popen", side_effect=spawn), \
                     patch.object(cr, "_stop_owned") as stop:
                    users = []
                    for _ in range(2):
                        with cr.open_client(root) as client:
                            users.append(client.scope.xuid)
                    self.assertEqual(users, ["123", "456"])
                    self.assertEqual(config.call_count, 2)
                    self.assertEqual(len(spawned), 4)  # New broker and helper on both calls.
                    self.assertEqual(stop.call_count, 2)
                self.assertEqual(sum("wineboot" in argv for argv, _ in wine_calls), 1)
                self.assertEqual(sum("-k" in argv for argv, _ in wine_calls), 2)
                self.assertEqual(sum("-w" in argv for argv, _ in wine_calls), 2)
                self.assertEqual(len({path for _, path in wine_calls}), 1)
                self.assertNotEqual(spawned[0][1]["env"]["XDG_RUNTIME_DIR"], spawned[2][1]["env"]["XDG_RUNTIME_DIR"])
                for child in children:
                    self.assertEqual(len(child.calls), 1)
                    self.assertEqual(child.calls[0]["op"], "init")
                    self.assertEqual(child.calls[0]["title_id"], INIT["title_id"])
                    self.assertEqual(child.calls[0]["config"], INIT["config"])
                for _, parameters in spawned:
                    self.assertEqual(parameters["env"]["XDG_DATA_HOME"], str(root / "private/xdg/data"))
                    self.assertNotIn("local/msfs-prefix", parameters["env"]["WINEPREFIX"])
            finally:
                for child in children:
                    child.close()

    def test_prefix_cleanup_requires_successful_wait_even_when_kill_succeeds(self):
        with patch.object(cr, "_run", side_effect=[None, CloudStorageError("deadline")]) as run:
            with self.assertRaises(CloudStorageError) as error:
                cr._stop_prefix(Path("/synthetic/wine"), env={}, cwd=Path("/synthetic/private"))
        self.assertEqual(error.exception.code, "deadline")
        self.assertEqual([call.args[0][-1] for call in run.call_args_list], ["-k", "-w"])

    def test_prefix_material_changes_with_runner_native_and_pinned_version(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); native = root / "native"; native.mkdir()
            files = ("runtime/xgameruntime.dll", "builtin/x86_64-windows/xodus_store_test.dll",
                     "builtin/x86_64-unix/xodus_store_test.so", cr.HELPER, "bin/xodus-service", "bin/xodus-cli")
            for name in files:
                path = native / name; path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(("synthetic " + name).encode())
            wine = root / "wine"; wine.write_bytes(b"synthetic wine")
            (root / "wineserver").write_bytes(b"synthetic wineserver")
            original = root / "original"; original.write_bytes(b"synthetic original")
            lock = {"native": {"files": {name: hashlib.sha256((native / name).read_bytes()).hexdigest() for name in files}},
                    "runner": {"archive_sha256": "a" * 64, "original_runtime_sha256": hashlib.sha256(original.read_bytes()).hexdigest()}}
            lockfile = root / "bootstrap.lock.json"
            def material():
                lockfile.write_text(json.dumps(lock))
                return cr._prefix_material(native, wine, original, root)[0]
            with patch.object(cr.bootstrap, "paths", return_value=(root, lockfile)):
                first = material()
                self.assertEqual(first, material())
                wine.write_bytes(b"synthetic updated wine")
                second = material(); self.assertNotEqual(first, second)
                lock["runner"]["archive_sha256"] = "b" * 64
                third = material(); self.assertNotEqual(second, third)
                (native / cr.HELPER).write_bytes(b"synthetic updated helper")
                with self.assertRaises(CloudStorageError): material()
                lock["native"]["files"][cr.HELPER] = hashlib.sha256((native / cr.HELPER).read_bytes()).hexdigest()
                self.assertNotEqual(third, material())


class CloudWriteTransportTests(unittest.TestCase):
    def transport(self, replies=(), *, old=False, cancel=None):
        child = Child([frame({"ok": True, "protocol": 1 if old else 2,
                            "features": [] if old else [cr.WRITE_FEATURE], "scope": SCOPE}), *replies])
        self.addCleanup(child.close)
        return cr.HelperTransport(child, INIT, cancel=cancel), child

    def response(self, body=None, status=200):
        raw = b"" if body is None else json.dumps(body).encode()
        return frame({"ok": True, "status": status, "headers": {}, "body_bytes": len(raw)}, raw)

    def test_old_reader_never_claims_write(self):
        t, child = self.transport(old=True)
        with self.assertRaises(CloudStorageError):
            t.acquire(timeout=1)
        self.assertEqual(len(child.calls), 1)

    def test_lease_identity_and_conflict_are_not_empty_success(self):
        t, child = self.transport([self.response({"owner_change_id": "synthetic-generation", "quota_bytes": 1048576}, 201),
                                   self.response(status=409)])
        acquired = t.acquire(timeout=1)
        self.assertEqual((acquired.status, acquired.owner_change_id), (201, "synthetic-generation"))
        self.assertEqual(t.renew(timeout=1).status, 409)
        self.assertEqual([x["op"] for x in child.calls], ["init", "lease_acquire", "lease_renew"])

    def test_cancel_before_operation_preserves_pipe_for_release(self):
        cancel = threading.Event()
        t, child = self.transport([self.response(status=204)], cancel=cancel)
        cancel.set()
        with self.assertRaises(CloudStorageError):
            t.renew(timeout=1)
        self.assertFalse(t.failed)
        self.assertEqual(t.release(timeout=1), 204)
        self.assertEqual(child.calls[-1]["op"], "lease_release")

    def test_missing_owner_poison_session(self):
        t, _ = self.transport([self.response({"quota_bytes": 100})])
        with self.assertRaises(CloudStorageError):
            t.acquire(timeout=1)
        self.assertTrue(t.failed)

    def test_container_fields_are_exact_and_time_is_utc(self):
        t, child = self.transport([self.response(status=204)])
        self.assertEqual(t.put_container("name/space,savedgame", "Display", 0, {"blob": SCID.upper()}, timeout=1), 204)
        self.assertEqual(child.calls[-1]["modified"], "1970-01-01T00:00:00Z")
        self.assertEqual(child.calls[-1]["atoms"], {"blob": SCID.upper()})
        self.assertEqual(child.calls[-1]["wire_name"], "name/space,savedgame")

    def test_arbitrary_method_url_body_and_fields_rejected(self):
        t, child = self.transport()
        for request in [{"op": "lease_acquire", "force": True}, {"op": "lease_acquire", "url": "https://invalid.example"},
                        {"op": "azure_put"}, {"op": "container_delete", "wire_name": "a,savedgame", "headers": {}}]:
            with self.subTest(request=request), self.assertRaises(CloudStorageError):
                t.write_operation(request, timeout=1)
        with self.assertRaises(CloudStorageError):
            t.write_operation({"op": "lease_acquire"}, b"data", timeout=1)
        self.assertEqual(len(child.calls), 1)

    def test_real_child_receives_exact_binary_atom_payload(self):
        payload = b"\x00\xff\r\nsynthetic-save\x00"
        handshake = frame({"ok": True, "protocol": 2, "features": [cr.WRITE_FEATURE], "scope": SCOPE}).hex()
        answer = self.response({"atom": SCID.upper()}).hex()
        code = """import sys,struct,json,hashlib
f=sys.stdin.buffer
def read():
 n=struct.unpack('<I',f.read(4))[0];return json.loads(f.read(n))
read();sys.stdout.buffer.write(bytes.fromhex(sys.argv[1]));sys.stdout.buffer.flush()
j=read();b=f.read(j['body_bytes']);assert j['op']=='atom_upload';assert hashlib.sha256(b).hexdigest()==sys.argv[3]
sys.stdout.buffer.write(bytes.fromhex(sys.argv[2]));sys.stdout.buffer.flush()
"""
        child = subprocess.Popen([sys.executable, "-c", code, handshake, answer, hashlib.sha256(payload).hexdigest()],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0)
        try:
            t = cr.HelperTransport(child, INIT)
            self.assertEqual(t.upload_atom(payload, timeout=1), SCID.upper())
            self.assertEqual(child.wait(timeout=2), 0)
        finally:
            child.stdin.close(); child.stdout.close()
            if child.poll() is None:
                child.kill(); child.wait()

    def test_context_preserves_partial_write_exception_during_cleanup(self):
        from flightdeck.cloud_write import CloudWriteError
        from unittest.mock import Mock
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); private = root / "private"; private.mkdir(mode=0o700)
            native = root / "native"
            for name in ("runtime/xgameruntime.dll", "builtin/x86_64-windows/xodus_store_test.dll"):
                path = native / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(b"synthetic")
            original = root / "original.dll"; original.write_bytes(b"synthetic")
            child = Child([frame({"ok": True, "protocol": 2, "features": [cr.WRITE_FEATURE], "scope": SCOPE})])
            child.wait = lambda **kwargs: 0
            broker = Mock(); broker.poll.return_value = None
            def setup(argv, *, cwd, **kwargs):
                if "wineboot" in argv:
                    (cwd / "prefix/drive_c/windows/system32").mkdir(parents=True)
            expected = CloudWriteError("readback", committed_containers=1, recovery_required=True)
            try:
                with patch.object(cr, "_paths", return_value=(native, root / "helper.exe", root / "wine", original)), \
                     patch.object(cr, "_prefix_material", return_value=("a" * 64, {
                         "xgameruntime.dll": b"synthetic", "xgameruntime_original.dll": b"synthetic",
                         "xodus_store_test.dll": b"synthetic"})), \
                     patch.object(cr, "_config", return_value=dict(INIT)), \
                     patch.object(cr, "_run", side_effect=setup), \
                     patch.object(Path, "is_socket", return_value=True), \
                     patch.object(cr.subprocess, "Popen", side_effect=[broker, child]), \
                     patch.object(cr, "_stop_owned", side_effect=OSError("synthetic cleanup failure")):
                    with self.assertRaises(CloudWriteError) as result:
                        with cr.open_client(root):
                            raise expected
                    self.assertIs(result.exception, expected)
                    self.assertTrue(result.exception.recovery_required)
                    self.assertFalse((private / "connected-storage-device.seed").exists())
            finally:
                child.close()

if __name__ == "__main__":
    unittest.main()
