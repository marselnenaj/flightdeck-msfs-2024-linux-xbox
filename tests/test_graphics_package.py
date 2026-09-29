# SPDX-License-Identifier: MIT
import importlib.util
import json
from pathlib import Path
import shutil
import unittest

from tests import test_installer, test_renderer

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("graphics_release_test", ROOT / "scripts/full-installer-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class GraphicsPackageTests(unittest.TestCase):
    runner_file = test_renderer.RendererTests.runner_file
    def setUp(self):
        test_renderer.RendererTests.setUp(self)
        self.source = self.root / "source"
        (self.source / "compat").mkdir(parents=True)
        self.packaged = self.source / "flightdeck/resources/graphics"
        shutil.copytree(self.bundle, self.packaged)
        license = b"Synthetic license fixture\n"
        (self.packaged / "LICENSE").write_bytes(license)
        self.lock = {"schema": 2, "base": self.base, "files": self.fixed,
                     "license_sha256": release.digest(license)}
        self.specification = (json.dumps(self.lock) + "\n").encode()
        (self.source / "compat/graphics.lock.json").write_bytes(self.specification)
        self.sources = {release.SOURCE_ROOT + "compat/graphics.lock.json": self.specification}

    def test_builder_and_installer_preserve_exact_verified_files(self):
        files = release.graphics_files(self.sources, self.packaged)
        installed = test_installer.installer.graphics_snapshot(self.source)
        self.assertEqual(files, {release.SOURCE_ROOT + name: value for name, value in installed.items()})
        self.assertEqual(len(files), 7)
        self.assertEqual(installed["flightdeck/resources/graphics/d3d12core.dll"],
                         (self.bundle / "d3d12core.dll").read_bytes())

    def test_corrupt_or_unrelated_graphics_payload_is_rejected(self):
        for name in ("d3d12core.dll", "LICENSE", "unrelated.dll"):
            with self.subTest(name=name):
                path = self.packaged / name
                before = path.read_bytes() if path.exists() else None
                path.write_bytes(b"changed")
                with self.assertRaises(ValueError):
                    release.graphics_files(self.sources, self.packaged)
                with self.assertRaises(test_installer.installer.InstallError):
                    test_installer.installer.graphics_snapshot(self.source)
                if before is None:
                    path.unlink()
                else:
                    path.write_bytes(before)

    def test_manifest_cannot_substitute_an_unpinned_renderer(self):
        manifest = self.packaged / "manifest.json"
        value = json.loads(manifest.read_text())
        value["files"]["d3d12core.dll"] = "0" * 64
        manifest.write_text(json.dumps(value))
        with self.assertRaises(ValueError):
            release.graphics_files(self.sources, self.packaged)
        with self.assertRaises(test_installer.installer.InstallError):
            test_installer.installer.graphics_snapshot(self.source)

    def test_source_only_distribution_is_supported(self):
        shutil.rmtree(self.packaged)
        self.assertEqual(release.graphics_files(self.sources, None), {})
        self.assertEqual(test_installer.installer.graphics_snapshot(self.source), {})
