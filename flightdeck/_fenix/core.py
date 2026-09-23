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


def manifest():
    return read_json(ROOT / "bundle.json")


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
    if regular(runner / "version", 4096).read_text().strip() != lock["runner_version"]:
        raise PatchError("Unsupported Wine runner. Use the pinned Flightdeck Xodus runner.")
    for name, expected in lock["runner_files"].items():
        if digest(regular(runner / name)) != expected:
            raise PatchError("The Wine runner differs from the supported build: " + name)


def verify_bundle(bundle):
    bundle = Path(bundle).expanduser().resolve(strict=True)
    lock = manifest()
    if read_json(bundle / "bundle.json") != lock:
        raise PatchError("The patch bundle does not match this installer version.")
    for relative, expected in lock["files"].items():
        path = contained(bundle / "payload", relative)
        if digest(regular(path)) != expected:
            raise PatchError("Patch checksum mismatch: " + relative)
    for name, expected in lock["integration"].items():
        if digest(regular(contained(bundle / "integration", name))) != expected:
            raise PatchError("Integration checksum mismatch: " + name)
    return bundle


def download(url, destination, expected, progress=lambda _: None, max_size=180 * 1024 * 1024):
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
               WINE=str(runner / "files/bin/wine"), WINESERVER=str(runner / "files/bin/wineserver"))
    for name in ("WINE_DLL_FILE_MAP", "WINE_D2D1_GEOMETRY_PROVIDER", "WINELOADER", "WINEDLLPATH"):
        env.pop(name, None)
    env["WINEDLLOVERRIDES"] = "winemenubuilder.exe=d"
    old = env.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "")
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (old + " --disable-features=HideCursorWhileTyping").strip()
    return env


class Wine:
    def __init__(self, prefix, runner, log):
        self.prefix, self.runner, self.log = prefix, runner, log
        self.env = wine_env(prefix, runner)

    def run(self, *args, env=None, accepted=(0,), timeout=1800):
        result = subprocess.run([str(self.runner / "files/bin/wine"), *map(str, args)],
            env={**self.env, **(env or {})}, cwd=self.prefix, stdin=subprocess.DEVNULL,
            stdout=self.log, stderr=subprocess.STDOUT, timeout=timeout)
        if result.returncode not in accepted:
            raise PatchError("Windows setup failed (exit %d). See the private Fenix setup log." % result.returncode)

    def reg(self, key, name, value, kind="REG_SZ"):
        self.run("reg", "add", key, "/v", name, "/t", kind, "/d", value, "/f")

    def stop(self):
        # Used only for the private staging profile we created, never a user's running game.
        subprocess.run([str(self.runner / "files/bin/wineserver"), "-k"], env=self.env,
                       stdout=self.log, stderr=subprocess.STDOUT, timeout=30, check=True)
        subprocess.run([str(self.runner / "files/bin/wineserver"), "-w"], env=self.env,
                       stdout=self.log, stderr=subprocess.STDOUT, timeout=30, check=True)


def has_framework(prefix):
    try:
        content = regular(prefix / "system.reg", 64 * 1024 * 1024).read_text(errors="replace")
        section = re.search(r"\[Software\\\\Microsoft\\\\NET Framework Setup\\\\NDP\\\\v4\\\\Full\][^\[]*", content)
        release = re.search(r'"Release"=dword:([0-9a-fA-F]+)', section.group(0)) if section else None
        return bool(release and int(release.group(1), 16) >= 528040 and
                    (prefix / "drive_c/windows/Microsoft.NET/Framework64/v4.0.30319/clr.dll").is_file())
    except OSError:
        return False


def prepare_framework(wine, cache, progress):
    if has_framework(wine.prefix):
        progress("Microsoft .NET Framework 4.8 is already installed.")
        return
    packages = {}
    for name, (url, expected) in DOWNLOADS.items():
        progress("Downloading Microsoft " + name)
        packages[name] = download(url, cache / name, expected)
    progress("Installing Microsoft .NET Framework. This can take several minutes …")
    listing = subprocess.run([str(wine.runner / "files/bin/wine"), "uninstaller", "--list"],
                             env=wine.env, capture_output=True, timeout=120, check=True).stdout.decode(errors="replace")
    for line in listing.splitlines():
        guid, _, title = line.partition("|")
        if title.startswith("Wine Mono") and re.fullmatch(r"\{[a-fA-F0-9-]{36}\}", guid):
            wine.run("uninstaller", "--silent", "--remove", guid)
    wine.reg(r"HKCU\Software\Wine", "Version", "winxp")
    try:
        wine.run(packages["dotNetFx40_Full_x86_x64.exe"], "/q", "/c:install.exe /q /norestart",
                 env={"WINEDLLOVERRIDES": "fusion=b;winemenubuilder.exe=d"}, accepted=(0, 194))
        wine.reg(r"HKCU\Software\Wine\DllOverrides", "mscoree", "native")
        for key in (r"HKLM\Software\Microsoft\.NETFramework", r"HKLM\Software\Wow6432Node\Microsoft\.NETFramework"):
            wine.reg(key, "OnlyUseLatestCLR", "1", "REG_DWORD")
        wine.reg(r"HKCU\Software\Wine", "Version", "win7")
        wine.run(packages["NDP48-x86-x64-AllOS-ENU.exe"], "/q", "/norestart",
                 env={"WINEDLLOVERRIDES": "fusion=b;winemenubuilder.exe=d"}, accepted=(0, 194))
    finally:
        wine.reg(r"HKCU\Software\Wine", "Version", "win10")
    if not has_framework(wine.prefix):
        # Wine may not have flushed the registry yet.
        wine.run("wineboot", "-u")
        wine.stop()
    if not has_framework(wine.prefix):
        raise PatchError("Microsoft .NET Framework 4.8 was not detected after setup.")


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
    wine.reg(r"HKCU\Software\Microsoft\Avalon.Graphics", "DisableHWAcceleration", "1", "REG_DWORD")


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


def install(runtime, bundle, progress=lambda _: None):
    host_check()
    bundle = verify_bundle(bundle)
    lock = manifest()
    with locked(runtime) as root:
        marker = root / MARKER
        if marker.exists():
            state = read_json(marker)
            if state.get("state") == "installed" and state.get("version") == lock["version"]:
                verify_installed(root, state)
                progress("This patch version is already installed.")
                return
            raise PatchError("A previous patch transaction exists. Restore it before reinstalling.")
        original_runner = (root / "runner").resolve(strict=True)
        verify_runner(original_runner, lock)
        for name, accepted in lock["accepted_scripts"].items():
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
                 "original_prefix_id": [prefix.stat().st_dev, prefix.stat().st_ino]}
        for name in ("launch-msfs.sh", "xodus-wine-launch"):
            shutil.copy2(root / "tools" / name, backup / name)
        if (root / "private/import-manifest.json").is_file():
            shutil.copy2(root / "private/import-manifest.json", backup / "import-manifest.json")
        write_json(marker, state)
        staged = work / "prefix"
        runner = work / "runner"
        wine = None
        try:
            progress("Copying Wine profile and runner; the original profile remains available …")
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
                    prepare_framework(wine, root / "private/fenix-downloads", progress)
                    progress("Preparing fonts, graphics dependencies and Fenix settings …")
                    graphics_and_fonts(staged, runner, wine)
                    state["configured"] = configure_prefix(staged)
                finally:
                    wine.stop()
            # Bootstrap native Framework with the unchanged runner first.
            # Publish the overlay only after every staging Wine process exited.
            for name in lock["files"]:
                data = (bundle / "payload" / name).read_bytes()
                atomic(contained(runner, name), data, 0o755 if "/bin/" in name else 0o644)
                if name.startswith("files/lib/wine/x86_64-windows/"):
                    atomic(contained(staged, "drive_c/windows/system32/" + Path(name).name), data, 0o644)
            atomic(contained(staged, "drive_c/windows/system32/FenixWindowGuard.exe"),
                   (bundle / "integration/FenixWindowGuard.exe").read_bytes(), 0o644)
            ensure_idle(prefix)
            state["state"] = "committing"
            write_json(marker, state)
            os.rename(prefix, root / state["previous_prefix"])
            os.rename(staged, prefix)
            replace_link(root / "runner", runner)
            for name in ("launch-msfs.sh", "xodus-wine-launch"):
                atomic(root / "tools" / name, (bundle / "integration" / name).read_bytes(), 0o700)
            imported = root / "private/import-manifest.json"
            if imported.exists():
                info = read_json(imported)
                for name in ("launch-msfs.sh", "xodus-wine-launch"):
                    info.setdefault("runtime_files", {})[name] = digest(root / "tools" / name)
                write_json(imported, info)
            state["state"] = "installed"
            write_json(marker, state)
            progress("Patch installed. Install and sign in to Fenix, then apply the aircraft settings.")
        except BaseException:
            # Preserve a journal and all profiles; restore is explicitly available
            # after process failure or power loss, without guessing what finished.
            progress("Setup did not finish. The original profile or its backup is retained. Use Restore.")
            raise


def verify_installed(root, state):
    lock = manifest()
    runner = (root / "runner").resolve(strict=True)
    expected = contained(root, state["work"]) / "runner"
    if runner != expected:
        raise PatchError("The active runner changed since patch installation.")
    for name, sha in lock["files"].items():
        if digest(regular(runner / name)) != sha:
            raise PatchError("Installed patch file changed: " + name)


def restore(runtime, progress=lambda _: None):
    with locked(runtime, recovery=True) as root:
        marker = root / MARKER
        state = read_json(marker)
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
        elif not manager or not (root / "private/fenix-compat.json").is_file():
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
            progress("Fenix is open. Complete its setup or sign-in, then close the application to continue.")
            args = [str(runner / "files/bin/wine"), str(app)]
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
                          configured=state.get("configured") is True, can_restore=True)
        elif (root / "private/fenix-compat.json").exists():
            result.update(state="legacy", message="An earlier local Fenix patch is active. Keep using it; automatic replacement is disabled.")
        else:
            host_check()
            verify_runner((root / "runner").resolve(strict=True), manifest())
            result["state"] = "available"
        try:
            ensure_idle(root / "local/msfs-prefix")
            result["idle"] = True
        except PatchError:
            result["idle"] = False
    except (OSError, ValueError, KeyError, PatchError) as error:
        result["message"] = str(error)
    return result
