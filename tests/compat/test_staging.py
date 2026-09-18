"""No Wine, credentials, downloads or real packages are used by these tests."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from flightdeck.setup import RUNTIME_FILES

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("stage_runtime", ROOT / "scripts/stage-runtime.py")
stage_runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage_runtime)


class RuntimeStaging(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.previous = stage_runtime.REPO
        stage_runtime.REPO = self.root / "source"
        repo = stage_runtime.REPO
        (repo / "scripts/runtime").mkdir(parents=True)
        for name in RUNTIME_FILES:
            (repo / "scripts/runtime" / name).write_text("#!/bin/sh\nexit 0\n")
        (repo / "compat").mkdir()
        original = b"synthetic original; not executable"
        (repo / "compat/upstreams.lock.json").write_text(json.dumps({"runner": {
            "original_runtime_sha256": hashlib.sha256(original).hexdigest()}}))
        self.args = argparse.Namespace(**{name: self.root/name for name in
            ("artifacts", "game", "runner", "prefix", "destination")},
            market="AT", local_saves=False, media_plugins=None)
        required = [self.args.runner / "files/bin/wine", self.args.prefix / "system.reg",
                    self.args.prefix / "user.reg"]
        required += [self.args.game / name for name in
                     ("FlightSimulator2024.exe", ".xodus-streaming.msixvc", "MicrosoftGame.Config")]
        for path in required:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic fixture")
            if path.name == "wine":
                path.chmod(0o700)
        self.original = self.args.runner / "files/lib/wine/x86_64-windows/xgameruntime.dll"
        self.original.parent.mkdir(parents=True); self.original.write_bytes(original)
        self.system32 = self.args.prefix / "drive_c/windows/system32"
        self.system32.mkdir(parents=True)
        (self.system32 / "xgameruntime.dll").symlink_to(self.original)
        files = {}
        for relative in stage_runtime.ARTIFACTS:
            path = self.args.artifacts / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(("synthetic artifact " + relative).encode())
            if relative.startswith("bin/"):
                path.chmod(0o700)
            files[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
        (self.args.artifacts / "manifest.json").write_text(json.dumps({"files": files}))

    def tearDown(self):
        stage_runtime.REPO = self.previous
        self.tmp.cleanup()

    def test_copy_is_independent_and_default_saves_off(self):
        before = self.original.read_bytes()
        stage_runtime.stage(self.args)
        dest = self.args.destination
        self.assertEqual(self.original.read_bytes(), before)
        self.assertTrue((self.system32 / "xgameruntime.dll").is_symlink())
        installed = dest / "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"
        self.assertFalse(installed.is_symlink())
        self.assertEqual(installed.read_bytes(), (self.args.artifacts / "runtime/xgameruntime.dll").read_bytes())
        self.assertFalse((dest / "private/local-saves.enabled").exists())
        self.assertEqual((dest / "games/MSFS2024").resolve(), self.args.game)

    def test_local_saves_require_explicit_option(self):
        self.args.local_saves = True
        stage_runtime.stage(self.args)
        gate = self.args.destination / "private/local-saves.enabled"
        self.assertEqual(gate.stat().st_mode & 0o777, 0o600)
        self.assertEqual(gate.parent.joinpath("local-saves").stat().st_mode & 0o777, 0o700)

    def test_destination_never_overwritten(self):
        self.args.destination.mkdir()
        sentinel = self.args.destination / "keep"
        sentinel.write_text("untouched")
        with self.assertRaisesRegex(ValueError, "Zielordner existiert"):
            stage_runtime.stage(self.args)
        self.assertEqual(sentinel.read_text(), "untouched")

    def test_hash_mismatch_stops_before_creation(self):
        (self.args.artifacts / "runtime/xgameruntime.dll").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "Prüfsumme"):
            stage_runtime.stage(self.args)
        self.assertFalse(self.args.destination.exists())

    def test_directory_symlink_cannot_redirect_install(self):
        (self.system32 / "xgameruntime.dll").unlink()
        self.system32.rmdir()
        self.system32.symlink_to(self.original.parent, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "echte Ordner"):
            stage_runtime.stage(self.args)
        self.assertFalse(self.args.destination.exists())

    def test_market_and_missing_game_rejected(self):
        self.args.market = "AT;bad"
        with self.assertRaisesRegex(ValueError, "Ländercode"):
            stage_runtime.stage(self.args)
        self.args.market = "AT"
        (self.args.game / ".xodus-streaming.msixvc").unlink()
        with self.assertRaisesRegex(ValueError, "fehlt"):
            stage_runtime.stage(self.args)
        self.assertFalse(self.args.destination.exists())


if __name__ == "__main__":
    unittest.main()
