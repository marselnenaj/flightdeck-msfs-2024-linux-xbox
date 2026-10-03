# SPDX-License-Identifier: MIT
"""Exercise inherited FD routing with a synthetic PE and a fake Wine program."""
import os
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LoaderWrapper(unittest.TestCase):
    def launch_case(self, game_id="msfs2024", portable=False, arguments=(), setting=None, expected=("-FastLaunch",)):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); runtime = root / "runtime"; game = root / "owned game"
            game.mkdir(); (runtime / "tools").mkdir(parents=True)
            directory, executable = {"msfs2020": ("MSFS2020", "FlightSimulator.exe"),
                                     "msfs2024": ("MSFS2024", "FlightSimulator2024.exe")}[game_id]
            (runtime / "games").mkdir(); (runtime / "games" / directory).symlink_to(game, target_is_directory=True)
            (runtime / "private").mkdir()
            (runtime / "private/runtime.json").write_text(json.dumps({"game_id": game_id}))
            prefix = runtime / "local/msfs-prefix"; prefix.mkdir(parents=True)
            wrapper = runtime / "tools/xodus-wine-launch"
            shutil.copy2(ROOT / "scripts/runtime/xodus-wine-launch", wrapper)
            source = game / executable; source.write_bytes(b"encrypted placeholder")
            (game / "nested").mkdir()
            library = game / "nested/probe.dll"; library.write_bytes(b"encrypted DLL placeholder")
            (game / "asset.dat").write_bytes(b"ordinary resource")
            wine = root / "fake-wine"
            wine.write_text("#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\n"
                            "p=Path(sys.argv[1]); assert p.exists()\n"
                            "assert sys.argv[2:]==json.loads(os.environ['FLIGHTDECK_EXPECT_ARGUMENTS']), sys.argv\n"
                            "assert os.environ['XODUS_STORE_PACKAGE_SCOPE']=='FlightdeckBaseGameOnlyV1'\n"
                            "assert Path.cwd()==p.parent\n"
                            "assert (Path.cwd()/'nested/probe.dll').read_bytes()==b'memory-backed DLL'\n"
                            "assert (p.parent/'asset.dat').read_bytes()==b'ordinary resource'\n"
                            "if os.environ.get('FLIGHTDECK_PROTON_LOADER')=='portable':\n"
                            " assert 'WINE_DLL_FILE_MAP' not in os.environ\n"
                            " assert p.is_symlink()\n"
                            "else:\n"
                            " entries=os.environ['WINE_DLL_FILE_MAP'].split('|')\n"
                            " assert len(entries)==4\n"
                            " assert not p.is_symlink() and p.read_bytes()[256:]==bytes(256)\n"
                            " fd=int(entries[0].split(':',1)[0]); assert os.pread(fd,2,0)==b'MZ'\n"
                            "assert p.read_bytes()[:2]==b'MZ'\nraise SystemExit(7)\n")
            wine.chmod(0o700)
            content = bytearray(512); content[:2] = b"MZ"
            struct.pack_into("<I", content, 0x3c, 128); content[128:132] = b"PE\0\0"
            struct.pack_into("<I", content, 128 + 24 + 60, 256)
            content[500:] = b"only-in-ram!"
            fd = os.memfd_create("synthetic-loader")
            dll_fd = os.memfd_create("synthetic-dll")
            try:
                os.write(fd, content)
                os.write(dll_fd, b"memory-backed DLL")
                ntpath = "\\??\\Z:" + str(source).replace("/", "\\")
                dll_path = "\\??\\Z:" + str(library).replace("/", "\\")
                env = dict(os.environ, WINEPREFIX=str(prefix), XODUS_WINE_RUNNER=str(wine),
                           WINE_DLL_FILE_MAP=f"{fd}:{ntpath}|{dll_fd}:{dll_path}", FLIGHTDECK_EXPECT_ARGUMENTS=json.dumps(expected),
                           FLIGHTDECK_PROTON_LOADER="portable" if portable else "native")
                env.pop("FLIGHTDECK_FAST_LAUNCH", None)
                if setting is not None:
                    env["FLIGHTDECK_FAST_LAUNCH"] = setting
                binary = env.get("FLIGHTDECK_TEST_BINARY")
                command = ([str(Path(binary).resolve()), "wine-launch", "--runtime", str(runtime), "--", ntpath]
                           if binary else ["python3", str(wrapper), ntpath])
                result = subprocess.run([*command, *arguments], env=env, pass_fds=(fd,dll_fd),
                                        capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 7, result.stderr.decode())
                self.assertIn(b"exit_code=7", result.stderr)
                self.assertEqual(set(game.iterdir()), {source, game / "nested", game / "asset.dat"})
                self.assertEqual(source.read_bytes(), b"encrypted placeholder")
                self.assertEqual(library.read_bytes(), b"encrypted DLL placeholder")
                self.assertEqual((game / "asset.dat").read_bytes(), b"ordinary resource")
            finally:
                os.close(fd)
                os.close(dll_fd)

    def test_external_owned_game_symlink_and_exit_status(self):
        self.launch_case()

    def test_fastlaunch_reaches_both_games_through_both_loader_modes(self):
        arguments = ("--fixture", "space ; $(not-a-command) ü")
        for game in ("msfs2020", "msfs2024"):
            for portable in (False, True):
                with self.subTest(game=game, portable=portable):
                    self.launch_case(game, portable, arguments, expected=("-FastLaunch", *arguments))

    def test_explicit_argument_is_not_duplicated_or_rewritten(self):
        for portable in (False, True):
            arguments = ("--fixture", "-fAsTlAuNcH", "original argument")
            self.launch_case(portable=portable, arguments=arguments, expected=arguments)

    def test_full_intro_opt_out_preserves_explicit_game_arguments(self):
        for portable in (False, True):
            self.launch_case(portable=portable, arguments=("--fixture",), setting="0", expected=("--fixture",))
            self.launch_case(portable=portable, arguments=("-FastLaunch",), setting="0", expected=("-FastLaunch",))


if __name__ == "__main__":
    unittest.main()
