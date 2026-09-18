# SPDX-License-Identifier: MIT
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from flightdeck.backend import Launcher, LauncherError
from flightdeck import game_update as update
from flightdeck import game_install
from flightdeck.setup import SetupCancelled


def info(revision="a", game_version="1.2.0.0"):
    return {"schema": 1, "store_id": game_install.MSFS_STORE_ID, "version": game_version,
            "version_id": "22222222-2222-4222-8222-222222222222",
            "content_id": "11111111-1111-4111-8111-111111111111", "package_identity": revision * 64, "size_bytes": 4096}


def game(path, version="1.1.0.0", name="Example.MSFS"):
    path.mkdir(parents=True, exist_ok=True)
    (path / "MicrosoftGame.Config").write_text(f'<Game><Identity Name="{name}" Publisher="CN=Example" Version="{version}"/><StoreId>{game_install.MSFS_STORE_ID}</StoreId><Executable Name="FlightSimulator2024.exe"/></Game>')
    (path / "FlightSimulator2024.exe").write_bytes(b"encrypted-synthetic-game")
    (path / ".xodus-streaming.msixvc").write_bytes(b"synthetic-metadata")
    return path


class UpdateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        for name in ("private", "games", "tools"):
            (self.runtime / name).mkdir(parents=True, mode=0o700)
        (self.runtime / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 0\n")
        (self.runtime / "private/runtime.json").write_text('{"market":"AT"}')
        self.game = game(self.root / "original-game")
        (self.runtime / "games/MSFS2024").symlink_to(self.game)
        self.launcher = Launcher(self.root / "state", str(self.runtime))
        self.addCleanup(self.launcher.setup.close)
        self.cancel = threading.Event()
        self.tool = self.root / "cli"
        self.tool.write_text("#!/usr/bin/env python3\nimport json\nprint(" + repr(json.dumps(info())) + ")\n")
        self.tool.chmod(0o700)
        self.sha = hashlib.sha256(self.tool.read_bytes()).hexdigest()

    def plan(self):
        return update.UpdatePlan(self.runtime, update.installed(self.game), self.game, info(), self.tool, self.sha,
                                 [update.FEATURE, game_install.RESUME_FEATURE], "AT")

    def download(self, cli, sha, target, market, **kw):
        self.assertEqual(kw["expected_package"], "a" * 64)
        self.assertFalse(kw["sign_in"])
        self.assertEqual(market, "AT")
        return game(target, "1.2.0.0")

    def install(self, plan=None, **kw):
        return update.install(self.launcher, plan or self.plan(), notify=lambda *args: None,
                              cancel=self.cancel, control=game_install.DownloadControl(), committing=lambda: None, **kw)

    def test_installed_requires_actual_identity_store_and_four_component_version(self):
        self.assertEqual(update.installed(self.game)["version"], "1.1.0.0")
        path = self.game / "MicrosoftGame.Config"
        path.write_text(path.read_text().replace(game_install.MSFS_STORE_ID, "OTHERPRODUCT"))
        with self.assertRaises(update.UpdateError): update.installed(self.game)
        for value in ("1.0", "65536.0.0.0", "1.0.0.-1", None):
            with self.assertRaises(update.UpdateError): update.version(value)

    def test_legacy_market_reads_exact_launcher_literal_without_execution_or_writes(self):
        metadata=self.runtime/"private/runtime.json";metadata.unlink()
        script=self.runtime/"tools/launch-msfs.sh"
        for literal in ("AT", "'AT'", '\"AT\" # explicit region'):
            script.write_text("#!/bin/sh\nexport XODUS_STORE_MARKET="+literal+"\nexit 99\n")
            self.assertEqual(update.configured_market(self.runtime),"AT")
            self.assertFalse(metadata.exists())
        with patch.object(update,"tools",return_value=(self.tool,self.sha,[])), patch.object(update,"package_info",return_value=info()) as query:
            plan=update.check(self.launcher,{},notify=lambda *args:None,cancel=self.cancel)
            self.assertEqual(plan.market,"AT");self.assertEqual(query.call_args.args[3],"AT")
        self.assertFalse(metadata.exists())

    def test_legacy_region_rejects_dynamic_ambiguous_missing_and_corrupt_metadata(self):
        metadata=self.runtime/"private/runtime.json";metadata.unlink()
        script=self.runtime/"tools/launch-msfs.sh"
        for text in ("export XODUS_STORE_MARKET =AT", "export XODUS_STORE_MARKET= AT", "XODUS_STORE_MARKET=AT", "export XODUS_STORE_MARKET=AT#comment", "export XODUS_STORE_MARKET=AT\nXODUS_STORE_MARKET=US", "export XODUS_STORE_MARKET=$(false)","export XODUS_STORE_MARKET=$REGION", "export XODUS_STORE_MARKET=AT; true", "export XODUS_STORE_MARKET=AT\nexport XODUS_STORE_MARKET=US", "# export XODUS_STORE_MARKET=AT"):
            script.write_text(text)
            with self.assertRaises(update.UpdateError):update.configured_market(self.runtime)
        script.write_text("export XODUS_STORE_MARKET=AT\n")
        metadata.write_text('{"market":"invalid"}')
        with self.assertRaises(update.UpdateError):update.configured_market(self.runtime)
        metadata.write_text('not json')
        with self.assertRaises(update.UpdateError):update.configured_market(self.runtime)

    def test_projection_is_strict_and_never_exposes_extra_fields(self):
        self.assertEqual(update.validate_info(info()), info())
        for key, value in (("url", "https://invalid.example/"), ("schema", True), ("size_bytes", True),
                           ("package_identity", "new"), ("version_id", "not-an-id")):
            broken = info(); broken[key] = value
            with self.assertRaises(update.UpdateError): update.validate_info(broken)

    def test_compound_revision_projection_requires_exact_matching_base_version(self):
        value=info(game_version="1.8.16.0");value["version_id"]="1.8.16.0.aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
        self.assertEqual(update.validate_info(value),value)
        for revision in ("1.8.17.0.aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "1.8.16.0.not-guid", "1.8.16.0.aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee.extra", "1.8.16.0.aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee "):
            invalid=copy.deepcopy(value);invalid["version_id"]=revision
            with self.assertRaises(update.UpdateError):update.validate_info(invalid)

    def test_real_synthetic_cli_reads_only_projected_json(self):
        self.assertEqual(update.package_info(self.tool, self.sha, self.runtime, "AT", self.cancel), info())

    def test_missing_auth_exit_never_starts_login_automatically(self):
        self.tool.write_text("#!/bin/sh\nexit 77\n")
        sha = hashlib.sha256(self.tool.read_bytes()).hexdigest()
        with self.assertRaises(update.AuthRequired): update.package_info(self.tool, sha, self.runtime, "AT", self.cancel)

    def test_cancel_stops_and_reaps_synthetic_check_child(self):
        self.tool.write_text("#!/usr/bin/env python3\nimport time\ntime.sleep(30)\n")
        sha = hashlib.sha256(self.tool.read_bytes()).hexdigest()
        timer = threading.Timer(.15, self.cancel.set); timer.start()
        started = time.monotonic()
        with self.assertRaises(SetupCancelled): update.package_info(self.tool, sha, self.runtime, "AT", self.cancel)
        timer.join(); self.assertLess(time.monotonic() - started, 3)

    def test_oversized_child_output_is_rejected(self):
        self.tool.write_text("#!/usr/bin/env python3\nprint('x'*10000)\n")
        sha = hashlib.sha256(self.tool.read_bytes()).hexdigest()
        with self.assertRaises((ValueError, update.UpdateError)):
            update.package_info(self.tool, sha, self.runtime, "AT", self.cancel)

    def test_update_preserves_old_package_prefix_addons_and_saves_then_rolls_back(self):
        retained = {}
        for name in ("local/msfs-prefix/user.reg", "private/local-saves/save.bin", "addons/manifest.json"):
            path = self.runtime / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(b"unchanged")
            retained[path] = (path.stat().st_ino, path.read_bytes())
        with patch.object(update, "download_game", side_effect=self.download): self.install()
        self.assertEqual(update.installed(self.runtime / "games/MSFS2024")["version"], "1.2.0.0")
        self.assertEqual(update.installed(self.game)["version"], "1.1.0.0")
        history, old = update._history(self.runtime)
        self.assertEqual(old.resolve(), self.game)
        self.launcher.setup.job = {"id":"previous", "mode":"update", "state":"complete", "runtime_path":str(self.runtime),
                                   "latest_version":"1.2.0.0", "update_available":False}
        self.assertTrue(update.rollback(self.launcher)["ok"])
        self.assertEqual((self.runtime / "games/MSFS2024").resolve(), self.game)
        with patch.object(update, "tools", return_value=(self.tool, self.sha, [])):
            status = update.snapshot(self.launcher)
        self.assertIsNone(status["update_available"])
        self.assertIsNone(status["latest_version"])
        self.assertIsNone(status["job"])
        for path, expected in retained.items(): self.assertEqual((path.stat().st_ino, path.read_bytes()), expected)

    def test_real_directory_install_can_exchange_and_rollback_without_two_step_gap(self):
        path = self.runtime / "games/MSFS2024"; path.unlink(); self.game.rename(path)
        plan = self.plan() if self.game.exists() else update.UpdatePlan(self.runtime, update.installed(path), path, info(), self.tool, self.sha, [], "AT")
        inode = path.stat().st_ino
        with patch.object(update, "download_game", side_effect=self.download): self.install(plan)
        self.assertTrue(path.is_symlink())
        update.rollback(self.launcher)
        self.assertFalse(path.is_symlink()); self.assertEqual(path.stat().st_ino, inode)

    def test_download_failure_or_wrong_version_cannot_switch(self):
        for failure in (update.UpdateError("fixture failure"), None):
            with self.subTest(failure=failure):
                with patch.object(update, "download_game", side_effect=failure if failure else lambda a,b,c,*args,**kw: game(c, "1.3.0.0")):
                    with self.assertRaises(update.UpdateError): self.install()
                self.assertEqual((self.runtime / "games/MSFS2024").resolve(), self.game)

    def test_new_attempt_uses_new_folder_and_never_reuses_an_old_revision_journal(self):
        folders = []
        def fail(a,b,target,*args,**kw):
            folders.append(target); target.mkdir(); (target/"old-journal").write_text("old")
            raise update.UpdateError("fixture")
        with patch.object(update, "download_game", side_effect=fail):
            for _ in range(2):
                with self.assertRaises(update.UpdateError): self.install()
        self.assertNotEqual(folders[0], folders[1])
        self.assertTrue(all((p/"old-journal").exists() for p in folders))

    def test_failed_atomic_exchange_keeps_active_version_and_old_history(self):
        record = self.runtime / "private/game-update.json"; record.write_text('{"prior":"history"}')
        with patch.object(update, "download_game", side_effect=self.download), patch.object(update, "exchange", side_effect=update.UpdateError("fixture")):
            with self.assertRaises(update.UpdateError): self.install()
        self.assertEqual((self.runtime/"games/MSFS2024").resolve(), self.game)
        self.assertEqual(json.loads(record.read_text()), {"prior":"history"})

    def test_external_runtime_lock_blocks_download(self):
        with self.launcher.runtime_lock(), patch.object(update, "download_game") as child:
            with self.assertRaises(LauncherError): self.install()
            child.assert_not_called()

    def test_busy_and_unsupported_cli_do_not_advertise_actions(self):
        with patch.object(update, "tools", side_effect=update.UpdateError("unsupported")):
            value = update.snapshot(self.launcher)
        self.assertFalse(value["available"]); self.assertFalse(value["can_start"])
        with patch.object(update, "tools", return_value=(self.tool, self.sha, [])):
            self.launcher.setup_busy = True
            self.assertFalse(update.snapshot(self.launcher)["can_check"])
        self.launcher.setup_busy = False

    def test_setup_update_job_has_ready_identity_auth_and_cancel_contract(self):
        manager = self.launcher.setup
        with patch.object(update, "check", return_value=self.plan()):
            result = manager.check({"mode":"update"}); manager.thread.join(3)
        self.assertEqual(manager.job["state"], "ready"); self.assertTrue(manager.job["update_available"])
        self.assertFalse(self.launcher.setup_busy)
        manager.cancel(result["job"]["id"])
        self.assertFalse(self.launcher.setup_busy)
        with patch.object(update, "check", side_effect=update.AuthRequired("fixture")):
            manager.check({"mode":"update"}); manager.thread.join(3)
        self.assertTrue(manager.job["auth_required"]); self.assertEqual(manager.job["state"], "failed")

    def test_up_to_date_check_completes_without_download_or_reservation(self):
        plan = self.plan(); plan.latest = info(game_version="1.1.0.0")
        with patch.object(update, "check", return_value=plan):
            self.launcher.setup.check({"mode":"update"}); self.launcher.setup.thread.join(3)
        self.assertFalse(self.launcher.setup.job["update_available"])
        self.assertEqual(self.launcher.setup.job["state"], "complete")
        self.assertFalse(self.launcher.setup_busy)

    def test_ready_check_can_be_replaced_but_start_rechecks_external_game_lock(self):
        manager = self.launcher.setup
        with patch.object(update, "check", return_value=self.plan()):
            first = manager.check({"mode":"update"}); manager.thread.join(3)
            second = manager.check({"mode":"update"}); manager.thread.join(3)
        self.assertNotEqual(first["job"]["id"], second["job"]["id"])
        self.assertFalse(self.launcher.setup_busy)
        with self.launcher.runtime_lock(), patch.object(update, "install") as install, patch.object(update, "tools", return_value=(self.tool, self.sha, [])):
            self.assertFalse(update.snapshot(self.launcher)["can_start"])
            with self.assertRaises(LauncherError): manager.start(second["job"]["id"])
            install.assert_not_called()
        self.assertEqual(manager.job["state"], "ready")

    def test_start_reserves_the_runtime_and_rejects_changed_runtime(self):
        manager = self.launcher.setup
        with patch.object(update, "check", return_value=self.plan()):
            result = manager.check({"mode":"update"}); manager.thread.join(3)
        started, finish = threading.Event(), threading.Event()
        def installer(*args, **kwargs):
            started.set(); finish.wait(3)
            return self.runtime
        with patch.object(update, "install", side_effect=installer):
            manager.start(result["job"]["id"])
            self.assertTrue(started.wait(3)); self.assertTrue(self.launcher.setup_busy)
            self.assertFalse(self.launcher.reserve_desktop_refresh())
            finish.set(); manager.thread.join(3)
        self.assertFalse(self.launcher.setup_busy)
        self.assertEqual(manager.job["state"], "complete")

    def test_commit_phase_rejects_cancel_and_preserves_false_pause_controls(self):
        manager = self.launcher.setup
        manager.job = {"id":"commit", "mode":"update", "state":"installing", "phase":"switch_update", "can_pause":False, "can_resume":False}
        with self.assertRaises(LauncherError): manager.cancel("commit")
        self.assertFalse(manager.cancel_event.is_set())

    def test_symlinked_update_root_cannot_write_outside_private_runtime(self):
        outside = self.root / "outside"; outside.mkdir()
        (self.runtime / "private/game-updates").symlink_to(outside)
        with patch.object(update, "download_game") as child:
            with self.assertRaises(update.UpdateError): self.install()
            child.assert_not_called()
        self.assertEqual(list(outside.iterdir()), [])

    def test_user_packages_inside_game_block_updates_without_reading_account(self):
        (self.runtime/"private/runtime.json").write_text(json.dumps({"market":"AT", "community_path":str(self.game/"Community")}))
        with patch.object(update, "tools", return_value=(self.tool, self.sha, [])), patch.object(update, "package_info") as network:
            with self.assertRaisesRegex(update.UpdateError, "Benutzerpakete"):
                update.check(self.launcher, {}, notify=lambda *a:None, cancel=self.cancel)
            network.assert_not_called()

    def test_cancel_of_old_ready_job_cannot_release_rollback_reservation(self):
        with patch.object(update, "download_game", side_effect=self.download): self.install()
        manager = self.launcher.setup
        manager.job = {"id":"old", "mode":"update", "state":"ready", "phase":"ready"}
        entered, finish = threading.Event(), threading.Event()
        real_exchange = update.exchange
        failures = []
        def barrier(*args):
            entered.set(); finish.wait(3)
            return real_exchange(*args)
        def worker():
            try: update.rollback(self.launcher)
            except Exception as error: failures.append(error)
        with patch.object(update, "exchange", side_effect=barrier):
            thread = threading.Thread(target=worker); thread.start()
            self.assertTrue(entered.wait(3))
            self.assertTrue(self.launcher.setup_busy)
            with self.assertRaises(LauncherError): manager.cancel("old")
            self.assertTrue(self.launcher.setup_busy)
            with self.assertRaises(LauncherError): self.launcher.reserve_setup()
            finish.set(); thread.join(3)
        self.assertFalse(thread.is_alive()); self.assertEqual(failures, [])
        self.assertFalse(self.launcher.setup_busy)

    def test_second_update_crash_before_exchange_retains_confirmed_rollback(self):
        with patch.object(update, "download_game", side_effect=self.download): self.install()
        active = self.runtime / "games/MSFS2024"
        second = self.plan(); second.current = update.installed(active); second.game_target = active.resolve()
        second.latest = info("b", "1.3.0.0")
        class Crash(BaseException): pass
        with patch.object(update, "download_game", side_effect=lambda a,b,c,*args,**kw: game(c,"1.3.0.0")), patch.object(update, "exchange", side_effect=Crash):
            with self.assertRaises(Crash): self.install(second)
        self.assertEqual(update.installed(active)["version"], "1.2.0.0")
        history, previous = update._history(self.runtime)
        self.assertEqual(update.installed(previous)["version"], "1.1.0.0")
        self.assertTrue(update.rollback(self.launcher)["ok"])
        self.assertEqual(update.installed(active)["version"], "1.1.0.0")

    def test_post_exchange_crash_has_rollback_and_next_update_preserves_it(self):
        class Crash(BaseException): pass
        real_write = update._write
        def crash_confirmation(path, value):
            if path.name == "game-update.json": raise Crash()
            return real_write(path, value)
        with patch.object(update, "download_game", side_effect=self.download), patch.object(update, "_write", side_effect=crash_confirmation):
            with self.assertRaises(Crash): self.install()
        active = self.runtime / "games/MSFS2024"
        self.assertEqual(update.installed(active)["version"], "1.2.0.0")
        self.assertEqual(update._history(self.runtime)[0]["old_identity"]["version"], "1.1.0.0")
        second = self.plan(); second.current = update.installed(active); second.game_target = active.resolve()
        second.latest = info("b", "1.3.0.0")
        with patch.object(update, "download_game", side_effect=lambda a,b,c,*args,**kw: game(c,"1.3.0.0")), patch.object(update, "exchange", side_effect=Crash):
            with self.assertRaises(Crash): self.install(second)
        self.assertEqual(update._history(self.runtime)[0]["old_identity"]["version"], "1.1.0.0")

    def test_own_update_lease_keeps_pause_resume_cancel_and_foreign_lock_detection(self):
        manager=self.launcher.setup
        with patch.object(update,"check",return_value=self.plan()):
            manager.check({"mode":"update"});manager.thread.join(3)
        job_id=manager.job["id"]
        entered=threading.Event()
        def pausable(cli,sha,target,market,**kw):
            control=kw["control"]
            control.downloading(True);entered.set()
            if not control.requested.wait(3):raise RuntimeError("pause not requested")
            control.wait_paused(kw["cancel"])
            control.downloading(True)
            while not kw["cancel"].wait(.02):pass
            game_install._cancelled(kw["cancel"])
        with patch.object(update,"download_game",side_effect=pausable):
            manager.start(job_id);self.assertTrue(entered.wait(3))
            self.assertEqual(self.launcher.status()["game"]["state"],"stopped")
            self.assertTrue(self.launcher.setup_busy);self.assertTrue(manager.job["can_pause"])
            self.assertFalse(self.launcher.reserve_desktop_refresh())
            manager.download_action(job_id,"pause")
            deadline=time.monotonic()+3
            while manager.job["phase"]!="paused" and time.monotonic()<deadline:time.sleep(.01)
            self.assertTrue(manager.job["can_resume"])
            self.assertEqual(self.launcher.status()["game"]["state"],"stopped")
            manager.download_action(job_id,"resume")
            manager.cancel(job_id);manager.thread.join(3)
        self.assertFalse(manager.thread.is_alive());self.assertEqual(manager.job["state"],"cancelled")
        self.assertIsNone(self.launcher._owned_runtime_operation)
        self.assertFalse(self.launcher.setup_busy)
        self.assertEqual((self.runtime/"games/MSFS2024").resolve(),self.game)
        with self.launcher.runtime_lock():
            self.assertEqual(self.launcher.status()["game"]["state"],"external")



if __name__ == "__main__": unittest.main()
