# SPDX-License-Identifier: MIT
"""Compile the original pure protocol validator; no Wine, account or network."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(shutil.which("g++"), "C++ compiler unavailable")
class ConnectedStorageProtocol(unittest.TestCase):
    def test_fixed_read_requests_and_unique_title_binding(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / "protocol-test"
            subprocess.run(["g++", "-std=c++17", "-O1",
                            "-I", str(ROOT / "compat/runtime/include"),
                            "-I", str(ROOT / "compat/runtime/tools"),
                            str(ROOT / "tests/compat/cloud_storage/protocol-test.cpp"),
                            "-o", str(output)], check=True, capture_output=True, timeout=120)
            result = subprocess.run([str(output)], check=True, capture_output=True, timeout=10)
            self.assertEqual(result.stdout, b"33 protocol checks PASS\n")

    def test_write_lease_fencing_and_private_azure_targets(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / "write-protocol-test"
            subprocess.run(["g++", "-std=c++17", "-O1",
                            "-I", str(ROOT / "compat/runtime/include"),
                            "-I", str(ROOT / "compat/runtime/tools"),
                            str(ROOT / "tests/compat/cloud_storage/write-protocol-test.cpp"),
                            "-o", str(output)], check=True, capture_output=True, timeout=120)
            result = subprocess.run([str(output)], check=True, capture_output=True, timeout=10)
            self.assertRegex(result.stdout, rb"^[0-9]+ write protocol checks PASS\n$")


if __name__ == "__main__":
    unittest.main()
