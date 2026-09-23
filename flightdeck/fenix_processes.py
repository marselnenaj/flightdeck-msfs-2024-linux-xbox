# SPDX-License-Identifier: MIT
"""Inspect and close only named Fenix processes in one selected Wine profile."""
from __future__ import annotations

import os
import ntpath
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
WEBVIEW = "fenix-webview2"
MANAGED = FENIX | {WEBVIEW}
INFRASTRUCTURE = frozenset({"wineserver", "services.exe", "winedevice.exe", "svchost.exe",
                            "plugplay.exe", "rpcss.exe", "explorer.exe", "tabtip.exe", "conhost.exe"})


def process_name(arguments):
    """Shared Edge processes count only when their host/data directory is Fenix's."""
    name = ntpath.basename(arguments[0].strip().strip('"')).casefold()
    if name != "msedgewebview2.exe":
        return name
    flags = {}
    for index, argument in enumerate(arguments[1:], 1):
        key, separator, value = argument.partition("=")
        if not separator and index + 1 < len(arguments):
            value = arguments[index + 1]
        flags[key.casefold()] = value.strip('"')
    host = flags.get("--webview-exe-name", "").casefold()
    directory = ntpath.normpath(flags.get("--user-data-dir", "")).casefold()
    if host == "fenixapp.exe" or directory == r"c:\programdata\fenix\app\webview2\ebwebview":
        return WEBVIEW
    return name


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
                arguments = (proc / "cmdline").read_bytes().split(b"\0")
                name = process_name([os.fsdecode(arg) for arg in arguments])
                self.poller.register(fd, select.POLLIN)
                self.handles[int(proc.name)] = (fd, name)
                fd = None
            except (OSError, ValueError):
                continue
            finally:
                if fd is not None:
                    os.close(fd)

    def live(self, names=MANAGED):
        exited = {fd for fd, _ in self.poller.poll(0)}
        return [(fd, name) for fd, name in self.handles.values() if fd not in exited and (names is None or name in names)]

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

    def finish_server(self):
        # A failed WebView host may leave only Wine services behind. Closing
        # their server also flushes its registry. Never do this while any app
        # (including an unknown add-on) remains in the selected profile.
        self.collect()
        if any(name not in INFRASTRUCTURE for _, name in self.live(None)):
            return
        for fd, _ in self.live({"wineserver"}):
            try:
                # SIGINT is wineserver -k: it closes clients and flushes state.
                signal.pidfd_send_signal(fd, signal.SIGINT)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 3
        while self.live(INFRASTRUCTURE) and time.monotonic() < deadline:
            time.sleep(.05)
            self.collect()


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
            processes.finish_server()
            return
        progress("Fenix wird beendet …")
        wine = root / "runner/files/bin/wine"
        named = sorted({name for _, name in processes.live(FENIX)})
        if wine.is_file() and named:
            args = [str(wine), "taskkill.exe"]
            for name in named:
                args.extend(("/IM", name))
            # Never taskkill /IM msedgewebview2.exe: another add-on may share it.
            # Fenix-owned WebView helpers are signalled through verified pidfds.
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
        processes.finish_server()
