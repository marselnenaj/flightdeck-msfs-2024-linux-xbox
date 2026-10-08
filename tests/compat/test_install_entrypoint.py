# SPDX-License-Identifier: MIT
"""Exercise pre-Rust installer errors without touching an installed profile."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]


class InstallEntrypoint(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="flightdeck-entrypoint-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.package = self.root / "package with spaces $(touch unintended)"
        (self.package / "bin").mkdir(parents=True)
        self.script = self.package / "install.sh"
        shutil.copyfile(ROOT / "install.sh", self.script)
        self.commands = self.root / "commands"
        self.commands.mkdir()
        for command in ("dirname", "timeout"):
            (self.commands / command).symlink_to(shutil.which(command))
        self.write_command("uname", """case "$1" in
  -s) printf '%s\\n' "${TEST_SYSTEM-Linux}" ;;
  -m) printf '%s\\n' "${TEST_ARCH-x86_64}" ;;
esac
""")
        self.binary = self.package / "bin/flightdeck"
        self.binary.write_text("""#!/bin/bash
printf '%s\\0' "$@" >> "$TEST_CALLS"
printf '\\0' >> "$TEST_CALLS"
if [[ "$1" == --version ]]; then
  case "${TEST_FAILURE:-}" in
    hang) exec /bin/sleep 30 ;;
    huge) for ((i=0; i<10000; i++)); do printf 'bounded diagnostic text '; done; exit 1 ;;
    *) if [[ -n ${TEST_FAILURE:-} ]]; then printf '%s\\n' "$TEST_FAILURE" >&2; exit 127; fi ;;
  esac
  printf '%s\\n' 'flightdeck 0.2.7'
fi
""")
        self.binary.chmod(0o700)
        self.calls_path = self.root / "calls"
        self.dialog_path = self.root / "dialog"
        self.environment = {
            "PATH": str(self.commands), "HOME": str(self.root),
            "LANG": "C", "LC_ALL": "C", "TEST_CALLS": str(self.calls_path),
            "TEST_DIALOG": str(self.dialog_path),
        }

    def write_command(self, name, body):
        path = self.commands / name
        path.write_text("#!/bin/bash\n" + body)
        path.chmod(0o700)

    def run_install(self, *args, **environment):
        return subprocess.run(["/bin/bash", str(self.script), *args],
                              env={**self.environment, **environment},
                              cwd=self.root, capture_output=True, text=True, timeout=12)

    def calls(self):
        if not self.calls_path.exists():
            return []
        return [part.decode().split("\0") for part in self.calls_path.read_bytes().split(b"\0\0") if part]

    def assert_not_installed(self):
        self.assertTrue(all(call == ["--version"] for call in self.calls()), self.calls())

    def test_probe_then_install_preserves_literal_arguments_without_ldd_or_getconf(self):
        args = ["--language", "de", "--language=en", "--gui", "--data-home", "literal $(touch unexpected); *"]
        result = self.run_install(*args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [["--version"], ["install", "--source", str(self.package), *args]])
        self.assertFalse((self.root / "unexpected").exists())
        self.assertFalse((self.root / "unintended").exists())
        self.assertEqual(result.stdout, "")

    def test_missing_optional_probe_and_platform_tools_do_not_block_installation(self):
        (self.commands / "timeout").unlink()
        (self.commands / "uname").unlink()
        result = self.run_install("--language=de")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [["install", "--source", str(self.package), "--language=de"]])

    def test_unknown_platform_information_does_not_block_a_working_binary(self):
        result = self.run_install(TEST_SYSTEM="", TEST_ARCH="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls()), 2)

    def test_clear_unsupported_platform_is_reported_before_running_binary(self):
        for environment in ({"TEST_ARCH": "aarch64"}, {"TEST_SYSTEM": "Darwin"}):
            with self.subTest(environment=environment):
                result = self.run_install(**environment)
                self.assertEqual(result.returncode, 1)
                self.assertIn("requires Linux on x86-64", result.stderr)
                self.assertEqual(self.calls(), [])

    def test_loader_failures_are_classified_and_never_install(self):
        examples = [
            ("/lib/libc.so.6: version `GLIBC_2.39' not found", "needs glibc 2.39 or newer"),
            ("error while loading shared libraries: liblzma.so.5: cannot open shared object file: No such file or directory", "runtime library"),
            ("timeout: failed to run command: No such file or directory", "program loader"),
            ("cannot execute: required file not found", "program loader"),
            ("Permission denied", "cannot start here"),
        ]
        for detail, expected in examples:
            with self.subTest(detail=detail):
                result = self.run_install(TEST_FAILURE=detail)
                self.assertEqual(result.returncode, 1)
                self.assertIn(expected, result.stderr)
                self.assertIn(detail, result.stderr)
                self.assert_not_installed()

    def test_explicit_language_and_german_locale_select_translated_errors(self):
        result = self.run_install("--language=de", TEST_FAILURE="GLIBC_2.39 not found")
        self.assertIn("glibc-Version dieses Systems", result.stderr)
        result = self.run_install(TEST_ARCH="aarch64", LC_ALL="", LANG="de_AT.UTF-8")
        self.assertIn("benötigt Linux auf x86-64", result.stderr)
        result = self.run_install("--language", "en", TEST_ARCH="aarch64", LC_ALL="", LANG="de_AT.UTF-8")
        self.assertIn("requires Linux on x86-64", result.stderr)
        self.assert_not_installed()

    def test_invalid_language_does_not_run_the_binary(self):
        for args in (("--language", "xx"), ("--language",)):
            result = self.run_install(*args)
            self.assertEqual(result.returncode, 2)
            self.assertIn("Choose --language", result.stderr)
            self.assertEqual(self.calls(), [])

    def test_missing_or_nonexecutable_package_is_reported(self):
        self.binary.chmod(0o600)
        result = self.run_install()
        self.assertEqual(result.returncode, 1)
        self.assertIn("not executable", result.stderr)
        self.assertEqual(self.calls(), [])
        self.binary.unlink()
        result = self.run_install("--language=de")
        self.assertIn("fehlt oder ist nicht ausführbar", result.stderr)

    def test_real_hung_probe_is_bounded(self):
        started = time.monotonic()
        result = self.run_install(TEST_FAILURE="hang")
        elapsed = time.monotonic() - started
        self.assertEqual(result.returncode, 1)
        self.assertIn("did not finish in time", result.stderr)
        self.assertGreaterEqual(elapsed, 4.5)
        self.assertLess(elapsed, 9)
        self.assert_not_installed()

    def test_diagnostic_output_is_bounded(self):
        result = self.run_install(TEST_FAILURE="huge")
        self.assertEqual(result.returncode, 1)
        self.assertLess(len(result.stderr.encode()), 4600)
        self.assertIn("bounded diagnostic text", result.stderr)
        self.assert_not_installed()

    def test_gui_uses_literal_bounded_zenity_arguments_only_when_requested(self):
        self.write_command("zenity", "printf '%s\\0' \"$@\" > \"$TEST_DIALOG\"\n")
        self.write_command("kdialog", "exit 99\n")
        detail = 'Permission denied <b>& $(touch unexpected)'
        result = self.run_install("--gui", "--language=de", TEST_FAILURE=detail, DISPLAY=":fixture")
        self.assertEqual(result.returncode, 1)
        args = self.dialog_path.read_bytes().decode().rstrip("\0").split("\0")
        self.assertEqual(args[:5], ["--error", "--no-markup", "--title", "Flightdeck", "--text"])
        self.assertEqual(len(args), 6)
        self.assertIn("kann hier nicht starten", args[5])
        self.assertIn(detail, args[5])
        self.assertFalse((self.root / "unexpected").exists())
        self.dialog_path.unlink()
        self.run_install(TEST_FAILURE=detail, DISPLAY=":fixture")
        self.assertFalse(self.dialog_path.exists())
        self.run_install("--gui", TEST_FAILURE=detail)
        self.assertFalse(self.dialog_path.exists())

    def test_kdialog_fallback_escapes_diagnostics_markup(self):
        self.write_command("kdialog", "printf '%s\\0' \"$@\" > \"$TEST_DIALOG\"\n")
        result = self.run_install("--gui", TEST_FAILURE="Permission denied <b>&", WAYLAND_DISPLAY="fixture")
        self.assertEqual(result.returncode, 1)
        args = self.dialog_path.read_bytes().decode().rstrip("\0").split("\0")
        self.assertEqual(args[:3], ["--title", "Flightdeck", "--error"])
        self.assertEqual(len(args), 4)
        self.assertIn("&lt;b&gt;&amp;", args[3])
        self.assertNotIn("<b>", args[3])


if __name__ == "__main__":
    unittest.main()
