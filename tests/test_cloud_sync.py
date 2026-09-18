from contextlib import contextmanager
from pathlib import Path
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch, Mock

from flightdeck.backend import Launcher, LauncherError
from flightdeck.cloud_storage import CloudStorageError
from flightdeck.cloud_sync import CloudSaveManager


class FakeRuntime:
    def __init__(self):
        self.calls = []
        self.failure = None
        self.wait = False
        self.entered = threading.Event()

    def available(self, runtime, source_root=None, *, verify=False, write=False):
        return not write

    @contextmanager
    def open_client(self, runtime, source_root=None, *, cancel=None):
        self.calls.append('open')
        try:
            yield self
        finally:
            self.calls.append('close')

    def read_inventory(self, *, cancel=None):
        self.calls.append('index')
        self.entered.set()
        if self.wait:
            if not cancel.wait(3):
                raise AssertionError('test operation was not cancelled')
            raise CloudStorageError('cancelled')
        if self.failure:
            raise self.failure
        return SimpleNamespace(container_count=2, total_bytes=7)

    def download_snapshot(self, parent_directory, *, cancel=None):
        self.calls.append('snapshot')
        self.asserted_parent = parent_directory
        return SimpleNamespace(container_count=2, blob_count=3, total_bytes=7,
                               path=parent_directory / 'snapshot-test', consistency='rechecked-unlocked')


class CloudSyncTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        runtime = self.root / 'runtime'
        (runtime / 'private').mkdir(mode=0o700, parents=True)
        (runtime / 'private/play.lock').touch(mode=0o600)
        (runtime / 'tools').mkdir()
        (runtime / 'tools/play-msfs.sh').write_text('#!/bin/sh\nexit 0\n')
        self.launcher = Launcher(self.root / 'state', str(runtime))
        self.api = FakeRuntime()
        self.manager = CloudSaveManager(self.launcher, self.api)
        self.launcher.cloud_saves = self.manager

    def tearDown(self):
        self.manager.close()
        self.launcher.setup.close()
        self.tmp.cleanup()

    def finish(self):
        self.manager.worker.join(3)
        self.assertFalse(self.manager.worker.is_alive())
        self.assertFalse(self.launcher.setup_busy)
        return self.manager.snapshot()['job']

    def test_status_is_local_and_does_not_contact_account(self):
        status = self.manager.snapshot()
        self.assertTrue(status['can_check'])
        self.assertFalse(status['sync_supported'])
        self.assertEqual(self.api.calls, [])

    def test_index_projects_only_counts_and_releases_runtime(self):
        self.manager.start('check')
        job = self.finish()
        self.assertEqual(job['state'], 'succeeded')
        self.assertEqual(job['result'], dict(container_count=2, blob_count=None, total_bytes=7,
                                             downloaded=False, rechecked=False))
        self.assertEqual(self.api.calls, ['open', 'index', 'close'])
        self.assertIsNone(self.launcher._owned_runtime_operation)

    def test_download_is_separate_archive_not_an_import(self):
        before = sorted(str(p.relative_to(self.root)) for p in self.root.rglob('*'))
        self.manager.start('download')
        job = self.finish()
        self.assertEqual(job['state'], 'succeeded')
        self.assertEqual(self.api.asserted_parent, self.launcher.runtime / 'private/cloud-saves')
        self.assertEqual(job['result']['consistency'], 'rechecked-unlocked')
        self.assertEqual(job['result']['snapshot_id'], 'snapshot-test')
        self.assertEqual(sorted(before + ['runtime/private/cloud-saves']),
                         sorted(str(p.relative_to(self.root)) for p in self.root.rglob('*')))
        self.assertEqual(self.api.asserted_parent.stat().st_mode & 0o777, 0o700)

    def test_download_parent_symlink_or_shared_directory_rejected(self):
        target = self.root / 'external'
        target.mkdir()
        destination = self.launcher.runtime / 'private/cloud-saves'
        destination.symlink_to(target, target_is_directory=True)
        self.manager.start('download')
        self.assertEqual(self.finish()['state'], 'failed')
        self.assertNotIn('snapshot', self.api.calls)
        self.assertEqual(list(target.iterdir()), [])
        destination.unlink()
        destination.mkdir(mode=0o755)
        self.manager.start('download')
        self.assertEqual(self.finish()['state'], 'failed')
        self.assertEqual(destination.stat().st_mode & 0o777, 0o755)

    def test_cancel_after_atomic_download_retains_result(self):
        original = self.api.download_snapshot
        def committed(parent_directory, *, cancel=None):
            result = original(parent_directory, cancel=cancel)
            cancel.set()
            return result
        self.api.download_snapshot = committed
        self.manager.start('download')
        job = self.finish()
        self.assertEqual(job['state'], 'succeeded')
        self.assertTrue(job['result']['downloaded'])

    def test_auth_failure_never_becomes_empty_cloud(self):
        self.api.failure = CloudStorageError('authentication')
        self.manager.start('check')
        job = self.finish()
        self.assertEqual(job['state'], 'failed')
        self.assertEqual(job['error_code'], 'authentication')
        self.assertIsNone(job['result'])

    def test_unexpected_error_does_not_expose_private_details(self):
        self.api.failure = RuntimeError('secret-account-credential')
        self.manager.start('check')
        job = self.finish()
        self.assertEqual(job['state'], 'failed')
        self.assertNotIn('secret-account', repr(job))
        self.assertIsNone(job['result'])

    def test_running_job_excludes_launch_config_backup_and_duplicate(self):
        self.api.wait = True
        job_id = self.manager.start('check')['job_id']
        self.assertTrue(self.api.entered.wait(2))
        self.assertFalse(self.launcher.status()['game']['can_start'])
        self.assertFalse(self.launcher.status()['saves']['can_backup'])
        self.assertFalse(self.manager.snapshot()['can_download'])
        for action in (lambda: self.manager.start('download'),
                       lambda: self.launcher.configure(str(self.launcher.runtime)),
                       self.launcher.launch, self.launcher.backup):
            with self.assertRaises(LauncherError):
                action()
        with self.assertRaises(LauncherError):
            self.manager.cancel('stale-job')
        self.assertFalse(self.manager.cancel_event.is_set())
        self.manager.cancel(job_id)
        self.assertEqual(self.finish()['state'], 'cancelled')

    def test_existing_external_runtime_lock_is_respected(self):
        with self.launcher.runtime_lock():
            with self.assertRaises(LauncherError):
                self.manager.start('check')
        self.assertEqual(self.api.calls, [])
        self.assertFalse(self.launcher.setup_busy)

    def test_close_cancels_and_no_write_actions_exist(self):
        for operation in ('upload', 'sync', 'delete', 'import'):
            with self.assertRaises(LauncherError):
                self.manager.start(operation)
        self.api.wait = True
        self.manager.start('check')
        self.assertTrue(self.api.entered.wait(2))
        self.manager.close()
        self.assertEqual(self.finish()['state'], 'cancelled')
        with self.assertRaises(LauncherError):
            self.manager.start('check')

    def enable_import(self):
        (self.launcher.runtime / 'private/local-saves.enabled').touch(mode=0o600)
        (self.launcher.runtime / 'private/local-saves').mkdir(mode=0o700)
        from flightdeck.cloud_storage import Scope
        self.api.scope = Scope('123', 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', 'Example.Game_0123456789abc', 1234)
        plan = Mock(remote_digest='synthetic-content')
        plan.summary.return_value = dict(container_count=2, blob_count=3, total_bytes=7,
            local_exists=True, local_container_count=1, add_count=1, replace_count=1,
            delete_count=0, unchanged_count=0, conflict_count=1)
        return plan

    def test_prepare_only_previews_then_exact_choice_imports_after_fresh_download(self):
        plan = self.enable_import()
        with patch('flightdeck.cloud_import.prepare', return_value=plan), \
                patch('flightdeck.cloud_import.apply') as apply:
            self.manager.start('prepare-import')
            self.assertEqual(self.finish()['state'], 'succeeded')
            preview = self.manager.snapshot()['plan']
            self.assertEqual(preview['conflict_count'], 1)
            apply.assert_not_called()
            for choice, plan_id in [('local', preview['id']), ('cloud', 'old-id'), (None, preview['id'])]:
                with self.assertRaises(LauncherError):
                    self.manager.start('import', choice=choice, plan_id=plan_id)
            apply.return_value.summary.return_value = dict(imported=True, backup_id='opaque-backup',
                generation=8, container_count=2, blob_count=3, total_bytes=7, durability_confirmed=True)
            self.manager.start('import', choice='cloud', plan_id=preview['id'])
            job = self.finish()
            self.assertEqual(job['state'], 'succeeded')
            self.assertTrue(job['result']['imported'])
            self.assertEqual(self.api.calls.count('snapshot'), 2)
            args, kwargs = apply.call_args
            self.assertIs(args[1], self.api.scope)
            self.assertIs(args[2], plan)
            self.assertIsInstance(kwargs['runtime_lock_fd'], int)
            self.assertIsNone(self.manager.snapshot()['plan'])
            with self.assertRaises(LauncherError):
                self.manager.start('import', choice='cloud', plan_id=preview['id'])

    def test_remote_changes_since_preview_abort_import(self):
        plan = self.enable_import()
        with patch('flightdeck.cloud_import.prepare', side_effect=[plan, Mock(remote_digest='changed')]), \
                patch('flightdeck.cloud_import.apply') as apply:
            self.manager.start('prepare-import'); self.finish()
            self.manager.start('import', choice='cloud', plan_id=self.manager.snapshot()['plan']['id'])
            job = self.finish()
            self.assertEqual(job['error_code'], 'changed')
            self.assertIsNone(job['result'])
            apply.assert_not_called()

    def test_expired_switched_or_discarded_plan_cannot_import(self):
        plan = self.enable_import()
        with patch('flightdeck.cloud_import.prepare', return_value=plan):
            self.manager.start('prepare-import'); self.finish()
            plan_id = self.manager.snapshot()['plan']['id']
            with patch('flightdeck.cloud_sync.time.monotonic', return_value=self.manager.plan_deadline+1):
                self.assertIsNone(self.manager.snapshot()['plan'])
                with self.assertRaises(LauncherError):
                    self.manager.start('import', choice='cloud', plan_id=plan_id)
            self.manager.plan_runtime = self.root / 'another-runtime'
            with self.assertRaises(LauncherError):
                self.manager.start('import', choice='cloud', plan_id=plan_id)
            self.manager.plan_runtime = self.launcher.runtime
            with self.assertRaises(LauncherError):
                self.manager.discard_plan('other-plan')
            self.manager.discard_plan(plan_id)
            self.assertIsNone(self.manager.snapshot()['plan'])

    def test_commit_survives_cancellation_and_late_connection_cleanup_failure(self):
        plan = self.enable_import()
        original = self.api.open_client
        with patch('flightdeck.cloud_import.prepare', return_value=plan), \
                patch('flightdeck.cloud_import.apply') as apply:
            self.manager.start('prepare-import'); self.finish()
            plan_id = self.manager.snapshot()['plan']['id']
            def committed(*args, **kwargs):
                kwargs['cancel'].set()
                return SimpleNamespace(summary=lambda: dict(imported=True, backup_id='backup-fixture',
                    generation=8, container_count=2, blob_count=3, total_bytes=7, durability_confirmed=True))
            @contextmanager
            def cleanup_failure(*args, **kwargs):
                with original(*args, **kwargs) as client:
                    yield client
                raise RuntimeError('private details')
            apply.side_effect = committed
            self.api.open_client = cleanup_failure
            self.manager.start('import', choice='cloud', plan_id=plan_id)
            job = self.finish()
            self.assertEqual(job['state'], 'succeeded')
            self.assertEqual(job['warning'], 'connection_cleanup')
            self.assertTrue(job['result']['imported'])
            self.assertNotIn('private details', repr(job))


if __name__ == '__main__':
    unittest.main()
