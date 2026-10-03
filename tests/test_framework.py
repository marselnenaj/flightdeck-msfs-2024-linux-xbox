# SPDX-License-Identifier: MIT
"""No account, downloads or Wine processes: exercise .NET evidence and repair policy."""
from pathlib import Path
from contextlib import ExitStack, nullcontext
import os
import tempfile
import unittest
from unittest.mock import patch

from flightdeck._fenix import core


def framework(prefix, *, x86=True, x64=True, release=528049, lowercase=False):
    sections = []
    for view, middle, architecture in ((x64, "", "Framework64"), (x86, r"Wow6432Node\\", "Framework")):
        if not view:
            continue
        key = r"Software\\" + middle + r"Microsoft\\NET Framework Setup\\NDP\\v4\\Full"
        sections.append('[%s] 123\n"Release"=dword:%08x\n' % (key, release))
        path = "drive_c/windows/Microsoft.NET/%s/v4.0.30319" % architecture
        folder = prefix / (path.lower() if lowercase else path)
        folder.mkdir(parents=True, exist_ok=True)
        for name in ("clr.dll", "csc.exe"):
            (folder / name).write_bytes(b"MZfixture")
    content = "\n".join(sections)
    (prefix / "system.reg").write_text(content.lower() if lowercase else content)


class FakeWine:
    def __init__(self, prefix, outcome=None):
        self.prefix = prefix
        self.runner = prefix / "runner"
        self.env = {}
        self.calls = []
        self.outcome = outcome
        self.attempts = 0

    def reg(self, *args):
        self.calls.append(("reg", *args))

    def run(self, *args, **kwargs):
        self.calls.append(tuple(map(str, args)))
        if Path(args[0]).name == "NDP48-x86-x64-AllOS-ENU.exe" and "/uninstall" not in args:
            self.attempts += 1
            if self.outcome:
                self.outcome(self, args)
        return 0

    def stop(self):
        self.calls.append(("stop",))


class FrameworkTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.prefix = Path(self.temp.name) / "prefix"
        self.prefix.mkdir()
        (self.prefix / "system.reg").write_text("WINE REGISTRY Version 2\n")

    def prepare(self, wine):
        with patch.object(core, "download", side_effect=lambda url, path, expected: path), \
             patch.object(core.subprocess, "run", return_value=type("Reply", (), {"stdout": b""})()):
            core.prepare_framework(wine, self.prefix / "cache", lambda text: None)

    def test_both_architectures_and_case_insensitive_windows_identity(self):
        framework(self.prefix, lowercase=True)
        self.assertTrue(core.has_framework(self.prefix))
        self.assertEqual(core.framework_status(self.prefix)["x64"]["release"], 528049)

    def test_partial_architecture_and_old_release_are_not_ready(self):
        framework(self.prefix, x86=False)
        self.assertFalse(core.has_framework(self.prefix))
        framework(self.prefix, release=461808)
        self.assertFalse(core.has_framework(self.prefix))
        framework(self.prefix)
        clr = self.prefix / "drive_c/windows/Microsoft.NET/Framework/v4.0.30319/clr.dll"
        clr.write_bytes(b"not a CLR")
        self.assertFalse(core.has_framework(self.prefix))

    def test_duplicate_sections_values_and_wrong_registry_type_are_rejected(self):
        framework(self.prefix)
        path = self.prefix / "system.reg"
        good = path.read_text()
        for bad in (good + good, good.replace('"Release"=dword:00080eb1', '"Release"=dword:00080eb1\n"Release"=dword:00080eb1'),
                    good.replace('"Release"=dword:00080eb1', '"Release"="00080eb1"'),
                    good.replace('v4\\\\Full]', 'v4\\\\Full\\\\1033]')):
            path.write_text(bad)
            self.assertFalse(core.has_framework(self.prefix))

    def test_runtime_files_cannot_escape_through_symlinks(self):
        framework(self.prefix)
        clr = self.prefix / "drive_c/windows/Microsoft.NET/Framework64/v4.0.30319/clr.dll"
        outside = self.prefix.parent / "external-clr.dll"
        outside.write_bytes(clr.read_bytes())
        clr.unlink()
        clr.symlink_to(outside)
        self.assertFalse(core.has_framework(self.prefix))

    def test_healthy_installation_is_probed_without_download_or_reinstall(self):
        framework(self.prefix)
        wine = FakeWine(self.prefix)
        with patch.object(core, "download") as download:
            core.prepare_framework(wine, self.prefix / "cache", lambda text: None)
        download.assert_not_called()
        self.assertEqual(sum(Path(call[0]).name == "csc.exe" for call in wine.calls), 2)
        self.assertIn(("reg", r"HKCU\Software\Wine\DllOverrides", "mscoree", "native"), wine.calls)

    def test_partial_48_repairs_without_reinstalling_40(self):
        framework(self.prefix, x86=False)
        def success(wine, args):
            self.assertIn("/repair", args)
            framework(wine.prefix)
        wine = FakeWine(self.prefix, success)
        self.prepare(wine)
        self.assertEqual(wine.attempts, 1)
        self.assertFalse(any(Path(call[0]).name == "dotNetFx40_Full_x86_x64.exe" for call in wine.calls))
        self.assertEqual(wine.calls[-2:], [("reg", r"HKCU\Software\Wine", "Version", "win10"), ("stop",)])

    def test_fresh_install_completes_virtual_restart_and_repairs_partial_result(self):
        def install(wine, args):
            if wine.attempts == 1:
                self.assertNotIn("/repair", args)
                framework(wine.prefix, x86=False)
            else:
                self.assertNotIn("/repair", args)
                framework(wine.prefix)
        wine = FakeWine(self.prefix, install)
        self.prepare(wine)
        self.assertEqual(wine.attempts, 2)
        self.assertTrue(core.has_framework(self.prefix))
        self.assertGreaterEqual(wine.calls.count(("wineboot", "-r")), 3)
        self.assertNotIn(("wineboot", "-u"), wine.calls)
        self.assertFalse(any("NET Framework Setup" in str(call) for call in wine.calls))
        self.assertTrue(any("/uninstall" in call for call in wine.calls))

    def test_installer_error_is_retried_once_after_restart(self):
        def install(wine, args):
            if wine.attempts == 1:
                raise core.PatchError("Windows setup failed (exit 67).")
            framework(wine.prefix)
        wine = FakeWine(self.prefix, install)
        self.prepare(wine)
        self.assertEqual(wine.attempts, 2)

    def test_failed_repair_stays_failed_and_always_restores_windows_version(self):
        framework(self.prefix, x86=False)
        wine = FakeWine(self.prefix)
        with self.assertRaisesRegex(core.PatchError, "could not be repaired automatically.*x86 release=0 CLR=no"):
            self.prepare(wine)
        self.assertEqual(wine.attempts, 2)
        self.assertEqual(wine.calls[-2:], [("reg", r"HKCU\Software\Wine", "Version", "win10"), ("stop",)])

    def test_real_clr_start_failure_is_not_hidden_by_registry_markers(self):
        framework(self.prefix)
        wine = FakeWine(self.prefix)
        original = wine.run
        def run(*args, **kwargs):
            if Path(args[0]).name == "csc.exe":
                raise core.PatchError("CLR startup failed")
            return original(*args, **kwargs)
        wine.run = run
        with self.assertRaisesRegex(core.PatchError, "could not be repaired automatically"):
            self.prepare(wine)
        self.assertEqual(wine.attempts, 2)

    def test_newer_framework_is_never_downgraded_by_repair(self):
        framework(self.prefix, x86=False, release=533320)
        wine = FakeWine(self.prefix)
        with self.assertRaisesRegex(core.PatchError, "newer .NET Framework"):
            self.prepare(wine)
        self.assertEqual(wine.attempts, 0)
        self.assertFalse(any("/uninstall" in call for call in wine.calls))

    def test_mono_advertised_release_without_native_clrs_does_not_skip_bootstrap(self):
        framework(self.prefix, release=533320)
        for architecture in ("Framework", "Framework64"):
            core._framework_path(self.prefix, architecture, "clr.dll").unlink()
        def success(wine, args):
            framework(wine.prefix)
        wine = FakeWine(self.prefix, success)
        self.prepare(wine)
        self.assertTrue(any(Path(call[0]).name == "dotNetFx40_Full_x86_x64.exe" for call in wine.calls))
        self.assertTrue(core.has_framework(self.prefix))


class PreparationRetryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        stamp = "20261003T150000-1234abcd"
        self.state = {"state": "preparing", "work": "local/fenix-patch-" + stamp,
                      "backup": "private/fenix-patch-backup-" + stamp,
                      "previous_prefix": "local/msfs-prefix.before-fenix-" + stamp,
                      "previous_runner": "stock"}
        for relative in ("tools", "local/msfs-prefix", "stock", self.state["work"] + "/prefix", self.state["backup"]):
            (self.root / relative).mkdir(parents=True)
        (self.root / "runner").symlink_to("stock")
        prefix = self.root / "local/msfs-prefix"
        self.state["original_prefix_id"] = [prefix.stat().st_dev, prefix.stat().st_ino]
        for name in ("launch-msfs.sh", "xodus-wine-launch"):
            (self.root / "tools" / name).write_text("original")
            (self.root / self.state["backup"] / name).write_text("original")
        core.write_json(self.root / core.MARKER, self.state)

    def test_retry_preserves_original_and_failed_profile_and_archives_journal(self):
        prefix = self.root / "local/msfs-prefix"
        (prefix / "keep").write_text("user data")
        staged = self.root / self.state["work"] / "prefix"
        with patch.object(core.Wine, "stop") as stop, patch.object(core, "ensure_idle"):
            core._retry_preparation(self.root, self.state, lambda text: None)
        stop.assert_called_once()
        self.assertEqual((prefix / "keep").read_text(), "user data")
        self.assertTrue(staged.is_dir())
        self.assertTrue((self.root / self.state["backup"] / "interrupted.json").is_file())
        self.assertFalse((self.root / core.MARKER).exists())

    def test_commits_upgrades_changed_sources_and_aliases_require_explicit_recovery(self):
        self.assertTrue(core.retryable_preparation(self.root, self.state))
        for fields in ({"state": "committing"}, {"upgrade_backup": "private/old"}, {"original_prefix_id": [0, 0]},
                       {"previous_runner": "different"}, {"work": "local/../escape"}):
            self.assertFalse(core.retryable_preparation(self.root, {**self.state, **fields}))
        staged = self.root / self.state["work"] / "prefix"
        staged.rmdir()
        staged.symlink_to(self.root / "local/msfs-prefix")
        self.assertFalse(core.retryable_preparation(self.root, self.state))
        staged.unlink()
        staged.mkdir()
        (self.root / "tools/launch-msfs.sh").write_text("changed")
        self.assertFalse(core.retryable_preparation(self.root, self.state))

    def test_install_retries_old_failure_then_publishes_only_a_complete_new_copy(self):
        prefix = self.root / "local/msfs-prefix"
        (prefix / "dosdevices").mkdir()
        (prefix / "keep").write_text("original user data")
        bundle = self.root / "bundle"
        (bundle / "integration").mkdir(parents=True)
        scripts = ("launch-msfs.sh", "xodus-wine-launch")
        for name in scripts:
            (bundle / "integration" / name).write_text("new integration")
        lock = {"version": "test-version", "accepted_scripts": {
            name: [core.digest(self.root / "tools" / name)] for name in scripts}}
        attempts = []

        def prepare(wine, cache, progress):
            self.assertNotEqual(wine.prefix, prefix)
            self.assertNotEqual(wine.prefix, self.root / self.state["work"] / "prefix")
            self.assertEqual((prefix / "keep").read_text(), "original user data")
            self.assertEqual(os.readlink(self.root / "runner"), "stock")
            attempts.append(wine.prefix)
            if len(attempts) == 1:
                raise core.PatchError("simulated .NET failure")
            framework(wine.prefix)

        with ExitStack() as stack:
            for name, options in {
                "host_check": {}, "verify_bundle": {"return_value": bundle},
                "manifest": {"return_value": lock}, "runner_variant": {"return_value": None},
                "locked": {"side_effect": lambda *args, **kwargs: nullcontext(self.root)},
                "ensure_idle": {}, "prepare_framework": {"side_effect": prepare},
                "graphics_and_fonts": {}, "configure_prefix": {"return_value": False},
                "prepare_geometry": {}, "apply_overlay": {},
            }.items():
                stack.enter_context(patch.object(core, name, **options))
            stack.enter_context(patch.object(core.Wine, "stop"))
            with self.assertRaisesRegex(core.PatchError, "simulated .NET failure"):
                core.install(self.root, bundle)
            failed = core.read_json(self.root / core.MARKER)
            self.assertEqual(failed["state"], "preparing")
            self.assertTrue(core.retryable_preparation(self.root, failed))
            self.assertEqual([prefix.stat().st_dev, prefix.stat().st_ino], self.state["original_prefix_id"])
            core.install(self.root, bundle)

        installed = core.read_json(self.root / core.MARKER)
        self.assertEqual(installed["state"], "installed")
        self.assertTrue(core.has_framework(prefix))
        self.assertEqual((prefix / "keep").read_text(), "original user data")
        original = self.root / installed["previous_prefix"]
        self.assertEqual([original.stat().st_dev, original.stat().st_ino], self.state["original_prefix_id"])
        for prior in (self.state, failed):
            self.assertTrue((self.root / prior["backup"] / "interrupted.json").is_file())
            self.assertTrue((self.root / prior["work"] / "prefix").is_dir())


if __name__ == "__main__":
    unittest.main()
