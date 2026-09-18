"""Synthetic guided setup tests: no Wine, accounts, game or native UI."""
# SPDX-License-Identifier: MIT
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from flightdeck import setup
from flightdeck.backend import Launcher, LauncherError


class SetupTests(unittest.TestCase):
    def test_transfer_counters_cannot_reactivate_or_replace_job_lifecycle(self):
        value = {"kind": "game", "received_bytes": 40, "verified_bytes": 20,
                 "total_bytes": 100, "completed_files": 1, "total_files": 3}
        self.manager.job = {"id": "own", "state": "installing", "phase": "download", "checks": []}
        callback = self.manager._transfer_callback()
        callback(value)
        self.assertEqual(self.manager.job['transfer'], value)
        self.manager.job['phase'] = 'pausing'
        callback({**value, 'received_bytes': 50})
        self.assertEqual(self.manager.job['phase'], 'pausing')
        self.assertEqual(self.manager.job['transfer']['received_bytes'], 50)
        self.manager.job['phase'] = 'paused'
        callback({**value, 'received_bytes': 60})
        self.assertEqual(self.manager.job['transfer']['received_bytes'], 50)
        self.manager._notify('authentication', 'Sign in')
        self.assertIsNone(self.manager.job['transfer'])
        callback(value)
        self.assertIsNone(self.manager.job['transfer'])
        self.manager.job.update(id='new', phase='download')
        callback(value)
        self.assertIsNone(self.manager.job['transfer'])
        fresh = self.manager._transfer_callback()
        self.manager.cancel_event.set()
        fresh(value)
        self.assertIsNone(self.manager.job['transfer'])

    def test_transfer_rejects_non_numeric_payload_and_clears_on_phase_end(self):
        self.manager.job = {"id": "own", "state": "installing", "phase": "bootstrap", "checks": []}
        callback = self.manager._transfer_callback()
        value = {"kind": "components", "received_bytes": 40, "verified_bytes": 0,
                 "total_bytes": None, "completed_files": None, "total_files": None}
        callback(value)
        self.assertEqual(self.manager.job['transfer'], value)
        callback({**value, 'token': 'never exposed'})
        self.assertIsNone(self.manager.job['transfer'])
        callback(value)
        self.manager._notify('provision', 'Preparing')
        self.assertIsNone(self.manager.job['transfer'])
        callback(value)
        self.assertIsNone(self.manager.job['transfer'])

    def test_pause_preserves_reservation_and_late_cancel_callback_stays_disabled(self):
        from flightdeck.game_install import DownloadControl
        from flightdeck.i18n import localize
        self.launcher.reserve_setup()
        self.manager.job = {"id": "synthetic", "mode": "install", "state": "installing", "phase": "download", "checks": []}
        self.manager.download_control = DownloadControl(self.manager._download_changed)
        self.manager.download_control.downloading(True)
        with self.assertRaises(LauncherError):
            self.manager.download_action("foreign", "pause")
        paused = self.manager.download_action("synthetic", "pause")["job"]
        self.assertEqual(paused["phase"], "pausing")
        self.manager._download_changed("paused")
        self.assertTrue(self.launcher.setup_busy)
        self.assertFalse(self.launcher.status()["game"]["can_start"])
        self.assertTrue(self.manager.snapshot()["job"]["can_resume"])
        self.assertIn("Download paused", localize(self.manager.snapshot(), "en")["job"]["message"])
        self.manager.cancel("synthetic")
        after_cancel = self.manager.snapshot()["job"]
        self.manager._download_changed("paused")
        self.manager._download_changed("download")
        self.assertEqual(self.manager.snapshot()["job"], after_cancel)
        self.assertFalse(after_cancel["can_pause"])
        self.assertFalse(after_cancel["can_resume"])
        with self.assertRaises(LauncherError):
            self.manager.download_action("synthetic", "resume")
        self.manager._failed(setup.SetupCancelled("Einrichtung abgebrochen."))
        self.assertFalse(self.launcher.setup_busy)
        self.assertEqual(self.manager.snapshot()["job"]["failure_phase"], "paused")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.repo = self.root / "source"
        tools = self.repo / "scripts/runtime"
        tools.mkdir(parents=True)
        for name in setup.RUNTIME_FILES:
            (tools / name).write_text("#!/bin/sh\nexit 0\n")
        original = b"synthetic runner original"
        lock = self.repo / "compat/upstreams.lock.json"
        lock.parent.mkdir()
        lock.write_text(json.dumps({"runner": {"original_runtime_sha256": hashlib.sha256(original).hexdigest()}}))
        self.data = {name + "_path": str(self.root / name) for name in ("artifacts", "game", "runner", "prefix", "destination")}
        self.data.update(mode="prepare", market="AT", local_saves=False)
        self.destination = self.root / "destination"
        for name in ("system.reg", "user.reg"):
            self.file("prefix/" + name, b"synthetic registry")
        self.system32 = self.root / "prefix/drive_c/windows/system32"
        self.system32.mkdir(parents=True)
        for name in ("FlightSimulator2024.exe", ".xodus-streaming.msixvc", "MicrosoftGame.Config"):
            self.file("game/" + name, b"synthetic game")
        self.file("runner/files/bin/wine", b"not run").chmod(0o700)
        self.original = self.file("runner/files/lib/wine/x86_64-windows/xgameruntime.dll", original)
        (self.system32 / "xgameruntime.dll").symlink_to(self.original)
        files = {}
        for relative in setup.ARTIFACTS:
            path = self.file("artifacts/" + relative, relative.encode())
            if relative.startswith("bin/"):
                path.chmod(0o700)
            files[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
        self.file("artifacts/manifest.json", json.dumps({"files": files}).encode())
        self.launcher = Launcher(self.root / "state")
        self.launcher.setup.source_root = self.repo
        self.manager = self.launcher.setup

    def file(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def tearDown(self):
        self.manager.close()
        self.temp.cleanup()

    def wait(self, state):
        self.manager.thread.join(timeout=5)
        self.assertFalse(self.manager.thread.is_alive())
        value = self.manager.snapshot()
        self.assertEqual(value["state"], state, value["job"])
        return value["job"]

    def ready(self, data=None):
        self.manager.check(data or self.data)
        return self.wait("ready")

    def install(self, local_saves=False):
        self.data["local_saves"] = local_saves
        job = self.ready()
        self.manager.start(job["id"])
        return self.wait("complete")

    def assert_no_staging(self):
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.root.glob(".flightdeck-setup-*")), [])

    def test_idle_default_and_capabilities(self):
        with patch.dict(setup.os.environ, {"LC_ALL": "de_AT.UTF-8", "XDG_DATA_HOME": str(self.root / "data")}, clear=True), \
                patch.object(setup, "_local_timezone", return_value=""), \
                patch.object(self.manager, "_picker", return_value=None):
            value = self.manager.snapshot()
        self.assertIsNone(value["job"])
        self.assertEqual(value["state"], "idle")
        self.assertEqual(value["defaults"]["mode"], "install")
        self.assertEqual(value["defaults"]["market"], "AT")
        self.assertTrue(value["defaults"]["local_saves"])
        self.assertTrue(value["prepare_available"])
        self.assertFalse(value["directory_picker"])

    def test_missing_runtime_helper_disables_preparation(self):
        (self.repo / "scripts/runtime/runtime-env.sh").unlink()
        self.assertFalse(self.manager.snapshot()["prepare_available"])
        self.manager.check(self.data)
        self.assertIn("runtime-env.sh", self.wait("failed")["error"])
        self.assert_no_staging()

    def test_region_suggestion_does_not_change_selected_job_or_existing_runtime(self):
        self.data["market"] = "US"
        with patch.object(setup, "suggested_market", return_value="AT"):
            job = self.ready()
            self.assertEqual(self.manager.snapshot()["defaults"]["market"], "AT")
            self.assertEqual(self.manager.plan.inputs["market"], "US")
            self.manager.start(job["id"])
            self.wait("complete")
            config = self.destination / "private/runtime.json"
            before = config.read_bytes()
            self.assertEqual(json.loads(before)["market"], "US")
            self.assertEqual(self.manager.snapshot()["defaults"]["market"], "AT")
            self.assertEqual(config.read_bytes(), before)

    def test_checking_job_exposes_only_valid_region_without_using_suggestion(self):
        with patch.object(self.manager, "_check"), patch.object(setup, "suggested_market", return_value="AT"):
            for mode in ("install", "prepare"):
                for region in ("DE", "US", "", None, "de", "US\n", {"region": "DE"}):
                    with self.subTest(mode=mode, region=region):
                        result = self.manager.check({"mode": mode, "market": region})
                        job = self.wait("checking")
                        expected = region if region in ("DE", "US") else ""
                        self.assertEqual(result["job"]["market"], expected)
                        self.assertEqual(job["market"], expected)
                        self.assertEqual(self.manager.snapshot()["defaults"]["market"], "AT")
                        self.manager._failed(setup.SetupCancelled("Einrichtung abgebrochen."))
                        self.assertEqual(self.manager.snapshot()["job"]["market"], expected)

    def test_ready_and_installing_job_region_comes_from_validated_plan(self):
        with patch.object(setup, "suggested_market", return_value="AT"):
            job = self.ready({**self.data, "market": "DE"})
            self.assertEqual(job["market"], "DE")
            self.assertEqual(self.manager.plan.inputs["market"], "DE")
            with patch.object(self.manager, "_install"):
                started = self.manager.start(job["id"])
                self.assertEqual(started["job"]["market"], "DE")
                self.assertEqual(self.wait("installing")["market"], "DE")
            self.assertEqual(self.manager.snapshot()["defaults"]["market"], "AT")
            self.assertEqual(self.manager.plan.inputs["market"], "DE")
            self.manager._failed(setup.SetupCancelled("Einrichtung abgebrochen."))

    def test_install_job_publishes_validated_bootstrap_region(self):
        plan = SimpleNamespace(inputs={"mode": "install", "market": "DE"}, destination=self.destination)
        provider = SimpleNamespace(availability=lambda **_: {"available": True}, preflight=lambda *_args, **_kwargs: plan)
        with patch.object(setup, "bootstrap_module", return_value=provider), \
                patch.object(setup, "suggested_market", return_value="AT"):
            # A preflight owns normalization/validation; the completed plan is
            # authoritative even when an older caller omitted the input field.
            self.manager.check({"mode": "install"})
            self.assertEqual(self.wait("ready")["market"], "DE")
            self.assertEqual(self.manager.snapshot()["defaults"]["market"], "AT")
            self.manager.cancel(self.manager.job["id"])

    def test_install_bootstrap_login_download_and_provision_complete(self):
        cli = self.root / "artifacts/bin/xodus-cli"
        cli.write_text('#!' + sys.executable + '\n' + '''
import pathlib, sys
if sys.argv[1] == 'streaming':
    target = pathlib.Path(sys.argv[3])
    (target / 'FlightSimulator2024.exe').write_bytes(b'encrypted synthetic exe')
    (target / '.xodus-streaming.msixvc').write_bytes(b'synthetic complete marker')
    (target / 'MicrosoftGame.Config').write_text('<Game><ExecutableList><Executable Name="FlightSimulator2024.exe"/></ExecutableList></Game>')
''')
        checksum = hashlib.sha256(cli.read_bytes()).hexdigest()
        manifest_file = self.root / "artifacts/manifest.json"
        manifest = json.loads(manifest_file.read_text())
        manifest["files"]["bin/xodus-cli"] = checksum
        manifest_file.write_text(json.dumps(manifest))
        workspace = self.root / "install-workspace"
        workspace.mkdir(mode=0o700)
        components = {"cli": cli, "cli_sha256": checksum, "workspace": workspace,
                      **{name + "_path": self.root / name for name in ("artifacts", "runner", "prefix")}}
        def preflight(data, **kwargs):
            return SimpleNamespace(inputs=data, destination=Path(data["destination_path"]))
        provider = SimpleNamespace(availability=lambda **_: {"available": True, "reason": ""}, preflight=preflight,
                                   bootstrap=lambda *_, **__: components)
        with patch.object(setup, "bootstrap_module", return_value=provider):
            self.assertTrue(self.manager.snapshot()["install_available"])
            self.manager.check({"mode": "install", "market": "AT", "local_saves": True, "destination_path": str(self.destination)})
            job = self.wait("ready")
            self.assertEqual(job["mode"], "install")
            self.assertFalse((workspace / "game").exists())
            self.manager.start(job["id"])
            final = self.wait("complete")
        self.assertEqual(final["mode"], "install")
        self.assertEqual(final["workspace_path"], str(workspace))
        self.assertEqual(self.launcher.runtime, self.destination)
        self.assertEqual((self.destination / "games/MSFS2024").resolve(), workspace / "game")
        self.assertEqual((self.destination / "private/xdg").resolve(), workspace / "xdg")
        self.assertTrue((workspace / "xdg/config").is_dir())
        self.assertTrue((self.destination / "private/local-saves.enabled").is_file())

    def test_missing_bootstrap_is_explained_without_auth_or_writes(self):
        with patch.object(setup, "bootstrap_module", return_value=None):
            status = self.manager.snapshot()
            self.assertFalse(status["install_available"])
            self.assertTrue(status["install_unavailable_reason"])
            self.manager.check({"mode": "install", "market": "AT"})
            self.assertIn("noch nicht verfügbar", self.wait("failed")["error"])
        self.assert_no_staging()

    def test_check_is_read_only_and_ready_reserves_launcher(self):
        before = self.original.read_bytes()
        job = self.ready()
        self.assertGreater(len(job["checks"]), 5)
        self.assertTrue(all(row["ok"] for row in job["checks"]))
        self.assertIsNone(self.launcher.runtime)
        self.assertFalse((self.root / "state/config.json").exists())
        self.assert_no_staging()
        self.assertEqual(self.original.read_bytes(), before)
        self.assertTrue(self.launcher.setup_busy)
        for action in (lambda: self.launcher.configure(str(self.root)), self.launcher.launch, self.launcher.backup, lambda: self.manager.check(self.data), lambda: self.manager.start("stale")):
            with self.assertRaises(LauncherError):
                action()
        self.manager.cancel(job["id"])
        self.assertFalse(self.launcher.setup_busy)
        self.assertEqual(self.manager.snapshot()["state"], "cancelled")

    def test_prepare_copies_verified_files_and_preserves_sources(self):
        self.file("artifacts/extra-should-not-copy", b"not in manifest")
        job = self.install()
        self.assertEqual(job["progress"], 100)
        self.assertEqual(self.launcher.runtime, self.destination)
        self.assertTrue(self.launcher.status()["game"]["can_start"])
        self.assertFalse(self.launcher.setup_busy)
        installed = self.destination / "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"
        self.assertEqual(installed.read_bytes(), b"runtime/xgameruntime.dll")
        self.assertFalse(installed.is_symlink())
        self.assertTrue((self.system32 / "xgameruntime.dll").is_symlink())
        self.assertEqual(self.original.read_bytes(), b"synthetic runner original")
        self.assertEqual((self.destination / "games/MSFS2024").resolve(), self.root / "game")
        self.assertFalse((self.destination / "private/local-saves.enabled").exists())
        self.assertEqual(list(self.destination.rglob("extra-should-not-copy")), [])
        self.assertEqual(self.destination.stat().st_mode & 0o777, 0o700)
        self.assertEqual((self.destination / "private/runtime.json").stat().st_mode & 0o777, 0o600)

    def test_local_saves_only_on_explicit_selection(self):
        self.install(local_saves=True)
        self.assertEqual((self.destination / "private/local-saves.enabled").stat().st_mode & 0o777, 0o600)
        self.assertEqual((self.destination / "private/local-saves").stat().st_mode & 0o777, 0o700)

    def test_optional_cloud_helper_is_verified_and_copied(self):
        helper = self.file("artifacts/" + setup.CONNECTED_STORAGE_HELPER, b"synthetic helper")
        helper.chmod(0o700)
        manifest_path = self.root / "artifacts/manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["files"][setup.CONNECTED_STORAGE_HELPER] = hashlib.sha256(helper.read_bytes()).hexdigest()
        manifest["features"] = [setup.CONNECTED_STORAGE_FEATURE]
        manifest_path.write_text(json.dumps(manifest))
        self.install()
        self.assertEqual((self.destination / setup.CONNECTED_STORAGE_HELPER).read_bytes(), helper.read_bytes())
        self.assertEqual((self.destination / setup.CONNECTED_STORAGE_HELPER).stat().st_mode & 0o777, 0o700)

    def test_advertised_missing_or_changed_cloud_helper_fails_preflight(self):
        path = self.root / "artifacts/manifest.json"
        manifest = json.loads(path.read_text())
        manifest["features"] = [setup.CONNECTED_STORAGE_FEATURE]
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(setup.SetupError, "Dateiliste"):
            setup.preflight(self.data, source_root=self.repo)
        self.file("artifacts/" + setup.CONNECTED_STORAGE_HELPER, b"changed").chmod(0o700)
        manifest["files"][setup.CONNECTED_STORAGE_HELPER] = "0" * 64
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(setup.SetupError, "Prüfsumme"):
            setup.preflight(self.data, source_root=self.repo)
        self.assertFalse(self.destination.exists())

    def test_manifest_change_after_check_stops_before_creation(self):
        job = self.ready()
        (self.root / "artifacts/runtime/xgameruntime.dll").write_bytes(b"changed")
        self.manager.start(job["id"])
        failed = self.wait("failed")
        self.assertIn("Prüfsumme", failed["error"])
        self.assert_no_staging()
        self.assertFalse(self.launcher.setup_busy)

    def test_runner_mismatch_and_invalid_types_fail_cleanly(self):
        self.original.write_bytes(b"wrong runner")
        self.manager.check(self.data)
        self.assertIn("Proton-Runner", self.wait("failed")["error"])
        self.assert_no_staging()
        for mode in (None, [], {}, 4):
            with self.assertRaises(LauncherError):
                self.manager.check({"mode": mode})

    def test_dangling_destination_is_not_replaced(self):
        self.destination.symlink_to(self.root / "missing")
        self.manager.check(self.data)
        self.assertIn("existiert", self.wait("failed")["error"])
        self.assertTrue(self.destination.is_symlink())
        self.assertEqual(self.destination.readlink(), self.root / "missing")

    def test_destination_race_keeps_other_directory(self):
        job = self.ready()
        publish = setup._publish
        def race(source, destination):
            destination.mkdir()
            (destination / "keep").write_text("other")
            return publish(source, destination)
        with patch.object(setup, "_publish", side_effect=race):
            self.manager.start(job["id"])
            self.wait("failed")
        self.assertEqual((self.destination / "keep").read_text(), "other")
        self.assertEqual(list(self.root.glob(".flightdeck-setup-*")), [])
        self.assertIsNone(self.launcher.runtime)

    def test_changed_copy_is_rejected_and_removed(self):
        job = self.ready()
        copy_file = setup._copy_file
        def change(source, target):
            copy_file(source, target)
            if target.name == "xgameruntime.dll":
                target.write_bytes(b"unexpected")
        with patch.object(setup, "_copy_file", side_effect=change):
            self.manager.start(job["id"])
            self.assertIn("Kopie verändert", self.wait("failed")["error"])
        self.assert_no_staging()

    def test_copied_prefix_system_directory_is_rechecked(self):
        job = self.ready()
        outside = self.root / "outside"
        outside.mkdir()
        def substituted(source, destination, cancel):
            (destination / "drive_c/windows").mkdir(parents=True)
            (destination / "drive_c/windows/system32").symlink_to(outside)
        with patch.object(setup, "_copy_prefix", side_effect=substituted):
            self.manager.start(job["id"])
            self.assertIn("echte Ordner", self.wait("failed")["error"])
        self.assertEqual(list(outside.iterdir()), [])
        self.assert_no_staging()

    def test_cancel_blocked_copy_cleans_own_staging(self):
        job = self.ready()
        entered = threading.Event()
        def blocked(source, destination, cancel):
            destination.mkdir()
            entered.set()
            if not cancel.wait(3):
                raise AssertionError("cancel did not reach copy")
            setup.interrupted(cancel)
        with patch.object(setup, "_copy_prefix", side_effect=blocked):
            self.manager.start(job["id"])
            self.assertTrue(entered.wait(2))
            self.manager.cancel(job["id"])
            self.wait("cancelled")
        self.assert_no_staging()
        self.assertFalse(self.launcher.setup_busy)

    def test_close_cancels_ready_job(self):
        self.ready()
        self.manager.close()
        self.assertEqual(self.manager.snapshot()["state"], "cancelled")
        self.assertFalse(self.launcher.setup_busy)
        self.assert_no_staging()

    def test_existing_runtime_requires_separate_start_without_writes(self):
        plan = setup.preflight(self.data, source_root=self.repo)
        setup.prepare(plan)
        before = {str(p.relative_to(self.destination)): p.lstat().st_mtime_ns for p in self.destination.rglob("*")}
        job = self.ready({"mode": "existing", "runtime_path": str(self.destination)})
        after = {str(p.relative_to(self.destination)): p.lstat().st_mtime_ns for p in self.destination.rglob("*")}
        self.assertEqual(before, after)
        self.assertIsNone(self.launcher.runtime)
        self.manager.start(job["id"])
        self.wait("complete")
        self.assertEqual(self.launcher.runtime, self.destination)

    def test_check_of_configured_runtime_does_not_create_play_lock(self):
        setup.prepare(setup.preflight(self.data, source_root=self.repo))
        self.launcher.configure(str(self.destination))
        lock = self.destination / "private/play.lock"
        self.assertFalse(lock.exists())
        self.ready({"mode": "existing", "runtime_path": str(self.destination)})
        self.assertFalse(lock.exists())

    def test_setup_check_rejects_fifo_lock_without_blocking(self):
        setup.prepare(setup.preflight(self.data, source_root=self.repo))
        self.launcher.configure(str(self.destination))
        lock = self.destination / "private/play.lock"
        os.mkfifo(lock)
        results = []
        def check():
            try:
                self.manager.check({"mode": "existing", "runtime_path": str(self.destination)})
            except LauncherError:
                results.append("rejected")
        thread = threading.Thread(target=check, daemon=True)
        thread.start()
        thread.join(timeout=2)
        if thread.is_alive():
            # Release a regressed blocking FIFO open before reporting failure.
            fd = os.open(lock, os.O_WRONLY | os.O_NONBLOCK)
            os.close(fd)
            thread.join(timeout=2)
            self.fail("Opening a FIFO lock blocked setup")
        self.assertEqual(results, ["rejected"])
        self.assertFalse(self.launcher.setup_busy)

    def test_existing_ready_reservation_blocks_game_config_and_backups(self):
        self.install(local_saves=True)
        (self.destination / "private/local-saves/synthetic-save").write_bytes(b"save")
        job = self.ready({"mode": "existing", "runtime_path": str(self.destination)})
        status = self.launcher.status()
        self.assertFalse(status["game"]["can_start"])
        self.assertFalse(status["saves"]["can_backup"])
        self.manager.cancel(job["id"])
        self.assertTrue(self.launcher.status()["game"]["can_start"])
        self.assertTrue(self.launcher.status()["saves"]["can_backup"])

    def test_picker_is_explicit_allowlisted_without_shell(self):
        with patch.object(self.manager, "_picker", return_value=("zenity", "/usr/bin/zenity")), patch.object(setup.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, str(self.root) + "\n")) as run:
            self.assertEqual(self.manager.pick("game_path", str(self.root))["path"], str(self.root))
            args, kwargs = run.call_args
            self.assertIsInstance(args[0], list)
            self.assertNotIn("shell", kwargs)
            self.assertEqual(kwargs["timeout"], 120)
            for field in ("command", None, [], {}):
                with self.assertRaises(LauncherError):
                    self.manager.pick(field)
            self.assertEqual(run.call_count, 1)

    def test_picker_cancel_timeout_and_headless_are_neutral(self):
        with patch.object(self.manager, "_picker", return_value=None):
            self.assertFalse(self.manager.snapshot()["directory_picker"])
            with self.assertRaises(LauncherError):
                self.manager.pick("game_path")
        with patch.object(self.manager, "_picker", return_value=("zenity", "/usr/bin/zenity")), patch.object(setup.subprocess, "run") as run:
            run.return_value = subprocess.CompletedProcess([], 1, "")
            self.assertTrue(self.manager.pick("game_path")["cancelled"])
            run.side_effect = subprocess.TimeoutExpired("zenity", 120)
            self.assertTrue(self.manager.pick("game_path")["cancelled"])


if __name__ == "__main__":
    unittest.main()
