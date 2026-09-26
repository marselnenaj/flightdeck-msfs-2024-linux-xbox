# SPDX-License-Identifier: MIT
"""Reviewed, edition-scoped removal and reversible Wine environment resets."""
from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import threading
import time
import uuid

from . import bootstrap, games
from .backend import LauncherError, atomic_json
from .setup import _copy_file, _publish, digest, prefix_system32


def _json(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > 2 * 1024**2:
            raise ValueError()
        return json.load(source)


def _id(path):
    info = path.lstat()
    return [info.st_dev, info.st_ino]


def _directory(path):
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise LauncherError("Der ausgewählte Ordner ist keine eigene, unverknüpfte Installation.")
    return path


def _inventory(root):
    """Never follow directory links or cross mounted filesystems during removal."""
    info = root.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise LauncherError("Der ausgewählte Ordner ist keine eigene, unverknüpfte Installation.")
    checksum, size, count = hashlib.sha256(), 0, 0
    def failed(error):
        raise error
    for folder, directories, names in os.walk(root, followlinks=False, onerror=failed):
        for name in sorted(directories + names):
            path = Path(folder) / name
            item = path.lstat()
            count += 1
            if count > 1000000 or item.st_dev != info.st_dev or item.st_uid != os.getuid():
                raise LauncherError("Der Ordner enthält fremde Dateien oder eingebundene Laufwerke. Er wird nicht entfernt.")
            if not (stat.S_ISREG(item.st_mode) or stat.S_ISDIR(item.st_mode) or stat.S_ISLNK(item.st_mode)):
                raise LauncherError("Der Ordner enthält fremde Dateien oder eingebundene Laufwerke. Er wird nicht entfernt.")
            if stat.S_ISREG(item.st_mode):
                size += item.st_size
            checksum.update(repr((str(path.relative_to(root)), item.st_ino, item.st_mode,
                                  item.st_size, item.st_mtime_ns,
                                  os.readlink(path) if stat.S_ISLNK(item.st_mode) else None)).encode())
        directories.sort()
    return {"id": _id(root), "fingerprint": checksum.hexdigest(), "bytes": size}


def _idle(launcher, descriptor):
    from .cloud_process_guard import check, UnsafeSessionError
    from .fenix_processes import Processes
    try:
        check(launcher.runtime, descriptor)
    except UnsafeSessionError as error:
        raise LauncherError(str(error)) from None
    with Processes(launcher.runtime) as processes:
        if processes.live(None):
            raise LauncherError("Beende zuerst alle Programme dieser Spielumgebung, einschließlich Fenix und Installer.")


def _game_entries(runtime):
    game = games.for_runtime(runtime)
    _directory(runtime / "games")
    return [games.path(runtime), *sorted(p for p in (runtime / "games").iterdir()
            if re.fullmatch(rf"\.{game.directory}-before-[0-9a-f]{{32}}", p.name))]


def _packages(launcher, runtime):
    from .game_integrity import installed_identity
    from .mods import _locations
    game = games.for_runtime(runtime)
    result = []
    locations, limited = _locations(runtime)
    if limited:
        raise ValueError()
    for entry in _game_entries(runtime):
        target = entry.resolve(strict=True)
        # A game root must never contain the runtime, its prefix or a home root.
        if (target in {Path.home(), Path('/')} or runtime.is_relative_to(target)
                or target.is_relative_to(runtime / "local")
                or (runtime / "private/local-saves").is_relative_to(target)):
            raise LauncherError("Dieser Spielordner kann nicht sicher entfernt werden.")
        installed_identity(target, game_id=game.id)
        if any(location.is_relative_to(target) for location, _ in locations):
            raise LauncherError("Im Basisspiel liegen zusätzliche Pakete oder Add-ons. Verschiebe sie zuerst oder behalte die Spieldateien.")
        for other in launcher.known_runtimes.values():
            if other != runtime and other.exists():
                if any(path.resolve() == target for path in _game_entries(other)):
                    raise LauncherError("Eine andere Installation verwendet diese Spieldateien. Wähle Spieldateien behalten.")
        if target not in result:
            result.append(target)
    # Refuse nested package roots so that no removal changes a second root.
    if any(a != b and a.is_relative_to(b) for a in result for b in result):
        raise ValueError()
    return result


def _reset_sources(runtime):
    if (runtime / "private/fenix-linux-patch.json").exists():
        raise LauncherError("Stelle unter Mods zuerst den ursprünglichen Fenix-Zustand wieder her. Danach kannst du die Spielumgebung zurücksetzen.")
    _directory(runtime / "local")
    prefix = _directory(runtime / "local/msfs-prefix")
    system = prefix_system32(prefix)
    record = _json(runtime / "private/import-manifest.json")
    expected = record["artifacts"]["files"]
    sources = {"xgameruntime.dll": (system / "xgameruntime.dll", expected["runtime/xgameruntime.dll"]),
               "xodus_store_test.dll": (runtime / "local/store-runtime/x86_64-windows/xodus_store_test.dll",
                                        expected["builtin/x86_64-windows/xodus_store_test.dll"]),
               "xgameruntime_original.dll": (runtime / "runner/files/lib/wine/x86_64-windows/xgameruntime.dll",
                                             record["original_runtime_sha256"])}
    for source, expected_hash in sources.values():
        if digest(source) != expected_hash:
            raise LauncherError("Die Kompatibilitätsdateien passen nicht zur Installation. Aktualisiere Flightdeck vor dem Zurücksetzen.")
    if not (runtime / "runner/files/bin/wine").is_file():
        raise ValueError()
    return sources


def _reset(runtime, cancel):
    from .game_update import exchange
    from .mods import _locations
    sources = _reset_sources(runtime)
    prefix = runtime / "local/msfs-prefix"
    stage = runtime / "local" / ("environment-backup-" + uuid.uuid4().hex)
    stage.mkdir(mode=0o700)
    fresh = stage / "prefix"
    bootstrap.prepare_prefix(runtime / "runner", fresh, cancel=cancel)
    system = prefix_system32(fresh)
    for name, (source, expected) in sources.items():
        _copy_file(source, system / name)
        if digest(system / name) != expected:
            raise ValueError()
    # Keep downloaded content accessible at its original in-prefix location.
    # Graphics settings, registry and add-on programs themselves are reset.
    locations, limited = _locations(runtime)
    if limited:
        raise ValueError()
    linked = []
    for packages in sorted({community.parent for community, _ in locations}, key=lambda p: len(p.parts)):
        if packages.is_relative_to(prefix) and packages.is_dir():
            if any(packages.is_relative_to(parent) for parent in linked):
                continue
            relative = packages.relative_to(prefix)
            destination = fresh / relative
            if destination.exists() or destination.is_symlink():
                raise ValueError()
            parent = fresh
            for name in relative.parts[:-1]:
                parent /= name
                if not parent.exists() and not parent.is_symlink():
                    parent.mkdir(mode=0o700)
                _directory(parent)
            destination.symlink_to(fresh / relative, target_is_directory=True)
            linked.append(packages)
            # After exchange, 'fresh' names the complete old profile.
    record = {"schema": 1, "backup": str(fresh.relative_to(runtime)),
              "original_id": _id(prefix), "fresh_id": _id(fresh)}
    if cancel.is_set():
        raise LauncherError("Wartung abgebrochen. Die bisherige Spielumgebung bleibt aktiv.")
    pending = runtime / "private/environment-reset-pending.json"
    atomic_json(pending, record)
    exchange(prefix, fresh)
    os.replace(pending, runtime / "private/environment-reset.json")
    return str(fresh)


def _restore_path(runtime):
    _directory(runtime / "local")
    _directory(runtime / "local/msfs-prefix")
    for filename in ("environment-reset-pending.json", "environment-reset.json"):
        try:
            record = _json(runtime / "private" / filename)
            name = record.get("backup", "")
            if not re.fullmatch(r"local/environment-backup-[0-9a-f]{32}/prefix", name):
                continue
            backup = runtime / name
            _directory(backup.parent)
            _directory(backup)
            if _id(backup) == record["original_id"] and _id(runtime / "local/msfs-prefix") == record["fresh_id"]:
                return backup
        except (OSError, ValueError, KeyError, TypeError):
            continue
    raise LauncherError("Die gesicherte Spielumgebung passt nicht mehr zur aktiven Installation.")


class Maintenance:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.job = None
        self.plan = None
        self.thread = None
        self.cancel = threading.Event()

    def snapshot(self):
        with self.lock:
            job = copy.deepcopy(self.job)
        can_restore = False
        try:
            if self.launcher.runtime:
                _restore_path(self.launcher.runtime)
                can_restore = True
        except (OSError, ValueError, KeyError, TypeError, LauncherError):
            pass
        return {"job": job, "can_restore": can_restore}

    def _reserve(self):
        with self.launcher.lock:
            # A completed sync failure must not trap users with a broken
            # prefix. Reset/undo leave private saves and recovery journals as
            # they are; uninstallation's preview explicitly reviews retention.
            self.launcher._require_cloud_idle(allow_attention=True)
            self.launcher.reserve_setup()

    def preview(self, data):
        operation = data.get("operation")
        keep_data = data.get("keep_data", True)
        delete_packages = data.get("delete_packages", True)
        if operation not in {"reset", "restore", "uninstall"} or type(keep_data) is not bool or type(delete_packages) is not bool:
            raise LauncherError("Ungültige Wartungsoptionen.")
        if not delete_packages and not keep_data:
            raise LauncherError("Wenn du Spieldateien behältst, muss auch die bisherige Installation erhalten bleiben.")
        with self.launcher.lock, self.lock:
            if self.thread and self.thread.is_alive():
                raise LauncherError("Eine Wartung läuft bereits.")
            if self.launcher.runtime is None:
                raise LauncherError("Zuerst eine Runtime auswählen.")
            game = games.for_runtime(self.launcher.runtime)
            self._reserve()
            self.cancel.clear()
            self.plan = None
            self.job = {"id": uuid.uuid4().hex, "state": "checking", "operation": operation,
                        "runtime_path": str(self.launcher.runtime), "game_name": game.name,
                        "keep_data": keep_data, "delete_packages": delete_packages,
                        "message": "Die ausgewählte Installation wird geprüft …", "error": None}
            self.thread = threading.Thread(target=self._check, daemon=False)
            self.thread.start()
            return {"ok": True, **self.snapshot()}

    def _check(self):
        try:
            root = self.launcher.runtime
            with self.launcher.runtime_lock(operation="maintenance") as descriptor:
                _idle(self.launcher, descriptor)
                paths = _packages(self.launcher, root) if self.job["operation"] == "uninstall" and self.job["delete_packages"] else []
                if self.job["operation"] == "reset":
                    _reset_sources(root)
                elif self.job["operation"] == "restore":
                    _restore_path(root)
                inventory = {str(p): _inventory(p) for p in [root, *paths]}
                self.plan = {"runtime": root, "packages": paths, "inventory": inventory, "created": time.monotonic()}
                with self.lock:
                    self.job.update(state="ready", message="Prüfe die Ordner und bestätige anschließend die Aktion.",
                                    packages=[str(p) for p in paths],
                                    package_bytes=sum(inventory[str(p)]["bytes"] for p in paths))
        except Exception as error:
            self._failed(error)
        finally:
            self.launcher.release_setup()

    def start(self, data):
        with self.launcher.lock, self.lock:
            if (not self.job or self.job["state"] != "ready" or data.get("job_id") != self.job["id"]
                    or data.get("confirmed") is not True or not self.plan
                    or self.launcher.runtime != self.plan["runtime"] or time.monotonic() - self.plan["created"] > 600
                    or (self.thread and self.thread.is_alive())):
                raise LauncherError("Die Vorschau ist nicht mehr gültig. Prüfe die Wartung erneut.")
            self._reserve()
            self.job.update(state="running", message="Die bestätigte Wartung wird ausgeführt …")
            self.thread = threading.Thread(target=self._run, daemon=False)
            self.thread.start()
            return {"ok": True, **self.snapshot()}

    def discard(self, job_id):
        with self.lock:
            if not self.job or self.job["id"] != job_id or self.job["state"] != "ready":
                raise LauncherError("Die Vorschau ist nicht mehr gültig. Prüfe die Wartung erneut.")
            self.plan = None
            self.job.update(state="cancelled", message="Wartung abgebrochen.")
            return {"ok": True, **self.snapshot()}

    def _failed(self, error):
        with self.lock:
            self.job.update(state="failed", error=str(error) if isinstance(error, LauncherError) else
                            "Die Wartung konnte nicht abgeschlossen werden. Vorhandene Sicherungen bleiben erhalten.",
                            message="Wartung nicht abgeschlossen.")

    def _run(self):
        try:
            root = self.plan["runtime"]
            with self.launcher.runtime_lock(operation="maintenance") as descriptor:
                _idle(self.launcher, descriptor)
                if any(_inventory(Path(p)) != before for p, before in self.plan["inventory"].items()):
                    raise LauncherError("Die Installation wurde seit der Vorschau geändert. Prüfe die Wartung erneut.")
                operation = self.job["operation"]
                if operation == "reset":
                    backup = _reset(root, self.cancel)
                    message = "Die Spielumgebung wurde zurückgesetzt. Basisspiel und lokale Spielstände bleiben erhalten. Zusatzprogramme müssen neu eingerichtet werden."
                elif operation == "restore":
                    from .game_update import exchange
                    backup = _restore_path(root)
                    exchange(root / "local/msfs-prefix", backup)
                    backup = str(backup)
                    message = "Die vorherige Spielumgebung ist wieder aktiv."
                else:
                    backup = self._uninstall(root)
                    message = "Das Spiel wurde aus Flightdeck entfernt. Du kannst es unter Installation erneut einrichten."
                with self.lock:
                    self.job.update(state="complete", message=message, backup_path=backup)
                self.launcher.graphics_report = None
        except Exception as error:
            self._failed(error)
        finally:
            self.launcher.release_setup()

    def _uninstall(self, root):
        if not shutil.rmtree.avoids_symlink_attacks:
            raise ValueError()
        # Quarantine first. Until configuration is published, all renames can
        # be reversed. Never recurse through runner, game or Wine drive links.
        archive = root.parent / (root.name + ".uninstalled-" + uuid.uuid4().hex)
        moves = [(package, package.parent / (".flightdeck-remove-" + uuid.uuid4().hex))
                 for package in self.plan["packages"]]
        moved = []
        archived = False
        def relocated(path):
            return archive / path.relative_to(root) if archived and path.is_relative_to(root) else path
        # Persist recovery locations before the first rename, including when a
        # process is interrupted before the archive/configuration is published.
        record = {"schema": 1, "original_runtime": str(root), "archive_runtime": str(archive),
                  "moves": [{"source": str(p), "quarantine": str(q)} for p, q in moves],
                  "keep_data": self.job["keep_data"], "state": "prepared"}
        with self.launcher.lock:
            if self.job["delete_packages"] and _packages(self.launcher, root) != self.plan["packages"]:
                raise LauncherError("Die Installation wurde seit der Vorschau geändert. Prüfe die Wartung erneut.")
            atomic_json(root / "private/uninstalled.json", record)
            try:
                for package, quarantine in moves:
                    _publish(package, quarantine)
                    moved.append((package, quarantine))
                _publish(root, archive)
                archived = True
                remembered = {key: value for key, value in self.launcher.known_runtimes.items() if value != root}
                selected = next(iter(remembered.values()), None)
                atomic_json(self.launcher.config_file, {"schema": 1, "runtime_path": str(selected) if selected else None,
                                                       "runtimes": {k: str(v) for k, v in remembered.items()}})
            except BaseException:
                if archived:
                    _publish(archive, root)
                    archived = False
                for package, quarantine in reversed(moved):
                    _publish(quarantine, package)
                (root / "private/uninstalled.json").unlink()
                raise
            self.launcher.runtime = selected
            self.launcher.known_runtimes = remembered
        # Record cleanup locations before deletion: interrupted cleanup can be
        # completed manually, and never masquerades as a successful uninstall.
        with self.lock:
            self.job["backup_path"] = str(archive)
        record.update(state="detached", cleanup=[str(relocated(q)) for _, q in moved])
        atomic_json(archive / "private/uninstalled.json", record)
        for _, quarantine in moved:
            shutil.rmtree(relocated(quarantine))
        if not self.job["keep_data"]:
            shutil.rmtree(archive)
            return None
        return str(archive)

    def close(self):
        self.cancel.set()
        if self.thread:
            self.thread.join(timeout=5)
