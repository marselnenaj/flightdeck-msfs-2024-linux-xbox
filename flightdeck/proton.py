# SPDX-License-Identifier: MIT
"""Proton selection with persistent add-ons and recoverable, private runner copies."""
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import threading
import uuid

from .backend import LauncherError, atomic_json
from .graphics_diagnostics import _read
from .maintenance import _directory, _id, _idle
from .setup import SetupError, SetupCancelled, _copy_file, _copy_prefix, digest, interrupted, prefix_system32, runner_wine, relocate_prefix_links
from ._fenix.core import PatchError

SETTINGS = "proton-selection.json"
JOURNAL = "proton-switch.json"
BACKUP = r"local/proton-tests/[0-9a-f]{32}/previous-prefix"
RUNNER = r"local/(?:proton-tests/[0-9a-f]{32}|fenix-patch-[0-9T]+-[0-9a-f]{8})/runner"
BRIDGE = ("xgameruntime.dll", "xgameruntime_original.dll", "xodus_store_test.dll")


def _json(path):
    return json.loads(_read(path, 65536))


def _managed_directory(runtime, path):
    current = runtime
    for name in path.relative_to(runtime).parts:
        current /= name
        _directory(current)
    return path


def _flush(*directories):
    for path in directories:
        fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)


def _selection(value):
    if (not isinstance(value, dict) or value.get("schema") not in (1, 2)
            or not isinstance(value.get("runner"), str) or not re.fullmatch(RUNNER, value["runner"])
            or not isinstance(value.get("base_prefix"), str) or not re.fullmatch(BACKUP, value["base_prefix"])
            or not isinstance(value.get("base_runner"), str) or not Path(value["base_runner"]).is_absolute()
            or not isinstance(value.get("version"), str) or len(value["version"]) > 256
            or not isinstance(value.get("base_hashes"), dict) or set(value["base_hashes"]) != set(BRIDGE)
            or any(not isinstance(v, str) or not re.fullmatch(r"[0-9a-f]{64}", v) for v in value["base_hashes"].values())):
        raise ValueError("Invalid Proton selection")
    return value


def selection(runtime):
    if runtime is None:
        return None
    try:
        return _selection(_json(runtime / "private" / SETTINGS))
    except FileNotFoundError:
        return None


def diagnostic(runtime):
    try:
        selected = selection(runtime)
        version = selected["version"] if selected else _version(runtime / "runner") if runtime else "unknown"
        if not re.fullmatch(r"(?:experimental|GE-Proton|cachyos|xodus|Proton|proton|\d)[A-Za-z0-9_. +()-]{0,127}", version):
            version = "custom"
        return {"mode": "proton" if selected else "flightdeck", "version": version,
                "loader": "portable" if selected else "native"}
    except (OSError, ValueError, TypeError):
        return {"mode": "unavailable", "version": "unknown", "loader": "unknown"}


def base_runner(runtime):
    selected = selection(runtime)
    return Path(selected["base_runner"]) if selected else runtime / "runner"


def check(runtime):
    if runtime is None:
        return ""
    try:
        if (runtime / "private" / JOURNAL).exists() or (runtime / "private" / JOURNAL).is_symlink():
            return "Proton-Wechsel unterbrochen. Unter Proton-Version zu Flightdeck zurückkehren."
        selected = selection(runtime)
        if selected and ((runtime / "runner").resolve() != runtime / selected["runner"]
                         or not (runtime / selected["runner"] / "files/bin/wine").is_file()):
            raise ValueError()
        return ""
    except (OSError, ValueError, TypeError):
        return "Die Proton-Auswahl ist ungültig. Stelle unter Proton-Version die Flightdeck-Umgebung wieder her."


def _version(root):
    try:
        text = _read(root / "version", 4096).decode().strip()
        text = re.sub(r"^\d+\s+", "", text)
        if text and len(text) <= 256 and not any(ord(c) < 32 for c in text):
            return text
    except (OSError, ValueError, UnicodeError):
        pass
    return root.name[:256]


def inspect(path):
    if not isinstance(path, str) or not path.strip() or len(path) > 4096:
        raise LauncherError("Wähle einen installierten Proton-Ordner.")
    root = Path(path).expanduser().resolve(strict=True)
    files = root / "files"
    if not files.is_dir():
        files = root / "dist"
    names = ["bin/wine", "bin/wineserver"]
    if (files / "bin/wine64").is_file():
        names.append("bin/wine64")
    for architecture in ("x86_64-windows", "i386-windows"):
        for library, dlls in (("dxvk", ("dxgi", "d3d11", "d3d10core")),
                              ("vkd3d-proton", ("d3d12", "d3d12core"))):
            names += [f"lib/wine/{library}/{architecture}/{name}.dll" for name in dlls]
    hashes = {}
    for name in names:
        target = (files / name).resolve(strict=True)
        if not target.is_relative_to(root) or not target.is_file() or target.stat().st_size > 256 * 1024**2:
            raise LauncherError("Der Proton-Ordner enthält keine vollständige Wine- und Grafikumgebung.")
        if name.startswith("bin/") and not os.access(target, os.X_OK):
            raise LauncherError("Wine ist in diesem Proton-Ordner nicht ausführbar.")
        hashes[name] = digest(target)
    return {"path": str(root), "files": files, "version": _version(root), "hashes": hashes}


def discover(home=None):
    """Read Steam's registered libraries, including Flatpak and custom tools."""
    home = Path.home() if home is None else Path(home)
    roots = {home / ".local/share/Steam", home / ".steam/root", home / ".steam/steam",
             home / ".var/app/com.valvesoftware.Steam/.local/share/Steam"}
    for root in tuple(roots):
        for relative in ("steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"):
            try:
                data = _read(root / relative, 1024 * 1024).decode()
                for path in re.findall(r'"path"\s+"((?:[^"\\]|\\.)*)"', data):
                    roots.add(Path(path.replace('\\\\', '\\').replace('\\"', '"')))
            except (OSError, ValueError, UnicodeError):
                pass
    directories = {root / sub for root in roots for sub in ("steamapps/common", "compatibilitytools.d")}
    directories.update((Path("/usr/share/steam/compatibilitytools.d"),
                        home / ".local/share/Steam/compatibilitytools.d"))
    choices = {}
    for directory in sorted(directories):
        try:
            for root in sorted(directory.iterdir())[:256]:
                resolved = root.resolve()
                files = resolved / "files"
                if not files.is_dir():
                    files = resolved / "dist"
                if ((files / "bin/wine").is_file() and (files / "bin/wineserver").is_file()
                        and (files / "lib/wine/vkd3d-proton/x86_64-windows/d3d12.dll").is_file()):
                    from .fenix import core
                    try:
                        core.runner_variant(resolved)
                        fenix = True
                    except (OSError, ValueError, core.PatchError):
                        fenix = False
                    choices[str(resolved)] = {"path": str(resolved), "label": root.name, "version": _version(resolved), "fenix": fenix}
        except OSError:
            continue
    return sorted(choices.values(), key=lambda item: item["label"].casefold())


def _prefix_env(prefix, work):
    env = {k: v for k, v in os.environ.items() if not k.startswith(("WINE", "PROTON", "DXVK", "VKD3D", "XODUS"))}
    env.update(WINEPREFIX=str(prefix), WINEARCH="win64", WINEESYNC="0", WINEFSYNC="0", WINEDEBUG="-all",
               WINE_DISABLE_FAST_SYNC="1", WINEDLLOVERRIDES="winemenubuilder.exe,mscoree,mshtml=d")
    for category in ("CONFIG", "DATA", "CACHE", "STATE"):
        path = work / "xdg" / category.lower()
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
        env["XDG_" + category + "_HOME"] = str(path)
    return env


def _bridge_hashes(runtime):
    record = _json(runtime / "private/import-manifest.json")
    expected = {"xgameruntime.dll": record["artifacts"]["files"]["runtime/xgameruntime.dll"],
                "xodus_store_test.dll": record["artifacts"]["files"]["builtin/x86_64-windows/xodus_store_test.dll"],
                "xgameruntime_original.dll": record["original_runtime_sha256"]}
    return expected


def _bridge(runtime):
    expected = _bridge_hashes(runtime)
    system = prefix_system32(runtime / "local/msfs-prefix")
    if any(digest(system / name) != value for name, value in expected.items()):
        raise LauncherError("Die Store-Komponenten passen nicht zur Installation. Aktualisiere Flightdeck zuerst.")
    return system, expected


def _prepare(runtime, candidate, cancel, notify, *, bundle=None, restoring=False):
    from . import bootstrap
    previous = selection(runtime)
    prefix = runtime / "local/msfs-prefix"
    # Every switch carries the current installation forward, including add-ons
    # installed after the first switch. Older profiles are retained as backups.
    original_prefix = prefix
    _managed_directory(runtime, original_prefix)
    original_runner = previous["base_runner"] if previous else str((runtime / "runner").resolve(strict=True))
    try:
        source, bridge_hashes = _bridge(runtime)
    except LauncherError:
        if not restoring or not previous:
            raise
        source = prefix_system32(_managed_directory(runtime, runtime / previous["base_prefix"]))
        bridge_hashes = _bridge_hashes(runtime)
        if any(digest(source / name) != expected for name, expected in bridge_hashes.items()):
            raise
    from .fenix import core
    fenix = None
    variant = None
    marker = runtime / core.MARKER
    legacy = runtime / "private/fenix-compat.json"
    if marker.exists() or legacy.exists():
        if bundle is None:
            raise LauncherError("Das passende Fenix-Paket muss vor dem Proton-Wechsel verfügbar sein.")
        if marker.exists():
            fenix = core.read_json(marker)
            if fenix.get("state") != "installed":
                raise LauncherError("Beende oder repariere zuerst die Fenix-Einrichtung.")
            core.verify_installed(runtime, fenix)
        else:
            fenix = {"format": 1, "state": "installed", "migrated": True, "configured": True,
                     "proton_before": None}
            if not core.has_framework(prefix):
                raise LauncherError("Die bestehende Fenix-Umgebung enthält kein vollständiges .NET Framework.")
        try:
            variant = core.runner_variant(Path(candidate["path"]))
        except core.PatchError:
            if not restoring:
                raise
            variant = core.runner_variant(Path(candidate["path"]), patched=True)
    tests = runtime / "local/proton-tests"
    tests.mkdir(mode=0o700, exist_ok=True)
    _directory(tests)
    work = tests / uuid.uuid4().hex
    work.mkdir(mode=0o700)
    runner = work / "runner"
    runner.mkdir(mode=0o700)
    fresh = work / "previous-prefix"
    env = None
    try:
        scripts = {}
        if fenix is not None:
            lock = core.manifest()
            legacy_state = core.read_json(legacy) if legacy.exists() else {}
            for name in ("launch-msfs.sh", "xodus-wine-launch"):
                before = digest(core.regular(runtime / "tools" / name))
                accepted = {*lock["accepted_scripts"][name], lock["integration"][name]}
                for release in lock.get("previous_releases", {}).values():
                    accepted.add(release["integration"][name])
                accepted.add(legacy_state.get("deployed_scripts", {}).get(name))
                if before not in accepted:
                    raise LauncherError("Ein Fenix-Startskript wurde angepasst. Die bestehende Installation wurde nicht verändert.")
                _copy_file(runtime / "tools" / name, work / ("before-" + name))
                _copy_file(bundle / "integration" / name, work / name)
                scripts[name] = {"before": before, "after": lock["integration"][name],
                                 "source": str((work / name).relative_to(runtime))}
        notify("Proton und Windows-Umgebung werden unabhängig kopiert …")
        _copy_prefix(candidate["files"], runner / "files", cancel)
        (runner / "version").write_text(candidate["version"] + "\n")
        if (inspect(str(runner))["hashes"] != candidate["hashes"]
                or inspect(candidate["path"])["hashes"] != candidate["hashes"]):
            raise LauncherError("Proton wurde während der Vorbereitung aktualisiert. Bitte erneut auswählen.")
        _copy_prefix(original_prefix, fresh, cancel)
        relocate_prefix_links(original_prefix, fresh)
        env = _prefix_env(fresh, work)
        env.update(WINELOADER=str(runner_wine(runner)), WINESERVER=str(runner / "files/bin/wineserver"))
        notify("Die neue Proton-Umgebung wird eingerichtet …")
        bootstrap._command([runner_wine(runner), "wineboot", "-u"], env=env, cancel=cancel)
        bootstrap._command([runner / "files/bin/wineserver", "-w"], env=env, cancel=cancel)
        bootstrap.install_graphics(runner, fresh, env=env, cancel=cancel)
        for name, expected in bridge_hashes.items():
            _copy_file(source / name, prefix_system32(fresh) / name)
            if digest(prefix_system32(fresh) / name) != expected:
                raise ValueError("Store component changed")
        bootstrap._command([runner / "files/bin/wineserver", "-w"], env=env, cancel=cancel)
        if fenix is not None:
            notify("Fenix wird an die gewählte Proton-Version angepasst …")
            with (work / "fenix-setup.log").open("xb") as log:
                wine = core.Wine(fresh, runner, log)
                try:
                    core.prepare_geometry(wine, runtime / "private/fenix-downloads", bundle, notify)
                finally:
                    wine.stop()
            core.apply_overlay(fresh, runner, bundle, variant)
            fenix = {**fenix, "variant": variant, "version": core.manifest()["version"],
                     "work": str(work.relative_to(runtime))}
            if fenix.get("migrated"):
                fenix["backup"] = str(work.relative_to(runtime))
        selected = {"schema": 2 if fenix is not None else 1, "version": candidate["version"], "source": candidate["path"],
                    "runner": str(runner.relative_to(runtime)), "base_runner": original_runner,
                    "base_prefix": previous["base_prefix"] if previous else str(fresh.relative_to(runtime)),
                    "base_hashes": previous["base_hashes"] if previous else bridge_hashes,
                    "fenix_state": fenix, "scripts": scripts}
        atomic_json(work / "provenance.json", {"source": candidate["path"], "version": candidate["version"],
                                               "files": candidate["hashes"], "previous_runner": str((runtime / "runner").resolve())})
        interrupted(cancel)
        return fresh, selected
    except BaseException:
        if env is not None:
            try:
                subprocess.run([str(runner / "files/bin/wineserver"), "-k"], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
            except (OSError, subprocess.SubprocessError):
                # Keep this isolated staging tree if its processes could not
                # be confirmed stopped. The active prefix was never changed.
                raise
        shutil.rmtree(work)
        raise


def recover(runtime):
    from .game_update import exchange
    from .fenix import core
    journal = runtime / "private" / JOURNAL
    if not journal.exists() and not journal.is_symlink():
        return
    data = _json(journal)
    if (data.get("schema") != 1 or not isinstance(data.get("backup"), str)
            or not re.fullmatch(BACKUP, data["backup"]) or not Path(data.get("runner", "")).is_absolute()):
        raise ValueError("Invalid Proton recovery record")
    if data.get("selection") is not None:
        _selection(data["selection"])
        if data["runner"] != str(runtime / data["selection"]["runner"]):
            raise ValueError("Inconsistent Proton recovery runner")
    fenix = data.get("fenix_state")
    if fenix is not None:
        from .fenix import core
        if (not isinstance(fenix, dict) or fenix.get("state") != "installed"
                or not re.fullmatch(RUNNER, str(fenix.get("work")) + "/runner")
                or str(runtime / fenix["work"] / "runner") != data["runner"]):
            raise ValueError("Invalid Fenix recovery record")
        core.manifest(fenix.get("variant"))
    scripts = data.get("scripts", {})
    if not isinstance(scripts, dict) or set(scripts) - {"launch-msfs.sh", "xodus-wine-launch"}:
        raise ValueError("Invalid Proton scripts")
    for name, item in scripts.items():
        if (not isinstance(item, dict) or not isinstance(item.get("source"), str)
                or not re.fullmatch(r"local/proton-tests/[0-9a-f]{32}/" + re.escape(name), item["source"])):
            raise ValueError("Invalid Proton script path")
        _managed_directory(runtime, (runtime / item["source"]).parent)
        if digest(core.regular(runtime / item["source"])) != item.get("after"):
            raise ValueError("Prepared Proton script changed")
        if digest(core.regular(runtime / "tools" / name)) not in (item.get("before"), item["after"]):
            raise ValueError("Active Proton script changed")
    for key in ("before", "after"):
        if (not isinstance(data.get(key), list) or len(data[key]) != 2
                or any(type(item) is not int or item < 0 for item in data[key])):
            raise ValueError("Invalid Proton prefix identity")
    prefix, backup = runtime / "local/msfs-prefix", runtime / data["backup"]
    for folder in (prefix, backup):
        _managed_directory(runtime, folder)
    if (not (runtime / "runner").is_symlink()
            or str((runtime / "runner").resolve()) not in {data.get("previous_runner"), data["runner"]}):
        raise ValueError("Proton runner link changed")
    if _id(prefix) == data["before"] and _id(backup) == data["after"]:
        exchange(prefix, backup)
    elif _id(prefix) != data["after"] or _id(backup) != data["before"]:
        raise ValueError("Proton prefix identity changed")
    _flush(prefix.parent, backup.parent)
    temporary = runtime / (".proton-runner-" + uuid.uuid4().hex)
    try:
        temporary.symlink_to(data["runner"], target_is_directory=True)
        os.replace(temporary, runtime / "runner")
        _flush(runtime)
    finally:
        temporary.unlink(missing_ok=True)
    target = runtime / "private" / SETTINGS
    if data["selection"] is None:
        target.unlink(missing_ok=True)
    else:
        atomic_json(target, data["selection"])
    if fenix is not None:
        atomic_json(runtime / core.MARKER, fenix)
    if scripts:
        imported = _json(runtime / "private/import-manifest.json")
        for name, item in scripts.items():
            core.atomic(runtime / "tools" / name, core.regular(runtime / item["source"]).read_bytes(), 0o700)
            if "runtime_files" in imported:
                imported["runtime_files"][name] = item["after"]
        atomic_json(runtime / "private/import-manifest.json", imported)
    _flush(target.parent)
    journal.unlink()
    _flush(target.parent)


def _switch(runtime, backup, selected, *, runner=None, fenix_state=None, scripts=None):
    runner = runner or (str(runtime / selected["runner"]) if selected else selection(runtime)["base_runner"])
    atomic_json(runtime / "private" / JOURNAL,
                {"schema": 1, "backup": str(backup.relative_to(runtime)), "runner": runner,
                 "previous_runner": str((runtime / "runner").resolve()),
                 "before": _id(runtime / "local/msfs-prefix"), "after": _id(backup), "selection": selected,
                 "fenix_state": selected.get("fenix_state") if selected else fenix_state,
                 "scripts": selected.get("scripts", {}) if selected else (scripts or {})})
    _flush(runtime / "private")
    recover(runtime)


class ProtonManager:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.job = None
        self.thread = None
        self.cancel_event = threading.Event()

    def snapshot(self):
        with self.launcher.lock, self.lock:
            root = self.launcher.runtime
            try:
                selected = selection(root)
                error = check(root)
            except (OSError, ValueError, TypeError):
                selected, error = None, "Die Proton-Auswahl konnte nicht gelesen werden."
            fenix = bool(root and any((root / "private" / name).exists() for name in ("fenix-linux-patch.json", "fenix-compat.json")))
            return {"runtime_path": str(root) if root else "", "selected": selected["version"] if selected else "Flightdeck (Xodus)",
                    "experimental": selected is not None, "can_restore": bool(selected or error), "error": error,
                    "fenix": fenix, "job": copy.deepcopy(self.job)}

    def start(self, data):
        with self.launcher.lock, self.lock:
            root = self.launcher.runtime
            if root is None or data.get("runtime_path") != str(root):
                raise LauncherError("Die ausgewählte Installation hat sich geändert. Bitte erneut auswählen.")
            mode = data.get("mode")
            if mode not in {"default", "proton"}:
                raise LauncherError("Ungültige Proton-Auswahl.")
            marker = root / "private/fenix-linux-patch.json"
            if mode == "proton" and marker.exists():
                try:
                    if _json(marker).get("state") != "installed":
                        raise ValueError()
                except (OSError, ValueError, AttributeError):
                    raise LauncherError("Beende oder repariere zuerst die Fenix-Einrichtung.") from None
            if mode == "proton" and (not isinstance(data.get("path"), str) or not data["path"].strip()):
                raise LauncherError("Wähle einen installierten Proton-Ordner.")
            self.launcher._require_cloud_idle(allow_attention=mode == "default")
            from .gsx_core import setup_complete
            if not setup_complete(root):
                raise LauncherError("Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.")
            self.launcher.reserve_setup()
            self.cancel_event.clear()
            self.job = {"id": uuid.uuid4().hex, "state": "preparing", "runtime_path": str(root),
                        "message": "Proton-Auswahl wird vorbereitet …", "error": ""}
            self.thread = threading.Thread(target=self._run, args=(root, mode, data.get("path")), daemon=False)
            self.thread.start()
            return {"ok": True, **self.snapshot()}

    def _notify(self, message):
        with self.lock:
            self.job["message"] = message

    def _run(self, root, mode, path):
        try:
            with self.launcher.runtime_lock(operation="proton") as descriptor:
                _idle(self.launcher, descriptor)
                recover(root)
                candidate = inspect(path) if mode == "proton" else None
                bundle = None
                if self.snapshot()["fenix"]:
                    from .fenix import obtain_bundle, core
                    if candidate:
                        core.runner_variant(Path(candidate["path"]))
                    bundle = obtain_bundle(self.launcher.state_dir / "fenix-bundles", None, self._notify)
                if mode == "default":
                    selected = selection(root)
                    if selected:
                        backup = root / selected["base_prefix"]
                        _managed_directory(root, backup)
                        current = _bridge_hashes(root)
                        system = prefix_system32(backup)
                        if any(digest(system / name) not in {selected["base_hashes"][name], current[name]} for name in BRIDGE):
                            raise LauncherError("Die gesicherte Flightdeck-Umgebung wurde verändert. Sie wurde nicht überschrieben.")
                        if any(digest(system / name) != current[name] for name in BRIDGE):
                            source, _ = _bridge(root)
                            for name in BRIDGE:
                                _copy_file(source / name, system / name)
                        candidate = inspect(selected["base_runner"])
                        fresh, prepared = _prepare(root, candidate, self.cancel_event, self._notify, bundle=bundle, restoring=True)
                        _switch(root, fresh, None, runner=str(root / prepared["runner"]),
                                fenix_state=prepared["fenix_state"], scripts=prepared["scripts"])
                else:
                    if not (root / "runner").is_symlink():
                        raise LauncherError("Diese Installation unterstützt keinen Proton-Wechsel.")
                    if bundle is None and b"FLIGHTDECK_PROTON_LOADER" not in _read(root / "tools/launch-msfs.sh", 65536):
                        raise LauncherError("Aktualisiere zuerst die Flightdeck-Runtime-Komponenten und öffne Flightdeck erneut.")
                    fresh, selected = _prepare(root, candidate, self.cancel_event, self._notify, bundle=bundle)
                    self._notify("Proton-Umgebung wird aktiviert …")
                    _switch(root, fresh, selected)
                self.launcher.graphics_report = None
            with self.lock:
                self.job.update(state="complete", message="Proton-Auswahl bereit. Du kannst den Simulator starten.")
        except (OSError, ValueError, KeyError, TypeError, LauncherError, SetupError, PatchError, subprocess.SubprocessError) as error:
            with self.lock:
                self.job.update(state="cancelled" if isinstance(error, SetupCancelled) else "failed",
                                message="", error=str(error) if isinstance(error, (LauncherError, SetupError, PatchError)) else
                                "Die Proton-Vorbereitung ist fehlgeschlagen. Prüfe den Proton-Ordner, freien Speicherplatz und die Wine-Abhängigkeiten.")
        finally:
            with self.launcher.lock:
                self.launcher.setup_busy = False

    def cancel(self, job_id):
        with self.lock:
            if not self.job or self.job["id"] != job_id or self.job["state"] != "preparing":
                raise LauncherError("Diese Proton-Vorbereitung läuft nicht mehr.")
            self.cancel_event.set()
        return {"ok": True}
