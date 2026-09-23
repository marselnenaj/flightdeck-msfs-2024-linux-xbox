#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Explicit maintainer import from the matching, locally built Fenix project."""
import argparse
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("project", type=Path)
    args = parser.parse_args()
    project = args.project.resolve(strict=True)
    bundle = json.loads((project / "bundle.json").read_text())
    release = json.loads((project / "dist/flightdeck-release.json").read_text())
    if bundle["version"] != release["version"]:
        raise ValueError("Release and installer versions differ")
    for name in ("core.py", "__init__.py"):
        shutil.copy2(project / "fenix_patch" / name, ROOT / "flightdeck/_fenix" / name)
    shutil.copy2(project / "bundle.json", ROOT / "compat/fenix/bundle.json")
    shutil.copy2(project / "dist/flightdeck-release.json", ROOT / "compat/fenix/release.json")
    shutil.copy2(project / "LICENSE", ROOT / "compat/fenix/LICENSE")
    print("Imported Fenix", bundle["version"], "— review manifests and run Fenix tests before release.")


if __name__ == "__main__": main()
