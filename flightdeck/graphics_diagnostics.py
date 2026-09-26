# SPDX-License-Identifier: MIT
"""Bounded graphics evidence. Never export paths, registry text or raw logs."""
from __future__ import annotations

from datetime import datetime
import json
import os
import re
import stat
import uuid

from . import __version__, games, graphics

MARKER = "graphics-launch.json"
DLLS = {"dxgi", "d3d11", "d3d10core", "d3d12", "d3d12core", "nvapi", "nvapi64",
        "nvofapi64", "nvngx", "_nvngx", "nvcuda"}
MODES = {"native", "builtin", "native,builtin", "builtin,native", "disabled", "other"}
STATES = {"preparing", "preparation_failed", "prepared", "spawned", "spawn_failed"}
NVIDIA = {"ready", "disabled", "not_present", "custom_runner", "unknown"}
FILTERS = {"DXVK_FILTER_DEVICE_NAME", "DXVK_FILTER_DEVICE_UUID", "VKD3D_FILTER_DEVICE_NAME", "VKD3D_VULKAN_DEVICE"}
FILTER_KINDS = {"nvidia", "amd", "intel", "other_device", "custom"}
LIBRARIES = {
    "dxgi": ("dxvk", "dxgi"), "d3d11": ("dxvk", "d3d11"),
    "d3d10core": ("dxvk", "d3d10core"), "d3d12": ("vkd3d-proton", "d3d12"),
    "d3d12core": ("vkd3d-proton", "d3d12core"),
    "nvapi64": ("nvapi", "nvapi64"), "nvofapi64": ("nvapi", "nvofapi64"),
}
_OVERRIDE_NAMES = DLLS | {"*" + name for name in DLLS}


def _read(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ValueError()
        data = source.read(limit + 1)
        if len(data) > limit:
            raise ValueError()
        return data


def _mode(value):
    values = [part.strip().lower() for part in value.split(",")]
    aliases = {"n": "native", "b": "builtin", "d": "disabled", "": "disabled"}
    value = ",".join(aliases.get(part, part) for part in values)
    return value if value in MODES else "other"


def environment_overrides(environment):
    result = {}
    for entry in environment.get("WINEDLLOVERRIDES", "")[:65536].split(";"):
        if "=" not in entry:
            continue
        names, mode = entry.split("=", 1)
        for name in names.split(","):
            name = name.strip().lower().removesuffix(".dll")
            if name in _OVERRIDE_NAMES:
                result[name] = _mode(mode)
    return result


def launch_record(runtime, report, environment, *, state, at):
    """Only launch intent: these fields do not claim that rendering succeeded."""
    known = {device["name"]: {0x10de: "nvidia", 0x1002: "amd", 0x8086: "intel"}.get(device["vendor_id"], "other_device")
             for device in report.get("devices", [])}
    filters = {key: known.get(environment[key], "custom") for key in sorted(FILTERS) if environment.get(key)}
    result = {"schema": 1, "at": at, "launcher_version": __version__, "game_id": games.for_runtime(runtime).id,
              "state": state, "nvidia": report.get("nvidia", "unknown"),
              "gpu_filters": filters, "dll_overrides": environment_overrides(environment),
              "nvapi_mode": {"1": "enabled", "0": "disabled"}.get(environment.get("DXVK_ENABLE_NVAPI"), "unspecified")}
    if type(report.get("ngx_available")) is bool:
        result["ngx_available"] = report["ngx_available"]
    if report.get("nvidia_mode") in graphics.NVIDIA_MODES:
        result["nvidia_mode"] = report["nvidia_mode"]
    result["hide_nvidia"] = environment.get("WINE_HIDE_NVIDIA_GPU") == "1"
    return result


def _private(runtime):
    fd = os.open(runtime / "private", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    if os.fstat(fd).st_uid != os.getuid():
        os.close(fd)
        raise ValueError()
    return fd


def save_launch(runtime, record):
    """Best effort: failed diagnostics must never prevent a game from starting."""
    directory = None
    created = False
    name = ".graphics-launch-" + uuid.uuid4().hex
    try:
        directory = _private(runtime)
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory)
        created = True
        with os.fdopen(fd, "w") as target:
            json.dump(record, target)
            target.flush()
            os.fsync(target.fileno())
        os.replace(name, MARKER, src_dir_fd=directory, dst_dir_fd=directory)
        os.fsync(directory)
        return True
    except (OSError, ValueError):
        return False
    finally:
        if directory is not None:
            if created:
                try:
                    os.unlink(name, dir_fd=directory)
                except OSError:
                    pass
            os.close(directory)


def load_launch(runtime):
    directory = None
    try:
        directory = _private(runtime)
        fd = os.open(MARKER, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(fd, "rb") as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size > 8192:
                return None
            value = json.loads(source.read(8193))
        if not isinstance(value, dict) or value.get("schema") != 1:
            return None
        if (value.get("state") not in STATES or value.get("nvidia") not in NVIDIA
                or value.get("game_id") not in games.GAMES
                or value.get("nvapi_mode") not in {"enabled", "disabled", "unspecified"}
                or not re.fullmatch(r"\d{1,3}\.\d{1,3}\.\d{1,3}(?:(?:a|b|rc|\.dev|\.post)\d{1,3})?", value.get("launcher_version", ""))
                or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?\+00:00", value.get("at", ""))):
            return None
        datetime.fromisoformat(value["at"])
        result = {key: value[key] for key in ("at", "launcher_version", "game_id", "state", "nvidia", "nvapi_mode")}
        for key, names, values in (("gpu_filters", FILTERS, FILTER_KINDS), ("dll_overrides", _OVERRIDE_NAMES, MODES)):
            data = value.get(key)
            if not isinstance(data, dict) or any(k not in names or v not in values for k, v in data.items()):
                return None
            result[key] = data
        if type(value.get("ngx_available")) is bool:
            result["ngx_available"] = value["ngx_available"]
        if value.get("nvidia_mode") in graphics.NVIDIA_MODES:
            result["nvidia_mode"] = value["nvidia_mode"]
        if type(value.get("hide_nvidia")) is bool:
            result["hide_nvidia"] = value["hide_nvidia"]
        return result
    except (OSError, ValueError, TypeError, KeyError):
        return None
    finally:
        if directory is not None:
            os.close(directory)


def _registry(path, executable):
    try:
        text = _read(path, 8 * 1024**2).decode("utf-8")
    except FileNotFoundError:
        return {"status": "missing"}
    except (OSError, ValueError):
        return {"status": "unavailable"}
    sections = {r"Software\\Wine\\DllOverrides".lower(): "global",
                (r"Software\\Wine\\AppDefaults\\" + executable + r"\\DllOverrides").lower(): "game"}
    result, section = {"status": "read", "global": {}, "game": {}}, None
    for line in text.splitlines():
        if line.startswith("["):
            section = sections.get(line[1:].split("]", 1)[0].lower())
        elif section:
            match = re.fullmatch(r'"([^"\\]{1,32})"="([^"\\]{0,64})"', line)
            if match and match[1].lower().removesuffix(".dll") in _OVERRIDE_NAMES:
                result[section][match[1].lower().removesuffix(".dll")] = _mode(match[2])
    return result


def prefix_summary(runtime):
    """Compare prefix DLLs with the selected runner; do not change any file."""
    try:
        prefix = runtime / "local/msfs-prefix"
        folders = [runtime / "local", prefix, prefix / "drive_c", prefix / "drive_c/windows", prefix / "drive_c/windows/system32"]
        if any(path.is_symlink() or not path.is_dir() for path in folders):
            return {"status": "unavailable"}
        executable = games.for_runtime(runtime).executable
        system = folders[-1]
        runner = runtime / "runner/files/lib/wine"
        libraries = {}
        for name, (component, library) in LIBRARIES.items():
            try:
                current = graphics._digest(system / (name + ".dll"))
                expected = graphics._digest(runner / component / "x86_64-windows" / (library + ".dll"))
                if current == expected:
                    libraries[name] = "matches_runner"
                else:
                    try:
                        builtin = graphics._digest(runner / "x86_64-windows" / (name + ".dll"))
                    except (OSError, graphics.GraphicsError):
                        builtin = None
                    libraries[name] = "wine_builtin" if current == builtin else "different"
            except FileNotFoundError:
                libraries[name] = "missing" if not (system / (name + ".dll")).exists() else "runner_unavailable"
            except (OSError, graphics.GraphicsError):
                libraries[name] = "unavailable"
        # NGX is provided by the host driver, not by the runner. Presence alone
        # does not validate the driver bridge or imply that DLSS works.
        for name in ("nvngx", "_nvngx"):
            try:
                graphics._digest(system / (name + ".dll"))
                libraries[name] = "present"
            except FileNotFoundError:
                libraries[name] = "missing"
            except (OSError, graphics.GraphicsError):
                libraries[name] = "unavailable"
        return {"status": "inspected", "libraries": libraries,
                "user_overrides": _registry(prefix / "user.reg", executable),
                "system_overrides": _registry(prefix / "system.reg", executable)}
    except (OSError, ValueError):
        return {"status": "unavailable"}


def log_summary(text):
    # Enumerated symbols only, never copy a matching line or arbitrary suffix.
    codes = {"VK_ERROR_" + name for name in ("OUT_OF_HOST_MEMORY", "OUT_OF_DEVICE_MEMORY", "INITIALIZATION_FAILED",
             "DEVICE_LOST", "MEMORY_MAP_FAILED", "LAYER_NOT_PRESENT", "EXTENSION_NOT_PRESENT", "FEATURE_NOT_PRESENT",
             "INCOMPATIBLE_DRIVER", "TOO_MANY_OBJECTS", "FORMAT_NOT_SUPPORTED", "SURFACE_LOST_KHR", "OUT_OF_DATE_KHR")}
    codes |= {"DXGI_ERROR_" + name for name in ("DEVICE_REMOVED", "DEVICE_HUNG", "DEVICE_RESET", "DRIVER_INTERNAL_ERROR", "UNSUPPORTED")}
    found = set(re.findall(r"\b(?:VK_ERROR_|DXGI_ERROR_)[A-Z_]+\b", text)) & codes
    components = []
    for name, pattern in (("vkd3d-proton", r"\b(?:info|warn|err):vkd3d-proton:"),
                          ("dxvk", r"\bDXVK: v[0-9]"), ("dxvk-nvapi", r"\bDXVK-NVAPI\b")):
        if re.search(pattern, text):
            components.append(name)
    return {"scope": "bounded_log_excerpt", "observed_components": components, "error_symbols": sorted(found)}
