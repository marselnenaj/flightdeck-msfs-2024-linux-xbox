# SPDX-License-Identifier: MIT
"""Synthetic local helpers only: no sign-in, game, network or checkout."""
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from flightdeck.backend import Launcher, LauncherError
from flightdeck import store_check, store_diagnostics

BUILD = {"launcher":"0.1.7", "files":{name:"a"*64 for name in store_diagnostics.COMPONENTS}}


class StoreCheckTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        for name in ("tools", "private", "bin"):
            (self.runtime / name).mkdir(parents=True)
        (self.runtime / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 0\n")
        self.launcher = Launcher(self.root / "state", str(self.runtime))
        self.check = self.launcher.store_check
        self.addCleanup(self.check.close)
        for target, result in (("verify_runtime", BUILD), ("config", {})):
            patcher = patch.object(store_check, target, return_value=result)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.helper("xodus-service", "import json\nfor s in ('account','catalog','license','library'):\n print(json.dumps({'stage':s,'state':'passed','code':{'account':'local_session','catalog':'available','license':'verified','library':'available'}[s]}),flush=True)\n")
        self.helper("xodus-cli", "print('{\"stage\":\"window\",\"state\":\"passed\",\"code\":\"visible\"}',flush=True)\n")

    def helper(self, name, body):
        p = self.runtime / "bin" / name
        p.write_text("#!"+sys.executable+"\n"+body)
        p.chmod(0o700)

    def done(self):
        self.check.thread.join(timeout=5)
        self.assertFalse(self.check.thread.is_alive())
        self.assertFalse(self.launcher.setup_busy)
        return self.check.snapshot()["job"]

    def test_explicit_check_collects_same_runtime_and_reading_never_starts(self):
        with patch.object(store_check.subprocess, "Popen") as child:
            self.assertIsNone(self.check.snapshot()["job"])
            self.assertIsNone(self.check.report())
            child.assert_not_called()
        self.check.start("de")
        job = self.done()
        self.assertEqual(job["state"], "passed")
        self.assertEqual(len(job["steps"]), 6)
        self.assertEqual(job["components"], BUILD)
        self.assertNotIn("id", self.check.report())
        self.launcher.runtime = None
        self.assertIsNone(self.check.report())

    def test_account_error_is_distinct_and_local_window_still_checked(self):
        self.helper("xodus-service", "print('{\"stage\":\"account\",\"state\":\"failed\",\"code\":\"expired\"}',flush=True)\n")
        self.check.start()
        job = self.done()
        self.assertEqual(job["state"], "failed")
        self.assertEqual(job["steps"][1]["code"], "expired")
        self.assertEqual(job["steps"][-1]["state"], "passed")
        self.assertTrue(all(r["state"] == "failed" for r in job["steps"][2:5]))

    def test_unknown_stdout_never_reaches_report_or_becomes_success(self):
        self.helper("xodus-service", "print('{\"stage\":\"account\",\"state\":\"passed\",\"code\":\"SECRET_TOKEN\"}',flush=True)\n")
        self.check.start()
        job = self.done()
        self.assertEqual(job["state"], "failed")
        self.assertNotIn("SECRET", json.dumps(self.check.report()))
        self.assertEqual(job["steps"][1]["code"], "error")

    def test_cancel_reaps_helper_and_releases_reservation(self):
        self.helper("xodus-service", "import os,time\nopen('private/check-ready','w').write(str(os.getpid()))\ntime.sleep(30)\n")
        job = self.check.start()["job"]
        ready = self.runtime / "private/check-ready"
        end = time.monotonic()+3
        while not ready.exists() and time.monotonic()<end:
            time.sleep(.01)
        self.assertTrue(ready.exists())
        self.assertTrue(self.launcher.setup_busy)
        with self.assertRaises(LauncherError):
            self.check.start()
        with self.assertRaises(LauncherError):
            self.launcher.configure(str(self.runtime))
        self.assertEqual(self.launcher.status()["game"]["state"], "stopped")
        self.check.cancel(job["id"])
        result = self.done()
        self.assertEqual(result["state"], "cancelled")
        with self.assertRaises(ProcessLookupError):
            os.kill(int(ready.read_text()), 0)
        self.assertEqual(result["steps"][-1]["state"], "cancelled")

    def test_missing_runtime_stops_before_any_account_check(self):
        with patch.object(store_check, "verify_runtime", side_effect=ValueError()), patch.object(store_check.subprocess, "Popen") as child:
            self.check.start()
            job = self.done()
            child.assert_not_called()
        self.assertEqual(job["steps"][0]["code"], "runtime_update")
        self.assertEqual(job["state"], "failed")

    def test_terminal_result_is_not_replaced_and_private_extra_fields_rejected(self):
        self.check.job = {"steps":[{"stage":"account","state":"running","code":"checking"}]}
        self.assertFalse(self.check._event('{"stage":"account","state":"passed","code":"verified","token":"private"}', {"account"}))
        self.assertTrue(self.check._event('{"stage":"account","state":"failed","code":"expired"}', {"account"}))
        self.assertFalse(self.check._event('{"stage":"account","state":"passed","code":"verified"}', {"account"}))
        self.assertEqual(self.check.job["steps"][0]["code"], "expired")

    def test_recovery_runs_real_cli_contract_then_rechecks_without_a_test_window(self):
        self.helper("xodus-cli", "import json,os,sys\nopen('login-call','w').write(json.dumps({'args':sys.argv[1:],'cwd':os.getcwd()}))\n")
        self.check.start(recover=True)
        job = self.done()
        self.assertEqual(job["state"], "passed")
        call = json.loads((self.runtime / "private/login-call").read_text())
        self.assertEqual(call["args"], ["login", "--same-account"])
        self.assertEqual(call["cwd"], str(self.runtime / "private"))
        self.assertEqual([r["stage"] for r in job["steps"]], ["runtime", "sign_in", "account", "catalog", "license", "library"])

    def test_unsuccessful_login_never_checks_or_resumes_cloud(self):
        self.helper("xodus-cli", "raise SystemExit(1)\n")
        with patch.object(self.check, "_process") as process, patch.object(self.launcher.cloud_saves.automation, "action") as resume:
            self.check.start(recover=True)
            job = self.done()
            process.assert_not_called()
            resume.assert_not_called()
        self.assertEqual(job["state"], "failed")
        self.assertEqual(job["steps"][1]["code"], "sign_in_required")

    def test_login_diagnostic_code_survives_store_report_and_translation(self):
        from flightdeck.i18n import localize
        for code, hint in [(74, "response"), (83, "XML"), (85, "decrypted"), (91, "root sign-in token")]:
            with self.subTest(code=code):
                self.helper("xodus-cli", "import sys\nprint('SYNTHETIC_PRIVATE_TOKEN',file=sys.stderr)\nraise SystemExit(" + str(code) + ")\n")
                with patch.object(self.check, "_process") as process:
                    self.check.start(recover=True)
                    self.done()
                    process.assert_not_called()
                report = localize(self.check.report(), "en")
                row = report["steps"][1]
                self.assertEqual(row["exit_code"], code)
                self.assertIn(str(code), row["message"])
                self.assertIn(hint, row["message"])
                self.assertNotIn("SYNTHETIC_PRIVATE", json.dumps(report))

    def test_successful_login_requires_online_verification_before_cloud_resume(self):
        cloud = self.launcher.cloud_saves.automation
        for fails in (True, False):
            if fails:
                self.helper("xodus-service", "print('{\"stage\":\"account\",\"state\":\"failed\",\"code\":\"connection\"}',flush=True)\n")
            else:
                self.helper("xodus-service", "import json\nfor s,c in [('account','local_session'),('catalog','available'),('license','verified'),('library','available')]: print(json.dumps({'stage':s,'state':'passed','code':c}),flush=True)\n")
            with patch.object(cloud, "snapshot", return_value={"can_sign_in":True,"request_id":"captured"}), patch.object(cloud, "action") as resume:
                self.check.start(recover=True, cloud_request_id="captured")
                job = self.done()
                if fails:
                    resume.assert_not_called()
                    self.assertEqual(job["state"], "failed")
                else:
                    resume.assert_called_once_with("retry", "captured")
                    self.assertEqual(job["state"], "passed")

    def test_recovery_rejects_stale_job_cloud_and_running_game_before_login(self):
        with patch.object(store_check, "run_cli") as login:
            with self.assertRaises(LauncherError): self.check.start(recover=True, job_id="stale")
            with patch.object(self.launcher.cloud_saves.automation, "snapshot", return_value={"can_sign_in":False,"request_id":"new"}):
                with self.assertRaises(LauncherError): self.check.start(recover=True, cloud_request_id="old")
            with patch.object(self.launcher, "reserve_setup", side_effect=LauncherError("running")):
                with self.assertRaises(LauncherError): self.check.start(recover=True)
            login.assert_not_called()

    def test_cancelling_login_reaps_it_and_keeps_cloud_attention(self):
        self.helper("xodus-cli", "import os,time\nopen('login-ready','w').write(str(os.getpid()))\ntime.sleep(30)\n")
        cloud = self.launcher.cloud_saves.automation
        with patch.object(cloud, "snapshot", return_value={"can_sign_in":True,"request_id":"captured"}), patch.object(cloud, "action") as resume:
            job = self.check.start(recover=True, cloud_request_id="captured")["job"]
            ready = self.runtime / "private/login-ready"
            end = time.monotonic()+3
            while not ready.exists() and time.monotonic()<end: time.sleep(.01)
            self.assertTrue(ready.exists())
            self.check.cancel(job["id"])
            self.assertEqual(self.done()["state"], "cancelled")
            resume.assert_not_called()
            with self.assertRaises(ProcessLookupError): os.kill(int(ready.read_text()), 0)


class StoreDiagnosticsTests(unittest.TestCase):
    def test_bounded_log_no_overlap_and_gap_is_marked(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"log"
            p.write_bytes(b"one\ntwo\nthree\n")
            text, clipped, _ = store_diagnostics.bounded_log(p, 8, 8)
            self.assertEqual(text, "one\ntwo\nthree\n")
            self.assertFalse(clipped)
            p.write_bytes(b"one\ntwo\n"+b"hidden\n"*10+b"three\nfour\n")
            text, clipped, _ = store_diagnostics.bounded_log(p, 8, 12)
            self.assertTrue(clipped)
            self.assertNotIn("hidden", text)

    def test_sequence_components_and_safe_export(self):
        with tempfile.TemporaryDirectory() as d:
            run=Path(d)
            (run/"game.log").write_text("[xodus-store-query] kind=9 hr=00000000 time_ms=1790540000020\n[xodus-store-query] kind=10 hr=80004001 time_ms=1790540000040\nSECRET_TOKEN=private\n")
            (run/"service.log").write_text("[flightdeck-store-build] "+json.dumps(BUILD)+"\n[flightdeck-store-event] time_ms=1790540000010 seq=0 phase=window_ready outcome=passed\n[flightdeck-store-event] time_ms=1790540000030 seq=1 phase=load_timeout outcome=timeout\n[flightdeck-store-event] time_ms=1790540000050 seq=2 phase=private outcome=secret\n")
            value=store_diagnostics.session(run)
            self.assertEqual(value["components_at_launch"], BUILD)
            self.assertEqual([r["time_ms"] for r in value["events"]], [1790540000010,1790540000020,1790540000030,1790540000040])
            self.assertNotIn("SECRET", json.dumps(value))
            self.assertNotIn("private", json.dumps(value))
            self.assertEqual(len(value["events"]),4)
            (run/"service.log").write_text("[flightdeck-store-build] "+json.dumps({**BUILD,"token":"secret"}))
            self.assertIsNone(store_diagnostics.session(run)["components_at_launch"])

    def test_actual_native_hashes_and_missing_file(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            for path in store_diagnostics.COMPONENTS.values():
                p=root/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b"native fixture")
            self.assertIsNotNone(store_diagnostics.launch_record(root))
            (root/"bin/xodus-cli").unlink()
            self.assertIsNone(store_diagnostics.launch_record(root))

    def test_bootstrap_failure_is_preserved_without_remote_details(self):
        with tempfile.TemporaryDirectory() as d:
            run=Path(d)
            (run/"service.log").write_text("[flightdeck-store-event] time_ms=1790540000010 seq=0 phase=bootstrap_started outcome=started\n[flightdeck-store-event] time_ms=1790540000020 seq=1 phase=bootstrap_error outcome=failed\nprivate token and URL must not be exported\n")
            events=store_diagnostics.session(run)["events"]
            self.assertEqual([(e["phase"],e["outcome"]) for e in events],[("bootstrap_started","started"),("bootstrap_error","failed")])
            self.assertNotIn("private",json.dumps(events))
