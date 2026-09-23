# SPDX-License-Identifier: MIT
"""Source installer tests; all installs use private temporary directories."""
import fcntl
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import subprocess
from string import Formatter
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import urlsplit
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("flightdeck_installer", ROOT / "scripts/install-launcher.py")
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.language_patch = patch.object(installer, "LANGUAGE", "de")
        self.language_patch.start()
        self.temp = tempfile.TemporaryDirectory(prefix="flightdeck-installer-")
        self.base = Path(self.temp.name)
        self.source = self.base / "source"
        self.source.mkdir()
        for name in ("flightdeck", "ui", "scripts/runtime"):
            shutil.copytree(ROOT / name, self.source / name,
                            ignore=shutil.ignore_patterns("__pycache__", "tests", "*.pyc"))
        (self.source / "scripts/install-launcher.py").write_bytes((ROOT / "scripts/install-launcher.py").read_bytes())
        (self.source / "scripts/install-launcher-gui.py").write_bytes((ROOT / "scripts/install-launcher-gui.py").read_bytes())
        (self.source / "compat").mkdir()
        shutil.copytree(ROOT / "compat/fenix", self.source / "compat/fenix")
        shutil.copyfile(ROOT / "compat/upstreams.lock.json", self.source / "compat/upstreams.lock.json")
        shutil.copyfile(ROOT / "compat/bootstrap.lock.json", self.source / "compat/bootstrap.lock.json")
        shutil.copyfile(ROOT / "LICENSE", self.source / "LICENSE")
        self.data = self.base / "xdg data" / "flightdeck-launcher"
        self.bin = self.base / "user bin"
        self.applications = self.base / "xdg data" / "applications"
        self.state = self.base / "xdg state" / "flightdeck"
        self.state.mkdir(parents=True)
        (self.state / "config.json").write_text('{"synthetic_setting":true}')
        self.saves = self.base / "runtime/private/local-saves"
        self.saves.mkdir(parents=True)
        (self.saves / "state.bin").write_bytes(b"synthetic save\x00data")

    def tearDown(self):
        self.temp.cleanup()
        self.language_patch.stop()

    def install(self, **kwargs):
        return installer.install(self.source, self.data, self.bin, self.applications, **kwargs)

    def update_source(self):
        path = self.source / "ui/index.html"
        path.write_text(path.read_text() + "\n<!-- updated synthetic fixture -->\n")

    def test_reproducible_install_and_setup_resources(self):
        first = self.install()
        repeated = self.install()
        self.assertEqual(first["current"], repeated["current"])
        self.assertIsNone(repeated["previous"])
        folder = installer.verify_release(self.data, first["current"])
        self.assertEqual((folder / "flightdeck/resources/runtime/launch-msfs.sh").read_bytes(),
                         (self.source / "scripts/runtime/launch-msfs.sh").read_bytes())
        self.assertEqual((folder / "flightdeck/resources/upstreams.lock.json").read_bytes(),
                         (self.source / "compat/upstreams.lock.json").read_bytes())
        self.assertEqual((folder / "flightdeck/resources/bootstrap.lock.json").read_bytes(),
                         (self.source / "compat/bootstrap.lock.json").read_bytes())
        for name in ("flightdeck/setup.py", "ui/setup.js", "ui/flight-panorama.png", "ui/flight-panorama-2020.png", "ui/manrope-variable.woff2", "ui/OFL-Manrope.txt"):
            self.assertEqual((folder / name).read_bytes(), (self.source / name).read_bytes())
        self.assertFalse((folder / "compat").exists())
        self.assertFalse((folder / "tests").exists())
        self.assertTrue(os.access(self.bin / "flightdeck", os.X_OK))
        self.assertTrue((self.applications / "flightdeck.desktop").is_file())
        second_data = self.base / "other-data"
        other = installer.install(self.source, second_data, self.base / "other-bin", self.base / "other-apps")
        self.assertEqual(first["current"], other["current"])

    def test_only_exact_reviewed_font_and_license_are_installed(self):
        (self.source / "ui/unreviewed.woff2").write_bytes(b"wOF2unreviewed")
        (self.source / "ui/unreviewed.txt").write_text("not an installation input")
        record = self.install()
        folder = installer.verify_release(self.data, record["current"])
        for name, checksum in installer.PUBLIC_FONT.items():
            self.assertEqual(installer.digest((folder / name).read_bytes()), checksum)
        self.assertFalse((folder / "ui/unreviewed.woff2").exists())
        self.assertFalse((folder / "ui/unreviewed.txt").exists())
        for name in installer.PUBLIC_FONT:
            path = self.source / name
            original = path.read_bytes()
            path.write_bytes(original + b"changed")
            with self.assertRaises(installer.InstallError): self.install()
            path.write_bytes(original)
        (self.source / "ui/OFL-Manrope.txt").unlink()
        with self.assertRaises(installer.InstallError): self.install()

    def test_update_and_rollback_preserve_old_release(self):
        first = self.install()
        self.update_source()
        second = self.install()
        self.assertNotEqual(first["current"], second["current"])
        self.assertEqual(second["previous"], first["current"])
        installer.verify_release(self.data, first["current"])
        result = installer.rollback(self.data)
        self.assertEqual(result["current"], first["current"])
        self.assertEqual(result["previous"], second["current"])
        self.assertIn(first["current"], (self.applications / "flightdeck.desktop").read_text())
        self.assertEqual(installer.rollback(self.data)["current"], second["current"])

    def test_update_removes_only_generated_python_bytecode(self):
        first = self.install()
        old = self.data / "releases" / first["current"]
        cache = old / "flightdeck/__pycache__"
        cache.mkdir()
        (cache / "__init__.cpython-314.pyc").write_bytes(b"generated cache")
        self.update_source()
        second = self.install()
        self.assertNotEqual(second["current"], first["current"])
        self.assertFalse(cache.exists())
        installer.verify_release(self.data, first["current"])

    def test_update_preserves_foreign_file_inside_python_cache(self):
        first = self.install()
        cache = self.data / "releases" / first["current"] / "flightdeck/__pycache__"
        cache.mkdir()
        foreign = cache / "personal.txt"
        foreign.write_text("leave this alone")
        self.update_source()
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertEqual(foreign.read_text(), "leave this alone")

    def test_uninstall_preserves_settings_saves_and_other_files(self):
        self.install()
        sibling = self.bin / "unrelated-tool"
        sibling.write_text("keep")
        self.assertEqual(installer.uninstall(self.data), [])
        self.assertFalse((self.bin / "flightdeck").exists())
        self.assertFalse((self.applications / "flightdeck.desktop").exists())
        self.assertEqual(sibling.read_text(), "keep")
        self.assertTrue((self.state / "config.json").exists())
        self.assertEqual((self.saves / "state.bin").read_bytes(), b"synthetic save\x00data")
        self.assertEqual(sorted(p.name for p in self.data.iterdir()), [".install.lock"])
        self.assertIsNotNone(self.install())

    def test_modified_source_and_entrypoint_are_retained_on_uninstall(self):
        record = self.install()
        changed = self.data / "releases" / record["current"] / "ui/app.js"
        changed.write_text("personal changes")
        launcher = self.bin / "flightdeck"
        launcher.write_text("personal wrapper")
        unknown = changed.parent / "personal.txt"
        unknown.write_text("keep")
        retained = installer.uninstall(self.data)
        self.assertIn(str(changed), retained)
        self.assertIn(str(launcher), retained)
        self.assertEqual(changed.read_text(), "personal changes")
        self.assertEqual(launcher.read_text(), "personal wrapper")
        self.assertEqual(unknown.read_text(), "keep")

    def test_no_desktop_does_not_create_menu_entry(self):
        record = self.install(desktop=False)
        self.assertNotIn("desktop", record["entries"])
        self.assertFalse(self.applications.exists())

    def test_update_cannot_abandon_existing_desktop_entry(self):
        self.install()
        self.update_source()
        record = self.install(desktop=False)
        self.assertIn("desktop", record["entries"])
        self.assertIn(record["current"], (self.applications / "flightdeck.desktop").read_text())

    def test_missing_source_fails_before_creating_target(self):
        (self.source / "ui/index.html").unlink()
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertFalse(self.data.exists())

    def test_only_reviewed_png_bytes_are_allowed(self):
        image = self.source / "ui/flight-panorama.png"
        original = image.read_bytes()
        self.assertGreater(len(original), installer.MAX_FILE)
        image.write_bytes(original + b"modified")
        with self.assertRaisesRegex(installer.InstallError, "PNG"):
            self.install()
        image.write_bytes(original)
        (self.source / "ui/unreviewed.png").write_bytes(original[:32])
        with self.assertRaisesRegex(installer.InstallError, "PNG"):
            self.install()
        self.assertFalse(self.data.exists())

    def test_png_and_text_size_limits_are_bounded(self):
        image = self.source / "ui/flight-panorama.png"
        original = image.read_bytes()
        image.write_bytes(b"x" * (4 * 1024 * 1024 + 1))
        with self.assertRaisesRegex(installer.InstallError, "große"):
            self.install()
        image.write_bytes(original)
        (self.source / "ui/app.js").write_bytes(b"x" * (installer.MAX_FILE + 1))
        with self.assertRaisesRegex(installer.InstallError, "große"):
            self.install()
        self.assertFalse(self.data.exists())

    def test_source_links_and_special_files_are_rejected(self):
        path = self.source / "ui/index.html"
        path.unlink()
        path.symlink_to(self.state / "config.json")
        with self.assertRaises(installer.InstallError):
            self.install()
        path.unlink()
        os.mkfifo(path)
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertFalse(self.data.exists())

    def test_destination_symlink_refused(self):
        self.bin.parent.mkdir(exist_ok=True)
        self.bin.symlink_to(self.state, target_is_directory=True)
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertFalse((self.state / "flightdeck").exists())
        self.assertTrue((self.state / "config.json").exists())

    def test_dangling_launcher_link_is_not_replaced(self):
        self.bin.mkdir()
        link = self.bin / "flightdeck"
        link.symlink_to(self.base / "not-present")
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertTrue(link.is_symlink())

    def test_foreign_launcher_or_desktop_is_never_overwritten(self):
        for role in ("launcher", "desktop"):
            with self.subTest(role=role):
                self.bin.mkdir(exist_ok=True)
                self.applications.mkdir(parents=True, exist_ok=True)
                target = self.bin / "flightdeck" if role == "launcher" else self.applications / "flightdeck.desktop"
                target.write_text("unrelated application")
                with self.assertRaises(installer.InstallError):
                    self.install()
                self.assertEqual(target.read_text(), "unrelated application")
                if role == "desktop":
                    self.assertFalse((self.bin / "flightdeck").exists())
                self.assertFalse((self.data / "installation.json").exists())
                target.unlink()

    def test_write_failure_rolls_back_update(self):
        first = self.install()
        old_state = (self.data / "installation.json").read_bytes()
        old_launcher = (self.bin / "flightdeck").read_bytes()
        old_desktop = (self.applications / "flightdeck.desktop").read_bytes()
        self.update_source()
        real = installer.atomic_write
        def fail_commit(path, data, mode=0o600):
            if path == self.data / "installation.json":
                raise OSError("synthetic commit failure")
            return real(path, data, mode)
        with patch.object(installer, "atomic_write", fail_commit), self.assertRaises(OSError):
            self.install()
        self.assertEqual((self.data / "installation.json").read_bytes(), old_state)
        self.assertEqual((self.bin / "flightdeck").read_bytes(), old_launcher)
        self.assertEqual((self.applications / "flightdeck.desktop").read_bytes(), old_desktop)
        self.assertEqual([p.name for p in (self.data / "releases").iterdir()], [first["current"]])
        self.assertEqual(self.install()["previous"], first["current"])

    def test_initial_failure_is_retryable(self):
        real = installer.atomic_write
        def fail_commit(path, data, mode=0o600):
            if path == self.data / "installation.json":
                raise OSError("synthetic commit failure")
            return real(path, data, mode)
        with patch.object(installer, "atomic_write", fail_commit), self.assertRaises(OSError):
            self.install()
        self.assertFalse((self.bin / "flightdeck").exists())
        self.assertFalse((self.applications / "flightdeck.desktop").exists())
        self.assertIsNotNone(self.install())

    def test_modified_release_blocks_update_without_changes(self):
        current = self.install()
        source = self.data / "releases" / current["current"] / "ui/app.js"
        source.write_text("personal changes")
        previous = (self.data / "installation.json").read_bytes()
        self.update_source()
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertEqual((self.data / "installation.json").read_bytes(), previous)
        self.assertEqual(source.read_text(), "personal changes")

    def test_update_does_not_replace_dangling_release_link(self):
        first = self.install()
        self.update_source()
        identity, _ = installer.release_identity(installer.source_snapshot(self.source))
        target = self.data / "releases" / identity
        target.symlink_to(self.base / "absent-foreign-release")
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertTrue(target.is_symlink())
        self.assertEqual(installer.load_installation(self.data)["current"], first["current"])

    def test_foreign_root_directory_is_untouched(self):
        self.data.mkdir(parents=True)
        foreign = self.data / "personal.txt"
        foreign.write_text("keep")
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertEqual(foreign.read_text(), "keep")

    def test_concurrent_install_is_refused(self):
        installer.directory(self.data)
        with (self.data / ".install.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(installer.InstallError, "bereits"):
                self.install()
        self.assertFalse((self.data / "installation.json").exists())

    def test_uninstall_never_follows_release_file_link(self):
        current = self.install()
        path = self.data / "releases" / current["current"] / "ui/app.js"
        path.unlink()
        path.symlink_to(self.state / "config.json")
        retained = installer.uninstall(self.data)
        self.assertIn(str(path), retained)
        self.assertTrue(path.is_symlink())
        self.assertTrue((self.state / "config.json").exists())

    def test_paths_with_spaces_quotes_and_percent_are_not_shell_code(self):
        self.data = self.base / "install 'quoted' $dollar %field"
        self.bin = self.base / "bin with spaces %field"
        record = self.install()
        env = {**os.environ, "XDG_STATE_HOME": str(self.base / "isolated-state"), "PYTHONDONTWRITEBYTECODE": "1"}
        completed = subprocess.run([sys.executable, str(self.bin / "flightdeck"), "--help"],
                                   env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("--runtime", completed.stdout)
        self.assertIn('Exec="', (self.applications / "flightdeck.desktop").read_text())
        self.assertIn("%%field", (self.applications / "flightdeck.desktop").read_text())
        installer.verify_release(self.data, record["current"])

    def test_installed_wrapper_uninstall_management_path(self):
        self.install()
        # Invoke the real installed management entrypoint. No HOME override is
        # needed: the wrapper supplies its explicit installation directory.
        completed = subprocess.run([sys.executable, str(self.bin / "flightdeck"), "--uninstall"],
                                   capture_output=True, text=True, timeout=15)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertFalse((self.bin / "flightdeck").exists())
        self.assertTrue((self.state / "config.json").exists())

    def test_installed_launcher_serves_setup_resources(self):
        record = self.install()
        process = subprocess.Popen(
            [sys.executable, str(self.bin / "flightdeck"), "--no-browser", "--state-dir", str(self.state)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"})
        try:
            self.assertTrue(select.select([process.stdout], [], [], 10)[0], "Launcher startup timed out")
            line = process.stdout.readline().strip()
            if not line:
                _, errors = process.communicate(timeout=5)
                self.fail("Installed launcher failed: " + errors)
            self.assertTrue(line.startswith("Flightdeck: http://127.0.0.1:"), line)
            address = urlsplit(line.split(" ", 1)[1])
            base_url = address.scheme + "://" + address.netloc
            with urlopen(base_url + "/api/setup", timeout=5) as response:
                setup = json.load(response)
            self.assertTrue(setup["prepare_available"])
            self.assertEqual(setup["state"], "idle")
            self.assertEqual(setup["defaults"]["mode"], "install")
            for name in ("setup.js", "updates.js", "flight-panorama.png", "flight-panorama-2020.png", "manrope-variable.woff2", "index.html"):
                with urlopen(base_url + "/" + name, timeout=5) as response:
                    self.assertEqual(response.read(), (self.source / "ui" / name).read_bytes())
                    if name.endswith(".woff2"):
                        self.assertEqual(response.headers.get_content_type(), "font/woff2")
        finally:
            process.terminate()
            try:
                process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
        installer.verify_release(self.data, record["current"])

    def test_default_install_opens_installed_launcher(self):
        with patch.object(installer.os, "execv") as execute:
            result = installer.main(["--source", str(self.source), "--data-dir", str(self.data),
                                     "--bin-dir", str(self.bin), "--applications-dir", str(self.applications),
                                     "--language", "en"])
        self.assertEqual(result, 0)
        execute.assert_called_once_with(sys.executable, [sys.executable, str(self.bin / "flightdeck"), "--language", "en"])

    @unittest.skipUnless(shutil.which("desktop-file-validate"), "Desktop entry validator unavailable")
    def test_desktop_entry_is_valid(self):
        self.install()
        completed = subprocess.run(["desktop-file-validate", str(self.applications / "flightdeck.desktop")],
                                   capture_output=True, text=True, timeout=5)
        self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)

    def test_default_paths_use_temporary_home_and_xdg_values(self):
        synthetic_home = self.base / "synthetic-user"
        with patch.object(installer.Path, "home", return_value=synthetic_home), \
             patch.dict(os.environ, {"XDG_DATA_HOME": str(self.base / "custom-xdg")}), \
             patch.object(installer, "install", return_value={"current": "a" * 64, "entries": {"launcher": {"path": "unused"}}}) as call:
            self.assertEqual(installer.main(["--no-launch"]), 0)
        self.assertEqual(call.call_args.args[1], self.base / "custom-xdg/flightdeck-launcher")
        self.assertEqual(call.call_args.args[2], synthetic_home / ".local/bin")
        self.assertEqual(call.call_args.args[3], self.base / "custom-xdg/applications")

    def test_locale_precedence_and_english_fallback(self):
        cases = [({}, "en"), ({"LANG": "de_AT.UTF-8"}, "de"),
                 ({"LC_ALL": "C", "LC_MESSAGES": "de_DE", "LANG": "de_AT"}, "en"),
                 ({"LC_ALL": "", "LC_MESSAGES": "de-DE", "LANG": "en_US"}, "de"),
                 ({"LANG": "fr_FR.UTF-8"}, "en"), ({"LANG": "de@euro"}, "de"),
                 ({"LANG": "DE_de.UTF-8"}, "de"), ({"LANG": "default"}, "en")]
        for environment, expected in cases:
            with self.subTest(environment=environment):
                before = dict(environment)
                self.assertEqual(installer.detect_language(environment), expected)
                self.assertEqual(environment, before)

    def test_catalog_placeholders_and_opaque_values(self):
        fields = lambda message: {field for _, field, _, _ in Formatter().parse(message) if field}
        for german, english in installer.ENGLISH.items():
            with self.subTest(message=german):
                self.assertEqual(fields(german), fields(english))
                values = {field: "unchanged-{braces}-ü" for field in fields(german)}
                for language in ("de", "en"):
                    result = installer.tr(german, language=language, **values)
                    for value in values.values():
                        self.assertIn(value, result)

    def test_help_and_cli_errors_use_explicit_language_without_environment_changes(self):
        for language, help_word, error_word in (("de", "Optionen", "Ungültige Argumente"),
                                               ("en", "Options", "Invalid arguments")):
            with self.subTest(language=language):
                environment = dict(os.environ)
                output = io.StringIO()
                with contextlib.redirect_stdout(output), self.assertRaises(SystemExit) as ended:
                    installer.main(["--language", language, "--help"])
                self.assertEqual(ended.exception.code, 0)
                self.assertIn(help_word, output.getvalue())
                output = io.StringIO()
                with contextlib.redirect_stderr(output), self.assertRaises(SystemExit) as ended:
                    installer.main(["--language", language, "--unknown-option"])
                self.assertEqual(ended.exception.code, 2)
                self.assertIn(error_word, output.getvalue())
                self.assertEqual(dict(os.environ), environment)
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            self.assertEqual(installer.main(["--language", "de", "--language", "fr"]), 1)
        self.assertIn("Installation nicht abgeschlossen", output.getvalue())
        self.assertIn("Bitte --language de", output.getvalue())
        self.assertFalse(self.data.exists())

    def test_persisted_language_and_localized_desktop_comments(self):
        first = self.install(language="en")
        self.assertEqual(first["language"], "en")
        self.assertEqual(first["manager"], first["current"])
        desktop = (self.applications / "flightdeck.desktop").read_text()
        self.assertIn("Comment=Launch MSFS Xbox PC on Linux\n", desktop)
        self.assertIn("Comment[de]=MSFS Xbox PC unter Linux starten\n", desktop)
        self.assertIn("Comment[en]=Launch MSFS Xbox PC on Linux\n", desktop)
        changed = self.install(language="de")
        self.assertEqual(changed["current"], first["current"])
        self.assertEqual(changed["language"], "de")
        self.assertTrue((self.state / "config.json").exists())
        self.assertEqual((self.saves / "state.bin").read_bytes(), b"synthetic save\x00data")

    def test_wrapper_errors_and_uninstall_keep_language(self):
        self.install(language="de")
        invoke = lambda args: subprocess.run([sys.executable, str(self.bin / "flightdeck"), *args],
                                             capture_output=True, text=True, timeout=10)
        german = invoke(["--rollback"])
        self.assertEqual(german.returncode, 1)
        self.assertIn("Keine vorherige Launcher-Version", german.stderr)
        english = invoke(["--rollback", "--language=en"])
        self.assertEqual(english.returncode, 1)
        self.assertIn("No previous launcher version", english.stderr)
        extra = invoke(["--language", "en", "--uninstall", "unexpected"])
        self.assertEqual(extra.returncode, 1)
        self.assertIn("only accepts --language", extra.stderr)
        removed = invoke(["--uninstall", "--language", "en"])
        self.assertEqual(removed.returncode, 0, removed.stderr)
        self.assertIn("Launcher removed", removed.stdout)
        self.assertTrue((self.state / "config.json").exists())

    def test_wrapper_launch_update_and_legacy_rollback(self):
        module = self.source / "flightdeck/__main__.py"
        manager = self.source / "scripts/install-launcher.py"
        manager_bytes = manager.read_bytes()
        # A genuinely older manager does not understand the new language flag.
        manager.write_text('raise SystemExit("Legacy manager must not run")\n')
        module.write_text('import json,sys\ndef main():\n print(json.dumps(sys.argv[1:]))\n')
        first = self.install(language="de")
        manager.write_bytes(manager_bytes)
        module.write_text('SUPPORTED_LANGUAGES=("de","en")\nimport json,sys\ndef main():\n print(json.dumps(sys.argv[1:]))\n')
        second = self.install(language="en")
        invoke = lambda args: subprocess.run([sys.executable, str(self.bin / "flightdeck"), *args],
                                             capture_output=True, text=True, timeout=10)
        current = invoke([])
        self.assertEqual(json.loads(current.stdout), [])
        overridden = invoke(["--language=de"])
        self.assertEqual(json.loads(overridden.stdout), ["--language", "de"])
        rolled_back = invoke(["--rollback"])
        self.assertEqual(rolled_back.returncode, 0, rolled_back.stderr)
        self.assertEqual(json.loads(rolled_back.stdout.splitlines()[-1]), [])
        state = installer.load_installation(self.data)
        self.assertEqual(state["current"], first["current"])
        self.assertEqual(state["manager"], second["current"])
        unchanged_preference = invoke(["--update", str(self.source)])
        self.assertEqual(unchanged_preference.returncode, 0, unchanged_preference.stderr)
        self.assertEqual(json.loads(unchanged_preference.stdout.splitlines()[-1]), [])
        updated = invoke(["--update", str(self.source), "--language", "de"])
        self.assertEqual(updated.returncode, 0, updated.stderr)
        self.assertEqual(json.loads(updated.stdout.splitlines()[-1]), ["--language", "de"])
        self.assertEqual(installer.load_installation(self.data)["language"], "de")

    def test_wrapper_update_uses_selected_package_manager(self):
        manager = self.source / "scripts/install-launcher.py"
        current_manager = manager.read_bytes()
        manager.write_text('raise SystemExit("Old manager cannot install this package")\n')
        module = self.source / "flightdeck/__main__.py"
        module.write_text('def main():\n print("updated launcher started")\n')
        first = self.install()
        manager.write_bytes(current_manager)
        self.update_source()
        wrapper = self.bin / "flightdeck"
        invoke = lambda source: subprocess.run(
            [sys.executable, str(wrapper), "--update", str(source)],
            capture_output=True, text=True, timeout=15)
        missing = self.base / "missing-package"
        rejected = invoke(missing)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertEqual(installer.load_installation(self.data)["current"], first["current"])
        linked = self.base / "linked-package"
        linked.symlink_to(self.source, target_is_directory=True)
        rejected = invoke(linked)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertEqual(installer.load_installation(self.data)["current"], first["current"])
        source_file = self.source / "ui/index.html"
        source_bytes = source_file.read_bytes()
        source_file.unlink()
        rejected = invoke(self.source)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertEqual(installer.load_installation(self.data)["current"], first["current"])
        source_file.write_bytes(source_bytes)
        updated = invoke(self.source)
        self.assertEqual(updated.returncode, 0, updated.stderr)
        self.assertIn("updated launcher started", updated.stdout)
        state = installer.load_installation(self.data)
        self.assertEqual(state["previous"], first["current"])
        self.assertNotEqual(state["current"], first["current"])
        self.assertEqual((self.state / "config.json").read_text(), '{"synthetic_setting":true}')
        self.assertEqual((self.saves / "state.bin").read_bytes(), b"synthetic save\x00data")

    def test_locale_default_does_not_override_browser_language(self):
        with patch.object(installer.os, "execv") as execute, \
             patch.object(installer, "detect_language", return_value="de"):
            result = installer.main(["--source", str(self.source), "--data-dir", str(self.data),
                                     "--bin-dir", str(self.bin), "--applications-dir", str(self.applications)])
        self.assertEqual(result, 0)
        self.assertEqual(installer.load_installation(self.data)["language"], "de")
        execute.assert_called_once_with(sys.executable, [sys.executable, str(self.bin / "flightdeck")])

    def test_old_installation_metadata_and_invalid_language(self):
        original = self.install()
        metadata = self.data / "installation.json"
        old = {key: value for key, value in original.items() if key not in {"language", "manager"}}
        metadata.write_text(json.dumps(old))
        self.assertEqual(installer.load_installation(self.data)["current"], original["current"])
        migrated = self.install(language="en")
        self.assertEqual(migrated["language"], "en")
        metadata.write_text(json.dumps({**migrated, "language": ["en"]}))
        with self.assertRaises(installer.InstallError):
            installer.load_installation(self.data)

    def test_shell_bootstrap_localizes_missing_python(self):
        fake_bin = self.base / "fake-python"
        fake_bin.mkdir()
        executable = fake_bin / "python3"
        executable.write_text("#!/bin/sh\nexit 1\n")
        executable.chmod(0o700)
        for arguments, locale, expected in (([], "de_DE.UTF-8", "benötigt Python"),
                                           (["--language", "en"], "de_DE.UTF-8", "requires Python"),
                                           (["--language=de"], "en_US.UTF-8", "benötigt Python"),
                                           ([], "fr_FR.UTF-8", "requires Python")):
            with self.subTest(arguments=arguments, locale=locale):
                environment = {**os.environ, "PATH": str(fake_bin) + os.pathsep + os.environ["PATH"],
                               "LC_ALL": "", "LC_MESSAGES": "", "LANG": locale}
                completed = subprocess.run(["/bin/bash", str(ROOT / "install.sh"), *arguments],
                                           env=environment, capture_output=True, text=True, timeout=5)
                self.assertEqual(completed.returncode, 1)
                self.assertIn(expected, completed.stderr)
        self.assertFalse(self.data.exists())

    def test_wrapper_defaults_to_desktop_without_changing_explicit_modes(self):
        (self.source / "flightdeck/__main__.py").write_text(
            'SUPPORTED_LANGUAGES=("de","en")\nSUPPORTED_DESKTOP=True\nimport json,sys\ndef main():\n print(json.dumps(sys.argv[1:]))\n')
        self.install(language="de")
        for arguments, expected in (([], ["--desktop"]), (["--no-browser"], ["--no-browser"]),
                                    (["--help"], ["--help"]), (["-h"], ["-h"]),
                                    (["--desktop"], ["--desktop"]),
                                    (["--language", "en"], ["--language", "en", "--desktop"])):
            with self.subTest(arguments=arguments):
                completed = subprocess.run([sys.executable, str(self.bin / "flightdeck"), *arguments],
                                           capture_output=True, text=True, timeout=10)
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertEqual(json.loads(completed.stdout), expected)

    def test_source_desktop_entry_resolves_local_file_and_uri_without_shell(self):
        desktop = ROOT / "Install Flightdeck.desktop"
        command = next(line[5:] for line in desktop.read_text().splitlines() if line.startswith("Exec="))
        arguments = shlex.split(command)
        self.assertEqual(arguments[0:2], ["python3", "-c"])
        self.assertEqual(arguments[-1], "%k")
        self.assertNotIn("shell=True", command)
        source = self.base / "source with ü & $ characters"
        (source / "scripts").mkdir(parents=True)
        (source / "scripts/install-launcher.py").write_text('import json,sys\nprint(json.dumps(sys.argv[1:]))\n')
        copied = source / desktop.name
        copied.write_bytes(desktop.read_bytes())
        for location in (str(copied), copied.as_uri()):
            result = subprocess.run([sys.executable, *arguments[1:-1], location],
                                    cwd=self.base, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), ["--gui"])
        for location in ("https://example.invalid/install.desktop", "file://remote.invalid/tmp/install.desktop"):
            result = subprocess.run([sys.executable, *arguments[1:-1], location],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
        if shutil.which("desktop-file-validate"):
            result = subprocess.run(["desktop-file-validate", str(desktop)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_optional_gui_cancel_success_and_error_paths(self):
        specification = importlib.util.spec_from_file_location("installer_gui_test", ROOT / "scripts/install-launcher-gui.py")
        gui = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(gui)
        calls = []
        operation = lambda: calls.append("installed") or ("Ready", "/synthetic/launcher")
        with patch.object(gui, "command_dialog", return_value=["synthetic-dialog"]), \
             patch.object(gui.shutil, "which", return_value=None), \
             patch.object(gui.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)):
            self.assertEqual(gui.external_dialog(operation, str, "en", None), 0)
            self.assertEqual(calls, [])
        with patch.object(gui, "command_dialog", return_value=["synthetic-dialog"]), \
             patch.object(gui.shutil, "which", return_value=None), \
             patch.object(gui.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)), \
             patch.object(gui, "launch") as launch:
            self.assertEqual(gui.external_dialog(operation, str, "en", None), 0)
            self.assertEqual(calls, ["installed"])
            launch.assert_called_once_with("/synthetic/launcher", None)
        def failing():
            raise OSError("synthetic failure")
        with patch.object(gui, "command_dialog", return_value=["synthetic-dialog"]) as dialog, \
             patch.object(gui.shutil, "which", return_value=None), \
             patch.object(gui.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
            self.assertEqual(gui.external_dialog(failing, str, "de", "de"), 1)
            self.assertEqual(dialog.call_args.args, ("error", "synthetic failure", "de"))

    def test_gui_request_uses_existing_atomic_installation_and_no_terminal_launch(self):
        output = self.base / "gui-contract.json"
        helper = self.base / "gui-helper.py"
        helper.write_text('import json\nfrom pathlib import Path\ndef run(operation, explain, language, explicit_language, **kw):\n'
                          ' message, launcher = operation()\n'
                          f' Path({str(output)!r}).write_text(json.dumps([language, explicit_language, launcher]))\n return 0\n')
        real_spec = installer.importlib.util.spec_from_file_location
        def select_helper(name, path):
            return real_spec(name, helper if name == "flightdeck_install_gui" else path)
        with patch.object(installer.importlib.util, "spec_from_file_location", side_effect=select_helper):
            result = installer.main(["--gui", "--language", "en", "--source", str(self.source),
                                     "--data-dir", str(self.data), "--bin-dir", str(self.bin),
                                     "--applications-dir", str(self.applications)])
        self.assertEqual(result, 0)
        self.assertEqual(json.loads(output.read_text()), ["en", "en", str(self.bin / "flightdeck")])
        self.assertTrue((self.data / "installation.json").is_file())

    def native_bundle(self):
        root = self.source / "flightdeck/resources/native"
        root.mkdir(parents=True)
        hashes = {}
        for name in installer.NATIVE_FILES:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            content = (b"MZ\x00" if name.endswith((".dll", ".exe")) else b"\x7fELF\x00") + name.encode()
            path.write_bytes(content)
            hashes[name] = installer.digest(content)
        notices = b"Synthetic third-party notices for installer tests.\n"
        (root / "THIRD-PARTY-NOTICES.txt").write_bytes(notices)
        (root / "manifest.json").write_text(json.dumps({"format": 1, "files": hashes}))
        specification = self.source / "compat/bootstrap.lock.json"
        value = json.loads(specification.read_text())
        value["native"]["features"] = list(installer.NATIVE_FEATURES)
        value["native"]["files"] = hashes
        value["native"]["notice_sha256"] = installer.digest(notices)
        specification.write_text(json.dumps(value))
        return root

    def test_optional_native_bundle_requires_exact_pins_and_preserves_bytes(self):
        root = self.native_bundle()
        state = self.install()
        release = installer.verify_release(self.data, state["current"])
        for name in (*installer.NATIVE_FILES, "manifest.json", "THIRD-PARTY-NOTICES.txt"):
            installed = release / "flightdeck/resources/native" / name
            self.assertEqual(installed.read_bytes(), (root / name).read_bytes())
            self.assertEqual(installed.stat().st_mode & 0o777, 0o755 if name.startswith("bin/") else 0o644)
        self.assertEqual(installer.uninstall(self.data), [])

    def test_native_helper_requires_feature_and_file(self):
        root = self.native_bundle()
        specification = self.source / "compat/bootstrap.lock.json"
        value = json.loads(specification.read_text())
        for features in (None, [], "connected-storage-read-v1", [True], ["connected-storage-read-v1"], ["connected-storage-sync-v1"]):
            value["native"]["features"] = features
            specification.write_text(json.dumps(value))
            with self.subTest(features=features), self.assertRaises(installer.InstallError):
                self.install()
        value["native"]["features"] = list(installer.NATIVE_FEATURES)
        specification.write_text(json.dumps(value))
        (root / "bin/flightdeck-connected-storage.exe").unlink()
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertFalse(self.data.exists())

    def test_native_corrupt_binary_or_notice_fails_before_install(self):
        root = self.native_bundle()
        for name in (installer.NATIVE_FILES[0], "THIRD-PARTY-NOTICES.txt"):
            path = root / name
            original = path.read_bytes()
            path.write_bytes(original + b"changed")
            with self.assertRaisesRegex(installer.InstallError, "Prüfsumme"):
                self.install()
            self.assertFalse(self.data.exists())
            path.write_bytes(original)

    def test_native_unknown_file_and_manifest_mismatch_are_rejected(self):
        root = self.native_bundle()
        foreign = root / "unreviewed.dll"
        foreign.write_bytes(b"MZunreviewed")
        with self.assertRaises(installer.InstallError):
            self.install()
        foreign.unlink()
        manifest = root / "manifest.json"
        manifest.write_text(json.dumps({"format": 1, "files": {}}))
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertFalse(self.data.exists())

    def test_native_missing_file_or_symlink_is_rejected(self):
        root = self.native_bundle()
        path = root / installer.NATIVE_FILES[0]
        data = path.read_bytes()
        path.unlink()
        with self.assertRaises(installer.InstallError):
            self.install()
        target = self.base / "foreign-binary"
        target.write_bytes(data)
        path.symlink_to(target)
        with self.assertRaises(installer.InstallError):
            self.install()
        self.assertEqual(target.read_bytes(), data)
        self.assertFalse(self.data.exists())

    def test_native_file_and_total_budgets_are_enforced(self):
        self.native_bundle()
        with patch.object(installer, "NATIVE_FILE_MAX", 4), self.assertRaises(installer.InstallError):
            self.install()
        with patch.object(installer, "NATIVE_TOTAL_MAX", 4), self.assertRaisesRegex(installer.InstallError, "erlaubte Größe"):
            self.install()
        self.assertFalse(self.data.exists())


if __name__ == "__main__":
    unittest.main()
