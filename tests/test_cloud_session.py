# SPDX-License-Identifier: MIT
"""Private journal crash/restart tests; synthetic files and no network."""
from dataclasses import replace
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

from flightdeck import cloud_import as ci, cloud_session as session, cloud_write, save_state
from tests import test_cloud_import as import_fixtures
from tests.test_cloud_write import Operations


class CloudSessionTests(unittest.TestCase):
    setUp = import_fixtures.CloudImportTests.setUp
    write = import_fixtures.CloudImportTests.write
    state = import_fixtures.CloudImportTests.state
    install = import_fixtures.CloudImportTests.install
    snapshot = import_fixtures.CloudImportTests.snapshot
    lease = import_fixtures.CloudImportTests.lease

    def begin(self, *, remote=None):
        snapshot = self.snapshot({"profile": b"cloud"} if remote is None else remote)
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            return session.begin(self.runtime, self.scope, snapshot=snapshot, export=local)

    def receipt(self, local):
        operations = Operations(local.state)
        operations.scope = self.scope
        result = cloud_write.upload(self.scope, operations, operations.read, local.state,
                    expected_remote_digest=local.content_digest, assert_local_unchanged=local.assert_unchanged)
        return result

    @property
    def journal(self):
        return self.private / "cloud-sessions" / (self.namespace + ".json")

    def test_absent_is_read_only(self):
        self.assertIsNone(session.load(self.runtime, self.scope))
        self.assertFalse((self.private / "cloud-sessions").exists())
        self.assertFalse(self.folder.exists())

    def test_begin_durable_backup_and_new_process_load(self):
        raw = self.install({"profile": b"local"})
        record = self.begin()
        backup = self.private / "cloud-import-backups" / record["target_backup_id"]
        self.assertEqual((backup / "state.bin").read_bytes(), raw)
        self.assertEqual((backup / "imported.bin").read_bytes(), raw)
        self.assertEqual(backup.stat().st_mode & 0o777, 0o500)
        self.assertEqual((backup / "imported.bin").stat().st_mode & 0o777, 0o400)
        self.assertEqual(self.journal.stat().st_mode & 0o777, 0o600)
        self.assertEqual(session.load(self.runtime, self.scope), record)
        code = ("import json,sys; from flightdeck.cloud_session import load; "
                "from flightdeck.cloud_storage import Scope; from pathlib import Path; "
                "print(json.dumps(load(Path(sys.argv[1]),Scope(**json.loads(sys.argv[2])))))")
        result = subprocess.run([sys.executable, "-c", code, str(self.runtime), json.dumps(self.scope.__dict__)],
                                check=True, capture_output=True, text=True, timeout=10)
        self.assertEqual(json.loads(result.stdout), record)

    def test_missing_local_state_is_distinct_from_empty_file(self):
        record = self.begin(remote={})
        self.assertEqual(record["pre_local_sha256"], "missing")
        self.assertEqual(record["target_local_sha256"], "missing")
        self.assertEqual(session.load(self.runtime, self.scope), record)
        self.assertFalse((self.folder / "state.bin").exists())

    def test_existing_pending_is_never_replaced_by_begin(self):
        self.install({"profile": b"local"})
        record = self.begin()
        with self.assertRaises(session.CloudSessionError) as caught:
            self.begin()
        self.assertEqual(caught.exception.code, "busy")
        self.assertEqual(session.load(self.runtime, self.scope), record)

    def test_playing_update_needs_no_receipt_and_keeps_original_remote(self):
        self.install({"profile": b"before"})
        record = self.begin()
        self.install({"profile": b"merged"}, generation=8)
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            updated = session.update(self.runtime, self.scope, record, phase="playing", export=local)
        self.assertEqual(updated["before_remote_digest"], record["before_remote_digest"])
        self.assertEqual(updated["pre_local_digest"], record["pre_local_digest"])
        self.assertNotEqual(updated["target_digest"], record["target_digest"])
        self.assertNotEqual(updated["target_backup_id"], record["target_backup_id"])
        self.assertEqual(session.load(self.runtime, self.scope), updated)

    def test_uploading_requires_new_backup_under_active_export(self):
        self.install({"profile": b"before"})
        record = self.begin()
        with self.lease() as fd:
            with self.assertRaises(session.CloudSessionError):
                session.update(self.runtime, self.scope, record, phase="uploading", runtime_lock_fd=fd)
        raw = self.install({"profile": b"after game"}, generation=8)
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            updated = session.update(self.runtime, self.scope, record, phase="uploading", export=local)
        stored = self.private / "cloud-import-backups" / updated["target_backup_id"] / "imported.bin"
        self.assertEqual(stored.read_bytes(), raw)
        self.assertEqual(session.load(self.runtime, self.scope), updated)

    def test_pending_only_change_requires_play_lease_and_keeps_target(self):
        self.install({"profile": b"target"})
        record = self.begin()
        with self.assertRaises(session.CloudSessionError):
            session.update(self.runtime, self.scope, record, phase="pending")
        with self.lease() as fd:
            updated = session.update(self.runtime, self.scope, record, phase="pending", runtime_lock_fd=fd)
        self.assertEqual(updated["target_backup_id"], record["target_backup_id"])
        self.assertEqual(updated["revision"], record["revision"] + 1)
        self.assertEqual(session.load(self.runtime, self.scope), updated)

    def test_stale_revision_or_altered_record_cannot_clear_or_change(self):
        self.install({"profile": b"target"})
        record = self.begin()
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            updated = session.update(self.runtime, self.scope, record, phase="pending", export=local)
            for bad in (record, {**updated, "target_digest": "0" * 64}):
                with self.assertRaises(session.CloudSessionError):
                    session.update(self.runtime, self.scope, bad, phase="playing", export=local)
                with self.assertRaises(session.CloudSessionError):
                    session.complete(self.runtime, self.scope, bad, export=local, receipt=self.receipt(local))
        self.assertEqual(session.load(self.runtime, self.scope), updated)

    def test_checkpoint_and_complete_require_fresh_matching_receipt(self):
        self.install({"profile": b"target"})
        record = self.begin()
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            receipt = self.receipt(local)
            for bad in (object(), replace(receipt, source_content_digest="0" * 64)):
                with self.assertRaises(session.CloudSessionError):
                    session.checkpoint(self.runtime, self.scope, record, export=local, receipt=bad)
            checked = session.checkpoint(self.runtime, self.scope, record, export=local, receipt=receipt)
            self.assertEqual(checked["before_remote_digest"], local.content_digest)
            self.assertEqual(session.load(self.runtime, self.scope), checked)
            session.complete(self.runtime, self.scope, checked, export=local, receipt=receipt)
        self.assertIsNone(session.load(self.runtime, self.scope))
        self.assertTrue((self.private / "cloud-import-backups" / checked["target_backup_id"]).is_dir())

    def test_changed_local_after_crash_cannot_be_mistaken_for_old_target(self):
        self.install({"profile": b"target"})
        record = self.begin()
        self.install({"profile": b"new offline progress"}, generation=8)
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            with self.assertRaises(session.CloudSessionError):
                session.complete(self.runtime, self.scope, record, export=local, receipt=self.receipt(local))
        self.assertEqual(session.load(self.runtime, self.scope), record)

    def test_stale_export_and_wrong_scope_rejected(self):
        self.install({"profile": b"target"})
        snapshot = self.snapshot({"profile": b"cloud"})
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            with self.assertRaises(session.CloudSessionError):
                session.begin(self.runtime, replace(self.scope, xuid="987654321"), snapshot=snapshot, export=local)
        with self.assertRaises(session.CloudSessionError):
            session.begin(self.runtime, self.scope, snapshot=snapshot, export=local)

    def test_corrupt_snapshot_or_target_is_error_not_absence(self):
        self.install({"profile": b"target"})
        record = self.begin()
        path = self.private / "cloud-import-backups" / record["target_backup_id"] / "imported.bin"
        raw = path.read_bytes(); path.chmod(0o600); path.write_bytes(b"corrupt")
        with self.assertRaises(session.CloudSessionError):
            session.load(self.runtime, self.scope)
        path.write_bytes(raw); path.chmod(0o400)
        source = self.private / "cloud-saves" / record["snapshot"]["id"] / "manifest.json"
        source.write_bytes(b"corrupt")
        with self.assertRaises(session.CloudSessionError):
            session.load(self.runtime, self.scope)

    def test_journal_symlink_and_oversize_rejected(self):
        self.install({"profile": b"target"})
        record = self.begin()
        original = self.journal.read_bytes(); self.journal.unlink()
        elsewhere = self.private / "elsewhere"; self.write(elsewhere, original)
        self.journal.symlink_to(elsewhere)
        with self.assertRaises(session.CloudSessionError):
            session.load(self.runtime, self.scope)
        self.journal.unlink(); self.write(self.journal, b" " * 32769)
        with self.assertRaises(session.CloudSessionError):
            session.load(self.runtime, self.scope)

    def test_backup_or_journal_failure_never_returns_upload_ready(self):
        self.install({"profile": b"target"})
        record = self.begin()
        for function in ("_backup", "_write"):
            with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
                with patch.object(ci, function, side_effect=OSError("synthetic fault")):
                    with self.assertRaises(session.CloudSessionError):
                        session.update(self.runtime, self.scope, record, phase="uploading", export=local)
            self.assertEqual(session.load(self.runtime, self.scope), record)

    def test_export_paths_never_reopen_writer_lock(self):
        self.install({"profile": b"target"})
        snapshot = self.snapshot({"profile": b"cloud"})
        actual = os.open
        def checked(path, *args, **kwargs):
            if str(path).endswith("writer.lock"):
                raise AssertionError("Existing native lock must not be reopened")
            return actual(path, *args, **kwargs)
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            with patch.object(os, "open", side_effect=checked):
                record = session.begin(self.runtime, self.scope, snapshot=snapshot, export=local)
                record = session.update(self.runtime, self.scope, record, phase="uploading", export=local)
                session.complete(self.runtime, self.scope, record, export=local, receipt=self.receipt(local))

    def test_journal_replace_failure_preserves_previous_record(self):
        self.install({"profile": b"target"})
        record = self.begin()
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            with patch.object(session.os, "replace", side_effect=OSError("synthetic fault")):
                with self.assertRaises(session.CloudSessionError):
                    session.update(self.runtime, self.scope, record, phase="uploading", export=local)
        self.assertEqual(session.load(self.runtime, self.scope), record)
        self.assertFalse(list(self.journal.parent.glob(".session-*")))

    def test_journal_flush_failure_is_not_upload_permission(self):
        self.install({"profile": b"target"})
        record = self.begin()
        actual = os.fsync
        parent = self.journal.parent.stat()
        def fail_journal(fd):
            info = os.fstat(fd)
            if (info.st_dev, info.st_ino) == (parent.st_dev, parent.st_ino):
                raise OSError("synthetic journal flush fault")
            return actual(fd)
        with self.lease() as fd, ci.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as local:
            with patch.object(os, "fsync", side_effect=fail_journal):
                with self.assertRaises(session.CloudSessionError):
                    session.update(self.runtime, self.scope, record, phase="uploading", export=local)
        # Rename already happened, so recovery must be conservative. The API
        # raised and never granted permission to perform a remote mutation.
        pending = session.load(self.runtime, self.scope)
        self.assertEqual(pending["phase"], "uploading")
        self.assertEqual(pending["target_digest"], record["target_digest"])


if __name__ == "__main__":
    unittest.main()
