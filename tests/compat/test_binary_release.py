# SPDX-License-Identifier: MIT
"""Synthetic native/source archives only; no compiler, Wine, account or network."""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('native_release', REPO / 'scripts/binary-release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def sha(data):
    return hashlib.sha256(data).hexdigest()


class BinaryReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / 'repo'
        self.stage = self.root / 'stage'
        self.write(self.repo, 'scripts/build-compat.sh', (REPO / 'scripts/build-compat.sh').read_bytes())
        self.write(self.repo, 'BUILDING.md', b'Synthetic build guidance\n')
        self.write(self.repo, 'compat/LICENSES/LGPL.txt', b'Synthetic license preservation marker\n')
        self.sources = {
            'wine-src/LICENSE': b'Synthetic Wine license marker\n',
            'xodus-src/LICENSE': b'Synthetic Xodus license marker\n',
            'xodus-src/Cargo.lock': b'Synthetic locked source marker\n',
            **{name: b'Synthetic helper input: ' + name.encode() for name in release.HELPER_SOURCES},
        }
        for name, data in self.sources.items():
            self.write(self.stage, name, data)
        self.source_manifest = {'format': 1, 'files': {name: sha(data) for name, data in self.sources.items()}}
        self.hashes = {}
        for name in release.NATIVE_FILES:
            data = b'Synthetic binary: ' + name.encode()
            self.write(self.stage, 'artifacts/' + name, data)
            self.hashes[name] = sha(data)
        self.lock = {'native': {'files': self.hashes, 'features': list(release.NATIVE_FEATURES), 'cli_features': list(release.CLI_FEATURES)}}
        self.manifest = {'format': 1, 'files': self.hashes, 'features': list(release.NATIVE_FEATURES), 'cli_features': list(release.CLI_FEATURES)}
        self.sync_manifests()
        self.write(self.root, 'vendor/example/source.rs', b'// Synthetic dependency source\n')
        self.write(self.root, 'vendor-config.toml', b'[source.vendored-sources]\ndirectory = "example"\n')
        self.write(self.root, 'supplements/manifest.json', b'{"files":[]}\n')
        self.args = argparse.Namespace(stage=self.stage, output=self.root / 'output',
                                       vendor=self.root / 'vendor', vendor_config=self.root / 'vendor-config.toml',
                                       supplements=self.root / 'supplements')
        self.root_patch = patch.object(release, 'ROOT', self.repo)
        self.root_patch.start(); self.addCleanup(self.root_patch.stop)

    def write(self, root, name, data):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def sync_manifests(self):
        source = self.write(self.stage, 'source-manifest.json', json.dumps(self.source_manifest).encode())
        self.manifest['source_manifest_sha256'] = sha(source.read_bytes())
        self.write(self.stage, 'artifacts/manifest.json', json.dumps(self.manifest).encode())
        self.write(self.repo, 'compat/bootstrap.lock.json', json.dumps(self.lock).encode())

    def create(self):
        with patch.object(release, 'notices', return_value=(b'Synthetic complete notice\n', [])), contextlib.redirect_stdout(io.StringIO()):
            release.create(self.args)

    def test_six_binaries_and_full_corresponding_sources_are_preserved(self):
        self.create()
        with tarfile.open(self.args.output / f'flightdeck-compat-{release.VERSION}-linux-x86_64.tar.gz') as archive:
            self.assertEqual(set(archive.getnames()), release.NATIVE_FILES | {'manifest.json', 'THIRD-PARTY-NOTICES.txt'})
            helper = archive.getmember('bin/flightdeck-connected-storage.exe')
            self.assertEqual(helper.mode, 0o755)
            self.assertEqual(sha(archive.extractfile(helper).read()), self.hashes[helper.name])
        with tarfile.open(self.args.output / f'flightdeck-native-sources-{release.VERSION}.tar.gz') as archive:
            for name, data in self.sources.items():
                self.assertEqual(archive.extractfile(name).read(), data)
            self.assertEqual(archive.extractfile('build-compat.sh').read(), (REPO / 'scripts/build-compat.sh').read_bytes())
            self.assertIn(b'ConnectedStorage helper', archive.extractfile('README.md').read())
            self.assertIn(b'launcher coordinates automatic', archive.extractfile('README.md').read())
            self.assertIn('runtime/tools/ConnectedStorageWrite.h', archive.getnames())
            self.assertIn('vendor/example/source.rs', archive.getnames())
            self.assertIn('LICENSES/LGPL.txt', archive.getnames())

    def test_missing_corresponding_helper_input_is_rejected(self):
        for index, name in enumerate(release.HELPER_SOURCES):
            with self.subTest(name=name):
                self.args.output = self.root / ('missing-helper-' + str(index))
                checksum = self.source_manifest['files'].pop(name)
                self.sync_manifests()
                with self.assertRaisesRegex(ValueError, 'helper sources'):
                    self.create()
                self.source_manifest['files'][name] = checksum

    def test_unpinned_helper_change_is_rejected(self):
        self.write(self.stage, 'artifacts/bin/flightdeck-connected-storage.exe', b'different bytes')
        with self.assertRaisesRegex(ValueError, 'Build input changed'):
            self.create()

    def test_capability_must_exist_in_build_and_lock_as_lists(self):
        for target, value in [('lock', []), ('lock', release.NATIVE_FEATURES[0]), ('lock', [release.NATIVE_FEATURES[0]]), ('lock', [release.NATIVE_FEATURES[1]]),
                              ('build', []), ('build', release.NATIVE_FEATURES[0]), ('build', [release.NATIVE_FEATURES[0]]), ('build', [release.NATIVE_FEATURES[1]])]:
            with self.subTest(target=target, value=value):
                self.args.output = self.root / (target + str(len(list(self.root.iterdir()))))
                self.lock['native']['features'] = list(release.NATIVE_FEATURES)
                self.manifest['features'] = list(release.NATIVE_FEATURES)
                (self.lock['native'] if target == 'lock' else self.manifest)['features'] = value
                self.sync_manifests()
                with self.assertRaisesRegex(ValueError, 'capability'):
                    self.create()

    def test_progress_capability_is_required_in_both_build_and_lock(self):
        for target in ('lock', 'build'):
            for index, value in enumerate((None, 'streaming-progress-v1', list(release.CLI_FEATURES[:-1]), [1])):
                with self.subTest(target=target, value=value):
                    self.args.output = self.root / f'cli-{target}-{index}'
                    self.lock['native']['cli_features'] = list(release.CLI_FEATURES)
                    self.manifest['cli_features'] = list(release.CLI_FEATURES)
                    (self.lock['native'] if target == 'lock' else self.manifest)['cli_features'] = value
                    self.sync_manifests()
                    with self.assertRaisesRegex(ValueError, 'CLI capability'):
                        self.create()

    def test_old_five_binary_bundle_does_not_claim_new_capability(self):
        del self.hashes['bin/flightdeck-connected-storage.exe']
        self.sync_manifests()
        with self.assertRaisesRegex(ValueError, 'artifacts differ'):
            self.create()


if __name__ == '__main__':
    unittest.main()
