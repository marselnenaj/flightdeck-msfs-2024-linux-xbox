"""Local-only region suggestions; no locale, timezone or runtime mutations."""
# SPDX-License-Identifier: MIT
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from flightdeck import setup


ZONE_TABLE = """# Synthetic tzdata subset
AT\t+4813+01620\tEurope/Vienna
US\t+404251-0740023\tAmerica/New_York
DE,DK\t+5230+01322\tEurope/Berlin
"""


class SetupRegionTests(unittest.TestCase):
    def setUp(self):
        setup._zone_regions.cache_clear()
        self.addCleanup(setup._zone_regions.cache_clear)

    def suggest(self, env, *, link="/usr/share/zoneinfo/Europe/Vienna", timezone="", table=ZONE_TABLE):
        def read(path, maximum):
            return timezone if path == "/etc/timezone" else table
        with patch.dict(setup.os.environ, env, clear=True), \
                patch.object(setup.os, "readlink", return_value=link), \
                patch.object(setup, "_region_file", side_effect=read):
            return setup.suggested_market()

    def test_austrian_timezone_overrides_english_us_language(self):
        self.assertEqual(self.suggest({"LANG": "en_US.UTF-8"}), "AT")
        self.assertEqual(self.suggest({"TZ": "Europe/Vienna", "LANG": "en_US.UTF-8"}), "AT")

    def test_us_timezone_is_a_real_us_suggestion(self):
        self.assertEqual(self.suggest({"TZ": "America/New_York", "LANG": "de_AT.UTF-8"}), "US")

    def test_utc_unknown_empty_and_posix_timezone_do_not_borrow_machine_zone(self):
        for zone in ("UTC", "Etc/UTC", "", "Unknown/Zone", "EST5EDT,M3.2.0,M11.1.0", "/tmp/not-a-timezone"):
            with self.subTest(zone=zone):
                self.assertEqual(self.suggest({"TZ": zone, "LANG": "en_US.UTF-8"}), "US")
                self.assertEqual(self.suggest({"TZ": zone}), "")

    def test_multi_country_zone_requires_explicit_locale_region(self):
        self.assertEqual(self.suggest({"TZ": "Europe/Berlin", "LANG": "da_DK.UTF-8"}), "DK")
        self.assertEqual(self.suggest({"TZ": "Europe/Berlin", "LANG": "de"}), "")

    def test_region_categories_precede_message_language_fallback(self):
        self.assertEqual(self.suggest({"TZ": "UTC", "LC_ADDRESS": "de_AT.UTF-8",
                                      "LC_MONETARY": "de_DE.UTF-8", "LANG": "en_US.UTF-8"}), "AT")
        self.assertEqual(self.suggest({"TZ": "UTC", "LC_ADDRESS": "C", "LC_MONETARY": "de_AT.UTF-8",
                                      "LANG": "en_US.UTF-8"}), "AT")
        self.assertEqual(self.suggest({"TZ": "UTC", "LC_ALL": "fr_CA.UTF-8", "LANG": "en_US.UTF-8"}), "CA")

    def test_no_usable_region_requires_selection_instead_of_us(self):
        for value in ("", "C", "C.UTF-8", "en", "de", "en_US/invalid", "en_US\n", "x" * 1024):
            with self.subTest(locale=value):
                self.assertEqual(self.suggest({"TZ": "UTC", "LANG": value}), "")

    def test_standard_timezone_paths_and_relative_localtime_link(self):
        for value in (":Europe/Vienna", ":/usr/share/zoneinfo/Europe/Vienna",
                      "/usr/share/zoneinfo/posix/Europe/Vienna", "/usr/share/zoneinfo/right/Europe/Vienna"):
            with self.subTest(zone=value):
                self.assertEqual(self.suggest({"TZ": value, "LANG": "en_US.UTF-8"}), "AT")
        self.assertEqual(self.suggest({"LANG": "en_US.UTF-8"}, link="../usr/share/zoneinfo/Europe/Vienna"), "AT")

    def test_fixed_timezone_file_fallback_when_localtime_is_not_symlink(self):
        def read(path, maximum):
            return "Europe/Vienna\n" if path == "/etc/timezone" else ZONE_TABLE
        with patch.dict(setup.os.environ, {"LANG": "en_US.UTF-8"}, clear=True), \
                patch.object(setup.os, "readlink", side_effect=OSError), \
                patch.object(setup, "_region_file", side_effect=read):
            self.assertEqual(setup.suggested_market(), "AT")

    def test_table_is_read_once_across_polls_and_missing_table_uses_locale(self):
        with patch.dict(setup.os.environ, {"TZ": "Europe/Vienna"}, clear=True), \
                patch.object(setup, "_region_file", return_value=ZONE_TABLE) as read:
            self.assertEqual([setup.suggested_market() for _ in range(10)], ["AT"] * 10)
            read.assert_called_once_with("/usr/share/zoneinfo/zone.tab", 65536)
        setup._zone_regions.cache_clear()
        self.assertEqual(self.suggest({"TZ": "Europe/Vienna", "LANG": "en_US.UTF-8"}, table=""), "US")

    def test_conflicting_or_invalid_table_entries_never_guess(self):
        for extra in ("US\t+0000+00000\tEurope/Vienna\n", "invalid\t+0000+00000\tEurope/Vienna\n"):
            setup._zone_regions.cache_clear()
            self.assertEqual(self.suggest({"TZ": "Europe/Vienna"}, table=ZONE_TABLE + extra), "")

    def test_system_hint_reader_rejects_oversize_symlink_and_fifo(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            plain = root / "plain"
            plain.write_text("Europe/Vienna\n")
            self.assertEqual(setup._region_file(plain, 512), "Europe/Vienna\n")
            self.assertEqual(setup._region_file(plain, 4), "")
            alias = root / "alias"
            alias.symlink_to(plain)
            self.assertEqual(setup._region_file(alias, 512), "")
            fifo = root / "fifo"
            os.mkfifo(fifo)
            self.assertEqual(setup._region_file(fifo, 512), "")
            plain.write_bytes(b"\xff")
            self.assertEqual(setup._region_file(plain, 512), "")


if __name__ == "__main__":
    unittest.main()
