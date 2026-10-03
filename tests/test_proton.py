# SPDX-License-Identifier: MIT
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import bootstrap, graphics, proton, renderer
from flightdeck.backend import Launcher, LauncherError

ROOT = Path(__file__).resolve().parents[1]


def runner(root, version):
    (root / 'files/bin').mkdir(parents=True)
    (root / 'version').write_text('123456 ' + version)
    for name in ('wine', 'wineserver'):
        target = root / 'files/bin' / name
        target.write_text('#!/bin/sh\nexit 0\n')
        target.chmod(0o700)
    for architecture in ('x86_64-windows', 'i386-windows'):
        for library, names in (('dxvk', ('dxgi', 'd3d11', 'd3d10core')), ('vkd3d-proton', ('d3d12', 'd3d12core'))):
            folder = root / 'files/lib/wine' / library / architecture
            folder.mkdir(parents=True)
            for name in names:
                (folder / (name + '.dll')).write_bytes((version + name + architecture).encode())
    return root


class ProtonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.runtime = self.base / 'runtime'
        self.private = self.runtime / 'private'
        self.private.mkdir(parents=True)
        self.prefix = self.runtime / 'local/msfs-prefix'
        for relative in ('drive_c/windows/system32', 'drive_c/windows/syswow64'):
            (self.prefix / relative).mkdir(parents=True)
        for name in ('system.reg', 'user.reg', 'important-setting'):
            (self.prefix / name).write_text('original')
        system = self.prefix / 'drive_c/windows/system32'
        hashes = {}
        for name in proton.BRIDGE:
            data = ('synthetic ' + name).encode()
            (system / name).write_bytes(data)
            hashes[name] = hashlib.sha256(data).hexdigest()
        record = {'artifacts': {'files': {'runtime/xgameruntime.dll': hashes['xgameruntime.dll'],
                   'builtin/x86_64-windows/xodus_store_test.dll': hashes['xodus_store_test.dll']}},
                  'original_runtime_sha256': hashes['xgameruntime_original.dll']}
        (self.private / 'import-manifest.json').write_text(json.dumps(record))
        (self.private / 'runtime.json').write_text('{"game_id":"msfs2024","market":"AT"}')
        (self.private / 'local-saves').mkdir()
        (self.private / 'local-saves/keep').write_text('saved flight')
        shutil.copytree(ROOT / 'scripts/runtime', self.runtime / 'tools')
        self.original = runner(self.base / 'base-runner', 'Flightdeck-Xodus')
        (self.runtime / 'runner').symlink_to(self.original, target_is_directory=True)
        self.experimental = runner(self.base / 'Proton Experimental', 'experimental-11')
        self.ge = runner(self.base / 'GE-Proton', 'GE-11')
        self.launcher = Launcher(self.base / 'state', str(self.runtime))
        self.manager = self.launcher.proton

    def select(self, path=None):
        with patch.object(bootstrap, '_command'), patch.object(proton, '_idle'):
            self.manager.start({'runtime_path': str(self.runtime), 'mode': 'proton' if path else 'default', 'path': str(path) if path else ''})
            self.manager.thread.join(10)
        self.assertFalse(self.manager.thread.is_alive())
        return self.manager.snapshot()['job']

    def test_complete_runner_changes_and_return_preserve_original_and_every_trial(self):
        original_id = proton._id(self.prefix)
        self.assertEqual(self.select(self.experimental)['state'], 'complete')
        first = proton.selection(self.runtime)
        self.assertEqual(proton.base_runner(self.runtime), self.original)
        self.assertEqual(proton._id(self.runtime / first['base_prefix']), original_id)
        self.assertEqual((self.runtime / 'runner/version').read_text().strip(), 'experimental-11')
        (self.prefix / 'important-setting').write_text('experimental setting')
        self.assertEqual((self.experimental / 'version').read_text(), '123456 experimental-11')
        self.assertEqual(self.select(self.ge)['state'], 'complete')
        second = proton.selection(self.runtime)
        self.assertEqual(second['base_prefix'], first['base_prefix'])
        self.assertEqual((self.prefix / 'important-setting').read_text(), 'original')
        (self.prefix / 'important-setting').write_text('GE setting')
        # Native cloud-save helpers continue to use the original Wine runner.
        self.assertEqual(proton.base_runner(self.runtime), self.original)
        self.assertEqual(self.select()['state'], 'complete')
        self.assertIsNone(proton.selection(self.runtime))
        self.assertEqual(proton._id(self.prefix), original_id)
        self.assertEqual((self.runtime / 'runner').resolve(), self.original)
        self.assertEqual((self.prefix / 'important-setting').read_text(), 'original')
        saved = {p.read_text() for p in (self.runtime / 'local/proton-tests').glob('*/previous-prefix/important-setting')}
        self.assertEqual(saved, {'experimental setting', 'GE setting'})
        self.assertEqual((self.private / 'local-saves/keep').read_text(), 'saved flight')
        self.assertFalse((self.private / proton.JOURNAL).exists())

    def test_preparation_failure_leaves_prefix_runner_and_saves_untouched(self):
        before = proton._id(self.prefix)
        with patch.object(bootstrap, '_command', side_effect=proton.SetupError('failure')), patch.object(proton, '_idle'):
            self.manager.start({'runtime_path': str(self.runtime), 'mode': 'proton', 'path': str(self.experimental)})
            self.manager.thread.join(10)
        self.assertEqual(self.manager.job['state'], 'failed')
        self.assertFalse(self.launcher.setup_busy)
        self.assertEqual(proton._id(self.prefix), before)
        self.assertEqual((self.runtime / 'runner').resolve(), self.original)
        self.assertFalse((self.private / proton.SETTINGS).exists())
        self.assertEqual(list((self.runtime / 'local/proton-tests').iterdir()), [])

    def test_nvidia_start_preserves_the_selected_proton_renderer(self):
        self.assertEqual(self.select(self.experimental)['state'], 'complete')
        system = self.prefix / 'drive_c/windows/system32'
        before = {name: (system / name).read_bytes() for name in ('dxgi.dll', 'd3d11.dll', 'd3d12.dll')}
        device = {'name': 'NVIDIA test GPU', 'vendor_id': 0x10de, 'type': 2}
        with patch.object(graphics, 'nvidia_present', return_value=True), \
                patch.object(graphics, 'probe', return_value={'status': 'ready', 'devices': [device]}), \
                patch.object(renderer, 'install') as install:
            _, report = graphics.prepare(self.runtime, {})
        install.assert_not_called()
        self.assertEqual(report['nvidia_mode'], 'auto')
        self.assertEqual(before, {name: (system / name).read_bytes() for name in before})

    def test_recovery_after_prefix_swap_before_runner_publication(self):
        with patch.object(bootstrap, '_command'):
            fresh, selected = proton._prepare(self.runtime, proton.inspect(str(self.experimental)), None, lambda _: None)
        before = proton._id(self.prefix)
        original_replace = os.replace
        def fail_runner(source, target):
            if Path(target) == self.runtime / 'runner':
                raise OSError('simulated interruption')
            return original_replace(source, target)
        with patch.object(proton.os, 'replace', side_effect=fail_runner):
            with self.assertRaises(OSError):
                proton._switch(self.runtime, fresh, selected)
        self.assertTrue(proton.check(self.runtime))
        self.assertEqual(proton._id(fresh), before)
        self.assertEqual(self.select()['state'], 'complete')
        self.assertEqual(proton._id(self.prefix), before)
        self.assertEqual((self.runtime / 'runner').resolve(), self.original)

    def test_recovery_refuses_a_substituted_prefix(self):
        with patch.object(bootstrap, '_command'):
            fresh, selected = proton._prepare(self.runtime, proton.inspect(str(self.experimental)), None, lambda _: None)
        from flightdeck import game_update
        with patch.object(game_update, 'exchange', side_effect=OSError('before exchange')):
            with self.assertRaises(OSError):
                proton._switch(self.runtime, fresh, selected)
        old = self.prefix.with_name('user-moved-prefix')
        self.prefix.rename(old)
        self.prefix.mkdir()
        with self.assertRaises(ValueError):
            proton.recover(self.runtime)
        self.assertTrue((old / 'important-setting').exists())

    def test_fenix_busy_and_stale_runtime_cannot_start_a_switch(self):
        data = {'runtime_path': str(self.runtime), 'mode': 'proton', 'path': str(self.experimental)}
        (self.private / 'fenix-linux-patch.json').write_text('{}')
        with self.assertRaises(LauncherError):
            self.manager.start(data)
        (self.private / 'fenix-linux-patch.json').unlink()
        with self.assertRaises(LauncherError):
            self.manager.start({**data, 'runtime_path': '/another'})
        self.launcher.setup_busy = True
        with self.assertRaises(LauncherError):
            self.manager.start(data)
        self.assertIsNone(self.manager.job)

    def test_discovery_respects_extra_steam_libraries_and_deduplicates_links(self):
        home = self.base / 'home'
        steam = home / '.local/share/Steam'
        library = self.base / 'other library'
        tools = steam / 'compatibilitytools.d'
        tools.mkdir(parents=True)
        (tools / 'GE').symlink_to(self.ge)
        (tools / 'GE duplicate').symlink_to(self.ge)
        (steam / 'steamapps').mkdir()
        (library / 'steamapps/common').mkdir(parents=True)
        (library / 'steamapps/common/Proton - Experimental').symlink_to(self.experimental)
        (steam / 'steamapps/libraryfolders.vdf').write_text('"libraryfolders" { "1" { "path" "' + str(library) + '" } }')
        found = proton.discover(home)
        self.assertEqual({item['path'] for item in found}, {str(self.ge), str(self.experimental)})
        self.assertEqual(len(found), 2)

    def test_incomplete_graphics_stack_is_rejected(self):
        (self.ge / 'files/lib/wine/vkd3d-proton/x86_64-windows/d3d12core.dll').unlink()
        with self.assertRaises(OSError):
            proton.inspect(str(self.ge))

    def test_default_restore_does_not_require_healthy_trial_dlls(self):
        self.assertEqual(self.select(self.experimental)['state'], 'complete')
        (self.prefix / 'drive_c/windows/system32/xgameruntime.dll').write_bytes(b'broken trial')
        self.assertEqual(self.select()['state'], 'complete')
        self.assertEqual((self.prefix / 'drive_c/windows/system32/xgameruntime.dll').read_bytes(), b'synthetic xgameruntime.dll')

    def test_restore_does_not_write_through_a_substituted_backup_parent(self):
        self.assertEqual(self.select(self.experimental)['state'], 'complete')
        record = proton.selection(self.runtime)
        backup = self.runtime / record['base_prefix']
        original_parent = backup.parent
        moved = self.base / 'moved-by-user'
        original_parent.rename(moved)
        original_parent.symlink_to(moved, target_is_directory=True)
        before = (moved / 'previous-prefix/important-setting').read_bytes()
        self.assertEqual(self.select()['state'], 'failed')
        self.assertEqual((moved / 'previous-prefix/important-setting').read_bytes(), before)
        self.assertTrue((self.prefix / 'important-setting').exists())

    def test_classic_wine64_is_used_for_prefix_preparation(self):
        wine64 = self.ge / 'files/bin/wine64'
        wine64.write_text('#!/bin/sh\nexit 0\n')
        wine64.chmod(0o700)
        with patch.object(bootstrap, '_command') as command, patch.object(proton, '_idle'):
            self.manager.start({'runtime_path': str(self.runtime), 'mode': 'proton', 'path': str(self.ge)})
            self.manager.thread.join(10)
        self.assertEqual(self.manager.job['state'], 'complete')
        for call in command.call_args_list:
            args = call.args[0]
            if args[1] in ('wineboot', 'regedit'):
                self.assertEqual(args[0].name, 'wine64')
                self.assertEqual(call.kwargs['env']['WINE_DISABLE_FAST_SYNC'], '1')


class PortableLoaderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.loader = runpy.run_path(str(ROOT / 'scripts/runtime/xodus-wine-launch'))

    def test_maps_main_and_nested_dll_without_writing_decrypted_payloads(self):
        with tempfile.TemporaryDirectory() as directory:
            game = Path(directory)
            (game / 'nested').mkdir()
            (game / 'FlightSimulator2024.exe').write_bytes(b'encrypted exe')
            (game / 'nested/library.dll').write_bytes(b'encrypted dll')
            (game / 'data').write_text('unchanged resource')
            target = game / '.xodus-launch-test'
            target.mkdir()
            fds = [os.memfd_create('synthetic-probe') for _ in range(2)]
            try:
                for fd, data in zip(fds, (b'own executable', b'own library')):
                    os.write(fd, data)
                mappings = [(fd, self.loader['nt_path'](game / name)) for fd, name in zip(fds, ('FlightSimulator2024.exe', 'nested/library.dll'))]
                self.loader['portable_tree'](game, target, mappings)
                self.assertEqual((target / 'FlightSimulator2024.exe').read_bytes(), b'own executable')
                self.assertEqual((target / 'nested/library.dll').read_bytes(), b'own library')
                self.assertTrue((target / 'FlightSimulator2024.exe').is_symlink())
                self.assertEqual((game / 'FlightSimulator2024.exe').read_bytes(), b'encrypted exe')
                self.assertEqual((game / 'nested/library.dll').read_bytes(), b'encrypted dll')
                self.assertEqual((target / 'data').read_text(), 'unchanged resource')
            finally:
                for fd in fds:
                    os.close(fd)

    def test_rejects_outside_duplicate_and_unrepresented_image_mappings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, paths in [('outside', [root.parent / 'outside.dll']),
                                ('missing', [root / 'missing/f.dll']),
                                ('duplicate', [root / 'x.dll', root / 'X.dll'])]:
                with self.subTest(name=name):
                    target = root / ('.xodus-launch-' + name)
                    target.mkdir()
                    with self.assertRaises(RuntimeError):
                        self.loader['portable_tree'](root, target, [(1, self.loader['nt_path'](path)) for path in paths])
