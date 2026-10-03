# SPDX-License-Identifier: MIT
"""Select hash-pinned launcher scripts independently of the Wine overlay release.

Fenix archives retain their original hashes. Deploying an older overlay must
not reinstall an older loader over fixes shipped by the current launcher.
"""
from __future__ import annotations

import json
import re

from .setup import SetupError, digest, resource_paths

LAUNCH_FILES = ("launch-msfs.sh", "xodus-wine-launch")


def specification():
    from .bootstrap import paths
    _, lockfile = paths()
    value = json.loads(lockfile.read_text())["runtime_scripts"]
    if not isinstance(value, dict) or not isinstance(value.get("files"), dict) or not isinstance(value.get("upgrade_from", []), list):
        raise SetupError("Die Runtime-Skriptbeschreibung ist ungültig.")
    for item in [value["files"], *value.get("upgrade_from", [])]:
        if not isinstance(item, dict) or any(not isinstance(v, str) or not re.fullmatch(r"[a-f0-9]{64}", v) for v in item.values()):
            raise SetupError("Die Runtime-Skriptbeschreibung ist ungültig.")
    return value


def accepted(name):
    if name not in LAUNCH_FILES:
        raise SetupError("Unbekanntes Startskript.")
    spec = specification()
    return {entry[name] for entry in [spec["files"], *spec.get("upgrade_from", [])] if name in entry}


def current(name):
    if name not in LAUNCH_FILES:
        raise SetupError("Unbekanntes Startskript.")
    source, _ = resource_paths()
    path = source / name
    expected = specification()["files"].get(name)
    if path.is_symlink() or not path.is_file() or digest(path) != expected:
        raise SetupError("Ein neues Runtime-Skript stimmt nicht mit dem geprüften Launcher überein.")
    return path


def overlay_set(actual, releases):
    """Recognize known Fenix launch-script substitutions in a released set."""
    from .fenix import core
    overlay = core.manifest()
    known = {}
    for name in LAUNCH_FILES:
        known[name] = {overlay["integration"][name], *overlay["accepted_scripts"][name]}
        known[name].update(item["integration"][name] for item in overlay.get("previous_releases", {}).values())
    return any(set(actual) == set(release) and all(
        actual[name] == value or name in known and actual[name] in known[name]
        for name, value in release.items()) for release in releases)
