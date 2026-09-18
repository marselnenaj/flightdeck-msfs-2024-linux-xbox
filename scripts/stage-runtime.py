#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Prepare a new local runtime from built components and user-owned inputs."""
import argparse
from pathlib import Path
import sys

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))
from flightdeck.setup import ARTIFACTS, SetupError, preflight, prepare


def stage(args):
    data = {name + "_path": str(getattr(args, name).expanduser().absolute()) for name in
            ("artifacts", "game", "runner", "prefix", "destination")}
    data.update(mode="prepare", market=args.market, local_saves=args.local_saves,
                media_plugins_path=str(args.media_plugins.expanduser().absolute()) if args.media_plugins else "")
    plan = preflight(data, source_root=REPO)
    result = prepare(plan)
    print("Lokale Runtime vorbereitet. Keine Anmeldung und kein Spielstart ausgeführt.")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("artifacts", "game", "runner", "prefix", "destination"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--market", required=True)
    parser.add_argument("--local-saves", action="store_true", help="Enable local-only saves; no cloud sync")
    parser.add_argument("--media-plugins", type=Path, help="Optional user-prepared GStreamer plugin directory")
    try:
        stage(parser.parse_args())
    except (SetupError, OSError) as error:
        parser.exit(1, str(error) + "\n")
