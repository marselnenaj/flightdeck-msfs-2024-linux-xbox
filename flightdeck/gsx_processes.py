# SPDX-License-Identifier: MIT
"""FSDT manager lifecycle, scoped by Wine prefix and pinned Linux pidfds."""
import signal
import subprocess

from .backend import LauncherError
from .fenix_processes import Processes as ProfileProcesses, GAMES
from .gsx_core import wine_environment

MANAGERS = frozenset({"couatl_updater.exe", "couatl_updater2.exe", "qlmlicensewizard.exe"})


class Processes(ProfileProcesses):
    def live(self, names=MANAGERS):
        return super().live(names)

    def check_game(self):
        if self.live(GAMES):
            raise LauncherError("Beende MSFS, bevor du den FSDT-Installer schließt.")


def status(root):
    if root is None:
        return False, False
    with Processes(root) as processes:
        return bool(processes.live()), bool(processes.live(GAMES))


def stop(root):
    with Processes(root) as processes:
        processes.check_game()
        if processes.live():
            runner = (root / "runner").resolve()
            arguments = [str(runner / "files/bin/wine"), "taskkill.exe"]
            for name in sorted({name for _, name in processes.live()}):
                arguments.extend(("/IM", name))
            try:
                subprocess.run(arguments, env=wine_environment(root / "local/msfs-prefix", runner),
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               timeout=3, check=False)
            except (OSError, subprocess.TimeoutExpired):
                pass
            processes.wait(3)
            processes.send(signal.SIGTERM)
            processes.wait(1)
            processes.send(signal.SIGKILL)
            processes.wait(1)
        if processes.live():
            raise LauncherError("Der FSDT-Installer konnte nicht vollständig geschlossen werden.")
        processes.finish_server()
