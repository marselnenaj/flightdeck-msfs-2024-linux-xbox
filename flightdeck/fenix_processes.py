# SPDX-License-Identifier: MIT
"""Inspect and close only named Fenix processes in one selected Wine profile."""
from __future__ import annotations

import os
from pathlib import Path
import select
import signal
import subprocess
import time

from .backend import LauncherError
from ._fenix import core

FENIX = frozenset({"fenix.exe", "fenixapp.exe", "fenixbootstrapper.exe", "fenixsystem.exe",
                   "fenixdisplay.exe", "fenixcdu.exe", "fenixwizzard.exe",
                   "fenix.gqlgateway.exe", "fenixwindowguard.exe"})
GAMES = frozenset({"flightsimulator.exe", "flightsimulator2024.exe"})


class Processes:
    def __init__(self, root):
        self.prefix = (Path(root) / "local/msfs-prefix").resolve()
        self.handles = {}
        self.poller = select.poll()

    def __enter__(self):
        self.collect()
        return self

    def __exit__(self, *_):
        for fd, _ in self.handles.values():
            os.close(fd)

    def collect(self):
        for proc in Path("/proc").iterdir():
            if not proc.name.isdecimal() or int(proc.name) in self.handles:
                continue
            fd = None
            try:
                if proc.stat().st_uid != os.getuid():
                    continue
                fd = os.pidfd_open(int(proc.name))
                values = (proc / "environ").read_bytes().split(b"\0")
                prefix = next((v[11:] for v in values if v.startswith(b"WINEPREFIX=")), None)
                if prefix is None or Path(os.fsdecode(prefix)).resolve() != self.prefix:
                    continue
                command = (proc / "cmdline").read_bytes().split(b"\0", 1)[0]
                name = os.fsdecode(command).strip().strip('"').replace("\\", "/").rsplit("/", 1)[-1].casefold()
                if name not in FENIX | GAMES:
                    continue
                self.poller.register(fd, select.POLLIN)
                self.handles[int(proc.name)] = (fd, name)
                fd = None
            except (OSError, ValueError):
                continue
            finally:
                if fd is not None:
                    os.close(fd)

    def live(self, names=FENIX):
        exited = {fd for fd, _ in self.poller.poll(0)}
        return [(fd, name) for fd, name in self.handles.values() if fd not in exited and name in names]

    def check_game(self):
        if self.live(GAMES):
            raise LauncherError("Beende MSFS, bevor du Fenix schließt.")

    def wait(self, seconds):
        deadline = time.monotonic() + seconds
        while self.live() and time.monotonic() < deadline:
            time.sleep(.05)
            self.collect()
            self.check_game()

    def send(self, number):
        self.collect()
        self.check_game()
        for fd, _ in self.live():
            try:
                signal.pidfd_send_signal(fd, number)
            except ProcessLookupError:
                pass


def status(root):
    if root is None:
        return False, False
    with Processes(root) as processes:
        return bool(processes.live()), bool(processes.live(GAMES))


def stop(root, progress=lambda _: None):
    """Caller retains the runtime reservation until this bounded shutdown ends."""
    root = Path(root)
    with Processes(root) as processes:
        processes.check_game()
        if not processes.live():
            return
        progress("Fenix wird beendet …")
        wine = root / "runner/files/bin/wine"
        if wine.is_file():
            args = [str(wine), "taskkill.exe"]
            for name in sorted({name for _, name in processes.live()}):
                args.extend(("/IM", name))
            # Without /F Wine posts WM_CLOSE, allowing normal settings writes.
            try:
                subprocess.run(args, env=core.wine_env(processes.prefix, root / "runner"),
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    close_fds=True, timeout=3, check=False)
            except (OSError, subprocess.TimeoutExpired):
                pass
        processes.wait(2)
        processes.send(signal.SIGTERM)
        processes.wait(1)
        processes.send(signal.SIGKILL)
        processes.wait(1)
        if processes.live():
            raise LauncherError("Fenix konnte nicht vollständig beendet werden.")
