# SPDX-License-Identifier: MIT
"""Real Python reader/import/writer/manager integration with synthetic HTTP only."""
from contextlib import contextmanager
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import unquote
import uuid

from flightdeck import cloud_import, cloud_storage, cloud_sync, save_state
from flightdeck.backend import Launcher, LauncherError
from tests.test_cloud_write import Operations, SCOPE, state


class SyntheticTransport(Operations):
    def __call__(self, request, *, timeout, max_bytes):
        self.record('GET', timeout)
        if request.method != 'GET' or not request.url.startswith(self.scope.base_url):
            raise AssertionError('Unexpected synthetic request')
        suffix = request.url[len(self.scope.base_url):]
        atoms = {}
        containers = {}
        rows = []
        for name, entry in sorted(self.remote.containers.items()):
            mapping = {}
            for blob, payload in sorted(entry.blobs.items()):
                atom = str(uuid.uuid5(uuid.NAMESPACE_OID, name + '/' + blob + '/' + hashlib.sha256(payload).hexdigest())).upper()
                mapping[blob] = atom + ',binary'
                atoms[atom] = payload
            containers[name + ',savedgame'] = {'atoms': mapping}
            rows.append({'fileName': name + ',savedgame', 'displayName': entry.display_name,
                         'etag': save_state.content_digest(save_state.State(0, {name: entry})),
                         'clientFileTime': (entry.modified + 11644473600) * 10000000,
                         'size': sum(len(b) for b in entry.blobs.values())})
        if not suffix:
            value = {'blobs': rows, 'pagingInfo': {'continuationToken': None, 'totalItems': len(rows)}}
            body = json.dumps(value).encode()
        elif unquote(suffix).endswith(',savedgame'):
            body = json.dumps(containers[unquote(suffix[1:])]).encode()
        elif suffix.endswith(',binary'):
            body = atoms[unquote(suffix[1:-len(',binary')])]
        else:
            raise AssertionError('Unexpected synthetic route')
        if len(body) > max_bytes:
            raise cloud_storage.CloudStorageError('bounds')
        return cloud_storage.Response(200, {'Content-Length': str(len(body))}, body)


class SyntheticRuntime:
    def __init__(self, transport):
        self.transport = transport
        self.cleanup_failure = False

    def available(self, *args, **kwargs):
        return True

    @contextmanager
    def open_client(self, *args, **kwargs):
        yield cloud_storage.CloudStorageClient(self.transport.scope, self.transport)
        if self.cleanup_failure:
            raise RuntimeError('synthetic private cleanup detail')


class CloudSyncIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / 'runtime'
        private = self.runtime / 'private'
        private.mkdir(mode=0o700, parents=True)
        (private / 'play.lock').touch(mode=0o600)
        (private / 'local-saves.enabled').touch(mode=0o600)
        (private / 'local-saves').mkdir(mode=0o700)
        (self.runtime / 'tools').mkdir()
        (self.runtime / 'tools/play-msfs.sh').write_text('#!/bin/sh\nexit 0\n')
        self.namespace = save_state.namespace_key(SCOPE.title_id, SCOPE.scid, int(SCOPE.xuid))
        self.folder = private / 'local-saves' / self.namespace
        self.folder.mkdir(mode=0o700)
        self.install(state(profile=b'local'))
        self.launcher = Launcher(self.root / 'config', str(self.runtime))
        self.ops = SyntheticTransport(state(profile=b'cloud'))
        self.api = SyntheticRuntime(self.ops)
        self.manager = cloud_sync.CloudSaveManager(self.launcher, self.api)
        self.launcher.cloud_saves = self.manager
        self.addCleanup(self.launcher.setup.close)
        self.addCleanup(self.manager.close)

    def install(self, value):
        path = self.folder / 'state.bin'
        path.write_bytes(save_state.encode(value))
        path.chmod(0o600)

    def finish(self):
        self.manager.worker.join(5)
        self.assertFalse(self.manager.worker.is_alive())
        self.assertFalse(self.launcher.setup_busy)
        return self.manager.snapshot()['job']

    def prepare(self):
        self.manager.start('prepare-import')
        job = self.finish()
        self.assertEqual(job['state'], 'succeeded', job)
        return self.manager.snapshot()['plan']['id']

    def upload(self, plan):
        self.manager.start('upload', plan_id=plan, choice='local')
        return self.finish()

    def test_plan_to_real_writer_readback_and_persistent_baseline(self):
        original = (self.folder / 'state.bin').read_bytes()
        plan = self.prepare()
        self.assertTrue(self.manager.snapshot()['can_upload'])
        with self.assertRaises(LauncherError):
            self.manager.start('upload', plan_id=plan, choice='cloud')
        job = self.upload(plan)
        self.assertEqual(job['state'], 'succeeded', job)
        self.assertTrue(job['result']['uploaded'])
        self.assertTrue(job['result']['baseline_saved'])
        self.assertEqual(job['result']['changed_containers'], 1)
        self.assertEqual((self.folder / 'state.bin').read_bytes(), original)
        baseline = cloud_import.load_baseline(self.runtime, SCOPE)
        self.assertIsNotNone(baseline)
        self.assertEqual(save_state.content_digest(self.ops.remote),
                         save_state.content_digest(save_state.decode(original)))
        self.assertIsNone(self.manager.snapshot()['plan'])
        with self.assertRaises(LauncherError):
            self.manager.start('upload', plan_id=plan, choice='local')
        self.assertNotIn('profile', json.dumps(job))
        self.assertGreater(self.ops.calls.count('GET'), 5)

    def test_fresh_remote_conflict_has_no_write_or_receipt(self):
        plan = self.prepare()
        self.ops.remote = state(profile=b'newer cloud')
        job = self.upload(plan)
        self.assertEqual(job['error_code'], 'conflict')
        self.assertFalse(job['recovery_required'])
        self.assertNotIn('put', self.ops.calls)
        self.assertNotIn('atom', self.ops.calls)
        self.assertIsNone(cloud_import.load_baseline(self.runtime, SCOPE))
        self.assertIsNone(self.manager.snapshot()['plan'])

    def test_local_revision_change_rejected_before_native_acquire(self):
        plan = self.prepare()
        self.install(state(profile=b'newer local'))
        job = self.upload(plan)
        self.assertEqual(job['error_code'], 'changed')
        self.assertNotIn('acquire', self.ops.calls)
        self.assertIsNone(self.manager.snapshot()['plan'])

    def test_partial_cloud_commit_keeps_counts_and_requires_fresh_comparison(self):
        self.install(state(first=b'local1', second=b'local2'))
        self.ops.remote = state()
        plan = self.prepare()
        original = self.ops.put_container
        def put(name, *args, **kwargs):
            if name == 'second,savedgame':
                return 409
            return original(name, *args, **kwargs)
        self.ops.put_container = put
        job = self.upload(plan)
        self.assertEqual(job['state'], 'failed')
        self.assertEqual(job['committed_containers'], 1)
        self.assertTrue(job['recovery_required'])
        self.assertIn('first', self.ops.remote.containers)
        self.assertNotIn('second', self.ops.remote.containers)
        self.assertNotIn('nicht verändert', job['message'])
        self.assertIsNone(cloud_import.load_baseline(self.runtime, SCOPE))

    def test_receipt_persistence_and_late_transport_cleanup_preserve_remote_success(self):
        plan = self.prepare()
        self.api.cleanup_failure = True
        with patch('flightdeck.cloud_import.record_common', side_effect=OSError('synthetic private detail')):
            job = self.upload(plan)
        self.assertEqual(job['state'], 'succeeded')
        self.assertTrue(job['result']['uploaded'])
        self.assertFalse(job['result']['baseline_saved'])
        self.assertTrue(job['warning'])
        self.assertNotIn('private detail', json.dumps(job))
        self.assertIsNone(cloud_import.load_baseline(self.runtime, SCOPE))

    def test_remaining_deadline_forwarded_into_real_snapshot_reader(self):
        plan = self.prepare()
        original = cloud_storage.CloudStorageClient.download_snapshot
        observed = []
        def snapshot(client, *args, **kwargs):
            observed.append(client.limits.deadline_seconds)
            return original(client, *args, **kwargs)
        with patch.object(cloud_storage.CloudStorageClient, 'download_snapshot', snapshot), \
             patch('flightdeck.cloud_write.Limits', return_value=__import__('flightdeck.cloud_write', fromlist=['Limits']).Limits(deadline_seconds=45)):
            job = self.upload(plan)
        self.assertEqual(job['state'], 'succeeded')
        self.assertTrue(observed)
        self.assertTrue(all(0 < n <= 45 for n in observed))


if __name__ == '__main__':
    unittest.main()
