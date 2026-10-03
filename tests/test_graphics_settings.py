# SPDX-License-Identifier: MIT
"""Saved NVIDIA mode transitions in synthetic Wine profiles, with per-field undo."""
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import graphics, graphics_settings as gs

SOURCE = ('Version 66\r\n{Video\r\n\tAdapter "NVIDIA GeForce RTX 4080"\r\n'
          '\tAntiAliasing DLSS\r\n\tReflex ONBOOST\r\n\tFrameGeneration DLSSG\r\n'
          '\tAntiAliasingVR DLSS\r\n\tReflexVR ON\r\n\tFrameGenerationVR DLSSG\r\n'
          '\tResolution 3840 2160\r\n}\r\n{Graphics\r\n\t{Texture\r\n\t\tQuality 3\r\n\t}\r\n}\r\n'
          'InstalledPackagesPath "C:\\Flüge\\Packages"\r\n')


class GraphicsSettingsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "runtime"
        self.config = self.root / "local/msfs-prefix/drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024/UserCfg.opt"
        self.config.parent.mkdir(parents=True)
        (self.root / "private").mkdir()
        self.config.write_bytes(SOURCE.encode())
        self.marker = self.config.with_name(gs.MARKER)

    def test_startup_disables_only_unavailable_features_and_restores_exact_bytes(self):
        before = self.config.read_bytes()
        result = gs.prepare(self.root, True)
        self.assertEqual(result, {"changed_files": 1, "changed_options": sorted(gs.OPTIONS), "skipped_files": 0})
        edited = self.config.read_bytes()
        self.assertIn(b"\tAntiAliasing TAA\r\n", edited)
        self.assertIn(b"\tFrameGeneration NONE\r\n", edited)
        self.assertIn(b"\tReflex OFF\r\n", edited)
        self.assertIn('InstalledPackagesPath "C:\\Flüge\\Packages"'.encode(), edited)
        self.assertIn(b'Adapter "NVIDIA GeForce RTX 4080"', edited)
        self.assertNotIn("Flüge", self.marker.read_text())
        self.assertEqual(self.marker.stat().st_mode & 0o777, 0o600)
        timestamp = self.config.stat().st_mtime_ns
        self.assertEqual(gs.prepare(self.root, True)["changed_files"], 0)
        self.assertEqual(self.config.stat().st_mtime_ns, timestamp)
        self.assertEqual(gs.prepare(self.root, False)["changed_files"], 1)
        self.assertEqual(self.config.read_bytes(), before)
        self.assertFalse(self.marker.exists())

    def test_user_changes_and_other_upscalers_win_during_restore(self):
        gs.prepare(self.root, True)
        self.config.write_bytes(self.config.read_bytes().replace(b"AntiAliasing TAA", b"AntiAliasing FSR")
                                .replace(b"FrameGeneration NONE", b"FrameGeneration FSRFG")
                                .replace(b"Resolution 3840 2160", b"Resolution 1920 1080"))
        gs.prepare(self.root, False)
        result = self.config.read_bytes()
        self.assertIn(b"AntiAliasing FSR\r\n", result)
        self.assertIn(b"FrameGeneration FSRFG\r\n", result)
        self.assertIn(b"Resolution 1920 1080", result)
        self.assertIn(b"Reflex ONBOOST", result)

    def test_user_changes_to_a_different_unsupported_value_replace_the_undo_value(self):
        gs.prepare(self.root, True)
        self.config.write_bytes(self.config.read_bytes().replace(b"Reflex OFF", b"Reflex ON"))
        gs.prepare(self.root, True)
        gs.prepare(self.root, False)
        self.assertIn(b"Reflex ON\r\n", self.config.read_bytes())

    def test_safe_settings_and_features_without_a_record_are_untouched(self):
        timestamp = self.config.stat().st_mtime_ns
        self.assertEqual(gs.prepare(self.root, False)["changed_files"], 0)
        self.assertEqual(self.config.stat().st_mtime_ns, timestamp)
        safe = SOURCE.replace("AntiAliasing DLSS", "AntiAliasing FSR").replace("AntiAliasingVR DLSS", "AntiAliasingVR TAA")
        safe = safe.replace("Reflex ONBOOST", "Reflex OFF").replace("ReflexVR ON", "ReflexVR OFF")
        safe = safe.replace("FrameGeneration DLSSG", "FrameGeneration FSRFG").replace("FrameGenerationVR DLSSG", "FrameGenerationVR NONE")
        self.config.write_bytes(safe.encode())
        self.assertEqual(gs.prepare(self.root, True)["changed_files"], 0)
        self.assertEqual(self.config.read_bytes(), safe.encode())
        self.assertFalse(self.marker.exists())

    def test_encoding_bom_line_endings_and_last_line_are_preserved(self):
        for bom, codec in ((b"", "utf-8"), (b"\xef\xbb\xbf", "utf-8"), (b"\xff\xfe", "utf-16-le"), (b"\xfe\xff", "utf-16-be")):
            for text in (SOURCE, SOURCE.replace("\r\n", "\n").rstrip("\n")):
                with self.subTest(codec=codec, bom=bom):
                    data = bom + text.encode(codec)
                    changed, original, _ = gs.transform(data, {}, True)
                    self.assertEqual(gs.transform(changed, original, False)[0], data)

    def test_ambiguous_configs_and_invalid_backups_are_preserved(self):
        for text in (SOURCE + "{Video\n}\n", SOURCE.replace("\tReflex ONBOOST", "\tReflex OFF\n\tReflex ONBOOST"), SOURCE.replace("{Video", "{Video\n{Nested"), "Version 66\n{Video\n", "Version 66\n{Graphics\n}\n"):
            self.config.write_bytes(text.encode())
            self.assertEqual(gs.prepare(self.root, True)["skipped_files"], 1)
            self.assertEqual(self.config.read_bytes(), text.encode())
            self.assertFalse(self.marker.exists())
        self.config.write_bytes(SOURCE.encode())
        for record in ({"schema": 9, "original": {}}, {"schema": True, "original": {}},
                       {"schema": 1, "original": {"Reflex": "private-token"}},
                       {"schema": 1, "original": {"Reflex": []}}, {"schema": 1, "original": {}, "extra": "secret"}):
            self.marker.write_text(json.dumps(record))
            self.assertEqual(gs.prepare(self.root, True)["skipped_files"], 1)
            self.assertEqual(self.config.read_bytes(), SOURCE.encode())

    def test_only_the_video_block_is_changed(self):
        data = b"{Graphics\n Reflex ON\n}\n{Video\n AntiAliasing FSR\n FrameGeneration DLSSG\n}\n"
        edited, saved, changed = gs.transform(data, {}, True)
        self.assertEqual(edited, data.replace(b"FrameGeneration DLSSG", b"FrameGeneration NONE"))
        self.assertEqual(saved, {"FrameGeneration": "DLSSG"})
        self.assertEqual(changed, ["FrameGeneration"])

    def test_symlinks_hardlinks_and_special_files_are_not_modified(self):
        original = self.config.with_name("original")
        self.config.rename(original)
        self.config.symlink_to(original)
        self.assertEqual(gs.prepare(self.root, True)["skipped_files"], 1)
        self.config.unlink()
        os.link(original, self.config)
        self.assertEqual(gs.prepare(self.root, True)["skipped_files"], 1)
        self.config.unlink()
        os.mkfifo(self.config)
        self.assertEqual(gs.prepare(self.root, True)["skipped_files"], 1)
        self.assertEqual(original.read_bytes(), SOURCE.encode())

    def test_linked_parent_cannot_edit_an_external_profile(self):
        folder = self.config.parent
        real = folder.with_name("external")
        folder.rename(real)
        folder.symlink_to(real, target_is_directory=True)
        self.assertEqual(gs.prepare(self.root, True)["skipped_files"], 1)
        self.assertEqual((real / "UserCfg.opt").read_bytes(), SOURCE.encode())

    def test_case_insensitive_and_package_family_locations(self):
        renamed = self.config.with_name("usercfg.OPT")
        self.config.rename(renamed)
        self.assertEqual(gs.prepare(self.root, True)["changed_files"], 1)
        folder = self.root / "local/msfs-prefix/drive_c/users/steamuser/AppData/Local/Packages/Microsoft.Limitless_test/LocalCache"
        folder.mkdir(parents=True)
        (folder / "UserCfg.opt").write_bytes(SOURCE.encode())
        with patch("flightdeck.mods._family", return_value="Microsoft.Limitless_test"):
            self.assertEqual(gs.prepare(self.root, True)["changed_files"], 1)
        self.assertTrue((folder / gs.MARKER).exists())

    def test_backup_survives_proton_profile_copy(self):
        gs.prepare(self.root, True)
        copied = Path(self.temp.name) / "proton"
        shutil.copytree(self.root, copied)
        gs.prepare(copied, False)
        self.assertEqual((copied / self.config.relative_to(self.root)).read_bytes(), SOURCE.encode())
        self.assertIn(b"FrameGeneration NONE", self.config.read_bytes())

    def test_unreadable_package_identity_does_not_skip_the_roaming_configuration(self):
        with patch("flightdeck.mods._family", side_effect=gs.ParseError("bad optional metadata")):
            self.assertEqual(gs.prepare(self.root, True)["changed_files"], 1)
            self.assertEqual(gs.prepare(self.root, False)["changed_files"], 1)
        self.assertEqual(self.config.read_bytes(), SOURCE.encode())

    def test_interrupted_config_write_reuses_original_backup(self):
        real = gs._atomic
        def write(path, data):
            if path == self.config:
                raise OSError("synthetic write failure")
            real(path, data)
        with patch.object(gs, "_atomic", side_effect=write), self.assertRaises(OSError):
            gs.prepare(self.root, True)
        self.assertTrue(self.marker.exists())
        self.assertEqual(self.config.read_bytes(), SOURCE.encode())
        gs.prepare(self.root, True)
        gs.prepare(self.root, False)
        self.assertEqual(self.config.read_bytes(), SOURCE.encode())

    def test_other_edition_and_missing_config_are_untouched(self):
        (self.root / "private/runtime.json").write_text('{"game_id":"msfs2020"}')
        self.assertEqual(gs.prepare(self.root, True)["changed_files"], 0)
        self.assertEqual(self.config.read_bytes(), SOURCE.encode())
        (self.root / "private/runtime.json").unlink()
        self.config.unlink()
        self.assertEqual(gs.prepare(self.root, True), {"changed_files": 0, "changed_options": [], "skipped_files": 0})

    def test_effective_nvidia_profile_invokes_repair_and_features_restore(self):
        from tests.test_graphics import GraphicsTests
        fixture = GraphicsTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        target = fixture.runtime / self.config.relative_to(self.root)
        target.parent.mkdir(parents=True)
        target.write_bytes(SOURCE.encode())
        fixture.set_mode("auto")
        _, report = graphics.prepare(fixture.runtime, {})
        self.assertEqual(report["game_settings"]["changed_files"], 1)
        fixture.set_mode("features")
        graphics.prepare(fixture.runtime, {"PROTON_DISABLE_NVAPI": "1"})
        self.assertIn(b"FrameGeneration NONE", target.read_bytes())
        graphics.prepare(fixture.runtime, {})
        self.assertEqual(target.read_bytes(), SOURCE.encode())
