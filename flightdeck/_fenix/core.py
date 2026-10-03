# SPDX-License-Identifier: MIT
"""Transactional, per-user Flightdeck installation. Python standard library only.

Never imports or executes code from a downloaded bundle. Bundle metadata is
compared with the manifest shipped with this installer before any file is used.
"""
from __future__ import annotations

from contextlib import contextmanager
import ctypes
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import tempfile
import urllib.request
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
MARKER = "private/fenix-linux-patch.json"
PROGRAM = Path("drive_c/Program Files/FenixSim A320")
CONFIG = Path("drive_c/ProgramData/Fenix/FenixSim A320")
ENV = {
    "WINEARCH": "win64", "WINEESYNC": "0", "WINEFSYNC": "0",
    "WINE_TRACK_WRITECOPY": "apps:Fenix.exe,FenixSystem.exe,FenixDisplay.exe,FenixCDU.exe,FlightSimulator2024.exe",
    "WINE_D2D1_DISPLAY_EFFECTS": "FenixDisplay.exe;FenixCDU.exe",
    "WINE_D2D1_GEOMETRY_PROVIDER": "FenixDisplay.exe",
    "WINE_FENIX_HELPER_WINDOWS": "1",
    "WINE_DWRITE_UNHINTED_OUTLINES": "FenixDisplay.exe;FenixCDU.exe",
    "DOTNET_SYSTEM_GLOBALIZATION_USENLS": "1", "DOTNET_ReadyToRun": "0",
}
DOWNLOADS = {
    "dotNetFx40_Full_x86_x64.exe": (
        "https://download.microsoft.com/download/9/5/A/95A9616B-7A37-4AF6-BC36-D6EA96C8DAAE/dotNetFx40_Full_x86_x64.exe",
        "65e064258f2e418816b304f646ff9e87af101e4c9552ab064bb74d281c38659f"),
    "NDP48-x86-x64-AllOS-ENU.exe": (
        "https://download.microsoft.com/download/f/3/a/f3a6af84-da23-40a5-8d1c-49cc10c8e76f/NDP48-x86-x64-AllOS-ENU.exe",
        "0a3a390c47e639d0f7fc65b21195fee6b7f65b066f80f70c60fab191d14b7e40"),
}
GEOMETRY_DOWNLOADS = {
    "Windows6.1-KB2670838-x64.msu": (
        "https://download.microsoft.com/download/1/4/9/14936FE9-4D16-4019-A093-5E00182609EB/Windows6.1-KB2670838-x64.msu",
        "9fe71e7dcd2280ce323880b075ade6e56c49b68fc702a9b4c0a635f0f1fb9db8"),
    "msdelta.dll": (
        "https://msdl.microsoft.com/download/symbols/msdelta.dll/559F38C482000/msdelta.dll",
        "29c10fb3ffa0e3cfd04c5247c3ed3a975575fad44d8a1e447b23b43213781653"),
}
GEOMETRY_SHA256 = "663f1d59ec1c014b9ea47a6cef71b3d43579e9759a82f3ee2cbd80b8c6d9e85f"
GEOMETRY_PATH = "drive_c/windows/system32/d2d1_geometry.dll"


class PatchError(RuntimeError):
    pass


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def regular(path, limit=None):
    path = Path(path)
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or (limit and info.st_size > limit):
        raise PatchError("Expected a regular file: " + path.name)
    return path


def read_json(path):
    return json.loads(regular(path, 1024 * 1024).read_text())


def manifest(variant=None):
    lock = read_json(ROOT / "bundle.json")
    if variant:
        if variant not in lock.get("variants", {}):
            raise PatchError("No Fenix compatibility build is available for this Proton version.")
        lock = {**lock, **lock["variants"][variant], "variant": variant}
    return lock


def runner_variant(runner, *, patched=False):
    """Match complete Wine identities; a family/version label is not an ABI."""
    lock = manifest()
    for variant in (None, *lock.get("variants", {})):
        candidate = manifest(variant)
        try:
            if patched:
                candidate = {**candidate, "runner_files": {**candidate["runner_files"], **candidate["files"]}}
            verify_runner(runner, candidate)
            return variant
        except (OSError, PatchError):
            pass
    raise PatchError("Für diese Proton-Version fehlt ein passender Fenix-Patch. Wähle eine Version mit Fenix-Unterstützung.")


def wine_binary(runner):
    wine64 = runner / "files/bin/wine64"
    return wine64 if wine64.is_file() else runner / "files/bin/wine"


def atomic(path, content, mode=0o600):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=".fenix-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(name, mode)
        os.replace(name, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(name).unlink(missing_ok=True)


def write_json(path, value):
    atomic(path, (json.dumps(value, indent=2) + "\n").encode())


def contained(root, relative):
    """Permit Wine's file symlinks to be replaced, never parent escapes."""
    root = Path(root).resolve()
    relative = Path(relative)
    if relative.is_absolute() or ".." in relative.parts:
        raise PatchError("Invalid installation path")
    path = root / relative
    if not path.parent.resolve().is_relative_to(root):
        raise PatchError("Installation directory points outside the selected profile")
    return path


def runtime_path(value, *, recovery=False):
    path = Path(value).expanduser().resolve(strict=True)
    info = read_json(path / "private/runtime.json")
    if info.get("game_id", "msfs2024") != "msfs2024":
        raise PatchError("This preview supports MSFS 2024 only.")
    for relative in ("private", "tools", "local") + (() if recovery else ("local/msfs-prefix", "local/msfs-prefix/drive_c", "local/msfs-prefix/drive_c/windows", "local/msfs-prefix/drive_c/windows/system32")):
        item = contained(path, relative)
        if item.is_symlink() or not item.is_dir() or item.stat().st_uid != os.getuid():
            raise PatchError("Runtime needs owned, ordinary directories: " + relative)
    if not recovery:
        regular(path / "local/msfs-prefix/system.reg")
    if not (path / "runner").is_symlink():
        raise PatchError("Expected Flightdeck's runner symlink; the original runner will be retained.")
    return path


def default_runtime():
    data = Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share")))
    return data / "flightdeck/runtimes/msfs2024"


def ensure_idle(prefix):
    expected = str(Path(prefix).resolve()).encode()
    for proc in Path("/proc").iterdir():
        if not proc.name.isdecimal() or int(proc.name) == os.getpid():
            continue
        try:
            entries = (proc / "environ").read_bytes().split(b"\0")
            values = [entry[11:] for entry in entries if entry.startswith(b"WINEPREFIX=")]
            if any(value == expected or os.path.realpath(os.fsdecode(value)) == os.fsdecode(expected) for value in values):
                raise PatchError("Close MSFS, Fenix and all installers for this profile first.")
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue


@contextmanager
def locked(root, *, idle=True, recovery=False):
    root = runtime_path(root, recovery=recovery)
    if (root / "private/proton-switch.json").exists():
        raise PatchError("Recover the interrupted Proton switch before changing Fenix.")
    path = root / "private/play.lock"
    fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
            raise PatchError("Invalid runtime lock")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise PatchError("The simulator or another installation is using this runtime.") from None
        if idle:
            ensure_idle(root / "local/msfs-prefix")
        yield root
    finally:
        os.close(fd)


def host_check():
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "amd64"):
        raise PatchError("The binary preview requires x86_64 Linux.")
    libc = ctypes.CDLL(None)
    try:
        libc.gnu_get_libc_version.restype = ctypes.c_char_p
        version = libc.gnu_get_libc_version().decode()
        if tuple(map(int, version.split(".")[:2])) < (2, 38):
            raise PatchError("This binary build requires glibc 2.38 or newer. Build from source on older distributions.")
    except AttributeError:
        raise PatchError("The binary build requires glibc.") from None


def verify_runner(runner, lock):
    version = regular(runner / "version", 4096).read_text().strip()
    if re.sub(r"^\d+\s+", "", version) != re.sub(r"^\d+\s+", "", lock["runner_version"]):
        raise PatchError("The Wine runner version does not match this Fenix build.")
    for name, expected in lock["runner_files"].items():
        if digest(regular(runner / name)) != expected:
            raise PatchError("The Wine runner differs from the supported build: " + name)


def verify_bundle(bundle):
    bundle = Path(bundle).expanduser().resolve(strict=True)
    lock = manifest()
    if read_json(bundle / "bundle.json") != lock:
        raise PatchError("The patch bundle does not match this installer version.")
    files = dict(lock["files"])
    for variant, entry in lock.get("variants", {}).items():
        files.update({"variants/" + variant + "/" + name: sha for name, sha in entry["files"].items()})
    for relative, expected in files.items():
        path = contained(bundle / "payload", relative)
        if digest(regular(path)) != expected:
            raise PatchError("Patch checksum mismatch: " + relative)
    for name, expected in lock["integration"].items():
        if digest(regular(contained(bundle / "integration", name))) != expected:
            raise PatchError("Integration checksum mismatch: " + name)
    return bundle


def download(url, destination, expected, progress=lambda _: None, max_size=256 * 1024 * 1024):
    destination = Path(destination)
    if destination.is_file() and not destination.is_symlink() and digest(destination) == expected:
        return destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=".download-", dir=destination.parent)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "Fenix-Linux-Patch/0.1"})
        with os.fdopen(fd, "wb") as output, urllib.request.urlopen(request, timeout=45) as response:
            if not response.url.startswith("https://"):
                raise PatchError("Download redirected away from HTTPS")
            size = 0
            while chunk := response.read(1024 * 1024):
                size += len(chunk)
                if size > max_size:
                    raise PatchError("Download exceeds the allowed size")
                output.write(chunk)
            output.flush()
            os.fsync(output.fileno())
        if digest(name) != expected:
            raise PatchError("Download checksum mismatch: " + destination.name)
        os.replace(name, destination)
        return destination
    finally:
        Path(name).unlink(missing_ok=True)


def wine_env(prefix, runner):
    env = dict(os.environ, **ENV, WINEPREFIX=str(prefix), WINEDEBUG="-all",
               WINE=str(wine_binary(runner)), WINESERVER=str(runner / "files/bin/wineserver"),
               WINE_DISABLE_FAST_SYNC="1")
    for name in ("WINE_DLL_FILE_MAP", "WINELOADER", "WINEDLLPATH", "WINESERVERSOCKET", "WINEPRELOADRESERVE", "WINELOADERNOEXEC"):
        env.pop(name, None)
    env["WINEDLLOVERRIDES"] = "winemenubuilder.exe=d"
    old = env.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "")
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (old + " --disable-features=HideCursorWhileTyping").strip()
    return env


def apply_overlay(prefix, runner, bundle, variant=None):
    """Only call after every process in the staged prefix has stopped."""
    lock = manifest(variant)
    payload = bundle / "payload"
    if variant:
        payload /= "variants/" + variant
    for name, sha in lock["files"].items():
        data = regular(contained(payload, name)).read_bytes()
        if hashlib.sha256(data).hexdigest() != sha:
            raise PatchError("Fenix patch checksum mismatch: " + name)
        atomic(contained(runner, name), data, 0o755 if "/bin/" in name else 0o644)
        if name.startswith("files/lib/wine/x86_64-windows/"):
            atomic(contained(prefix, "drive_c/windows/system32/" + Path(name).name), data, 0o644)
    for name in ("FenixWindowGuard.exe", "FenixMCDURefresh.exe", "fenix-display-refresh.py"):
        if name in lock["integration"]:
            data = regular(bundle / "integration" / name).read_bytes()
            if hashlib.sha256(data).hexdigest() != lock["integration"][name]:
                raise PatchError("Fenix helper checksum mismatch: " + name)
            atomic(contained(prefix, "drive_c/windows/system32/" + name), data, 0o644)
    # Include untouched ABI partners and the loader in the final verification.
    # A concurrently updated source runner must not produce a mixed build.
    verify_runner(runner, {**lock, "runner_files": {**lock["runner_files"], **lock["files"]}})


class Wine:
    def __init__(self, prefix, runner, log):
        self.prefix, self.runner, self.log = prefix, runner, log
        self.env = wine_env(prefix, runner)

    def run(self, *args, env=None, accepted=(0,), timeout=1800):
        result = subprocess.run([str(wine_binary(self.runner)), *map(str, args)],
            env={**self.env, **(env or {})}, cwd=self.prefix, stdin=subprocess.DEVNULL,
            stdout=self.log, stderr=subprocess.STDOUT, timeout=timeout)
        if result.returncode not in accepted:
            raise PatchError("Windows setup failed (exit %d). See the private Fenix setup log." % result.returncode)
        return result.returncode

    def reg(self, key, name, value, kind="REG_SZ"):
        self.run("reg", "add", key, "/v", name, "/t", kind, "/d", value, "/f")

    def stop(self):
        # Used only for the private staging profile we created, never a user's running game.
        result = subprocess.run([str(self.runner / "files/bin/wineserver"), "-k"], env=self.env,
                                stdout=self.log, stderr=subprocess.STDOUT, timeout=30)
        # Proton returns 1 when no server exists, e.g. when an already installed
        # geometry dependency needs no Wine commands. Still wait for shutdown.
        if result.returncode not in (0, 1):
            result.check_returncode()
        subprocess.run([str(self.runner / "files/bin/wineserver"), "-w"], env=self.env,
                       stdout=self.log, stderr=subprocess.STDOUT, timeout=30, check=True)


def _framework_path(prefix, architecture, name):
    """Wine paths are case-insensitive, even on a case-sensitive Linux host."""
    root = Path(prefix).resolve(strict=True)
    path = root
    for part in ("drive_c", "windows", "Microsoft.NET", architecture, "v4.0.30319", name):
        matches = []
        with os.scandir(path) as entries:
            for count, entry in enumerate(entries):
                if count >= 4096:
                    raise PatchError("Too many entries in the .NET installation directory.")
                if entry.name.casefold() == part.casefold():
                    matches.append(entry.name)
        if len(matches) != 1:
            raise PatchError("A .NET Framework file is missing or ambiguous.")
        path /= matches[0]
        # The staged copy must not read or repair another profile through links.
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise PatchError("A .NET Framework path points outside the copied profile.")
    return regular(path, 128 * 1024 * 1024)


def framework_status(prefix):
    """Inspect both CLR architectures without executing Wine or exposing paths."""
    result = {view: {"release": 0, "clr": False} for view in ("x86", "x64")}
    try:
        content = regular(prefix / "system.reg", 64 * 1024 * 1024).read_text(errors="replace")
        keys = {view: (r"Software\\" + middle + r"Microsoft\\NET Framework Setup\\NDP\\v4\\Full").casefold()
                for view, middle in (("x64", ""), ("x86", r"Wow6432Node\\"))}
        sections = {}
        current = None
        for line in content.splitlines():
            if line.startswith("["):
                header = re.match(r"^\[([^\]\r\n]+)\]", line)
                current = header[1].casefold() if header else None
                if current not in keys.values():
                    current = None
                if current is not None:
                    sections.setdefault(current, []).append([])
            elif current is not None and line.lower().startswith('"release"='):
                sections[current][-1].append(line)
        for view, key in keys.items():
            found = sections.get(key, [])
            if len(found) == 1 and len(found[0]) == 1:
                values = re.findall(r'(?im)^"Release"=dword:([0-9a-f]{1,8})\s*$', "\n".join(found[0]))
                if len(values) == 1:
                    result[view]["release"] = int(values[0], 16)
    except (OSError, PatchError, UnicodeError):
        pass
    for view, architecture in (("x64", "Framework64"), ("x86", "Framework")):
        try:
            with _framework_path(prefix, architecture, "clr.dll").open("rb") as stream:
                result[view]["clr"] = stream.read(2) == b"MZ"
        except (OSError, PatchError):
            pass
    result["ready"] = all(value["release"] >= 528040 and value["clr"] for value in result.values())
    return result


def has_framework(prefix):
    return framework_status(prefix)["ready"]


def _framework_settings(wine):
    wine.reg(r"HKCU\Software\Wine\DllOverrides", "mscoree", "native")
    for key in (r"HKLM\Software\Microsoft\.NETFramework", r"HKLM\Software\Wow6432Node\Microsoft\.NETFramework"):
        wine.reg(key, "OnlyUseLatestCLR", "1", "REG_DWORD")


def _framework_probe(wine):
    if not has_framework(wine.prefix):
        return False
    try:
        # Microsoft's managed compiler exercises real CLR startup in both views.
        # A registry marker alone cannot establish a functioning installation.
        for architecture in ("Framework", "Framework64"):
            compiler = _framework_path(wine.prefix, architecture, "csc.exe")
            wine.run(compiler, "/nologo", "/help", timeout=120)
        return True
    except (OSError, PatchError, subprocess.TimeoutExpired):
        return False


def _framework_restart(wine):
    # Complete pending file replacements and RunOnce inside this staging prefix.
    # -u updates Wine's files; -r performs the pending Windows restart work.
    wine.run("wineboot", "-r", timeout=180)
    wine.stop()  # Also flush the registry before inspecting system.reg.


def _framework_reinstall(wine, packages, progress):
    """Last resort, exclusively inside the uncommitted copy owned by the caller."""
    status = framework_status(wine.prefix)
    if any(status[view]["release"] >= 533320 and status[view]["clr"] for view in ("x86", "x64")):
        raise PatchError("A newer .NET Framework is installed; the pinned 4.8 package cannot repair it safely.")
    progress("Reinstalling Microsoft .NET in the copied profile to complete the repair …")
    options = {"env": {"WINEDLLOVERRIDES": "fusion=b;winemenubuilder.exe=d"}, "accepted": (0, 194)}
    if any(status[view]["release"] >= 528040 for view in ("x86", "x64")):
        wine.run(packages["NDP48-x86-x64-AllOS-ENU.exe"], "/uninstall", "/q", "/norestart", **options)
        _framework_restart(wine)
    wine.reg(r"HKCU\Software\Wine", "Version", "winxp")
    wine.run(packages["dotNetFx40_Full_x86_x64.exe"], "/q", "/c:install.exe /q /norestart", **options)
    _framework_settings(wine)
    _framework_restart(wine)
    wine.reg(r"HKCU\Software\Wine", "Version", "win7")


def prepare_framework(wine, cache, progress):
    if has_framework(wine.prefix):
        _framework_settings(wine)
    if _framework_probe(wine):
        progress("Microsoft .NET Framework 4.8 is already installed.")
        return
    progress("Checking and completing pending Microsoft .NET setup …")
    _framework_restart(wine)
    if has_framework(wine.prefix):
        _framework_settings(wine)
    if _framework_probe(wine):
        progress("Microsoft .NET Framework 4.8 is ready.")
        return
    listing = subprocess.run([str(wine_binary(wine.runner)), "uninstaller", "--list"],
                             env=wine.env, capture_output=True, timeout=120, check=True).stdout.decode(errors="replace")
    removed_mono = False
    for line in listing.splitlines():
        guid, _, title = line.partition("|")
        if title.startswith("Wine Mono") and re.fullmatch(r"\{[a-fA-F0-9-]{36}\}", guid):
            wine.run("uninstaller", "--silent", "--remove", guid)
            removed_mono = True
    if removed_mono:
        # Wine Mono advertises .NET 4.8.1 registry values, without a native CLR.
        # Flush its removal before deciding whether a newer Microsoft CLR exists.
        wine.stop()
    initial = framework_status(wine.prefix)
    if any(initial[view]["release"] >= 533320 and initial[view]["clr"] for view in ("x86", "x64")):
        raise PatchError("A newer .NET Framework is installed; the pinned 4.8 package cannot repair it safely.")
    packages = {}
    for name, (url, expected) in DOWNLOADS.items():
        progress("Downloading Microsoft " + name)
        packages[name] = download(url, cache / name, expected)
    progress("Installing Microsoft .NET Framework. This can take several minutes …")
    try:
        initial = framework_status(wine.prefix)
        # Wine/Mono may retain advertised release values after removal. Only
        # native CLR files establish an existing prerequisite chain; metadata
        # without either CLR must not suppress the .NET 4.0 bootstrap.
        if not any(initial[view]["clr"] for view in ("x86", "x64")):
            wine.reg(r"HKCU\Software\Wine", "Version", "winxp")
            wine.run(packages["dotNetFx40_Full_x86_x64.exe"], "/q", "/c:install.exe /q /norestart",
                     env={"WINEDLLOVERRIDES": "fusion=b;winemenubuilder.exe=d"}, accepted=(0, 194))
            _framework_restart(wine)
        _framework_settings(wine)
        wine.reg(r"HKCU\Software\Wine", "Version", "win7")
        failure = None
        for attempt in range(2):
            status = framework_status(wine.prefix)
            repair = any(status[view]["release"] >= 528040 for view in ("x86", "x64"))
            if attempt or repair:
                progress("Repairing Microsoft .NET Framework 4.8 automatically …")
            try:
                # Wine can report successful MSI repair while leaving missing
                # files absent. A final reinstall restores the prerequisite
                # chain; it never replaces the user's active Windows profile.
                if attempt:
                    _framework_reinstall(wine, packages, progress)
                args = ("/repair",) if repair and not attempt else ()
                wine.run(packages["NDP48-x86-x64-AllOS-ENU.exe"], *args, "/q", "/norestart",
                         env={"WINEDLLOVERRIDES": "fusion=b;winemenubuilder.exe=d"}, accepted=(0, 194))
                failure = None
            except PatchError as error:
                failure = error
            except subprocess.TimeoutExpired:
                failure = PatchError("The Microsoft .NET installer exceeded its time limit.")
                wine.stop()
            _framework_settings(wine)
            _framework_restart(wine)
            if _framework_probe(wine):
                progress("Microsoft .NET Framework 4.8 was verified for 32-bit and 64-bit applications.")
                return
        state = framework_status(wine.prefix)
        # Fixed numeric/boolean diagnostics only; no registry/account contents.
        detail = "; ".join("%s release=%d CLR=%s" % (view, state[view]["release"], "yes" if state[view]["clr"] else "no")
                           for view in ("x86", "x64"))
        raise PatchError("Microsoft .NET Framework 4.8 could not be repaired automatically (%s). "
                         "The original profile is unchanged. See the private Fenix setup log.%s" %
                         (detail, " " + str(failure) if failure else ""))
    finally:
        wine.reg(r"HKCU\Software\Wine", "Version", "win10")
        wine.stop()


def prepare_geometry(wine, cache, bundle, progress):
    """Install the geometry-only provider in the private staging prefix.

    Rendering stays in Wine. Microsoft packages are downloaded from Microsoft,
    never redistributed. The x86 file is just the update's delta basis; only
    the verified x64 geometry DLL is installed, under a separate filename.
    """
    target = contained(wine.prefix, GEOMETRY_PATH)
    if target.is_file() and not target.is_symlink() and digest(target) == GEOMETRY_SHA256:
        return
    packages = {}
    for name, (url, expected) in GEOMETRY_DOWNLOADS.items():
        progress("Downloading Microsoft geometry dependency: " + name)
        packages[name] = download(url, cache / name, expected, max_size=16 * 1024 * 1024)
    helper = regular(bundle / "integration/FenixGeometrySetup.exe")
    def windows(path):
        return "Z:" + str(Path(path).resolve()).replace("/", "\\")
    def checked(path, expected):
        if digest(regular(path, 32 * 1024 * 1024)) != expected:
            raise PatchError("Geometry dependency checksum mismatch: " + path.name)
    with tempfile.TemporaryDirectory(prefix="fenix-geometry-") as directory:
        work = Path(directory)
        cabinet = work / "update.cab"
        progress("Preparing Direct2D geometry for Fenix route rendering …")
        wine.run(helper, "extract", windows(packages["Windows6.1-KB2670838-x64.msu"]),
                 "Windows6.1-KB2670838-x64.cab", windows(cabinet), timeout=180)
        checked(cabinet, "470b9ab769a46bbd1acc5e352156e2173a57cbbff03ff6f950d0989d9a150fbc")
        for member, expected in (("0", "e74c3bf4727f145ad81d9f1c2674cf6f8bab20acccee9d9c4cf2c375c75aed20"),
                                 ("1", "1cc95120454739d69a08fddd7e0e0f125e76115c4eba0748db94c7753c84788c")):
            wine.run(helper, "extract", windows(cabinet), member, windows(work / member), timeout=180)
            checked(work / member, expected)
        basis, output = work / "basis.dll", work / "geometry.dll"
        library = windows(packages["msdelta.dll"])
        wine.run(helper, "delta", library, "-", windows(work / "0"), windows(basis), timeout=60)
        checked(basis, "82dc3ddb8c3441ef2de0eb43ed187bc56b747e422a2f240cd33c978356746a7d")
        wine.run(helper, "delta", library, windows(basis), windows(work / "1"), windows(output), timeout=60)
        checked(output, GEOMETRY_SHA256)
        atomic(target, output.read_bytes(), 0o644)


def ensure_ui_fonts(prefix, runner, wine):
    # Chromium's Windows UI font fallback can recurse until its stack overflows
    # if Tahoma is absent from the 64-bit DirectWrite font collection. Old
    # profiles may contain the files and only the 32-bit registry entries.
    registry = regular(prefix / "system.reg", 64 * 1024 * 1024).read_text(errors="replace")
    section = re.search(r"(?m)^\[Software\\\\Microsoft\\\\Windows NT\\\\CurrentVersion\\\\Fonts\][^\[]*", registry)
    entries = section.group(0) if section else ""
    for name, family in (("tahoma.ttf", "Tahoma"), ("tahomabd.ttf", "Tahoma Bold")):
        value = family + " (TrueType)"
        if re.search(r'(?m)^"' + re.escape(value) + r'"="[^"\r\n]+"', entries):
            continue
        target = contained(prefix, "drive_c/windows/Fonts/" + name)
        if not target.exists():
            source = regular(runner / "files/share/wine/fonts" / name)
            atomic(target, source.read_bytes(), 0o644)
        regular(target)
        wine.run("reg", "add", r"HKLM\Software\Microsoft\Windows NT\CurrentVersion\Fonts",
                 "/v", value, "/t", "REG_SZ", "/d", name, "/f", "/reg:64")


def graphics_and_fonts(prefix, runner, wine):
    for arch, folder in (("x86_64", "system32"), ("i386", "syswow64")):
        for name in ("libvkd3d-1.dll", "libvkd3d-shader-1.dll", "libvkd3d-utils-1.dll"):
            source = regular(runner / f"files/lib/vkd3d/{arch}-windows/{name}")
            atomic(contained(prefix, f"drive_c/windows/{folder}/{name}"), source.read_bytes(), 0o644)
    fonts = {"arial.ttf": "Arial", "arialbd.ttf": "Arial Bold", "cour.ttf": "Courier New",
             "courbd.ttf": "Courier New Bold", "georgia.ttf": "Georgia", "times.ttf": "Times New Roman",
             "micross.ttf": "Microsoft Sans Serif"}
    for name, family in fonts.items():
        source = regular(runner / "files/share/fonts" / name)
        atomic(contained(prefix, "drive_c/windows/Fonts/" + name), source.read_bytes(), 0o644)
        wine.reg(r"HKLM\Software\Microsoft\Windows NT\CurrentVersion\Fonts", family + " (TrueType)", name)
    ensure_ui_fonts(prefix, runner, wine)
    wine.reg(r"HKCU\Software\Microsoft\Avalon.Graphics", "DisableHWAcceleration", "1", "REG_DWORD")
    wine.reg(r"HKCU\Software\Wine\Explorer", "ShowSystray", "0", "REG_DWORD")


def xml_settings(path, changes):
    if not path.exists():
        return False
    raw = regular(path, 2 * 1024 * 1024).read_bytes()
    if b"<!DOCTYPE" in raw.upper() or b"<!ENTITY" in raw.upper():
        raise PatchError("Unsupported XML declarations in " + path.name)
    root = ET.fromstring(raw)
    for tag, value in changes.items():
        node = root.find(tag)
        if node is None:
            node = ET.SubElement(root, tag)
        node.text = value
    atomic(path, ET.tostring(root, encoding="utf-8", xml_declaration=True))
    return True


def configure_prefix(prefix):
    if not (prefix / PROGRAM / "Fenix.exe").is_file():
        return False
    settings = contained(prefix, CONFIG / "fenixConfig.xml")
    persisted = contained(prefix, CONFIG / "persistancy.xml")
    ready = xml_settings(settings, {"displayMode": "CPU", "newRender": "false", "preferCPU": "true", "multithread": "true"})
    legacy = xml_settings(persisted, {"fcuReadoutsType": "0"})
    # The official installer owns the aircraft and its Community location.
    # Register its unchanged bootstrapper in the simulator's actual user profile.
    homes = [p for p in (prefix / "drive_c/users").iterdir() if p.name not in ("Public", "Default", "Default User") and p.is_dir()]
    candidates = [contained(prefix, p.relative_to(prefix) / "AppData/Roaming/Microsoft Flight Simulator 2024") for p in homes]
    candidates = [p for p in candidates if p.is_dir()]
    if len(candidates) != 1:
        raise PatchError("Could not identify one MSFS 2024 settings folder. Run the simulator once first.")
    path = candidates[0] / "exe.xml"
    if path.exists():
        raw = regular(path, 2 * 1024 * 1024).read_bytes()
        if b"<!DOCTYPE" in raw.upper() or b"<!ENTITY" in raw.upper():
            raise PatchError("Unsupported exe.xml declarations")
        root = ET.fromstring(raw)
    else:
        root = ET.Element("SimBase.Document", {"Type": "Launch", "version": "1,0"})
        ET.SubElement(root, "Descr").text = "Launch"
        ET.SubElement(root, "Filename").text = "exe.xml"
        ET.SubElement(root, "Disabled").text = "False"
    entries = [e for e in root.findall("Launch.Addon") if "fenix" in (e.findtext("Name", "") + e.findtext("Path", "")).lower()]
    entry = entries[0] if entries else ET.SubElement(root, "Launch.Addon")
    for duplicate in entries[1:]:
        root.remove(duplicate)
    for tag, value in {"Name": "FenixA320", "Disabled": "False", "ManualLoad": "False", "Path": r"C:\Program Files\FenixSim A320\deps\FenixBootstrapper.exe"}.items():
        node = entry.find(tag)
        if node is None:
            node = ET.SubElement(entry, tag)
        node.text = value
    atomic(path, ET.tostring(root, encoding="utf-8", xml_declaration=True))
    return ready and legacy


def copy_tree(source, destination):
    subprocess.run(["cp", "-a", "--reflink=auto", "--", str(source), str(destination)], check=True)


def replace_link(path, target):
    temp = path.with_name(".fenix-link-" + uuid.uuid4().hex)
    try:
        temp.symlink_to(target)
        os.replace(temp, path)
    finally:
        temp.unlink(missing_ok=True)


def retryable_preparation(root, state):
    """Only abandon a staging attempt when nothing was published to the runtime."""
    try:
        if (state.get("state") != "preparing" or state.get("upgrade_backup") or state.get("migrated")
                or not re.fullmatch(r"local/fenix-patch-[0-9]{8}T[0-9]{6}-[0-9a-f]{8}", state.get("work", ""))
                or not re.fullmatch(r"private/fenix-patch-backup-[0-9]{8}T[0-9]{6}-[0-9a-f]{8}", state.get("backup", ""))
                or not re.fullmatch(r"local/msfs-prefix.before-fenix-[0-9]{8}T[0-9]{6}-[0-9a-f]{8}", state.get("previous_prefix", ""))):
            return False
        prefix = root / "local/msfs-prefix"
        if ([prefix.stat().st_dev, prefix.stat().st_ino] != state.get("original_prefix_id")
                or os.readlink(root / "runner") != state.get("previous_runner")
                or (root / state["previous_prefix"]).exists() or (root / state["previous_prefix"]).is_symlink()):
            return False
        for relative in (state["work"], state["backup"]):
            path = contained(root, relative)
            if path.is_symlink() or not path.is_dir() or path.stat().st_uid != os.getuid():
                return False
        backup = root / state["backup"]
        for name in ("launch-msfs.sh", "xodus-wine-launch"):
            if digest(regular(root / "tools" / name)) != digest(regular(backup / name)):
                return False
        staged = root / state["work"] / "prefix"
        if staged.is_symlink() or (staged.exists() and (not staged.is_dir() or staged.stat().st_uid != os.getuid())):
            return False
        # A same-inode alias must never let staging cleanup stop the live profile.
        if staged.exists() and staged.samefile(prefix):
            return False
        return True
    except (OSError, ValueError, TypeError, PatchError):
        return False


def _retry_preparation(root, state, progress):
    if not retryable_preparation(root, state):
        raise PatchError("A previous patch transaction exists. Restore it before reinstalling.")
    progress("Retrying interrupted Fenix preparation automatically; the original profile is retained …")
    staged = root / state["work"] / "prefix"
    backup = root / state["backup"]
    if staged.exists():
        fd = os.open(backup / "retry.log", os.O_WRONLY | os.O_CREAT | os.O_APPEND | os.O_NOFOLLOW, 0o600)
        try:
            Wine(staged, (root / "runner").resolve(strict=True), fd).stop()
        finally:
            os.close(fd)
        ensure_idle(staged)
    if not retryable_preparation(root, state):
        raise PatchError("The original profile changed; automatic retry was stopped.")
    write_json(backup / "interrupted.json", state)
    (root / MARKER).unlink()
    # Keep the failed copy and logs. The next attempt always starts from the
    # unchanged active profile, so partial .NET changes never become live.


def install(runtime, bundle, progress=lambda _: None):
    host_check()
    bundle = verify_bundle(bundle)
    lock = manifest()
    with locked(runtime) as root:
        marker = root / MARKER
        upgrade = None
        if marker.exists():
            state = read_json(marker)
            if retryable_preparation(root, state):
                _retry_preparation(root, state, progress)
            elif state.get("state") == "installed" and state.get("version") == lock["version"]:
                verify_installed(root, state)
                progress("This patch version is already installed.")
                return
            elif state.get("state") == "installed" and state.get("version") in lock.get("previous_releases", {}):
                verify_installed(root, state)
                upgrade = state
            else:
                raise PatchError("A previous patch transaction exists. Restore it before reinstalling.")
        original_runner = (root / "runner").resolve(strict=True)
        variant = upgrade.get("variant") if upgrade else runner_variant(original_runner)
        lock = manifest(variant)
        from .. import runtime_scripts
        for name, accepted in lock["accepted_scripts"].items():
            accepted = [*accepted, *runtime_scripts.accepted(name)]
            if upgrade is not None:
                accepted = [*accepted, lock["previous_releases"][upgrade["version"]]["integration"][name]]
            if digest(regular(root / "tools" / name)) not in accepted:
                raise PatchError("Custom Flightdeck launch script detected; it will not be overwritten: " + name)
        prefix = root / "local/msfs-prefix"
        # Conservative bound; reflinks often consume much less physical storage.
        size = sum(p.stat().st_size for base in (prefix, original_runner) for p in base.rglob("*") if p.is_file() and not p.is_symlink())
        if shutil.disk_usage(root).free < size + 1024 ** 3:
            raise PatchError("Not enough free space for the independent Wine profile and runner copies.")
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S") + "-" + uuid.uuid4().hex[:8]
        backup = root / "private" / ("fenix-patch-backup-" + stamp)
        backup.mkdir(mode=0o700)
        work = root / "local" / ("fenix-patch-" + stamp)
        work.mkdir(mode=0o700)
        state = {"format": 1, "version": lock["version"], "state": "preparing", "work": str(work.relative_to(root)),
                 "backup": str(backup.relative_to(root)), "previous_runner": os.readlink(root / "runner"),
                 "previous_prefix": "local/msfs-prefix.before-fenix-" + stamp, "configured": False,
                 "original_prefix_id": [prefix.stat().st_dev, prefix.stat().st_ino], "variant": variant,
                 "proton_before": read_json(root / "private/proton-selection.json")
                 if (root / "private/proton-selection.json").exists() else None}
        if upgrade is not None:
            # Keep the original pre-patch restore point. The profile being
            # updated is retained separately, including later aircraft/account
            # data. Never bootstrap .NET again in an already patched profile.
            write_json(backup / "previous-patch.json", upgrade)
            state.update({key: upgrade[key] for key in
                          ("backup", "previous_runner", "previous_prefix", "original_prefix_id", "configured")})
            state["proton_before"] = upgrade.get("proton_before")
            state["upgrade_backup"] = str(backup.relative_to(root))
            state["upgrade_previous_prefix"] = "local/msfs-prefix.before-fenix-update-" + stamp
        for name in ("launch-msfs.sh", "xodus-wine-launch"):
            shutil.copy2(root / "tools" / name, backup / name)
        if (root / "private/import-manifest.json").is_file():
            shutil.copy2(root / "private/import-manifest.json", backup / "import-manifest.json")
        write_json(marker, state)
        staged = work / "prefix"
        runner = work / "runner"
        wine = None
        try:
            progress("Updating the Fenix patch; installed aircraft and settings are retained …" if upgrade else
                     "Copying Wine profile and runner; the original profile remains available …")
            copy_tree(prefix, staged)
            # A copied prefix may contain an absolute C: symlink. Never let
            # the staging installer write through it into the original.
            drive = staged / "dosdevices/c:"
            if drive.is_symlink():
                drive.unlink()
            drive.symlink_to("../drive_c")
            copy_tree(original_runner, runner)
            logpath = backup / "setup.log"
            with logpath.open("xb") as log:
                logpath.chmod(0o600)
                wine = Wine(staged, runner, log)
                try:
                    if upgrade is None:
                        prepare_framework(wine, root / "private/fenix-downloads", progress)
                        progress("Preparing fonts, graphics dependencies and Fenix settings …")
                        graphics_and_fonts(staged, runner, wine)
                        state["configured"] = configure_prefix(staged)
                    prepare_geometry(wine, root / "private/fenix-downloads", bundle, progress)
                finally:
                    wine.stop()
            # Bootstrap native Framework with the unchanged runner first.
            # Publish the overlay only after every staging Wine process exited.
            apply_overlay(staged, runner, bundle, variant)
            ensure_idle(prefix)
            state["state"] = "committing"
            write_json(marker, state)
            os.rename(prefix, root / state.get("upgrade_previous_prefix", state["previous_prefix"]))
            os.rename(staged, prefix)
            replace_link(root / "runner", runner)
            for name in ("launch-msfs.sh", "xodus-wine-launch"):
                atomic(root / "tools" / name, runtime_scripts.current(name).read_bytes(), 0o700)
            imported = root / "private/import-manifest.json"
            if imported.exists():
                info = read_json(imported)
                for name in ("launch-msfs.sh", "xodus-wine-launch"):
                    if "runtime_files" in info:
                        info["runtime_files"][name] = digest(root / "tools" / name)
                write_json(imported, info)
            selected = read_json(root / "private/proton-selection.json") if (root / "private/proton-selection.json").exists() else None
            if selected:
                selected = {**selected, "schema": 2, "runner": str(runner.relative_to(root))}
                write_json(root / "private/proton-selection.json", selected)
            state["state"] = "installed"
            write_json(marker, state)
            progress("Fenix compatibility patch updated." if upgrade else
                     "Patch installed. Install and sign in to Fenix, then apply the aircraft settings.")
        except BaseException:
            # Preserve a journal and all profiles; restore is explicitly available
            # after process failure or power loss, without guessing what finished.
            progress("Setup did not finish. The original profile is unchanged. Use Repair setup to retry."
                     if retryable_preparation(root, state) else
                     "Setup did not finish. The original profile or its backup is retained. Use Restore.")
            raise


def verify_installed(root, state):
    current = manifest(state.get("variant"))
    lock = current
    if state.get("version") != current["version"]:
        lock = current.get("previous_releases", {}).get(state.get("version"))
        if lock is None:
            raise PatchError("This installed patch version is not supported by the current installer.")
    runner = (root / "runner").resolve(strict=True)
    expected = contained(root, state["work"]) / "runner"
    if runner != expected:
        raise PatchError("The active runner changed since patch installation.")
    files = dict(current["runner_files"], **lock["files"])
    for name, sha in files.items():
        if digest(regular(runner / name)) != sha:
            raise PatchError("Installed patch file changed: " + name)
    for name, sha in lock.get("prefix_files", {}).items():
        if digest(regular(contained(root / "local/msfs-prefix", name))) != sha:
            raise PatchError("Installed Fenix dependency changed: " + name)


def restore(runtime, progress=lambda _: None):
    with locked(runtime, recovery=True) as root:
        marker = root / MARKER
        state = read_json(marker)
        if state.get("migrated"):
            raise PatchError("This migrated profile has no managed pre-patch restore point.")
        backup = contained(root, state["backup"])
        previous = contained(root, state["previous_prefix"])
        work = contained(root, state["work"])
        if state.get("state") == "restored":
            raise PatchError("This patch is already restored.")
        if (work / "prefix").exists():
            ensure_idle(work / "prefix")
        prefix = root / "local/msfs-prefix"
        state["state"] = "restoring"
        write_json(marker, state)
        if previous.exists():
            if prefix.exists():
                retained = root / "local" / ("msfs-prefix.fenix-retained-" + uuid.uuid4().hex)
                os.rename(prefix, retained)
                state["retained_prefix"] = str(retained.relative_to(root))
                write_json(marker, state)
            os.rename(previous, prefix)
        elif not prefix.exists() or [prefix.stat().st_dev, prefix.stat().st_ino] != state.get("original_prefix_id"):
            raise PatchError("The original profile backup is missing; nothing was removed.")
        replace_link(root / "runner", state["previous_runner"])
        for name in ("launch-msfs.sh", "xodus-wine-launch"):
            atomic(root / "tools" / name, regular(backup / name).read_bytes(), 0o700)
        if (backup / "import-manifest.json").exists():
            atomic(root / "private/import-manifest.json", (backup / "import-manifest.json").read_bytes())
        selected = state.get("proton_before")
        if selected:
            write_json(root / "private/proton-selection.json", selected)
        else:
            (root / "private/proton-selection.json").unlink(missing_ok=True)
        state["state"] = "restored"
        write_json(backup / "restored.json", state)
        marker.unlink()
        progress("Original runner and Windows profile restored. The newer profile is retained locally.")


def configure(runtime, progress=lambda _: None):
    with locked(runtime) as root:
        state = read_json(root / MARKER)
        if state.get("state") != "installed":
            raise PatchError("Finish patch installation first.")
        verify_installed(root, state)
        prefix = root / "local/msfs-prefix"
        # Preserve settings on every user-triggered reconfiguration.
        backup = contained(root, state["backup"]) / ("settings-" + uuid.uuid4().hex)
        backup.mkdir(mode=0o700)
        for path in (prefix / CONFIG).glob("*.xml"):
            if path.name in ("fenixConfig.xml", "persistancy.xml"):
                shutil.copy2(regular(path), backup / path.name)
        state["configured"] = configure_prefix(prefix)
        write_json(root / MARKER, state)
        if not state["configured"]:
            raise PatchError("Start Fenix once and sign in, then close it and apply settings again. CPU rendering and Legacy readouts need its initial settings files.")
        progress("CPU displays, Legacy readouts and Fenix autostart configured.")


def manager_path(prefix):
    candidates = list((prefix / "drive_c/users").glob("*/AppData/Local/FenixApp/current/FenixApp.exe"))
    candidates = [p for p in candidates if p.is_file() and p.resolve().is_relative_to(prefix.resolve())]
    if len(candidates) != 1:
        raise PatchError("Install the official Fenix Installer first.")
    return regular(candidates[0])


def windows_app(runtime, executable=None, progress=lambda _: None, *, manager=False, wait=None):
    with locked(runtime) as root:
        if (root / MARKER).exists():
            state = read_json(root / MARKER)
            if state.get("state") != "installed":
                raise PatchError("Install the compatibility patch first.")
            verify_installed(root, state)
        elif executable or not (root / "private/fenix-compat.json").is_file():
            raise PatchError("Install the compatibility patch first.")
        prefix = root / "local/msfs-prefix"
        if executable:
            app = Path(executable).expanduser().resolve(strict=True)
            regular(app, 1024 ** 3)
            with app.open("rb") as stream:
                signature = stream.read(2)
            if app.suffix.lower() != ".exe" or signature != b"MZ":
                raise PatchError("Select the official Fenix Installer .exe from your Fenix account.")
        elif manager:
            app = manager_path(prefix)
        else:
            app = regular(prefix / PROGRAM / "Fenix.exe")
        runner = (root / "runner").resolve(strict=True)
        path = root / "private/fenix-app.log"
        fd = os.open(path, os.O_CREAT | os.O_APPEND | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
        try:
            wine = Wine(prefix, runner, fd)
            ensure_ui_fonts(prefix, runner, wine)
            # Wine's fallback notification area otherwise becomes a separate
            # blank window on desktops without an XEmbed tray. Scope to this
            # runtime; the main Fenix UI remains available through Flightdeck.
            wine.reg(r"HKCU\Software\Wine\Explorer", "ShowSystray", "0", "REG_DWORD")
            progress("Fenix is open. Complete its setup or sign-in, then close the application to continue.")
            args = [str(wine_binary(runner)), str(app)]
            options = dict(cwd=app.parent, env=wine_env(prefix, runner), stdin=subprocess.DEVNULL,
                           stdout=fd, stderr=subprocess.STDOUT)
            if wait is None:
                subprocess.run(args, **options, check=True)
            else:
                # A launcher may offer cancellation while retaining this exact
                # runtime lease through companion cleanup. The default CLI/GUI
                # still simply waits for the official application to exit.
                child = subprocess.Popen(args, **options)
                try:
                    result = wait(child)
                except BaseException:
                    if child.poll() is None:
                        child.terminate()
                        try:
                            child.wait(timeout=3)
                        except subprocess.TimeoutExpired:
                            child.kill()
                            child.wait()
                    raise
                if result:
                    raise subprocess.CalledProcessError(result, args)
        finally:
            os.close(fd)


def snapshot(runtime):
    result = {"state": "unavailable", "version": manifest()["version"], "configured": False,
              "installed": False, "fenix_installed": False, "can_restore": False, "message": ""}
    try:
        root = runtime_path(runtime, recovery=True)
        result["fenix_installed"] = (root / "local/msfs-prefix" / PROGRAM / "Fenix.exe").is_file()
        try:
            manager_path(root / "local/msfs-prefix")
            result["manager_installed"] = True
        except PatchError:
            result["manager_installed"] = False
        marker = root / MARKER
        if marker.exists():
            state = read_json(marker)
            result.update(state=state.get("state", "interrupted"), installed=state.get("state") == "installed",
                          configured=state.get("configured") is True, can_restore=not state.get("migrated"),
                          can_retry=retryable_preparation(root, state),
                          installed_version=state.get("version"),
                          update_available=state.get("state") == "installed" and
                          state.get("version") in manifest().get("previous_releases", {}))
        elif (root / "private/fenix-compat.json").exists():
            result.update(state="legacy", message="An earlier local Fenix patch is active. Keep using it; automatic replacement is disabled.")
        else:
            host_check()
            runner_variant((root / "runner").resolve(strict=True))
            result["state"] = "available"
        try:
            ensure_idle(root / "local/msfs-prefix")
            result["idle"] = True
        except PatchError:
            result["idle"] = False
    except (OSError, ValueError, KeyError, PatchError) as error:
        result["message"] = str(error)
    return result
