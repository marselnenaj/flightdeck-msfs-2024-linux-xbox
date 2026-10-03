#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise real .NET install/repair in a new, account-free Wine profile.

Requires an unmodified supported runner and the two cached, pinned Microsoft
installers. Existing profiles and the supplied cache are never modified.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from flightdeck.fenix import core


def require(value, message):
    if not value:
        raise AssertionError(message)


def run(args):
    runner = args.runner.resolve(strict=True)
    variant = core.runner_variant(runner)
    cached = args.cache.resolve(strict=True)
    work = args.output.resolve()
    require(work.is_relative_to(ROOT / "build") and work != ROOT / "build",
            "Choose a new output directory below build/.")
    require(not work.exists(), "The test output must be a new directory.")
    for name, (_, digest) in core.DOWNLOADS.items():
        require(core.digest(core.regular(cached / name, 256 * 1024 * 1024)) == digest,
                "A required cached Microsoft installer is missing or has the wrong checksum.")
    os.umask(0o077)
    work.mkdir(parents=True, mode=0o700)
    prefix, cache = work / "prefix", work / "cache"
    prefix.mkdir()
    cache.mkdir()
    for name in core.DOWNLOADS:
        shutil.copyfile(cached / name, cache / name)
    print("Isolated evidence:", work, flush=True)
    progress = lambda text: print(text, flush=True)
    with (work / "setup.log").open("xb") as log:
        wine = core.Wine(prefix, runner, log)
        for key in ("CONFIG", "DATA", "CACHE", "STATE"):
            folder = work / ("xdg-" + key.lower())
            folder.mkdir()
            wine.env["XDG_" + key + "_HOME"] = str(folder)
        try:
            wine.run("wineboot", "--init", timeout=180)
            wine.stop()
            progress("Testing fresh .NET setup")
            core.prepare_framework(wine, cache, progress)
            wine.stop()
            require(core.has_framework(prefix), "Fresh .NET setup is incomplete.")
            clr = core._framework_path(prefix, "Framework", "clr.dll")
            expected = core.digest(clr)
            clr.rename(clr.with_name("clr.dll.retained-for-test"))
            require(not core.has_framework(prefix), "A missing x86 CLR was reported as ready.")
            progress("Testing automatic repair of the deliberately missing x86 CLR")
            core.prepare_framework(wine, cache, progress)
            wine.stop()
            require(core.has_framework(prefix), "Automatic repair is incomplete.")
            require(core.digest(clr) == expected, "The restored CLR differs from the original.")
            progress("Testing the next start of the repaired profile")
            messages = []
            core.prepare_framework(wine, cache, lambda text: (messages.append(text), progress(text)))
            wine.stop()
            require(messages == ["Microsoft .NET Framework 4.8 is already installed."],
                    "The healthy profile entered installation or repair again.")
            report = {"passed": True, "account_calls": False, "runner_variant": variant or "flightdeck",
                      "fresh_install": True, "missing_x86_clr_repaired": True, "same_clr_hash": True,
                      "idempotent_restart": True, "status": core.framework_status(prefix)}
            core.write_json(work / "result.json", report)
            print(json.dumps(report), flush=True)
        finally:
            wine.stop()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
