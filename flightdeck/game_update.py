# SPDX-License-Identifier: MIT
"""Explicit, revision-bound base-game updates. No in-place package writes.

The existing prefix, saves and package folders are retained. Each download has
a new private journal; publication exchanges one directory entry atomically.
The previous entry stays available for an explicit rollback, including after
a crash between exchange and journal completion.
"""
from __future__ import annotations

import copy
import ctypes
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET

from .backend import LauncherError, atomic_json
from .game_install import (MSFS_STORE_ID, RESUME_FEATURE, verify_cli, run_cli,
                           download_game, _stop_owned)
from .setup import SetupError, interrupted
from .mods import _read

FEATURE = "package-info-json-v1"


def installed_identity(game):
    from .game_integrity import installed_identity as read_identity
    return read_identity(game)


class UpdateError(SetupError):
    pass


class AuthRequired(UpdateError):
    pass


def version(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]{1,5}(?:\.[0-9]{1,5}){3}", value):
        raise UpdateError("Die Spielversion ist nicht eindeutig lesbar. Das vorhandene Paket wurde nicht verändert.")
    parts = tuple(map(int, value.split(".")))
    if max(parts) > 65535:
        raise UpdateError("Die Spielversion ist nicht eindeutig lesbar. Das vorhandene Paket wurde nicht verändert.")
    return parts


def installed(game, *, data=None):
    config = game / "MicrosoftGame.Config"
    if not config.exists():
        config = game / "MicrosoftGame.config"
    try:
        if data is None:
            data = _read(config, 1024 * 1024)
        # Decode before checking declarations, including UTF-16 game configs.
        text = data.decode("utf-16" if data.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig")
        if "<!DOCTYPE" in text.upper() or "<!ENTITY" in text.upper():
            raise ValueError()
        root = ET.fromstring(text)
        identity = [n for n in root.iter() if n.tag.rsplit("}", 1)[-1] == "Identity"]
        stores = [n for n in root.iter() if n.tag.rsplit("}", 1)[-1] == "StoreId"]
        if len(identity) != 1 or len(stores) != 1 or stores[0].text != MSFS_STORE_ID:
            raise ValueError()
        attrs = identity[0].attrib
        result = {"name": attrs["Name"], "publisher": attrs["Publisher"], "version": attrs["Version"]}
        if not result["name"] or not result["publisher"]:
            raise ValueError()
        version(result["version"])
        return result
    except (OSError, ValueError, KeyError, ET.ParseError):
        raise UpdateError("Für diese Installation fehlen eindeutige MSFS-Store-Identität und Spielversion. Ein Update wird nicht geraten.") from None


def _private(path, *, create=False):
    if create:
        path.mkdir(mode=0o700, exist_ok=True)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o022:
        raise UpdateError("Der Updateordner muss ein eigener, nicht gemeinsam beschreibbarer Ordner sein.")
    return path


def _sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def _write(path, data):
    atomic_json(path, data)
    _sync(path.parent)


def exchange(first, second):
    rename = getattr(ctypes.CDLL(None, use_errno=True), "renameat2", None)
    if rename is None:
        raise UpdateError("Dieses Linux unterstützt den atomaren Spielwechsel nicht. Die bisherige Version bleibt aktiv.")
    rename.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    rename.restype = ctypes.c_int
    if rename(-100, os.fsencode(first), -100, os.fsencode(second), 2):
        raise UpdateError("Der atomare Spielwechsel ist fehlgeschlagen. Die bisherige Version bleibt aktiv.")


def tools(runtime, source_root=None, *, verify=True):
    from . import bootstrap
    _, lockfile = bootstrap.paths(source_root)
    lock = json.loads(_read(lockfile, 65536))
    native = lock["native"]
    features = native.get("cli_features", [])
    if FEATURE not in features or RESUME_FEATURE not in features:
        raise UpdateError("Diese Flightdeck-Komponenten unterstützen noch keine sicheren Spielupdates. Bitte Flightdeck aktualisieren.")
    expected = native["files"]["bin/xodus-cli"]
    # An existing runtime may still have an older CLI. Prefer the current
    # verified packaged CLI; never silently claim capability for the old one.
    candidates = [runtime / "bin/xodus-cli"]
    source, _ = bootstrap.paths(source_root)
    packaged = bootstrap.native_path(source, lock if verify else None)
    if packaged:
        candidates.insert(0, packaged / "bin/xodus-cli")
    for candidate in candidates:
        try:
            if not verify and candidate.is_file():
                return candidate, expected, features
            return verify_cli(candidate, expected), expected, features
        except ValueError:
            pass
    raise UpdateError("Der geprüfte Update-Downloader fehlt. Bitte das vollständige aktuelle Flightdeck-Paket installieren.")


def package_info(cli, expected, runtime, market, cancel):
    path = verify_cli(cli, expected)
    environment = dict(os.environ, XODUS_LOG="off", RUST_BACKTRACE="0")
    for category in ("config", "data", "cache", "state"):
        environment["XDG_" + category.upper() + "_HOME"] = str(runtime / "private/xdg" / category)
    # Even an unexpected CLI failure cannot accumulate unbounded output or
    # expose a URL. Only the strict JSON projection below leaves this function.
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen([str(path), "package-info", MSFS_STORE_ID, "--market", market],
                                   cwd=runtime / "private", env=environment, stdin=subprocess.DEVNULL,
                                   stdout=output, stderr=subprocess.DEVNULL, close_fds=True,
                                   start_new_session=True, umask=0o077)
        started = time.monotonic()
        try:
            while process.poll() is None:
                interrupted(cancel)
                if time.monotonic() - started > 90 or os.fstat(output.fileno()).st_size > 8192:
                    raise UpdateError("Die Updateprüfung konnte nicht abgeschlossen werden. Verbindung prüfen und erneut versuchen.")
                cancel.wait(.1) if cancel is not None else time.sleep(.1)
            interrupted(cancel)
            if process.returncode == 77:
                raise AuthRequired("Für die Updateprüfung ist eine erneute Microsoft-Anmeldung erforderlich.")
            if process.returncode:
                raise UpdateError("Die Updateprüfung konnte nicht abgeschlossen werden. Verbindung prüfen und erneut versuchen.")
            output.seek(0)
            value = json.loads(output.read(8193))
        finally:
            _stop_owned(process)
    return validate_info(value)


def validate_info(value):
    keys = {"schema", "store_id", "version", "version_id", "content_id", "package_identity", "size_bytes"}
    try:
        if not isinstance(value, dict) or set(value) != keys or type(value["schema"]) is not int or value["schema"] != 1 or value["store_id"] != MSFS_STORE_ID:
            raise ValueError()
        version(value["version"])
        for key in ("content_id",):
            if not isinstance(value[key], str) or str(uuid.UUID(value[key])) != value[key].lower():
                raise ValueError()
        revision = value["version_id"]
        if not isinstance(revision, str) or len(revision) > 128:
            raise ValueError()
        try:
            simple = str(uuid.UUID(revision)) == revision.lower()
        except ValueError:
            simple = False
        if not simple:
            prefix, suffix = revision.rsplit(".", 1)
            if version(prefix) != version(value["version"]) or str(uuid.UUID(suffix)) != suffix.lower():
                raise ValueError()
        if not isinstance(value["package_identity"], str) or not re.fullmatch("[0-9a-f]{64}", value["package_identity"]):
            raise ValueError()
        if type(value["size_bytes"]) is not int or not 0 < value["size_bytes"] < 8 * 1024**4:
            raise ValueError()
    except (ValueError, TypeError, KeyError, AttributeError):
        raise UpdateError("Die Paketantwort enthält keine gültige, eindeutige Updateversion.") from None
    return value


@dataclass
class UpdatePlan:
    runtime: Path
    current: dict
    game_target: Path
    latest: dict
    cli: Path
    cli_hash: str
    features: list
    market: str



def configured_market(runtime):
    """Read the current runtime format or its explicit legacy launcher literal.

    Older imported Flightdeck workspaces kept this setting in the launch script.
    This compatibility read never executes shell or writes migration metadata.
    """
    try:
        settings = json.loads(_read(runtime / "private/runtime.json", 65536))
        market = settings.get("market")
    except FileNotFoundError:
        try:
            script = _read(runtime / "tools/launch-msfs.sh", 128 * 1024).decode("utf-8")
            declarations = re.findall(r"(?m)^[ \t]*(export[ \t]+)?XODUS_STORE_MARKET=(.*)$", script)
            if len(declarations) != 1 or not declarations[0][0]:
                raise ValueError()
            literal = re.fullmatch(r"(?:([A-Z]{2})|\"([A-Z]{2})\"|'([A-Z]{2})')(?:[ \t]+#.*|[ \t]*)", declarations[0][1])
            if literal is None:
                raise ValueError()
            market = next(value for value in literal.groups() if value)
        except (OSError, ValueError):
            raise UpdateError("Für diese ältere Runtime fehlt eine eindeutige Store-Region. Bitte die Runtimekonfiguration prüfen.") from None
    except (OSError, ValueError, AttributeError):
        raise UpdateError("Die konfigurierte Store-Region ist ungültig.") from None
    if not isinstance(market, str) or not re.fullmatch("[A-Z]{2}", market):
        raise UpdateError("Die konfigurierte Store-Region ist ungültig.")
    return market


def check(launcher, data, *, notify, cancel, source_root=None):
    runtime = launcher.runtime
    if runtime is None:
        raise UpdateError("Zuerst eine Runtime auswählen.")
    _private(runtime / "private")
    _private(runtime / "games")
    cli, cli_hash, features = tools(runtime, source_root)
    if data.get("operation") == "repair":
        from .game_integrity import FEATURE as INTEGRITY_FEATURE
        if INTEGRITY_FEATURE not in features:
            raise UpdateError("Für eine vollständige Reparatur mit Prüfnachweis bitte das aktuelle Flightdeck-Paket installieren.")
    current = installed_identity(runtime / "games/MSFS2024")
    # If user data lives inside the package, swapping the path may hide it.
    # Require an explicit move in the game rather than guessing what to copy.
    from .mods import _locations
    locations, limited = _locations(runtime)
    target = (runtime / "games/MSFS2024").resolve(strict=True)
    if limited or any(path.is_relative_to(target) for path, _ in locations):
        raise UpdateError("Community- oder Benutzerpakete liegen im Spielpaket. Diese bitte zuerst im Spiel in einen separaten Paketordner verschieben.")
    market = configured_market(runtime)
    if type(data.get("sign_in", False)) is not bool:
        raise UpdateError("Ungültige Updateanfrage.")
    with launcher.runtime_lock(operation="game_update"):
        if data.get("sign_in"):
            notify("authentication", "Bitte im Microsoft-Fenster mit dem Konto anmelden, das MSFS besitzt.")
            result = run_cli(cli, ["login"], cwd=runtime / "private", cancel=cancel, timeout=900,
                             xdg_root=runtime / "private/xdg")
            if result:
                raise AuthRequired("Die Microsoft-Anmeldung wurde nicht abgeschlossen. Bitte ausdrücklich erneut anmelden.")
        notify("package_check", "Installierte Spielversion und aktuelle Store-Paketversion werden geprüft …")
        latest = package_info(cli, cli_hash, runtime, market, cancel)
    if version(latest["version"]) < version(current["version"]):
        raise UpdateError("Der Store liefert eine ältere Spielversion. Es wird kein Downgrade durchgeführt.")
    return UpdatePlan(runtime, current, target, latest, cli, cli_hash, features, market)


def _history_record(runtime, path):
    value = json.loads(_read(path, 16384))
    if value.get("format") != 1 or not re.fullmatch(r"\.MSFS2024-before-[0-9a-f]{32}", value.get("previous_entry", "")):
        raise ValueError()
    previous = runtime / "games" / value["previous_entry"]
    target = Path(value["new_target"])
    if not target.is_absolute() or not target.is_relative_to(runtime / "private/game-updates"):
        raise ValueError()
    # The pointer itself resolves a crash between exchange and journal update.
    active = runtime / "games/MSFS2024"
    if not active.is_symlink() or active.resolve() != target or not previous.exists():
        raise ValueError()
    if installed_identity(active) != value["new_identity"] or installed_identity(previous) != value["old_identity"]:
        raise ValueError()
    return value, previous


def _history(runtime):
    # A prepared journal is authoritative only after its new pointer became
    # active. Before exchange the previous confirmed rollback remains valid.
    for name in ("game-update-pending.json", "game-update.json"):
        try:
            return _history_record(runtime, runtime / "private" / name)
        except (OSError, ValueError, KeyError, TypeError):
            continue
    raise ValueError("No rollback matches the active package")


def install(launcher, plan, *, notify, cancel, control, committing, transfer=None):
    runtime = plan.runtime
    with launcher.runtime_lock(operation="game_update"):
        _private(runtime / "private")
        _private(runtime / "games")
        if launcher.runtime != runtime or (runtime / "games/MSFS2024").resolve() != plan.game_target or installed_identity(runtime / "games/MSFS2024") != plan.current:
            raise UpdateError("Das installierte Spiel hat sich geändert. Bitte Updates erneut prüfen.")
        # Space is an estimate for the entire new package, not a delta promise.
        if shutil.disk_usage(runtime / "private").free < plan.latest["size_bytes"] * 2 + 1024**3:
            raise UpdateError("Für die neue Spielversion und die erhaltene Rückfallversion fehlt freier Speicherplatz.")
        root = _private(runtime / "private/game-updates", create=True)
        work = Path(tempfile.mkdtemp(prefix="update-", dir=root))
        xdg = runtime / "private/xdg"
        if xdg.is_symlink():
            xdg = xdg.resolve(strict=True)
        _private(xdg, create=True)
        game = download_game(plan.cli, plan.cli_hash, work / "game", plan.market, cancel=cancel,
                             notify=notify, xdg_root=xdg, control=control, cli_features=plan.features,
                             sign_in=False, expected_package=plan.latest["package_identity"], transfer=transfer)
        notify("verify_update", "Die heruntergeladene Spielversion wird vor dem Wechsel geprüft …")
        identity = installed(game)
        if identity != {**plan.current, "version": plan.latest["version"]}:
            raise UpdateError("Das heruntergeladene Paket passt nicht zur geprüften Spielidentität und Version. Die bisherige Version bleibt aktiv.")
        interrupted(cancel)
        if (runtime / "games/MSFS2024").resolve() != plan.game_target or installed_identity(runtime / "games/MSFS2024") != plan.current:
            raise UpdateError("Das installierte Spiel hat sich geändert. Bitte Updates erneut prüfen.")
        previous = runtime / "games" / (".MSFS2024-before-" + uuid.uuid4().hex)
        previous.symlink_to(game, target_is_directory=True)
        history = {"format": 1, "previous_entry": previous.name, "new_target": str(game),
                   "old_identity": plan.current, "new_identity": identity,
                   "package": plan.latest}
        # The journal is durable before the atomic switch. A interrupted switch
        # is resolved from the active pointer; neither version is deleted.
        record = runtime / "private/game-update.json"
        pending = runtime / "private/game-update-pending.json"
        old_record = json.loads(_read(record, 16384)) if record.exists() else None
        try:
            recovered, _ = _history(runtime)
            # Finish an earlier post-exchange crash before replacing pending.
            if recovered != old_record:
                _write(record, recovered)
                old_record = recovered
        except ValueError:
            pass
        committing()
        swapped = False
        try:
            _write(pending, history)
            exchange(previous, runtime / "games/MSFS2024")
            swapped = True
            _sync(runtime / "games")
            _write(record, history)
            pending.unlink()
            _sync(pending.parent)
        except Exception:
            if swapped:
                exchange(previous, runtime / "games/MSFS2024")
                _sync(runtime / "games")
            if old_record is not None:
                _write(record, old_record)
            else:
                record.unlink(missing_ok=True)
                _sync(record.parent)
            pending.unlink(missing_ok=True)
            _sync(pending.parent)
            raise
        return runtime


def rollback(launcher):
    # Claim the old ready plan and the new reservation together. Otherwise a
    # concurrent cancel(old_id) could release the rollback's reservation.
    with launcher.setup.lock:
        launcher.reserve_setup()
        if launcher.setup.job and launcher.setup.job.get("mode") == "update":
            launcher.setup.job = None
            launcher.setup.plan = None
    try:
        with launcher.runtime_lock(operation="game_update"):
            runtime = launcher.runtime
            value, previous = _history(runtime)
            exchange(previous, runtime / "games/MSFS2024")
            _sync(runtime / "games")
        return {"ok": True, "message": "Die vorherige Spielversion wurde wieder aktiviert. Spielstände und Add-ons wurden nicht verändert."}
    except (OSError, ValueError, KeyError, TypeError):
        raise LauncherError("Die vorherige Spielversion ist nicht eindeutig verfügbar. Es wurde nichts gelöscht.") from None
    finally:
        launcher.release_setup()


def snapshot(launcher):
    result = {"available": False, "unavailable_reason": "Zuerst eine Runtime auswählen.",
              "installed_version": None, "latest_version": None, "update_available": None,
              "can_check": False, "can_start": False, "can_rollback": False,
              "auth_required": False, "job": None, "can_repair": False,
              "integrity": {"available": False, "unavailable_reason": "Zuerst eine Runtime auswählen.", "can_check": False, "result": None}}
    with launcher.lock:
        runtime = launcher.runtime
        launcher._poll()
        busy = launcher.setup_busy or launcher.process is not None or launcher.desktop_closing
        if not busy and runtime is not None:
            busy = launcher._external(create_lock=False)
    if runtime is None:
        return result
    from . import game_integrity
    usable = game_integrity.available(runtime / "games/MSFS2024")
    result["integrity"].update(available=usable, unavailable_reason="" if usable else game_integrity.UNAVAILABLE, can_check=usable and not busy)
    try:
        result["installed_version"] = installed_identity(runtime / "games/MSFS2024")["version"]
        _, _, features = tools(runtime, launcher.setup.source_root, verify=False)
        result.update(available=True, unavailable_reason="", can_check=not busy, can_repair=not busy and game_integrity.FEATURE in features)
    except (OSError, ValueError, KeyError) as error:
        result["unavailable_reason"] = error.args[0] if isinstance(error, UpdateError) else "Die Updatekonfiguration ist unvollständig. Bitte Flightdeck und die Runtime prüfen."
    with launcher.setup.lock:
        job = launcher.setup.job
        if job and job.get("mode") == "update" and job.get("runtime_path") == str(runtime):
            result["job"] = copy.deepcopy(job)
            result["integrity"]["result"] = copy.deepcopy(job.get("integrity_result"))
            for key in ("latest_version", "update_available", "auth_required"):
                result[key] = job.get(key, result[key])
            result["can_start"] = result["available"] and not busy and job["state"] == "ready" and (job.get("update_available") is True or job.get("operation") == "repair")
    if not busy:
        try:
            _history(runtime)
            result["can_rollback"] = True
        except (OSError, ValueError, KeyError, TypeError):
            pass
    return result
