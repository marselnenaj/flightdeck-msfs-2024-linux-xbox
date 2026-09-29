"""Frozen, local-only support reports and their privacy boundary."""
# SPDX-License-Identifier: MIT
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from flightdeck.backend import LauncherError, atomic_json
from flightdeck.problem_reports import MAX_BYTES, ProblemReports, encoded


class ProblemReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.diagnostic = {
            'generated_at': '2026-09-29T12:00:00Z', 'csrf_token': 'PRIVATE-CSRF',
            'private_log': 'PRIVATE-TOKEN',
            'summary': {'context': {'launcher_version': '0.1.9', 'game_id': 'msfs2024',
                                    'cloud_sync_scope': 'current_service'},
                        'graphics': {'devices': [{'name': 'NVIDIA RTX 5060 Ti', 'driver_version': '595.91.07'}]},
                        'cloud_sync': {'state': 'attention', 'error_code': 'transport'},
                        'private': 'PRIVATE-PATH'},
            'checks': [{'id': 'game', 'ok': True, 'detail': '/synthetic/PRIVATE-USER/game'},
                       {'id': 'runner', 'ok': None}, {'id': 'bad/id', 'ok': True}],
        }
        self.launcher = SimpleNamespace(state_dir=self.root, lock=threading.RLock(),
                                       runtime=Path('/fixture/msfs2024'), require_open=Mock(),
                                       diagnostics=lambda: copy.deepcopy(self.diagnostic))
        self.manager = ProblemReports(self.launcher)
        self.data = {'runtime_path': '/fixture/msfs2024', 'category': 'graphics',
                     'description': 'Menüs sichtbar, Hauptfenster schwarz. ✈',
                     'observations': ['main_view_black', 'menus_visible']}

    def test_snapshot_is_explicit_private_and_frozen_across_restart_and_game_switch(self):
        with patch('socket.create_connection', side_effect=AssertionError('No network')):
            self.assertIsNone(self.manager.snapshot()['draft'])
            self.assertFalse(self.manager.path.exists())
            result = self.manager.prepare(self.data)
            report = result['draft']['report']
            self.assertEqual(result['recipient'], 'contact@flightdeck-app.com')
            self.assertEqual(report['description'], self.data['description'])
            self.assertEqual(report['diagnostics']['checks'], [{'id':'game','ok':True}, {'id':'runner','ok':None}])
            self.assertNotIn('PRIVATE', json.dumps(report))
            self.assertNotIn('/fixture', json.dumps(report))
            self.assertEqual(self.manager.path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(result['draft']['sha256'], hashlib.sha256(encoded(report)).hexdigest())
            self.diagnostic['summary']['graphics'] = {'devices': []}
            self.launcher.runtime = Path('/fixture/msfs2020')
            self.assertEqual(ProblemReports(self.launcher).snapshot()['draft'], result['draft'])
            with self.assertRaises(LauncherError):
                self.manager.prepare(self.data)
            self.assertEqual(self.manager.snapshot()['draft'], result['draft'])

    def test_input_rejects_unknown_fields_controls_and_invalid_observations(self):
        invalid = [None, [], {**self.data, 'url':'https://upload.invalid'},
                   {**self.data, 'category':[]}, {**self.data, 'category':'toString'},
                   {**self.data, 'description':'short'}, {**self.data,'description':'x'*4001},
                   {**self.data, 'description':'invalid\x00description'},
                   {**self.data, 'description':'invalid\ud800description'},
                   {**self.data,'observations':{}}, {**self.data,'observations':['private']},
                   {**self.data,'category':'cloud'}]
        for data in invalid:
            with self.subTest(data=repr(data)), self.assertRaises(LauncherError):
                self.manager.prepare(data)
        self.assertFalse(self.manager.path.exists())

    def test_stale_discard_does_not_remove_new_report(self):
        first = self.manager.prepare(self.data)['draft']['report']['id']
        current = self.manager.prepare({**self.data,'description':'Another black frame after startup'})['draft']
        with self.assertRaises(LauncherError):
            self.manager.discard(first)
        self.assertEqual(self.manager.snapshot()['draft'], current)
        self.assertIsNone(self.manager.discard(current['report']['id'])['draft'])
        self.assertFalse(self.manager.path.exists())

    def test_tampered_or_unsafe_report_is_not_displayed(self):
        self.manager.prepare(self.data)
        value = json.loads(self.manager.path.read_text())
        value['report']['description'] = 'Changed after the capture'
        atomic_json(self.manager.path, value)
        self.assertTrue(self.manager.snapshot()['unreadable'])
        with self.assertRaises(LauncherError):
            self.manager.discard(value['report']['id'])
        self.manager.prepare(self.data)
        self.manager.path.chmod(0o644)
        self.assertTrue(self.manager.snapshot()['unreadable'])
        self.manager.path.unlink()
        secret = self.root / 'private-other-file'
        secret.write_text('PRIVATE-DATA')
        self.manager.path.symlink_to(secret)
        self.assertTrue(self.manager.snapshot()['unreadable'])
        self.manager.prepare(self.data)
        self.assertFalse(self.manager.path.is_symlink())
        self.assertEqual(secret.read_text(), 'PRIVATE-DATA')
        self.manager.path.write_text('x' * (MAX_BYTES + 1))
        self.assertTrue(self.manager.snapshot()['unreadable'])

    def test_invalid_recipient_disables_email_without_losing_report(self):
        for value in ('', 'x@example.com\r\nBcc:leak@example.com', 'x@example.com?bcc=leak@example.com'):
            with patch.dict(os.environ, {'FLIGHTDECK_SUPPORT_EMAIL':value}):
                self.assertIsNone(self.manager.prepare(self.data)['recipient'])
                self.assertIsNotNone(self.manager.snapshot()['draft'])

    def test_failed_capture_does_not_replace_previous_report(self):
        old = self.manager.prepare(self.data)['draft']
        self.diagnostic['summary']['graphics'] = {'huge': 'x' * MAX_BYTES}
        with self.assertRaises(LauncherError):
            self.manager.prepare(self.data)
        self.assertEqual(self.manager.snapshot()['draft'], old)

if __name__ == '__main__':
    unittest.main()
