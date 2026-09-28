# SPDX-License-Identifier: MIT
"""Synthetic session failures; no game, account, device or network access."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import log_reader, run_diagnostics, store_diagnostics


class RunDiagnosticsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.run = Path(self.temp.name)
        self.path = self.run / "game.log"

    def read(self, text):
        self.path.write_text(text)
        return run_diagnostics.read(self.path)[0]

    def test_successful_store_does_not_hide_later_token_failure(self):
        report = self.read("[xodus-store-query] kind=1 hr=00000000 time_ms=1790623215379\n" +
            "unrelated padding\n" * 70000 +
            "xodus-user-policy-cache: stage=title_fetch hr=80072ee2\n"
            "xodus-signature-policy: call=4 host=private-title.playfabapi.com matched=0 has_policy=0 index=0 version=0 supported=0 token_only=0 hr=80004001\n"
            "xodus-user-api: signature.policy call=0 hr=80004001\n"
            "xodus-user-api: XUserGetTokenAndSignatureAsync.complete call=0 hr=80004001\n" +
            "unrelated padding\n" * 40000 +
            "xodus-wine-launch: wine_pid=123 exit_code=0 elapsed_seconds=214.242\n")
        self.assertEqual(report["store_calls"], [{"method": "XStoreQueryEntitledProductsAsync", "hresult": "00000000"}])
        self.assertEqual(report["user_calls"], [
            {"method": "XUserGetTokenAndSignatureAsync.complete", "hresult": "80004001"},
            {"method": "signature.policy", "hresult": "80004001"}])
        self.assertEqual(report["policy_cache"], [{"stage": "title_fetch", "hresult": "80072ee2"}])
        self.assertEqual(report["signature_policy"][0]["host_category"], "playfab")
        self.assertTrue(report["log_coverage"]["complete"])
        self.assertNotIn("private-title", json.dumps(report))

    def test_middle_store_event_is_retained_with_consistent_coverage(self):
        self.read("padding\n" * 150000 +
                  "[xodus-store-query] kind=8 hr=80004004 time_ms=1790623216608\n" + "padding\n" * 90000)
        (self.run / "service.log").write_text("")
        session = store_diagnostics.session(self.run)
        self.assertEqual(session["events"][0]["hresult"], "80004004")
        self.assertFalse(session["partial"])
        self.assertTrue(session["coverage"]["game"]["complete"])

    def test_audio_and_numeric_renderer_errors_are_visible_without_raw_text(self):
        report = self.read(
            "00ac:err:mmdevapi:init_driver:No driver from L\"private-path\" could be initialized.\n"
            "00ac:warn:pulse:pulse_contextcallback:Context failed: private-server\n"
            "00ac:warn:mmdevapi:get_mmdevice_by_activatepath:Failed to get requested device (private-device): 80070490\n"
            "00ac:err:xaudio2:private_function:private-account\n"
            "123:00ac:err:vkd3d-proton:vkd3d_create_device:Failed, vr -4. private-token\n"
            "xodus-user-api: private-token call=0 hr=80004001\n"
            "xodus-user-policy-cache: stage=private-token hr=80004001\n"
            "Authorization: Bearer private-token\n")
        self.assertEqual(report["audio"]["error_counts"], {"mmdevapi": 1, "xaudio2": 1})
        self.assertEqual(report["audio"]["observations"], {"no_audio_driver": 1, "pulse_context_failed": 1, "audio_device_unavailable": 1})
        self.assertEqual(report["graphics_log"]["error_counts"], {"vkd3d-proton": 1})
        self.assertEqual(report["user_calls"], [])
        self.assertNotIn("private", json.dumps(report))

    def test_no_audio_error_is_not_a_claim_of_working_audio(self):
        report = self.read("[xodus-store-query] kind=0 hr=00000000\n")
        self.assertNotIn("status", report["audio"])
        self.assertEqual(report["audio"]["error_counts"], {})

    def test_signal_exit_is_not_silently_dropped(self):
        report = self.read("xodus-wine-launch: wine_pid=123 signal=11 shell_exit_code=139 elapsed_seconds=2.250\n")
        self.assertEqual(report["exit"], {"signal": 11, "code": 139, "seconds": 2.25})

    def test_split_lines_are_parsed_once_and_cap_never_joins_fragments(self):
        self.path.write_bytes(b"first\nsecond\nthird\nlast\n")
        parts = []
        with patch.object(log_reader, "BLOCK", 3):
            coverage, _ = log_reader.scan(self.path, parts.append)
        self.assertEqual("".join(parts), "first\nsecond\nthird\nlast\n")
        self.assertTrue(coverage["complete"])
        parts = []
        coverage, _ = log_reader.scan(self.path, parts.append, maximum=8, tail=8)
        self.assertEqual("".join(parts), "first\nlast\n")
        self.assertFalse(coverage["complete"])
        self.assertEqual(coverage["bytes_read"], 16)

    def test_adjacent_head_and_tail_do_not_omit_or_duplicate_lines(self):
        self.path.write_bytes(b"first\nsecond\nlast")
        parts = []
        coverage, _ = log_reader.scan(self.path, parts.append, maximum=8, tail=12)
        self.assertEqual("".join(parts), "first\nsecond\nlast\n")
        self.assertTrue(coverage["complete"])

    def test_deadline_still_reads_shutdown_but_marks_gap(self):
        self.path.write_text("hidden\n" * 30 + "end\n")
        parts = []
        coverage, _ = log_reader.scan(self.path, parts.append, seconds=0, tail=8)
        self.assertEqual("".join(parts), "end\n")
        self.assertEqual(coverage["omitted_bytes"], self.path.stat().st_size - 8)

    def test_long_lines_cannot_forge_events_from_their_tail(self):
        self.path.write_bytes(b"x" * (log_reader.MAX_LINE + 1) + b"[xodus-store-query] kind=0 hr=80004001\nOK\n")
        parts = []
        with patch.object(log_reader, "BLOCK", 1024):
            coverage, _ = log_reader.scan(self.path, parts.append)
        self.assertEqual(coverage["oversized_lines"], 1)
        self.assertFalse(coverage["complete"])
        self.assertNotIn("xodus", "".join(parts))

    def test_changed_file_never_reports_complete(self):
        self.path.write_text("first\n")
        def consume(_):
            with self.path.open("a") as stream:
                stream.write("new\n")
        coverage, _ = log_reader.scan(self.path, consume)
        self.assertTrue(coverage["changed_during_read"])
        self.assertFalse(coverage["complete"])

    def test_nonregular_files_are_rejected(self):
        os.mkfifo(self.path)
        with self.assertRaises(OSError):
            run_diagnostics.read(self.path)
        self.path.unlink()
        (self.run / "secret").write_text("private")
        self.path.symlink_to(self.run / "secret")
        with self.assertRaises(OSError):
            run_diagnostics.read(self.path)

    def test_unique_outcome_flood_is_bounded_and_marked(self):
        report = self.read("".join(f"xodus-user-api: signature.policy call=0 hr={n:08x}\n" for n in range(300)))
        self.assertEqual(len(report["user_calls"]), 256)
        self.assertTrue(report["summary_limited"])

    def test_network_policy_keeps_failure_but_discards_private_host_and_unknown_policy(self):
        report = self.read(
            "[xodus-network] security scheme=https host=private.xboxlive.com policy=fetch-failed result=80072ee2\n"
            "[xodus-network] security scheme=https host=xboxlive.com.private policy=unmatched result=80070490\n"
            "[xodus-network] security scheme=https host=private policy=private result=80004001\n")
        self.assertEqual(report["network_security"], [
            {"host_category": "xboxlive", "policy": "fetch-failed", "hresult": "80072ee2"},
            {"host_category": "other", "policy": "unmatched", "hresult": "80070490"}])
        self.assertNotIn("private", json.dumps(report))

    def test_offline_export_reads_existing_run_and_never_overwrites_output(self):
        self.path.write_text("xodus-user-api: signature.policy call=0 hr=80004001\nprivate-token\n")
        before = self.path.read_bytes()
        output = self.run / "report.json"
        command = [sys.executable, "scripts/diagnose-run.py", "--run", str(self.run), "--output", str(output)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(output.read_text())
        self.assertEqual(report["summary"]["context"]["cloud_sync_scope"], "not_collected")
        self.assertEqual(report["summary"]["user_calls"][0]["hresult"], "80004001")
        self.assertNotIn("private-token", output.read_text())
        self.assertEqual(output.stat().st_mode & 0o777, 0o600)
        output.write_text("keep existing report")
        self.assertEqual(subprocess.run(command, capture_output=True, timeout=5).returncode, 1)
        self.assertEqual(output.read_text(), "keep existing report")
        self.assertEqual(self.path.read_bytes(), before)
