"""Real synthetic child processes, never a Microsoft login or game download."""
# SPDX-License-Identifier: MIT
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from flightdeck import game_install
from flightdeck.i18n import error_message, translate_message


class GameDownloadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.destination = self.root / "game"
        self.cli = self.root / "synthetic-xodus"
        self.observed = self.root / "arguments.jsonl"
        self.cli.write_text('#!' + sys.executable + '\n' + '''
import json, os, pathlib, sys, time
root = pathlib.Path(__file__).parent
with (root / 'arguments.jsonl').open('a') as out:
    out.write(json.dumps(sys.argv[1:]) + '\\n')
print('synthetic-sensitive-output-must-be-discarded', flush=True)
print('https://invalid.example/signed?secret=synthetic', file=sys.stderr, flush=True)
settings = json.loads((root / 'settings.json').read_text())
if '--progress-fd' in sys.argv:
    fd = int(sys.argv[sys.argv.index('--progress-fd') + 1])
    for value in settings.get('progress_frames', []):
        os.write(fd, (json.dumps(value) + '\\n').encode())
if settings.get('block') == sys.argv[1]:
    (root / 'blocked').write_text(str(os.getpid()))
    time.sleep(30)
if sys.argv[1] == 'login':
    sys.exit(settings.get('login_exit', 0))
if settings.get('auth_needed') and (settings.get('auth_always') or not (root / 'auth-requested').exists()):
    (root / 'auth-requested').write_text('1')
    sys.exit(77)
if settings.get('complete', True):
    target = pathlib.Path(sys.argv[3])
    (target / 'FlightSimulator2024.exe').write_bytes(b'synthetic encrypted executable')
    (target / '.xodus-streaming.msixvc').write_bytes(b'synthetic marker')
    (target / 'MicrosoftGame.Config').write_text('<Game><ExecutableList><Executable Name="FlightSimulator2024.exe"/></ExecutableList></Game>')
sys.exit(settings.get('download_exit', 0))
''')
        self.cli.chmod(0o700)
        self.checksum = hashlib.sha256(self.cli.read_bytes()).hexdigest()
        self.settings()

    def settings(self, **values):
        (self.root / "settings.json").write_text(json.dumps(values))

    def tearDown(self):
        self.temp.cleanup()

    def invoke(self, **kwargs):
        return game_install.download_game(self.cli, self.checksum, self.destination, "AT", **kwargs)

    def arguments(self):
        return [json.loads(row) for row in self.observed.read_text().splitlines()] if self.observed.exists() else []

    def test_real_fixed_commands_and_validated_completion(self):
        progress = []
        actual = self.invoke(notify=lambda *row: progress.append(row))
        self.assertEqual(actual, self.destination)
        self.assertEqual(self.arguments(), [["login"], ["streaming", "9P38D19T7LRV", str(self.destination), "--market", "AT", "--parallel", "4"]])
        self.assertEqual([row[0] for row in progress], ["authentication", "download"])
        self.assertTrue(all(row[2] is None for row in progress))
        self.assertEqual(self.destination.stat().st_mode & 0o777, 0o700)
        self.assertEqual((self.destination / "FlightSimulator2024.exe").stat().st_mode & 0o077, 0)
        self.assertNotIn("sensitive", repr(progress))
        self.assertNotIn("invalid.example", repr(progress))

    def test_declared_integrity_feature_requires_complete_sealed_record(self):
        with self.assertRaisesRegex(game_install.GameInstallError, "Prüfnachweis"):
            self.invoke(cli_features=[game_install.RESUME_FEATURE,"streaming-integrity-index-v1"])

    def test_login_failure_never_starts_download(self):
        self.settings(login_exit=3)
        with self.assertRaises(game_install.GameInstallError) as result:
            self.invoke()
        english = translate_message(error_message(result.exception), "en")
        self.assertIn("sign-in", english)
        self.assertIn("3", english)
        self.assertEqual(self.arguments(), [["login"]])
        self.assertNotIn("sensitive", str(result.exception))

    def test_update_uses_exact_revision_without_implicit_initial_login(self):
        self.invoke(sign_in=False, expected_package="a" * 64,
                    cli_features=[game_install.RESUME_FEATURE, "package-info-json-v1"])
        commands = self.arguments()
        self.assertEqual(len(commands), 1)
        self.assertEqual(commands[0][-3:], ["--resume-files", "--expect-package", "a" * 64])

    def test_changed_package_revision_is_a_recheck_error(self):
        self.settings(complete=False, download_exit=78)
        with self.assertRaisesRegex(game_install.GameInstallError, "Paketrevision"):
            self.invoke(sign_in=False, expected_package="a" * 64,
                        cli_features=[game_install.RESUME_FEATURE, "package-info-json-v1"])
        self.assertEqual(len(self.arguments()), 1)
        self.assertFalse((self.destination / '.xodus-streaming.msixvc').exists())
        self.assertFalse((self.destination / ".xodus-streaming.msixvc").exists())

    def test_zero_exit_without_final_files_is_not_success(self):
        self.settings(complete=False)
        with self.assertRaisesRegex(game_install.GameInstallError, "nicht vollständig"):
            self.invoke()
        self.assertEqual(len(self.arguments()), 2)

    def test_nonzero_download_is_not_success_even_with_files(self):
        self.settings(download_exit=7)
        with self.assertRaisesRegex(game_install.GameInstallError, "Code 7"):
            self.invoke()
        self.assertTrue(self.destination.exists())

    def test_checksum_or_existing_destination_prevents_all_execution(self):
        with self.assertRaises(game_install.GameInstallError):
            game_install.download_game(self.cli, "0" * 64, self.destination, "AT")
        self.assertFalse(self.destination.exists())
        self.destination.mkdir()
        (self.destination / "keep").write_text("existing game")
        with self.assertRaises(game_install.GameInstallError):
            self.invoke()
        self.assertEqual((self.destination / "keep").read_text(), "existing game")
        self.assertEqual(self.arguments(), [])

    def test_cancel_stops_only_own_login_or_download_process(self):
        for phase in ("login", "streaming"):
            with self.subTest(phase=phase):
                self.destination = self.root / ("game-" + phase)
                (self.root / "blocked").unlink(missing_ok=True)
                self.settings(block=phase)
                cancel = threading.Event()
                outcome = []
                unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"], start_new_session=True)
                def run():
                    try:
                        self.invoke(cancel=cancel)
                    except game_install.GameInstallCancelled:
                        outcome.append("cancelled")
                thread = threading.Thread(target=run)
                try:
                    thread.start()
                    deadline = time.monotonic() + 3
                    while not (self.root / "blocked").exists() and time.monotonic() < deadline:
                        time.sleep(.01)
                    self.assertTrue((self.root / "blocked").exists())
                    pid = int((self.root / "blocked").read_text())
                    cancel.set()
                    thread.join(timeout=7)
                    self.assertFalse(thread.is_alive())
                    self.assertEqual(outcome, ["cancelled"])
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)
                    self.assertIsNone(unrelated.poll())
                finally:
                    cancel.set()
                    thread.join(timeout=7)
                    unrelated.terminate()
                    unrelated.wait(timeout=3)

    def test_cancel_before_start_creates_nothing(self):
        cancel = threading.Event()
        cancel.set()
        with self.assertRaises(game_install.GameInstallCancelled):
            self.invoke(cancel=cancel)
        self.assertFalse(self.destination.exists())
        self.assertEqual(self.arguments(), [])

    def test_child_output_is_not_piped_or_logged(self):
        real = subprocess.Popen
        with patch.object(game_install.subprocess, "Popen", wraps=real) as call:
            self.invoke()
        for invoked in call.call_args_list:
            self.assertEqual(invoked.kwargs["stdout"], subprocess.DEVNULL)
            self.assertEqual(invoked.kwargs["stderr"], subprocess.DEVNULL)
            self.assertEqual(invoked.kwargs["stdin"], subprocess.DEVNULL)
            self.assertNotIn("shell", invoked.kwargs)
            self.assertTrue(invoked.kwargs["start_new_session"])
            self.assertEqual(invoked.kwargs["env"]["XODUS_LOG"], "off")
            self.assertEqual(invoked.kwargs["env"]["XDG_CONFIG_HOME"], str(self.root / "xdg/config"))

    def test_progress_uses_only_declared_dedicated_pipe_not_cli_logs(self):
        frame = {"format": 1, "received_bytes": 40, "verified_bytes": 20,
                 "total_bytes": 100, "completed_files": 1, "total_files": 3}
        self.settings(progress_frames=[frame])
        seen = []
        self.invoke(cli_features=[game_install.RESUME_FEATURE, game_install.PROGRESS_FEATURE], transfer=seen.append)
        self.assertEqual(self.arguments()[0], ['login'])
        self.assertIn('--progress-fd', self.arguments()[1])
        numeric = [value for value in seen if value is not None]
        self.assertEqual(numeric, [{"kind": "game", **{k:v for k,v in frame.items() if k != 'format'}}])
        self.assertIsNone(seen[-1])
        self.assertNotIn('sensitive', repr(seen))
        self.assertNotIn('signed', repr(seen))

    def test_legacy_cli_does_not_receive_progress_flag(self):
        seen = []
        self.invoke(transfer=seen.append)
        self.assertTrue(all('--progress-fd' not in command for command in self.arguments()))
        self.assertTrue(all(value is None for value in seen))

    def test_full_progress_does_not_make_incomplete_download_successful(self):
        self.settings(complete=False, progress_frames=[{
            "format": 1, "received_bytes": 100, "verified_bytes": 100,
            "total_bytes": 100, "completed_files": 1, "total_files": 1}])
        seen = []
        with self.assertRaises(game_install.GameInstallError):
            self.invoke(cli_features=[game_install.RESUME_FEATURE, game_install.PROGRESS_FEATURE], transfer=seen.append)
        self.assertTrue(any(value and value['verified_bytes'] == 100 for value in seen))
        self.assertIsNone(seen[-1])

    def test_invalid_completion_and_partial_marker_are_rejected(self):
        self.invoke()
        marker = self.destination / ".xodus-streaming-tmp.msixvc"
        marker.write_bytes(b"partial")
        with self.assertRaises(game_install.GameInstallError):
            game_install.validate_download(self.destination)

        marker.unlink()
        config = self.destination / "MicrosoftGame.Config"
        config.write_text("<NotGame/>")
        with self.assertRaises(game_install.GameInstallError):
            game_install.validate_download(self.destination)

    def download_worker(self, control, cancel):
        outcome = []
        def run():
            try:
                outcome.append(self.invoke(control=control, cancel=cancel, cli_features=[game_install.RESUME_FEATURE]))
            except Exception as error:
                outcome.append(error)
        thread = threading.Thread(target=run)
        thread.start()
        self.addCleanup(lambda: (cancel.set(), thread.join(timeout=7)))
        return thread, outcome

    def wait_until(self, predicate):
        deadline = time.monotonic() + 7
        while not predicate() and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertTrue(predicate())

    def test_pause_reaps_own_child_then_resume_restarts_same_folder(self):
        self.settings(block="streaming")
        control, cancel = game_install.DownloadControl(), threading.Event()
        thread, outcome = self.download_worker(control, cancel)
        self.wait_until(lambda: (self.root / "blocked").exists())
        pid = int((self.root / "blocked").read_text())
        control.pause()
        self.wait_until(lambda: control.state == "paused")
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        retained = self.destination / "synthetic-completed-file"
        retained.write_bytes(b"verified completed data")
        before = retained.stat().st_mtime_ns
        self.assertTrue(thread.is_alive())
        self.assertEqual(len(self.arguments()), 2)
        with self.assertRaises(game_install.GameInstallError):
            control.pause()
        self.settings()
        control.resume()
        thread.join(timeout=7)
        self.assertEqual(outcome, [self.destination])
        self.assertEqual(retained.stat().st_mtime_ns, before)
        self.assertEqual(self.arguments()[1], self.arguments()[2])
        self.assertEqual(self.arguments()[2][-1], "--resume-files")
        self.assertEqual(self.arguments().count(["login"]), 1)
        self.assertEqual(control.state, "unavailable")

    def test_cancel_while_paused_exits_without_restarting(self):
        self.settings(block="streaming")
        control, cancel = game_install.DownloadControl(), threading.Event()
        thread, outcome = self.download_worker(control, cancel)
        self.wait_until(lambda: (self.root / "blocked").exists())
        control.pause()
        self.wait_until(lambda: control.state == "paused")
        cancel.set()
        thread.join(timeout=7)
        self.assertFalse(thread.is_alive())
        self.assertIsInstance(outcome[0], game_install.GameInstallCancelled)
        self.assertEqual(len(self.arguments()), 2)
        self.assertEqual(control.state, "unavailable")

    def test_resume_rechecks_executable_hash(self):
        self.settings(block="streaming")
        control, cancel = game_install.DownloadControl(), threading.Event()
        thread, outcome = self.download_worker(control, cancel)
        self.wait_until(lambda: (self.root / "blocked").exists())
        control.pause()
        self.wait_until(lambda: control.state == "paused")
        self.cli.write_text(self.cli.read_text() + "\n# replaced\n")
        control.resume()
        thread.join(timeout=7)
        self.assertIsInstance(outcome[0], game_install.GameInstallError)
        self.assertIn("Prüfsumme", str(outcome[0]))
        self.assertEqual(len(self.arguments()), 2)

    def test_expired_auth_reuses_real_login_once_not_a_silent_retry_loop(self):
        for persistent in (False, True):
            with self.subTest(persistent=persistent):
                self.destination = self.root / ("auth-" + str(persistent))
                self.observed.unlink(missing_ok=True)
                (self.root / "auth-requested").unlink(missing_ok=True)
                self.settings(auth_needed=True, auth_always=persistent)
                events = []
                args = dict(cli_features=[game_install.RESUME_FEATURE], notify=lambda *row: events.append(row))
                if persistent:
                    with self.assertRaisesRegex(game_install.GameInstallError, "Code 77"):
                        self.invoke(**args)
                else:
                    self.assertEqual(self.invoke(**args), self.destination)
                self.assertEqual([row[0] for row in self.arguments()], ["login", "streaming", "login", "streaming"])
                self.assertEqual([row[0] for row in events], ["authentication", "download", "authentication", "download"])

    def test_legacy_cli_does_not_offer_pause_or_gain_flags(self):
        states = []
        control = game_install.DownloadControl(states.append)
        self.invoke(control=control)
        self.assertNotIn("download", states)
        self.assertNotIn("--resume-files", self.arguments()[1])
        with self.assertRaises(game_install.GameInstallError):
            control.pause()


if __name__ == "__main__":
    unittest.main()
