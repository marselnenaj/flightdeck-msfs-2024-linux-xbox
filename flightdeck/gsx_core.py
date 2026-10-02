# SPDX-License-Identifier: MIT
"""Experimental FSDT setup. Account activation stays in the official installer."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import uuid
import xml.etree.ElementTree as ET

from . import games, mods
from ._fenix import core

MARKER = "private/gsx-setup.json"
STARTUP = "private/gsx-startup.json"
INSTALLER_URL = "https://www.fsdreamteam.com/update/FSDT_Universal_Installer.exe"
INSTALLER_SHA256 = "4d0230ffbf3d0a8d69fb23dbf280be4783a13b87fb0f721c0e3a377eea114865"
MANAGER_DIR = Path("drive_c/Program Files (x86)/Addon Manager")
COMPANIONS = frozenset({"couatl64_boot.exe", "couatl64_msfs.exe", "couatl64_msfs2024.exe"})


def owned_directory(path, root):
    if path.is_symlink() or not path.is_dir() or path.stat().st_uid != os.getuid() or not path.resolve().is_relative_to(root.resolve()):
        raise core.PatchError("Der GSX-Ordner muss innerhalb des gewählten Windows-Profils liegen.")
    return path


def manager_directory(prefix):
    """Honor the official HKCU root, but never run/copy an external installation."""
    candidates = []
    registry = prefix / "user.reg"
    if registry.exists():
        content = mods._read(registry, 32 * 1024 * 1024).decode("utf-8", errors="replace")
        section = re.search(r"(?mi)^\[Software\\\\Fsdreamteam\][^\[]*", content)
        value = re.search(r'(?m)^"root"=("(?:[^"\\]|\\.)*")$', section.group(0)) if section else None
        if value:
            candidates.append(mods._configured_path(json.loads(value[1]), prefix))
    candidates += [prefix / MANAGER_DIR, prefix / "drive_c/Program Files/Addon Manager"]
    for directory in dict.fromkeys(candidates):
        if directory.exists():
            owned_directory(directory, prefix / "drive_c")
            if (directory / "Couatl_Updater.exe").exists():
                core.regular(directory / "Couatl_Updater.exe", 180 * 1024 * 1024)
                return directory
    return None


def read_marker(root):
    path = root / MARKER
    if not path.exists() and not path.is_symlink():
        return None
    value = core.read_json(path)
    if (not isinstance(value, dict) or value.get("format") != 1 or
            not isinstance(value.get("id"), str) or not re.fullmatch(r"[0-9a-f]{32}", value["id"]) or
            value.get("state") not in {"preparing", "committing", "ready"}):
        raise core.PatchError("Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.")
    return value


def setup_complete(root):
    try:
        state = read_marker(root)
        return state is None or state["state"] == "ready"
    except (OSError, ValueError, core.PatchError):
        return False


def identity(path):
    info = path.stat()
    return [info.st_dev, info.st_ino]


def wine_environment(prefix, runner):
    # Share the proven .NET setup environment without applying a Fenix patch.
    env = core.wine_env(prefix, runner)
    for key in ("WINE_TRACK_WRITECOPY", "WINE_D2D1_DISPLAY_EFFECTS", "WINE_D2D1_GEOMETRY_PROVIDER",
                "WINE_FENIX_HELPER_WINDOWS", "WINE_DWRITE_UNHINTED_OUTLINES"):
        env.pop(key, None)
    return env


def copy_profile(prefix, staged):
    core.copy_tree(prefix, staged)
    # Absolute links back into the live prefix must follow the copy, including
    # installed add-on links. Relative links also survive the final rename.
    for link in staged.rglob("*"):
        if not link.is_symlink():
            continue
        target = link.readlink()
        if target.is_absolute() and target.is_relative_to(prefix):
            target = staged / target.relative_to(prefix)
            link.unlink()
            link.symlink_to(os.path.relpath(target, link.parent))
    drive = staged / "dosdevices/c:"
    if drive.is_symlink():
        drive.unlink()
    drive.symlink_to("../drive_c")


def updater_override(prefix, name):
    path = prefix / "user.reg"
    if not path.exists():
        return None
    text = mods._read(path, 32 * 1024 * 1024).decode("utf-8", errors="replace")
    section_name = r"Software\\Wine\\AppDefaults\\" + name + r"\\DllOverrides"
    section = re.search(r"(?mi)^\[" + re.escape(section_name) + r"\][^\[]*", text)
    value = re.search(r'(?m)^"mscoree"=("(?:[^"\\]|\\.)*")$', section.group(0)) if section else None
    return json.loads(value[1]) if value else None


def prepare(root, cache, progress=lambda _: None):
    root = core.runtime_path(root)
    previous = read_marker(root)
    if previous and previous["state"] != "ready":
        raise core.PatchError("Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.")
    prefix, runner = root / "local/msfs-prefix", (root / "runner").resolve(strict=True)
    # Validate a previous custom FSDT location before invoking its installer.
    manager_directory(prefix)
    progress("Der offizielle FSDT-Installer wird heruntergeladen und geprüft …")
    installer = core.download(INSTALLER_URL, cache / "FSDT_Universal_Installer.exe", INSTALLER_SHA256)
    needed = sum(p.stat().st_size for p in prefix.rglob("*") if p.is_file() and not p.is_symlink())
    if shutil.disk_usage(root).free < needed + 1024 ** 3:
        raise core.PatchError("Für die GSX-Vorbereitung fehlt Speicherplatz für eine Profilkopie.")
    token = uuid.uuid4().hex
    staged = root / "local" / (".gsx-prefix-" + token)
    backup = root / "local" / ("msfs-prefix.before-gsx-" + token)
    state = {"format": 1, "id": token, "state": "preparing", "original_prefix_id": identity(prefix), "prior": previous}
    core.write_json(root / MARKER, state)
    wine = None
    try:
        progress("Das Windows-Profil wird für die GSX-Einrichtung kopiert …")
        copy_profile(prefix, staged)
        logpath = root / "private" / ("gsx-setup-" + token + ".log")
        with logpath.open("xb") as log:
            wine = core.Wine(staged, runner, log)
            wine.env = wine_environment(staged, runner)
            try:
                core.prepare_framework(wine, cache, progress)
                progress("Der FSDT-Installer wird im kopierten Profil eingerichtet …")
                directory = manager_directory(staged) or staged / MANAGER_DIR
                windows_dir = "C:\\" + str(directory.relative_to(staged / "drive_c")).replace("/", "\\")
                # Inno's post-install action otherwise starts the live updater,
                # which could change an external Community folder while staged.
                # Suppress only these managed updater executables during setup;
                # restore their previous overrides before publishing the copy.
                overrides = {name: updater_override(staged, name) for name in ("Couatl_Updater.exe", "Couatl_Updater2.exe")}
                try:
                    for name in overrides:
                        wine.reg("HKCU\\Software\\Wine\\AppDefaults\\" + name + "\\DllOverrides", "mscoree", "")
                    wine.run(installer, "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/DIR=" + windows_dir)
                finally:
                    wine.stop()
                    for name, old in overrides.items():
                        key = "HKCU\\Software\\Wine\\AppDefaults\\" + name + "\\DllOverrides"
                        if old is None:
                            wine.run("reg", "delete", key, "/v", "mscoree", "/f", accepted=(0, 1))
                        else:
                            wine.reg(key, "mscoree", old)
                # Setup may return 0 even if its unquoted RegAsm call failed.
                directory = manager_directory(staged)
                if directory is None:
                    raise core.PatchError("Der FSDT-Installer wurde nicht vollständig eingerichtet.")
                assembly = core.regular(directory / "QlmLicenseLib.dll", 64 * 1024 * 1024)
                wine.run(staged / "drive_c/windows/Microsoft.NET/Framework/v4.0.30319/RegAsm.exe",
                         "/codebase", "C:\\" + str(assembly.relative_to(staged / "drive_c")).replace("/", "\\"))
            finally:
                # This server belongs solely to our private staging copy.
                wine.stop()
        if not core.has_framework(staged) or manager_directory(staged) is None:
            raise core.PatchError("Der FSDT-Installer wurde nicht vollständig eingerichtet.")
        if identity(prefix) != state["original_prefix_id"] or (root / "runner").resolve() != runner:
            raise core.PatchError("Die Runtime hat sich während der GSX-Einrichtung geändert.")
        state.update(state="committing", staged_prefix_id=identity(staged))
        core.write_json(root / MARKER, state)
        os.rename(prefix, backup)
        os.rename(staged, prefix)
        state.update(state="ready")
        state.pop("prior", None)
        core.write_json(root / MARKER, state)
        progress("FSDT ist vorbereitet. Öffne den Installer, um GSX zu installieren und zu aktivieren.")
    except BaseException:
        # Keep the journal and copies for explicit recovery, including a crash
        # between the two renames. Never remove a possibly active profile.
        raise


def recover(root):
    state = read_marker(root)
    if state is None or state["state"] == "ready":
        raise core.PatchError("Keine unterbrochene GSX-Einrichtung vorhanden.")
    prefix = root / "local/msfs-prefix"
    staged = root / "local" / (".gsx-prefix-" + state["id"])
    if staged.exists():
        owned_directory(staged, root)
        # Also recover after the launcher was killed with a staging installer
        # still alive. This is never the user's live Wine server.
        with (root / "private" / ("gsx-recovery-" + uuid.uuid4().hex + ".log")).open("xb") as log:
            core.Wine(staged, (root / "runner").resolve(), log).stop()
    backup = root / "local" / ("msfs-prefix.before-gsx-" + state["id"])
    if backup.exists():
        owned_directory(backup, root)
        if identity(backup) != state.get("original_prefix_id"):
            raise core.PatchError("Die GSX-Sicherung hat sich geändert; Wiederherstellung abgebrochen.")
        if prefix.exists():
            owned_directory(prefix, root)
            if identity(prefix) != state.get("staged_prefix_id"):
                raise core.PatchError("Die GSX-Sicherung hat sich geändert; Wiederherstellung abgebrochen.")
            os.rename(prefix, root / "local" / ("msfs-prefix.after-gsx-" + uuid.uuid4().hex))
        os.rename(backup, prefix)
    elif not prefix.is_dir() or identity(prefix) != state.get("original_prefix_id"):
        raise core.PatchError("Die GSX-Sicherung hat sich geändert; Wiederherstellung abgebrochen.")
    # A partially prepared copy stays available for diagnosis, never for play.
    if state.get("prior"):
        core.write_json(root / MARKER, state["prior"])
    else:
        (root / MARKER).unlink()


def community_package(root):
    locations, limited = mods._locations(root)
    paths = {p.resolve() for p, _ in locations}
    if limited or len(paths) != 1:
        return None
    package = next(iter(paths)) / "fsdreamteam-gsx-pro"
    if not package.is_dir():
        return None
    value = json.loads(mods._read(package / "manifest.json", mods.MANIFEST_LIMIT))
    return package if isinstance(value, dict) and value.get("title") and value.get("package_version") else None


def startup_entry(root, directory):
    """Use only the entry written by FSDT; do not guess boot arguments."""
    if directory is None:
        return None
    prefix = root / "local/msfs-prefix"
    users, limited = mods._children(prefix / "drive_c/users", mods.MAX_USERS)
    if limited:
        raise core.PatchError("Die FSDT-Starteinstellung ist nicht eindeutig. Im FSDT-Installer aktualisieren.")
    family = mods._family(root)
    found = []
    for user in users:
        folders = [Path(user.path) / "AppData/Roaming" / games.for_runtime(root).user_config]
        if family:
            folders.append(Path(user.path) / "AppData/Local/Packages" / family / "LocalCache")
        for folder in folders:
            path = folder / "exe.xml"
            if not path.exists():
                continue
            core.contained(prefix, str(path.relative_to(prefix)))
            raw = mods._read(path, mods.CONFIG_LIMIT)
            declarations = raw.replace(b"\x00", b"").upper()
            if b"<!DOCTYPE" in declarations or b"<!ENTITY" in declarations:
                raise core.PatchError("Die FSDT-Starteinstellung ist nicht eindeutig. Im FSDT-Installer aktualisieren.")
            tree = ET.fromstring(raw, parser=ET.XMLParser(target=ET.TreeBuilder(insert_comments=True)))
            for addon in tree.findall("Launch.Addon"):
                value = (addon.findtext("Path") or "").strip().strip('"')
                if value.replace("\\", "/").rsplit("/", 1)[-1].casefold() not in COMPANIONS:
                    continue
                executable = mods._configured_path(value, prefix)
                if not executable.is_relative_to(directory.resolve()):
                    raise core.PatchError("Der GSX-Start verweist auf eine andere Installation.")
                core.regular(executable, 180 * 1024 * 1024)
                found.append((path, tree, addon, raw))
    if len(found) > 1:
        raise core.PatchError("Die FSDT-Starteinstellung ist nicht eindeutig. Im FSDT-Installer aktualisieren.")
    return found[0] if found else None


def configure(root, enabled):
    directory = manager_directory(root / "local/msfs-prefix")
    if not community_package(root):
        raise core.PatchError("Installiere GSX zuerst im FSDT-Installer und prüfe die Community-Verknüpfung.")
    entry = startup_entry(root, directory)
    if entry is None:
        raise core.PatchError("Der FSDT-Start fehlt. Führe im FSDT-Installer ein Update aus und prüfe erneut.")
    path, tree, addon, raw = entry
    if (tree.findtext("Disabled") or "False").strip().casefold() == "true":
        raise core.PatchError("Der automatische Add-on-Start ist in exe.xml ausgeschaltet.")
    backup = root / "private" / ("gsx-exe-" + uuid.uuid4().hex + ".xml")
    core.atomic(backup, raw)
    disabled = addon.find("Disabled")
    if disabled is None:
        disabled = ET.SubElement(addon, "Disabled")
    disabled.text = "False" if enabled else "True"
    core.atomic(path, ET.tostring(tree, encoding="utf-8", xml_declaration=True))
    core.write_json(root / STARTUP, {"format": 1, "enabled": enabled})


def snapshot(root):
    value = {"state": "unavailable", "message": "GSX ist zunächst für MSFS 2024 verfügbar.",
             "prepared": False, "package_installed": False, "startup_found": False,
             "configured": False, "can_recover": False, "idle": False, "verified_in_simulator": False}
    if root is None:
        return value
    try:
        root = core.runtime_path(root, recovery=True)
        state = read_marker(root)
        if state and state["state"] != "ready":
            value.update(state="interrupted", can_recover=True,
                         message="Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.")
        else:
            prefix = root / "local/msfs-prefix"
            directory = manager_directory(prefix)
            prepared = directory is not None and core.has_framework(prefix)
            package = community_package(root) is not None
            entry = startup_entry(root, directory)
            enabled = bool(entry and (entry[2].findtext("Disabled") or "False").strip().casefold() != "true"
                           and (entry[1].findtext("Disabled") or "False").strip().casefold() != "true")
            setting = core.read_json(root / STARTUP) if (root / STARTUP).exists() else {}
            configured = isinstance(setting, dict) and setting.get("format") == 1 and setting.get("enabled") is True
            value.update(state="available", prepared=prepared, package_installed=package,
                         startup_found=entry is not None, configured=bool(prepared and package and enabled and configured),
                         message="Installation und Lizenz prüft FSDT. Der GSX-Flugbetrieb unter Linux ist noch nicht bestätigt.")
        try:
            core.ensure_idle(root / "local/msfs-prefix")
            value["idle"] = True
        except core.PatchError:
            pass
    except (OSError, ValueError, KeyError, core.PatchError, ET.ParseError) as error:
        value.update(state="unavailable", message=str(error))
    return value
