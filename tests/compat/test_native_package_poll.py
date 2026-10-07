# SPDX-License-Identifier: MIT
"""Exercise native package handoff polling against a local HTTP service."""
import contextlib
import http.server
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
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


class NativePackagePoll(unittest.TestCase):
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
