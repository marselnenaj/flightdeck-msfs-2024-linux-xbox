# SPDX-License-Identifier: MIT
"""Synthetic reboot-marker and runtime-lease tests; no actual processes or saves."""
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import cloud_process_guard as guard


BOOT_A = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
BOOT_B = "11111111-2222-4333-8444-555555555555"


class CloudProcessGuardTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="flightdeck-process-guard-")
        self.addCleanup(temporary.cleanup)
        self.runtime = Path(temporary.name)
        self.private = self.runtime / "private"
        self.private.mkdir(mode=0o700)
        self.marker = self.private / guard._NAME
        fd = os.open(self.private / "play.lock", os.O_CREAT | os.O_RDWR, 0o600)
        os.close(fd)
        self.boot_reader = guard._boot_id
        mock = patch.object(guard, "_boot_id", return_value=BOOT_A)
        self.boot = mock.start()
        self.addCleanup(mock.stop)

    @contextmanager
    def lease(self):
        with (self.private / "play.lock").open("r+") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield stream.fileno()

    def write(self, payload):
        self.marker.write_bytes(payload)
        self.marker.chmod(0o600)

    def test_absence_is_read_only(self):
        with self.lease() as fd:
            guard.check(self.runtime, fd)
        self.assertEqual({x.name for x in self.private.iterdir()}, {"play.lock"})

    def test_mark_is_durable_private_minimal_and_idempotent(self):
        synced = []
        real_sync = os.fsync
        def sync(fd):
            synced.append(os.readlink(f"/proc/self/fd/{fd}"))
            real_sync(fd)
        with self.lease() as fd, patch.object(guard.os, "fsync", side_effect=sync):
            guard.mark(self.runtime, fd)
            original = self.marker.read_bytes()
            guard.mark(self.runtime, fd)
            self.assertEqual(self.marker.read_bytes(), original)
            with self.assertRaises(guard.UnsafeSessionError):
                guard.check(self.runtime, fd)
        self.assertEqual(json.loads(original), {"schema": 1, "boot_id": BOOT_A})
        self.assertEqual(self.marker.stat().st_mode & 0o777, 0o600)
        self.assertIn(str(self.private), synced)
        self.assertTrue(any(".interrupted-process-" in x for x in synced))

    def test_only_confirmed_boot_change_clears_marker(self):
        with self.lease() as fd:
            guard.mark(self.runtime, fd)
            for _ in range(2):
                with self.assertRaises(guard.UnsafeSessionError):
                    guard.check(self.runtime, fd)
                self.assertTrue(self.marker.exists())
            self.boot.return_value = BOOT_B
            guard.check(self.runtime, fd)
        self.assertFalse(self.marker.exists())

    def test_explicit_completion_clears_current_marker_under_same_lease(self):
        with self.lease() as fd:
            guard.mark(self.runtime, fd)
            guard.clear(self.runtime, fd)
            guard.check(self.runtime, fd)
        self.assertFalse(self.marker.exists())

    def test_clear_requires_valid_current_marker_and_held_lease(self):
        with self.lease() as fd:
            with self.assertRaises(guard.UnsafeSessionError):
                guard.clear(self.runtime, fd)
            guard.mark(self.runtime, fd)
        with (self.private / "play.lock").open("r+") as unheld:
            with self.assertRaises(guard.UnsafeSessionError):
                guard.clear(self.runtime, unheld.fileno())
        self.boot.return_value = BOOT_B
        with self.lease() as fd:
            with self.assertRaises(guard.UnsafeSessionError):
                guard.clear(self.runtime, fd)
        self.assertTrue(self.marker.exists())

    def test_bad_marker_never_cleared_or_overwritten_even_after_reboot(self):
        self.boot.return_value = BOOT_B
        for data in (b"not json", b"{}", b"[]", b" " * 513,
                     json.dumps({"schema": True, "boot_id": BOOT_A}).encode(),
                     json.dumps({"schema": 1, "boot_id": "malformed"}).encode(),
                     json.dumps({"schema": 1, "boot_id": BOOT_A, "unexpected": True}).encode(),
                     b'{"schema":1,"schema":1,"boot_id":"' + BOOT_A.encode() + b'"}'):
            with self.subTest(data_type=len(data)):
                self.write(data)
                with self.lease() as fd:
                    for operation in (guard.check, guard.mark, guard.clear):
                        with self.assertRaises(guard.UnsafeSessionError):
                            operation(self.runtime, fd)
                self.assertEqual(self.marker.read_bytes(), data)

    def test_unheld_and_different_open_description_do_not_grant_access(self):
        with (self.private / "play.lock").open("r+") as other:
            for operation in (guard.check, guard.mark):
                with self.assertRaises(guard.UnsafeSessionError):
                    operation(self.runtime, other.fileno())
            with self.lease() as held:
                for operation in (guard.check, guard.mark):
                    with self.assertRaises(guard.UnsafeSessionError):
                        operation(self.runtime, other.fileno())
                guard.mark(self.runtime, held)

    def test_symlink_hardlink_fifo_directory_and_public_mode_rejected(self):
        foreign = self.runtime / "foreign"
        foreign.write_bytes(b"unrelated")
        foreign.chmod(0o600)
        for kind in ("symlink", "hardlink", "fifo", "directory", "public"):
            with self.subTest(kind=kind):
                if kind == "symlink": self.marker.symlink_to(foreign)
                elif kind == "hardlink": os.link(foreign, self.marker)
                elif kind == "fifo": os.mkfifo(self.marker, 0o600)
                elif kind == "directory": self.marker.mkdir(mode=0o700)
                else:
                    self.write(json.dumps({"schema": 1, "boot_id": BOOT_A}).encode())
                    self.marker.chmod(0o644)
                with self.lease() as fd:
                    for operation in (guard.check, guard.mark, guard.clear):
                        with self.assertRaises(guard.UnsafeSessionError):
                            operation(self.runtime, fd)
                if kind == "directory": self.marker.rmdir()
                else: self.marker.unlink()
        self.assertEqual(foreign.read_bytes(), b"unrelated")

    def test_unavailable_boot_id_fails_closed_with_or_without_marker(self):
        for exists in (False, True):
            with self.subTest(exists=exists), self.lease() as fd:
                if exists:
                    self.boot.side_effect = None
                    guard.mark(self.runtime, fd)
                self.boot.side_effect = OSError("synthetic private value")
                for operation in (guard.check, guard.mark):
                    with self.assertRaises(guard.UnsafeSessionError) as error:
                        operation(self.runtime, fd)
                    self.assertNotIn("synthetic private value", str(error.exception))
                self.assertEqual(self.marker.exists(), exists)

    def test_failed_write_never_publishes_partial_marker(self):
        with self.lease() as fd, patch.object(guard.ci, "_write", side_effect=OSError("synthetic")):
            with self.assertRaises(guard.UnsafeSessionError):
                guard.mark(self.runtime, fd)
        self.assertFalse(self.marker.exists())
        self.assertFalse(list(self.private.glob(".interrupted-process-*")))

    def test_exchanged_private_directory_never_receives_marker(self):
        original = guard.ci._write
        previous = self.runtime / "private-before-exchange"
        def exchange(fd, name, data, **kwargs):
            original(fd, name, data, **kwargs)
            self.private.rename(previous)
            self.private.mkdir(mode=0o700)
        with self.lease() as fd, patch.object(guard.ci, "_write", side_effect=exchange):
            with self.assertRaises(guard.UnsafeSessionError):
                guard.mark(self.runtime, fd)
        self.assertEqual(list(self.private.iterdir()), [])
        self.assertFalse((previous / guard._NAME).exists())
        self.assertFalse(list(previous.glob(".interrupted-process-*")))

    def test_parent_flush_failure_after_publish_still_blocks_next_start(self):
        parent = self.private.stat()
        real_sync = os.fsync
        def sync(fd):
            info = os.fstat(fd)
            if (info.st_dev, info.st_ino) == (parent.st_dev, parent.st_ino):
                raise OSError("synthetic flush fault")
            real_sync(fd)
        with self.lease() as fd:
            with patch.object(guard.os, "fsync", side_effect=sync):
                with self.assertRaises(guard.UnsafeSessionError):
                    guard.mark(self.runtime, fd)
            with self.assertRaises(guard.UnsafeSessionError):
                guard.check(self.runtime, fd)
        self.assertTrue(self.marker.exists())

    def test_existing_old_boot_marker_can_be_marked_for_current_boot(self):
        with self.lease() as fd:
            guard.mark(self.runtime, fd)
            self.boot.return_value = BOOT_B
            guard.mark(self.runtime, fd)
            with self.assertRaises(guard.UnsafeSessionError):
                guard.check(self.runtime, fd)
        self.assertEqual(json.loads(self.marker.read_bytes())["boot_id"], BOOT_B)

    def test_kernel_boot_id_parser_is_bounded_and_strict(self):
        source = self.runtime / "synthetic-boot-id"
        for payload, valid in ((BOOT_A.encode() + b"\n", True), (BOOT_B.encode(), True),
                               (b"", False), (b"x" * 81, False), (BOOT_A.encode() + b"\nextra", False),
                               (b"00000000-0000-0000-0000-000000000000", False)):
            source.write_bytes(payload)
            with patch.object(guard, "_BOOT_ID", str(source)):
                if valid: self.assertEqual(self.boot_reader(), payload.decode().strip())
                else:
                    with self.assertRaises(guard.UnsafeSessionError):
                        self.boot_reader()


if __name__ == "__main__":
    unittest.main()
