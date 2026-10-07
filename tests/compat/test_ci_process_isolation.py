# SPDX-License-Identifier: MIT
"""The CI test boundary preserves process-safety coverage without root tests."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("native_tests", ROOT / "scripts/check-native-tests.py")
native_tests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native_tests)


class ProcessIsolation(unittest.TestCase):
    def test_original_user_private_proc_no_capabilities_or_credentials_and_full_suite(self):
        environment = {"HOME": "/fixture/home", "PATH": "/tools:/usr/bin",
                       "ICED_TEST_BACKEND": "tiny-skia", "GH_TOKEN": "must-not-leak",
                       "GITHUB_TOKEN": "must-not-leak", "CARGO_HOME": "", "RUSTUP_HOME": ""}
        command = native_tests.command(environment, 1001, 1002, "/tools/cargo")
        for argument in ("--pid", "--mount-proc", "--propagation", "private", "--reuid=1001",
                         "--regid=1002", "--bounding-set=-all", "--inh-caps=-all",
                         "--ambient-caps=-all", "--no-new-privs", "--kill-child=SIGKILL"):
            self.assertIn(argument, command)
        self.assertNotIn("--user", command)
        self.assertEqual(command[-4:], ["/tools/cargo", "test", "--locked", "--workspace"])
        self.assertIn("CARGO_HOME=/fixture/home/.cargo", command)
        self.assertIn("RUSTUP_HOME=/fixture/home/.rustup", command)
        self.assertIn("ICED_TEST_BACKEND=tiny-skia", command)
        self.assertNotIn("must-not-leak", " ".join(command))
        self.assertLess(command.index("/usr/bin/setpriv"), command.index("/tools/cargo"))
        self.assertEqual(command[command.index("/usr/bin/env") + 1], "-i")

    def test_root_or_missing_toolchain_is_rejected_before_namespace_setup(self):
        with self.assertRaises(ValueError):
            native_tests.command({}, 0, 0, "/tools/cargo")
        with self.assertRaises(ValueError):
            native_tests.command({}, 1001, 1001, None)
