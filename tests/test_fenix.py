# SPDX-License-Identifier: MIT
import json
from pathlib import Path
import tempfile
import threading
import unittest
import zipfile
import shutil
import subprocess
import sys
from unittest.mock import patch
from flightdeck import fenix
from flightdeck.backend import LauncherError


class FakeLauncher:
    def __init__(self, root):
        self.lock = threading.RLock()
        self.runtime = root
        self.state_dir = root / "state"
        self.setup_busy = False
        self.desktop_closing = False
        self.process = None

    def require_open(self): pass
    def _external(self, **kwargs): return False
    def reserve_setup(self):
        if self.setup_busy or self.process: raise LauncherError("busy")
        self.setup_busy = True
    def release_setup(self): self.setup_busy = False


class FenixTests(unittest.TestCase):
    def test_stop_keeps_interactive_reservation_until_all_helpers_are_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launcher = FakeLauncher(root)
            manager = fenix.FenixManager(launcher)
            entered, cleaning, finish = threading.Event(), threading.Event(), threading.Event()
            children = []
            def app(runtime, executable, progress, *, manager, wait):
                child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
                children.append(child)
                entered.set()
                return wait(child)
            def stop(runtime, progress):
                self.assertEqual(runtime, root)
                cleaning.set()
                self.assertTrue(finish.wait(3))
            with patch.object(fenix.core, "windows_app", side_effect=app), \
                 patch.object(fenix.core, "snapshot", return_value={"state": "installed", "idle": False}), \
                 patch.object(fenix.fenix_processes, "status", return_value=(True, False)), \
                 patch.object(fenix.fenix_processes, "stop", side_effect=stop), \
                 patch.object(launcher, "_external", side_effect=lambda **_: launcher.setup_busy):
                try:
                    started = manager.start("open", {})
                    self.assertTrue(entered.wait(2))
                    self.assertTrue(manager.snapshot()["can_stop"])
                    self.assertEqual(manager.start("stop", {}), started)
                    self.assertTrue(cleaning.wait(2))
                    self.assertTrue(launcher.setup_busy)
                    self.assertFalse(manager.snapshot()["can_stop"])
                    with self.assertRaises(LauncherError): manager.start("configure", {})
                    finish.set()
                    manager.worker.join(3)
                    self.assertFalse(manager.worker.is_alive())
                    self.assertFalse(launcher.setup_busy)
                    self.assertEqual(manager.job["state"], "complete")
                    self.assertEqual(manager.job["message"], "Fenix wurde beendet.")
                    self.assertIsNotNone(children[0].poll())
                finally:
                    finish.set()
                    for child in children:
                        if child.poll() is None: child.kill()
                        child.wait(timeout=3)
                    manager.worker.join(3)

    def test_stop_is_blocked_during_game_installer_and_other_jobs(self):
        with tempfile.TemporaryDirectory() as directory:
            launcher = FakeLauncher(Path(directory))
            manager = fenix.FenixManager(launcher)
            with patch.object(fenix.core, "snapshot", return_value={"state": "installed"}), \
                 patch.object(fenix.fenix_processes, "status", return_value=(True, False)):
                launcher.process = object()
                with self.assertRaises(LauncherError): manager.stop()
                launcher.process = None
                launcher.setup_busy = True
                for operation in ("installer", "configure", "install", "restore"):
                    manager.job = {"operation": operation, "state": "running"}
                    manager.job_runtime = launcher.runtime
                    with self.subTest(operation=operation), self.assertRaises(LauncherError): manager.stop()
                manager.job = {"operation": "open", "state": "running", "app_exited": True}
                with self.assertRaises(LauncherError): manager.stop()
                # The waiter may finish after snapshot has reported can_stop.
                with patch.object(manager, "snapshot", return_value={"can_stop": True}):
                    with self.assertRaises(LauncherError): manager.stop()
                self.assertFalse(manager.stop_requested.is_set())
                manager.job = {"operation": "open", "state": "running"}
                with patch.object(fenix.fenix_processes, "status", return_value=(True, True)):
                    with self.assertRaises(LauncherError): manager.stop()

    def test_background_fenix_stop_reserves_setup_and_runtime_lock(self):
        from contextlib import contextmanager
        with tempfile.TemporaryDirectory() as directory:
            launcher = FakeLauncher(Path(directory))
            manager = fenix.FenixManager(launcher)
            lease = []
            @contextmanager
            def locked(root, *, idle, recovery):
                self.assertTrue(launcher.setup_busy)
                self.assertFalse(idle)
                self.assertTrue(recovery)
                lease.append(root)
                try: yield root
                finally: lease.pop()
            def stop(root, progress):
                self.assertEqual(lease, [root])
                self.assertTrue(launcher.setup_busy)
            with patch.object(fenix.core, "snapshot", return_value={"state": "installed", "idle": False}), \
                 patch.object(fenix.core, "locked", side_effect=locked), \
                 patch.object(fenix.fenix_processes, "status", return_value=(True, False)), \
                 patch.object(fenix.fenix_processes, "stop", side_effect=stop):
                self.assertTrue(manager.stop()["ok"])
                manager.worker.join(3)
                self.assertFalse(launcher.setup_busy)
                self.assertEqual(manager.job["state"], "complete")
                self.assertEqual(lease, [])

    def test_release_extraction_uses_only_verified_fixed_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = b"synthetic Wine file"
            import hashlib
            lock = {"version": "fixture", "files": {"files/bin/wineserver": hashlib.sha256(payload).hexdigest()}, "integration": {}}
            archive = root / "fixture.zip"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("fixture/bundle.json", json.dumps(lock))
                output.writestr("fixture/payload/files/bin/wineserver", payload)
                output.writestr("fixture/fenix_patch/core.py", "raise RuntimeError('must never be imported')")
                output.writestr("fixture/../../outside", "must not be extracted")
            release = {"archive_root": "fixture", "url": "https://example.invalid/fixed-release.zip", "sha256": fenix.core.digest(archive)}
            (root / "release.json").write_text(json.dumps(release))
            def downloaded(url, target, expected, progress):
                self.assertEqual(url, release["url"])
                self.assertEqual(expected, fenix.core.digest(archive))
                shutil.copy2(archive, target)
                return target
            with patch.object(fenix.core, "ROOT", root), patch.object(fenix.core, "manifest", return_value=lock), patch.object(fenix.core, "download", side_effect=downloaded):
                result = fenix.obtain_bundle(root / "cache", None, lambda message: None)
                self.assertEqual((result / "payload/files/bin/wineserver").read_bytes(), payload)
                self.assertFalse((result / "fenix_patch").exists())
                self.assertFalse((root / "outside").exists())
                self.assertEqual(fenix.obtain_bundle(root / "cache", None, lambda message: None), result)

    def test_job_reserves_runtime_and_releases_after_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            launcher = FakeLauncher(Path(directory))
            manager = fenix.FenixManager(launcher)
            entered, release = threading.Event(), threading.Event()
            def operation(root, progress):
                self.assertEqual(root, launcher.runtime)
                entered.set(); release.wait(5)
                raise RuntimeError("Synthetic setup failure")
            with patch.object(fenix.core, "configure", side_effect=operation):
                manager.start("configure", {})
                self.assertTrue(entered.wait(2))
                self.assertTrue(launcher.setup_busy)
                with self.assertRaises(LauncherError): manager.start("configure", {})
                release.set(); manager.worker.join(3)
            self.assertFalse(launcher.setup_busy)
            self.assertEqual(manager.job["state"], "failed")
            self.assertEqual(manager.job["message"], "Synthetic setup failure")

    def test_downloaded_release_pin_and_local_bundle_match(self):
        lock = fenix.core.manifest()
        release = fenix.core.read_json(fenix.core.ROOT / "release.json")
        self.assertEqual(lock["version"], release["version"])
        self.assertTrue(release["url"].startswith(lock["repository"] + "/releases/download/v" + lock["version"] + "/"))
        self.assertEqual(len(release["sha256"]), 64)

    def test_display_readiness_requires_both_official_settings_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manager = fenix.FenixManager(FakeLauncher(root))
            config = root / "local/msfs-prefix" / fenix.core.CONFIG
            config.mkdir(parents=True)
            with patch.object(fenix.core, "snapshot", return_value={"state": "installed", "fenix_installed": True, "idle": True}):
                self.assertFalse(manager.snapshot()["settings_ready"])
                (config / "fenixConfig.xml").write_text("<fixture/>")
                self.assertFalse(manager.snapshot()["settings_ready"])
                (config / "persistancy.xml").write_text("<fixture/>")
                self.assertTrue(manager.snapshot()["settings_ready"])

    def test_unconfigured_runtime_and_bad_operation_do_not_start_work(self):
        with tempfile.TemporaryDirectory() as directory:
            launcher = FakeLauncher(Path(directory)); launcher.runtime = None
            manager = fenix.FenixManager(launcher)
            self.assertFalse(manager.snapshot()["can_change"])
            with self.assertRaises(LauncherError): manager.start("install", {})
            with self.assertRaises(LauncherError): manager.start("shell", {})
            self.assertFalse(launcher.setup_busy)


if __name__ == "__main__": unittest.main()
