# SPDX-License-Identifier: MIT
"""Component refresh on a synthetic runtime; no game, Wine or account access."""
import fcntl
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from contextlib import redirect_stdout

from flightdeck.backend import Launcher, LauncherError
from flightdeck import runtime_components as components
from flightdeck.server import Server


def sha(data):
    return hashlib.sha256(data).hexdigest()


class RuntimeComponentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "runtime"
        self.native = self.base / "native"
        self.source = self.base / "source"
        self.lockfile = self.source / "compat/bootstrap.lock.json"
        self.lockfile.parent.mkdir(parents=True)
        self.old = {}
        self.new = {}
        for name, paths in components._targets(self.root).items():
            old = ("old " + name).encode()
            new = ("new " + name).encode()
            self.old[name] = sha(old)
            self.new[name] = sha(new)
            for path in paths:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(old)
                if name.startswith("bin/"):
                    path.chmod(0o700)
            artifact = self.native / name
            artifact.parent.mkdir(parents=True, exist_ok=True)
            artifact.write_bytes(new)
            if name.startswith("bin/"):
                artifact.chmod(0o700)
        self.lockfile.write_text(json.dumps({"native": {"files": self.new, "upgrade_from": [self.old],
            "archive_sha256": "a" * 64, "features": ["connected-storage-read-v1"]}}))
        self.private = self.root / "private"
        self.private.mkdir(mode=0o700)
        self.manifest = self.private / components.MANIFEST
        self.manifest.write_text(json.dumps({"format": 1,
            "artifacts": {"format": 1, "files": self.old}, "user_setting": "preserved"}))
        (self.private / "save.bin").write_bytes(b"synthetic save data")
        script = self.root / "tools/play-msfs.sh"
        script.parent.mkdir(parents=True)
        script.write_text("#!/bin/sh\nexit 0\n")
        script.chmod(0o700)
        for relative in ("games/MSFS2024/FlightSimulator2024.exe", "local/msfs-prefix/system.reg"):
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic fixture")
        self.launcher = Launcher(self.base / "state", str(self.root))
        self.patches = [patch.object(components.bootstrap, "paths", return_value=(self.source, self.lockfile)),
                        patch.object(components.bootstrap, "native_path", return_value=self.native)]
        for replacement in self.patches:
            replacement.start()
            self.addCleanup(replacement.stop)

    def assert_files(self, hashes):
        for name, paths in components._targets(self.root).items():
            for path in paths:
                self.assertEqual(sha(path.read_bytes()), hashes[name])
        self.assertEqual((self.private / "save.bin").read_bytes(), b"synthetic save data")

    def scripts(self, *, recorded=False):
        self.script_source = self.source / "scripts/runtime"
        self.script_source.mkdir(parents=True)
        self.old_scripts, self.new_scripts = {}, {}
        for name in components.RUNTIME_FILES:
            old, new = ("#!/bin/sh\n# old " + name).encode(), ("#!/bin/sh\n# new " + name).encode()
            path, source = self.root / "tools" / name, self.script_source / name
            path.write_bytes(old); path.chmod(0o700)
            source.write_bytes(new); source.chmod(0o644)
            self.old_scripts[name], self.new_scripts[name] = sha(old), sha(new)
        lock = json.loads(self.lockfile.read_text())
        lock["runtime_scripts"] = {"files": self.new_scripts, "upgrade_from": [self.old_scripts]}
        self.lockfile.write_text(json.dumps(lock))
        if recorded:
            record = json.loads(self.manifest.read_text()); record["runtime_files"] = self.old_scripts
            self.manifest.write_text(json.dumps(record))
        replacement = patch.object(components, "resource_paths", return_value=(self.script_source, None))
        replacement.start(); self.addCleanup(replacement.stop)

    def assert_scripts(self, hashes):
        for name, expected in hashes.items():
            path = self.root / "tools" / name
            self.assertEqual(sha(path.read_bytes()), expected)
            self.assertTrue(path.stat().st_mode & 0o100)

    def test_migrate_pinned_legacy_scripts_with_native_files(self):
        self.scripts()
        self.assertEqual(components.update_state(self.root), "pending")
        self.assertTrue(components.refresh(self.launcher))
        self.assert_files(self.new); self.assert_scripts(self.new_scripts)
        self.assertEqual(json.loads(self.manifest.read_text())["runtime_files"], self.new_scripts)
        self.assertEqual(components.update_state(self.root), "current")
        self.assertFalse(components.refresh(self.launcher))

    def test_script_only_update_is_detected_after_native_only_upgrade(self):
        components.refresh(self.launcher)
        self.scripts()
        self.assertEqual(components.update_state(self.root), "pending")
        self.assertTrue(components.refresh_on_startup(self.launcher))
        self.assert_scripts(self.new_scripts); self.assert_files(self.new)

    def test_user_script_edits_are_preserved_before_any_native_write(self):
        for recorded in (False, True):
            with self.subTest(recorded=recorded):
                if not hasattr(self, "old_scripts"):
                    self.scripts()
                record = json.loads(self.manifest.read_text())
                if recorded: record["runtime_files"] = self.old_scripts
                self.manifest.write_text(json.dumps(record))
                target = self.root / "tools/xodus-wine-launch"
                target.write_bytes(b"#!/bin/sh\n# user settings")
                self.assertEqual(components.update_state(self.root), "custom")
                with self.assertRaises(components.CustomScripts):
                    components.refresh(self.launcher)
                self.assert_files(self.old)
                self.assertEqual(target.read_bytes(), b"#!/bin/sh\n# user settings")
                self.assertFalse((self.private / components.JOURNAL).exists())

    def test_unverified_new_script_is_rejected_before_native_write(self):
        self.scripts()
        (self.script_source / "launch-msfs.sh").write_bytes(b"unexpected change")
        with self.assertRaises(components.ComponentUpdateError):
            components.refresh(self.launcher)
        self.assert_files(self.old); self.assert_scripts(self.old_scripts)

    def test_script_failure_rolls_back_binaries_scripts_and_manifest(self):
        self.scripts(recorded=True)
        original = components._copy_verified
        before = self.manifest.read_bytes()
        failed = False
        def fail_once(source, destination, expected):
            nonlocal failed
            if not failed and source == self.script_source / "runtime-env.sh":
                failed = True
                raise OSError("synthetic write failure")
            return original(source, destination, expected)
        with patch.object(components, "_copy_verified", side_effect=fail_once):
            with self.assertRaises(OSError):
                components.refresh(self.launcher)
        self.assert_files(self.old); self.assert_scripts(self.old_scripts)
        self.assertEqual(self.manifest.read_bytes(), before)
        self.assertFalse((self.private / components.JOURNAL).exists())

    def test_script_only_interruption_recovers_even_when_native_hashes_match(self):
        components.refresh(self.launcher)
        self.scripts()
        original = components._copy_verified
        def interrupt(source, destination, expected):
            if source == self.script_source / "runtime-env.sh":
                raise KeyboardInterrupt()
            return original(source, destination, expected)
        with patch.object(components, "_copy_verified", side_effect=interrupt):
            with self.assertRaises(KeyboardInterrupt):
                components.refresh(self.launcher)
        self.assertEqual(components.update_state(self.root), "interrupted")
        components._recover(self.root)
        self.assert_files(self.new); self.assert_scripts(self.old_scripts)
        self.assertNotIn("runtime_files", json.loads(self.manifest.read_text()))
        self.assertTrue(components.refresh(self.launcher))
        self.assert_scripts(self.new_scripts)

    def test_changed_script_backup_prevents_partial_rollback(self):
        self.scripts()
        original = components._copy_verified
        def interrupt(source, destination, expected):
            if source == self.script_source / "runtime-env.sh":
                raise KeyboardInterrupt()
            return original(source, destination, expected)
        with patch.object(components, "_copy_verified", side_effect=interrupt):
            with self.assertRaises(KeyboardInterrupt):
                components.refresh(self.launcher)
        journal = json.loads((self.private / components.JOURNAL).read_text())
        (self.private / journal["backup"] / "script-launch-msfs.sh").write_bytes(b"changed")
        with self.assertRaises(components.ComponentUpdateError):
            components._recover(self.root)
        self.assert_files(self.new)
        self.assertTrue((self.private / components.JOURNAL).exists())

    def test_refresh_updates_all_copies_and_is_idempotent(self):
        self.assertFalse(self.launcher.status()["game"]["can_start"])
        self.assertEqual(components.update_state(self.root), "pending")
        self.assertTrue(components.refresh(self.launcher))
        self.assert_files(self.new)
        self.assertEqual(json.loads(self.manifest.read_text())["user_setting"], "preserved")
        self.assertFalse(components.refresh(self.launcher))
        self.assertFalse((self.private / components.JOURNAL).exists())
        self.assertTrue(self.launcher.status()["game"]["can_start"])

    def test_server_startup_applies_bundled_update(self):
        with Server(self.launcher) as server:
            self.assertIsNone(server.launcher.component_update_error)
            self.assertTrue(server.launcher.status()["game"]["can_start"])
        self.assert_files(self.new)

    def other_runtime(self):
        other = self.base / "other-runtime"
        (other / "tools").mkdir(parents=True)
        (other / "private").mkdir(mode=0o700)
        (other / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 0\n")
        return other

    def test_switch_applies_pending_components_without_a_launcher_restart(self):
        other = self.other_runtime()
        self.launcher.configure(str(other))
        self.assert_files(self.old)
        self.launcher.configure(str(self.root))
        self.assert_files(self.new)
        self.assertEqual(self.launcher.runtime, self.root)
        self.assertEqual(json.loads(self.launcher.config_file.read_text())["runtime_path"], str(self.root))
        self.assertTrue(self.launcher.status()["game"]["can_start"])

    def test_failed_switch_update_keeps_previous_selection_and_component_set(self):
        other = self.other_runtime()
        self.launcher.configure(str(other))
        before = self.launcher.config_file.read_bytes()
        (self.native / "bin/xodus-service").write_bytes(b"changed unverified bundle")
        with self.assertRaises(LauncherError):
            self.launcher.configure(str(self.root))
        self.assertEqual(self.launcher.runtime, other)
        self.assertEqual(self.launcher.config_file.read_bytes(), before)
        self.assert_files(self.old)

    def test_server_startup_defers_busy_runtime_and_recovers_on_restart(self):
        with (self.private / "play.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with Server(self.launcher) as server:
                self.assertFalse(server.launcher.status()["game"]["can_start"])
                self.assertIn("läuft bereits", server.launcher.component_update_error)
            self.assert_files(self.old)
        with Server(self.launcher) as server:
            self.assertTrue(server.launcher.status()["game"]["can_start"])
        self.assert_files(self.new)

    def test_malformed_manifest_does_not_crash_status_or_replace_files(self):
        self.manifest.write_text('[]')
        self.assertEqual(components.update_state(self.root), "invalid")
        self.assertFalse(self.launcher.status()["game"]["can_start"])
        with self.assertRaises(components.ComponentUpdateError):
            components.refresh_on_startup(self.launcher)
        self.assert_files(self.old)

    def test_changed_old_file_and_external_lock_fail_closed(self):
        target = self.root / "bin/xodus-cli"
        target.write_bytes(b"user modified")
        with self.assertRaisesRegex(components.ComponentUpdateError, "verändert"):
            components.refresh(self.launcher)
        self.assertEqual(target.read_bytes(), b"user modified")
        target.write_bytes(b"old bin/xodus-cli")
        with (self.private / "play.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(LauncherError):
                components.refresh(self.launcher)
        self.assert_files(self.old)

    def test_unmanaged_runtime_requires_new_import(self):
        self.manifest.unlink()
        self.assertEqual(components.update_state(self.root), "unmanaged")
        detail = next(row["detail"] for row in self.launcher.status()["runtime"]["checks"] if row["id"] == "component_update")
        self.assertIn("Ältere Runtime", detail)
        with self.assertRaisesRegex(components.ComponentUpdateError, "keine verwaltete Komponentenliste"):
            components.refresh(self.launcher)
        self.assert_files(self.old)

    def test_custom_manifest_is_not_replaced_by_manual_refresh(self):
        other = dict(self.old)
        other["bin/xodus-cli"] = sha(b"custom xodus-cli")
        (self.root / "bin/xodus-cli").write_bytes(b"custom xodus-cli")
        self.manifest.write_text(json.dumps({"format": 1, "artifacts": {"format": 1, "files": other}}))
        self.assertEqual(components.update_state(self.root), "custom")
        with self.assertRaisesRegex(components.ComponentUpdateError, "eigene oder unbekannte Komponenten"):
            components.refresh(self.launcher)
        self.assertEqual((self.root / "bin/xodus-cli").read_bytes(), b"custom xodus-cli")
        self.assertFalse((self.private / components.JOURNAL).exists())

    def test_interruption_recovers_old_set_before_retry(self):
        original = components._copy_verified
        interrupted = False
        def fail_once(source, destination, expected):
            nonlocal interrupted
            if not interrupted and destination == self.root / "bin/xodus-service":
                interrupted = True
                raise KeyboardInterrupt()
            return original(source, destination, expected)
        with patch.object(components, "_copy_verified", side_effect=fail_once):
            with self.assertRaises(KeyboardInterrupt):
                components.refresh(self.launcher)
        self.assertTrue((self.private / components.JOURNAL).is_file())
        self.assertFalse(self.launcher.status()["game"]["can_start"])
        self.assertTrue(components.refresh(self.launcher))
        self.assert_files(self.new)
        self.assertFalse((self.private / components.JOURNAL).exists())

    def test_cli_refresh_uses_configured_runtime_without_browser(self):
        from flightdeck.__main__ import main
        output = io.StringIO()
        with redirect_stdout(output):
            main(["--refresh-components", "--state-dir", str(self.base / "state")])
        self.assertIn("updated", output.getvalue())
        self.assert_files(self.new)
