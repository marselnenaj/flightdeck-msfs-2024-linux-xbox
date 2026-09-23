# SPDX-License-Identifier: MIT
"""Synthetic MSFS 2020/2024 routing and isolated runtime switching."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck import games, game_install, game_update, cloud_runtime
from flightdeck.backend import Launcher, LauncherError
from flightdeck.setup import existing_checks


def package(root, game_id, version="1.0.0.0"):
    spec = games.select(game_id)
    root.mkdir(parents=True, exist_ok=True)
    (root / spec.executable).write_bytes(b"synthetic encrypted executable")
    (root / ".xodus-streaming.msixvc").write_bytes(b"synthetic marker")
    (root / "MicrosoftGame.Config").write_text(
        f'<Game><Identity Name="Example.FlightSimulator" Publisher="CN=Example" Version="{version}"/>'
        f'<StoreId>{spec.store_id}</StoreId><TitleId>7B</TitleId>'
        f'<Executable Name="{spec.executable}"/></Game>')
    return root


def runtime(root, game_id):
    spec = games.select(game_id)
    for name in ("private", "games", "tools", "local/msfs-prefix"):
        (root / name).mkdir(parents=True, exist_ok=True, mode=0o700)
    (root / "private/runtime.json").write_text(json.dumps({"format": 1, "game_id": game_id, "market": "AT"}))
    (root / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 0\n")
    (root / "tools/play-msfs.sh").chmod(0o700)
    (root / "local/msfs-prefix/system.reg").write_text("synthetic")
    bridge = root / "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"
    bridge.parent.mkdir(parents=True)
    bridge.write_bytes(b"synthetic")
    original = package(root.parent / (game_id + "-package"), game_id)
    (root / "games" / spec.directory).symlink_to(original)
    return root, original


class GameVersions(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_legacy_version_and_invalid_runtime_metadata(self):
        folder = self.root / "legacy"
        (folder / "private").mkdir(parents=True)
        self.assertEqual(games.for_runtime(folder).id, "msfs2024")
        settings = folder / "private/runtime.json"
        settings.write_text('{"game_id":"msfs2020"}')
        self.assertEqual(games.for_runtime(folder).store_id, "9NRRJLLXM68V")
        settings.write_text('{"game_id":"unknown"}')
        with self.assertRaises(ValueError): games.for_runtime(folder)
        settings.unlink(); settings.symlink_to(self.root / "outside")
        with self.assertRaises(ValueError): games.for_runtime(folder)

    def test_switch_keeps_both_prefixes_packages_and_saves(self):
        base = self.root / "data/flightdeck/runtimes"
        first, _ = runtime(base / "msfs2024", "msfs2024")
        second, _ = runtime(base / "msfs2020", "msfs2020")
        for path in (first, second):
            saved = path / "private/local-saves"
            saved.mkdir()
            (saved / "save.bin").write_text(path.name)
        launcher = Launcher(self.root / "state", str(first))
        self.addCleanup(launcher.setup.close)
        with patch("flightdeck.setup.data_home", return_value=self.root / "data"):
            discovered = launcher.setup.discover()["runtimes"]
        self.assertEqual({row["game_id"] for row in discovered}, {"msfs2020", "msfs2024"})
        self.assertTrue(all(row["ready"] for row in discovered))
        self.assertEqual(launcher.status()["runtime"]["game_id"], "msfs2024")
        launcher.configure(str(second))
        self.assertEqual(launcher.status()["runtime"]["game_id"], "msfs2020")
        self.assertTrue(all(row["ok"] for row in existing_checks(second)))
        launcher.configure(str(first))
        self.assertEqual((first / "private/local-saves/save.bin").read_text(), "msfs2024")
        self.assertEqual((second / "private/local-saves/save.bin").read_text(), "msfs2020")

    def test_remember_legacy_location_and_switch_after_restart(self):
        current, _ = runtime(self.root / "data/flightdeck/runtimes/msfs2020", "msfs2020")
        legacy, _ = runtime(self.root / "older-installation", "msfs2024")
        launcher = Launcher(self.root / "state", str(current))
        self.addCleanup(launcher.setup.close)
        with patch("flightdeck.setup.data_home", return_value=self.root / "data"):
            self.assertFalse(launcher.version_runtimes()["msfs2024"]["installed"])
            launcher.register_runtime(str(legacy))
            self.assertEqual(launcher.status()["runtime"]["game_id"], "msfs2020")
            self.assertEqual(launcher.version_runtimes()["msfs2024"],
                             {"path": str(legacy), "installed": True, "ready": True})
            self.assertEqual({row["game_id"] for row in launcher.setup.discover()["runtimes"]},
                             {"msfs2020", "msfs2024"})
            launcher.select_game("msfs2024")
            self.assertEqual(launcher.status()["runtime"]["path"], str(legacy))
            launcher.select_game("msfs2020")
            self.assertEqual(launcher.status()["runtime"]["path"], str(current))
            restarted = Launcher(self.root / "state")
            self.addCleanup(restarted.setup.close)
            self.assertEqual(restarted.version_runtimes()["msfs2024"]["path"], str(legacy))
            restarted.select_game("msfs2024")
            self.assertEqual(restarted.status()["runtime"]["game_id"], "msfs2024")

    def test_switch_refuses_unready_unknown_and_busy_game(self):
        current, _ = runtime(self.root / "data/flightdeck/runtimes/msfs2020", "msfs2020")
        legacy, _ = runtime(self.root / "older-installation", "msfs2024")
        launcher = Launcher(self.root / "state", str(current))
        self.addCleanup(launcher.setup.close)
        with patch("flightdeck.setup.data_home", return_value=self.root / "data"):
            with self.assertRaises(LauncherError): launcher.select_game("unknown")
            with self.assertRaises(LauncherError): launcher.select_game("msfs2024")
            launcher.register_runtime(str(legacy))
            launcher.setup_busy = True
            with self.assertRaises(LauncherError): launcher.select_game("msfs2024")
            launcher.setup_busy = False
            (legacy / "tools/play-msfs.sh").unlink()
            with self.assertRaises(LauncherError): launcher.select_game("msfs2024")
            self.assertEqual(launcher.status()["runtime"]["path"], str(current))

    def test_2020_download_uses_exact_store_id_and_executable(self):
        cli = self.root / "cli"
        cli.write_text("#!" + sys.executable + "\n" + '''
import pathlib, sys
if sys.argv[1] == 'streaming':
    if sys.argv[2] != '9NRRJLLXM68V': sys.exit(2)
    target = pathlib.Path(sys.argv[3]); target.mkdir(exist_ok=True)
    (target / 'FlightSimulator.exe').write_bytes(b'synthetic')
    (target / '.xodus-streaming.msixvc').write_bytes(b'synthetic')
    (target / 'MicrosoftGame.Config').write_text('<Game><StoreId>9NRRJLLXM68V</StoreId><Executable Name="FlightSimulator.exe"/></Game>')
''')
        cli.chmod(0o700)
        checksum = hashlib.sha256(cli.read_bytes()).hexdigest()
        target = self.root / "download"
        self.assertEqual(game_install.download_game(cli, checksum, target, "AT", game_id="msfs2020"), target)
        self.assertTrue((target / "FlightSimulator.exe").is_file())
        with self.assertRaises(game_install.GameInstallError):
            game_install.validate_download(target, "msfs2024")
        config = target / "MicrosoftGame.Config"
        config.write_text(config.read_text().replace("9NRRJLLXM68V", "9P38D19T7LRV"))
        with self.assertRaises(game_install.GameInstallError):
            game_install.validate_download(target, "msfs2020")

    def test_2020_update_and_rollback_touch_only_its_package(self):
        folder, original = runtime(self.root / "msfs2020", "msfs2020")
        other, _ = runtime(self.root / "msfs2024", "msfs2024")
        launcher = Launcher(self.root / "state", str(folder))
        self.addCleanup(launcher.setup.close)
        latest = {"schema": 1, "store_id": games.select("msfs2020").store_id,
                  "version": "2.0.0.0", "version_id": "22222222-2222-4222-8222-222222222222",
                  "content_id": "11111111-1111-4111-8111-111111111111",
                  "package_identity": "a" * 64, "size_bytes": 4096}
        self.assertEqual(game_update.validate_info(latest, game_id="msfs2020"), latest)
        with self.assertRaises(game_update.UpdateError):
            game_update.validate_info(latest)
        plan = game_update.UpdatePlan(folder, game_update.installed(original, game_id="msfs2020"),
                                      original, latest, self.root / "unused-cli", "a" * 64, [], "AT")
        def download(_cli, _sha, target, _market, **kw):
            self.assertEqual(kw["game_id"], "msfs2020")
            return package(target, "msfs2020", "2.0.0.0")
        with patch.object(game_update, "download_game", side_effect=download):
            game_update.install(launcher, plan, notify=lambda *args: None, cancel=threading.Event(),
                                control=game_install.DownloadControl(), committing=lambda: None)
        self.assertEqual(game_update.installed(games.path(folder), game_id="msfs2020")["version"], "2.0.0.0")
        self.assertEqual(game_update.installed(games.path(other))["version"], "1.0.0.0")
        self.assertTrue(game_update.rollback(launcher)["ok"])
        self.assertEqual(games.path(folder).resolve(), original)

    def test_2020_cloud_scope_uses_2020_store_identity(self):
        folder, _ = runtime(self.root / "msfs2020", "msfs2020")
        self.assertEqual(cloud_runtime._config(folder)["title_id"], 0x7b)
        game_config = games.path(folder) / "MicrosoftGame.Config"
        game_config.write_text(game_config.read_text().replace("9NRRJLLXM68V", "9P38D19T7LRV"))
        from flightdeck.cloud_storage import CloudStorageError
        with self.assertRaises(CloudStorageError): cloud_runtime._config(folder)

    def test_start_script_routes_each_edition_to_its_own_entrypoint(self):
        scripts = Path(__file__).resolve().parents[1] / "scripts/runtime"
        for game_id in ("msfs2020", "msfs2024"):
            with self.subTest(game_id=game_id):
                folder, _ = runtime(self.root / (game_id + "-launch"), game_id)
                for name in ("runtime-env.sh", "launch-msfs.sh", "xodus.sh"):
                    shutil.copy2(scripts / name, folder / "tools" / name)
                    (folder / "tools" / name).chmod(0o700)
                cli = folder / "bin/xodus-cli"
                cli.parent.mkdir()
                cli.write_text("#!" + sys.executable + "\nimport json,os,sys\n"
                               "open(os.environ['FLIGHTDECK_TEST_CAPTURE'],'w').write(json.dumps(sys.argv[1:]))\n")
                cli.chmod(0o700)
                run_dir = self.root / (game_id + "-xdg-run")
                run_dir.mkdir()
                capture = self.root / (game_id + "-arguments.json")
                environment = dict(os.environ, XDG_RUNTIME_DIR=str(run_dir), FLIGHTDECK_TEST_CAPTURE=str(capture))
                result = subprocess.run(["bash", str(folder / "tools/launch-msfs.sh")],
                                        env=environment, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                argv = json.loads(capture.read_text())
                self.assertEqual(argv[0:2], ["run", str(games.path(folder))])
                self.assertEqual(argv[-4:], ["--exe", games.select(game_id).executable, "--market", "AT"])
                self.assertEqual(argv[2], str(folder / "tools/xodus-wine-launch"))
                capture.unlink()
                (games.path(folder) / games.select(game_id).executable).unlink()
                result = subprocess.run(["bash", str(folder / "tools/launch-msfs.sh")],
                                        env=environment, capture_output=True, text=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(capture.exists())


if __name__ == "__main__":
    unittest.main()
