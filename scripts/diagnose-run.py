#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Export safe evidence from an existing Flightdeck run, without starting Wine."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
if (ROOT / "flightdeck").is_dir():
    sys.path.insert(0, str(ROOT))

from flightdeck import graphics_diagnostics, run_diagnostics, store_diagnostics


def latest_run(runtime):
    private = runtime / "private"
    if private.is_symlink() or not private.is_dir():
        raise ValueError()
    runs = [p for p in private.iterdir() if re.fullmatch(r"run-\d{8}-\d{6}-[A-Za-z0-9]+", p.name)
            and not p.is_symlink() and p.is_dir()]
    if not runs:
        raise ValueError()
    return max(runs, key=lambda p: p.name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--run", type=Path, help="Existing private/run-* directory")
    source.add_argument("--runtime", type=Path, help="Installation whose latest run should be read")
    parser.add_argument("--output", type=Path, help="New JSON file (defaults to stdout)")
    args = parser.parse_args()
    try:
        runtime = args.runtime
        if args.run is not None:
            run = args.run.expanduser()
        else:
            if runtime is None:
                state = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "flightdeck"
                config = json.loads(graphics_diagnostics._read(state / "config.json", 65536))
                runtime = Path(config["runtime_path"])
            run = latest_run(runtime.expanduser())
        if run.is_symlink() or not run.is_dir():
            raise ValueError()
        data, info = run_diagnostics.read(run / "game.log")
        data["graphics"] = {"log": data.pop("graphics_log")}
        data["store_session"] = store_diagnostics.session(run)
        data["context"] = {"diagnostics_schema": 5, "tool": "flightdeck-run-log-diagnostics-v1",
                           "run_log_modified_at": datetime.fromtimestamp(info.st_mtime, timezone.utc).isoformat(),
                           "cloud_sync_scope": "not_collected"}
        report = {"summary": data, "generated_at": datetime.now(timezone.utc).isoformat()}
        encoded = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
        if args.output:
            fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                stream.write(encoded)
        else:
            print(encoded, end="")
        return 0
    except (OSError, ValueError, KeyError, TypeError):
        print("Could not read the run or create the output. Use --runtime or --run and a new output filename.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
