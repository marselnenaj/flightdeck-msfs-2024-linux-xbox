# SPDX-License-Identifier: MIT
"""Synthetic known download baselines; never derive expected hashes after damage."""
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck import game_integrity as integrity, game_update as update, game_install
from flightdeck.backend import Launcher, LauncherError
from flightdeck.setup import SetupCancelled
from tests.test_game_update import game, info


def seal(path, files):
    journal = path / ".xodus-resume"
    journal.mkdir(mode=0o700)
    (journal / "lock").touch(mode=0o600)
    (journal / "package.json").write_text(json.dumps({"format":1,"package_sha256":"a"*64}))
    rows = [{"name":name,"length":len(data),"sha256":hashlib.sha256(data).hexdigest()} for name,data in files.items()]
    index = {"format":1,"source":"xodus-completed-download-v1","package_sha256":"a"*64,"files":rows}
    (journal / "integrity.json").write_text(json.dumps(index))
    return index


class IntegrityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root/"runtime"
        for part in ("tools","private","games"):
            (self.runtime/part).mkdir(parents=True,mode=0o700)
        (self.runtime/"tools/play-msfs.sh").write_text("#!/bin/sh\nexit 0\n")
        (self.runtime/"private/runtime.json").write_text('{"market":"AT"}')
        self.game = game(self.root/"game")
        (self.runtime/"games/MSFS2024").symlink_to(self.game)
        self.launcher = Launcher(self.root/"state",str(self.runtime)); self.addCleanup(self.launcher.setup.close)
        self.cancel = threading.Event()
        self.files = {"nested/data.bin":b"original bytes", "empty":b"", "encrypted.exe":b"\x00\xffopaque", ".xodus-streaming.msixvc":b"known metadata", "MicrosoftGame.Config":(self.game/"MicrosoftGame.Config").read_bytes()}
        for name,data in self.files.items():
            path=self.game/name;path.parent.mkdir(exist_ok=True,parents=True);path.write_bytes(data)
        self.index = seal(self.game,self.files)

    def verify(self,notify=lambda *args:None):
        return integrity.verify(self.launcher,notify=notify,cancel=self.cancel)

    def test_full_expected_set_and_encrypted_bytes_match_without_native_magic(self):
        value=self.verify()
        self.assertEqual(value,{"checked":5,"missing":0,"changed":0,"unreadable":0,"total":5,"healthy":True})
        self.assertNotIn("nested",json.dumps(value))

    def test_missing_changed_and_symlink_counts_are_disjoint(self):
        (self.game/"empty").unlink()
        (self.game/"encrypted.exe").write_bytes(b"changed!")
        (self.game/"nested/data.bin").unlink()
        (self.game/"nested/data.bin").symlink_to(self.root/"private-outside")
        value=self.verify()
        self.assertEqual((value["checked"],value["missing"],value["changed"],value["unreadable"]),(5,1,1,1))
        self.assertFalse(value["healthy"])

    def test_deleted_file_and_receipt_cannot_disappear_from_complete_set(self):
        (self.game/"empty").unlink()
        value=self.verify();self.assertEqual(value["total"],5);self.assertEqual(value["missing"],1)

    def test_cache_corruption_deletion_and_missing_index_entry_are_detected(self):
        cache=self.game/".xodus-streaming.msixvc"
        cache.write_bytes(b"corrupt metadata")
        self.assertEqual(self.verify()["changed"],1)
        cache.unlink()
        self.assertEqual(self.verify()["missing"],1)
        value=copy.deepcopy(self.index);value["files"]=[row for row in value["files"] if row["name"]!=".xodus-streaming.msixvc"]
        (self.game/".xodus-resume/integrity.json").write_text(json.dumps(value))
        self.assertFalse(integrity.available(self.game))

    def test_no_index_does_not_create_or_trust_a_baseline(self):
        (self.game/".xodus-resume/integrity.json").unlink()
        self.assertFalse(integrity.available(self.game))
        with self.assertRaisesRegex(ValueError,"Prüfnachweis"):self.verify()
        self.assertFalse((self.game/".xodus-resume/integrity.json").exists())

    def test_invalid_revision_traversal_duplicate_hash_and_bounds_are_unavailable(self):
        path=self.game/".xodus-resume/integrity.json"
        cases=[]
        v=copy.deepcopy(self.index);v["package_sha256"]="b"*64;cases.append(v)
        for name in ("../outside","/absolute","nested/../outside","nested\\outside",".xodus-resume/lock"):
            v=copy.deepcopy(self.index);v["files"][0]["name"]=name;cases.append(v)
        v=copy.deepcopy(self.index);v["files"].append(v["files"][0]);cases.append(v)
        v=copy.deepcopy(self.index);v["files"][0]["sha256"]="bad";cases.append(v)
        v=copy.deepcopy(self.index);v["files"][0]["length"]=True;cases.append(v)
        v=copy.deepcopy(self.index);v["files"]=[];cases.append(v)
        for value in cases:
            path.write_text(json.dumps(value));self.assertFalse(integrity.available(self.game))
        path.write_bytes(b" "*(integrity.MAX_INDEX+1));self.assertFalse(integrity.available(self.game))

    def test_index_and_intermediate_directory_symlinks_never_followed(self):
        path=self.game/".xodus-resume/integrity.json"
        outside=self.root/"index";path.rename(outside);path.symlink_to(outside)
        self.assertFalse(integrity.available(self.game))
        path.unlink();outside.rename(path)
        (self.game/"nested/data.bin").unlink();(self.game/"nested").rmdir()
        (self.game/"nested").symlink_to(self.root)
        self.assertEqual(self.verify()["unreadable"],1)

    def test_cancellation_does_not_publish_partial_success_or_modify_files(self):
        def cancel_after_first(*args):self.cancel.set()
        with self.assertRaises(SetupCancelled):self.verify(cancel_after_first)
        self.assertEqual((self.game/"nested/data.bin").read_bytes(),self.files["nested/data.bin"])

    def test_verification_reserves_launcher_and_projects_own_lock_only(self):
        entered,finish=threading.Event(),threading.Event()
        real_verify=integrity.verify
        def held(*args,**kwargs):
            def notify(*values):
                entered.set();finish.wait(3);kwargs["notify"](*values)
            return real_verify(*args,notify=notify,cancel=kwargs["cancel"])
        with patch.object(integrity,"verify",side_effect=held):
            response=self.launcher.setup.check({"mode":"update","operation":"verify"})
            self.assertTrue(entered.wait(3))
            status=self.launcher.status()
            self.assertEqual(status["game"]["state"],"stopped");self.assertTrue(status["setup"]["busy"])
            self.assertFalse(status["game"]["can_start"])
            with self.assertRaises(LauncherError):self.launcher.reserve_setup()
            self.launcher.setup.cancel(response["job"]["id"]);finish.set();self.launcher.setup.thread.join(3)
        self.assertEqual(self.launcher.setup.job["state"],"cancelled")
        self.assertNotIn("integrity_result",self.launcher.setup.job)
        with self.launcher.runtime_lock():
            self.assertEqual(self.launcher.status()["game"]["state"],"external")

    def test_verified_download_identity_can_repair_missing_config_only_with_bound_index(self):
        expected=update.installed(self.game)
        integrity.record_installation(self.game)
        (self.game/"MicrosoftGame.Config").unlink()
        self.assertEqual(integrity.installed_identity(self.game),expected)
        path=self.game/".xodus-resume/integrity.json"
        bad=copy.deepcopy(self.index);bad["files"][0]["sha256"]="b"*64;path.write_text(json.dumps(bad))
        with self.assertRaises(ValueError):integrity.installed_identity(self.game)

    def test_changed_config_cannot_be_blessed_as_original_download_identity(self):
        path=self.game/"MicrosoftGame.Config"
        path.write_text(path.read_text().replace("1.1.0.0","1.9.0.0"))
        with self.assertRaises(ValueError):integrity.record_installation(self.game)
        self.assertFalse((self.game/".xodus-resume/installed.json").exists())

    def test_same_version_repair_is_explicit_ready_plan_and_replaces_only_base(self):
        tool=self.root/"cli";tool.write_text("unused")
        plan=update.UpdatePlan(self.runtime,update.installed(self.game),self.game,info(game_version="1.1.0.0"),tool,"a"*64,[],"AT")
        with patch.object(update,"check",return_value=plan):
            self.launcher.setup.check({"mode":"update","operation":"repair"});self.launcher.setup.thread.join(3)
        job=self.launcher.setup.job
        self.assertEqual(job["state"],"ready");self.assertFalse(job["update_available"]);self.assertFalse(self.launcher.setup_busy)
        with patch.object(update,"tools",return_value=(tool,"a"*64,[])):
            self.assertTrue(update.snapshot(self.launcher)["can_start"])
        with patch.object(update,"download_game",side_effect=lambda a,b,target,*args,**kw:game(target,"1.1.0.0")):
            self.launcher.setup.start(job["id"]);self.launcher.setup.thread.join(3)
        self.assertEqual(self.launcher.setup.job["state"],"complete")
        self.assertNotEqual((self.runtime/"games/MSFS2024").resolve(),self.game)
        self.assertTrue(self.game.exists());self.assertTrue(update.rollback(self.launcher)["ok"])
        self.assertEqual((self.runtime/"games/MSFS2024").resolve(),self.game)
