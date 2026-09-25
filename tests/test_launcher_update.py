# SPDX-License-Identifier: MIT
"""GitHub update validation and real install/rollback in isolated user directories."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
from urllib.error import URLError

from flightdeck import desktop, launcher_update as updates
from flightdeck.backend import Launcher, LauncherError
from flightdeck.i18n import CATALOG, localize

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("update_test_installer", ROOT / "scripts/install-launcher.py")
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


def metadata(payload=b"archive", tag="v0.1.5"):
    return {"draft": False, "prerelease": False, "tag_name": tag, "body": "Release notes",
            "assets": [{"name": updates.ASSET, "state": "uploaded", "size": len(payload),
                        "digest": "sha256:" + hashlib.sha256(payload).hexdigest(),
                        "browser_download_url": updates.PROJECT + "/releases/download/" + tag + "/" + updates.ASSET}]}


class UpdateTests(unittest.TestCase):
    def setUp(self):
        running = patch.object(updates, "__version__", "0.1.4")
        running.start()
        self.addCleanup(running.stop)
        self.temp = tempfile.TemporaryDirectory(prefix="flightdeck-update-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.cancel = threading.Event()

    def archive(self, extra=(), version="0.1.5"):
        target = self.root / "update.tar.gz"
        with tarfile.open(target, "w:gz") as output:
            for name, value, kind in [("flightdeck-linux/flightdeck/__init__.py", f'__version__ = "{version}"\n'.encode(), tarfile.REGTYPE),
                                      ("flightdeck-linux/scripts/install-launcher.py", b"# fixture", tarfile.REGTYPE), *extra]:
                entry = tarfile.TarInfo(name)
                entry.size = len(value)
                entry.type = kind
                entry.linkname = "/tmp/never-follow-this-link"
                output.addfile(entry, io.BytesIO(value))
        return target

    def test_semantic_versions_and_stable_metadata(self):
        self.assertGreater(updates.version("v0.10.0"), updates.version("0.9.99"))
        for value in (None, "0.1", "v01.2.3", "1.0.0-rc.1", "../1.2.3", "0.1.5\n"):
            with self.subTest(value=value), self.assertRaises(updates.UpdateError):
                updates.version(value)
        result = updates.release_metadata(metadata())
        self.assertEqual(result["version"], "0.1.5")
        self.assertEqual(result["url"], metadata()["assets"][0]["browser_download_url"])
        for key, value in (("prerelease", True), ("draft", True), ("assets", []), ("assets", metadata()["assets"] * 2)):
            with self.subTest(key=key), self.assertRaises(updates.UpdateError):
                updates.release_metadata({**metadata(), key: value})

    def test_asset_requires_exact_repository_digest_and_bounded_size(self):
        for key, value in (("digest", None), ("digest", "sha256:short"), ("size", 0), ("size", True),
                           ("size", updates.MAX_ARCHIVE + 1), ("state", "new"),
                           ("browser_download_url", "https://github.com/someone/other/releases/download/v0.1.5/" + updates.ASSET)):
            raw = metadata()
            raw["assets"][0][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(updates.UpdateError):
                updates.release_metadata(raw)

    def test_redirects_only_follow_github_https_hosts(self):
        for url in ("http://github.com/a", "https://github.com.evil.test/a", "https://user@github.com/a",
                    "https://github.com:bad/a", "file:///tmp/a", "https://[invalid"):
            self.assertFalse(updates.safe_url(url), url)
            with self.assertRaises(updates.UpdateError):
                updates.GitHubRedirects().redirect_request(None, None, 302, "", {}, url)
        self.assertTrue(updates.safe_url("https://release-assets.githubusercontent.com/path?sig=fixture"))

    def test_latest_response_is_bounded_and_validated(self):
        for value in (b"not-json", b"null", b"x" * (2 * 1024 * 1024 + 1)):
            with patch.object(updates, "open_url", return_value=io.BytesIO(value)), self.assertRaises(updates.UpdateError):
                updates.latest_release()
        with patch.object(updates, "open_url", return_value=io.BytesIO(json.dumps(metadata()).encode())) as remote:
            self.assertEqual(updates.latest_release()["version"], "0.1.5")
            remote.assert_called_once_with(updates.API)

    def test_download_checks_entire_payload_size_digest_and_cancellation(self):
        payload = b"fixture" * 70000
        release = updates.release_metadata(metadata(payload))
        progress = []
        with patch.object(updates, "open_url", return_value=io.BytesIO(payload)):
            updates.download(release, self.root / "good", self.cancel, lambda *values: progress.append(values))
        self.assertEqual((self.root / "good").read_bytes(), payload)
        self.assertEqual(progress[-1], (len(payload), len(payload)))
        for index, bad in enumerate((payload[:-1], payload + b"x", b"X" + payload[1:])):
            with patch.object(updates, "open_url", return_value=io.BytesIO(bad)), self.assertRaises(updates.UpdateError):
                updates.download(release, self.root / f"bad-{index}", self.cancel, lambda *_: None)
        self.cancel.set()
        with patch.object(updates, "open_url", return_value=io.BytesIO(payload)), self.assertRaises(updates.Cancelled):
            updates.download(release, self.root / "cancelled", self.cancel, lambda *_: None)

    def test_archive_rejects_escape_links_duplicates_version_and_expansion(self):
        cases = [("../escaped", b"x", tarfile.REGTYPE), ("/tmp/escaped", b"x", tarfile.REGTYPE),
                 ("flightdeck-linux/../escaped", b"x", tarfile.REGTYPE),
                 ("flightdeck-linux/link", b"", tarfile.SYMTYPE), ("flightdeck-linux/link", b"", tarfile.LNKTYPE),
                 ("flightdeck-linux/device", b"", tarfile.CHRTYPE),
                 ("flightdeck-linux/flightdeck/__init__.py", b"overwrite", tarfile.REGTYPE)]
        for index, entry in enumerate(cases):
            with self.subTest(entry=entry), self.assertRaises(updates.UpdateError):
                updates.extract(self.archive([entry]), self.root / f"bad-{index}", "0.1.5", self.cancel)
        with self.assertRaises(updates.UpdateError):
            updates.extract(self.archive(version="0.1.4"), self.root / "wrong-version", "0.1.5", self.cancel)
        with patch.object(updates, "MAX_EXPANDED", 4), self.assertRaises(updates.UpdateError):
            updates.extract(self.archive(), self.root / "too-big", "0.1.5", self.cancel)
        source = updates.extract(self.archive(), self.root / "good", "0.1.5", self.cancel)
        self.assertTrue((source / "scripts/install-launcher.py").is_file())
        self.assertFalse((self.root / "escaped").exists())

    def make_manager(self):
        launcher = Launcher(self.root / "state")
        self.addCleanup(launcher.cloud_saves.close)
        manager = launcher.launcher_updates
        self.addCleanup(manager.close)
        return launcher, manager

    def finish(self, manager):
        manager.thread.join(15)
        self.assertFalse(manager.thread.is_alive(), "update worker did not finish")
        return manager.snapshot()

    def test_source_checkout_checks_but_never_installs(self):
        launcher, manager = self.make_manager()
        with patch.object(updates, "latest_release", return_value=updates.release_metadata(metadata())):
            manager.start("check")
            state = self.finish(manager)
        self.assertTrue(state["update_available"])
        self.assertFalse(state["managed"])
        self.assertFalse(state["can_install"])
        self.assertIn("official installer", localize(state, "en")["unavailable_reason"])
        with self.assertRaises(updates.UpdateError):
            manager.start("install", state["check_id"])
        self.assertFalse(launcher.setup_busy)

    def test_failed_recheck_invalidates_previous_install_offer(self):
        _, manager = self.make_manager()
        with patch.object(updates, "latest_release", return_value=updates.release_metadata(metadata())):
            manager.start("check")
            self.finish(manager)
        with patch.object(updates, "latest_release", side_effect=URLError("private network detail")):
            manager.start("check")
            state = self.finish(manager)
        self.assertIsNone(state["check_id"])
        self.assertIsNone(state["update_available"])
        self.assertEqual(state["job"]["state"], "failed")
        self.assertIn(state["job"]["error"], CATALOG)
        self.assertNotIn("private network", state["job"]["error"])

    def test_check_during_game_and_cancel_do_not_reserve_runtime(self):
        launcher, manager = self.make_manager()
        waiting, proceed = threading.Event(), threading.Event()
        self.addCleanup(proceed.set)
        def remote():
            waiting.set()
            proceed.wait(5)
            return updates.release_metadata(metadata())
        with patch.object(launcher, "_external", return_value=True), patch.object(updates, "latest_release", side_effect=remote):
            manager.start("check")
            self.assertTrue(waiting.wait(3))
            self.assertFalse(launcher.setup_busy)
            self.assertFalse(manager.snapshot()["can_check"])
            with self.assertRaises(updates.UpdateError): manager.cancel("old-job")
            manager.cancel(manager.job["id"])
            proceed.set()
            self.assertEqual(self.finish(manager)["job"]["state"], "cancelled")
            self.assertIsNone(manager.check_id)

    def installed_manager(self):
        source = self.root / "source"
        source.mkdir()
        for name in ("flightdeck", "ui", "scripts/runtime", "compat/fenix"):
            shutil.copytree(ROOT / name, source / name, ignore=shutil.ignore_patterns("__pycache__", "tests", "*.pyc"))
        for name in ("scripts/install-launcher.py", "scripts/install-launcher-gui.py", "compat/upstreams.lock.json", "compat/bootstrap.lock.json", "LICENSE"):
            shutil.copyfile(ROOT / name, source / name)
        # Keep the simulated installed version stable across real releases.
        (source / "flightdeck/__init__.py").write_text('__version__ = "0.1.4"\n')
        data = self.root / "data"
        initial = installer.install(source, data, self.root / "bin", self.root / "apps", language="de")
        launcher, manager = self.make_manager()
        manager.context = installer, data, initial["current"]
        return source, data, initial, launcher, manager

    def test_real_verified_install_restart_and_rollback_preserve_settings(self):
        source, data, initial, launcher, manager = self.installed_manager()
        settings = launcher.state_dir / "unrelated-settings.json"
        settings.write_text('{"keep":"exactly"}')
        (source / "flightdeck/__init__.py").write_text('__version__ = "0.1.5"\n')
        archive = self.root / "release.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            output.add(source, arcname="flightdeck-linux")
        payload = archive.read_bytes()
        manager.release = updates.release_metadata(metadata(payload))
        manager.check_id = "checked-release"
        with patch.object(updates, "open_url", return_value=io.BytesIO(payload)):
            manager.start("install", "checked-release")
            result = self.finish(manager)
        self.assertEqual(result["job"]["state"], "complete", result["job"])
        self.assertTrue(result["pending_restart"])
        self.assertTrue(result["can_restart"])
        self.assertFalse(result["can_install"])
        self.assertFalse(launcher.setup_busy)
        current = installer.load_installation(data)
        self.assertEqual(current["previous"], initial["current"])
        self.assertNotEqual(current["current"], initial["current"])
        for identity in (current["current"], current["previous"]): installer.verify_release(data, identity)
        self.assertEqual(settings.read_text(), '{"keep":"exactly"}')
        with patch.object(updates.subprocess, "Popen") as process:
            process.return_value.poll.return_value = None
            manager.restart("de")
            self.assertIn(str(self.root / "bin/flightdeck"), process.call_args.args[0])
            self.assertIn(str(launcher.state_dir), process.call_args.args[0])
            self.assertNotIn("shell", process.call_args.kwargs)
            self.assertFalse(manager.snapshot()["can_restart"])
            with self.assertRaises(updates.UpdateError): manager.restart("de")
            process.return_value.poll.return_value = 1
            self.assertTrue(manager.snapshot()["can_restart"])
            self.assertEqual(manager.job["operation"], "restart")
            self.assertEqual(manager.job["state"], "failed")
        manager.context = installer, data, current["current"]  # next launched service
        manager.start("rollback")
        self.assertEqual(self.finish(manager)["job"]["state"], "complete")
        self.assertEqual(installer.load_installation(data)["current"], initial["current"])
        self.assertEqual(settings.read_text(), '{"keep":"exactly"}')

    def test_installed_restart_hands_off_real_local_service_to_new_release(self):
        source, data, initial, launcher, _ = self.installed_manager()
        browser = self.root / "bin/chromium"
        browser.write_text("#!/bin/sh\nexit 0\n")  # no real desktop window
        browser.chmod(0o700)
        environment = {**os.environ, "PATH": str(browser.parent) + os.pathsep + os.environ["PATH"],
                       "XDG_DATA_HOME": str(self.root / "empty-data")}
        process = subprocess.Popen([sys.executable, "-B", "-m", "flightdeck", "--desktop-service", "--state-dir", str(launcher.state_dir)],
                                   cwd=data / "releases" / initial["current"], env=environment,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        def cleanup():
            record = desktop.read_record(launcher.state_dir)
            if record and desktop.process_start(record["pid"]) == record["start"]:
                os.kill(record["pid"], signal.SIGTERM)
            if process.poll() is None: process.terminate()
            process.wait(timeout=5)
        self.addCleanup(cleanup)
        def wait_for(predicate):
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                record = desktop.read_record(launcher.state_dir)
                if record and predicate(record): return record
                time.sleep(0.05)
            self.fail("Local update service did not reach the expected state")
        old = wait_for(lambda record: desktop.verified_service(launcher.state_dir, record))
        self.assertEqual(old["pid"], process.pid)
        (source / "flightdeck/__init__.py").write_text('__version__ = "0.1.5"\n')
        installer.install(source, data, self.root / "bin", self.root / "apps", language="de", expected_current=initial["current"])
        self.assertTrue(desktop.request(old, "/api/launcher-update")["pending_restart"])
        self.assertTrue(desktop.request(old, "/api/launcher-update/restart", {})["ok"])
        new = wait_for(lambda record: record["pid"] != old["pid"] and desktop.verified_service(launcher.state_dir, record))
        self.assertEqual(new["port"], old["port"])
        self.assertEqual(desktop.request(new, "/api/status")["app"]["version"], "0.1.5")
        self.assertFalse(desktop.request(new, "/api/launcher-update")["pending_restart"])
        process.wait(timeout=5)

    def test_install_rejects_busy_stale_check_and_changed_installation(self):
        source, data, initial, launcher, manager = self.installed_manager()
        manager.release = updates.release_metadata(metadata())
        manager.check_id = "fresh"
        with self.assertRaises(updates.UpdateError): manager.start("install", "stale")
        for flag in ("setup_busy", "desktop_closing"):
            setattr(launcher, flag, True)
            with self.assertRaises(updates.UpdateError): manager.start("install", "fresh")
            setattr(launcher, flag, False)
        with patch.object(launcher, "_external", return_value=True):
            with self.assertRaises(updates.UpdateError): manager.start("install", "fresh")
        with patch.object(launcher, "_require_cloud_idle", side_effect=LauncherError("cloud busy")):
            with self.assertRaises(LauncherError): manager.start("install", "fresh")
        self.assertFalse(launcher.setup_busy)
        before = (data / "installation.json").read_bytes()
        with self.assertRaises(installer.InstallError):
            installer.install(source, data, self.root / "bin", self.root / "apps", expected_current="0" * 64)
        with self.assertRaises(installer.InstallError): installer.rollback(data, expected_current="0" * 64)
        self.assertEqual((data / "installation.json").read_bytes(), before)

    def test_corrupt_download_and_cancel_leave_current_release_untouched(self):
        _, data, initial, launcher, manager = self.installed_manager()
        manager.release = updates.release_metadata(metadata())
        manager.check_id = "fresh"
        before = (data / "installation.json").read_bytes()
        with patch.object(updates, "open_url", return_value=io.BytesIO(b"garbage")):
            manager.start("install", "fresh")
            self.assertEqual(self.finish(manager)["job"]["state"], "failed")
        self.assertEqual((data / "installation.json").read_bytes(), before)
        self.assertFalse(launcher.setup_busy)
        waiting = threading.Event()
        def cancelled_download(*args):
            waiting.set()
            manager.cancel_event.wait(5)
            raise updates.Cancelled()
        with patch.object(updates, "download", side_effect=cancelled_download):
            manager.start("install", "fresh")
            self.assertTrue(waiting.wait(3))
            self.assertTrue(launcher.setup_busy)
            manager.cancel(manager.job["id"])
            self.assertEqual(self.finish(manager)["job"]["state"], "cancelled")
        self.assertEqual((data / "installation.json").read_bytes(), before)
        self.assertFalse(launcher.setup_busy)
