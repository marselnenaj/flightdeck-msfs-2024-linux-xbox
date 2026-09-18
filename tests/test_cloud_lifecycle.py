# SPDX-License-Identifier: MIT
"""One complete automatic session with a real supervisor and synthetic cloud.

Only the game/service executables, local ping, and remote HTTP are synthetic.
The coordinator, reader/importer, local save codec, subprocess and flock
handoff, stop/cleanup, writer/readback, backups and receipts execute normally.
"""
import json
import os
from pathlib import Path
import subprocess
import threading
import unittest
from unittest.mock import patch

from flightdeck import cloud_import, cloud_session, save_state
from tests import test_backend as backend_fixtures
from tests import test_cloud_sync_integration as integration
from tests.test_cloud_write import SCOPE


class CloudLifecycleTests(unittest.TestCase):
    install = integration.CloudSyncIntegrationTests.install
    managed_fixture = backend_fixtures.BackendTests.managed_fixture
    assert_competitor_blocked = backend_fixtures.BackendTests.assert_competitor_blocked
    wait_for = backend_fixtures.BackendTests.wait_for

    def setUp(self):
        integration.CloudSyncIntegrationTests.setUp(self)
        self.base = self.root
        self.upload_waiting = threading.Event()
        self.allow_upload = threading.Event()
        self.owned_process = None
        self.addCleanup(self.stop_fixture)
        for name in ("games/MSFS2024/FlightSimulator2024.exe", "local/msfs-prefix/system.reg",
                     "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"):
            path = self.runtime / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("synthetic fixture, not game content\n")
        (self.runtime / "tools/play-msfs.sh").chmod(0o700)

    def stop_fixture(self):
        self.allow_upload.set()
        child = self.owned_process or self.launcher.process
        if child is not None and child.poll() is None:
            child.terminate()  # Only this test's exact supervisor.
            try:
                child.wait(timeout=8)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=3)
        worker = self.manager.automation.worker
        if worker is not None:
            worker.join(8)

    def test_actual_supervisor_import_game_save_stop_and_verified_upload(self):
        environment = self.managed_fixture()
        original_local = (self.folder / "state.bin").read_bytes()
        game = self.runtime / "tools/launch-msfs.sh"
        project = Path(__file__).resolve().parents[1]
        # This real child reads the imported XDLOCAL1 state and commits its
        # own new generation under the same native writer-lock convention.
        mutation = f"""
sys.path.insert(0, {str(project)!r})
from flightdeck import save_state
folder = root / 'private/local-saves' / {self.namespace!r}
with (folder / 'writer.lock').open('r+b') as writer:
    fcntl.lockf(writer, fcntl.LOCK_EX | fcntl.LOCK_NB, 1, 0, os.SEEK_SET)
    value = save_state.decode((folder / 'state.bin').read_bytes())
    assert value.containers['profile'].blobs['data'] == b'cloud'
    assert (root / 'private/cloud-interrupted-process.json').is_file()
    assert (root / 'private/cloud-offline.pending').is_file()
    value.generation += 1
    value.containers['profile'].blobs['data'] = b'flight saved by actual synthetic child'
    temporary = folder / '.tmp-synthetic-state'
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'wb') as target:
        target.write(save_state.encode(value)); target.flush(); os.fsync(target.fileno())
    os.replace(temporary, folder / 'state.bin')
    directory = os.open(folder, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(directory)
    finally: os.close(directory)
(root / 'private/game-saved').write_text('import checked; generation committed')
"""
        game.write_text(game.read_text().replace(
            "while not (root/'private/game-done').exists()", mutation + "\nwhile not (root/'private/game-done').exists()"))

        record = self.ops.record
        def locked_record(name, timeout):
            # Every synthetic HTTP/lease operation still runs inside the
            # coordinator's continuous runtime lease, including readback.
            self.assert_competitor_blocked()
            record(name, timeout)

        acquire = self.ops.acquire
        def pause_before_upload(*, timeout):
            self.upload_waiting.set()
            if not self.allow_upload.wait(5):
                raise AssertionError("Synthetic upload barrier was not released")
            return acquire(timeout=timeout)

        with patch.dict(os.environ, environment), patch.object(self.ops, "record", side_effect=locked_record), \
             patch.object(self.ops, "acquire", side_effect=pause_before_upload):
            result = self.launcher.launch()
            self.assertTrue(result["cloud_sync"])
            self.wait_for(lambda: (self.runtime / "private/game-saved").exists(), timeout=8)
            self.owned_process = self.launcher.process
            self.assertIsInstance(self.owned_process, subprocess.Popen)
            self.assertEqual(Path(self.owned_process.args[1]).resolve(),
                             (project / "scripts/runtime/play-msfs.sh").resolve())
            self.assertIn("--lock-fd", self.owned_process.args)
            self.assertNotIn("acquire", self.ops.calls)
            for kind in ("game", "service"):
                observed = json.loads((self.runtime / f"private/{kind}-ready").read_text())
                self.assertEqual(observed, {"inherited": False, "blocked": True})
            self.assertEqual(self.manager.automation.snapshot()["state"], "playing")
            self.assertEqual(cloud_session.load(self.runtime, SCOPE)["phase"], "playing")
            self.assert_competitor_blocked()

            self.assertTrue(self.launcher.stop()["ok"])
            self.assertTrue(self.upload_waiting.wait(8), self.manager.automation.snapshot())
            self.assertEqual(self.owned_process.poll(), 0)
            self.assertTrue((self.runtime / "private/game-stopped").is_file())
            self.assertTrue((self.runtime / "private/service-stopped").is_file())
            self.assertIsNone(self.launcher.process)
            self.assertTrue(self.launcher.setup_busy)
            self.assertIsNotNone(self.launcher.managed_session)
            self.assertEqual(self.manager.automation.snapshot()["phase"], "after_exit")
            self.assertFalse((self.runtime / "private/cloud-interrupted-process.json").exists())
            self.assert_competitor_blocked()
            self.allow_upload.set()
            self.manager.automation.worker.join(8)
            self.assertFalse(self.manager.automation.worker.is_alive())

        outcome = self.manager.automation.snapshot()
        self.assertEqual(outcome["state"], "synced", outcome)
        self.assertFalse(self.launcher.setup_busy)
        self.assertIsNone(self.launcher.managed_session)
        self.assertFalse(self.launcher._external())
        expected = b"flight saved by actual synthetic child"
        local = save_state.decode((self.folder / "state.bin").read_bytes())
        self.assertEqual(local.containers["profile"].blobs["data"], expected)
        self.assertEqual(self.ops.remote.containers["profile"].blobs["data"], expected)
        self.assertEqual(save_state.content_digest(local), save_state.content_digest(self.ops.remote))
        self.assertEqual(self.ops.calls.count("put"), 1)
        self.assertEqual(self.ops.calls.count("atom"), 1)
        self.assertEqual(self.ops.calls[-1], "release")
        self.assertIsNotNone(cloud_import.load_baseline(self.runtime, SCOPE))
        self.assertIsNone(cloud_session.load(self.runtime, SCOPE))
        backed_up = [p.read_bytes() for p in (self.runtime / "private/save-backups").rglob("state.bin")]
        self.assertIn(original_local, backed_up)
        self.assertIn((self.folder / "state.bin").read_bytes(), backed_up)
        # Runtime-wide marker intentionally persists: another account may
        # have created local progress during the game without our preflight.
        self.assertTrue((self.runtime / "private/cloud-offline.pending").is_file())


if __name__ == "__main__":
    unittest.main()
