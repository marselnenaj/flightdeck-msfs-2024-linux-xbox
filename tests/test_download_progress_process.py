# SPDX-License-Identifier: MIT
"""Real synthetic children only; no game, account or network operations."""
import errno
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck import download_progress, game_install


FRAME = {"format": 1, "received_bytes": 40, "verified_bytes": 20,
         "total_bytes": 100, "completed_files": 1, "total_files": 3}
FLOOD = """
import os, sys
fd = int(sys.argv[sys.argv.index('--progress-fd') + 1])
line = (sys.argv[1] + '\\n').encode()
print('synthetic-private-stdout', flush=True)
print('synthetic-private-stderr', file=sys.stderr, flush=True)
while True:
    os.write(fd, line * 32)
"""


class DownloadProgressProcessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.pipes = []

    def capture_pipe(self):
        original = download_progress.ProgressPipe

        def create(callback):
            pipe = original(callback)
            self.pipes.append((pipe, pipe.read_fd, pipe.write_fd))
            return pipe

        return patch.object(download_progress, "ProgressPipe", side_effect=create)

    def assert_pipes_closed(self):
        self.assertEqual(len(self.pipes), 1)
        for pipe, read_fd, write_fd in self.pipes:
            self.assertIsNone(pipe.read_fd)
            self.assertIsNone(pipe.write_fd)
            for fd in (read_fd, write_fd):
                with self.assertRaises(OSError) as raised:
                    os.fstat(fd)
                self.assertEqual(raised.exception.errno, errno.EBADF)

    def test_spawn_failure_closes_both_progress_descriptors(self):
        seen = []
        with self.capture_pipe():
            with self.assertRaises(FileNotFoundError):
                game_install.run_cli(Path(self.temp.name) / "absent-synthetic-cli", [],
                                     cwd=self.temp.name, cancel=None, transfer=seen.append)
        self.assert_pipes_closed()
        self.assertEqual(seen, [])

    def flood_then_interrupt(self, operation):
        cancel = threading.Event()
        ready = threading.Event()
        control = game_install.DownloadControl()
        control.downloading(True)
        seen, outcome, owned = [], [], []
        original_popen = subprocess.Popen
        unrelated = original_popen([sys.executable, "-B", "-c", "import time; time.sleep(30)"],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True)

        def spawn(*args, **kwargs):
            process = original_popen(*args, **kwargs)
            owned.append(process)
            return process

        def observe(value):
            seen.append(value)
            if value is not None:
                ready.set()

        def run():
            try:
                outcome.append(game_install.run_cli(
                    sys.executable, ["-B", "-c", FLOOD, json.dumps(FRAME)],
                    cwd=self.temp.name, cancel=cancel, control=control, transfer=observe))
            except Exception as error:
                outcome.append(error)

        worker = threading.Thread(target=run, daemon=True)
        try:
            with self.capture_pipe(), patch.object(game_install.subprocess, "Popen", side_effect=spawn):
                worker.start()
                self.assertTrue(ready.wait(7), "Numeric progress must drain while the child floods its pipe")
                if operation == "cancel":
                    cancel.set()
                else:
                    control.pause()
                worker.join(timeout=7)
                self.assertFalse(worker.is_alive(), "Progress flooding must not prevent interruption")
                self.assertEqual(len(outcome), 1)
                expected = game_install.GameInstallCancelled if operation == "cancel" else game_install._PauseDownload
                self.assertIsInstance(outcome[0], expected)
                self.assertEqual(len(owned), 1)
                self.assertIsNotNone(owned[0].poll(), "The owned child must be reaped")
                self.assertIsNone(unrelated.poll(), "An unrelated child must not be signalled")
                self.assert_pipes_closed()
                expected_frame = {"kind": "game", **{key: value for key, value in FRAME.items() if key != "format"}}
                self.assertTrue(seen)
                self.assertTrue(all(value == expected_frame for value in seen))
                self.assertNotIn("synthetic-private", repr(seen))
        finally:
            cancel.set()
            for process in owned:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=7)
            if worker.ident is not None:
                worker.join(timeout=7)
            if unrelated.poll() is None:
                unrelated.terminate()
            unrelated.wait(timeout=7)

    def test_numeric_flood_cannot_block_cancel_or_stop_an_unrelated_process(self):
        self.flood_then_interrupt("cancel")

    def test_numeric_flood_cannot_block_pause_or_stop_an_unrelated_process(self):
        self.flood_then_interrupt("pause")

    def test_legacy_child_output_is_still_discarded_without_a_progress_descriptor(self):
        # The child confirms the actual inherited streams, not just Popen kwargs.
        script = """
import json, os, pathlib, sys
assert '--progress-fd' not in sys.argv
null = os.stat('/dev/null')
assert all(os.fstat(fd).st_rdev == null.st_rdev for fd in (0, 1, 2))
print('synthetic-private-stdout', flush=True)
print('synthetic-private-stderr', file=sys.stderr, flush=True)
pathlib.Path(sys.argv[1]).write_text(json.dumps({'discarded': True}))
"""
        result_path = Path(self.temp.name) / "streams.json"
        with patch.object(download_progress, "ProgressPipe") as pipe:
            result = game_install.run_cli(sys.executable, ["-B", "-c", script, str(result_path)],
                                          cwd=self.temp.name, cancel=None)
        self.assertEqual(result, 0)
        self.assertEqual(json.loads(result_path.read_text()), {"discarded": True})
        pipe.assert_not_called()


if __name__ == "__main__":
    unittest.main()
