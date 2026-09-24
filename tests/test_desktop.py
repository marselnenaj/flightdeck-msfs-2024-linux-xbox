# SPDX-License-Identifier: MIT
"""Desktop ownership tests use only private temporary state and local sockets."""
from concurrent.futures import ThreadPoolExecutor
import fcntl
import json
import os
from pathlib import Path
import signal
import shutil
import stat
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

from flightdeck import desktop
from flightdeck.backend import Launcher, LauncherError


class DesktopTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="flightdeck-desktop-test-")
        self.base = Path(self.temporary.name)
        self.root = desktop.state_directory(self.base / "state")
        self.owned = []

    def tearDown(self):
        for pid, started in self.owned:
            if desktop.process_start(pid) == started:
                os.kill(pid, signal.SIGTERM)
                deadline = time.monotonic() + 4
                while desktop.process_start(pid) == started and time.monotonic() < deadline:
                    time.sleep(0.025)
                if desktop.process_start(pid) == started:
                    os.kill(pid, signal.SIGKILL)
        self.temporary.cleanup()

    def record(self):
        return {"app": desktop.APP, "schema": 1, "uid": os.getuid(), "pid": os.getpid(),
                "start": desktop.process_start(os.getpid()), "port": 43210, "token": "x" * 43,
                "release": desktop.release_identity()}

    def test_private_state_and_locks_reject_symlinks_and_fifo(self):
        self.assertEqual(stat.S_IMODE(self.root.stat().st_mode), 0o700)
        link = self.base / "linked-state"
        link.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(desktop.DesktopError):
            desktop.state_directory(link)
        lock = self.root / desktop.START_LOCK
        os.mkfifo(lock)
        with self.assertRaises(desktop.DesktopError):
            with desktop.lock_file(lock):
                self.fail("A FIFO is not a lock file")
        lock.unlink()
        lock.symlink_to(self.base / "foreign")
        with self.assertRaises(OSError):
            with desktop.lock_file(lock):
                self.fail("A symlink is not a lock file")
        self.assertFalse((self.base / "foreign").exists())

    def test_record_schema_mode_and_corruption_are_checked(self):
        record = self.record()
        desktop.write_record(self.root, record)
        self.assertEqual(stat.S_IMODE((self.root / desktop.RECORD).stat().st_mode), 0o600)
        self.assertEqual(desktop.read_record(self.root)["pid"], os.getpid())
        path = self.root / desktop.RECORD
        for change in ({"pid": True}, {"start": "1"}, {"port": 0}, {"token": "short"}, {"app": "foreign"}, {"release": "bad"}):
            path.write_text(json.dumps({**record, **change}))
            with self.assertRaises(desktop.DesktopError):
                desktop.read_record(self.root)
        path.write_text(json.dumps(record))
        path.chmod(0o644)
        with self.assertRaises(desktop.DesktopError):
            desktop.read_record(self.root)

    def test_record_never_overwrites_foreign_data(self):
        path = self.root / desktop.RECORD
        path.write_text("foreign personal data")
        path.chmod(0o600)
        with self.assertRaises(desktop.DesktopError):
            desktop.write_record(self.root, self.record())
        self.assertEqual(path.read_text(), "foreign personal data")

    def test_process_identity_requires_uid_birthtime_and_live_process(self):
        self.assertIsInstance(desktop.process_start(os.getpid()), int)
        self.assertIsNone(desktop.process_start(True))
        self.assertIsNone(desktop.process_start(-1))
        record = self.record()
        with patch.object(desktop, "service_locked", return_value=True), \
             patch.object(desktop, "request", return_value={"app": {"name": "Flightdeck"}, "csrf_token": record["token"],
                                                           "service": {"release": record["release"]}}):
            self.assertIsNotNone(desktop.verified_service(self.root, record))
            self.assertIsNone(desktop.verified_service(self.root, {**record, "start": record["start"] + 1}))
            self.assertIsNone(desktop.verified_service(self.root, {**record, "release": "0" * 64}))
        with patch.object(desktop, "service_locked", return_value=True), \
             patch.object(desktop, "request", return_value={"app": {"name": "Flightdeck"}, "csrf_token": "other"}):
            self.assertIsNone(desktop.verified_service(self.root, record))
        with patch.object(desktop, "service_locked", return_value=False), patch.object(desktop, "request") as request:
            self.assertIsNone(desktop.verified_service(self.root, record))
            request.assert_not_called()

    def test_held_unverified_service_is_left_untouched(self):
        with desktop.lock_file(self.root / desktop.SERVICE_LOCK), patch.object(desktop.subprocess, "Popen") as spawn:
            with self.assertRaises(desktop.DesktopError) as caught:
                desktop.ensure_service(self.root)
            self.assertEqual(caught.exception.key, "unverified")
            spawn.assert_not_called()

    def test_busy_start_lock_is_bounded(self):
        with desktop.lock_file(self.root / desktop.START_LOCK):
            began = time.monotonic()
            with self.assertRaises(desktop.DesktopError):
                with desktop.lock_file(self.root / desktop.START_LOCK, wait=0.05):
                    self.fail("Second lock succeeded")
            self.assertLess(time.monotonic() - began, 1)

    def test_real_service_is_private_reused_and_pid_mismatch_never_replaced(self):
        root, first = desktop.ensure_service(self.root, timeout=8)
        self.owned.append((first["pid"], first["start"]))
        self.assertNotEqual(first["pid"], os.getpid())
        self.assertTrue(desktop.service_locked(root))
        _, second = desktop.ensure_service(root, timeout=8)
        self.assertEqual(first["pid"], second["pid"])
        self.assertEqual(first["start"], second["start"])
        desktop.write_record(root, {**first, "start": first["start"] + 1})
        with patch.object(desktop.subprocess, "Popen") as spawn:
            with self.assertRaises(desktop.DesktopError):
                desktop.ensure_service(root, timeout=1)
            spawn.assert_not_called()
        self.assertEqual(desktop.process_start(first["pid"]), first["start"])
        desktop.write_record(root, first)
        self.assertIsNotNone(desktop.verified_service(root))

    def test_concurrent_real_starts_reuse_one_service(self):
        with ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(desktop.ensure_service, self.root, timeout=8) for _ in range(2)]
            results = [future.result(timeout=15)[1] for future in futures]
        self.owned.append((results[0]["pid"], results[0]["start"]))
        self.assertEqual(results[0]["pid"], results[1]["pid"])
        self.assertEqual(results[0]["start"], results[1]["start"])

    def test_chromium_app_window_has_private_profile_and_only_explicit_language(self):
        for language, suffix in ((None, ""), ("de", "/?lang=de")):
            with patch.object(desktop.shutil, "which", return_value="/synthetic/chromium"), \
                 patch.object(desktop.subprocess, "Popen") as spawn:
                url = desktop.open_interface(self.root, self.record(), language)
                self.assertEqual(url, "http://127.0.0.1:43210" + suffix)
                arguments = spawn.call_args.args[0]
                self.assertIn("--app=" + url, arguments)
                self.assertIn("--user-data-dir=" + str(self.root / "desktop-browser"), arguments)
                self.assertTrue(spawn.call_args.kwargs["start_new_session"])
                self.assertNotIn("shell", spawn.call_args.kwargs)
        self.assertEqual(stat.S_IMODE((self.root / "desktop-browser").stat().st_mode), 0o700)

    def test_browser_fallback_and_failure_are_explicit(self):
        with patch.object(desktop.shutil, "which", return_value=None), \
             patch.object(desktop.webbrowser, "open", return_value=True) as browser:
            self.assertEqual(desktop.open_interface(self.root, self.record()), "http://127.0.0.1:43210")
            browser.assert_called_once_with("http://127.0.0.1:43210", new=1)
        with patch.object(desktop.shutil, "which", return_value=None), \
             patch.object(desktop.webbrowser, "open", return_value=False):
            with self.assertRaises(desktop.DesktopError) as caught:
                desktop.open_interface(self.root, self.record())
            self.assertEqual(caught.exception.key, "browser")

    def test_reusing_service_configures_runtime_through_authenticated_api(self):
        record = self.record()
        with patch.object(desktop, "read_record", return_value=record), \
             patch.object(desktop, "verified_service", return_value=record), \
             patch.object(desktop, "request", return_value={"ok": True}) as request:
            desktop.ensure_service(self.root, runtime="/synthetic/runtime")
            self.assertEqual(request.call_args.args[1:], ("/api/config", {"runtime_path": "/synthetic/runtime"}))
        with patch.object(desktop, "read_record", return_value=record), \
             patch.object(desktop, "verified_service", return_value=record), \
             patch.object(desktop, "request", return_value=None):
            with self.assertRaises(desktop.DesktopError) as caught:
                desktop.ensure_service(self.root, runtime="/synthetic/runtime")
            self.assertEqual(caught.exception.key, "runtime")

    def test_failed_startup_terminates_only_its_own_child(self):
        child = Mock()
        child.poll.return_value = None
        with patch.object(desktop.subprocess, "Popen", return_value=child) as spawn, \
             patch.object(desktop, "service_locked", return_value=False):
            with self.assertRaises(desktop.DesktopError) as caught:
                desktop.ensure_service(self.root, timeout=0)
        self.assertEqual(caught.exception.key, "start")
        child.terminate.assert_called_once()
        child.wait.assert_called_once_with(timeout=3)
        child.kill.assert_not_called()
        self.assertIn("-B", spawn.call_args.args[0])
        self.assertTrue(spawn.call_args.kwargs["start_new_session"])

    def test_release_identity_tracks_code_not_user_data_or_install_path(self):
        first = self.base / "release-a"
        package = first / "flightdeck"
        package.mkdir(parents=True)
        (package / "__init__.py").write_text('__version__ = "0.1.0"\n')
        (package / "desktop.py").write_text("original code\n")
        (first / "private").mkdir()
        (first / "private/config.json").write_text("private fixture")
        with patch.object(desktop, "__file__", str(package / "desktop.py")):
            initial = desktop.release_identity()
            (first / "private/config.json").write_text("different private fixture")
            self.assertEqual(initial, desktop.release_identity())
        second = self.base / "release-b"
        shutil.copytree(first, second)
        with patch.object(desktop, "__file__", str(second / "flightdeck/desktop.py")):
            self.assertEqual(initial, desktop.release_identity())
            (second / "flightdeck/desktop.py").write_text("updated code, same version\n")
            self.assertNotEqual(initial, desktop.release_identity())
            for name in ("scripts/install-launcher.py", "ui/notices.js", "ui/launcher-updates.js", "flightdeck/_fenix/core.py", "ui/fenix.js", "compat/fenix/bundle.json",
                         "compat/fenix/release.json", "flightdeck/resources/fenix/bundle.json",
                         "flightdeck/resources/fenix/release.json"):
                with self.subTest(resource=name):
                    resource = second / name
                    resource.parent.mkdir(parents=True, exist_ok=True)
                    resource.write_text("original resource\n")
                    before = desktop.release_identity()
                    resource.write_text("updated resource\n")
                    self.assertNotEqual(before, desktop.release_identity())

    def test_changed_code_busy_or_legacy_service_is_not_terminated(self):
        record = self.record()
        for response in ({"ok": True, "refresh": "busy"}, None):
            with self.subTest(response=response), patch.object(desktop, "release_identity", return_value="0" * 64), \
                 patch.object(desktop, "read_record", return_value=record), \
                 patch.object(desktop, "verified_service", return_value=record), \
                 patch.object(desktop, "request", return_value=response) as request, \
                 patch.object(desktop.subprocess, "Popen") as spawn:
                _, result = desktop.ensure_service(self.root)
                self.assertTrue(result["update_pending"])
                self.assertEqual(result["pid"], record["pid"])
                request.assert_called_once_with(record, "/api/desktop/refresh", {})
                spawn.assert_not_called()

    def test_changed_code_idle_refresh_spawns_new_child_on_same_origin(self):
        record = self.record()
        newer = {**record, "release": "0" * 64, "pid": record["pid"] + 1}
        child = Mock()
        child.poll.return_value = None
        with patch.object(desktop, "release_identity", return_value=newer["release"]), \
             patch.object(desktop, "read_record", return_value=record), \
             patch.object(desktop, "verified_service", side_effect=[record, newer]), \
             patch.object(desktop, "service_locked", return_value=False), \
             patch.object(desktop, "request", return_value={"ok": True, "refresh": "restarting"}), \
             patch.object(desktop.subprocess, "Popen", return_value=child) as spawn:
            _, result = desktop.ensure_service(self.root)
            self.assertEqual(result, newer)
            arguments = spawn.call_args.args[0]
            self.assertEqual(arguments[arguments.index("--port") + 1], str(record["port"]))
            child.terminate.assert_not_called()
            child.kill.assert_not_called()

    def test_refresh_reservation_rejects_game_setup_and_late_mutations(self):
        launcher = Launcher(self.root)
        launcher.reserve_setup()
        self.assertFalse(launcher.reserve_desktop_refresh())
        launcher.release_setup()
        process = Mock()
        process.poll.return_value = None
        launcher.process = process
        self.assertFalse(launcher.reserve_desktop_refresh())
        launcher.process = None
        self.assertTrue(launcher.reserve_desktop_refresh())
        for operation in (launcher.reserve_setup, lambda: launcher.configure("/fixture"), launcher.launch, launcher.backup):
            with self.assertRaises(LauncherError):
                operation()
        self.assertFalse(launcher.setup_busy)
        process.terminate.assert_not_called()

    def test_external_game_lock_prevents_desktop_refresh(self):
        runtime = self.base / "runtime"
        (runtime / "private").mkdir(parents=True)
        (runtime / "tools").mkdir()
        (runtime / "tools/play-msfs.sh").write_text("#!/bin/sh\n")
        launcher = Launcher(self.root, str(runtime))
        with (runtime / "private/play.lock").open("w") as external:
            fcntl.flock(external, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertFalse(launcher.reserve_desktop_refresh())
            self.assertFalse(launcher.desktop_closing)
        self.assertTrue(launcher.reserve_desktop_refresh())


if __name__ == "__main__":
    unittest.main()
