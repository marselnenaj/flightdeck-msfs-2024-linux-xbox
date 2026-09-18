"""Backend localization without changing worker state, paths or identifiers."""
# SPDX-License-Identifier: MIT
import copy
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import string
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck.backend import Launcher
from flightdeck.i18n import CATALOG, error_message, language, localize, message, translate_message
from flightdeck.setup import SetupError, regular


class LanguageTests(unittest.TestCase):
    def test_language_negotiation(self):
        cases = {None: "de", "": "de", "de": "de", "en": "en", "DE-at": "de", "en-US,en;q=0.8": "en",
                 "fr": "en", "fr-CA, de;q=0.8": "de", "de;q=0, en;q=0.5": "en", "en;q=0.5, de;q=0.9": "de",
                 "en;q=0.9,de;q=0.9": "en", "de;q=garbage": "en", "de;q=NaN": "en", "*": "en"}
        for header, expected in cases.items():
            with self.subTest(header=header):
                self.assertEqual(language(header), expected)

    def test_catalog_templates_have_identical_parameters(self):
        formatter = string.Formatter()
        for source, target in CATALOG.items():
            source_fields = {name for _, name, _, _ in formatter.parse(source) if name is not None}
            target_fields = {name for _, name, _, _ in formatter.parse(target) if name is not None}
            self.assertEqual(source_fields, target_fields, source)
            self.assertTrue(target.strip())

    def test_translates_only_known_ui_fields(self):
        raw = {"label": "Vorhanden", "state": "ready", "id": "Vorhanden", "path": "Vorhanden", "csrf_token": "Vorhanden",
               "defaults": {"runtime_path": "/tmp/Spielstände/Geprüft", "market": "AT"},
               "checks": [{"id": "private", "detail": "Privater Datenordner fehlt", "ok": False}],
               "summary": {"auth_http": [200], "hresult": "00000000"}}
        result = localize(raw, "en")
        self.assertEqual(result["label"], "Present")
        self.assertEqual(result["checks"][0]["detail"], "Private data folder missing")
        for key in ("state", "id", "path", "csrf_token", "defaults", "summary"):
            self.assertEqual(result[key], raw[key])
        self.assertEqual(raw["label"], "Vorhanden")
        self.assertEqual(localize(raw, "de"), raw)

    def test_formatted_label_translates_without_modifying_filename(self):
        name = "/tmp/Spielpaket/Vorhanden {raw}.dll"
        label = message("Build-Artefakt: {name}", name=name)
        value = message("{label} fehlt oder ist nicht lesbar.", label=label)
        self.assertEqual(str(value), f"Build-Artefakt: {name} fehlt oder ist nicht lesbar.")
        self.assertEqual(translate_message(value, "en"), f"Build artifact: {name} is missing or is not readable.")
        copied = copy.deepcopy(value)
        self.assertEqual(translate_message(copied, "en"), translate_message(value, "en"))
        self.assertEqual(translate_message(error_message(SetupError(copied)), "en"), translate_message(value, "en"))

    def test_real_missing_file_error_keeps_format_metadata(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(SetupError) as raised:
                regular(Path(root) / "missing", message("Mitgeliefertes Startskript: {name}", name="runtime-env.sh"))
        self.assertEqual(translate_message(error_message(raised.exception), "en"), "Bundled launch script: runtime-env.sh is missing or is not readable.")

    def test_unknown_text_is_never_guessed_or_partially_rewritten(self):
        value = "/tmp/Vorhanden/Prüfsumme stimmt nicht: untouched"
        self.assertEqual(translate_message(value, "en"), value)
        self.assertEqual(localize({"error": value}, "en")["error"], value)


class WorkerLanguageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.launcher = Launcher(self.root / "state")
        self.manager = self.launcher.setup

    def tearDown(self):
        self.manager.close()
        self.temp.cleanup()

    def test_active_job_can_be_read_in_both_languages_concurrently(self):
        entered, finish = threading.Event(), threading.Event()
        def checking(data):
            self.manager._notify("prefix", "Wine-Umgebung wird unabhängig kopiert. Das kann einige Minuten dauern …", None,
                                 {"id": "prefix", "label": "Wine-Umgebung", "ok": True, "detail": "Geprüft"})
            entered.set()
            if not finish.wait(3):
                raise SetupError("Einrichtung abgebrochen.")
            return self.root
        with patch.object(self.manager, "_check_existing", side_effect=checking):
            result = self.manager.check({"mode": "existing", "runtime_path": str(self.root)})
            try:
                self.assertTrue(entered.wait(2))
                def read(locale):
                    return locale, localize(self.manager.snapshot(), locale)["job"]
                with ThreadPoolExecutor(max_workers=4) as pool:
                    results = list(pool.map(read, ["de", "en"] * 10))
                for locale, job in results:
                    self.assertEqual(job["id"], result["job"]["id"])
                    self.assertEqual(job["state"], "checking")
                    self.assertEqual(job["phase"], "prefix")
                    self.assertEqual(job["checks"][0]["label"], "Wine prefix" if locale == "en" else "Wine-Umgebung")
                    self.assertTrue(job["message"].startswith("Creating an independent" if locale == "en" else "Wine-Umgebung"))
            finally:
                finish.set()
                self.manager.thread.join(timeout=3)
        self.assertEqual(self.manager.snapshot()["state"], "ready")
        self.assertEqual(localize(self.manager.snapshot(), "en")["job"]["message"], "Checks complete. Setup can now start.")
        self.manager.cancel(result["job"]["id"])
        self.assertEqual(localize(self.manager.snapshot(), "en")["job"]["message"], "Setup cancelled.")
        self.assertEqual(self.manager.snapshot()["job"]["message"], "Einrichtung abgebrochen.")

    def test_failed_async_formatted_error_relocalizes_after_completion(self):
        failure = SetupError(message("{label} fehlt oder ist nicht lesbar.", label=message("Build-Artefakt: {name}", name="Original.dll")))
        with patch.object(self.manager, "_check_existing", side_effect=failure):
            self.manager.check({"mode": "existing", "runtime_path": str(self.root)})
            self.manager.thread.join(timeout=2)
        raw = self.manager.snapshot()
        self.assertEqual(raw["state"], "failed")
        en = localize(raw, "en")["job"]
        self.assertEqual(en["error"], "Build artifact: Original.dll is missing or is not readable.")
        self.assertEqual(en["message"], en["error"])
        self.assertIn("Build-Artefakt", localize(raw, "de")["job"]["error"])

    def test_native_picker_uses_request_language_only(self):
        with patch.object(self.manager, "_picker", return_value=("zenity", "/usr/bin/zenity")), patch("flightdeck.setup.subprocess.run", return_value=subprocess.CompletedProcess([], 1, "")) as run:
            self.manager.pick("game_path", str(self.root), language="en")
            self.assertIn("--title=Flightdeck – Select folder", run.call_args.args[0])
            self.manager.pick("game_path", str(self.root), language="de")
            self.assertIn("--title=Flightdeck – Ordner auswählen", run.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
