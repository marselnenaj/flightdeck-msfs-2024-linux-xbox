# SPDX-License-Identifier: MIT
"""Synthetic archives only: no real binaries, extraction, execution or network."""
import contextlib
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("full_release", ROOT / "scripts/full-installer-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def encoded(value):
    return (json.dumps(value, sort_keys=True) + "\n").encode()


def archive(entries):
    stream = io.BytesIO()
    with gzip.GzipFile(fileobj=stream, mode="wb", mtime=0, filename="") as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as target:
            for name, value in entries:
                entry = tarfile.TarInfo(name)
                entry.mode = 0o777
                entry.uid = entry.gid = 1234
                entry.mtime = 123456
                if isinstance(value, bytes):
                    entry.size = len(value)
                    target.addfile(entry, io.BytesIO(value))
                else:
                    entry.type = value
                    entry.linkname = "outside"
                    target.addfile(entry)
    return stream.getvalue()


class FullInstaller(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.root_patch = mock.patch.object(release, "ROOT", self.root)
        self.root_patch.start()
        self.source = self.root / "source.tar.gz"
        self.native = self.root / "native.tar.gz"
        self.output = self.root / "build/Flightdeck-Linux-x86_64.tar.gz"
        self.binaries = {name: b"synthetic native bytes: " + name.encode() for name in release.NATIVE_FILES}
        self.hashes = {name: release.digest(data) for name, data in self.binaries.items()}
        self.notice = b"Original synthetic fixture notice. No third-party program included.\n"
        self.native_contents = {**self.binaries, "THIRD-PARTY-NOTICES.txt": self.notice,
                                "manifest.json": encoded({"format": 1, "files": self.hashes})}
        self.prepare()

    def tearDown(self):
        self.root_patch.stop()
        self.temporary.cleanup()

    def prepare(self, native_entries=None, lock_changes=None):
        native_bytes = archive(native_entries if native_entries is not None else self.native_contents.items())
        self.native.write_bytes(native_bytes)
        native_lock = {"features": list(release.NATIVE_FEATURES), "files": self.hashes, "archive_sha256": release.digest(native_bytes),
                       "notice_sha256": release.digest(self.notice)}
        native_lock.update(lock_changes or {})
        self.sources = {"README.md": b"Synthetic source package.\n", "install.sh": b"#!/bin/sh\nexit 0\n",
                        "Install Flightdeck.desktop": b"[Desktop Entry]\nType=Application\n",
                        "ui/app.js": b"export const fixture = true;\n",
                        "compat/bootstrap.lock.json": encoded({"format": 1, "native": native_lock})}
        self.source_manifest = encoded({"format": 1, "status": "PASS", "files": {
            name: release.digest(data) for name, data in self.sources.items()}})
        self.source_entries = [(release.SOURCE_ROOT + name, data) for name, data in self.sources.items()]
        self.source_entries.append((release.SOURCE_ROOT + "SOURCE-MANIFEST.json", self.source_manifest))
        self.source.write_bytes(archive(self.source_entries))

    def create(self, output=None):
        with contextlib.redirect_stdout(io.StringIO()):
            return release.create(self.source, self.native, output or self.output)

    def rejected(self):
        with self.assertRaises((ValueError, OSError)):
            self.create()
        self.assertFalse(self.output.exists())

    def test_valid_reproducible_archive_has_exact_bytes_modes_and_zero_metadata(self):
        # A mutable checkout lock must never override the verified source lock.
        (self.root / "compat").mkdir()
        (self.root / "compat/bootstrap.lock.json").write_text("not a release lock")
        report = self.create()
        second = self.root / "build/another-name.tar.gz"
        self.create(second)
        self.assertEqual(self.output.read_bytes(), second.read_bytes())
        self.assertEqual(self.output.read_bytes()[4:8], b"\0" * 4)
        self.assertEqual(report["sha256"], hashlib.sha256(self.output.read_bytes()).hexdigest())
        with tarfile.open(self.output, "r:gz") as result:
            expected = dict(self.source_entries)
            expected.update({release.NATIVE_ROOT + name: data for name, data in self.native_contents.items()})
            self.assertEqual(result.getnames(), sorted(expected))
            for member in result:
                self.assertTrue(member.isfile())
                self.assertEqual(result.extractfile(member).read(), expected[member.name])
                executable = member.name in {release.SOURCE_ROOT + "install.sh", release.SOURCE_ROOT + "Install Flightdeck.desktop",
                                             *(release.NATIVE_ROOT + name for name in release.EXECUTABLE_NATIVE)}
                self.assertEqual(member.mode, 0o755 if executable else 0o644)
                self.assertEqual((member.uid, member.gid, member.mtime, member.uname, member.gname), (0, 0, 0, "", ""))

    def test_source_manifest_rejects_changed_missing_and_extra_files(self):
        for change in ("changed", "missing", "extra"):
            with self.subTest(change=change):
                self.prepare()
                entries = list(self.source_entries)
                if change == "changed":
                    entries[0] = (entries[0][0], b"changed after review")
                elif change == "missing":
                    entries.pop(0)
                else:
                    entries.append((release.SOURCE_ROOT + "unexpected.txt", b"not reviewed"))
                self.source.write_bytes(archive(entries))
                self.rejected()

    def test_native_outer_inner_notice_and_manifest_hashes_are_all_checked(self):
        for change in ("archive", "binary", "notice", "manifest", "lock-files"):
            with self.subTest(change=change):
                self.prepare()
                if change == "archive":
                    self.native.write_bytes(self.native.read_bytes() + b"different compressed archive")
                elif change == "lock-files":
                    self.prepare(lock_changes={"files": {"other": "0" * 64}})
                else:
                    contents = dict(self.native_contents)
                    key = next(iter(self.binaries)) if change == "binary" else "THIRD-PARTY-NOTICES.txt" if change == "notice" else "manifest.json"
                    contents[key] = encoded({"format": 1, "files": {}}) if change == "manifest" else b"replaced bytes"
                    # Recompute the outer archive pin, so inner checks must catch it.
                    self.prepare(contents.items())
                self.rejected()

    def test_native_archive_requires_exactly_eight_unique_regular_members(self):
        entries = list(self.native_contents.items())
        for altered in (entries[:-1], entries + [("extra.txt", b"extra")], entries + [entries[0]]):
            with self.subTest(count=len(altered)):
                self.prepare(altered)
                self.rejected()

    def test_cloud_helper_requires_real_capability_and_its_own_pin(self):
        for features in (None, [], "connected-storage-read-v1", [True], ["connected-storage-read-v1"], ["connected-storage-sync-v1"]):
            with self.subTest(features=features):
                self.prepare(lock_changes={"features": features})
                self.rejected()
        contents = dict(self.native_contents)
        contents["bin/flightdeck-connected-storage.exe"] = b"changed helper"
        self.prepare(contents.items())
        self.rejected()

    def test_unsafe_paths_links_special_files_and_parent_collisions_are_rejected(self):
        names = ("/absolute", "flightdeck-linux/../escape", "flightdeck-linux//alias",
                 "flightdeck-linux/./alias", "flightdeck-linux/back\\slash", "different-root/file")
        for name in names:
            with self.subTest(name=name):
                self.source.write_bytes(archive(self.source_entries + [(name, b"unsafe")]))
                self.rejected()
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.DIRTYPE, tarfile.CHRTYPE):
            with self.subTest(kind=kind):
                self.source.write_bytes(archive(self.source_entries + [(release.SOURCE_ROOT + "special", kind)]))
                self.rejected()
        self.source.write_bytes(archive(self.source_entries + [self.source_entries[0]]))
        self.rejected()
        self.source.write_bytes(archive(self.source_entries + [(release.SOURCE_ROOT + "ui", b"parent file")]))
        self.rejected()
        self.source.write_bytes(archive(self.source_entries + [(release.NATIVE_ROOT + "extra", b"reserved")]))
        self.rejected()

    def test_compressed_expanded_member_count_and_json_limits(self):
        for constant in ("SOURCE_ARCHIVE_MAX", "NATIVE_ARCHIVE_MAX", "SOURCE_FILE_MAX",
                         "SOURCE_TOTAL_MAX", "NATIVE_FILE_MAX", "NATIVE_TOTAL_MAX", "JSON_MAX", "MEMBER_MAX"):
            with self.subTest(limit=constant), mock.patch.object(release, constant, 1):
                self.rejected()

    def test_output_must_be_new_and_under_build_and_failed_write_is_not_published(self):
        with self.assertRaises(ValueError):
            self.create(self.root / "outside.tar.gz")
        self.output.parent.mkdir()
        self.output.write_bytes(b"keep this archive")
        with self.assertRaises(ValueError):
            self.create()
        self.assertEqual(self.output.read_bytes(), b"keep this archive")
        self.output.unlink()
        elsewhere = self.root / "elsewhere"
        elsewhere.mkdir()
        (self.output.parent / "link").symlink_to(elsewhere, target_is_directory=True)
        with self.assertRaises(ValueError):
            self.create(self.output.parent / "link/escape.tar.gz")
        with mock.patch.object(tarfile.TarFile, "addfile", side_effect=OSError("synthetic write failure")):
            self.rejected()
        self.assertEqual(list(self.output.parent.glob(".flightdeck-full-*")), [])

    def test_malformed_duplicate_json_and_nonregular_inputs_fail_closed(self):
        entries = list(self.source_entries)
        entries[-1] = (entries[-1][0], b'{"format":1,"format":1,"status":"PASS","files":{}}')
        self.source.write_bytes(archive(entries))
        self.rejected()
        self.source.write_bytes(b"not a gzip archive")
        self.rejected()
        self.prepare()
        original = self.root / "actual-source.tar.gz"
        self.source.rename(original)
        self.source.symlink_to(original)
        self.rejected()


if __name__ == "__main__":
    unittest.main()
