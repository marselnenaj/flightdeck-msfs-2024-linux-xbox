# SPDX-License-Identifier: MIT
import json
import os
from pathlib import Path
import shutil
import tempfile
import threading
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

from flightdeck import gsx_core as core, gsx
from flightdeck.backend import LauncherError


class GSXTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "runtime with spaces"
        self.prefix = self.root / "local/msfs-prefix"
        for path in [self.root / "private", self.root / "tools", self.prefix / "drive_c/windows/system32", self.prefix / "dosdevices", self.root / "stock/files/bin"]:
            path.mkdir(parents=True, exist_ok=True)
        (self.root / "runner").symlink_to("stock")
        (self.prefix / "dosdevices/c:").symlink_to("../drive_c")
        (self.prefix / "system.reg").write_text("WINE REGISTRY Version 2\n")
        (self.root / "private/runtime.json").write_text('{"format":1,"game_id":"msfs2024","market":"AT"}')
        self.config = self.prefix / "drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024"
        self.community = self.config / "Packages/Community"
        self.community.mkdir(parents=True)
        (self.config / "UserCfg.opt").write_text('InstalledPackagesPath "C:\\users\\steamuser\\AppData\\Roaming\\Microsoft Flight Simulator 2024\\Packages"\n')
        self.directory = self.prefix / core.MANAGER_DIR

    def installed(self):
        self.directory.mkdir(parents=True)
        (self.directory / "Couatl_Updater.exe").write_bytes(b"MZ")
        (self.directory / "QlmLicenseLib.dll").write_bytes(b"MZ")
        from test_framework import framework
        framework(self.prefix)

    def package(self):
        package = self.directory / "MSFS/fsdreamteam-gsx-pro"
        package.mkdir(parents=True)
        (package / "manifest.json").write_text('{"title":"GSX Pro","package_version":"4.0.23"}')
        (self.community / "fsdreamteam-gsx-pro").symlink_to(package)
        exe = self.directory / "couatl64/couatl64_boot.exe"
        exe.parent.mkdir()
        exe.write_bytes(b"MZ")
        (self.config / "exe.xml").write_text('''<?xml version="1.0"?><SimBase.Document Type="Launch"><Disabled>False</Disabled>
<!-- keep other add-ons -->
<Launch.Addon><Name>Fenix</Name><Path>C:\\Fenix.exe</Path><Disabled>False</Disabled></Launch.Addon>
<Launch.Addon><Name>Couatl</Name><Path>C:\\Program Files (x86)\\Addon Manager\\couatl64\\couatl64_boot.exe</Path><CommandLine>existing official arguments</CommandLine><Disabled>True</Disabled></Launch.Addon>
</SimBase.Document>''')

    def test_only_actual_files_complete_steps_never_claim_license_or_flight_validation(self):
        value = core.snapshot(self.root)
        self.assertEqual(value["state"], "available")
        self.assertFalse(value["prepared"])
        self.installed()
        value = core.snapshot(self.root)
        self.assertTrue(value["prepared"])
        self.assertFalse(value["package_installed"])
        self.assertFalse(value["verified_in_simulator"])
        self.package()
        core.configure(self.root, True)
        value = core.snapshot(self.root)
        self.assertTrue(value["configured"])
        self.assertFalse(value["verified_in_simulator"])
        (self.community / "fsdreamteam-gsx-pro").unlink()
        self.assertFalse(core.snapshot(self.root)["configured"])

    def test_autostart_preserves_fenix_official_arguments_and_comments(self):
        self.installed(); self.package()
        core.configure(self.root, True)
        tree = ET.parse(self.config / "exe.xml")
        self.assertEqual(tree.findtext("Launch.Addon/Name"), "Fenix")
        self.assertEqual(tree.findtext("Launch.Addon/Disabled"), "False")
        self.assertEqual(tree.findall("Launch.Addon")[1].findtext("CommandLine"), "existing official arguments")
        self.assertIn("keep other add-ons", (self.config / "exe.xml").read_text())
        self.assertEqual(len(list((self.root / "private").glob("gsx-exe-*.xml"))), 1)
        core.configure(self.root, False)
        tree = ET.parse(self.config / "exe.xml")
        self.assertEqual(tree.findall("Launch.Addon")[0].findtext("Disabled"), "False")
        self.assertEqual(tree.findall("Launch.Addon")[1].findtext("Disabled"), "True")

    def test_missing_ambiguous_and_external_startup_entries_are_not_guessed(self):
        self.installed(); self.package()
        path = self.config / "exe.xml"
        original = path.read_text()
        for content in ["<SimBase.Document />", original.replace("</SimBase.Document>", original[original.index('<Launch.Addon><Name>Couatl'):original.index('</SimBase.Document>')] + "</SimBase.Document>"),
                        original.replace("C:\\Program Files (x86)\\Addon Manager", "C:\\Other Manager")]:
            path.write_text(content)
            with self.assertRaises((core.core.PatchError, FileNotFoundError)):
                core.configure(self.root, True)
            self.assertEqual(path.read_text(), content)

    def test_existing_external_manager_directory_is_rejected(self):
        outside = Path(self.temp.name) / "external"
        outside.mkdir()
        (self.prefix / core.MANAGER_DIR).parent.mkdir(parents=True)
        (self.prefix / core.MANAGER_DIR).symlink_to(outside)
        (outside / "Couatl_Updater.exe").write_bytes(b"MZ")
        with self.assertRaises(core.core.PatchError):
            core.manager_directory(self.prefix)

    def test_profile_copy_remaps_internal_links_and_retains_external_community(self):
        self.installed(); self.package()
        outside = Path(self.temp.name) / "external-community"
        outside.mkdir()
        (self.prefix / "external-community").symlink_to(outside)
        copied = self.root / "local/.gsx-prefix-test"
        core.copy_profile(self.prefix, copied)
        original_package = self.directory / "MSFS/fsdreamteam-gsx-pro"
        linked = copied / (self.community / "fsdreamteam-gsx-pro").relative_to(self.prefix)
        self.assertEqual(linked.resolve(), copied / original_package.relative_to(self.prefix))
        self.assertFalse(linked.readlink().is_absolute())
        self.assertEqual((copied / "external-community").resolve(), outside)
        self.assertEqual((self.community / "fsdreamteam-gsx-pro").resolve(), original_package)

    def test_xml_declarations_in_utf16_are_rejected_without_changing_other_addons(self):
        self.installed(); self.package()
        path = self.config / "exe.xml"
        raw = path.read_text().replace('<?xml version="1.0"?>', '<?xml version="1.0" encoding="UTF-16"?><!DOCTYPE SimBase.Document [<!ENTITY name "Couatl">]>').encode('utf-16')
        path.write_bytes(raw)
        with self.assertRaises(core.core.PatchError):
            core.configure(self.root, True)
        self.assertEqual(path.read_bytes(), raw)

    def test_2020_is_unavailable(self):
        (self.root / "private/runtime.json").write_text('{"game_id":"msfs2020"}')
        self.assertEqual(core.snapshot(self.root)["state"], "unavailable")

    def test_failed_staging_retains_live_profile_and_can_recover(self):
        original = core.identity(self.prefix)
        installer = Path(self.temp.name) / "installer.exe"
        installer.write_bytes(b"MZ")
        class Wine:
            def __init__(self, *args): pass
            def stop(self): pass
            def reg(self, *args): pass
        with patch.object(core.core, "download", return_value=installer), patch.object(core.core, "Wine", Wine), \
             patch.object(core.core, "prepare_framework", side_effect=RuntimeError("simulated prerequisite failure")):
            with self.assertRaisesRegex(RuntimeError, "prerequisite failure"):
                core.prepare(self.root, Path(self.temp.name) / "cache")
        self.assertEqual(core.identity(self.prefix), original)
        self.assertFalse(core.setup_complete(self.root))
        self.assertTrue(core.snapshot(self.root)["can_recover"])
        with patch.object(core.core.Wine, "stop"):
            core.recover(self.root)
        self.assertTrue(core.setup_complete(self.root))
        self.assertEqual(core.identity(self.prefix), original)

    def test_commit_journal_recovers_every_rename_boundary_and_keeps_newer_profile(self):
        for boundary in (0, 1, 2):
            with self.subTest(boundary=boundary):
                token = str(boundary) * 32
                original = core.identity(self.prefix)
                staged = self.root / "local" / (".gsx-prefix-" + token)
                shutil.copytree(self.prefix, staged, symlinks=True)
                (staged / "newer-profile").touch()
                state = {"format": 1, "id": token, "state": "committing", "original_prefix_id": original,
                         "staged_prefix_id": core.identity(staged), "prior": None}
                core.core.write_json(self.root / core.MARKER, state)
                if boundary >= 1: os.rename(self.prefix, self.root / "local" / ("msfs-prefix.before-gsx-" + token))
                if boundary >= 2: os.rename(staged, self.prefix)
                with patch.object(core.core.Wine, "stop"):
                    core.recover(self.root)
                self.assertEqual(core.identity(self.prefix), original)
                self.assertFalse((self.prefix / "newer-profile").exists())
        self.assertEqual(len(list((self.root / "local").glob("msfs-prefix.after-gsx-*"))), 1)

    def test_successful_setup_verifies_hash_and_quotes_registration_argument(self):
        self.installed()
        (self.prefix / "user.reg").write_text(r'[Software\\Wine\\AppDefaults\\Couatl_Updater.exe\\DllOverrides]' + '\n"mscoree"="native,builtin"\n')
        calls = []
        overrides = []
        installer = Path(self.temp.name) / "installer.exe"
        installer.write_bytes(b"MZ")
        class Wine:
            def __init__(self, prefix, runner, log): self.prefix = prefix
            def run(self, *args, **kwargs): calls.append(args)
            def stop(self): pass
            def reg(self, *args): overrides.append(args)
        original = core.identity(self.prefix)
        with patch.object(core.core, "download", return_value=installer) as download, patch.object(core.core, "Wine", Wine), \
             patch.object(core.core, "prepare_framework"):
            core.prepare(self.root, Path(self.temp.name) / "cache")
        self.assertEqual(download.call_args.args[2], core.INSTALLER_SHA256)
        self.assertIn('/codebase', calls[-1])
        self.assertEqual(calls[-1][-1], r'C:\Program Files (x86)\Addon Manager\QlmLicenseLib.dll')
        key = r'HKCU\Software\Wine\AppDefaults\Couatl_Updater.exe\DllOverrides'
        self.assertIn((key, 'mscoree', ''), overrides)
        self.assertEqual(overrides[-1], (key, 'mscoree', 'native,builtin'))
        self.assertIn(('reg', 'delete', r'HKCU\Software\Wine\AppDefaults\Couatl_Updater2.exe\DllOverrides', '/v', 'mscoree', '/f'), calls)
        self.assertNotEqual(core.identity(self.prefix), original)
        self.assertTrue(core.setup_complete(self.root))
        self.assertEqual((self.prefix / 'dosdevices/c:').readlink(), Path('../drive_c'))

    def test_actions_reject_stale_runtime_before_reserving_or_starting(self):
        from types import SimpleNamespace
        from unittest.mock import Mock
        launcher = SimpleNamespace(runtime=self.root, lock=threading.RLock(), require_open=Mock(), reserve_setup=Mock())
        manager = gsx.GSXManager(launcher)
        with self.assertRaisesRegex(LauncherError, "Runtime"):
            manager.start("prepare", {"runtime_path": "/other"})
        launcher.reserve_setup.assert_not_called()
        self.assertIsNone(manager.worker)


if __name__ == '__main__':
    unittest.main()
