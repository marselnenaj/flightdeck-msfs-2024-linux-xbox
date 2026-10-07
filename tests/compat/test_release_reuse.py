# SPDX-License-Identifier: MIT
"""A one-time artifact retry must never accept changed production sources."""
import importlib.util
import hashlib
import io
import json
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_reuse", ROOT / "scripts/release-reuse.py")
reuse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reuse)


def result(stdout="", code=0):
    return subprocess.CompletedProcess([], code, stdout, "")


class ReleaseReuse(unittest.TestCase):
    def test_pinned_archive_requires_matching_package_manifest_and_zip(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            files = {name: b"synthetic " + name.encode() for name in (
                "install.sh", "Install Flightdeck.desktop", "compat/bootstrap.lock.json", "compat/graphics.lock.json")}
            for name, data in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            files["bin/flightdeck"] = b"synthetic executable"
            files["FLIGHTDECK-PACKAGE.json"] = json.dumps({"kind": "rust-launcher", "version": "0.2.6",
                "files": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}).encode()
            tar_bytes, zip_bytes = io.BytesIO(), io.BytesIO()
            with tarfile.open(fileobj=tar_bytes, mode="w:gz") as archive:
                for name, data in files.items():
                    item = tarfile.TarInfo("flightdeck-linux/" + name)
                    item.size = len(data)
                    archive.addfile(item, io.BytesIO(data))
            with zipfile.ZipFile(zip_bytes, "w") as archive:
                for name, data in files.items():
                    archive.writestr("flightdeck-linux/" + name, data)
            assets = {reuse.release.PREVIOUS: tar_bytes.getvalue(), "Flightdeck-Linux-x86_64.zip": zip_bytes.getvalue(),
                      "THIRD-PARTY-NOTICES.txt": b"notices", "launcher-dependencies.json": b"[]"}
            assets["SHA256SUMS"] = "".join(hashlib.sha256(data).hexdigest() + "  " + name + "\n"
                                           for name, data in assets.items()).encode()
            artifact = root / "artifact.zip"
            with zipfile.ZipFile(artifact, "w") as archive:
                for name, data in assets.items():
                    archive.writestr("release-assets/" + name, data)
            with patch.object(reuse, "ROOT", root), patch.object(reuse, "SIZE", artifact.stat().st_size), \
                    patch.object(reuse, "DIGEST", hashlib.sha256(artifact.read_bytes()).hexdigest()), \
                    patch.object(reuse.release, "version", return_value="0.2.6"):
                reuse.extract(artifact, root / "accepted")
                self.assertEqual((root / "accepted" / reuse.release.PREVIOUS).read_bytes(), tar_bytes.getvalue())
                artifact.write_bytes(artifact.read_bytes() + b"changed")
                with self.assertRaisesRegex(ValueError, "pinned SHA256"):
                    reuse.extract(artifact, root / "rejected")

    def test_only_the_exact_ancestor_and_test_tooling_delta_can_reuse(self):
        with patch.object(reuse.release, "version", return_value="0.2.6"):
            for changed, allowed in [("scripts/check-native-package.py\n", True),
                                     ("native/server.rs\n", False),
                                     ("Cargo.lock\n", False), ("", False)]:
                with self.subTest(changed=changed), patch.object(reuse.release, "run", side_effect=[result(), result(changed)]):
                    self.assertEqual(reuse.reusable(), allowed)
            with patch.object(reuse.release, "run", return_value=result(code=1)):
                self.assertFalse(reuse.reusable())
        with patch.object(reuse.release, "version", return_value="0.2.7"):
            self.assertFalse(reuse.reusable())

    def test_changed_artifact_identity_and_failed_build_checks_are_rejected(self):
        metadata = {"id": reuse.ARTIFACT, "name": "release-evidence-" + reuse.SOURCE,
                    "size_in_bytes": reuse.SIZE, "digest": "sha256:" + reuse.DIGEST, "expired": False,
                    "workflow_run": {"id": reuse.RUN, "repository_id": 1374954725, "head_repository_id": 1374954725,
                                     "head_branch": "main", "head_sha": reuse.SOURCE}}
        steps = [{"name": name, "conclusion": "success"} for name in [
            "Format, lint and test the locked workspace", "Check native HTTP contract and render both UI languages",
            "Fetch and validate pinned compatibility, graphics and corresponding sources",
            "Build and assemble the complete native package"]]
        jobs = {"jobs": [{"id": 113052882089, "name": "build", "steps": steps}]}
        reuse.validate_metadata(metadata, jobs)
        with self.assertRaises(ValueError):
            reuse.validate_metadata({**metadata, "digest": "sha256:" + "0" * 64}, jobs)
        with self.assertRaises(ValueError):
            reuse.validate_metadata({**metadata, "workflow_run": {**metadata["workflow_run"], "head_sha": "0" * 40}}, jobs)
        steps[-1]["conclusion"] = "failure"
        with self.assertRaises(ValueError):
            reuse.validate_metadata(metadata, jobs)
