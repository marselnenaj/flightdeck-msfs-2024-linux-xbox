# SPDX-License-Identifier: MIT
"""Per-installation OpenXR integration for direct Wine starts, on AMD and NVIDIA.

Discovery never loads a driver. Explicit checks and enabled starts use an isolated
OpenXR/Vulkan probe. No host runtime registration or driver settings are changed.
"""
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import tempfile
import threading

from .backend import LauncherError, atomic_json, utc_now

MODES = {"off", "auto", "wivrn", "steamvr", "monado"}
SETTINGS = "vr-settings.json"
MESSAGES = {
    "off": "VR ist ausgeschaltet. Der Simulator startet wie bisher.",
    "detected": "VR-System gefunden. Verbinde das Headset und prüfe die Verbindung.",
    "missing": "Kein passendes VR-System gefunden. Starte WiVRn, SteamVR oder Monado und verbinde dein Headset.",
    "invalid": "Die OpenXR-Konfiguration ist nicht nutzbar. Wähle die aktive Runtime in deinem VR-Programm erneut aus.",
    "bridge_missing": "Im gewählten Runner fehlen OpenXR-Komponenten. Verwende den mit Flightdeck eingerichteten Runner.",
    "checking": "VR-System und Headset werden geprüft …",
    "ready": "OpenXR erreicht das Headset und dessen Vulkan-Grafikkarte. Bild und Tracking prüfst du anschließend im Simulator.",
    "headset_missing": "Die VR-Runtime antwortet, aber kein Headset ist verfügbar. Verbinde oder aktiviere das Headset und prüfe erneut.",
    "loader_missing": "Der Linux-OpenXR-Loader fehlt oder kann nicht geladen werden. Installiere die OpenXR-Pakete deiner Distribution.",
    "timeout": "Das VR-System antwortet nicht. Starte es mit verbundenem Headset neu und prüfe erneut.",
    "failed": "OpenXR oder Vulkan konnte nicht initialisiert werden. Prüfe dein VR-Programm und den Grafiktreiber.",
    "cancelled": "VR-Prüfung abgebrochen.",
}


def read_json(path, limit=65536, *, nofollow=False):
    flags = os.O_RDONLY | os.O_NONBLOCK | (os.O_NOFOLLOW if nofollow else 0)
    with os.fdopen(os.open(path, flags), "rb") as stream:
        meta = os.fstat(stream.fileno())
        if not stat.S_ISREG(meta.st_mode) or meta.st_size > limit:
            raise ValueError("invalid JSON file")
        return json.loads(stream.read(limit + 1))


def settings(runtime):
    if runtime is None:
        return {"mode": "off"}
    try:
        value = read_json(runtime / "private" / SETTINGS, 4096, nofollow=True)
        if not isinstance(value, dict) or value.get("schema") != 1 or not isinstance(value.get("mode"), str) or value["mode"] not in MODES:
            raise ValueError()
        return {"mode": value["mode"]}
    except FileNotFoundError:
        return {"mode": "off"}
    except (OSError, ValueError):
        raise LauncherError("Die VR-Einstellungen sind ungültig. Bitte unter Einrichtung erneut speichern.") from None


def _absolute(value):
    return isinstance(value, str) and 0 < len(value) <= 4096 and value.startswith("/") and not any(ord(c) < 32 for c in value)


def discover(environment=None):
    env = os.environ if environment is None else environment
    home = Path(env.get("HOME", str(Path.home())))
    config = Path(env.get("XDG_CONFIG_HOME") or home / ".config")
    data = Path(env.get("XDG_DATA_HOME") or home / ".local/share")
    paths = []
    explicit = env.get("XR_RUNTIME_JSON")
    if explicit:
        paths.append((Path(explicit), True))
    config_roots = [config, *[Path(p) for p in env.get("XDG_CONFIG_DIRS", "/etc/xdg").split(":") if _absolute(p)], Path("/etc")]
    for root in config_roots:
        for name in ("active_runtime.x86_64.json", "active_runtime.json"):
            paths.append((root / "openxr/1" / name, True))
    for root in [data, Path("/usr/local/share"), Path("/usr/share")]:
        folder = root / "openxr/1"
        try:
            paths.extend((p, p.name.startswith("active_runtime")) for p in sorted(folder.glob("*.json"))[:32])
        except OSError:
            pass
    openvr = env.get("VR_PATHREG_OVERRIDE") or str(config / "openvr/openvrpaths.vrpath")
    try:
        raw = read_json(openvr)
        for root in raw.get("runtime", [])[:8]:
            if _absolute(root):
                paths.append((Path(root) / "steamxr_linux64.json", False))
    except (OSError, ValueError, TypeError, AttributeError):
        pass
    for root in (data / "Steam", home / ".steam/steam", home / ".steam/root"):
        paths.append((root / "steamapps/common/SteamVR/steamxr_linux64.json", False))
    candidates, seen, active_seen = [], set(), False
    for path, active in paths:
        is_explicit = explicit is not None and path == Path(explicit)
        is_active = False
        try:
            if not path.exists() and not path.is_symlink() and not is_explicit:
                continue
            is_active = active and not active_seen
            if active:
                active_seen = True
            if is_explicit and not _absolute(explicit):
                raise ValueError()
            resolved = path.resolve(strict=True)
            if resolved in seen:
                continue
            seen.add(resolved)
            value = read_json(resolved)
            lib = value["runtime"]["library_path"]
            if not isinstance(lib, str) or not lib or len(lib) > 4096 or any(ord(x) < 32 for x in lib):
                raise ValueError()
            # Relative libraries are relative to the manifest, as in the loader.
            if "/" in lib and not (Path(lib) if lib.startswith("/") else resolved.parent / lib).is_file():
                raise ValueError()
            if lib.lower().endswith(".dll") or ":" in lib:
                raise ValueError()
            identity = (str(resolved) + " " + lib + " " + str(value["runtime"].get("name", ""))).lower()
            provider = next((name for name in ("wivrn", "steamvr", "monado") if name in identity), "other")
            candidates.append({"provider": provider, "path": str(resolved), "active": is_active, "valid": True})
        except (OSError, ValueError, KeyError, TypeError, AttributeError, RuntimeError):
            candidates.append({"provider": "other", "path": str(path), "active": is_active, "valid": False})
    return candidates


def choose(mode, environment=None):
    candidates = discover(environment)
    if mode == "auto":
        active = next((c for c in candidates if c["active"]), None)
        if active:
            return active
        valid = [c for c in candidates if c["valid"]]
        return valid[0] if len(valid) == 1 else None
    return next((c for c in candidates if c["provider"] == mode and c["valid"]), None)


def bridge_available(runtime):
    return bool(runtime and all((runtime / relative).is_file() for relative in (
        "runner/files/bin/wine", "runner/files/lib/wine/x86_64-windows/wineopenxr.dll",
        "runner/files/lib/wine/x86_64-unix/wineopenxr.so")))


def snapshot(runtime):
    result = {"mode": "off", "available": runtime is not None, "state": "off", "providers": [], "error": ""}
    try:
        result.update(settings(runtime))
        candidates = discover()
        result["providers"] = sorted({c["provider"] for c in candidates if c["valid"]})
        if result["mode"] != "off":
            selected = choose(result["mode"])
            result["state"] = ("bridge_missing" if not bridge_available(runtime) else
                               "missing" if selected is None else "detected" if selected["valid"] else "invalid")
    except LauncherError as error:
        result["state"], result["error"] = "invalid", str(error)
    result["message"] = MESSAGES[result["state"]]
    return result


def run_child(command, environment, *, timeout=15, cancelled=None):
    # Driver crashes, hangs and verbose output must not affect the local service.
    with tempfile.TemporaryFile() as output:
        child = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL,
                                 stdout=output, stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
        import time
        deadline = time.monotonic() + timeout
        try:
            while child.poll() is None:
                if cancelled is not None and cancelled.is_set():
                    return {"state": "cancelled"}
                if time.monotonic() >= deadline or output.tell() > 1024 * 1024:
                    return {"state": "timeout"}
                time.sleep(.05)
            output.seek(0)
            return {"returncode": child.returncode, "output": output.read(1024 * 1024)}
        finally:
            if child.poll() is None:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            child.wait()


def probe(selected, environment=None, cancelled=None):
    env = host_environment(os.environ if environment is None else environment)
    env["XR_RUNTIME_JSON"] = selected["path"]
    try:
        output = run_child([sys.executable, str(Path(__file__).with_name("vr_probe.py"))], env, cancelled=cancelled)
        if "state" in output:
            return output
        if output["returncode"] != 0:
            return {"state": "failed"}
        lines = [line for line in output["output"].splitlines() if line.startswith(b"FLIGHTDECK_XR_RESULT=")]
        result = json.loads(lines[-1].split(b"=", 1)[1])
        if result.get("state") not in {"ready", "failed", "headset_missing", "loader_missing"}:
            raise ValueError()
        if result["state"] == "ready":
            for key in ("vendor_id", "device_id"):
                if type(result.get(key)) is not int or not 0 <= result[key] < 2 ** 32:
                    raise ValueError()
            if not re.fullmatch(r"[0-9a-f]{32}", result.get("device_uuid", "")):
                raise ValueError()
            for key in ("instance_extensions", "device_extensions"):
                value = result.get(key)
                if not isinstance(value, list) or len(value) > 256 or any(not isinstance(n, str) or not re.fullmatch(r"VK_[A-Za-z0-9_]{1,200}", n) for n in value):
                    raise ValueError()
        return result
    except (OSError, ValueError, IndexError, TypeError, AttributeError):
        return {"state": "failed"}


def public_result(result):
    # Exclude paths, device UUIDs and arbitrary runtime/driver output.
    return {"state": result["state"], "message": MESSAGES[result["state"]], "checked_at": utc_now()}


def host_environment(environment):
    """Retain OpenVR registration when runtime-env.sh isolates the game's XDG dirs."""
    env = dict(environment)
    config = Path(env.get("XDG_CONFIG_HOME") or Path(env.get("HOME", str(Path.home()))) / ".config")
    registration = env.get("VR_PATHREG_OVERRIDE") or str(config / "openvr/openvrpaths.vrpath")
    if _absolute(registration) and Path(registration).is_file():
        env.setdefault("VR_PATHREG_OVERRIDE", registration)
    return env


def _managed_directory(root, relative):
    current = root
    for part in Path(relative).parts:
        current = current / part
        current.mkdir(mode=0o700, exist_ok=True)
        info = current.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
            raise LauncherError("Der VR-Einrichtungsordner ist ungültig.")
    return current


def prepare(runtime, environment):
    env = dict(environment)
    mode = settings(runtime)["mode"]
    if mode == "off":
        return env
    env = host_environment(env)
    if not bridge_available(runtime):
        raise LauncherError(MESSAGES["bridge_missing"])
    selected = choose(mode, env)
    if not selected or not selected["valid"]:
        raise LauncherError(MESSAGES["missing" if selected is None else "invalid"])
    if any(env.get(name) for name in ("DXVK_FILTER_DEVICE_NAME", "VKD3D_FILTER_DEVICE_NAME", "VKD3D_VULKAN_DEVICE")):
        raise LauncherError("Entferne manuelle GPU-Namens- und Indexfilter, damit Spiel und VR-System dieselbe Grafikkarte verwenden können.")
    result = probe(selected, env)
    if result["state"] != "ready":
        raise LauncherError(MESSAGES[result["state"]])
    folder = _managed_directory(runtime, "private/vr")
    manifest = folder / "wineopenxr64.json"
    atomic_json(manifest, {"file_format_version": "1.0.0", "runtime": {"library_path": r"C:\windows\system32\wineopenxr.dll"}})
    # Wine preserves XR_* for the Unix loader and imports WINEXR_* for Windows.
    # This keeps the Linux runtime distinct from wineopenxr's Windows manifest.
    env["XR_RUNTIME_JSON"] = selected["path"]
    env["WINEXR_RUNTIME_JSON"] = "Z:" + str(manifest).replace("/", "\\")
    # Both D3D and XR must use the physical device chosen by the compositor.
    uuid = result["device_uuid"]
    if uuid != "0" * 32:
        explicit = environment.get("DXVK_FILTER_DEVICE_UUID")
        if explicit and explicit.lower().replace("-", "") != uuid:
            raise LauncherError("Spiel und VR-System verwenden verschiedene Grafikkarten. Wähle dieselbe GPU und prüfe erneut.")
        env["DXVK_FILTER_DEVICE_UUID"] = uuid
    # Initialize wineopenxr's registry contract without starting Steam's game
    # loader, which cannot preserve Xodus' inherited Store file descriptors.
    instance_ext = list(dict.fromkeys(result["instance_extensions"] + ["VK_KHR_surface", "VK_KHR_win32_surface"]))
    registry = ('Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\VR]\n'
                '"state"=dword:00000001\n"is_hmd_present"=dword:00000001\n'
                '"openxr_vulkan_instance_extensions"="' + " ".join(instance_ext) + '"\n'
                '"openxr_vulkan_device_extensions"="' + " ".join(result["device_extensions"]) + '"\n'
                f'"openxr_vulkan_device_vid"=dword:{result["vendor_id"]:08x}\n'
                f'"openxr_vulkan_device_pid"=dword:{result["device_id"]:08x}\n')
    fd, name = tempfile.mkstemp(prefix=".vr-", suffix=".reg", dir=folder)
    try:
        with os.fdopen(fd, "w", encoding="utf-16") as stream:
            stream.write(registry)
        child_env = dict(env, WINEPREFIX=str(runtime / "local/msfs-prefix"), WINEDEBUG="-all", WINEESYNC="0", WINEFSYNC="0")
        child_env.pop("WINE_DLL_FILE_MAP", None)
        from .setup import runner_wine
        applied = run_child([str(runner_wine(runtime / "runner")), "regedit", "/S", name], child_env, timeout=20)
        if applied.get("returncode") != 0:
            raise LauncherError("Die OpenXR-Anbindung konnte nicht eingerichtet werden. Beende Programme dieser Spielumgebung und versuche es erneut.")
    except OSError:
        raise LauncherError("Die OpenXR-Anbindung konnte nicht eingerichtet werden. Beende Programme dieser Spielumgebung und versuche es erneut.") from None
    finally:
        Path(name).unlink(missing_ok=True)
    return env


class VRManager:
    def __init__(self, launcher):
        self.launcher, self.thread = launcher, None
        self.lock = threading.Lock()
        self.cancelled = threading.Event()
        self.result, self.runtime, self.mode = None, None, None

    def snapshot(self):
        with self.lock:
            value = snapshot(self.launcher.runtime)
            if self.runtime == self.launcher.runtime and self.mode == value["mode"]:
                value["check"] = copy.deepcopy(self.result)
            return value

    def configure(self, runtime_path, mode):
        with self.launcher.lock:
            self._idle(runtime_path)
            if not isinstance(mode, str) or mode not in MODES:
                raise LauncherError("Bitte einen gültigen VR-Modus auswählen.")
            with self.launcher.runtime_lock():
                atomic_json(self.launcher.runtime / "private" / SETTINGS, {"schema": 1, "mode": mode})
                with self.lock:
                    self.result = None
            return {"ok": True}

    def _idle(self, runtime_path):
        self.launcher.require_open()
        if self.launcher.runtime is None or runtime_path != str(self.launcher.runtime):
            raise LauncherError("Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.")
        self.launcher._poll()
        if self.launcher.setup_busy or self.launcher.process is not None or self.launcher._external():
            raise LauncherError("Beende zuerst Spiel, Cloud-Abgleich und laufende Einrichtungen.")
        self.launcher._require_cloud_idle(allow_attention=True)

    def check(self, runtime_path):
        with self.launcher.lock:
            self._idle(runtime_path)
            mode = settings(self.launcher.runtime)["mode"]
            if mode == "off":
                raise LauncherError("Aktiviere zuerst VR und speichere den Modus.")
            self.launcher.reserve_setup()
            with self.lock:
                self.runtime, self.mode = self.launcher.runtime, mode
                self.result = public_result({"state": "checking"})
                self.cancelled.clear()
                self.thread = threading.Thread(target=self._check, args=(self.runtime, mode), daemon=False)
                try:
                    self.thread.start()
                except RuntimeError:
                    self.thread = None
                    self.result = public_result({"state": "failed"})
                    self.launcher.release_setup()
                    raise LauncherError(MESSAGES["failed"]) from None
            return {"ok": True}

    def _check(self, runtime, mode):
        result = {"state": "failed"}
        try:
            with self.launcher.runtime_lock(operation="vr-check"):
                selected = choose(mode)
                result = ({"state": "bridge_missing"} if not bridge_available(runtime) else
                          {"state": "missing"} if selected is None else
                          {"state": "invalid"} if not selected["valid"] else probe(selected, cancelled=self.cancelled))
        except (OSError, ValueError, LauncherError):
            pass
        finally:
            with self.lock:
                self.result = public_result(result)
            self.launcher.release_setup()

    def close(self):
        self.cancelled.set()
        if self.thread:
            self.thread.join(timeout=20)
