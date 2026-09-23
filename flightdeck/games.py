# SPDX-License-Identifier: MIT
"""Fixed Xbox PC game identities and the edition bound to a prepared runtime."""
from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
import stat


@dataclass(frozen=True)
class Game:
    id: str
    name: str
    store_id: str
    directory: str
    executable: str
    user_config: str


GAMES = {
    "msfs2024": Game("msfs2024", "Microsoft Flight Simulator 2024", "9P38D19T7LRV",
                     "MSFS2024", "FlightSimulator2024.exe", "Microsoft Flight Simulator 2024"),
    "msfs2020": Game("msfs2020", "Microsoft Flight Simulator 2020", "9NRRJLLXM68V",
                     "MSFS2020", "FlightSimulator.exe", "Microsoft Flight Simulator"),
}


def select(game_id):
    if type(game_id) is not str or game_id not in GAMES:
        raise ValueError("Unbekannte MSFS-Version.")
    return GAMES[game_id]


def for_runtime(runtime):
    """Legacy runtimes without a game_id are MSFS 2024; invalid data fails closed."""
    path = Path(runtime) / "private/runtime.json"
    try:
        info = path.lstat()
    except FileNotFoundError:
        return GAMES["msfs2024"]
    except OSError:
        raise ValueError("Ungültige Runtime-Konfiguration.") from None
    if not stat.S_ISREG(info.st_mode) or info.st_size > 65536:
        raise ValueError("Ungültige Runtime-Konfiguration.")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        raise ValueError("Ungültige Runtime-Konfiguration.") from None
    if not isinstance(value, dict):
        raise ValueError("Ungültige Runtime-Konfiguration.")
    return select(value.get("game_id", "msfs2024"))


def path(runtime):
    return Path(runtime) / "games" / for_runtime(runtime).directory
