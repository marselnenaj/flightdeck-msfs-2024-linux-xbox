"""Startup discovery uses synthetic metadata, never accounts or network."""
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck.backend import Launcher
from flightdeck import game_update, launcher_update


class StartupUpdateTests(unittest.TestCase):
    def setUp(self):
        running = patch.object(launcher_update, "__version__", "0.1.5")
        running.start()
        self.addCleanup(running.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        (self.runtime / "tools").mkdir(parents=True)
        (self.runtime / "tools/play-msfs.sh").write_text("synthetic")
        (self.runtime / "private").mkdir(mode=0o700)
        (self.runtime / "games/MSFS2024").mkdir(parents=True)
        self.launcher = Launcher(self.root / "state", str(self.runtime))
        self.manager = self.launcher.startup_updates
        self.addCleanup(self.launcher.cloud_saves.close)
        self.addCleanup(self.launcher.launcher_updates.close)
        self.addCleanup(self.manager.close)
        for name, value in (("installed_identity", {"version": "1.8.16.0"}),
                            ("tools", (Path("synthetic-cli"), "hash", [])),
                            ("configured_market", "AT"), ("package_info", {"version": "1.9.0.0"})):
            mocker = patch.object(game_update, name, return_value=value)
            setattr(self, name, mocker.start())
            self.addCleanup(mocker.stop)
        latest = patch.object(launcher_update, "latest_release", return_value={"version": "0.1.6",
            "release_url": launcher_update.PROJECT + "/releases/tag/v0.1.6", "notes": "", "size": 1})
        self.latest = latest.start()
        self.addCleanup(latest.stop)

    def finish(self):
        for thread in (self.manager.thread, self.launcher.launcher_updates.thread):
            if thread:
                thread.join(3)
                self.assertFalse(thread.is_alive())

    def test_startup_discovers_both_updates_without_install_plan_or_runtime_reservation(self):
        self.manager.check()
        self.finish()
        self.assertTrue(self.manager.snapshot(self.runtime)["update_available"])
        self.assertTrue(self.launcher.launcher_updates.snapshot()["update_available"])
        self.assertIsNone(self.launcher.setup.job)
        self.assertFalse(self.launcher.setup_busy)
        with self.launcher.runtime_lock():
            pass
        view = game_update.snapshot(self.launcher)
        self.assertEqual(view["latest_version"], "1.9.0.0")
        self.assertFalse(view["can_start"])
        self.assertTrue(view["can_check"])

    def test_reopening_or_duplicate_windows_does_not_repeat_requests(self):
        for _ in range(3):
            self.manager.check()
            self.finish()
        self.latest.assert_called_once()
        self.package_info.assert_called_once()

    def test_slow_store_does_not_own_runtime_and_drops_cancelled_result(self):
        waiting = threading.Event()
        def slow(cli, checksum, runtime, market, cancel):
            waiting.set()
            cancel.wait(3)
            return {"version": "1.9.0.0"}
        self.package_info.side_effect = slow
        self.manager.check()
        self.assertTrue(waiting.wait(2))
        with self.launcher.runtime_lock():
            pass
        self.assertFalse(self.launcher.setup_busy)
        self.manager.close()
        self.finish()
        self.assertNotIn("latest_version", self.manager.snapshot(self.runtime))

    def test_busy_game_defers_only_game_discovery(self):
        self.launcher.setup_busy = True
        self.assertTrue(self.manager.check()["deferred"])
        self.finish()
        self.latest.assert_called_once()
        self.package_info.assert_not_called()
        self.launcher.setup_busy = False
        self.manager.check()
        self.finish()
        self.package_info.assert_called_once()

    def test_game_discovery_remains_bound_to_the_selected_runtime(self):
        self.manager.check()
        self.finish()
        self.assertEqual(self.manager.snapshot(self.root / "other-runtime"), {})
        self.manager.records[self.runtime]["installed_version"] = "1.7.0.0"
        self.assertIsNone(game_update.snapshot(self.launcher)["latest_version"])

    def test_authentication_needs_explicit_sign_in_and_errors_omit_private_data(self):
        for error in (game_update.AuthRequired("private account"), OSError("private path")):
            self.manager.records.clear()
            self.package_info.side_effect = error
            self.manager.check()
            self.finish()
            result = self.manager.snapshot(self.runtime)
            self.assertEqual(result["state"], "failed")
            self.assertNotIn("private", str(result))
            self.assertEqual(result.get("auth_required", False), isinstance(error, game_update.AuthRequired))
            self.assertIsNone(self.launcher.setup.job)

    def test_manual_plan_wins_over_background_discovery(self):
        self.manager.check()
        self.finish()
        self.launcher.setup.job = {"mode": "update", "runtime_path": str(self.runtime), "state": "ready",
                                  "latest_version": "1.10.0.0", "update_available": True}
        self.assertEqual(game_update.snapshot(self.launcher)["latest_version"], "1.10.0.0")
        self.manager.records.clear()
        self.manager.check()
        self.package_info.assert_called_once()

    def test_manual_launcher_check_suppresses_immediate_automatic_recheck(self):
        self.launcher.launcher_updates.start("check")
        self.finish()
        self.manager.check()
        self.finish()
        self.latest.assert_called_once()
        self.launcher.launcher_updates.last_check_attempt -= 1801
        self.manager.check()
        self.finish()
        self.assertEqual(self.latest.call_count, 2)
