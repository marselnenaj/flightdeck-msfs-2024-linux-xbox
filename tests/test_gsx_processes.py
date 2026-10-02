# SPDX-License-Identifier: MIT
import json
import os
import signal
import sys
import unittest

import test_fenix_processes as fixtures
from flightdeck import gsx_processes
from flightdeck.backend import LauncherError


class GSXProcessTests(unittest.TestCase):
    setUp = fixtures.FenixProcessTests.setUp
    close_children = fixtures.FenixProcessTests.close_children
    process = fixtures.FenixProcessTests.process

    def test_stop_is_scoped_to_fsdts_managers_and_selected_prefix(self):
        manager, closed = self.process(r'C:\Program Files (x86)\Addon Manager\Couatl_Updater2.exe')
        survivors = [self.process('Couatl_Updater2.exe', other=True)[0], self.process('Fenix.exe')[0],
                     self.process('OtherAircraft.exe')[0], self.process('Couatl64_MSFS2024.exe')[0]]
        self.assertEqual(gsx_processes.status(self.root), (True, False))
        wine = self.root / 'runner/files/bin/wine'
        wine.parent.mkdir(parents=True)
        arguments = self.root / 'taskkill-arguments'
        wine.write_text('#!' + sys.executable + '\nimport os, signal, sys, json\nfrom pathlib import Path\n'
                        + f'Path({str(arguments)!r}).write_text(json.dumps(sys.argv[1:]))\n'
                        + f'os.kill({manager.pid},signal.SIGUSR1)\n')
        wine.chmod(0o700)
        gsx_processes.stop(self.root)
        self.assertEqual(manager.wait(timeout=2), 0)
        self.assertEqual(json.loads(arguments.read_text()), ['taskkill.exe', '/IM', 'couatl_updater2.exe'])
        self.assertTrue(all(child.poll() is None for child in survivors))

    def test_fsdts_manager_cannot_be_stopped_during_a_flight(self):
        manager, _ = self.process('Couatl_Updater2.exe')
        game, _ = self.process('FlightSimulator2024.exe')
        with self.assertRaises(LauncherError):
            gsx_processes.stop(self.root)
        self.assertIsNone(manager.poll())
        self.assertIsNone(game.poll())
