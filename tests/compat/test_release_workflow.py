# SPDX-License-Identifier: MIT
"""Release gating, archive bounds and fail-closed publication without network."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_workflow", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
COMMIT = "a" * 40


def result(stdout="", returncode=0, stderr=""):
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


class ReleaseWorkflow(unittest.TestCase):
    def test_gate_requires_push_main_exact_commit_subject_and_an_absent_tag(self):
        environment = {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/main",
                       "GITHUB_REPOSITORY": release.REPOSITORY, "GITHUB_SHA": COMMIT}
        with patch.dict(os.environ, environment), patch.object(release, "version", return_value="0.2.6"):
            for subject, head, refs, accepted in [
                ("Release Flightdeck 0.2.6", COMMIT, "", True),
                ("Release Flightdeck 0.2.6 extra", COMMIT, "", False),
                ("Release Flightdeck 0.2.5", COMMIT, "", False),
                ("Release Flightdeck 0.2.6", "b" * 40, "", False),
                ("Release Flightdeck 0.2.6", COMMIT, f"{COMMIT}\trefs/tags/v0.2.6", False),
            ]:
                with self.subTest(subject=subject, head=head, refs=refs), \
                        patch.object(release, "run", side_effect=[result(head), result(subject)]), \
                        patch.object(release, "tag_refs", return_value=refs) as tags:
                    self.assertEqual(release.eligible(), accepted)
                    if head == COMMIT and subject == "Release Flightdeck 0.2.6":
                        tags.assert_called_once_with("v0.2.6")
                    else:
                        tags.assert_not_called()
            for key, value in [("GITHUB_EVENT_NAME", "pull_request"),
                               ("GITHUB_REF", "refs/heads/another"),
                               ("GITHUB_REPOSITORY", "another/repository")]:
                with self.subTest(key=key), patch.dict(os.environ, {key: value}), \
                        patch.object(release, "run") as command:
                    self.assertFalse(release.eligible())
                    command.assert_not_called()

    def test_archive_rejects_links_and_traversal_before_writing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name, kind in [("flightdeck-linux/../../escape", tarfile.REGTYPE),
                               ("flightdeck-linux/link", tarfile.SYMTYPE)]:
                archive = root / "bad.tar.gz"
                with tarfile.open(archive, "w:gz") as stream:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = "/tmp/escape"
                    stream.addfile(member, io.BytesIO(b""))
                with self.assertRaises(ValueError):
                    release.unpack(archive, root / "extracted")
                self.assertFalse((root / "extracted").exists())

    def staged(self, root):
        assets, cache = root / "assets", root / "cache"
        assets.mkdir()
        cache.mkdir()
        for name in release.asset_names() - set(release.SOURCES):
            (assets / name).write_text("synthetic " + name)
        binary = b"synthetic executable"
        with tarfile.open(assets / release.PREVIOUS, "w:gz") as archive:
            for name, data in [("bin/flightdeck", binary), ("FLIGHTDECK-PACKAGE.json", b"{}")]:
                member = tarfile.TarInfo("flightdeck-linux/" + name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        for name in release.SOURCES:
            (cache / name).write_bytes(b"synthetic source")
        (assets / "SHA256SUMS").write_text("")
        check = root / "check.json"
        check.write_text(json.dumps({"status": "PASS", "native_binary_sha256": hashlib.sha256(binary).hexdigest(),
                                     "checks": {"native_update_and_rollback": "PASS"}}))
        release.stage(argparse.Namespace(assets=assets, cache=cache, check=check, commit=COMMIT))
        return assets

    def test_stage_binds_exact_assets_commit_and_sources_and_rejects_tampering(self):
        sources = {"source.tar.gz": hashlib.sha256(b"synthetic source").hexdigest()}
        with tempfile.TemporaryDirectory() as temporary, patch.object(release, "SOURCES", sources):
            assets = self.staged(Path(temporary))
            self.assertIn("SHA256SUMS", release.verify(assets, COMMIT))
            with self.assertRaisesRegex(ValueError, "version and commit"):
                release.verify(assets, "b" * 40)
            (assets / release.PREVIOUS).write_text("changed")
            with self.assertRaisesRegex(ValueError, "changed after validation"):
                release.verify(assets, COMMIT)

    def test_bad_uploaded_bytes_leave_draft_unpublished(self):
        sources = {"source.tar.gz": hashlib.sha256(b"synthetic source").hexdigest()}
        with tempfile.TemporaryDirectory() as temporary, patch.object(release, "SOURCES", sources):
            root = Path(temporary)
            assets = self.staged(root)
            calls = []

            def github(*args, **kwargs):
                calls.append(args)
                if args[0] == "view" and kwargs.get("check") is False:
                    return result(returncode=1, stderr="release not found")
                if args[0] == "view":
                    return result(json.dumps({"tagName": "v" + release.version(), "targetCommitish": COMMIT,
                        "isDraft": True, "isPrerelease": False,
                        "assets": [{"name": p.name, "size": p.stat().st_size} for p in assets.iterdir()]}))
                if args[0] == "download":
                    for source in assets.iterdir():
                        shutil.copyfile(source, root / "readback" / source.name)
                    (root / "readback" / release.PREVIOUS).write_text("corrupted upload")
                return result()

            with patch.dict(os.environ, GITHUB_SHA=COMMIT), patch.object(release, "eligible", return_value=True), \
                    patch.object(release, "run", return_value=result()), patch.object(release, "tag_refs", return_value=""), \
                    patch.object(release, "gh", side_effect=github):
                with self.assertRaisesRegex(ValueError, "changed after validation"):
                    release.publish(argparse.Namespace(assets=assets, notes=root / "notes.md", readback=root / "readback"))
            self.assertTrue(any(call[0] == "create" and "--draft" in call for call in calls))
            self.assertFalse(any(call[0] == "edit" for call in calls))

    def test_changed_cleanup_tip_is_never_deleted(self):
        with patch.object(release, "version", return_value="0.2.6"), \
                patch.object(release, "run", return_value=result("changed\trefs/heads/fix/security-api-auth-rustls-20261006")) as command, \
                patch.object(release.subprocess, "run") as process:
            with self.assertRaisesRegex(ValueError, "changed"):
                release.cleanup_merged_branch(COMMIT)
            self.assertEqual(command.call_count, 1)
            process.assert_not_called()

    def test_cleanup_deletion_uses_an_exact_one_process_lease(self):
        branch = "refs/heads/fix/security-api-auth-rustls-20261006"
        expected = "6606305dd893176bd3218d00d149c572f8be3bd8"
        with patch.object(release, "version", return_value="0.2.6"), \
                patch.dict(os.environ, GH_TOKEN="synthetic-token"), \
                patch.object(release, "run", side_effect=[result(f"{expected}\t{branch}"), result(json.dumps({"status": "ahead", "base": expected})), result()]), \
                patch.object(release.subprocess, "run", return_value=result()) as process:
            release.cleanup_merged_branch(COMMIT)
            command = process.call_args.args[0]
            self.assertIn(f"--force-with-lease={branch}:{expected}", command)
            self.assertEqual(command[-1], ":" + branch)
            self.assertNotIn("synthetic-token", " ".join(command))
            self.assertNotIn("GH_TOKEN", process.call_args.kwargs["env"])
            self.assertEqual(process.call_args.kwargs["env"]["GIT_CONFIG_COUNT"], "1")
