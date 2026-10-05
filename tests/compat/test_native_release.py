# SPDX-License-Identifier: MIT
"""The GUI dependency graph must not raise manifest bounds or the host ABI."""
import importlib.util
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("native_release", ROOT / "scripts/native-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class NativeRelease(unittest.TestCase):
    def test_larger_cargo_graph_does_not_relax_the_component_manifest_limit(self):
        data = b'{"packages":[],"padding":"' + b'x' * (1024 * 1024) + b'"}'
        with self.assertRaisesRegex(ValueError, "size limit"):
            release.components.read_json(data, "manifest")
        self.assertEqual(release.components.read_json(data, "Cargo graph", maximum=32 * 1024 * 1024)["packages"], [])
        with self.assertRaisesRegex(ValueError, "invalid JSON"):
            release.components.read_json(b'{"packages":[],"packages":[]}', "Cargo graph", maximum=32 * 1024 * 1024)

    def test_new_glibc_math_imports_cannot_silently_raise_the_release_requirement(self):
        for symbols, accepted in [
            (b'UND memcpy@GLIBC_2.14\nUND posix_spawn@GLIBC_2.39', True),
            (b'UND memcpy@GLIBC_2.14\nUND atan2f@GLIBC_2.43', False),
            (b'', False),
        ]:
            with self.subTest(symbols=symbols), patch.object(release.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, stdout=symbols)):
                if accepted:
                    release.validate_abi(Path("synthetic-binary"))
                else:
                    with self.assertRaisesRegex(ValueError, "glibc 2.39"):
                        release.validate_abi(Path("synthetic-binary"))
