# SPDX-License-Identifier: MIT
"""Exercise inherited FD routing with a synthetic PE and a fake Wine program."""
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LoaderWrapper(unittest.TestCase):
    def test_external_owned_game_symlink_and_exit_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); runtime = root / "runtime"; game = root / "owned game"
            game.mkdir(); (runtime / "tools").mkdir(parents=True)
            (runtime / "games").mkdir(); (runtime / "games/MSFS2024").symlink_to(game, target_is_directory=True)
            prefix = runtime / "local/msfs-prefix"; prefix.mkdir(parents=True)
            wrapper = runtime / "tools/xodus-wine-launch"
            shutil.copy2(ROOT / "scripts/runtime/xodus-wine-launch", wrapper)
            source = game / "Synthetic.exe"; source.write_bytes(b"encrypted placeholder")
            wine = root / "fake-wine"
            wine.write_text("#!/usr/bin/env python3\nimport os,sys\nfrom pathlib import Path\n"
                            "p=Path(sys.argv[1]); assert p.exists()\n"
                            "entries=os.environ['WINE_DLL_FILE_MAP'].split('|')\n"
                            "assert len(entries)==2\n"
                            "fd=int(entries[0].split(':',1)[0]); assert os.pread(fd,2,0)==b'MZ'\n"
                            "assert p.read_bytes()[:2]==b'MZ'\nraise SystemExit(7)\n")
            wine.chmod(0o700)
            content = bytearray(512); content[:2] = b"MZ"
            struct.pack_into("<I", content, 0x3c, 128); content[128:132] = b"PE\0\0"
            struct.pack_into("<I", content, 128 + 24 + 60, 256)
            fd = os.memfd_create("synthetic-loader")
            try:
                os.write(fd, content)
                ntpath = "\\??\\Z:" + str(source).replace("/", "\\")
                env = dict(os.environ, WINEPREFIX=str(prefix), XODUS_WINE_RUNNER=str(wine),
                           WINE_DLL_FILE_MAP=f"{fd}:{ntpath}")
                result = subprocess.run(["python3", str(wrapper), ntpath], env=env, pass_fds=(fd,),
                                        capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 7, result.stderr.decode())
                self.assertIn(b"exit_code=7", result.stderr)
                self.assertEqual(list(game.iterdir()), [source])
                self.assertEqual(source.read_bytes(), b"encrypted placeholder")
            finally:
                os.close(fd)


if __name__ == "__main__":
    unittest.main()
