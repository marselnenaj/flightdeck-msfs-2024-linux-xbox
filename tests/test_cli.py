# SPDX-License-Identifier: MIT
"""Language choices in the installed CLI must reach the browser URL."""
import contextlib
import io
import unittest
from unittest.mock import Mock, patch

from flightdeck import __main__ as cli
from flightdeck import desktop


class CliLanguageTests(unittest.TestCase):
    def run_cli(self, arguments):
        server = Mock(url="http://127.0.0.1:43210")
        server.serve_forever.side_effect = KeyboardInterrupt
        output = io.StringIO()
        with patch.object(cli, "Launcher"), patch.object(cli, "Server", return_value=server), \
                patch.object(cli.webbrowser, "open") as browser, contextlib.redirect_stdout(output):
            cli.main(arguments)
        server.server_close.assert_called_once()
        return output.getvalue(), browser

    def test_explicit_german_reaches_browser(self):
        output, browser = self.run_cli(["--language", "de"])
        browser.assert_called_once_with("http://127.0.0.1:43210/?lang=de")
        self.assertIn("Nur lokal.", output)

    def test_explicit_english_and_no_browser(self):
        output, browser = self.run_cli(["--language", "en", "--no-browser"])
        browser.assert_not_called()
        self.assertIn("/?lang=en", output)
        self.assertIn("Local only.", output)

    def test_no_flag_preserves_browser_preference(self):
        with patch.dict(cli.os.environ, {"LC_ALL": "en_US.UTF-8"}):
            output, browser = self.run_cli([])
        browser.assert_called_once_with("http://127.0.0.1:43210")
        self.assertNotIn("?lang=", output)
        self.assertIn("Local only.", output)

    def test_environment_order_and_fallback(self):
        with patch.dict(cli.os.environ, {"LC_ALL": "de_AT.UTF-8", "LC_MESSAGES": "en_US", "LANG": "en"}):
            self.assertEqual(cli.environment_language(), "de")
        with patch.dict(cli.os.environ, {"LC_ALL": "", "LC_MESSAGES": "de-DE", "LANG": "en"}):
            self.assertEqual(cli.environment_language(), "de")
        with patch.dict(cli.os.environ, {"LC_ALL": "fr_FR.UTF-8"}):
            self.assertEqual(cli.environment_language(), "en")

    def test_help_is_localized(self):
        for language, expected in (("de", "Browser nicht automatisch"), ("en", "Do not open the browser")):
            with self.subTest(language=language), contextlib.redirect_stdout(io.StringIO()) as output:
                with self.assertRaises(SystemExit) as stopped:
                    cli.main(["--language", language, "--help"])
                self.assertEqual(stopped.exception.code, 0)
                self.assertIn(expected, output.getvalue())

    def test_startup_error_keeps_language(self):
        with patch.object(cli, "Launcher", side_effect=cli.LauncherError("Zuerst eine Runtime auswählen.")), \
                contextlib.redirect_stderr(io.StringIO()) as error:
            with self.assertRaises(SystemExit) as stopped:
                cli.main(["--language", "en", "--no-browser"])
            self.assertEqual(stopped.exception.code, 1)
            self.assertIn("Select a runtime first.", error.getvalue())

    def test_desktop_dispatch_preserves_explicit_language_only(self):
        for arguments, expected in ((["--desktop"], None), (["--desktop", "--language", "de"], "de")):
            with self.subTest(arguments=arguments), patch.object(desktop, "start") as start, \
                 patch.object(cli, "Server") as server:
                cli.main(arguments)
                self.assertEqual(start.call_args.args[3], expected)
                server.assert_not_called()

    def test_no_browser_remains_foreground_even_with_desktop_flag(self):
        with patch.object(desktop, "start") as start:
            _, browser = self.run_cli(["--desktop", "--no-browser"])
            start.assert_not_called()
            browser.assert_not_called()

    def test_hidden_service_does_not_open_an_interface(self):
        with patch.object(desktop, "serve") as serve, patch.object(desktop, "start") as start:
            cli.main(["--desktop-service", "--port", "43123"])
            self.assertEqual(serve.call_args.args[2], 43123)
            start.assert_not_called()


if __name__ == "__main__":
    unittest.main()
