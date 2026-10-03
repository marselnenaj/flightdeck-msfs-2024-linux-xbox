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
from contextlib import ExitStack

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

    def test_switches_and_return_carry_settings_forward_and_retain_backups(self):
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
        self.assertEqual((self.prefix / 'important-setting').read_text(), 'experimental setting')
        (self.prefix / 'important-setting').write_text('GE setting')
        # Native cloud-save helpers continue to use the original Wine runner.
        self.assertEqual(proton.base_runner(self.runtime), self.original)
        self.assertEqual(self.select()['state'], 'complete')
        self.assertIsNone(proton.selection(self.runtime))
        self.assertEqual(proton._id(self.runtime / first['base_prefix']), original_id)
        self.assertEqual(proton._version(self.runtime / 'runner'), 'Flightdeck-Xodus')
        self.assertEqual((self.prefix / 'important-setting').read_text(), 'GE setting')
        saved = {p.read_text() for p in (self.runtime / 'local/proton-tests').glob('*/previous-prefix/important-setting')}
        self.assertEqual(saved, {'original', 'experimental setting', 'GE setting'})
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
        self.assertEqual(proton._id(fresh), before)
        self.assertEqual(proton._version(self.runtime / 'runner'), 'Flightdeck-Xodus')

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

    def test_gsx_can_be_prepared_on_proton_and_survives_later_switches(self):
        from flightdeck import gsx_core
        self.assertEqual(self.select(self.experimental)['state'], 'complete')
        self.assertEqual(gsx_core.proton_error(self.runtime), '')
        def prepare(root, *args):
            self.assertEqual(proton._version(root / 'runner'), 'experimental-11')
            (self.prefix / 'gsx-installed-later').write_text('GSX package and settings')
            (root / gsx_core.MARKER).write_text(json.dumps({'format': 1, 'id': 'a' * 32, 'state': 'ready'}))
        with patch.object(gsx_core, 'prepare', side_effect=prepare) as setup:
            self.launcher.gsx.start('prepare', {'runtime_path': str(self.runtime)})
            self.launcher.gsx.worker.join(10)
        setup.assert_called_once()
        self.assertFalse(self.launcher.setup_busy)
        self.assertEqual(self.select(self.ge)['state'], 'complete')
        self.assertEqual(self.select()['state'], 'complete')
        self.assertEqual((self.prefix / 'gsx-installed-later').read_text(), 'GSX package and settings')
        self.assertTrue(gsx_core.setup_complete(self.runtime))

    def test_interrupted_gsx_setup_must_be_recovered_before_switching(self):
        from flightdeck import gsx_core
        marker = self.runtime / gsx_core.MARKER
        marker.write_text(json.dumps({'format': 1, 'id': 'a' * 32, 'state': 'preparing',
                                      'original_prefix_id': proton._id(self.prefix), 'prior': None}))
        with self.assertRaisesRegex(LauncherError, 'GSX-Einrichtung'):
            self.select(self.experimental)
        self.assertFalse(self.launcher.setup_busy)
        gsx_core.recover(self.runtime)
        self.assertEqual(self.select(self.experimental)['state'], 'complete')

    def test_internal_addon_links_stay_in_the_trial_during_preparation_and_after_switch(self):
        package = self.prefix / 'drive_c/Addon Manager/GSX'
        package.mkdir(parents=True)
        (package / 'marker').write_text('original add-on')
        link = self.prefix / 'drive_c/Community/GSX'
        link.parent.mkdir()
        link.symlink_to(package, target_is_directory=True)
        with patch.object(bootstrap, '_command'):
            fresh, selected = proton._prepare(self.runtime, proton.inspect(str(self.experimental)), None, lambda _: None)
        relative = link.relative_to(self.prefix)
        self.assertEqual((fresh / relative).resolve(), fresh / package.relative_to(self.prefix))
        (fresh / relative / 'marker').write_text('trial add-on')
        self.assertEqual((package / 'marker').read_text(), 'original add-on')
        proton._switch(self.runtime, fresh, selected)
        self.assertEqual((link / 'marker').read_text(), 'trial add-on')
        self.assertEqual(self.select()['state'], 'complete')
        self.assertEqual((link / 'marker').read_text(), 'trial add-on')

    def fenix_fixture(self, *, legacy=False):
        """Real manifest matching/overlay/journal code, synthetic Wine files."""
        from flightdeck import fenix
        core = fenix.core
        bundle = self.base / 'fenix-bundle'
        integration = bundle / 'integration'
        integration.mkdir(parents=True)
        for name in ('launch-msfs.sh', 'xodus-wine-launch'):
            shutil.copy2(self.runtime / 'tools' / name, integration / name)
        (integration / 'FenixWindowGuard.exe').write_bytes(b'synthetic helper')
        def entry(source, variant=None):
            relative = 'files/lib/wine/x86_64-windows/ntdll.dll'
            target = bundle / 'payload'
            if variant:
                target /= 'variants/' + variant
            target /= relative
            target.parent.mkdir(parents=True)
            target.write_bytes(('patched ' + source.name).encode())
            return {'runner_version': (source / 'version').read_text(),
                    'runner_files': {'files/bin/wine': proton.digest(source / 'files/bin/wine')},
                    'files': {relative: proton.digest(target)}}
        lock = {'format': 1, 'version': 'synthetic-proton', **entry(self.original),
                'variants': {'experimental': entry(self.experimental, 'experimental'),
                             'ge': entry(self.ge, 'ge')},
                'integration': {p.name: proton.digest(p) for p in integration.iterdir()},
                'accepted_scripts': {n: [proton.digest(integration / n)] for n in ('launch-msfs.sh', 'xodus-wine-launch')},
                'prefix_files': {'drive_c/windows/system32/FenixWindowGuard.exe': proton.digest(integration / 'FenixWindowGuard.exe')}}
        (bundle / 'bundle.json').write_text(json.dumps(lock))
        context = ExitStack()
        self.addCleanup(context.close)
        context.enter_context(patch.object(core, 'ROOT', bundle))
        context.enter_context(patch.object(fenix, 'obtain_bundle', return_value=bundle))
        context.enter_context(patch.object(core, 'Wine'))
        context.enter_context(patch.object(core, 'prepare_geometry'))
        context.enter_context(patch.object(core, 'has_framework', return_value=True))
        work = self.runtime / 'local/fenix-patch-20261003T120000-12345678'
        shutil.copytree(self.original, work / 'runner')
        core.apply_overlay(self.prefix, work / 'runner', bundle)
        (self.runtime / 'runner').unlink()
        (self.runtime / 'runner').symlink_to(work / 'runner')
        marker = self.private / ('fenix-compat.json' if legacy else 'fenix-linux-patch.json')
        marker.write_text(json.dumps({'state': 'installed', 'version': lock['version'],
                         'work': str(work.relative_to(self.runtime)), 'configured': True,
                         'backup': 'private/original-fenix-backup', 'proton_before': None}))
        return core, bundle

    def test_fenix_uses_matching_modules_and_preserves_addons_on_every_switch(self):
        core, bundle = self.fenix_fixture()
        for candidate, variant in ((self.experimental, 'experimental'), (self.ge, 'ge'), (None, None)):
            with self.subTest(variant=variant):
                (self.prefix / 'addon-state').write_text('new aircraft, login and GSX settings')
                result = self.select(candidate)
                self.assertEqual(result['state'], 'complete', result)
                state = core.read_json(self.runtime / core.MARKER)
                self.assertEqual(state['variant'], variant)
                core.verify_installed(self.runtime, state)
                self.assertEqual((self.prefix / 'addon-state').read_text(), 'new aircraft, login and GSX settings')
                self.assertEqual((self.prefix / 'drive_c/windows/system32/ntdll.dll').read_bytes(),
                                 (self.runtime / 'runner/files/lib/wine/x86_64-windows/ntdll.dll').read_bytes())
        self.assertFalse((self.experimental / 'files/lib/wine/x86_64-windows/ntdll.dll').exists())

    def test_legacy_fenix_migrates_without_reinstalling_or_losing_its_profile(self):
        core, _ = self.fenix_fixture(legacy=True)
        (self.prefix / 'fenix-account-fixture').write_text('retain existing account state')
        result = self.select(self.experimental)
        self.assertEqual(result['state'], 'complete', result)
        state = core.read_json(self.runtime / core.MARKER)
        self.assertTrue(state['migrated'])
        core.verify_installed(self.runtime, state)
        self.assertEqual((self.prefix / 'fenix-account-fixture').read_text(), 'retain existing account state')

    def test_unknown_or_modified_fenix_runner_is_rejected_before_download(self):
        from flightdeck import fenix
        self.fenix_fixture()
        before = proton._id(self.prefix)
        (self.experimental / 'files/bin/wine').write_text('#!/bin/sh\nexit 1\n')
        with patch.object(fenix, 'obtain_bundle') as download:
            result = self.select(self.experimental)
        self.assertEqual(result['state'], 'failed')
        self.assertIn('Fenix', result['error'])
        self.assertEqual(proton._id(self.prefix), before)
        download.assert_not_called()

    def test_recovery_finishes_fenix_metadata_and_scripts_after_runner_switch(self):
        core, bundle = self.fenix_fixture()
        with patch.object(bootstrap, '_command'):
            fresh, selected = proton._prepare(self.runtime, proton.inspect(str(self.experimental)), None, lambda _: None, bundle=bundle)
        writer = proton.atomic_json
        def fail_marker(path, value):
            if path == self.runtime / core.MARKER:
                raise OSError('interrupted before Fenix metadata')
            return writer(path, value)
        with patch.object(proton, 'atomic_json', side_effect=fail_marker), self.assertRaises(OSError):
            proton._switch(self.runtime, fresh, selected)
        self.assertTrue(proton.check(self.runtime))
        with self.assertRaises(core.PatchError), core.locked(self.runtime):
            pass
        proton.recover(self.runtime)
        core.verify_installed(self.runtime, core.read_json(self.runtime / core.MARKER))
        self.assertFalse(proton.check(self.runtime))


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
