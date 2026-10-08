# SPDX-License-Identifier: MIT
"""Exercise native package handoff polling against a local HTTP service."""
import contextlib
import fcntl
import http.server
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("native_package_check", ROOT / "scripts/check-native-package.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


@contextlib.contextmanager
def service(answer):
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            status, value = answer(self.path, self.headers["X-Flightdeck-Token"])
            body = json.dumps(value).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": .01})
    worker.start()
    try:
        yield server.server_port
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)


@contextlib.contextmanager
def install_coordinator(path):
    child = subprocess.Popen([sys.executable, "-u", "-c", """
import fcntl, sys
with open(sys.argv[1], 'rb') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    print('locked', flush=True)
    sys.stdin.buffer.read(1)
""", str(path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        assert child.stdout.readline() == "locked\n"
        yield child
    finally:
        child.stdin.close()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
            raise
        child.stdout.close()


class NativePackagePoll(unittest.TestCase):
    def test_reachable_replacement_waits_for_the_installation_coordinator(self):
        with tempfile.TemporaryDirectory() as temporary, \
                service(lambda path, token: (200, {"csrf_token": token, "service": {"release": "new"},
                                                   "runtime": {"configured": False}})) as port:
            state = Path(temporary)
            installation = state / "installed"
            installation.mkdir()
            lock_path = installation / ".install.lock"
            lock_path.touch(mode=0o600)
            previous = {"port": port, "token": "previous", "pid": 123, "release": "old"}
            current = {**previous, "token": "current", "pid": 456, "release": "new"}
            (state / "desktop-service.json").write_text(json.dumps(current))
            with install_coordinator(lock_path):
                self.assertEqual(check.record_at(state), current)
                self.assertIsNone(check.record_after_handoff(state, previous, installation, "new"))
                started = time.monotonic()
                with self.assertRaisesRegex(AssertionError, "Timed out"):
                    check.wait_for(lambda: check.record_after_handoff(state, previous, installation, "new"),
                                   timeout=.12)
                self.assertLess(time.monotonic() - started, 2)
            self.assertEqual(check.wait_for(lambda: check.record_after_handoff(state, previous, installation, "new"),
                                            timeout=2), current)
            # The readiness probe must release its own flock immediately.
            with lock_path.open("rb") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                fcntl.flock(lock, fcntl.LOCK_UN)

    def test_handoff_does_not_create_a_missing_installation_lock(self):
        with tempfile.TemporaryDirectory() as temporary, \
                service(lambda path, token: (200, {"csrf_token": token, "service": {"release": "new"},
                                                   "runtime": {"configured": False}})) as port:
            state = Path(temporary)
            current = {"port": port, "token": "current", "pid": 456, "release": "new"}
            previous = {**current, "token": "previous", "pid": 123}
            (state / "desktop-service.json").write_text(json.dumps(current))
            with self.assertRaisesRegex(AssertionError, "Cannot inspect"):
                check.wait_for(lambda: check.record_after_handoff(state, previous, state, "new"))
            self.assertFalse((state / ".install.lock").exists())

    def test_handoff_reloads_record_after_old_token_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            state = Path(temporary)
            record_path = state / "desktop-service.json"
            observed = []

            def answer(path, token):
                observed.append((path, token))
                if token == "previous-token":
                    replacement = {**json.loads(record_path.read_text()), "token": "current-token", "release": "new"}
                    record_path.write_text(json.dumps(replacement))
                    return 403, {"ok": False, "error": "Session expired"}
                return 200, {"csrf_token": token, "service": {"release": "new"}, "runtime": {"configured": False}}

            with service(answer) as port:
                record_path.write_text(json.dumps({"port": port, "token": "previous-token", "release": "old"}))
                current = check.wait_for(lambda: check.record_at(state), timeout=2)
            self.assertEqual(current["token"], "current-token")
            self.assertEqual(observed, [("/api/status", "previous-token"), ("/api/status", "current-token")])

    def test_persistent_authentication_rejection_still_fails_bounded_poll(self):
        for status in (401, 403):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as temporary, \
                    service(lambda *_: (status, {"ok": False})) as port:
                state = Path(temporary)
                (state / "desktop-service.json").write_text(json.dumps({"port": port, "token": "invalid", "release": "new"}))
                with self.assertRaisesRegex(AssertionError, "Timed out"):
                    check.wait_for(lambda: check.record_at(state), timeout=.12)

    def test_non_authentication_errors_and_other_requests_remain_fatal(self):
        with tempfile.TemporaryDirectory() as temporary, service(lambda *_: (500, {"ok": False})) as port:
            state = Path(temporary)
            record = {"port": port, "token": "current", "release": "new"}
            (state / "desktop-service.json").write_text(json.dumps(record))
            with self.assertRaises(check.RequestFailure):
                check.wait_for(lambda: check.record_at(state))
        with service(lambda *_: (403, {"ok": False})) as port:
            with self.assertRaises(check.RequestFailure):
                check.request({"port": port, "token": "expired"}, "/api/launcher-update")
