# SPDX-License-Identifier: MIT
"""Automatic game lifecycle with real save codecs and an in-memory Xbox service."""
from contextlib import contextmanager
from dataclasses import replace
import fcntl
import threading
import time
import unittest
from unittest.mock import patch

from flightdeck import cloud_auto, cloud_cache, cloud_import, cloud_session, cloud_storage, save_state
from flightdeck.backend import LauncherError
from tests import test_cloud_sync_integration as integration
from tests.test_cloud_write import SCOPE, state


class Game:
    def __init__(self):
        self.done = threading.Event()
    def wait(self):
        if not self.done.wait(5): raise RuntimeError('Synthetic child not stopped')
        return getattr(self, 'exit_code', 0)
    def poll(self): return 0 if self.done.is_set() else None
    def send_signal(self, signal): self.done.set()


class AutomaticCloudTests(unittest.TestCase):
    install = integration.CloudSyncIntegrationTests.install
    finish = integration.CloudSyncIntegrationTests.finish
    prepare = integration.CloudSyncIntegrationTests.prepare

    def setUp(self):
        integration.CloudSyncIntegrationTests.setUp(self)
        for name in ('games/MSFS2024/FlightSimulator2024.exe', 'local/msfs-prefix/system.reg',
                     'local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll'):
            path = self.runtime / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('synthetic fixture')
        (self.runtime / 'tools/play-msfs.sh').chmod(0o700)
        self.auto = self.manager.automation
        self.game = Game()
        self.spawned = threading.Event()
        self.spawn_count = 0
        def spawn(fd):
            self.assertIsNotNone(fd)
            self.assertTrue(self.launcher.setup_busy)
            self.assertIsNotNone(self.launcher.managed_session)
            self.launcher.process = self.game
            self.launcher.started_monotonic = time.monotonic()
            self.spawn_count += 1
            self.spawned.set()
            return self.game
        patcher = patch.object(self.launcher, '_spawn_reserved', side_effect=spawn)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(self.stop_game)

    def stop_game(self):
        self.game.done.set()
        if self.auto.worker: self.auto.worker.join(5)

    def start_game(self):
        result = self.auto.launch()
        self.assertTrue(result['cloud_sync'])
        self.assertTrue(self.spawned.wait(3), self.auto.snapshot())
        self.assertEqual(self.auto.snapshot()['state'], 'playing')
        return result

    def finish_auto(self):
        self.auto.worker.join(5)
        self.assertFalse(self.auto.worker.is_alive())
        self.assertFalse(self.launcher.setup_busy)
        self.assertIsNone(self.launcher.managed_session)
        return self.auto.snapshot()

    def local_state(self):
        return save_state.decode((self.folder / 'state.bin').read_bytes())

    def test_one_click_cloud_first_then_exit_upload_with_backup_and_receipt(self):
        old = (self.folder / 'state.bin').read_bytes()
        self.start_game()
        self.assertEqual(self.local_state().containers['profile'].blobs['data'], b'cloud')
        self.assertNotIn('acquire', self.ops.calls)  # Normal prestart is a read/import.
        self.assertEqual(self.spawn_count, 1)
        self.assertEqual(cloud_session.load(self.runtime, SCOPE)['phase'], 'playing')
        with (self.runtime / 'private/play.lock').open('rb') as contender:
            with self.assertRaises(BlockingIOError): fcntl.flock(contender, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.launcher.status()  # Polling cannot prematurely release the session.
        self.install(state(profile=b'new flight'))
        self.game.done.set()
        outcome = self.finish_auto()
        self.assertEqual(outcome['state'], 'synced', outcome)
        timings = outcome['timings']
        for phase in ('before', 'after'):
            self.assertGreaterEqual(timings[phase + '_total_seconds'], timings[phase + '_connect_seconds'])
            self.assertGreaterEqual(timings[phase + '_cleanup_seconds'], 0)
        self.assertTrue(all(isinstance(value, (int, float)) for value in timings.values()))
        self.assertIsNone(self.launcher.process)
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'new flight')
        self.assertIsNotNone(cloud_import.load_baseline(self.runtime, SCOPE))
        self.assertIsNone(cloud_session.load(self.runtime, SCOPE))
        self.assertTrue(any(p.read_bytes() == old for p in (self.runtime/'private/save-backups').rglob('state.bin')))
        self.assertEqual(self.spawn_count, 1)

    def test_failed_cloud_read_never_starts_game_or_changes_saves(self):
        old = (self.folder / 'state.bin').read_bytes()
        @contextmanager
        def unavailable(*args, **kwargs):
            raise cloud_storage.CloudStorageError('authentication')
            yield
        with patch.object(self.api, 'open_client', unavailable):
            self.auto.launch()
            outcome = self.finish_auto()
        self.assertEqual(outcome['state'], 'attention')
        self.assertTrue(outcome['can_play_local'])
        self.assertFalse(self.spawned.is_set())
        self.assertEqual((self.folder / 'state.bin').read_bytes(), old)
        self.assertEqual(self.ops.calls, [])

    def test_next_launch_reuses_verified_exit_readback_with_two_fresh_index_reads(self):
        self.start_game()
        self.install(state(profile=b'new flight'))
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.ops.calls.clear()
        self.game = Game()
        self.spawned.clear()
        self.start_game()
        self.assertEqual(self.ops.calls, ['GET', 'GET'])
        self.assertEqual(self.local_state().containers['profile'].blobs['data'], b'new flight')
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')


    def test_unchanged_exit_checks_fresh_revisions_without_redownloading_all_saves(self):
        self.ops.remote = state(**{f'c{i:02}': bytes([65+i]) for i in range(18)})
        self.start_game()
        self.ops.calls.clear()
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertEqual(self.ops.calls.count('GET'), 6)
        self.assertNotIn('put', self.ops.calls)
        self.assertNotIn('atom', self.ops.calls)
        self.assertIsNotNone(cloud_import.load_baseline(self.runtime, SCOPE))

    def test_changed_exit_reads_written_payloads_but_reuses_unchanged_containers(self):
        payloads = {f'c{i:02}': bytes([65+i]) for i in range(18)}
        self.ops.remote = state(**payloads)
        self.start_game()
        self.install(state(**{**payloads, 'c00': b'new flight'}))
        self.ops.calls.clear()
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertEqual(self.ops.calls.count('GET'), 14)
        self.assertEqual(self.ops.calls.count('put'), 1)
        self.assertEqual(self.ops.remote.containers['c00'].blobs['data'], b'new flight')
        self.assertIsNone(cloud_session.load(self.runtime, SCOPE))

    def test_write_readback_fetches_all_bytes_even_with_warm_cache(self):
        client = cloud_storage.CloudStorageClient(SCOPE, self.ops)
        self.auto._download(self.runtime, client)
        self.ops.calls.clear()
        with patch.object(cloud_cache, 'remember', return_value=False):
            remote = self.auto._readback(self.runtime, client)(timeout=30)
        self.assertEqual(self.ops.calls, ['GET'] * 5)
        self.assertEqual(remote.containers['profile'].blobs['data'], b'cloud')

    def test_changed_and_added_containers_avoid_repeated_complete_downloads(self):
        payloads = {f'c{i:02}': bytes([65+i]) for i in range(18)}
        self.ops.remote = state(**payloads)
        self.start_game()
        self.install(state(**{**payloads, 'c00': b'changed', 'c18': b'new save'}))
        self.ops.calls.clear()
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertEqual(self.ops.calls.count('GET'), 22)
        self.assertEqual(self.ops.calls.count('put'), 2)
        self.assertEqual(len(self.ops.remote.containers), 19)
        self.assertEqual(self.ops.remote.containers['c18'].blobs['data'], b'new save')
        self.assertIsNone(cloud_session.load(self.runtime, SCOPE))

    def test_optional_cache_reread_failure_does_not_invalidate_remote_write_proof(self):
        client = cloud_storage.CloudStorageClient(SCOPE, self.ops)
        with patch.object(cloud_cache, 'remember', side_effect=OSError('cache unavailable')):
            remote = self.auto._readback(self.runtime, client)(timeout=30)
        self.assertEqual(self.ops.calls, ['GET'] * 5)
        self.assertEqual(remote.containers['profile'].blobs['data'], b'cloud')

    def test_explicit_local_session_is_backed_up_and_never_uploads(self):
        @contextmanager
        def unavailable(*args, **kwargs):
            raise cloud_storage.CloudStorageError('authentication')
            yield
        with patch.object(self.api, 'open_client', unavailable):
            self.auto.launch()
            first = self.finish_auto()
        with self.assertRaises(LauncherError): self.auto.action('play-local', 'stale')
        self.auto.action('play-local', first['request_id'])
        self.assertTrue(self.spawned.wait(3))
        self.install(state(profile=b'offline flight'))
        self.game.done.set()
        result = self.finish_auto()
        self.assertEqual(result['state'], 'local', result)
        self.assertEqual(self.ops.calls, [])
        self.assertTrue((self.runtime/'private/cloud-offline.pending').is_file())
        self.auto.launch()
        result = self.finish_auto()
        self.assertTrue(result['conflict'], result)
        self.assertEqual(self.local_state().containers['profile'].blobs['data'], b'offline flight')

    def test_same_container_conflict_stops_before_launch_and_resolution_continues(self):
        plan = self.prepare()
        self.manager.start('import', plan_id=plan, choice='cloud')
        self.finish()
        self.install(state(profile=b'local edit'))
        self.ops.remote = state(profile=b'other device')
        self.auto.launch()
        outcome = self.finish_auto()
        self.assertTrue(outcome['conflict'], outcome)
        self.assertFalse(self.spawned.is_set())
        self.auto.action('resolve', outcome['request_id'], choice='cloud')
        self.assertTrue(self.spawned.wait(3), self.auto.snapshot())
        self.assertEqual(self.local_state().containers['profile'].blobs['data'], b'other device')
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')

    def test_cloud_changes_during_game_block_exit_upload_and_explicit_local_choice_works(self):
        self.start_game()
        self.install(state(profile=b'local flight'))
        self.ops.remote = state(profile=b'other computer')
        self.game.done.set()
        outcome = self.finish_auto()
        self.assertTrue(outcome['conflict'], outcome)
        self.assertEqual(outcome['phase'], 'after_exit')
        self.assertFalse(outcome['can_play_local'])
        self.assertNotIn('put', self.ops.calls)
        self.auto.action('resolve', outcome['request_id'], choice='local')
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'local flight')
        self.assertEqual(self.spawn_count, 1)

    def test_partial_upload_keeps_journal_and_target_backup_for_next_start(self):
        self.start_game()
        self.install(state(first=b'a', second=b'b'))
        put = self.ops.put_container
        self.ops.put_container = lambda name, *a, **kw: 409 if name == 'second,savedgame' else put(name, *a, **kw)
        self.game.done.set()
        outcome = self.finish_auto()
        self.assertEqual(outcome['state'], 'attention', outcome)
        record = cloud_session.load(self.runtime, SCOPE)
        self.assertEqual(record['phase'], 'uploading')
        self.assertTrue(record['target_backup_id'])
        self.auto.launch()
        outcome = self.finish_auto()
        self.assertTrue(outcome['conflict'], outcome)
        self.assertEqual(self.spawn_count, 1)

    def test_failed_exit_upload_can_start_offline_without_discarding_pending_progress(self):
        self.start_game()
        self.install(state(profile=b'first flight'))
        self.ops.acquire_status = 503
        self.game.done.set()
        failed = self.finish_auto()
        self.assertEqual(failed['phase'], 'after_exit')
        self.assertEqual(failed['error_code'], 'transport')
        self.assertEqual(failed['error_details'], {'http_status': 503})
        self.assertTrue(failed['can_play_local'])
        journal = cloud_session.load(self.runtime, SCOPE)
        self.ops.calls.clear()
        self.game = Game()
        self.spawned.clear()
        self.auto.action('play-local', failed['request_id'])
        self.assertTrue(self.spawned.wait(3), self.auto.snapshot())
        self.assertEqual(self.ops.calls, [])
        self.assertEqual(cloud_session.load(self.runtime, SCOPE), journal)
        self.install(state(profile=b'second offline flight'))
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'local')
        self.assertEqual(self.ops.calls, [])
        self.assertEqual(cloud_session.load(self.runtime, SCOPE), journal)
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'cloud')
        self.assertTrue(any(b'second offline flight' in p.read_bytes()
                            for p in (self.runtime / 'private/save-backups').rglob('state.bin')))
        # The old upload target cannot silently overwrite subsequent play.
        self.ops.acquire_status = 201
        self.auto.launch()
        conflict = self.finish_auto()
        self.assertTrue(conflict['conflict'], conflict)
        self.assertFalse(conflict['can_play_local'])
        self.assertEqual(self.local_state().containers['profile'].blobs['data'], b'second offline flight')
        self.assertFalse({'put', 'delete', 'atom'} & set(self.ops.calls))
        self.game = Game()
        self.spawned.clear()
        self.auto.action('resolve', conflict['request_id'], choice='local')
        self.assertTrue(self.spawned.wait(3), self.auto.snapshot())
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'second offline flight')
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertIsNone(cloud_session.load(self.runtime, SCOPE))

    def test_partial_cloud_write_then_local_play_requires_fresh_explicit_resolution(self):
        self.start_game()
        self.install(state(first=b'a', second=b'b'))
        put = self.ops.put_container
        self.ops.put_container = lambda name, *a, **kw: 409 if name == 'second,savedgame' else put(name, *a, **kw)
        self.game.done.set()
        failed = self.finish_auto()
        self.assertTrue(failed['can_play_local'], failed)
        journal = cloud_session.load(self.runtime, SCOPE)
        self.ops.calls.clear()
        self.game = Game()
        self.spawned.clear()
        self.auto.action('play-local', failed['request_id'])
        self.assertTrue(self.spawned.wait(3))
        self.install(state(first=b'new progress', second=b'b'))
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'local')
        self.assertEqual(self.ops.calls, [])
        self.assertEqual(cloud_session.load(self.runtime, SCOPE), journal)
        self.ops.put_container = put
        self.auto.launch()
        conflict = self.finish_auto()
        self.assertTrue(conflict['conflict'], conflict)
        self.assertEqual(self.local_state().containers['first'].blobs['data'], b'new progress')
        self.assertFalse({'put', 'delete', 'atom'} & set(self.ops.calls))

    def test_unsafe_or_wrong_account_exit_cannot_be_bypassed_with_local_play(self):
        for code in ('unsafe_session', 'invalid_scope', 'local_storage', 'changed', 'graphics'):
            self.auto.runtime = self.runtime
            self.auto.request_id = 'current'
            self.auto.phase = 'after_exit'
            self.auto.state = 'attention'
            self.auto.error_code = code
            self.assertFalse(self.auto.snapshot()['can_play_local'], code)
            with self.assertRaises(LauncherError):
                self.auto.action('play-local', 'current')
        self.assertEqual(self.spawn_count, 0)

    def test_native_cloud_diagnostics_are_numeric_only(self):
        error = cloud_storage.CloudStorageError('transport')
        error.http_status = 503
        error.native_hresult = 0x80072efd
        error.token = 'must not export'
        self.assertEqual(cloud_auto.error_details(error), {'http_status': 503, 'native_hresult': 0x80072efd})
        error.http_status = True
        error.native_hresult = 'private raw response'
        self.assertEqual(cloud_auto.error_details(error), {})

    def test_graphics_failure_is_not_misreported_as_a_cloud_connection_failure(self):
        error = LauncherError('Synthetic graphics prerequisite missing')
        error.code = 'graphics'
        with patch.object(self.launcher, '_spawn_reserved', side_effect=error):
            self.auto.launch()
            result = self.finish_auto()
        self.assertEqual(result['error_code'], 'graphics')
        self.assertEqual(result['message'], str(error))
        self.assertFalse(result['can_play_local'])
        self.assertFalse((self.runtime / 'private/cloud-interrupted-process.json').exists())

    def test_cancel_before_play_retains_backups_and_never_spawns(self):
        original = self.auto._before
        def cancel(*args, **kwargs):
            value = original(*args, **kwargs)
            self.auto.cancel.set()
            return value
        with patch.object(self.auto, '_before', side_effect=cancel):
            self.auto.launch()
            result = self.finish_auto()
        self.assertEqual(result['state'], 'idle')
        self.assertFalse(self.spawned.is_set())
        self.assertIsNotNone(cloud_session.load(self.runtime, SCOPE))

    def test_no_status_request_contacts_cloud_or_starts_recovery(self):
        for _ in range(3):
            self.auto.snapshot()
            self.manager.snapshot()
            self.launcher.status()
        self.assertEqual(self.ops.calls, [])
        self.assertIsNone(self.auto.worker)

    def test_helper_cleanup_after_verified_exit_preserves_success(self):
        self.start_game()
        self.api.cleanup_failure = True
        self.install(state(profile=b'confirmed'))
        self.game.done.set()
        outcome = self.finish_auto()
        self.assertEqual(outcome['state'], 'synced', outcome)
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'confirmed')

    def test_missing_local_save_gate_is_enabled_as_part_of_normal_start(self):
        (self.runtime/'private/local-saves.enabled').unlink()
        self.start_game()
        self.assertTrue((self.runtime/'private/local-saves.enabled').is_file())
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')

    def test_killed_supervisor_never_uploads_and_blocks_another_same_boot_session(self):
        self.game.exit_code = -9
        self.start_game()
        self.install(state(profile=b'possibly still active'))
        self.game.done.set()
        result = self.finish_auto()
        self.assertEqual(result['error_code'], 'unsafe_session')
        self.assertNotIn('acquire', self.ops.calls)
        self.assertIsNotNone(cloud_session.load(self.runtime, SCOPE))
        self.auto.launch()
        result = self.finish_auto()
        self.assertEqual(result['error_code'], 'unsafe_session')
        self.assertFalse(result['can_play_local'])
        self.assertEqual(self.spawn_count, 1)

    def test_failed_spawn_clears_guard_only_when_no_child_was_created(self):
        with patch.object(self.launcher, '_spawn_reserved', side_effect=OSError('synthetic spawn failure')):
            self.auto.launch()
            result = self.finish_auto()
        self.assertEqual(result['state'], 'attention')
        self.assertFalse((self.runtime/'private/cloud-interrupted-process.json').exists())
        self.assertEqual(self.ops.calls.count('acquire'), 0)

    def journal(self, phase='playing'):
        """Prepare the exact durable state a prior launcher would leave."""
        with self.launcher.runtime_lock() as fd:
            with self.api.open_client(self.runtime) as client:
                snapshot = client.download_snapshot(cloud_auto._snapshot_directory(self.runtime))
                with cloud_import.export_local(self.runtime, client.scope, runtime_lock_fd=fd) as local:
                    record = cloud_session.begin(self.runtime, client.scope, snapshot=snapshot, export=local)
                    return cloud_session.update(self.runtime, client.scope, record, phase=phase, export=local)

    def fresh_coordinator(self):
        self.assertFalse(self.auto.worker and self.auto.worker.is_alive())
        self.auto = cloud_auto.CloudAutomation(self.manager)
        self.manager.automation = self.auto
        self.game = Game()
        self.spawned.clear()

    def test_account_b_sync_does_not_clear_account_a_offline_progress_guard(self):
        self.install(state(profile=b'account A offline progress'))
        original = (self.folder / 'state.bin').read_bytes()
        with self.launcher.runtime_lock() as fd:
            cloud_auto._offline(self.runtime, fd, 'set')
        other = integration.SyntheticTransport(state(profile=b'account B cloud'))
        other.scope = replace(SCOPE, xuid='987654321')
        self.api.transport = other
        self.start_game()
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertIsNotNone(cloud_import.load_baseline(self.runtime, other.scope))
        self.assertTrue((self.runtime / 'private/cloud-offline.pending').is_file())
        self.assertEqual((self.folder / 'state.bin').read_bytes(), original)
        self.assertIsNone(cloud_import.load_baseline(self.runtime, SCOPE))
        # A fresh launcher must still ask before replacing account A's progress.
        self.fresh_coordinator()
        self.api.transport = self.ops
        self.auto.launch()
        result = self.finish_auto()
        self.assertTrue(result['conflict'], result)
        self.assertFalse(self.spawned.is_set())
        self.assertEqual((self.folder / 'state.bin').read_bytes(), original)
        self.assertFalse({'put', 'delete', 'atom'} & set(self.ops.calls))

    def test_changed_account_after_exit_and_retry_never_uploads_to_new_account(self):
        self.start_game()
        self.install(state(profile=b'account A latest flight'))
        other = integration.SyntheticTransport(state(profile=b'account B cloud'))
        other.scope = replace(SCOPE, xuid='987654321')
        other_digest = save_state.content_digest(other.remote)
        self.api.transport = other
        self.game.done.set()
        result = self.finish_auto()
        self.assertEqual(result['error_code'], 'invalid_scope', result)
        self.assertEqual(result['phase'], 'after_exit')
        self.assertEqual(other.calls, [])
        original_journal = cloud_session.load(self.runtime, SCOPE)
        self.assertIsNotNone(original_journal)
        self.assertIsNone(cloud_session.load(self.runtime, other.scope))
        self.auto.action('retry', result['request_id'])
        result = self.finish_auto()
        self.assertEqual(result['error_code'], 'invalid_scope', result)
        self.assertEqual(other.calls, [])
        self.assertEqual(cloud_session.load(self.runtime, SCOPE), original_journal)
        self.assertEqual(save_state.content_digest(other.remote), other_digest)
        self.api.transport = self.ops
        self.auto.action('retry', result['request_id'])
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'account A latest flight')
        self.assertEqual(self.spawn_count, 1)

    def test_restart_playing_record_preserves_new_progress_when_cloud_unchanged(self):
        record = self.journal('playing')
        self.install(state(profile=b'progress after old launcher exited'))
        current = (self.folder / 'state.bin').read_bytes()
        self.fresh_coordinator()
        self.start_game()
        self.assertEqual((self.folder / 'state.bin').read_bytes(), current)
        resumed = cloud_session.load(self.runtime, SCOPE)
        self.assertEqual(resumed['session_id'], record['session_id'])
        self.assertEqual(resumed['before_remote_digest'], record['before_remote_digest'])
        self.assertNotEqual(resumed['target_digest'], record['target_digest'])
        self.assertNotIn('acquire', self.ops.calls)
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertEqual(self.ops.remote.containers['profile'].blobs['data'], b'progress after old launcher exited')

    def test_restart_uncertain_upload_with_later_local_progress_requires_resolution(self):
        record = self.journal('uploading')
        self.install(state(profile=b'later local progress'))
        current = (self.folder / 'state.bin').read_bytes()
        self.fresh_coordinator()
        self.auto.launch()
        result = self.finish_auto()
        self.assertTrue(result['conflict'], result)
        self.assertFalse(self.spawned.is_set())
        self.assertEqual((self.folder / 'state.bin').read_bytes(), current)
        self.assertEqual(cloud_session.load(self.runtime, SCOPE), record)
        self.assertFalse({'acquire', 'put', 'delete', 'atom'} & set(self.ops.calls))

    def test_restart_fully_committed_target_is_verified_without_reupload(self):
        record = self.journal('uploading')
        self.ops.remote = self.local_state()  # Server committed before process loss.
        current = (self.folder / 'state.bin').read_bytes()
        self.fresh_coordinator()
        self.start_game()
        recovered = cloud_session.load(self.runtime, SCOPE)
        self.assertEqual(recovered['before_remote_digest'], record['target_digest'])
        self.assertEqual(recovered['phase'], 'playing')
        self.assertIsNotNone(cloud_import.load_baseline(self.runtime, SCOPE))
        self.assertEqual((self.folder / 'state.bin').read_bytes(), current)
        self.assertIn('acquire', self.ops.calls)
        self.assertFalse({'put', 'delete', 'atom'} & set(self.ops.calls))
        self.game.done.set()
        self.assertEqual(self.finish_auto()['state'], 'synced')
        self.assertIsNone(cloud_session.load(self.runtime, SCOPE))
        self.assertFalse({'put', 'delete', 'atom'} & set(self.ops.calls))

    def test_restart_unknown_partial_cloud_never_uses_first_cloud_policy(self):
        record = self.journal('uploading')
        self.ops.remote = state(profile=b'unknown partial result')
        current = (self.folder / 'state.bin').read_bytes()
        self.fresh_coordinator()
        self.auto.launch()
        result = self.finish_auto()
        self.assertTrue(result['conflict'], result)
        self.assertFalse(self.spawned.is_set())
        self.assertEqual((self.folder / 'state.bin').read_bytes(), current)
        self.assertEqual(cloud_session.load(self.runtime, SCOPE), record)
        self.assertFalse({'acquire', 'put', 'delete', 'atom'} & set(self.ops.calls))
