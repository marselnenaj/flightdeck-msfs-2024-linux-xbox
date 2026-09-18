# SPDX-License-Identifier: MIT
"""Synthetic local import tests; no real runtime, cloud or credentials."""
from contextlib import contextmanager
from dataclasses import replace
import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
import uuid

from flightdeck import cloud_import as importer, save_state
from flightdeck.cloud_storage import Scope, Snapshot


class CloudImportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.runtime = Path(self.temporary.name) / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.private = self.runtime / "private"
        self.private.mkdir(mode=0o700)
        for name in ("local-saves", "cloud-saves"):
            (self.private / name).mkdir(mode=0o700)
        self.write(self.private / "local-saves.enabled", b"enabled")
        self.write(self.private / "play.lock", b"")
        self.scope = Scope("123456789", "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "Example.Game_0123456789abc", 1234)
        self.namespace = save_state.namespace_key(self.scope.title_id, self.scope.scid, int(self.scope.xuid))
        self.folder = self.private / "local-saves" / self.namespace

    def write(self, path, data):
        path.write_bytes(data)
        path.chmod(0o600)

    def state(self, content, generation=7, modified=100):
        return save_state.State(generation, {name: save_state.Container(name, modified, {"data": payload})
                                             for name, payload in content.items()})

    def install(self, content, generation=7, modified=100):
        self.folder.mkdir(mode=0o700, exist_ok=True)
        raw = save_state.encode(self.state(content, generation, modified))
        self.write(self.folder / "state.bin", raw)
        return raw

    def snapshot(self, content, *, modified=200):
        path = self.private / "cloud-saves" / ("snapshot-" + uuid.uuid4().hex)
        path.mkdir(mode=0o700)
        (path / "blobs").mkdir(mode=0o700)
        rows = []
        size = 0
        for index, (name, payload) in enumerate(sorted(content.items())):
            filename = f"blobs/{index:08x}.bin"
            self.write(path / filename, payload)
            rows.append({"name": name + ",savedgame", "display_name": name, "etag": "synthetic-revision",
                         "client_file_time": (11644473600 + modified) * 10_000_000,
                         "size": len(payload), "blobs": [{"name": "data", "atom": str(uuid.uuid4()),
                             "file": filename, "size": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}]})
            size += len(payload)
        manifest = {"schema": 1, "scope_binding": self.scope.binding, "consistency": "rechecked-unlocked", "containers": rows}
        self.write(path / "manifest.json", json.dumps(manifest).encode())
        return Snapshot(path, self.scope.binding, len(content), len(content), size)

    @contextmanager
    def lease(self):
        with (self.private / "play.lock").open("r+") as stream:
            fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield stream.fileno()

    def apply(self, plan, **kwargs):
        kwargs.setdefault("choice", "cloud")
        with self.lease() as fd:
            return importer.apply(self.runtime, self.scope, plan, runtime_lock_fd=fd, **kwargs)

    def error(self, code, action):
        with self.assertRaises(importer.CloudImportError) as caught:
            action()
        self.assertEqual(caught.exception.code, code)
        self.assertNotIn(self.namespace, str(caught.exception))

    def contents(self):
        return save_state.decode((self.folder / "state.bin").read_bytes())

    def test_explicit_full_import_backs_up_and_preserves_other_namespaces(self):
        original = self.install({"profile": b"old", "local-only": b"keep in backup"})
        other = self.private / "local-saves" / ("f" * 64)
        other.mkdir(mode=0o700); self.write(other / "state.bin", b"unrelated")
        remote = self.snapshot({"profile": b"new", "nested/settings": b"added"})
        plan = importer.prepare(self.runtime, self.scope, remote)
        self.assertEqual(plan.summary()["conflict_count"], 2)
        self.assertEqual(plan.summary()["add_count"], 1)
        self.assertEqual(plan.summary()["delete_count"], 1)
        self.assertEqual((self.folder / "state.bin").read_bytes(), original)
        result = self.apply(plan)
        self.assertTrue(result.imported and result.durability_confirmed)
        self.assertEqual(result.generation, 8)
        self.assertEqual(self.contents().containers["nested/settings"].modified, 200)
        self.assertEqual(set(self.contents().containers), {"profile", "nested/settings"})
        backup = self.private / "cloud-import-backups" / result.backup_id
        self.assertEqual((backup / "state.bin").read_bytes(), original)
        self.assertEqual(stat.S_IMODE(backup.stat().st_mode), 0o500)
        self.assertEqual(stat.S_IMODE((backup / "state.bin").stat().st_mode), 0o400)
        metadata = json.loads((backup / "manifest.json").read_text())
        self.assertEqual(metadata["replacement_sha256"], hashlib.sha256((self.folder / "state.bin").read_bytes()).hexdigest())
        self.assertEqual((other / "state.bin").read_bytes(), b"unrelated")
        self.assertNotIn(self.namespace, json.dumps(result.summary()))
        self.assertNotIn("nested/settings", repr(plan))

    def test_missing_state_import_increments_from_zero_with_absence_backup(self):
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"new"}))
        self.assertFalse(plan.summary()["local_exists"])
        self.assertFalse(self.folder.exists())
        result = self.apply(plan)
        self.assertEqual(result.generation, 1)
        backup = self.private / "cloud-import-backups" / result.backup_id
        self.assertFalse((backup / "state.bin").exists())
        self.assertFalse(json.loads((backup / "manifest.json").read_text())["existed"])

    def test_empty_cloud_needs_explicit_choice_and_preserves_backup(self):
        raw = self.install({"profile": b"existing"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({}))
        self.assertEqual(plan.summary()["conflict_count"], 1)
        self.error("conflict", lambda: self.apply(plan, choice=None))
        self.assertEqual((self.folder / "state.bin").read_bytes(), raw)
        result = self.apply(plan, choice="cloud")
        self.assertTrue(result.imported)
        self.assertEqual(self.contents().containers, {})

    def test_local_choice_and_equal_contents_are_noops(self):
        raw = self.install({"profile": b"existing"})
        for data, choice in ((b"remote", "local"), (b"existing", "cloud")):
            plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": data}, modified=999))
            result = self.apply(plan, choice=choice)
            self.assertFalse(result.imported)
            self.assertIsNone(result.backup_id)
            self.assertEqual((self.folder / "state.bin").read_bytes(), raw)
        self.assertFalse((self.private / "cloud-import-backups").exists())

    def test_three_way_changes_deletions_and_explicit_conflict_resolution(self):
        initial = {"left": b"base", "right": b"base", "both": b"base", "delete": b"base"}
        baseline = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot(initial))).baseline
        local = {"left": b"local", "right": b"base", "both": b"local", "delete": b"base"}
        self.install(local, generation=2)
        remote = self.snapshot({"left": b"base", "right": b"remote", "both": b"remote"})
        plan = importer.prepare(self.runtime, self.scope, remote, baseline=baseline)
        self.assertEqual({d.name: d.action for d in plan.decisions},
                         {"left": "local", "right": "cloud", "both": "conflict", "delete": "cloud"})
        self.error("conflict", lambda: self.apply(plan, choice={}))
        result = self.apply(plan, choice={"both": "local"})
        self.assertTrue(result.imported)
        state = self.contents()
        self.assertEqual({n: e.blobs["data"] for n, e in state.containers.items()},
                         {"left": b"local", "right": b"remote", "both": b"local"})

    def test_unknown_baseline_does_not_choose_newer_timestamp(self):
        self.install({"profile": b"local"}, modified=10000)
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}, modified=10))
        self.assertEqual(plan.decisions[0].action, "conflict")
        self.error("conflict", lambda: self.apply(plan, choice=None))

    def test_wrong_account_title_scid_or_package_rejects_without_writes(self):
        raw = self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        for fields in ({"xuid": "987654321"}, {"title_id": 4321},
                       {"scid": "11111111-2222-4333-8444-555555555555"}, {"package_family_name": "Other.Game_abc"}):
            with self.lease() as fd:
                self.error("invalid_scope", lambda: importer.apply(self.runtime, replace(self.scope, **fields), plan, runtime_lock_fd=fd))
        self.assertEqual((self.folder / "state.bin").read_bytes(), raw)

    def test_plan_and_baseline_tampering_rejected(self):
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        self.error("invalid_plan", lambda: self.apply(replace(plan, local_digest="invented")))
        baseline = self.apply(plan).baseline
        self.error("invalid_plan", lambda: importer.prepare(self.runtime, self.scope, self.snapshot({}),
                   baseline=replace(baseline, containers=())))

    def test_changed_local_generation_or_bytes_rejects_stale_plan(self):
        self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        newer = self.install({"profile": b"changed"}, generation=8)
        self.error("changed", lambda: self.apply(plan))
        self.assertEqual((self.folder / "state.bin").read_bytes(), newer)

    def test_changed_snapshot_payload_and_replaced_manifest_reject(self):
        snapshot = self.snapshot({"profile": b"cloud"})
        plan = importer.prepare(self.runtime, self.scope, snapshot)
        self.write(snapshot.path / "blobs/00000000.bin", b"other")
        self.error("invalid_snapshot", lambda: self.apply(plan))
        self.assertFalse(self.folder.exists())
        self.write(snapshot.path / "blobs/00000000.bin", b"cloud")
        manifest = json.loads((snapshot.path / "manifest.json").read_text())
        manifest["containers"][0]["etag"] = "changed"
        self.write(snapshot.path / "manifest.json", json.dumps(manifest).encode())
        self.error("changed", lambda: self.apply(plan))

    def test_snapshot_scope_counts_paths_duplicates_and_hashes_checked(self):
        for mode in ("scope", "traversal", "duplicate", "hash", "wire", "extra", "time"):
            with self.subTest(mode=mode):
                snapshot = self.snapshot({"profile": b"cloud"})
                path = snapshot.path / "manifest.json"
                m = json.loads(path.read_text()); row = m["containers"][0]
                if mode == "scope": m["scope_binding"] = "0" * 64
                if mode == "traversal": row["blobs"][0]["file"] = "../escape"
                if mode == "duplicate": m["containers"].append(row.copy())
                if mode == "hash": row["blobs"][0]["sha256"] = "0" * 64
                if mode == "wire": row["name"] = "profile,binary"
                if mode == "extra": self.write(snapshot.path / "extra", b"unexpected")
                if mode == "time": row["client_file_time"] = -1
                self.write(path, json.dumps(m).encode())
                self.error("invalid_snapshot", lambda: importer.prepare(self.runtime, self.scope, snapshot))
        snapshot = self.snapshot({"profile": b"cloud"})
        self.error("invalid_snapshot", lambda: importer.prepare(self.runtime, self.scope, replace(snapshot, total_bytes=999)))

    def test_snapshot_symlink_hardlink_fifo_and_shared_permissions_reject(self):
        for mode in ("symlink", "hardlink", "fifo", "permissions"):
            with self.subTest(mode=mode):
                snapshot = self.snapshot({"profile": b"cloud"})
                target = snapshot.path / "blobs/00000000.bin"
                outside = self.runtime / (mode + "-outside")
                self.write(outside, b"cloud")
                if mode != "permissions": target.unlink()
                if mode == "symlink": target.symlink_to(outside)
                if mode == "hardlink": os.link(outside, target)
                if mode == "fifo": os.mkfifo(target, 0o600)
                if mode == "permissions": target.chmod(0o644)
                with self.assertRaises(importer.CloudImportError):
                    importer.prepare(self.runtime, self.scope, snapshot)
                self.assertEqual(outside.read_bytes(), b"cloud")

    def test_disabled_and_symlinked_local_root_reject_without_enabling(self):
        snapshot = self.snapshot({"profile": b"cloud"})
        (self.private / "local-saves.enabled").unlink()
        self.error("unsupported", lambda: importer.prepare(self.runtime, self.scope, snapshot))
        self.assertFalse((self.private / "local-saves.enabled").exists())
        self.write(self.private / "local-saves.enabled", b"")
        root = self.private / "local-saves"; root.rmdir(); root.symlink_to(self.runtime, target_is_directory=True)
        self.error("local_storage", lambda: importer.prepare(self.runtime, self.scope, snapshot))

    def test_apply_requires_actual_matching_held_runtime_lease(self):
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        self.error("invalid_lock", lambda: importer.apply(self.runtime, self.scope, plan, runtime_lock_fd=None))
        with (self.private / "play.lock").open("r+") as stream:
            self.error("invalid_lock", lambda: importer.apply(self.runtime, self.scope, plan, runtime_lock_fd=stream.fileno()))
        other = self.runtime / "other.lock";self.write(other, b"")
        with self.lease(), other.open("r+") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            self.error("invalid_lock", lambda: importer.apply(self.runtime, self.scope, plan, runtime_lock_fd=stream.fileno()))
        self.assertFalse(self.folder.exists())

    def test_native_byte_range_lock_blocks_import_and_preview(self):
        self.install({"profile": b"local"})
        snapshot = self.snapshot({"profile": b"cloud"})
        plan = importer.prepare(self.runtime, self.scope, snapshot)
        code = 'import fcntl,sys;f=open(sys.argv[1],"r+");fcntl.lockf(f,fcntl.LOCK_EX,1,0);print("ready",flush=True);sys.stdin.read()'
        child = subprocess.Popen([sys.executable, "-c", code, str(self.folder / "writer.lock")],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(child.stdout.readline().strip(), "ready")
            self.error("busy", lambda: importer.prepare(self.runtime, self.scope, snapshot))
            self.error("busy", lambda: self.apply(plan))
        finally:
            child.communicate(timeout=5)
        self.assertEqual(child.returncode, 0)

    def test_changed_local_during_backup_is_detected_before_replace(self):
        self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        original_write = importer._write
        changed = save_state.encode(self.state({"profile": b"raced"}, generation=8))
        def race(parent, name, payload, **kwargs):
            original_write(parent, name, payload, **kwargs)
            if name.startswith(".cloud-import-"):
                self.write(self.folder / "state.bin", changed)
        with patch.object(importer, "_write", side_effect=race):
            self.error("changed", lambda: self.apply(plan))
        self.assertEqual((self.folder / "state.bin").read_bytes(), changed)
        self.assertFalse(list(self.folder.glob(".cloud-import-*")))

    def test_parent_exchange_cannot_redirect_commit(self):
        raw = self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        original_write = importer._write
        moved = self.folder.with_name("moved")
        def exchange(parent, name, payload, **kwargs):
            original_write(parent, name, payload, **kwargs)
            if name.startswith(".cloud-import-"):
                self.folder.rename(moved)
                self.folder.mkdir(mode=0o700)
                self.write(self.folder / "state.bin", b"unrelated")
        with patch.object(importer, "_write", side_effect=exchange):
            self.error("changed", lambda: self.apply(plan))
        self.assertEqual((moved / "state.bin").read_bytes(), raw)
        self.assertEqual((self.folder / "state.bin").read_bytes(), b"unrelated")
        self.assertFalse(list(moved.glob(".cloud-import-*")))

    def test_write_or_backup_failure_preserves_original_and_own_cleanup(self):
        raw = self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        with patch.object(importer, "_write", side_effect=OSError("synthetic write fault")):
            self.error("local_storage", lambda: self.apply(plan))
        self.assertEqual((self.folder / "state.bin").read_bytes(), raw)
        self.assertFalse(list((self.private / "cloud-import-backups").glob(".backup-*")))

    def test_cancel_before_commit_keeps_backup_and_original(self):
        raw = self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        cancel = threading.Event(); original_write = importer._write
        def stop(parent, name, payload, **kwargs):
            original_write(parent, name, payload, **kwargs)
            if name.startswith(".cloud-import-"): cancel.set()
        with patch.object(importer, "_write", side_effect=stop):
            self.error("cancelled", lambda: self.apply(plan, cancel=cancel))
        self.assertEqual((self.folder / "state.bin").read_bytes(), raw)
        self.assertFalse(list(self.folder.glob(".cloud-import-*")))

    def test_post_commit_cancel_and_flush_failure_report_committed_result(self):
        self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        cancel = threading.Event(); actual_replace = os.replace; actual_fsync = os.fsync
        def commit(*args, **kwargs):
            actual_replace(*args, **kwargs); cancel.set()
        def flush(fd):
            if cancel.is_set(): raise OSError("synthetic post-commit fault")
            actual_fsync(fd)
        with patch.object(importer.os, "replace", side_effect=commit), patch.object(importer.os, "fsync", side_effect=flush):
            result = self.apply(plan, cancel=cancel)
        self.assertTrue(result.imported)
        self.assertFalse(result.durability_confirmed)
        self.assertEqual(self.contents().containers["profile"].blobs["data"], b"cloud")

    def test_generation_overflow_and_unsupported_names_never_write(self):
        raw = self.install({"profile": b"local"}, generation=2**64-1)
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        self.error("unsupported", lambda: self.apply(plan))
        self.assertEqual((self.folder / "state.bin").read_bytes(), raw)
        snapshot = self.snapshot({"../escape": b"cloud"})
        self.error("invalid_snapshot", lambda: importer.prepare(self.runtime, self.scope, snapshot))

    def test_baseline_persists_across_process_key_and_local_changes(self):
        self.assertIsNone(importer.load_baseline(self.runtime, self.scope))
        result = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"common"})))
        self.install({"profile": b"later local"}, generation=2)
        with patch.object(importer, "_SEAL_KEY", b"new synthetic process key"):
            baseline = importer.load_baseline(self.runtime, self.scope)
            plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"common"}), baseline=baseline)
            self.assertEqual(plan.decisions[0].action, "local")
        self.assertEqual(result.baseline.containers, baseline.containers)

    def test_plain_snapshot_and_noop_never_create_persistent_baseline(self):
        self.install({"profile": b"same"})
        snapshot = self.snapshot({"profile": b"same"})
        result = self.apply(importer.prepare(self.runtime, self.scope, snapshot))
        self.assertFalse(result.imported)
        self.assertIsNone(result.baseline)
        self.assertIsNone(importer.load_baseline(self.runtime, self.scope))

    def test_corrupt_receipt_backup_or_scope_cannot_become_baseline(self):
        result = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"common"})))
        pointer_path = self.folder / "cloud-baseline.json"
        pointer = json.loads(pointer_path.read_text())
        receipt_path = self.private / "cloud-import-receipts" / pointer["receipt"]
        receipt_bytes = receipt_path.read_bytes()
        receipt_path.chmod(0o600); receipt_path.write_bytes(b"corrupt")
        self.error("invalid_plan", lambda: importer.load_baseline(self.runtime, self.scope))
        receipt_path.write_bytes(receipt_bytes); receipt_path.chmod(0o400)
        imported = self.private / "cloud-import-backups" / result.backup_id / "imported.bin"
        imported.chmod(0o600); original = imported.read_bytes(); imported.write_bytes(b"corrupt")
        self.error("invalid_plan", lambda: importer.load_baseline(self.runtime, self.scope))
        imported.write_bytes(original); imported.chmod(0o400)
        pointer["scope_binding"] = "0" * 64
        self.write(pointer_path, json.dumps(pointer).encode())
        self.error("invalid_plan", lambda: importer.load_baseline(self.runtime, self.scope))

    def test_receipt_failure_after_commit_returns_success_with_warning(self):
        self.install({"profile": b"old"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"new"}))
        with patch.object(importer, "_publish_common", side_effect=OSError("synthetic receipt fault")):
            result = self.apply(plan)
        self.assertTrue(result.imported)
        self.assertFalse(result.durability_confirmed)
        self.assertIsNone(result.baseline)
        self.assertEqual(self.contents().containers["profile"].blobs["data"], b"new")
        self.assertIsNone(importer.load_baseline(self.runtime, self.scope))

    def test_restore_exact_import_revision_retains_both_versions_and_increments(self):
        self.install({"profile": b"old"}, generation=7)
        imported = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"new"})))
        current = (self.folder / "state.bin").read_bytes()
        with self.lease() as fd:
            result = importer.restore(self.runtime, self.scope, imported.backup_id, runtime_lock_fd=fd)
        self.assertTrue(result.imported)
        self.assertEqual(result.generation, 9)
        self.assertEqual(self.contents().containers["profile"].blobs["data"], b"old")
        backup = self.private / "cloud-import-backups" / result.backup_id
        self.assertEqual((backup / "state.bin").read_bytes(), current)
        self.error("changed", lambda: self._restore(imported.backup_id))

    def _restore(self, backup_id, **kwargs):
        with self.lease() as fd:
            return importer.restore(self.runtime, self.scope, backup_id, runtime_lock_fd=fd, **kwargs)

    def test_restore_missing_previous_state_uses_empty_new_generation(self):
        result = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"new"})))
        restored = self._restore(result.backup_id)
        self.assertEqual(restored.generation, 2)
        self.assertEqual(self.contents().containers, {})

    def test_restore_rejects_later_save_foreign_id_and_paths(self):
        result = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"new"})))
        changed = self.install({"profile": b"later"}, generation=2)
        self.error("changed", lambda: self._restore(result.backup_id))
        for candidate in ("../state.bin", str(self.folder), "backup-invalid"):
            self.error("invalid_plan", lambda: self._restore(candidate))
        self.assertEqual((self.folder / "state.bin").read_bytes(), changed)

    def test_restore_corrupt_backup_never_overwrites_current(self):
        self.install({"profile": b"old"})
        result = self.apply(importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"new"})))
        current = (self.folder / "state.bin").read_bytes()
        old = self.private / "cloud-import-backups" / result.backup_id / "state.bin"
        old.chmod(0o600); old.write_bytes(b"corrupt")
        self.error("invalid_plan", lambda: self._restore(result.backup_id))
        self.assertEqual((self.folder / "state.bin").read_bytes(), current)

    def test_export_is_exact_immutable_and_rejects_closed_or_forged_handle(self):
        raw = self.install({"profile": b"local"})
        with self.lease() as fd:
            with importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as export:
                self.assertEqual(export.encoded, raw)
                self.assertEqual(export.local_digest, hashlib.sha256(raw).hexdigest())
                modified = export.state; modified.containers.clear()
                self.assertEqual(set(export.state.containers), {"profile"})
                export.assert_unchanged()
                clone = replace(export)
                self.error("invalid_plan", clone.assert_unchanged)
        self.error("invalid_plan", export.assert_unchanged)

    def test_export_allows_remote_snapshot_validation_without_nested_lock(self):
        self.install({"profile": b"local"})
        snapshot = self.snapshot({"profile": b"remote"})
        with self.lease() as fd, importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as export:
            remote = importer.read_snapshot(self.runtime, self.scope, snapshot)
            self.assertEqual(remote.containers["profile"].blobs["data"], b"remote")
            self.error("busy", lambda: importer.load_baseline(self.runtime, self.scope))
            export.assert_unchanged()

    def test_export_keeps_native_lock_until_context_exit(self):
        self.install({"profile": b"local"})
        code = ('import fcntl,sys;f=open(sys.argv[1],"r+");'
                '\ntry: fcntl.lockf(f,fcntl.LOCK_EX|fcntl.LOCK_NB,1,0)'
                '\nexcept BlockingIOError: sys.exit(3)')
        with self.lease() as fd:
            with importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd):
                # A prohibited nested call must not close another descriptor to
                # writer.lock, which would release POSIX locks for this process.
                self.error("busy", lambda: importer.load_baseline(self.runtime, self.scope))
                result = subprocess.run([sys.executable, "-c", code, str(self.folder / "writer.lock")], capture_output=True)
                self.assertEqual(result.returncode, 3)
            result = subprocess.run([sys.executable, "-c", code, str(self.folder / "writer.lock")], capture_output=True)
            self.assertEqual(result.returncode, 0)

    def test_export_preserves_remote_errors_and_rechecks_local_bytes(self):
        self.install({"profile": b"local"})
        class RemoteFailure(Exception):
            pass
        with self.lease() as fd:
            with self.assertRaises(RemoteFailure):
                with importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd):
                    raise RemoteFailure()
            with importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as export:
                self.install({"profile": b"changed"}, generation=8)
                self.error("changed", export.assert_unchanged)

    def test_default_apply_never_resolves_unknown_conflict_as_cloud(self):
        self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"remote"}))
        with self.lease() as fd:
            self.error("conflict", lambda: importer.apply(self.runtime, self.scope, plan, runtime_lock_fd=fd))

    def test_real_orchestrator_receipt_persists_common_state_under_export_lock(self):
        from flightdeck import cloud_write
        from tests.test_cloud_write import Operations
        raw = self.install({"profile": b"local"})
        operations = Operations(save_state.State(0))
        operations.scope = self.scope
        with self.lease() as fd, importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as export:
            receipt = cloud_write.upload(self.scope, operations, operations.read, export.state,
                        expected_remote_digest=save_state.content_digest(operations.remote),
                        assert_local_unchanged=export.assert_unchanged)
            baseline = importer.record_common(export, receipt)
            export.assert_unchanged()
        self.assertEqual((self.folder / "state.bin").read_bytes(), raw)
        restored = importer.load_baseline(self.runtime, self.scope)
        self.assertEqual(restored.containers, baseline.containers)
        self.assertEqual(save_state.content_digest(operations.remote), save_state.content_digest(self.contents()))

    def test_invalid_or_rebound_remote_receipt_never_persists_baseline(self):
        from flightdeck import cloud_write
        from tests.test_cloud_write import Operations
        self.install({"profile": b"local"})
        operations = Operations(self.contents()); operations.scope = self.scope
        with self.lease() as fd, importer.export_local(self.runtime, self.scope, runtime_lock_fd=fd) as export:
            receipt = cloud_write.upload(self.scope, operations, operations.read, export.state,
                        expected_remote_digest=export.content_digest, assert_local_unchanged=export.assert_unchanged)
            for bad in (object(), replace(receipt, committed=False), replace(receipt, scope_binding="wrong"),
                        replace(receipt, source_content_digest="0" * 64)):
                self.error("invalid_plan", lambda: importer.record_common(export, bad))
        self.assertIsNone(importer.load_baseline(self.runtime, self.scope))

    def test_descriptor_cleanup_failure_after_commit_is_not_false_failure(self):
        self.install({"profile": b"local"})
        plan = importer.prepare(self.runtime, self.scope, self.snapshot({"profile": b"cloud"}))
        original_close, original_publish = os.close, importer._publish_common
        published = False
        injected = False
        def publish(*args, **kwargs):
            nonlocal published
            original_publish(*args, **kwargs)
            published = True
        def closed(fd):
            nonlocal injected
            original_close(fd)
            if published and not injected:
                injected = True
                raise OSError("synthetic close fault")
        with patch.object(importer, "_publish_common", side_effect=publish), patch.object(importer.os, "close", side_effect=closed):
            result = self.apply(plan)
        self.assertTrue(result.imported)
        self.assertFalse(result.durability_confirmed)
        self.assertEqual(self.contents().containers["profile"].blobs["data"], b"cloud")
        self.assertIsNotNone(importer.load_baseline(self.runtime, self.scope))


if __name__ == "__main__":
    unittest.main()
