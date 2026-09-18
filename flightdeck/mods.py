# SPDX-License-Identifier: MIT
"""Read the configured MSFS Community directory; never execute add-on code.

Only known configuration locations inside the selected Wine prefix are visited.
Package-directory symlinks are supported; manifest files themselves must be
bounded regular files. Inventory data stays out of diagnostic exports.
"""
from __future__ import annotations

import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import xml.etree.ElementTree as ET

from .backend import LauncherError

MAX_MODS = 256
MAX_USERS = 16
MANIFEST_LIMIT = 256 * 1024
CONFIG_LIMIT = 1024 * 1024


def _read(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ValueError("not a bounded regular configuration file")
        result = stream.read(limit + 1)
        if len(result) > limit:
            raise ValueError("configuration changed size")
        return result


def _children(path, limit):
    with os.scandir(path) as entries:
        result = list(itertools.islice(entries, limit + 1))
    return result[:limit], len(result) > limit


def _case_path(root, parts):
    """Resolve configured Windows spelling without an unbounded filesystem scan."""
    for part in parts:
        candidate = root / part
        if not candidate.exists() and not candidate.is_symlink() and root.is_dir():
            entries, limited = _children(root, 256)
            matches = [entry.name for entry in entries if entry.name.casefold() == part.casefold()]
            if limited or len(matches) > 1:
                raise ValueError("ambiguous Windows path")
            if matches:
                candidate = root / matches[0]
        root = candidate
    return root.resolve()


def _configured_path(raw, prefix):
    if not isinstance(raw, str) or not raw or len(raw) > 4096 or any(ord(c) < 32 for c in raw):
        raise ValueError("invalid configured path")
    if raw.startswith("/"):
        return Path(raw).resolve()
    match = re.fullmatch(r"([A-Za-z]):[\\/](.*)", raw)
    if not match:
        raise ValueError("unsupported configured path")
    parts = re.split(r"[\\/]", match[2].rstrip("\\/"))
    if any(part in {"", ".", ".."} for part in parts):
        raise ValueError("invalid Windows path")
    drive = prefix / "dosdevices" / (match[1].lower() + ":")
    if not drive.is_dir():
        raise ValueError("Wine drive is not mapped")
    return _case_path(drive.resolve(), parts)


def _family(runtime):
    # MSIX's publisher ID is the first 64 SHA-256 bits of the exact UTF-16LE
    # publisher string, encoded MSB-first with the documented base32 alphabet.
    for spelling in ("MicrosoftGame.Config", "MicrosoftGame.config"):
        path = runtime / "games/MSFS2024" / spelling
        if path.exists():
            data = _read(path, CONFIG_LIMIT)
            if b"<!DOCTYPE" in data.upper() or b"<!ENTITY" in data.upper():
                raise ValueError("unsupported XML declarations")
            root = ET.fromstring(data)
            identities = [node for node in root.iter() if node.tag.rsplit("}", 1)[-1] == "Identity"]
            if len(identities) != 1:
                return None
            name, publisher = identities[0].get("Name", ""), identities[0].get("Publisher", "")
            if not re.fullmatch(r"[A-Za-z0-9.-]{1,200}", name) or not publisher:
                return None
            value = int.from_bytes(hashlib.sha256(publisher.encode("utf-16-le")).digest()[:8], "big") << 1
            alphabet = "0123456789abcdefghjkmnpqrstvwxyz"
            return name + "_" + "".join(alphabet[(value >> shift) & 31] for shift in range(60, -1, -5))
    return None


def _locations(runtime):
    prefix = runtime / "local/msfs-prefix"
    candidates = []
    settings = runtime / "private/runtime.json"
    if settings.exists() or settings.is_symlink():
        value = json.loads(_read(settings, 65536))
        if not isinstance(value, dict):
            raise ValueError("invalid runtime settings")
        for key in ("community_path", "CommunityLocation", "installed_packages_path", "InstalledPackagesPath"):
            if key in value:
                path = _configured_path(value[key], prefix)
                candidates.append((path if key in {"community_path", "CommunityLocation"} else path / "Community", "runtime_config"))
    users = prefix / "drive_c/users"
    if not users.is_dir():
        return candidates, False
    profiles, limited = _children(users, MAX_USERS)
    family = _family(runtime)
    for profile in profiles:
        if not profile.is_dir():
            continue
        base = Path(profile.path)
        configs = [base / "AppData/Roaming/Microsoft Flight Simulator 2024/UserCfg.opt"]
        if family:
            configs.append(base / "AppData/Local/Packages" / family / "LocalCache/UserCfg.opt")
        for config in configs:
            if not (config.exists() or config.is_symlink()):
                continue
            data = _read(config, CONFIG_LIMIT)
            text = data.decode("utf-16" if data.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig")
            paths = re.findall(r'^\s*InstalledPackagesPath\s+"([^"\r\n]+)"\s*$', text, re.MULTILINE)
            if len(paths) != 1:
                raise ValueError("missing or duplicate packages location")
            packages = _configured_path(paths[0], prefix)
            candidates.append((_case_path(packages, ["Community"]), "usercfg"))
    return candidates, limited


def _opener():
    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        return None
    for program in ("xdg-open", "gio"):
        if executable := shutil.which(program):
            return [executable] if program == "xdg-open" else [executable, "open", "--"]
    return None


def _field(value, key, fallback=""):
    text = value.get(key, fallback)
    if not isinstance(text, str):
        return fallback
    return "".join(c for c in text[:512] if ord(c) >= 32).strip()


def _inventory(folder):
    entries, limited = _children(folder, MAX_MODS)
    mods = []
    for entry in sorted(entries, key=lambda item: item.name.casefold()):
        if not entry.is_dir() and not entry.is_symlink():
            continue
        item = {"id": entry.name, "name": entry.name, "version": "", "creator": "", "content_type": "",
                "status": "available", "is_link": entry.is_symlink()}
        try:
            directory = Path(entry.path).resolve(strict=True)
            if not directory.is_dir():
                raise ValueError("not a package directory")
            value = json.loads(_read(directory / "manifest.json", MANIFEST_LIMIT))
            if not isinstance(value, dict):
                raise ValueError("invalid manifest")
            if not _field(value, "title") or not _field(value, "package_version"):
                raise ValueError("missing manifest identity")
            item.update(name=_field(value, "title", entry.name) or entry.name,
                        version=_field(value, "package_version"), creator=_field(value, "creator"),
                        content_type=_field(value, "content_type"))
        except FileNotFoundError:
            item["status"] = "missing_manifest" if entry.is_dir() else "unreadable"
        except (ValueError, UnicodeError, RecursionError):
            item["status"] = "invalid_manifest"
        except (OSError, RuntimeError):
            item["status"] = "unreadable"
        mods.append(item)
    return mods, len(entries), limited


def snapshot(launcher):
    with launcher.lock:
        runtime = launcher.runtime
        busy = launcher.setup_busy or launcher.desktop_closing
    result = {"state": "unconfigured", "message": "Zuerst eine Runtime einrichten oder auswählen.", "folder_path": None,
              "source": None, "can_open": False, "mods": [], "count": 0, "scanned_count": 0, "limited": False}
    if runtime is None:
        return result
    try:
        locations, limited = _locations(runtime)
        unique = {path.resolve(): source for path, source in locations}
        if limited or len(unique) > 1:
            result.update(state="ambiguous", limited=limited, message="Mehrere oder zu viele Spielkonfigurationen gefunden. Den Community-Pfad bitte im Spiel prüfen.")
            return result
        if not unique:
            result.update(state="unknown", message="Der Community-Ordner ist noch nicht bekannt. MSFS einmal starten und den Paketordner im Spiel einrichten.")
            return result
        folder, source = next(iter(unique.items()))
        result.update(folder_path=str(folder), source=source)
        if not folder.is_dir():
            result.update(state="missing", message="Der konfigurierte Community-Ordner existiert noch nicht. Den Paketordner bitte im Spiel prüfen.")
            return result
        mods, scanned, limited = _inventory(folder)
        result.update(state="ready", message="Community-Ordner erkannt. Die Liste zeigt lokale Pakete, keine Kompatibilitätsprüfung.",
                      mods=mods, count=len(mods), scanned_count=scanned, limited=limited, can_open=not busy and _opener() is not None)
        return result
    except (OSError, ValueError, RuntimeError, ET.ParseError, RecursionError):
        result.update(state="error", message="Die Mod-Konfiguration konnte nicht sicher gelesen werden. Dateirechte und den Paketordner im Spiel prüfen.")
        return result


def open_folder(launcher):
    with launcher.lock:
        launcher.require_open()
        if launcher.setup_busy:
            raise LauncherError("Bitte die laufende Einrichtung abschließen oder abbrechen.")
        selected = launcher.runtime
    value = snapshot(launcher)
    if value["state"] != "ready":
        raise LauncherError(value["message"])
    command = _opener()
    if not command:
        raise LauncherError("Kein Linux-Ordneröffner verfügbar. Bitte xdg-utils installieren und Flightdeck in der grafischen Sitzung öffnen.")
    with launcher.lock:
        launcher.require_open()
        if selected != launcher.runtime or launcher.setup_busy:
            raise LauncherError("Die Runtime hat sich geändert. Bitte die Mod-Liste neu laden.")
        try:
            # URI encoding makes spaces, metacharacters and leading dashes data.
            completed = subprocess.run([*command, Path(value["folder_path"]).as_uri() + "/"], stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, close_fds=True,
                                       start_new_session=True, timeout=10, check=False)
        except (OSError, subprocess.TimeoutExpired):
            raise LauncherError("Der Community-Ordner konnte nicht geöffnet werden. Bitte den angezeigten Pfad im Dateimanager öffnen.") from None
        if completed.returncode:
            raise LauncherError("Der Community-Ordner konnte nicht geöffnet werden. Bitte den angezeigten Pfad im Dateimanager öffnen.")
    return {"ok": True, "message": "Der Community-Ordner wurde an den Dateimanager übergeben."}
