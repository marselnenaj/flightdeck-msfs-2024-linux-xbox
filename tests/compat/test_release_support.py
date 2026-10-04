# SPDX-License-Identifier: MIT
"""Synthetic native archives: retain the retired installer's boundary checks."""
import gzip
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_support", ROOT / "scripts/release-support.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def encoded(value):
    return json.dumps(value, sort_keys=True).encode()


def archive(entries):
    stream = io.BytesIO()
    with gzip.GzipFile(fileobj=stream, mode="wb", mtime=0, filename="") as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as target:
            for name, value in entries:
                entry = tarfile.TarInfo(name)
                if isinstance(value, bytes):
                    entry.size = len(value)
                    target.addfile(entry, io.BytesIO(value))
                else:
                    entry.type = value
                    entry.linkname = "outside"
                    target.addfile(entry)
    return stream.getvalue()


class NativeComponents(unittest.TestCase):
    def setUp(self):
        self.members = {name: ("synthetic " + name).encode() for name in release.NATIVE_FILES}
        hashes = {name: release.digest(data) for name, data in self.members.items()}
        self.members["THIRD-PARTY-NOTICES.txt"] = b"Synthetic fixture notice."
        self.members["manifest.json"] = encoded({"format": 1, "files": hashes})
        self.lock = {"features": list(release.NATIVE_FEATURES), "files": hashes,
                     "archive_sha256": release.digest(archive(self.members.items())),
                     "notice_sha256": release.digest(self.members["THIRD-PARTY-NOTICES.txt"])}

    def verify(self, entries):
        release.verify_native(release.read_archive(archive(entries)), self.lock)

    def test_exact_native_members_and_notices_survive_without_extraction(self):
        contents = release.read_archive(archive(self.members.items()))
        self.assertEqual(contents, self.members)
        release.verify_native(contents, self.lock)

    def test_changed_missing_extra_duplicate_and_unsafe_members_fail(self):
        entries = list(self.members.items())
        changes = [entries[:-1], entries + [entries[0]], entries + [("extra", b"x")]]
        for name in self.members:
            changes.append([(n, b"changed" if n == name else data) for n, data in entries])
        for name in ("/absolute", "../escape", "bin//xodus-cli", "bin/./xodus-cli", "bin/back\\slash", "bin"):
            changes.append(entries[:-1] + [(name, b"x")])
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.DIRTYPE, tarfile.CHRTYPE):
            changes.append([(entries[0][0], kind), *entries[1:]])
        for changed in changes:
            with self.subTest(changed=changed[0][0]), self.assertRaises(ValueError):
                self.verify(changed)

    def test_capability_and_hash_pins_are_required(self):
        for features in (None, [], "connected-storage-read-v1", [True], ["connected-storage-read-v1"]):
            with self.subTest(features=features), self.assertRaises(ValueError):
                release.verify_native(self.members, {**self.lock, "features": features})
        for changes in ({"files": {"other": "0" * 64}}, {"archive_sha256": "bad"}, {"notice_sha256": "0" * 64}):
            with self.assertRaises(ValueError):
                release.verify_native(self.members, {**self.lock, **changes})

    def test_bounded_inputs_and_strict_json(self):
        for constant in ("NATIVE_FILE_MAX", "NATIVE_TOTAL_MAX", "NATIVE_NOTICE_MAX", "JSON_MAX"):
            with self.subTest(limit=constant), mock.patch.object(release, constant, 1), self.assertRaises(ValueError):
                self.verify(self.members.items())
        for raw in (b'{"format":1,"format":1}', b'{"bad":NaN}', b'[]', b'broken'):
            with self.assertRaises(ValueError):
                release.read_json(raw, "fixture")
        with self.assertRaises(ValueError):
            release.read_archive(b"not an archive")
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "input"
            file.write_bytes(b"1234")
            with self.assertRaises(ValueError):
                release.read_regular(file, 3)
            link = file.with_name("link")
            link.symlink_to(file)
            with self.assertRaises(OSError):
                release.read_regular(link, 100)

    def test_graphics_bundle_requires_exact_files_hashes_and_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ("d3d12.dll", "d3d12core.dll", "dxgi.dll", "d3d11.dll", "d3d10core.dll")
            files = {name: release.digest(name.encode()) for name in names}
            lock = {"schema": 2, "base": files, "files": files, "license_sha256": release.digest(b"license")}
            for name in names:
                (root / name).write_bytes(name.encode())
            (root / "LICENSE").write_bytes(b"license")
            (root / "manifest.json").write_bytes(encoded({"schema": 2, "base": files, "files": files}))
            self.assertEqual(set(release.graphics_files(encoded(lock), root)), {*names, "manifest.json", "LICENSE"})
            (root / "dxgi.dll").write_bytes(b"changed")
            with self.assertRaises(ValueError):
                release.graphics_files(encoded(lock), root)
            (root / "dxgi.dll").write_bytes(b"dxgi.dll")
            (root / "extra").write_bytes(b"unreviewed")
            with self.assertRaises(ValueError):
                release.graphics_files(encoded(lock), root)


if __name__ == "__main__":
    unittest.main()
