# SPDX-License-Identifier: MIT
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

from flightdeck import fenix_processes
from flightdeck.backend import LauncherError


class FenixProcessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "runtime with spaces"
        self.children = []
        self.addCleanup(self.close_children)

    def close_children(self):
        for child in self.children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=3)

    def process(self, name, *, other=False, stubborn=False):
        ready = Path(self.temp.name) / f"ready-{len(self.children)}"
        closed = ready.with_name(ready.name + "-closed")
        code = """
import os, signal, sys, time
from pathlib import Path
def stop(number, frame):
    Path(sys.argv[2]).write_text(str(number)); raise SystemExit(0)
signal.signal(signal.SIGUSR1, stop)
signal.signal(signal.SIGTERM, signal.SIG_IGN if sys.argv[3]=='stubborn' else stop)
Path(sys.argv[1]).touch()
while True: time.sleep(.01)
"""
        prefix = self.root / ("other-prefix" if other else "local/msfs-prefix")
        child = subprocess.Popen([name, "-c", code, str(ready), str(closed), "stubborn" if stubborn else "normal"],
            executable=sys.executable, env={**os.environ, "WINEPREFIX": str(prefix)},
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        self.children.append(child)
        deadline = time.monotonic() + 3
        while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertTrue(ready.exists())
        return child, closed

    def test_graceful_stop_includes_manager_and_preserves_other_profiles_and_apps(self):
        fenix, closed = self.process(r"C:\Program Files\FenixSim A320\Fenix.exe")
        manager, manager_closed = self.process("FenixApp.exe")
        survivors = [self.process("Fenix.exe", other=True)[0], self.process("OtherAircraft.exe")[0]]
        self.assertEqual(fenix_processes.status(self.root), (True, False))
        wine = self.root / "runner/files/bin/wine"
        wine.parent.mkdir(parents=True)
        arguments = self.root / "taskkill-arguments"
        wine.write_text("#!" + sys.executable + "\nimport json, os, signal, sys\nfrom pathlib import Path\n"
            + f"Path({str(arguments)!r}).write_text(json.dumps(sys.argv[1:]))\n"
            + f"os.kill({fenix.pid}, signal.SIGUSR1)\nos.kill({manager.pid}, signal.SIGUSR1)\n")
        wine.chmod(0o700)
        fenix_processes.stop(self.root)
        self.assertEqual(fenix.wait(timeout=2), 0)
        self.assertEqual(manager.wait(timeout=2), 0)
        self.assertEqual(int(closed.read_text()), signal.SIGUSR1)
        self.assertEqual(int(manager_closed.read_text()), signal.SIGUSR1)
        self.assertEqual(json.loads(arguments.read_text()), ["taskkill.exe", "/IM", "fenix.exe", "/IM", "fenixapp.exe"])
        self.assertTrue(all(child.poll() is None for child in survivors))
        self.assertEqual(fenix_processes.status(self.root), (False, False))

    def test_running_game_blocks_all_stop_signals(self):
        fenix, closed = self.process("Fenix.exe")
        game, _ = self.process("FlightSimulator2024.exe")
        self.assertEqual(fenix_processes.status(self.root), (True, True))
        with self.assertRaisesRegex(LauncherError, "MSFS"):
            fenix_processes.stop(self.root)
        self.assertIsNone(fenix.poll())
        self.assertIsNone(game.poll())
        self.assertFalse(closed.exists())

    def test_stuck_helper_is_killed_without_waiting_indefinitely(self):
        child, closed = self.process("FenixDisplay.exe", stubborn=True)
        start = time.monotonic()
        fenix_processes.stop(self.root)
        self.assertEqual(child.wait(timeout=2), -signal.SIGKILL)
        self.assertFalse(closed.exists())
        self.assertLess(time.monotonic() - start, 8)


if __name__ == "__main__": unittest.main()
