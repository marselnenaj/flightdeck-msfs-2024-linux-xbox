# SPDX-License-Identifier: MIT
"""Refresh hash-identified native files and scripts in an idle prepared runtime.

The game's files, Wine user data, saves, Store account and runner are never
rewritten. A small journal restores the old component set after interruption.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import stat
import uuid

from . import bootstrap
from .backend import atomic_json
from .setup import RUNTIME_FILES, _copy_file, digest, resource_paths


class ComponentUpdateError(ValueError):
    pass


class CustomScripts(ComponentUpdateError):
    pass


FILES = (
    "bin/xodus-cli", "bin/xodus-service", "bin/flightdeck-connected-storage.exe",
    "runtime/xgameruntime.dll", "builtin/x86_64-windows/xodus_store_test.dll",
    "builtin/x86_64-unix/xodus_store_test.so",
)
HEX = re.compile(r"[a-f0-9]{64}\Z")
JOURNAL = "component-update.json"
MANIFEST = "import-manifest.json"


def _targets(root):
    system32 = root / "local/msfs-prefix/drive_c/windows/system32"
    return {
        "bin/xodus-cli": (root / "bin/xodus-cli",),
        "bin/xodus-service": (root / "bin/xodus-service",),
        "bin/flightdeck-connected-storage.exe": (root / "bin/flightdeck-connected-storage.exe",),
        "runtime/xgameruntime.dll": (system32 / "xgameruntime.dll",),
        "builtin/x86_64-windows/xodus_store_test.dll": (
            root / "local/store-runtime/x86_64-windows/xodus_store_test.dll",
            system32 / "xodus_store_test.dll",
        ),
        "builtin/x86_64-unix/xodus_store_test.so": (
            root / "local/store-runtime/x86_64-unix/xodus_store_test.so",),
    }


def _regular(root, path):
    if not path.is_relative_to(root):
        raise ComponentUpdateError("Der Runtime-Pfad ist ungültig.")
    for parent in (root, *reversed(path.relative_to(root).parents)):
        candidate = root / parent if not parent.is_absolute() else parent
        if candidate == path:
            continue
        if candidate.is_symlink():
            raise ComponentUpdateError("Die Runtime enthält verknüpfte Komponentenpfade.")
    try:
        info = path.lstat()
    except OSError:
        raise ComponentUpdateError("Eine Runtime-Komponente fehlt.") from None
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
        raise ComponentUpdateError("Eine Runtime-Komponente ist keine eigene reguläre Datei.")


def _hashes(value):
    if not isinstance(value, dict) or set(value) != set(FILES) or any(
        not isinstance(v, str) or not HEX.fullmatch(v) for v in value.values()
    ):
        raise ComponentUpdateError("Die Runtime hat keine vollständig geprüfte Komponentenliste.")
    return value


def _script_hashes(value):
    if (not isinstance(value, dict) or set(value) != set(RUNTIME_FILES)
            or any(not isinstance(v, str) or not HEX.fullmatch(v) for v in value.values())):
        raise ComponentUpdateError("Die Runtime hat keine vollständig geprüfte Skriptliste.")
    return value


def _scripts(root, record, lock):
    specification = lock.get("runtime_scripts")
    if specification is None:
        return {}, {}  # Older releases have the native-only journal contract.
    if not isinstance(specification, dict):
        raise ComponentUpdateError("Die Runtime-Skriptbeschreibung ist ungültig.")
    current = _script_hashes(specification.get("files"))
    previous = specification.get("upgrade_from", [])
    if not isinstance(previous, list):
        raise ComponentUpdateError("Die Runtime-Skriptbeschreibung ist ungültig.")
    previous = [_script_hashes(value) for value in previous]
    actual = {}
    for name in RUNTIME_FILES:
        path = root / "tools" / name
        _regular(root, path)
        if not path.stat().st_mode & stat.S_IXUSR:
            raise ComponentUpdateError("Ein Runtime-Skript ist nicht ausführbar.")
        actual[name] = digest(path)
    recorded = record.get("runtime_files")
    if recorded is not None and _script_hashes(recorded) != actual:
        raise CustomScripts("Ein Runtime-Skript wurde verändert. Es wird nichts überschrieben.")
    # Older import manifests did not record scripts. Migrate only an exact set
    # of scripts shipped in a pinned release, never arbitrary existing files.
    if actual != current and actual not in previous:
        raise CustomScripts("Diese Runtime verwendet eigene oder unbekannte Startskripte. Es wird nichts überschrieben.")
    return actual, current


def _verify_scripts(root, hashes, *, backup=False):
    for name, expected in hashes.items():
        path = root / ("script-" + name) if backup else root / "tools" / name
        _regular(root, path)
        if digest(path) != expected or not path.stat().st_mode & stat.S_IXUSR:
            raise ComponentUpdateError("Ein Runtime-Skript wurde verändert. Es wird nichts überschrieben.")


def update_state(root):
    """Recognize pinned releases without replacing a user's custom native build."""
    if root is None:
        return "unmanaged"
    private = root / "private"
    journal = private / JOURNAL
    if journal.exists() or journal.is_symlink():
        return "interrupted"
    manifest = private / MANIFEST
    if not manifest.exists() and not manifest.is_symlink():
        return "unmanaged"
    try:
        _regular(root, manifest)
        record = json.loads(manifest.read_text())
        if not isinstance(record, dict) or record.get("format") != 1:
            return "invalid"
        installed = record["artifacts"]["files"]
        if not isinstance(installed, dict) or not installed or any(
            not isinstance(name, str) or not isinstance(value, str) or not HEX.fullmatch(value)
            for name, value in installed.items()
        ):
            return "invalid"
        _, lockfile = bootstrap.paths()
        lock = json.loads(lockfile.read_text())
        current = _hashes(lock["native"]["files"])
        upgrades = lock["native"].get("upgrade_from", [])
        if not isinstance(upgrades, list):
            return "invalid"
        for value in upgrades:
            _hashes(value)
        if installed != current and installed not in upgrades:
            return "custom"
        old_scripts, new_scripts = _scripts(root, record, lock)
        return "current" if installed == current and old_scripts == new_scripts else "pending"
    except CustomScripts:
        return "custom"
    except (ComponentUpdateError, OSError, ValueError, KeyError, TypeError):
        return "invalid"


def refresh_on_startup(launcher):
    """Apply a bundled update before serving the UI when no game owns the runtime."""
    state = update_state(launcher.runtime)
    if state in {"pending", "interrupted"}:
        return refresh(launcher)
    if state == "invalid":
        raise ComponentUpdateError("Die Runtime-Komponentenbeschreibung ist ungültig.")
    return False


def _read_manifest(path, root):
    _regular(root, path)
    try:
        value = json.loads(path.read_text())
        old = _hashes(value["artifacts"]["files"])
        if value.get("format") != 1:
            raise ValueError()
        return value, old
    except (OSError, ValueError, KeyError, TypeError):
        raise ComponentUpdateError("Die Runtime-Komponentenbeschreibung ist ungültig.") from None


def _verify_targets(root, hashes):
    for name, paths in _targets(root).items():
        for path in paths:
            _regular(root, path)
            if digest(path) != hashes[name]:
                raise ComponentUpdateError("Eine Runtime-Komponente wurde verändert. Es wird nichts überschrieben.")


def _backup_name(name, index):
    return f"{list(FILES).index(name)}-{index}"


def _copy_verified(source, destination, expected):
    _copy_file(source, destination)
    if digest(destination) != expected:
        raise ComponentUpdateError("Eine Komponente wurde während der Kopie verändert.")


def _cleanup(private, backup):
    if backup.is_dir() and not backup.is_symlink():
        shutil.rmtree(backup)
    (private / JOURNAL).unlink(missing_ok=True)


def _recover(root):
    private = root / "private"
    journal_path = private / JOURNAL
    if not journal_path.exists() and not journal_path.is_symlink():
        return
    _regular(root, journal_path)
    try:
        journal = json.loads(journal_path.read_text())
        backup_id = journal["backup"]
        if journal.get("format") not in (1, 2) or not isinstance(backup_id, str) or not re.fullmatch(r"\.component-[a-f0-9]{32}", backup_id):
            raise ValueError()
        before, after = _hashes(journal["before"]), _hashes(journal["after"])
        before_scripts = _script_hashes(journal.get("scripts_before")) if journal["format"] == 2 else {}
        after_scripts = _script_hashes(journal.get("scripts_after")) if journal["format"] == 2 else {}
        manifest_hash = journal["manifest_sha256"]
        if not isinstance(manifest_hash, str) or not HEX.fullmatch(manifest_hash):
            raise ValueError()
    except (ValueError, KeyError, TypeError):
        raise ComponentUpdateError("Das unterbrochene Komponentenupdate hat ungültige Metadaten.") from None
    backup = private / backup_id
    manifest = private / MANIFEST
    try:
        current, hashes = _read_manifest(manifest, root)
        if hashes == after and (not after_scripts or current.get("runtime_files") == after_scripts):
            _verify_targets(root, after)
            _verify_scripts(root, after_scripts)
            _cleanup(private, backup)
            return
    except ComponentUpdateError:
        pass
    if backup.is_symlink() or not backup.is_dir():
        raise ComponentUpdateError("Die Sicherung des Komponentenupdates fehlt.")
    saved_manifest = backup / MANIFEST
    _regular(root, saved_manifest)
    if digest(saved_manifest) != manifest_hash:
        raise ComponentUpdateError("Die Sicherung des Komponentenupdates wurde verändert.")
    _verify_scripts(backup, before_scripts, backup=True)
    for name in before_scripts:
        _regular(root, root / "tools" / name)
    for name, paths in _targets(root).items():
        for index, path in enumerate(paths):
            saved = backup / _backup_name(name, index)
            _regular(root, saved)
            if digest(saved) != before[name]:
                raise ComponentUpdateError("Die Sicherung des Komponentenupdates wurde verändert.")
            _regular(root, path)
    for name, paths in _targets(root).items():
        for index, path in enumerate(paths):
            _copy_verified(backup / _backup_name(name, index), path, before[name])
    for name, expected in before_scripts.items():
        _copy_verified(backup / ("script-" + name), root / "tools" / name, expected)
    _copy_verified(saved_manifest, manifest, manifest_hash)
    _cleanup(private, backup)


def refresh(launcher):
    """Apply the installed bundle, returning True only when bytes changed."""
    with launcher.lock:
        launcher.require_open()
        launcher._poll()
        if launcher.runtime is None or launcher.setup_busy or launcher.process is not None or launcher.managed_session is not None:
            raise ComponentUpdateError("Das Spiel und die Einrichtung müssen für das Komponentenupdate beendet sein.")
        root = launcher.runtime
        with launcher.runtime_lock(operation="components"):
            private = root / "private"
            if private.is_symlink() or not private.is_dir() or private.stat().st_uid != os.getuid():
                raise ComponentUpdateError("Der private Runtimeordner ist ungültig.")
            _recover(root)
            source, lockfile = bootstrap.paths()
            try:
                lock = json.loads(lockfile.read_text())
                new = _hashes(lock["native"]["files"])
                upgrade_from = lock["native"].get("upgrade_from", [])
                if not isinstance(upgrade_from, list):
                    raise ValueError()
                upgrade_from = [_hashes(value) for value in upgrade_from]
                native = bootstrap.native_path(source, lock)
                if native is None:
                    raise ValueError()
                bootstrap.verify_native(native, lock)
            except (OSError, ValueError, KeyError, TypeError):
                raise ComponentUpdateError("Das geprüfte neue Kompatibilitätspaket fehlt. Bitte den vollständigen Launcher installieren.") from None
            manifest = private / MANIFEST
            if not manifest.exists() and not manifest.is_symlink():
                raise ComponentUpdateError("Diese ältere Runtime hat keine verwaltete Komponentenliste. Bitte eine neue Runtime über die Einrichtung anlegen und vorhandene Spieldateien dort importieren.")
            record, old = _read_manifest(manifest, root)
            _verify_targets(root, old)
            old_scripts, new_scripts = _scripts(root, record, lock)
            scripts, _ = resource_paths()
            for name, expected in new_scripts.items():
                path = scripts / name
                _regular(scripts, path)
                if digest(path) != expected:
                    raise ComponentUpdateError("Ein neues Runtime-Skript stimmt nicht mit dem geprüften Launcher überein.")
            if old == new and old_scripts == new_scripts:
                return False
            if old != new and old not in upgrade_from:
                raise ComponentUpdateError("Diese Runtime verwendet eigene oder unbekannte Komponenten. Das automatische Update ist dafür nicht freigegeben.")
            journal_path = private / JOURNAL
            if journal_path.exists() or journal_path.is_symlink():
                raise ComponentUpdateError("Ein Komponentenupdate ist noch nicht abgeschlossen.")
            backup = private / (".component-" + uuid.uuid4().hex)
            backup.mkdir(mode=0o700)
            started = False
            try:
                for name, paths in _targets(root).items():
                    for index, path in enumerate(paths):
                        _copy_verified(path, backup / _backup_name(name, index), old[name])
                for name, expected in old_scripts.items():
                    _copy_verified(root / "tools" / name, backup / ("script-" + name), expected)
                _copy_verified(manifest, backup / MANIFEST, digest(manifest))
                journal = {"format": 1, "backup": backup.name, "before": old, "after": new,
                           "manifest_sha256": digest(manifest)}
                if new_scripts:
                    journal.update(format=2, scripts_before=old_scripts, scripts_after=new_scripts)
                atomic_json(journal_path, journal)
                started = True
                for name, paths in _targets(root).items():
                    for path in paths:
                        _copy_verified(native / name, path, new[name])
                for name, expected in new_scripts.items():
                    _copy_verified(scripts / name, root / "tools" / name, expected)
                    # Installed launcher resources are data files (0644), as
                    # in setup. Only the verified runtime copy is executable.
                    (root / "tools" / name).chmod(0o700)
                updated = dict(record)
                updated["artifacts"] = {"format": 1, "files": new,
                    "features": lock["native"].get("features", []),
                    "cli_features": lock["native"].get("cli_features", []),
                    "native_archive_sha256": lock["native"].get("archive_sha256")}
                if new_scripts:
                    updated["runtime_files"] = new_scripts
                atomic_json(manifest, updated)
                _verify_targets(root, new)
                _verify_scripts(root, new_scripts)
                _cleanup(private, backup)
                return True
            except Exception:
                if started:
                    _recover(root)
                elif backup.is_dir() and not backup.is_symlink():
                    shutil.rmtree(backup)
                raise
