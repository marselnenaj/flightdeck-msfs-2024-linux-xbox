"""Graphics evidence using synthetic libraries/registries, never a real game."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import graphics_diagnostics as gd
from tests.test_graphics import NVIDIA, IGPU


class GraphicsEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.runtime = Path(self.temp.name)/'runtime'
        self.prefix = self.runtime/'local/msfs-prefix'
        self.system = self.prefix/'drive_c/windows/system32'
        self.system.mkdir(parents=True)
        (self.runtime/'private').mkdir(mode=0o700)
        self.runner = self.runtime/'runner/files/lib/wine'
        for name, (component, library) in gd.LIBRARIES.items():
            source = self.runner/component/'x86_64-windows'/(library+'.dll')
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_bytes(('synthetic '+component+name).encode())
            (self.system/(name+'.dll')).write_bytes(source.read_bytes())
        self.at = '2026-09-26T10:20:30.123456+00:00'

    def record(self):
        return gd.launch_record(self.runtime, {'nvidia':'ready','ngx_available':True,'devices':[NVIDIA, IGPU]},
            {'DXVK_FILTER_DEVICE_NAME':NVIDIA['name'], 'VKD3D_FILTER_DEVICE_NAME':NVIDIA['name'],
             'DXVK_ENABLE_NVAPI':'1','WINEDLLOVERRIDES':'nvapi64=n;dxgi=n,b'}, state='spawned', at=self.at)

    def test_native_libraries_are_distinguished_from_wine_builtin_custom_and_missing(self):
        builtin = self.runner/'x86_64-windows/dxgi.dll'
        builtin.parent.mkdir()
        builtin.write_bytes(b'synthetic Wine builtin DXGI')
        (self.system/'dxgi.dll').write_bytes(builtin.read_bytes())
        (self.system/'d3d12.dll').write_bytes(b'user supplied d3d12')
        (self.system/'nvapi64.dll').unlink()
        (self.system/'nvngx.dll').write_bytes(b'synthetic host NGX')
        before = {p:p.read_bytes() for p in self.system.iterdir()}
        result = gd.prefix_summary(self.runtime)
        self.assertEqual(result['status'], 'inspected')
        self.assertEqual(result['libraries']['dxgi'], 'wine_builtin')
        self.assertEqual(result['libraries']['d3d12'], 'different')
        self.assertEqual(result['libraries']['d3d12core'], 'matches_runner')
        self.assertEqual(result['libraries']['nvapi64'], 'missing')
        self.assertEqual(result['libraries']['nvngx'], 'present')
        self.assertEqual(before, {p:p.read_bytes() for p in self.system.iterdir()})
        self.assertNotIn(str(self.runtime), json.dumps(result))

    def test_only_graphics_override_modes_are_exported_from_global_and_game_registry(self):
        (self.prefix/'user.reg').write_text(r'''WINE REGISTRY Version 2
[Software\\Wine\\DllOverrides] 123
"dxgi"="native"
"token"="synthetic-secret"
"unrelated.dll"="synthetic-secret"
[Software\\Wine\\AppDefaults\\FlightSimulator2024.exe\\DllOverrides] 456
"dxgi"="builtin"
"d3d12"="synthetic-secret"
[Software\\Private]
"nvapi64"="native"
''')
        (self.prefix/'system.reg').write_text(r'''WINE REGISTRY Version 2
[Software\\Wine\\DllOverrides]
"d3d12"="native,builtin"
''')
        result = gd.prefix_summary(self.runtime)
        self.assertEqual(result['user_overrides'], {'status':'read','global':{'dxgi':'native'},
                          'game':{'dxgi':'builtin','d3d12':'other'}})
        self.assertEqual(result['system_overrides']['global'], {'d3d12':'native,builtin'})
        self.assertNotIn('synthetic-secret', json.dumps(result))
        self.assertNotIn('nvapi64', result['user_overrides']['game'])

    def test_environment_last_override_wins_without_exporting_arbitrary_names_or_values(self):
        result = gd.environment_overrides({'WINEDLLOVERRIDES':'path-secret=n;DXGI.DLL=b;dxgi=n,b;'
                                          '*nvapi64=n;nvapi64,*nvapi64=;d3d12=synthetic-secret'})
        self.assertEqual(result, {'dxgi':'native,builtin','*nvapi64':'disabled','nvapi64':'disabled','d3d12':'other'})

    def test_launch_record_survives_a_new_reader_and_excludes_environment_paths_and_tokens(self):
        record = self.record()
        self.assertTrue(gd.save_launch(self.runtime, record))
        saved = gd.load_launch(self.runtime)
        self.assertEqual(saved['state'], 'spawned')
        self.assertEqual(saved['at'], self.at)
        self.assertEqual(saved['gpu_filters'], {'DXVK_FILTER_DEVICE_NAME':'nvidia','VKD3D_FILTER_DEVICE_NAME':'nvidia'})
        self.assertEqual(saved['dll_overrides'], {'nvapi64':'native','dxgi':'native,builtin'})
        other = gd.launch_record(self.runtime, {'nvidia':'disabled'},
            {'DXVK_FILTER_DEVICE_UUID':'synthetic-private-uuid','NVIDIA_WINE_DLL_DIR':'/synthetic/private',
             'ACCESS_TOKEN':'synthetic-secret','DXVK_FILTER_DEVICE_NAME':'synthetic-secret','DXVK_ENABLE_NVAPI':'0'},
            state='prepared', at=self.at)
        self.assertEqual(other['gpu_filters'], {'DXVK_FILTER_DEVICE_NAME':'custom','DXVK_FILTER_DEVICE_UUID':'custom'})
        self.assertNotIn('synthetic-', json.dumps(other))

    def test_nvidia_profile_and_effective_vendor_hiding_survive_service_restart(self):
        record = gd.launch_record(self.runtime, {"nvidia": "disabled", "nvidia_mode": "compatibility"},
            {"WINE_HIDE_NVIDIA_GPU": "1", "DXVK_ENABLE_NVAPI": "0", "DXVK_CONFIG": "private-unexported"},
            state="spawned", at=self.at)
        self.assertTrue(gd.save_launch(self.runtime, record))
        saved = gd.load_launch(self.runtime)
        self.assertEqual(saved["nvidia_mode"], "compatibility")
        self.assertTrue(saved["hide_nvidia"])
        self.assertNotIn("private-unexported", json.dumps(saved))
        self.assertNotIn('schema', saved)

    def test_modified_marker_cannot_add_raw_data_to_export(self):
        record = self.record()
        record['token'] = 'synthetic-private-token'
        gd.save_launch(self.runtime, record)
        self.assertNotIn('synthetic-private-token', json.dumps(gd.load_launch(self.runtime)))
        for field, value in [('state','synthetic-private-token'), ('nvidia',{}),
                             ('gpu_filters',{'DXVK_FILTER_DEVICE_NAME':'synthetic-private-token'}),
                             ('dll_overrides',{'dxgi':[]}), ('at','not-a-timestamp'), ('launcher_version','arbitrary')]:
            with self.subTest(field=field):
                gd.save_launch(self.runtime, {**self.record(), field:value})
                self.assertIsNone(gd.load_launch(self.runtime))

    def test_special_files_and_linked_prefixes_are_not_read_as_regular_files(self):
        target = self.system/'dxgi.dll'
        target.unlink(); os.mkfifo(target)
        os.mkfifo(self.prefix/'user.reg')
        result = gd.prefix_summary(self.runtime)
        self.assertEqual(result['libraries']['dxgi'], 'unavailable')
        self.assertEqual(result['user_overrides']['status'], 'unavailable')
        local = self.runtime/'local'
        local.rename(self.runtime/'original-local')
        local.symlink_to(self.runtime/'original-local', target_is_directory=True)
        self.assertEqual(gd.prefix_summary(self.runtime), {'status':'unavailable'})

    def test_marker_links_and_oversized_files_are_rejected_and_save_never_follows_private_link(self):
        marker = self.runtime/'private'/gd.MARKER
        secret = Path(self.temp.name)/'secret'
        secret.write_text('synthetic-secret')
        marker.symlink_to(secret)
        self.assertIsNone(gd.load_launch(self.runtime))
        self.assertTrue(gd.save_launch(self.runtime, self.record()))
        self.assertEqual(secret.read_text(), 'synthetic-secret')
        marker.write_bytes(b' ' * 8193)
        self.assertIsNone(gd.load_launch(self.runtime))
        marker.unlink(); os.mkfifo(marker)
        self.assertIsNone(gd.load_launch(self.runtime))
        private = self.runtime/'private'
        private.rename(self.runtime/'original-private')
        private.symlink_to(self.runtime/'original-private', target_is_directory=True)
        self.assertFalse(gd.save_launch(self.runtime, self.record()))
        self.assertIsNone(gd.load_launch(self.runtime))

    def test_failed_diagnostic_write_is_nonfatal_and_does_not_leave_temporary_files(self):
        with patch.object(gd.os, 'replace', side_effect=OSError('synthetic disk error')):
            self.assertFalse(gd.save_launch(self.runtime, self.record()))
        self.assertEqual(list((self.runtime/'private').iterdir()), [])

    def test_log_evidence_is_explicitly_partial_and_contains_only_known_symbols(self):
        result = gd.log_summary('''trace:private Bearer synthetic-secret
info:  DXVK: v2.7
123.12:009c:info:vkd3d-proton:vkd3d_create_device: synthetic-path
err: VK_ERROR_DEVICE_LOST: user synthetic-secret
DXGI_ERROR_DEVICE_HUNG VK_ERROR_SYNTHETIC_PRIVATE_TOKEN
DXVK-NVAPI synthetic-private-id
''')
        self.assertEqual(result, {'scope':'bounded_log_excerpt',
                         'observed_components':['vkd3d-proton','dxvk','dxvk-nvapi'],
                         'error_symbols':['DXGI_ERROR_DEVICE_HUNG','VK_ERROR_DEVICE_LOST']})
        self.assertNotIn('synthetic', json.dumps(result))
        self.assertEqual(gd.log_summary(''), {'scope':'bounded_log_excerpt','observed_components':[], 'error_symbols':[]})
