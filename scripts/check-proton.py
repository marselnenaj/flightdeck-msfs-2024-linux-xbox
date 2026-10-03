#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Test the Flightdeck runner or Proton switching with own PEs and fresh prefixes."""
import argparse
import atexit
import http.client
import re
import selectors
import signal
import time
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from flightdeck import bootstrap, proton, setup
from flightdeck.backend import Launcher


class NativeLauncher:
    def __init__(self, binary, output, runtime):
        self.child = subprocess.Popen([str(binary.resolve(strict=True)), '--state-dir', str(output / 'native-state'),
            '--runtime', str(runtime), '--no-browser'], stdout=subprocess.PIPE, stderr=(output / 'native-service.log').open('wb'))
        atexit.register(self.close)
        with selectors.DefaultSelector() as selector:
            selector.register(self.child.stdout, selectors.EVENT_READ)
            assert selector.select(15), 'Native service did not start'
            line = self.child.stdout.readline().decode()
        match = re.fullmatch(r'Flightdeck: http://127\.0\.0\.1:(\d+)\n', line)
        assert match, 'Unexpected native service output'
        self.port = int(match[1])
        self.token = self.request('/api/status')['csrf_token']

    def request(self, path, body=None):
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=15)
        try:
            headers = {'Content-Type': 'application/json'}
            if body is not None:
                headers['X-Flightdeck-Token'] = self.token
            connection.request('POST' if body is not None else 'GET', path,
                               body=json.dumps(body) if body is not None else None, headers=headers)
            reply = connection.getresponse()
            result = json.loads(reply.read())
            assert reply.status == 200, result
            return result
        finally:
            connection.close()

    def switch(self, runtime, mode, path=None):
        self.request('/api/proton/select', {'runtime_path': str(runtime), 'mode': mode, 'path': str(path or '')})
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            job = self.request('/api/proton')['job']
            if job and job['state'] in ('complete', 'failed', 'cancelled'):
                assert job['state'] == 'complete', job
                return
            time.sleep(.2)
        raise RuntimeError('Native Proton preparation timed out')

    def close(self):
        if self.child.poll() is None:
            self.child.send_signal(signal.SIGINT)
            try:
                self.child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait()
        self.child.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', type=Path, required=True, help='Read only: source of the installed Store DLLs and base runner')
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--runner', type=Path, help='Exercise selection and return from this installed Proton')
    mode.add_argument('--default-runner', action='store_true', help='Exercise the default Flightdeck runner and native memfd mapping')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--fenix-bundle', type=Path, help='Test matching Fenix modules with synthetic apps; no licensed Fenix application')
    parser.add_argument('--native-launcher', type=Path, help='Exercise Rust HTTP Proton jobs and the Rust memfd loader')
    args = parser.parse_args()
    original = args.runtime.resolve(strict=True)
    runner = proton.base_runner(original).resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    runtime = output / 'runtime'
    (runtime / 'private').mkdir(parents=True, mode=0o700)
    (runtime / 'local').mkdir()
    (runtime / 'runner').symlink_to(runner)
    shutil.copytree(ROOT / 'scripts/runtime', runtime / 'tools')
    for path in (runtime / 'tools').iterdir():
        path.chmod(0o700)
    (runtime / 'private/runtime.json').write_text('{"game_id":"msfs2024","market":"AT"}\n')
    prefix = runtime / 'local/msfs-prefix'
    bootstrap.prepare_prefix(runner, prefix)
    source = setup.prefix_system32(original / 'local/msfs-prefix')
    system = setup.prefix_system32(prefix)
    hashes = {}
    for name in proton.BRIDGE:
        setup._copy_file(source / name, system / name)
        hashes[name] = setup.digest(system / name)
    shutil.copytree(original / 'local/store-runtime', runtime / 'local/store-runtime', symlinks=True)
    manifest = {'original_runtime_sha256': hashes['xgameruntime_original.dll'], 'artifacts': {'files': {
        'runtime/xgameruntime.dll': hashes['xgameruntime.dll'],
        'builtin/x86_64-windows/xodus_store_test.dll': hashes['xodus_store_test.dll']}}}
    (runtime / 'private/import-manifest.json').write_text(json.dumps(manifest))
    (prefix / 'original-marker').write_text('preserve this profile')
    before = proton._id(prefix)
    exe = output / 'probe.exe'
    dll = output / 'probe.dll'
    dll_source = output / 'dll.c'
    dll_source.write_text('__declspec(dllexport) int probe(void) { return 42; }\n')
    subprocess.run(['x86_64-w64-mingw32-gcc', '-O2', '-Wall', '-Wextra', str(ROOT / 'tests/graphics/proton-loader.c'), '-o', str(exe)], check=True)
    subprocess.run(['x86_64-w64-mingw32-gcc', '-shared', str(dll_source), '-o', str(dll)], check=True)
    graphics = output / 'multiwindow.exe'
    subprocess.run(['x86_64-w64-mingw32-gcc', '-O2', '-Wall', '-Wextra', '-Werror', str(ROOT / 'tests/graphics/multiwindow.c'), '-o', str(graphics),
                    '-ld3d12', '-ld3d11', '-ldxgi', '-ldxguid', '-lgdi32'], check=True)
    game = runtime / 'games/MSFS2024'
    (game / 'nested').mkdir(parents=True)
    names = ('FlightSimulator2024.exe', 'nested/probe.dll')
    for name in names:
        (game / name).write_bytes(b'Synthetic encrypted placeholder; must remain unchanged.\n')
    expected = {name: setup.digest(game / name) for name in names}
    launcher = Launcher(output / 'state', str(runtime))
    if args.fenix_bundle:
        from flightdeck.fenix import core
        bundle = core.verify_bundle(args.fenix_bundle)
        work = runtime / 'local/fenix-patch-20261003T000000-00000000'
        work.mkdir()
        core.copy_tree(runner, work / 'runner')
        with (output / 'geometry-setup.log').open('w') as log:
            wine = core.Wine(prefix, work / 'runner', log)
            try:
                core.prepare_geometry(wine, bundle / 'build/downloads', bundle, print)
            finally:
                wine.stop()
        core.apply_overlay(prefix, work / 'runner', bundle)
        core.replace_link(runtime / 'runner', work / 'runner')
        core.write_json(runtime / core.MARKER, {'format': 1, 'state': 'installed', 'version': core.manifest()['version'],
            'work': str(work.relative_to(runtime)), 'backup': 'private/fenix-fixture', 'configured': False})
        cache = launcher.state_dir / 'fenix-bundles'
        cache.mkdir(parents=True)
        (cache / core.manifest()['version']).symlink_to(bundle, target_is_directory=True)
    native = NativeLauncher(args.native_launcher, output, runtime) if args.native_launcher else None
    if args.runner:
        if native:
            native.switch(runtime, 'proton', args.runner.resolve())
        else:
            launcher.proton.start({'runtime_path': str(runtime), 'mode': 'proton', 'path': str(args.runner.resolve())})
            launcher.proton.thread.join(600)
            if launcher.proton.job['state'] != 'complete':
                raise RuntimeError(launcher.proton.job)
    selected = proton.selection(runtime)
    assert bool(selected) == bool(args.runner)
    env = proton._prefix_env(prefix, output)
    if args.fenix_bundle:
        env.update(core.ENV)
    env.update(FLIGHTDECK_PROTON_LOADER='portable' if selected else 'native', WINEDLLPATH=str(runtime / 'local/store-runtime'),
               FLIGHTDECK_FAST_LAUNCH='1',
               XODUS_WINE_RUNNER=str(setup.runner_wine(runtime / 'runner')),
               WINELOADER=str(setup.runner_wine(runtime / 'runner')), WINESERVER=str(runtime / 'runner/files/bin/wineserver'),
               WINEDLLOVERRIDES='xgameruntime=n;xgameruntime_original=n,b;xodus_store_test=b;winemenubuilder.exe=d',
               DXVK_LOG_LEVEL='info', VKD3D_DEBUG='warn')
    fds = []
    try:
        for path in (exe, dll):
            fd = os.memfd_create('flightdeck-synthetic-probe', 0)
            os.write(fd, path.read_bytes())
            fds.append(fd)
        env['WINE_DLL_FILE_MAP'] = '|'.join(f'{fd}:\\??\\Z:' + str(game / name).replace('/', '\\') for fd, name in zip(fds, names))
        with (output / 'loader.log').open('w') as log:
            command = ([str(args.native_launcher.resolve()), 'wine-launch', '--runtime', str(runtime), '--', str(game / names[0])]
                       if native else [str(runtime / 'tools/xodus-wine-launch'), str(game / names[0])])
            result = subprocess.run(command,
                                    env=env, pass_fds=tuple(fds), stdout=log, stderr=subprocess.STDOUT, timeout=60)
        env.pop('WINE_DLL_FILE_MAP')
        with (output / 'render.log').open('w') as log:
            rendering = subprocess.run([str(setup.runner_wine(runtime / 'runner')), str(graphics)],
                                       cwd=output, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=90)
    finally:
        for fd in fds:
            os.close(fd)
        for option in ('-k', '-w'):
            subprocess.run([str(runtime / 'runner/files/bin/wineserver'), option], env=env, timeout=10)
    assert result.returncode == 0, (output / 'loader.log').read_text()
    assert rendering.returncode == 0, (output / 'render.log').read_text()
    assert all(setup.digest(game / name) == expected[name] for name in names)
    assert not list(game.glob('.xodus-launch-*'))
    (prefix / 'installed-on-proton').write_text('retain add-ons and settings')
    if selected:
        if native:
            native.switch(runtime, 'default')
        else:
            launcher.proton.start({'runtime_path': str(runtime), 'mode': 'default'})
            launcher.proton.thread.join(180)
            assert launcher.proton.job['state'] == 'complete', launcher.proton.job
    if native:
        native.close()
    assert proton._id(runtime / selected['base_prefix'] if selected else prefix) == before
    assert (prefix / 'original-marker').read_text() == 'preserve this profile'
    assert (prefix / 'installed-on-proton').read_text() == 'retain add-ons and settings'
    assert proton.selection(runtime) is None
    assert proton._version(runtime / 'runner') == proton._version(runner)
    if args.fenix_bundle:
        core.verify_installed(runtime, core.read_json(runtime / core.MARKER))
    report = {'implementation': 'rust' if native else 'python', 'mode': 'proton' if selected else 'flightdeck',
              'version': selected['version'] if selected else proton._version(runner),
              'loader': 'portable' if selected else 'native',
              'loader_passed': True, 'fastlaunch_argument_passed': True, 'working_directory_dll_passed': True, 'rendering_passed': True,
              'original_files_unchanged': True, 'return_to_default_passed': True if selected else None,
              'profile_identity_preserved': True, 'addons_retained': True,
              'fenix_overlay': bool(args.fenix_bundle),
              'scope': 'Synthetic memfd EXE/DLL, working-directory and module-directory loading, Store library loading and D3D11/D3D12 rendering; profile round trip only with --runner; no game/account'}
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
