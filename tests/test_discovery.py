"""Bounded discovery of synthetic prepared runtimes; no game execution."""
# SPDX-License-Identifier: MIT
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck.backend import Launcher
from flightdeck.i18n import localize


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.data = self.root / "data"
        self.source = self.root / "source"
        self.launcher = Launcher(self.root / "state")
        self.manager = self.launcher.setup
        self.manager.source_root = self.source
        self.environment = patch("flightdeck.setup.data_home", return_value=self.data)
        self.environment.start()

    def tearDown(self):
        self.manager.close()
        self.environment.stop()
        self.temp.cleanup()

    def runtime(self, path):
        for name in ("tools/play-msfs.sh", "games/MSFS2024/FlightSimulator2024.exe", "local/msfs-prefix/system.reg", "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"):
            file = path / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(b"synthetic fixture, never execute")
            if name == "tools/play-msfs.sh":
                file.chmod(0o700)
        (path / "private").mkdir()
        return path

    def test_empty_search_does_not_create_directories_or_jobs(self):
        with patch("flightdeck.setup.subprocess.Popen", side_effect=AssertionError("discovery must not run programs")), patch("flightdeck.setup.subprocess.run", side_effect=AssertionError("discovery must not run programs")):
            result = self.manager.discover()
        self.assertEqual(result, {"ok": True, "runtimes": [], "checked_count": 0, "limited": False})
        self.assertFalse(self.data.exists())
        self.assertIsNone(self.manager.snapshot()["job"])
        self.assertFalse(self.launcher.setup_busy)

    def test_configured_first_deduplicated_and_read_only(self):
        configured = self.runtime(self.root / "configured")
        self.launcher.configure(str(configured))
        folder = self.data / "flightdeck/runtimes"
        folder.mkdir(parents=True)
        (folder / "alias").symlink_to(configured)
        other = self.runtime(folder / "other")
        before = {str(p): p.lstat().st_mtime_ns for p in configured.rglob("*")}
        result = self.manager.discover()
        self.assertEqual([row["path"] for row in result["runtimes"]], [str(configured), str(other)])
        self.assertTrue(result["runtimes"][0]["configured"])
        self.assertTrue(all(row["ready"] for row in result["runtimes"]))
        self.assertEqual(before, {str(p): p.lstat().st_mtime_ns for p in configured.rglob("*")})
        self.assertFalse((configured / "private/play.lock").exists())
        self.assertFalse(self.launcher.setup_busy)

    def test_source_siblings_are_explicit_candidates_only(self):
        (self.source / "compat").mkdir(parents=True)
        (self.source / "compat/upstreams.lock.json").write_text("{}")
        (self.source / "scripts/runtime").mkdir(parents=True)
        sibling = self.runtime(self.root / "msfs-linux")
        nested = self.runtime(sibling / "runtime")
        self.runtime(self.root / "unrelated")
        paths = {row["path"] for row in self.manager.discover()["runtimes"]}
        self.assertEqual(paths, {str(sibling), str(nested)})

    def test_incomplete_runtime_has_localized_checks(self):
        runtime = self.runtime(self.data / "flightdeck/runtimes/incomplete")
        (runtime / "local/msfs-prefix/system.reg").unlink()
        result = self.manager.discover()
        self.assertEqual(len(result["runtimes"]), 1)
        row = result["runtimes"][0]
        self.assertFalse(row["ready"])
        self.assertFalse(next(check for check in row["checks"] if check["id"] == "prefix")["ok"])
        english = localize(result, "en")["runtimes"][0]
        self.assertEqual(english["path"], str(runtime))
        self.assertEqual(next(check for check in english["checks"] if check["id"] == "prefix")["detail"], "Wine prefix is missing or is not readable.")

    def test_at_most_32_direct_entries_and_no_recursive_scan(self):
        folder = self.data / "flightdeck/runtimes"
        for index in range(40):
            self.runtime(folder / f"candidate-{index:02}")
        result = self.manager.discover()
        self.assertTrue(result["limited"])
        self.assertEqual(result["checked_count"], 32)
        self.assertEqual(len(result["runtimes"]), 32)
        # A nested prepared runtime must not become another suggestion.
        self.runtime(folder / "candidate-00/nested")
        self.assertFalse(any(row["path"].endswith("/nested") for row in self.manager.discover()["runtimes"]))

    def test_unrelated_game_folder_and_broken_links_are_not_suggested(self):
        folder = self.data / "flightdeck/runtimes"
        folder.mkdir(parents=True)
        (folder / "broken").symlink_to(self.root / "missing")
        (folder / "loop").symlink_to(folder / "loop")
        game = folder / "just-game"
        game.mkdir()
        (game / "FlightSimulator2024.exe").write_text("not a prepared runtime")
        self.assertEqual(self.manager.discover()["runtimes"], [])

    def test_discovered_runtime_uses_existing_check_and_connect_flow(self):
        runtime = self.runtime(self.data / "flightdeck/runtimes/msfs")
        found = self.manager.discover()["runtimes"][0]
        self.assertIsNone(self.launcher.runtime)
        self.manager.check({"mode": "existing", "runtime_path": found["path"]})
        self.manager.thread.join(timeout=2)
        job = self.manager.snapshot()["job"]
        self.assertEqual(job["state"], "ready")
        self.assertIsNone(self.launcher.runtime)
        self.manager.start(job["id"])
        self.manager.thread.join(timeout=2)
        self.assertEqual(self.manager.snapshot()["state"], "complete")
        self.assertEqual(self.launcher.runtime, runtime)


if __name__ == "__main__":
    unittest.main()
