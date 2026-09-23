# SPDX-License-Identifier: MIT
import json
from pathlib import Path
import tempfile
import threading
import unittest
import zipfile
import shutil
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
