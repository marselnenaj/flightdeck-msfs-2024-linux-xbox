import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import maintenance
from flightdeck.backend import Launcher, LauncherError
from tests.test_game_update import game


class MaintenanceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / 'runtime'
        for name in ('private/local-saves', 'games', 'tools', 'local/msfs-prefix/drive_c/windows/system32',
                     'local/store-runtime/x86_64-windows', 'runner/files/bin', 'runner/files/lib/wine/x86_64-windows'):
            (self.runtime / name).mkdir(parents=True, exist_ok=True, mode=0o700)
        (self.runtime / 'tools/play-msfs.sh').write_text('#!/bin/sh\nexit 0\n')
        (self.runtime / 'private/runtime.json').write_text('{"game_id":"msfs2024","market":"AT"}')
        (self.runtime / 'private/local-saves/save').write_text('precious progress')
        (self.runtime / 'runner/files/bin/wine').write_text('synthetic')
        self.prefix = self.runtime / 'local/msfs-prefix'
        (self.prefix / 'system.reg').write_text('old settings')
        (self.prefix / 'user.reg').write_text('old user settings')
        self.game = game(self.root / 'game')
        (self.runtime / 'games/MSFS2024').symlink_to(self.game)
        self.foreign = self.root / 'foreign'
        self.foreign.mkdir()
        (self.foreign / 'keep').write_text('untouched')
        (self.prefix / 'external').symlink_to(self.foreign, target_is_directory=True)
        hashes = {}
        for key, target in [('runtime/xgameruntime.dll', self.prefix / 'drive_c/windows/system32/xgameruntime.dll'),
                            ('builtin/x86_64-windows/xodus_store_test.dll', self.runtime / 'local/store-runtime/x86_64-windows/xodus_store_test.dll')]:
            target.write_bytes(key.encode())
            hashes[key] = hashlib.sha256(target.read_bytes()).hexdigest()
        original = self.runtime / 'runner/files/lib/wine/x86_64-windows/xgameruntime.dll'
        original.write_bytes(b'original')
        (self.runtime / 'private/import-manifest.json').write_text(json.dumps({'artifacts':{'files':hashes}, 'original_runtime_sha256':hashlib.sha256(b'original').hexdigest()}))
        self.launcher = Launcher(self.root / 'state', str(self.runtime))
        self.manager = self.launcher.maintenance
        self.addCleanup(self.manager.close)
        self.addCleanup(self.launcher.setup.close)
        idle = patch.object(maintenance, '_idle')
        idle.start(); self.addCleanup(idle.stop)

    def preview(self, operation='uninstall', **options):
        self.manager.preview({'operation':operation, **options})
        self.manager.thread.join(5)
        self.assertFalse(self.manager.thread.is_alive())
        self.assertEqual(self.manager.job['state'], 'ready', self.manager.job)
        return self.manager.job['id']

    def run_job(self, job_id):
        self.manager.start({'job_id':job_id,'confirmed':True})
        self.manager.thread.join(5)
        self.assertFalse(self.manager.thread.is_alive())

    @staticmethod
    def prepare_prefix(runner, prefix, **kwargs):
        (prefix / 'drive_c/windows/system32').mkdir(parents=True)
        (prefix / 'system.reg').write_text('fresh settings')
        (prefix / 'user.reg').write_text('fresh user settings')

    def test_preview_never_deletes_and_requires_confirmation_and_current_id(self):
        identifier = self.preview()
        for payload in ({'job_id':identifier}, {'job_id':'stale','confirmed':True}):
            with self.assertRaises(LauncherError): self.manager.start(payload)
        self.assertTrue(self.game.exists()); self.assertTrue(self.runtime.exists())
        self.manager.discard(identifier)
        with self.assertRaises(LauncherError): self.manager.start({'job_id':identifier,'confirmed':True})

    def test_uninstall_removes_package_preserves_saves_and_external_links_and_allows_reinstall(self):
        identifier = self.preview()
        self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'complete', self.manager.job)
        self.assertFalse(self.runtime.exists()); self.assertFalse(self.game.exists())
        backup = Path(self.manager.job['backup_path'])
        self.assertEqual((backup / 'private/local-saves/save').read_text(), 'precious progress')
        self.assertEqual((backup / 'local/msfs-prefix/user.reg').read_text(), 'old user settings')
        self.assertEqual((self.foreign / 'keep').read_text(), 'untouched')
        self.assertIsNone(self.launcher.runtime)
        self.assertEqual(self.launcher.known_runtimes,{})
        self.assertIsNone(Launcher(self.root / 'state').runtime)
        self.runtime.mkdir()  # the original installation path is reusable

    def test_uninstall_can_delete_local_data_but_never_follows_links(self):
        identifier = self.preview(keep_data=False)
        self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'complete')
        self.assertIsNone(self.manager.job['backup_path'])
        self.assertFalse(list(self.root.glob('runtime.uninstalled-*')))
        self.assertTrue((self.foreign / 'keep').is_file())

    def test_keep_packages_detaches_without_deleting_game_or_user_data(self):
        identifier = self.preview(delete_packages=False)
        self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'complete')
        self.assertTrue(self.game.exists())
        self.assertTrue(Path(self.manager.job['backup_path']).exists())
        with self.assertRaises(LauncherError):
            self.manager.preview({'operation':'uninstall','keep_data':False,'delete_packages':False})

    def test_previous_internal_game_package_is_removed_from_backup_too(self):
        previous = game(self.runtime / 'private/game-updates/update-test/game')
        (self.runtime / 'games' / ('.MSFS2024-before-'+'a'*32)).symlink_to(previous)
        identifier = self.preview()
        self.assertEqual(len(self.manager.job['packages']), 2)
        self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'complete',self.manager.job)
        backup = Path(self.manager.job['backup_path'])
        self.assertEqual(list((backup/'private/game-updates/update-test').iterdir()), [])

    def test_changed_preview_rejected_before_any_removal(self):
        identifier = self.preview()
        (self.game/'new-important-file').write_text('keep')
        self.run_job(identifier)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertTrue(self.game.exists()); self.assertTrue(self.runtime.exists())
        self.assertFalse(self.launcher.setup_busy)

    def test_other_runtime_and_external_process_lock_block_execution(self):
        identifier = self.preview()
        with self.launcher.runtime_lock():
            with self.assertRaises(LauncherError): self.manager.start({'job_id':identifier,'confirmed':True})
        self.launcher.runtime = self.root/'elsewhere'
        with self.assertRaises(LauncherError): self.manager.start({'job_id':identifier,'confirmed':True})
        self.assertTrue(self.game.exists())

    def test_publishing_failure_rolls_back_all_renames(self):
        identifier = self.preview()
        original_write = maintenance.atomic_json
        def fail_configuration(path, value):
            if path == self.launcher.config_file:
                raise OSError('test configuration failure')
            return original_write(path, value)
        with patch.object(maintenance,'atomic_json',side_effect=fail_configuration):
            self.run_job(identifier)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertTrue(self.game.exists()); self.assertTrue(self.runtime.exists())
        self.assertEqual(self.launcher.runtime,self.runtime)
        self.assertFalse(list(self.root.glob('.flightdeck-remove-*')))
        self.assertFalse((self.runtime/'private/uninstalled.json').exists())

    def test_cleanup_failure_keeps_recovery_locations_and_saved_data(self):
        identifier = self.preview()
        with patch.object(maintenance.shutil, 'rmtree', side_effect=OSError('test cleanup failure')) as removal:
            removal.avoids_symlink_attacks = True
            self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'failed')
        archive = Path(self.manager.job['backup_path'])
        record = json.loads((archive/'private/uninstalled.json').read_text())
        self.assertEqual(record['state'], 'detached')
        self.assertEqual(record['original_runtime'], str(self.runtime))
        self.assertTrue(all(Path(p).is_dir() for p in record['cleanup']))
        self.assertEqual((archive/'private/local-saves/save').read_text(), 'precious progress')

    def test_symlinked_runtime_subdirectory_never_deletes_target(self):
        # A package-level link is explicitly reviewed; nested links are never followed.
        (self.game/'nested').symlink_to(self.foreign,target_is_directory=True)
        self.run_job(self.preview(keep_data=False))
        self.assertEqual((self.foreign/'keep').read_text(),'untouched')

    def test_shared_package_cannot_be_deleted(self):
        other=self.root/'other';(other/'games').mkdir(parents=True)
        (other/'games/MSFS2024').symlink_to(self.game)
        self.launcher.known_runtimes['synthetic_other']=other
        self.manager.preview({'operation':'uninstall'})
        self.manager.thread.join(5)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertIn('andere Installation',self.manager.job['error'])
        self.assertTrue(self.game.exists())

    def test_package_shared_after_preview_cannot_be_deleted(self):
        identifier = self.preview()
        other = self.root/'other'
        (other/'games').mkdir(parents=True)
        (other/'games/MSFS2024').symlink_to(game(self.root/'other-game'))
        (other/'games'/('.MSFS2024-before-'+'a'*32)).symlink_to(self.game)
        self.launcher.known_runtimes['synthetic_other'] = other
        self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'failed')
        self.assertIn('andere Installation', self.manager.job['error'])
        self.assertTrue(self.game.exists())
        self.assertTrue(self.runtime.exists())

    def test_reset_preserves_game_and_saves_and_can_be_undone(self):
        identifier=self.preview('reset')
        with patch.object(maintenance.bootstrap,'prepare_prefix',side_effect=self.prepare_prefix): self.run_job(identifier)
        self.assertEqual(self.manager.job['state'],'complete',self.manager.job)
        self.assertEqual((self.prefix/'user.reg').read_text(),'fresh user settings')
        self.assertEqual((Path(self.manager.job['backup_path'])/'user.reg').read_text(),'old user settings')
        self.assertTrue(self.game.exists())
        self.assertEqual((self.runtime/'private/local-saves/save').read_text(),'precious progress')
        self.assertTrue(self.manager.snapshot()['can_restore'])
        self.run_job(self.preview('restore'))
        self.assertEqual(self.manager.job['state'],'complete',self.manager.job)
        self.assertEqual((self.prefix/'user.reg').read_text(),'old user settings')
        self.assertFalse(self.manager.snapshot()['can_restore'])

    def test_reset_preparation_failure_keeps_old_environment_active(self):
        identifier=self.preview('reset')
        with patch.object(maintenance.bootstrap,'prepare_prefix',side_effect=OSError('synthetic')): self.run_job(identifier)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertEqual((self.prefix/'user.reg').read_text(),'old user settings')
        self.assertFalse(self.launcher.setup_busy)

    def test_reset_preserves_in_prefix_downloaded_packages(self):
        packages=self.prefix/'drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024/Packages'
        (packages/'Community').mkdir(parents=True)
        (packages/'content').write_text('large downloaded content')
        with patch('flightdeck.mods._locations',return_value=([(packages/'Community','usercfg'),
                   (packages/'Community','runtime_config'), (packages/'nested/Community','usercfg')],False)):
            identifier=self.preview('reset')
            with patch.object(maintenance.bootstrap,'prepare_prefix',side_effect=self.prepare_prefix): self.run_job(identifier)
        self.assertEqual(self.manager.job['state'],'complete',self.manager.job)
        self.assertEqual((packages/'content').read_text(),'large downloaded content')
        self.assertTrue(packages.is_symlink())

    def test_reset_does_not_write_package_links_through_new_prefix_links(self):
        packages = self.prefix/'drive_c/users/steamuser/Packages'
        (packages/'Community').mkdir(parents=True)
        def linked_users(runner, prefix, **kwargs):
            self.prepare_prefix(runner, prefix, **kwargs)
            (prefix/'drive_c/users').symlink_to(self.foreign, target_is_directory=True)
        with patch('flightdeck.mods._locations', return_value=([(packages/'Community','usercfg')],False)):
            identifier = self.preview('reset')
            with patch.object(maintenance.bootstrap, 'prepare_prefix', side_effect=linked_users):
                self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'failed')
        self.assertEqual((self.prefix/'user.reg').read_text(), 'old user settings')
        self.assertEqual([p.name for p in self.foreign.iterdir()], ['keep'])

    def test_fenix_requires_explicit_existing_restore_first(self):
        (self.runtime/'private/fenix-linux-patch.json').write_text('{}')
        self.manager.preview({'operation':'reset'});self.manager.thread.join(5)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertIn('Fenix',self.manager.job['error'])

    def test_reset_never_writes_through_linked_local_directory(self):
        local=self.runtime/'local';local.rename(self.root/'original-local')
        local.symlink_to(self.root/'original-local',target_is_directory=True)
        self.manager.preview({'operation':'reset'});self.manager.thread.join(5)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertEqual((self.root/'original-local/msfs-prefix/user.reg').read_text(),'old user settings')

    def test_interrupted_reset_after_exchange_can_still_be_restored(self):
        identifier=self.preview('reset')
        original_replace=maintenance.os.replace
        def interrupt(source,target):
            if str(source).endswith('environment-reset-pending.json'): raise OSError('interrupted journal publication')
            return original_replace(source,target)
        with patch.object(maintenance.bootstrap,'prepare_prefix',side_effect=self.prepare_prefix), patch.object(maintenance.os,'replace',side_effect=interrupt):
            self.run_job(identifier)
        self.assertEqual(self.manager.job['state'],'failed')
        self.assertTrue(self.manager.snapshot()['can_restore'])
        self.run_job(self.preview('restore'))
        self.assertEqual((self.prefix/'user.reg').read_text(),'old user settings')

    def test_existing_wine_process_and_active_cloud_sync_block_maintenance(self):
        with patch.object(maintenance,'_idle',side_effect=LauncherError('Close Wine')):
            self.manager.preview({'operation':'reset'});self.manager.thread.join(5)
        self.assertEqual(self.manager.job['state'],'failed')
        auto=self.launcher.cloud_saves.automation
        auto.runtime=self.runtime;auto.state='syncing'
        with self.assertRaises(LauncherError): self.manager.preview({'operation':'uninstall'})

    def test_finished_cloud_failure_does_not_trap_reset_or_remove_saved_recovery(self):
        auto = self.launcher.cloud_saves.automation
        auto.runtime = self.runtime
        auto.state = 'attention'
        auto.error_code = 'graphics'
        journal = self.runtime/'private/synthetic-cloud-recovery.json'
        journal.write_text('{"pending":"synthetic recovery"}')
        # Other operations still require explicit cloud recovery.
        with self.assertRaises(LauncherError): self.launcher._require_cloud_idle()
        identifier = self.preview('reset')
        with patch.object(maintenance.bootstrap,'prepare_prefix',side_effect=self.prepare_prefix):
            self.run_job(identifier)
        self.assertEqual(self.manager.job['state'], 'complete', self.manager.job)
        self.assertEqual(auto.state, 'attention')
        self.assertEqual(journal.read_text(), '{"pending":"synthetic recovery"}')
        self.assertEqual((self.runtime/'private/local-saves/save').read_text(), 'precious progress')
        with patch.object(auto, 'worker') as worker:
            worker.is_alive.return_value = True
            with self.assertRaises(LauncherError): self.manager.preview({'operation':'reset'})
