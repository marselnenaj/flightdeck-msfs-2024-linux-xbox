# SPDX-License-Identifier: MIT
"""Private helper prefix cache tests; no Wine, account, network or game calls."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import cloud_prefix as cache

BOOT_A = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
BOOT_B = "11111111-2222-4333-8444-555555555555"
KEY = "a" * 64
LIBRARIES = {"xgameruntime.dll": b"synthetic proxy", "xgameruntime_original.dll": b"synthetic original",
             "xodus_store_test.dll": b"synthetic native"}


def wine_layout(prefix):
    """Create only the relevant shape of a fresh, uncredentialed Wine prefix."""
    (prefix / "drive_c/windows/system32").mkdir(parents=True, mode=0o700)
    for name in ("system.reg", "user.reg", "userdef.reg"):
        (prefix / name).write_bytes(b"WINE REGISTRY Version 2\n;; synthetic\n")
    (prefix / "dosdevices").mkdir(mode=0o700)
    (prefix / "dosdevices/c:").symlink_to("../drive_c")
    (prefix / "dosdevices/z:").symlink_to("/")


class CloudPrefixTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="flightdeck-prefix-test-")
        self.addCleanup(temporary.cleanup)
        self.runtime = Path(temporary.name)
        (self.runtime / "private").mkdir(mode=0o700)
        mock = patch.object(cache, "_boot_id", return_value=BOOT_A)
        self.boot = mock.start(); self.addCleanup(mock.stop)

    @property
    def cached(self):
        return self.runtime / "private/cloud-helper-prefixes" / KEY

    def cold(self, lease):
        self.assertFalse(lease.reusable)
        wine_layout(lease.path)
        lease.install_libraries()

    def ready(self):
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.cold(lease)
            lease.complete()
        return lease.path

    def test_clean_prefix_reused_only_after_explicit_confirmed_cleanup(self):
        original = self.ready()
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.assertTrue(lease.reusable)
            self.assertEqual(lease.path, original)
            record = json.loads((self.cached / "prefix-state.json").read_bytes())
            self.assertEqual(record["phase"], "in_use")
            lease.complete()
        self.assertEqual(json.loads((self.cached / "prefix-state.json").read_bytes())["phase"], "ready")
        self.assertEqual((self.cached / "prefix-state.json").stat().st_mode & 0o777, 0o600)

    def test_same_boot_abandoned_prefix_is_not_reused_or_removed(self):
        with cache.session(self.runtime, KEY, LIBRARIES) as interrupted:
            self.cold(interrupted)
        original = (interrupted.path / "system.reg").read_bytes()
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.assertNotEqual(lease.path, interrupted.path)
            self.cold(lease)
            lease.complete()
        self.assertFalse(lease.work.exists())
        self.assertEqual((interrupted.path / "system.reg").read_bytes(), original)
        self.assertEqual(json.loads((self.cached / "prefix-state.json").read_bytes())["phase"], "in_use")

    def test_reboot_allows_rebuild_of_owned_interrupted_generation(self):
        with cache.session(self.runtime, KEY, LIBRARIES) as interrupted:
            self.cold(interrupted)
            (interrupted.path / "old-marker").write_text("synthetic")
        self.boot.return_value = BOOT_B
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.assertEqual(lease.path, interrupted.path)
            self.assertFalse(lease.path.exists())
            self.cold(lease); lease.complete()
        self.assertFalse((lease.path / "old-marker").exists())

    def test_concurrent_open_falls_back_without_sharing_active_prefix(self):
        self.ready()
        with cache.session(self.runtime, KEY, LIBRARIES) as first:
            with cache.session(self.runtime, KEY, LIBRARIES) as second:
                self.assertTrue(first.reusable)
                self.assertFalse(second.reusable)
                self.assertNotEqual(first.path, second.path)
                self.cold(second); second.complete()
            first.complete()

    def test_binding_change_creates_independent_cold_prefix(self):
        original = self.ready()
        with cache.session(self.runtime, "b" * 64, LIBRARIES) as lease:
            self.assertNotEqual(lease.path, original)
            self.cold(lease); lease.complete()
        self.assertTrue(original.exists())

    def test_changed_dll_registry_or_drive_mapping_invalidates_clean_cache(self):
        self.ready()
        for changed in ("dll", "registry", "mapping"):
            prefix = self.cached / "prefix"
            if changed == "dll": (prefix / "drive_c/windows/system32/xgameruntime.dll").write_bytes(b"bad")
            elif changed == "registry": (prefix / "user.reg").write_bytes(b"bad")
            else:
                (prefix / "dosdevices/z:").unlink()
                (prefix / "dosdevices/z:").symlink_to("/tmp")
            with self.subTest(changed=changed), cache.session(self.runtime, KEY, LIBRARIES) as lease:
                self.assertFalse(lease.reusable)
                self.assertFalse(lease.path.exists())
                self.cold(lease); lease.complete()

    def test_library_install_replaces_links_without_modifying_their_target(self):
        outside = self.runtime / "unrelated.dll"; outside.write_bytes(b"untouched")
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            wine_layout(lease.path)
            target = lease.path / "drive_c/windows/system32/xgameruntime.dll"
            target.symlink_to(outside)
            lease.install_libraries()
            self.assertFalse(target.is_symlink())
            self.assertEqual(target.read_bytes(), LIBRARIES["xgameruntime.dll"])
            lease.complete()
        self.assertEqual(outside.read_bytes(), b"untouched")

    def test_symlink_cache_or_unrecognised_prefix_only_causes_fresh_fallback(self):
        outside = self.runtime / "unrelated"; outside.mkdir()
        parent = self.runtime / "private/cloud-helper-prefixes"
        parent.symlink_to(outside)
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.cold(lease); lease.complete()
        self.assertEqual(list(outside.iterdir()), [])
        parent.unlink(); self.cached.mkdir(parents=True, mode=0o700)
        (self.cached / "prefix").mkdir()
        (self.cached / "prefix/unknown").write_text("preserve")
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.assertNotEqual(lease.work, self.cached)
            self.cold(lease); lease.complete()
        self.assertEqual((self.cached / "prefix/unknown").read_text(), "preserve")

    def test_symlink_fifo_or_invalid_marker_is_never_followed_or_replaced(self):
        self.ready()
        marker = self.cached / "prefix-state.json"
        unrelated = self.runtime / "unrelated"; unrelated.write_text("preserve")
        for kind in ("symlink", "fifo", "invalid"):
            marker.unlink()
            if kind == "symlink": marker.symlink_to(unrelated)
            elif kind == "fifo": os.mkfifo(marker, 0o600)
            else: marker.write_bytes(b"bad"); marker.chmod(0o600)
            with self.subTest(kind=kind), cache.session(self.runtime, KEY, LIBRARIES) as lease:
                self.assertNotEqual(lease.work, self.cached)
                self.cold(lease); lease.complete()
        self.assertEqual(unrelated.read_text(), "preserve")

    def test_optional_ready_publication_failure_never_masks_body_success(self):
        with cache.session(self.runtime, KEY, LIBRARIES) as lease:
            self.cold(lease)
            with patch.object(cache, "_state", side_effect=OSError("synthetic")):
                lease.complete()
        self.assertEqual(json.loads((self.cached / "prefix-state.json").read_bytes())["phase"], "in_use")

    def test_body_exception_preserved_and_dirty_prefix_retained(self):
        error = RuntimeError("synthetic partial operation")
        with self.assertRaises(RuntimeError) as caught:
            with cache.session(self.runtime, KEY, LIBRARIES) as lease:
                self.cold(lease)
                raise error
        self.assertIs(caught.exception, error)
        self.assertTrue(lease.path.exists())

    def test_unavailable_boot_identity_only_disables_cache(self):
        with patch.object(cache, "_boot_id", side_effect=OSError("synthetic")):
            with cache.session(self.runtime, KEY, LIBRARIES) as lease:
                self.assertNotEqual(lease.work, self.cached)
                self.cold(lease); lease.complete()
        self.assertFalse(lease.work.exists())

    def test_portable_cleanup_unlinks_drive_links_and_fifo_without_following(self):
        outside = self.runtime / "unrelated"; outside.mkdir()
        (outside / "preserve").write_bytes(b"unchanged")
        owned = self.runtime / "owned"; owned.mkdir(mode=0o700)
        (owned / "drive-link").symlink_to(outside)
        os.mkfifo(owned / "fifo", 0o600)
        (owned / "nested").mkdir(mode=0o700)
        (owned / "nested/file").write_bytes(b"owned")
        parent = os.open(self.runtime, os.O_RDONLY | os.O_DIRECTORY)
        try:
            cache._remove_tree(parent, "owned")
            self.assertFalse(owned.exists())
            self.assertEqual((outside / "preserve").read_bytes(), b"unchanged")
            owned.symlink_to(outside)
            with self.assertRaises(OSError):
                cache._remove_tree(parent, "owned")
            self.assertTrue(owned.is_symlink())
        finally:
            os.close(parent)

    def test_portable_cleanup_rejects_swapped_opened_directory(self):
        owned = self.runtime / "owned"; owned.mkdir(mode=0o700)
        (owned / "preserve").write_bytes(b"unchanged")
        parent = os.open(self.runtime, os.O_RDONLY | os.O_DIRECTORY)
        original = cache._directory
        def swapped(fd, name):
            opened = original(fd, name)
            os.rename(name, name + "-moved", src_dir_fd=fd, dst_dir_fd=fd)
            os.mkdir(name, 0o700, dir_fd=fd)
            return opened
        try:
            with patch.object(cache, "_directory", side_effect=swapped):
                with self.assertRaises(OSError):
                    cache._remove_tree(parent, "owned")
            self.assertTrue(owned.is_dir())
        finally:
            os.close(parent)


if __name__ == "__main__":
    unittest.main()
