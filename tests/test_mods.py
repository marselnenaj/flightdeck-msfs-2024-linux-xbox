# SPDX-License-Identifier: MIT
"""Bounded synthetic configuration/add-on fixtures; no mod code or GUI runs."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from flightdeck import mods
from flightdeck.backend import LauncherError
from flightdeck.i18n import localize


class ModsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        self.prefix = self.runtime / "local/msfs-prefix"
        self.users = self.prefix / "drive_c/users"
        self.users.mkdir(parents=True)
        (self.prefix / "dosdevices").mkdir()
        (self.prefix / "dosdevices/c:").symlink_to(self.prefix / "drive_c")
        (self.prefix / "dosdevices/z:").symlink_to("/")
        (self.runtime / "private").mkdir()
        self.launcher = SimpleNamespace(runtime=self.runtime, lock=threading.RLock(), setup_busy=False,
                                        desktop_closing=False, require_open=lambda: None)
        self.community = self.prefix / "drive_c/Packages/Community"
        self.community.mkdir(parents=True)

    def usercfg(self, contents='InstalledPackagesPath "C:\\Packages"', profile="pilot", encoding="utf-8"):
        path = self.users / profile / "AppData/Roaming/Microsoft Flight Simulator 2024/UserCfg.opt"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents + "\n", encoding=encoding)
        return path

    def package(self, name, value=None):
        root = self.community / name
        root.mkdir()
        (root / "manifest.json").write_text(json.dumps(value or {"title": "Synthetic aircraft", "package_version": "1.2.3",
                                                               "creator": "Example author", "content_type": "AIRCRAFT"}))
        return root

    def test_unknown_does_not_guess_existing_community_directory(self):
        self.assertEqual(mods.snapshot(self.launcher)["state"], "unknown")
        self.launcher.runtime = None
        self.assertEqual(mods.snapshot(self.launcher)["state"], "unconfigured")

    def test_actual_usercfg_maps_windows_case_and_reads_only_manifest(self):
        cfg = self.usercfg('InstalledPackagesPath "c:\\packages"')
        package = self.package("example-aircraft")
        marker = self.root / "must-not-exist"
        (package / "install.py").write_text(f"open({str(marker)!r}, 'w').write('executed')")
        before = cfg.stat().st_mtime_ns
        with patch.object(mods, "_opener", return_value=["/usr/bin/xdg-open"]):
            result = mods.snapshot(self.launcher)
        self.assertEqual(result["state"], "ready")
        self.assertEqual(result["source"], "usercfg")
        self.assertEqual(result["folder_path"], str(self.community))
        self.assertEqual(result["mods"][0]["version"], "1.2.3")
        self.assertTrue(result["can_open"])
        self.assertEqual(cfg.stat().st_mtime_ns, before)
        self.assertFalse(marker.exists())

    def test_unicode_utf16_and_z_mapping_are_supported(self):
        target = self.root / "Packages with spaces ä"
        (target / "Community").mkdir(parents=True)
        self.usercfg('InstalledPackagesPath "Z:' + str(target).replace("/", "\\") + '"', encoding="utf-16")
        self.assertEqual(mods.snapshot(self.launcher)["folder_path"], str(target / "Community"))

    def test_community_mod_directory_symlinks_are_read_but_manifest_symlinks_are_not(self):
        self.usercfg()
        outside = self.root / "linked-addon"
        outside.mkdir()
        (outside / "manifest.json").write_text('{"title":"Linked addon","package_version":"2"}')
        (self.community / "linked").symlink_to(outside)
        broken = self.community / "broken"
        broken.symlink_to(self.root / "absent")
        bad = self.community / "manifest-link"
        bad.mkdir()
        (bad / "manifest.json").symlink_to(outside / "manifest.json")
        items = {entry["id"]: entry for entry in mods.snapshot(self.launcher)["mods"]}
        self.assertEqual(items["linked"]["status"], "available")
        self.assertTrue(items["linked"]["is_link"])
        self.assertEqual(items["broken"]["status"], "unreadable")
        self.assertEqual(items["manifest-link"]["status"], "unreadable")

    def test_bad_oversize_fifo_and_absent_manifests_are_bounded_statuses(self):
        self.usercfg()
        for name in ("bad", "large", "fifo", "missing"):
            (self.community / name).mkdir()
        (self.community / "bad/manifest.json").write_text("[]")
        (self.community / "large/manifest.json").write_bytes(b"x" * (mods.MANIFEST_LIMIT + 1))
        os.mkfifo(self.community / "fifo/manifest.json")
        items = {entry["id"]: entry["status"] for entry in mods.snapshot(self.launcher)["mods"]}
        self.assertEqual(items, {"bad":"invalid_manifest", "large":"invalid_manifest", "fifo":"invalid_manifest", "missing":"missing_manifest"})

    def test_inventory_and_strings_are_bounded_and_titles_not_translated(self):
        self.usercfg()
        for i in range(6):
            self.package(str(i), {"title":"<script>untrusted</script>\x00", "creator":"x"*2000,"package_version":"Der Download wird fortgesetzt …"})
        with patch.object(mods, "MAX_MODS", 4):
            result = mods.snapshot(self.launcher)
        self.assertTrue(result["limited"])
        self.assertEqual(result["count"], 4)
        self.assertEqual(result["scanned_count"], 4)
        self.assertEqual(result["mods"][0]["name"], "<script>untrusted</script>")
        self.assertEqual(len(result["mods"][0]["creator"]), 512)
        translated = localize(result, "en")
        self.assertEqual(translated["mods"], result["mods"])
        self.assertIn("Community folder", translated["message"])

    def test_conflicting_profiles_and_too_many_profiles_are_not_guessed(self):
        self.usercfg()
        self.usercfg('InstalledPackagesPath "C:\\Elsewhere"', profile="second")
        result = mods.snapshot(self.launcher)
        self.assertEqual(result["state"], "ambiguous")
        self.assertIsNone(result["folder_path"])
        with patch.object(mods, "MAX_USERS", 1):
            self.assertTrue(mods.snapshot(self.launcher)["limited"])

    def test_explicit_runtime_location_and_missing_folder_are_honest(self):
        settings = self.runtime / "private/runtime.json"
        settings.write_text(json.dumps({"community_path":str(self.community)}))
        result = mods.snapshot(self.launcher)
        self.assertEqual(result["source"], "runtime_config")
        self.assertEqual(result["state"], "ready")
        settings.write_text(json.dumps({"InstalledPackagesPath":str(self.root / "missing")}))
        self.assertEqual(mods.snapshot(self.launcher)["state"], "missing")
        self.assertFalse((self.root / "missing").exists())

    def test_invalid_config_is_not_empty_inventory_or_a_guessed_path(self):
        config = self.usercfg('InstalledPackagesPath "C:\\Packages"\nInstalledPackagesPath "C:\\Other"')
        self.assertEqual(mods.snapshot(self.launcher)["state"], "error")
        config.unlink()
        os.mkfifo(config)
        self.assertEqual(mods.snapshot(self.launcher)["state"], "error")

    def test_folder_open_has_fixed_argv_uri_and_never_runs_for_unknown_or_busy(self):
        self.usercfg()
        with patch.object(mods, "_opener", return_value=["/usr/bin/xdg-open"]), patch.object(mods.subprocess, "run", return_value=subprocess.CompletedProcess([],0)) as run:
            self.assertTrue(mods.open_folder(self.launcher)["ok"])
            self.assertEqual(run.call_args.args[0], ["/usr/bin/xdg-open", self.community.as_uri() + "/"])
            self.assertEqual(run.call_args.kwargs["stdout"], subprocess.DEVNULL)
            self.assertNotIn("shell", run.call_args.kwargs)
            self.launcher.setup_busy = True
            self.assertFalse(mods.snapshot(self.launcher)["can_open"])
            with self.assertRaises(LauncherError):
                mods.open_folder(self.launcher)
            self.assertEqual(run.call_count, 1)
        self.launcher.setup_busy = False
        with patch.object(mods, "_opener", return_value=None):
            with self.assertRaisesRegex(LauncherError, "Ordneröffner"):
                mods.open_folder(self.launcher)

    def test_msix_family_is_derived_from_actual_identity(self):
        game = self.runtime / "games/MSFS2024"
        game.mkdir(parents=True)
        (game / "MicrosoftGame.Config").write_text('<Game><Identity Name="Example.Game" Publisher="CN=Example"/></Game>')
        family = mods._family(self.runtime)
        self.assertRegex(family, r'^Example\.Game_[0-9a-z]{13}$')
        cache = self.users / "pilot/AppData/Local/Packages" / family / "LocalCache"
        cache.mkdir(parents=True)
        (cache / "UserCfg.opt").write_text('InstalledPackagesPath "C:\\Packages"')
        self.assertEqual(mods.snapshot(self.launcher)["state"], "ready")


if __name__ == "__main__":
    unittest.main()
