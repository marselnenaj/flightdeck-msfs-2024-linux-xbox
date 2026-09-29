# SPDX-License-Identifier: MIT
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import graphics, renderer, setup


class RendererTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        self.private = self.runtime / "private"
        self.private.mkdir(parents=True)
        self.runner = self.runtime / "runner/files/lib/wine/vkd3d-proton/x86_64-windows"
        self.runner.mkdir(parents=True)
        self.system = self.runtime / "local/msfs-prefix/drive_c/windows/system32"
        self.system.mkdir(parents=True)
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        self.base, self.fixed = {}, {}
        for name in renderer.FILES:
            base, fixed = b"synthetic runner " + name.encode(), b"synthetic backport " + name.encode()
            (self.runner / name).write_bytes(base)
            (self.system / name).write_bytes(base)
            (self.bundle / name).write_bytes(fixed)
            self.base[name] = hashlib.sha256(base).hexdigest()
            self.fixed[name] = hashlib.sha256(fixed).hexdigest()
        (self.bundle / "manifest.json").write_text(json.dumps({"schema": 1, "base": self.base, "files": self.fixed}))

    def install(self):
        return renderer.install(self.runtime, bundle=self.bundle)

    def test_installs_pair_and_leaves_runner_unchanged(self):
        self.assertEqual(self.install(), "backport")
        for name in renderer.FILES:
            self.assertEqual((self.system / name).read_bytes(), (self.bundle / name).read_bytes())
            self.assertEqual(graphics._digest(self.runner / name), self.base[name])
        with patch.object(setup, "_copy_file") as copy:
            self.assertEqual(self.install(), "backport")
            copy.assert_not_called()

    def test_invalid_second_binary_changes_neither_destination(self):
        (self.bundle / "d3d12core.dll").write_bytes(b"truncated")
        with self.assertRaises(graphics.GraphicsError):
            self.install()
        for name in renderer.FILES:
            self.assertEqual(graphics._digest(self.system / name), self.base[name])

    def test_custom_renderer_preserves_entire_pair(self):
        (self.system / "d3d12core.dll").write_bytes(b"custom renderer")
        with patch.object(setup, "_copy_file") as copy:
            self.assertEqual(self.install(), "custom")
            copy.assert_not_called()
        self.assertFalse((self.private / renderer.MARKER).exists())

    def test_runner_upgrade_restores_matched_pair(self):
        self.install()
        for name in renderer.FILES:
            (self.runner / name).write_bytes(b"new runner " + name.encode())
        self.assertEqual(self.install(), "runner")
        for name in renderer.FILES:
            self.assertEqual((self.system / name).read_bytes(), (self.runner / name).read_bytes())
        self.assertFalse((self.private / renderer.MARKER).exists())

    def test_source_only_launcher_restores_owned_files(self):
        self.install()
        self.assertEqual(renderer.install(self.runtime, bundle=self.root / "absent"), "runner")
        for name in renderer.FILES:
            self.assertEqual(graphics._digest(self.system / name), self.base[name])

    def test_failed_second_copy_blocks_launch_and_next_attempt_recovers(self):
        copy = setup._copy_file
        def interrupted(source, destination):
            if destination.name == "d3d12core.dll":
                raise OSError("synthetic interrupted copy")
            copy(source, destination)
        with patch.object(setup, "_copy_file", side_effect=interrupted):
            with self.assertRaises(graphics.GraphicsError):
                self.install()
        self.assertFalse((self.private / renderer.MARKER).exists())
        self.assertEqual(self.install(), "backport")
        for name in renderer.FILES:
            self.assertEqual(graphics._digest(self.system / name), self.fixed[name])

    def test_marker_and_bundle_symlinks_are_rejected(self):
        external = self.root / "outside.json"
        external.write_text(json.dumps(self.fixed))
        marker = self.private / renderer.MARKER
        marker.symlink_to(external)
        with self.assertRaises(graphics.GraphicsError):
            self.install()
        marker.unlink()
        (self.bundle / "d3d12.dll").unlink()
        (self.bundle / "d3d12.dll").symlink_to(self.runner / "d3d12.dll")
        with self.assertRaises(graphics.GraphicsError):
            self.install()

    def test_source_install_without_bundle_does_not_touch_prefix(self):
        with patch.object(setup, "_copy_file") as copy:
            self.assertEqual(renderer.install(self.runtime, bundle=self.root / "absent"), "runner")
            copy.assert_not_called()
