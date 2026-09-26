# SPDX-License-Identifier: MIT
"""Prepare the NVIDIA pieces normally installed by Proton's Python launcher.

Flightdeck invokes Wine directly. Use NVAPI from that same runner and NGX from
the installed host driver, never download or spoof a different driver. Native
driver discovery runs in short-lived children, not in the launcher process.
"""
from __future__ import annotations

import ctypes as ct
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import tempfile

_NVAPI = {
    "system32/nvapi64.dll": "x86_64-windows/nvapi64.dll",
    "system32/nvofapi64.dll": "x86_64-windows/nvofapi64.dll",
    "syswow64/nvapi.dll": "i386-windows/nvapi.dll",
}
_FILES = set(_NVAPI) | {"system32/nvngx.dll", "system32/_nvngx.dll"}
_SELECTORS = ("DXVK_FILTER_DEVICE_NAME", "DXVK_FILTER_DEVICE_UUID",
              "VKD3D_FILTER_DEVICE_NAME", "VKD3D_VULKAN_DEVICE")
NVIDIA_MODES = {"auto", "compatibility"}
SETTINGS_FILE = "graphics-settings.json"


class GraphicsError(Exception):
    code = "graphics"


def settings(runtime):
    """Per-installation preferences, read again for every start (no service restart)."""
    try:
        private = runtime / "private"
        if private.is_symlink() or not private.is_dir():
            raise ValueError()
        fd = os.open(private / SETTINGS_FILE, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size > 4096:
                raise ValueError()
            value = json.loads(source.read(4097))
        if (not isinstance(value, dict) or value.get("schema") != 1
                or not isinstance(value.get("nvidia_mode"), str)
                or value["nvidia_mode"] not in NVIDIA_MODES):
            raise ValueError()
        return {"nvidia_mode": value["nvidia_mode"]}
    except FileNotFoundError:
        return {"nvidia_mode": "auto"}
    except (OSError, ValueError) as error:
        raise GraphicsError("Die Grafikeinstellungen sind ungültig. Bitte unter Einrichtung erneut speichern.") from error


def snapshot(runtime):
    present = nvidia_present()
    result = {"nvidia_present": present, "available": bool(present and runtime is not None
              and (runtime / "runner/files/bin/wine").is_file()), "nvidia_mode": "auto", "error": ""}
    if runtime is not None:
        try:
            result.update(settings(runtime))
        except GraphicsError as error:
            result["error"] = str(error)
    return result


def _text(value, maximum=256):
    return isinstance(value, str) and 0 < len(value) <= maximum and all(32 <= ord(c) < 127 for c in value)


def nvidia_present(root=Path("/sys/bus/pci/devices")):
    try:
        for device in root.iterdir():
            try:
                if (int((device / "vendor").read_text(), 16) == 0x10de
                        and int((device / "class").read_text(), 16) >> 16 == 3):
                    return True
            except (OSError, ValueError):
                continue
    except OSError:
        pass
    return False


def _native_probe():
    """Vulkan 1.0 ABI; enumerate real adapters without vulkan-tools or a display."""
    class Application(ct.Structure):
        _fields_ = [("type", ct.c_uint32), ("next", ct.c_void_p), ("name", ct.c_char_p),
                    ("version", ct.c_uint32), ("engine", ct.c_char_p),
                    ("engine_version", ct.c_uint32), ("api", ct.c_uint32)]
    class InstanceInfo(ct.Structure):
        _fields_ = [("type", ct.c_uint32), ("next", ct.c_void_p), ("flags", ct.c_uint32),
                    ("application", ct.POINTER(Application)), ("layer_count", ct.c_uint32),
                    ("layers", ct.c_void_p), ("extension_count", ct.c_uint32), ("extensions", ct.c_void_p)]
    library = ct.CDLL("libvulkan.so.1")
    library.vkCreateInstance.argtypes = [ct.POINTER(InstanceInfo), ct.c_void_p, ct.POINTER(ct.c_void_p)]
    library.vkCreateInstance.restype = ct.c_int32
    library.vkEnumeratePhysicalDevices.argtypes = [ct.c_void_p, ct.POINTER(ct.c_uint32), ct.c_void_p]
    library.vkEnumeratePhysicalDevices.restype = ct.c_int32
    library.vkGetPhysicalDeviceProperties.argtypes = [ct.c_void_p, ct.c_void_p]
    library.vkGetPhysicalDeviceProperties.restype = None
    library.vkDestroyInstance.argtypes = [ct.c_void_p, ct.c_void_p]
    library.vkDestroyInstance.restype = None
    application = Application(0, None, b"Flightdeck graphics check", 1, None, 0, 1 << 22)
    info = InstanceInfo(1, None, 0, ct.pointer(application), 0, None, 0, None)
    instance = ct.c_void_p()
    if library.vkCreateInstance(ct.byref(info), None, ct.byref(instance)) != 0:
        raise OSError()
    try:
        count = ct.c_uint32()
        if library.vkEnumeratePhysicalDevices(instance, ct.byref(count), None) != 0 or not 0 < count.value <= 16:
            raise OSError()
        devices = (ct.c_void_p * count.value)()
        if library.vkEnumeratePhysicalDevices(instance, ct.byref(count), devices) != 0:
            raise OSError()
        result = []
        for device in devices[:count.value]:
            # VkPhysicalDeviceProperties has a fixed Vulkan 1.0 layout. The
            # aligned allocation exceeds the entire struct, not just its header.
            properties = (ct.c_uint64 * 512)()
            library.vkGetPhysicalDeviceProperties(device, ct.byref(properties))
            raw = bytes(properties)
            api, driver, vendor, _, kind = struct.unpack_from("=5I", raw)
            name = raw[20:276].split(b"\0", 1)[0].decode("utf-8", "replace")
            if not _text(name):
                continue
            api_version = f"{(api >> 22) & 127}.{(api >> 12) & 1023}.{api & 4095}"
            version = (f"{driver >> 22}.{(driver >> 14) & 255}.{(driver >> 6) & 255}.{driver & 63}"
                       if vendor == 0x10de else f"{driver >> 22}.{(driver >> 12) & 1023}.{driver & 4095}")
            result.append({"name": name, "vendor_id": vendor, "type": kind,
                           "api_version": api_version, "driver_version": version})
        return {"status": "ready" if result else "failed", "devices": result}
    finally:
        library.vkDestroyInstance(instance, None)


def _native_nvidia_directory():
    # Same loader-relative discovery as the pinned Proton launch script. It
    # handles Debian multiarch, /usr/lib64 and driver-versioned library paths.
    library = ct.CDLL("libGLX_nvidia.so.0")
    libdl = ct.CDLL("libdl.so.2")
    class LinkMap(ct.Structure):
        _fields_ = [("address", ct.c_void_p), ("name", ct.c_char_p)]
    pointer = ct.POINTER(LinkMap)()
    libdl.dlinfo.argtypes = [ct.c_void_p, ct.c_int, ct.c_void_p]
    libdl.dlinfo.restype = ct.c_int
    if libdl.dlinfo(library._handle, 2, ct.byref(pointer)) or not pointer or not pointer.contents.name:
        return None
    folder = Path(os.fsdecode(pointer.contents.name)).resolve().parent / "nvidia/wine"
    return str(folder) if (folder / "nvngx.dll").is_file() else None


def _child(mode):
    # Bound both runtime and the data read back. Driver stderr is never exported.
    try:
        with tempfile.TemporaryFile() as output:
            completed = subprocess.run([sys.executable, "-B", str(Path(__file__).resolve()), mode],
                stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.DEVNULL, timeout=5,
                env=dict(os.environ, LC_ALL="C"), close_fds=True)
            if completed.returncode:
                return None
            output.seek(0)
            raw = output.read(16385)
            return json.loads(raw) if len(raw) <= 16384 else None
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None


def probe():
    value = _child("--probe")
    result = {"status": "failed", "devices": [],
              "session": os.environ.get("XDG_SESSION_TYPE") if os.environ.get("XDG_SESSION_TYPE") in {"x11", "wayland"} else "unknown"}
    if not isinstance(value, dict) or value.get("status") != "ready" or not isinstance(value.get("devices"), list):
        return result
    for entry in value["devices"][:16]:
        if (isinstance(entry, dict) and _text(entry.get("name"))
                and type(entry.get("vendor_id")) is int and 0 <= entry["vendor_id"] <= 0xffffffff
                and type(entry.get("type")) is int and entry["type"] in range(5)
                and all(isinstance(entry.get(key), str) and re.fullmatch(r"[0-9.]{1,40}", entry[key])
                        for key in ("api_version", "driver_version"))):
            result["devices"].append({key: entry[key] for key in ("name", "vendor_id", "type", "api_version", "driver_version")})
    if result["devices"]:
        result["status"] = "ready" if any(d["type"] != 4 for d in result["devices"]) else "software_only"
    return result


def nvidia_directory(environment):
    path = environment.get("NVIDIA_WINE_DLL_DIR") or _child("--nvidia-directory")
    if not isinstance(path, str) or not Path(path).is_absolute() or any(ord(c) < 32 for c in path):
        return None
    try:
        folder = Path(path).resolve(strict=True)
        return folder if (folder / "nvngx.dll").is_file() else None
    except (OSError, RuntimeError):
        return None


def _digest(path):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > 128 * 1024**2:
            raise GraphicsError("Die NVIDIA-Laufzeitdateien sind ungültig. Bitte Flightdeck erneut installieren.")
        result = hashlib.sha256()
        while block := source.read(1024 * 1024):
            result.update(block)
        return result.hexdigest()


def _install(runtime, directory):
    from .backend import atomic_json
    from .setup import _copy_file, prefix_system32, SetupError
    prefix = runtime / "local/msfs-prefix"
    try:
        if prefix.is_symlink() or not prefix.is_dir():
            raise ValueError()
        prefix_system32(prefix)
        windows = prefix / "drive_c/windows"
        if (windows / "syswow64").is_symlink() or not (windows / "syswow64").is_dir():
            raise ValueError()
        private = runtime / "private"
        if private.is_symlink() or not private.is_dir():
            raise ValueError()
        marker = private / "nvidia-runtime.json"
        previous = {}
        if marker.exists() or marker.is_symlink():
            fd = os.open(marker, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(fd, "rb") as source:
                if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
                    raise ValueError()
                previous = json.loads(source.read(16385))
            if (not isinstance(previous, dict) or set(previous) - _FILES
                    or any(not isinstance(v, str) or not re.fullmatch(r"[0-9a-f]{64}", v) for v in previous.values())):
                raise ValueError()
        sources = {target: runtime / "runner/files/lib/wine/nvapi" / source for target, source in _NVAPI.items()}
        if directory is not None:
            for name in ("nvngx.dll", "_nvngx.dll"):
                if (directory / name).is_file():
                    sources["system32/" + name] = directory / name
        # Validate the whole input before changing the prefix. Missing NVAPI
        # must not enable DXVK's NVIDIA path with a half-prepared bridge.
        hashes = {name: _digest(source) for name, source in sources.items()}
        managed, custom = dict(previous), []
        for name, source in sources.items():
            target = windows / name
            if target.exists() or target.is_symlink():
                current = _digest(target)
                if current == hashes[name]:
                    # Recognize the exact runner/driver bytes, including an
                    # interrupted installation before its marker was saved.
                    managed[name] = hashes[name]
                    continue
                if current != previous.get(name):
                    custom.append(name.rsplit("/", 1)[-1])
                    continue
            _copy_file(source, target)
            managed[name] = hashes[name]
        for name in set(previous) - set(sources):
            target = windows / name
            if target.exists() and _digest(target) == previous[name]:
                target.unlink()
            managed.pop(name, None)
        atomic_json(marker, managed)
        return custom
    except (OSError, ValueError, SetupError) as error:
        raise GraphicsError("Die NVIDIA-Laufzeit konnte nicht vorbereitet werden. Bitte Runner und Schreibrechte der Wine-Umgebung prüfen.") from error


def _overrides(environment):
    entries = environment.get("WINEDLLOVERRIDES", "").split(";")
    present = {name.strip().lower().removesuffix(".dll") for entry in entries if "=" in entry
               for name in entry.split("=", 1)[0].split(",")}
    for name, mode in (("nvapi", "n"), ("nvapi64", "n"), ("nvofapi64", "n"), ("nvcuda", "b")):
        if name not in present:
            entries.append(name + "=" + mode)
    environment["WINEDLLOVERRIDES"] = ";".join(entry for entry in entries if entry)


def _select_adapter(environment, devices):
    """Keep DXGI and D3D12 on the same uniquely identified physical adapter."""
    if not any(environment.get(key) for key in _SELECTORS):
        discrete = [d for d in devices if d["type"] == 2]
        if len(discrete) == 1 and discrete[0]["vendor_id"] == 0x10de:
            environment["DXVK_FILTER_DEVICE_NAME"] = discrete[0]["name"]
            environment["VKD3D_FILTER_DEVICE_NAME"] = discrete[0]["name"]
    elif not any(environment.get(key) for key in ("DXVK_FILTER_DEVICE_UUID", "VKD3D_VULKAN_DEVICE")):
        # A single unambiguous name filter can be completed for the other API.
        # Never guess the meaning of a UUID/index or overwrite a second choice.
        for source, target in (("DXVK_FILTER_DEVICE_NAME", "VKD3D_FILTER_DEVICE_NAME"),
                               ("VKD3D_FILTER_DEVICE_NAME", "DXVK_FILTER_DEVICE_NAME")):
            if environment.get(source) and not environment.get(target):
                matched = [d for d in devices if environment[source] in d["name"] and d["type"] != 4]
                if len(matched) == 1:
                    # Wine may report a different marketing name than the
                    # host. Retain the user's working substring, not a newly
                    # constructed full name from the native probe.
                    environment[target] = environment[source]


def _nvidia_mode(environment, mode):
    if mode == "compatibility":
        environment["PROTON_DISABLE_NVAPI"] = "1"
        environment["DXVK_ENABLE_NVAPI"] = "0"
        environment["PROTON_HIDE_NVIDIA_GPU"] = "1"
        # NGX can also be loaded directly by a game. Leave the files intact so
        # returning to automatic mode restores driver-backed NVIDIA features.
        entries = [environment.get("WINEDLLOVERRIDES", ""), "nvngx,_nvngx,*nvngx,*_nvngx="]
        environment["WINEDLLOVERRIDES"] = ";".join(entry for entry in entries if entry)
    if environment.get("PROTON_HIDE_NVIDIA_GPU", "0") not in {"", "0"}:
        # The Proton Python launcher normally translates this. Wine itself
        # never reads PROTON_* options, and DXVK has its own vendor reporting.
        environment["WINE_HIDE_NVIDIA_GPU"] = "1"
    if environment.get("WINE_HIDE_NVIDIA_GPU") == "1":
        config = environment.get("DXVK_CONFIG", "").rstrip(" ;\n")
        environment["DXVK_CONFIG"] = (config + "; " if config else "") + "dxgi.hideNvidiaGpu = True"


def prepare(runtime, environment=None):
    """Called only while holding this runtime's play.lock, before spawning Wine."""
    environment = dict(os.environ if environment is None else environment)
    if not (runtime / "runner/files/bin/wine").is_file():
        return environment, {"nvidia": "custom_runner"}
    if not nvidia_present():
        return environment, {"nvidia": "not_present"}
    mode = settings(runtime)["nvidia_mode"]
    _nvidia_mode(environment, mode)
    report = probe()
    if report["status"] != "ready" or not any(d["vendor_id"] == 0x10de and d["type"] != 4 for d in report["devices"]):
        raise GraphicsError("NVIDIA wurde erkannt, aber Vulkan ist nicht verfügbar. Bitte den empfohlenen NVIDIA-Treiber der Distribution installieren und Linux neu starten.")
    # Adapter selection and GLVND setup are needed even without NVAPI. An
    # opt-out must not also change which physical GPU DXGI and D3D12 use.
    # Keep explicit choices and never guess among multiple discrete GPUs.
    _select_adapter(environment, report["devices"])
    report.update(nvidia_mode=mode, hide_nvidia=environment.get("WINE_HIDE_NVIDIA_GPU") == "1")
    environment.setdefault("__GLVND_DISALLOW_PATCHING", "1")
    if environment.get("PROTON_DISABLE_NVAPI", "0") not in {"", "0"} or environment.get("DXVK_ENABLE_NVAPI") == "0":
        # Direct Wine starts do not interpret PROTON_DISABLE_NVAPI. Skipping
        # installation alone leaves DLLs from previous starts loadable. Block
        # loading for this process, including app-local/wildcard overrides,
        # without deleting managed or user-supplied files. Wine's last entry
        # wins, so an explicit disable also overrides inherited native entries.
        environment["DXVK_ENABLE_NVAPI"] = "0"
        entries = [environment.get("WINEDLLOVERRIDES", ""),
                   "nvapi,nvapi64,nvofapi64,*nvapi,*nvapi64,*nvofapi64="]
        environment["WINEDLLOVERRIDES"] = ";".join(entry for entry in entries if entry)
        report["nvidia"] = "disabled"
        return environment, report
    directory = nvidia_directory(environment)
    custom = _install(runtime, directory)
    _overrides(environment)
    environment.setdefault("DXVK_ENABLE_NVAPI", "1")
    if directory is not None:
        environment["NVIDIA_WINE_DLL_DIR"] = str(directory)
    report.update(nvidia="ready", ngx_available=directory is not None, custom_dlls=custom)
    return environment, report


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["--probe"]:
            print(json.dumps(_native_probe()))
        elif sys.argv[1:] == ["--nvidia-directory"]:
            print(json.dumps(_native_nvidia_directory()))
        else:
            raise ValueError()
    except Exception:
        raise SystemExit(1)
