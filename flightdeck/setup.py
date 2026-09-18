# SPDX-License-Identifier: MIT
"""Local, cancellable runtime preparation from explicitly selected user files.

Only a new private staging directory is written. Source prefixes, game files,
account stores and already prepared runtimes are never changed by this module.
"""
from __future__ import annotations

import copy
import ctypes
from dataclasses import dataclass
import errno
from functools import lru_cache
import hashlib
import importlib
import itertools
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile
import threading
import uuid

from .i18n import error_message, message, translate_message

ARTIFACTS = (
    "bin/xodus-cli", "bin/xodus-service", "runtime/xgameruntime.dll",
    "builtin/x86_64-windows/xodus_store_test.dll",
    "builtin/x86_64-unix/xodus_store_test.so",
)
CONNECTED_STORAGE_HELPER = "bin/flightdeck-connected-storage.exe"
CONNECTED_STORAGE_FEATURE = "connected-storage-read-v1"


def artifact_names(manifest):
    """Legacy game runtimes need five files; an advertised helper must be pinned."""
    files = manifest.get("files")
    features = manifest.get("features", [])
    if (not isinstance(files, dict) or not isinstance(features, list)
            or any(not isinstance(feature, str) for feature in features)):
        raise SetupError("Das Build-Manifest enthält keine gültige Dateiliste.")
    has_helper = CONNECTED_STORAGE_HELPER in files
    if CONNECTED_STORAGE_FEATURE in features and not has_helper:
        raise SetupError("Das Build-Manifest enthält keine gültige Dateiliste.")
    return ARTIFACTS + ((CONNECTED_STORAGE_HELPER,) if has_helper else ())


RUNTIME_FILES = (
    "launch-msfs.sh", "play-msfs.sh", "runtime-env.sh", "xodus.sh",
    "xodus-service.sh", "xodus-wine-launch",
)
ACTIVE = {"checking", "ready", "installing"}
EXISTING_FILES = (
    ("launcher", "Startprogramm", "tools/play-msfs.sh", True),
    ("game", "Spielpaket", "games/MSFS2024/FlightSimulator2024.exe", False),
    ("prefix", "Wine-Umgebung", "local/msfs-prefix/system.reg", False),
    ("bridge", "Kompatibilitätsbibliothek", "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll", False),
)


class SetupError(ValueError):
    pass


class SetupCancelled(SetupError):
    pass


def interrupted(cancel):
    if cancel is not None and cancel.is_set():
        raise SetupCancelled("Einrichtung abgebrochen.")


def digest(path, cancel=None):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            interrupted(cancel)
            value.update(chunk)
    return value.hexdigest()


def resource_paths(source_root=None):
    if source_root is not None:
        root = Path(source_root)
        return root / "scripts/runtime", root / "compat/upstreams.lock.json"
    packaged = Path(__file__).resolve().parent / "resources"
    if (packaged / "runtime").is_dir() and (packaged / "upstreams.lock.json").is_file():
        return packaged / "runtime", packaged / "upstreams.lock.json"
    root = Path(__file__).resolve().parents[1]
    return root / "scripts/runtime", root / "compat/upstreams.lock.json"


def data_home():
    folder = Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share"))).expanduser()
    return folder if folder.is_absolute() else Path.home() / ".local/share"


def _region_file(path, maximum):
    """Small, fixed system hints only; never wait on a pipe or scan user files."""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
        with os.fdopen(fd, "rb") as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size > maximum:
                return ""
            raw = stream.read(maximum + 1)
            return raw.decode("utf-8") if len(raw) <= maximum else ""
    except (OSError, UnicodeError):
        return ""


def _zone_name(value):
    if not isinstance(value, str) or len(value) > 256:
        return ""
    value = value.removeprefix(":")
    if value.startswith("/usr/share/zoneinfo/"):
        value = value[len("/usr/share/zoneinfo/"):]
        for prefix in ("posix/", "right/"):
            value = value.removeprefix(prefix)
    return value if re.fullmatch(r"[A-Za-z0-9_+-]+(?:/[A-Za-z0-9_+-]+)*", value) else ""


def _local_timezone():
    # An explicit but unmappable TZ (including UTC or a POSIX offset) must not
    # silently borrow the machine's possibly unrelated configured timezone.
    if "TZ" in os.environ:
        return _zone_name(os.environ["TZ"])
    try:
        target = os.readlink("/etc/localtime")
        name = _zone_name(os.path.normpath(os.path.join("/etc", target)))
        if name:
            return name
    except OSError:
        pass
    return _zone_name(_region_file("/etc/timezone", 512).strip())


@lru_cache(maxsize=1)
def _zone_regions():
    # The system tzdata table is stable for this service process. Load it once,
    # not on every status poll. A grouped zone is useful only for one country.
    for path in ("/usr/share/zoneinfo/zone.tab", "/usr/share/zoneinfo/zone1970.tab"):
        text = _region_file(path, 64 * 1024)
        if not text:
            continue
        regions = {}
        for line in text.splitlines():
            if not line or line.startswith("#"):
                continue
            fields = line.split("\t")
            if len(fields) < 3 or not _zone_name(fields[2]):
                continue
            countries = fields[0].split(",")
            if not all(re.fullmatch(r"[A-Z]{2}", country) for country in countries):
                countries = [""]
            regions.setdefault(fields[2], set()).update(countries)
        return {zone: next(iter(countries)) for zone, countries in regions.items()
                if len(countries) == 1 and "" not in countries}
    return {}


def suggested_market():
    """Suggest a new-install region from local evidence, never from language alone."""
    region = _zone_regions().get(_local_timezone())
    if region:
        return region
    for category in ("LC_ADDRESS", "LC_MONETARY", "LC_ALL", "LC_MESSAGES", "LANG"):
        value = os.environ.get(category, "")
        if len(value) > 256:
            continue
        match = re.fullmatch(r"[A-Za-z]{2,3}(?:_[A-Za-z]{4})?_([A-Z]{2})(?:\.[A-Za-z0-9_-]+)?(?:@[A-Za-z0-9_-]+)?", value)
        if match:
            return match[1]
    return ""


def bootstrap_module():
    try:
        return importlib.import_module(".bootstrap", __package__)
    except ModuleNotFoundError as error:
        if error.name != __package__ + ".bootstrap":
            raise
        return None


def existing_checks(path):
    """Inspect prepared-runtime files without reading contents or creating locks."""
    checks = []
    for key, label, relative, executable in EXISTING_FILES:
        try:
            regular(path / relative, label, executable)
            detail, ok = "Vorhanden", True
        except SetupError as error:
            detail, ok = error_message(error), False
        checks.append({"id": key, "label": label, "ok": ok, "detail": detail})
    private = path / "private"
    private_ok = private.is_dir() and not private.is_symlink()
    checks.append({"id": "private", "label": "Privater Datenordner", "ok": private_ok,
                   "detail": "Vorhanden" if private_ok else "Der private Datenordner der Runtime fehlt oder ist ungültig."})
    if private_ok:
        lock = private / "play.lock"
        lock_ok = not lock.is_symlink() and (not lock.exists() or lock.is_file())
        checks.append({"id": "lock", "label": "Runtime-Sperrdatei", "ok": lock_ok,
                       "detail": "Geprüft" if lock_ok else "Die Runtime-Sperrdatei ist ungültig."})
    return checks


def path_input(value, label, *, exists=True):
    if not isinstance(value, str) or not value.strip() or len(value) > 4096:
        raise SetupError(message("Bitte den Pfad für {label} angeben.", label=message(label)))
    if any(ord(char) < 32 or char in ":;|" for char in value):
        raise SetupError(message("Der Pfad für {label} enthält von Wine nicht unterstützte Zeichen.", label=message(label)))
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise SetupError(message("Der Pfad für {label} muss absolut sein.", label=message(label)))
    if not exists and (path.exists() or path.is_symlink()):
        raise SetupError("Der Zielordner existiert bereits. Bitte einen neuen Ordner wählen.")
    try:
        resolved = path.resolve(strict=exists)
    except (OSError, RuntimeError):
        raise SetupError(message("Der Pfad für {label} wurde nicht gefunden oder ist nicht lesbar.", label=message(label))) from None
    if exists and not resolved.is_dir():
        raise SetupError(message("Bitte für {label} einen Ordner auswählen.", label=message(label)))
    return resolved


def regular(path, label, executable=False):
    if not path.is_file() or not os.access(path, os.R_OK):
        raise SetupError(message("{label} fehlt oder ist nicht lesbar.", label=message(label)))
    if executable and not os.access(path, os.X_OK):
        raise SetupError(message("{label} ist nicht ausführbar. Bitte die Dateirechte prüfen.", label=message(label)))


def prefix_system32(prefix):
    path = prefix
    for name in ("drive_c", "windows", "system32"):
        path /= name
        if path.is_symlink() or not path.is_dir():
            raise SetupError("Die Windows-Systemordner im Prefix müssen echte Ordner sein, keine Verknüpfungen.")
    return path


def prefix_size(root, cancel):
    size = 0
    def failed(_):
        raise SetupError("Die Wine-Umgebung enthält einen nicht lesbaren Ordner.")
    for directory, _, files in os.walk(root, followlinks=False, onerror=failed):
        interrupted(cancel)
        for name in files:
            info = (Path(directory) / name).lstat()
            if stat.S_ISREG(info.st_mode):
                size += info.st_size
    return size


@dataclass
class Plan:
    inputs: dict
    sources: dict
    destination: Path
    tools: Path
    original: Path
    original_hash: str
    manifest: dict
    plugins: Path | None
    prefix_bytes: int


def preflight(data, *, source_root=None, notify=None, cancel=None):
    """Read-only validation. notify receives completed checks or a live phase."""
    def event(phase, message, progress=None, check=None):
        interrupted(cancel)
        if notify:
            notify(phase, message, progress, check)

    def check(key, label, action):
        event(key, label)
        try:
            result = action()
        except (OSError, ValueError) as error:
            if isinstance(error, SetupCancelled):
                raise
            detail = error_message(error) if isinstance(error, SetupError) else message("{label}: Dateien oder Zugriffsrechte prüfen.", label=message(label))
            event(key, detail, check={"id": key, "label": label, "ok": False, "detail": detail})
            raise SetupError(detail) from None
        event(key, label, check={"id": key, "label": label, "ok": True, "detail": "Geprüft"})
        return result

    if not isinstance(data, dict):
        raise SetupError("Ungültige Einrichtungseinstellungen.")
    sources = check("paths", "Eingabeordner", lambda: {
        name: path_input(data.get(name + "_path"), label) for name, label in
        (("artifacts", "Build-Artefakte"), ("game", "Spielpaket"), ("runner", "Proton-Runner"), ("prefix", "Wine-Umgebung"))
    })
    destination = check("destination", "Neuer Zielordner", lambda: path_input(data.get("destination_path"), "Zielordner", exists=False))
    if any(destination == path or destination.is_relative_to(path) for path in sources.values()):
        raise SetupError("Der Zielordner darf nicht innerhalb eines Eingabeordners liegen.")
    market = data.get("market")
    if not isinstance(market, str) or not re.fullmatch(r"[A-Z]{2}", market):
        raise SetupError("Bitte einen Ländercode mit zwei Großbuchstaben wählen, zum Beispiel AT.")
    local_saves = data.get("local_saves", True)
    if not isinstance(local_saves, bool):
        raise SetupError("Die Auswahl für lokale Spielstände muss Ja oder Nein sein.")
    tools, lock = resource_paths(source_root)

    def support_files():
        for name in RUNTIME_FILES:
            regular(tools / name, message("Mitgeliefertes Startskript: {name}", name=name))
        regular(lock, "Mitgelieferte Runner-Beschreibung")
        expected = json.loads(lock.read_text())["runner"]["original_runtime_sha256"]
        if not isinstance(expected, str) or not re.fullmatch(r"[a-f0-9]{64}", expected):
            raise SetupError("Die Runner-Beschreibung ist ungültig. Flightdeck bitte erneut installieren.")
        return expected
    try:
        expected = check("support", "Flightdeck-Installationsdateien", support_files)
    except (KeyError, TypeError):
        raise SetupError("Die Runner-Beschreibung ist unvollständig. Flightdeck bitte erneut installieren.") from None

    def game_files():
        for name in ("FlightSimulator2024.exe", ".xodus-streaming.msixvc"):
            regular(sources["game"] / name, message("Spielpaket: {name}", name=name))
        config = sources["game"] / "MicrosoftGame.config"
        if not config.is_file():
            config = sources["game"] / "MicrosoftGame.Config"
        regular(config, "MicrosoftGame.Config im Spielpaket")
    check("game", "Eigenes Xbox-PC-Spielpaket", game_files)
    original = sources["runner"] / "files/lib/wine/x86_64-windows/xgameruntime.dll"

    def runner_files():
        regular(sources["runner"] / "files/bin/wine", "Wine im Proton-Runner", executable=True)
        regular(original, "Original-Runtime im Proton-Runner")
        if digest(original, cancel) != expected:
            raise SetupError("Dieser Proton-Runner passt nicht zur geprüften Runtime. Bitte die in Flightdeck dokumentierte Runner-Version verwenden.")
    check("runner", "Kompatibler Proton-Runner", runner_files)

    def prefix_files():
        for name in ("system.reg", "user.reg"):
            regular(sources["prefix"] / name, message("Vorbereitete Wine-Umgebung: {name}", name=name))
        prefix_system32(sources["prefix"])
    check("prefix", "Vorbereitete Wine-Umgebung", prefix_files)

    def artifacts():
        path = sources["artifacts"] / "manifest.json"
        regular(path, "manifest.json der Build-Artefakte")
        if path.stat().st_size > 2 * 1024 * 1024:
            raise SetupError("Das Build-Manifest ist ungewöhnlich groß oder ungültig.")
        manifest = json.loads(path.read_text())
        files = manifest.get("files") if isinstance(manifest, dict) else None
        if not isinstance(files, dict):
            raise SetupError("Das Build-Manifest enthält keine gültige Dateiliste.")
        for relative in artifact_names(manifest):
            candidate = sources["artifacts"] / relative
            regular(candidate, message("Build-Artefakt: {name}", name=relative), executable=relative.startswith("bin/"))
            if not isinstance(files.get(relative), str) or digest(candidate, cancel) != files[relative]:
                raise SetupError(message("Prüfsumme stimmt nicht: {name}. Bitte die Artefakte erneut bauen oder vollständig kopieren.", name=relative))
        return manifest
    manifest = check("artifacts", "Build-Manifest und Prüfsummen", artifacts)
    plugins = None
    if data.get("media_plugins_path"):
        plugins = check("media", "Zusätzliche Medienmodule", lambda: path_input(data["media_plugins_path"], "Medienmodule"))
    event("space", "Speicherbedarf der Wine-Kopie wird ermittelt …")
    size = prefix_size(sources["prefix"], cancel)

    def space():
        parent = destination.parent
        while not parent.exists():
            parent = parent.parent
        if not parent.is_dir() or not os.access(parent, os.W_OK | os.X_OK):
            raise SetupError("Der Zielordner kann hier nicht angelegt werden. Bitte Schreibrechte oder Speicherort ändern.")
        required = size + sum((sources["artifacts"] / name).stat().st_size for name in artifact_names(manifest)) * 2 + original.stat().st_size + 16 * 1024 * 1024
        if shutil.disk_usage(parent).free < required:
            raise SetupError(message("Am Ziel fehlt Speicherplatz. Für eine vollständige Kopie werden ungefähr {gib} GiB benötigt.", gib=format(required / (1024 ** 3), ".1f")))
    check("space", "Freier Speicherplatz für eine unabhängige Kopie", space)
    normalized = {name + "_path": str(path) for name, path in sources.items()}
    normalized.update(destination_path=str(destination), market=market, local_saves=local_saves, media_plugins_path=str(plugins) if plugins else "", mode="prepare")
    return Plan(normalized, sources, destination, tools, original, expected, manifest, plugins, size)


def _copy_prefix(source, destination, cancel):
    try:
        process = subprocess.Popen(["cp", "-a", "--reflink=auto", "--", str(source), str(destination)],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError:
        raise SetupError("Die Kopierfunktion cp ist nicht verfügbar.") from None
    try:
        while process.poll() is None:
            if cancel is not None and cancel.wait(.1):
                raise SetupCancelled("Einrichtung abgebrochen.")
            if cancel is None:
                process.wait()
        if process.returncode:
            raise SetupError("Die Wine-Umgebung konnte nicht vollständig kopiert werden. Bitte Leserechte und Speicherplatz prüfen.")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def _copy_file(source, target):
    """Replace the directory entry, never write through a prefix file symlink."""
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, name = tempfile.mkstemp(prefix=".flightdeck-copy-", dir=target.parent)
    os.close(fd)
    try:
        shutil.copy2(source, name)
        os.replace(name, target)
    finally:
        Path(name).unlink(missing_ok=True)


def _publish(source, destination):
    # Linux renameat2 provides atomic publication without replacing even an
    # empty directory another process created after preflight.
    library = ctypes.CDLL(None, use_errno=True)
    rename = getattr(library, "renameat2", None)
    if rename is None:
        raise SetupError("Dieses Linux stellt die sichere Ordnerübernahme renameat2 nicht bereit.")
    rename.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    rename.restype = ctypes.c_int
    if rename(-100, os.fsencode(source), -100, os.fsencode(destination), 1):
        code = ctypes.get_errno()
        if code in (errno.EEXIST, errno.ENOTEMPTY):
            raise SetupError("Der Zielordner wurde inzwischen angelegt. Er wurde nicht verändert.")
        raise OSError(code, "Runtime konnte nicht an den Zielort übernommen werden")


def prepare(plan, *, notify=None, cancel=None, xdg_root=None):
    """Create and verify a runtime; return its path only after atomic publish."""
    def event(phase, message, progress=None):
        interrupted(cancel)
        if notify:
            notify(phase, message, progress, None)
    destination = plan.destination
    if destination.exists() or destination.is_symlink():
        raise SetupError("Der Zielordner existiert bereits und wird nicht überschrieben.")
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = Path(tempfile.mkdtemp(prefix=".flightdeck-setup-", dir=destination.parent))
    try:
        event("prepare", "Privater Runtimeordner wird vorbereitet …", 5)
        for name in ("private", "local", "games", "bin"):
            (temporary / name).mkdir(mode=0o700)
        if xdg_root is not None:
            xdg_root = Path(xdg_root)
            if not xdg_root.is_absolute() or not xdg_root.is_dir() or xdg_root.is_symlink():
                raise SetupError("Der private Downloadordner ist noch nicht vorbereitet.")
            (temporary / "private/xdg").symlink_to(xdg_root, target_is_directory=True)
        shutil.copytree(plan.tools, temporary / "tools")
        for script in (temporary / "tools").iterdir():
            if script.is_file():
                script.chmod(0o700)
        (temporary / "games/MSFS2024").symlink_to(plan.sources["game"], target_is_directory=True)
        (temporary / "runner").symlink_to(plan.sources["runner"], target_is_directory=True)
        event("copy_prefix", "Wine-Umgebung wird unabhängig kopiert. Das kann einige Minuten dauern …")
        _copy_prefix(plan.sources["prefix"], temporary / "local/msfs-prefix", cancel)
        system32 = prefix_system32(temporary / "local/msfs-prefix")
        system32.chmod(system32.stat().st_mode | stat.S_IWUSR)
        event("install_artifacts", "Geprüfte Kompatibilitätsdateien werden eingesetzt …", 75)
        for relative in artifact_names(plan.manifest):
            if relative.startswith("builtin/"):
                target = temporary / "local/store-runtime" / relative.removeprefix("builtin/")
            elif relative.startswith("bin/"):
                target = temporary / relative
            else:
                target = system32 / "xgameruntime.dll"
            _copy_file(plan.sources["artifacts"] / relative, target)
            if digest(target, cancel) != plan.manifest["files"][relative]:
                raise SetupError("Ein Build-Artefakt wurde während der Kopie verändert. Die Einrichtung wurde verworfen.")
        _copy_file(plan.original, system32 / "xgameruntime_original.dll")
        if digest(system32 / "xgameruntime_original.dll", cancel) != plan.original_hash:
            raise SetupError("Der Proton-Runner wurde während der Einrichtung verändert.")
        _copy_file(temporary / "local/store-runtime/x86_64-windows/xodus_store_test.dll", system32 / "xodus_store_test.dll")
        if plan.plugins:
            (temporary / "local/media-plugins").symlink_to(plan.plugins, target_is_directory=True)
        config = {"format": 1, "market": plan.inputs["market"], "local_saves": plan.inputs["local_saves"]}
        for filename, content in (("runtime.json", config), ("import-manifest.json", {
            "format": 1, "artifacts": plan.manifest, "original_runtime_sha256": plan.original_hash,
            "local_saves": plan.inputs["local_saves"], "inputs": {name: str(path) for name, path in plan.sources.items()},
        })):
            file = temporary / "private" / filename
            file.write_text(json.dumps(content, indent=2) + "\n")
            file.chmod(0o600)
        if plan.inputs["local_saves"]:
            (temporary / "private/local-saves").mkdir(mode=0o700)
            gate = temporary / "private/local-saves.enabled"
            gate.write_text("local-only\n")
            gate.chmod(0o600)
        event("verify", "Vollständige Runtime wird übernommen …", 95)
        _publish(temporary, destination)
        return destination
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


class SetupManager:
    def __init__(self, launcher, source_root=None):
        self.launcher = launcher
        self.source_root = source_root
        self.lock = threading.RLock()
        self.job = None
        self.plan = None
        self.cancel_event = threading.Event()
        self.thread = None
        self.picker_lock = threading.Lock()
        self.download_control = None

    @staticmethod
    def _picker():
        if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
            return None
        for name in ("zenity", "kdialog"):
            command = shutil.which(name)
            if command:
                return name, command
        return None

    def snapshot(self):
        tools, lock = resource_paths(self.source_root)
        available = all((tools / name).is_file() for name in RUNTIME_FILES) and lock.is_file()
        root = self.launcher.runtime
        artifacts = Path(__file__).resolve().parents[1] / "build/compat/artifacts"
        bootstrap = bootstrap_module()
        install = bootstrap.availability(source_root=self.source_root) if bootstrap else {
            "available": False, "reason": "Die automatische Installation ist in diesem Quellstand noch nicht verfügbar."}
        defaults = {"mode": "existing" if root else "install", "runtime_path": str(root) if root else "",
                    "artifacts_path": str(artifacts) if (artifacts / "manifest.json").is_file() else "",
                    "game_path": str((root / "games/MSFS2024").resolve()) if root else "",
                    "runner_path": str((root / "runner").resolve()) if root and (root / "runner").is_dir() else "",
                    "prefix_path": str(root / "local/msfs-prefix") if root else "",
                    "destination_path": str(data_home() / "flightdeck/runtimes/msfs2024"),
                    "market": suggested_market(), "local_saves": True, "media_plugins_path": ""}
        with self.lock:
            return {"available": True, "prepare_available": available, "directory_picker": self._picker() is not None,
                    "install_available": bool(install["available"]), "install_unavailable_reason": install.get("reason", ""),
                    "prepare_unavailable_reason": "" if available else "Die Installationsdateien fehlen. Bitte Flightdeck vollständig installieren oder eine vorbereitete Runtime auswählen.",
                    "state": self.job["state"] if self.job else "idle", "job": copy.deepcopy(self.job), "defaults": defaults}

    def discover(self):
        """Bounded discovery of prepared runtimes, never a recursive game scan."""
        from .backend import LauncherError
        with self.launcher.lock:
            configured = self.launcher.runtime
        candidates = [configured] if configured else []
        source = Path(self.source_root) if self.source_root is not None else Path(__file__).resolve().parents[1]
        if (source / "compat/upstreams.lock.json").is_file() and (source / "scripts/runtime").is_dir():
            sibling = source.parent / "msfs-linux"
            candidates.extend((sibling, sibling / "runtime"))
        limited = False
        try:
            # islice bounds even a directory containing thousands of entries.
            # Inspect no grandchildren and execute no candidate programs.
            with os.scandir(data_home() / "flightdeck/runtimes") as entries:
                children = list(itertools.islice(entries, 33))
            limited = len(children) > 32
            for child in sorted(children[:32], key=lambda item: item.name):
                if child.is_dir():
                    candidates.append(Path(child.path))
        except OSError:
            pass
        seen, found = set(), []
        checked = 0
        for candidate in candidates:
            try:
                path = candidate.resolve()
                if path in seen:
                    continue
                seen.add(path)
                checked += 1
                path = self.launcher.validate_runtime(str(path))
                checks = existing_checks(path)
            except (OSError, RuntimeError, LauncherError):
                continue
            found.append({"name": "Microsoft Flight Simulator 2024", "path": str(path),
                          "ready": all(row["ok"] for row in checks), "configured": path == configured,
                          "checks": checks})
        found.sort(key=lambda item: (not item["configured"], not item["ready"], item["path"]))
        return {"ok": True, "runtimes": found, "checked_count": checked, "limited": limited}

    def pick(self, field, initial=None, *, language="de"):
        """Open a native directory dialog only after the explicit POST action."""
        from .backend import LauncherError
        allowed = {"runtime_path", "artifacts_path", "game_path", "runner_path", "prefix_path", "destination_path", "media_plugins_path"}
        if not isinstance(field, str) or field not in allowed:
            raise LauncherError("Dieses Verzeichnisfeld wird nicht unterstützt.")
        picker = self._picker()
        if not picker:
            raise LauncherError("Kein Verzeichnisdialog verfügbar. Bitte den Pfad direkt eingeben.")
        with self.lock:
            if self.job and self.job["state"] in ACTIVE:
                raise LauncherError("Bitte die laufende Einrichtung zuerst abschließen oder abbrechen.")
        with self.launcher.lock:
            self.launcher.require_open()
            if not self.picker_lock.acquire(blocking=False):
                raise LauncherError("Ein Verzeichnisdialog ist bereits geöffnet.")
        try:
            initial = initial or self.snapshot()["defaults"][field] or str(Path.home())
            if not isinstance(initial, str) or len(initial) > 4096 or any(ord(c) < 32 for c in initial):
                raise LauncherError("Der Ausgangspfad ist ungültig.")
            folder = Path(initial).expanduser()
            if not folder.is_absolute():
                raise LauncherError("Der Ausgangspfad muss absolut sein.")
            while not folder.is_dir() and folder != folder.parent:
                folder = folder.parent
            name, command = picker
            title = translate_message("Flightdeck – Ordner auswählen", language)
            arguments = ([command, "--file-selection", "--directory", "--title=" + title, "--filename=" + str(folder) + "/"]
                         if name == "zenity" else [command, "--getexistingdirectory", str(folder), "--title", title])
            try:
                result = subprocess.run(arguments, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                        text=True, timeout=120, check=False)
            except subprocess.TimeoutExpired:
                return {"ok": True, "cancelled": True, "field": field}
            if result.returncode:
                return {"ok": True, "cancelled": True, "field": field}
            value = result.stdout.rstrip("\r\n")
            if not value or len(value) > 4096 or not Path(value).is_absolute() or not Path(value).is_dir():
                return {"ok": True, "cancelled": True, "field": field}
            return {"ok": True, "cancelled": False, "field": field, "path": value}
        finally:
            self.picker_lock.release()

    def _update(self, **values):
        with self.lock:
            if values.get("state") in {"complete", "failed", "cancelled"}:
                values["transfer"] = None
            self.job.update(values)

    def _notify(self, phase, message, progress=None, check=None):
        with self.lock:
            self.job.update(phase=phase, message=message, progress=progress)
            if phase not in {"bootstrap", "download", "pausing", "paused"}:
                self.job["transfer"] = None
            if check:
                self.job["checks"] = [row for row in self.job["checks"] if row["id"] != check["id"]] + [check]

    def _transfer_callback(self):
        """Bind optional counters to this job without changing lifecycle state."""
        from .download_progress import normalize_transfer
        with self.lock:
            job_id, cancel = self.job["id"], self.cancel_event

        def update(value):
            value = normalize_transfer(value) if value is not None else None
            with self.lock:
                if (cancel.is_set() or cancel is not self.cancel_event or not self.job
                        or self.job["id"] != job_id or self.job["state"] != "installing"):
                    return
                if value is not None:
                    allowed = {"bootstrap"} if value["kind"] == "components" else {"download", "pausing"}
                    if self.job["phase"] not in allowed:
                        return
                self.job["transfer"] = value
        return update

    def check(self, data):
        from .backend import LauncherError
        with self.lock:
            if self.job and self.job["state"] in ACTIVE and not (self.job.get("mode") == "update" and self.job["state"] == "ready"):
                raise LauncherError("Eine Einrichtung läuft bereits. Bitte abschließen oder abbrechen.")
            mode = data.get("mode") if isinstance(data, dict) else None
            if not isinstance(mode, str) or mode not in {"install", "prepare", "existing", "update"}:
                raise LauncherError("Bitte vorhandene Runtime oder neue Einrichtung wählen.")
            operation = data.get("operation", "update") if mode == "update" else None
            if mode == "update" and operation not in {"update", "repair", "verify"}:
                raise LauncherError("Ungültige Updateanfrage.")
            self.launcher.reserve_setup()
            self.plan = None
            self.cancel_event = threading.Event()
            from .game_install import DownloadControl
            self.download_control = DownloadControl(self._download_changed)
            self.job = {"id": uuid.uuid4().hex, "state": "checking", "mode": mode, "phase": "paths",
                        "message": "Ausgewählte Dateien werden geprüft …", "progress": None, "checks": [],
                        "can_pause": False, "can_resume": False, "transfer": None}
            if mode in {"install", "prepare"}:
                # Reloaded clients display this job's explicit region instead
                # of the unrelated suggestion for a future installation.
                market = data.get("market")
                self.job["market"] = market if isinstance(market, str) and re.fullmatch(r"[A-Z]{2}", market) else ""
            if mode == "update":
                self.job["runtime_path"] = str(self.launcher.runtime)
                self.job["operation"] = operation
            self.thread = threading.Thread(target=self._check, args=(copy.deepcopy(data),), daemon=True)
            self.thread.start()
            return {"ok": True, "job": copy.deepcopy(self.job)}

    def _check_existing(self, data):
        path = self.launcher.validate_runtime(data.get("runtime_path"))
        for check in existing_checks(path):
            interrupted(self.cancel_event)
            self._notify(check["id"], check["label"] if check["ok"] else check["detail"], check=check)
            if not check["ok"]:
                raise SetupError(check["detail"])
        return path

    def _check(self, data):
        try:
            if data["mode"] == "update":
                from . import game_update
                if data.get("operation") == "verify":
                    from . import game_integrity
                    result = game_integrity.verify(self.launcher, notify=self._notify, cancel=self.cancel_event)
                    with self.lock:
                        interrupted(self.cancel_event)
                        self.job.update(state="complete", phase="complete", integrity_result=result, progress=100,
                                        message="Dateiprüfung abgeschlossen. Alle geprüften Dateien stimmen überein." if result["healthy"] else "Dateiprüfung abgeschlossen. Fehlende, veränderte oder unlesbare Dateien wurden gefunden.")
                        self.launcher.release_setup()
                    return
                plan = game_update.check(self.launcher, data, notify=self._notify, cancel=self.cancel_event, source_root=self.source_root)
                newer = game_update.version(plan.latest["version"]) > game_update.version(plan.current["version"])
                ready = newer or data.get("operation") == "repair"
                with self.lock:
                    interrupted(self.cancel_event)
                    self.plan = plan
                    self.launcher.release_setup()
                    self.job.update(state="ready" if ready else "complete", phase="ready" if ready else "complete",
                                    installed_version=plan.current["version"], latest_version=plan.latest["version"],
                                    update_available=newer, auth_required=False, progress=100,
                                    message="Die vollständige Reparatur ist vorbereitet. Das Store-Basispaket wird nach Bestätigung neu heruntergeladen." if data.get("operation") == "repair" else "Eine neue Spielversion ist verfügbar. Das Update kann gestartet werden." if newer else "Die installierte Spielversion ist aktuell.")
                return
            if data["mode"] == "install":
                bootstrap = bootstrap_module()
                if bootstrap is None:
                    raise SetupError("Die automatische Installation ist in diesem Quellstand noch nicht verfügbar.")
                data = {**data, "destination_path": data.get("destination_path") or str(data_home() / "flightdeck/runtimes/msfs2024")}
                plan = bootstrap.preflight(data, source_root=self.source_root, notify=self._notify, cancel=self.cancel_event)
            else:
                plan = self._check_existing(data) if data["mode"] == "existing" else preflight(data, source_root=self.source_root, notify=self._notify, cancel=self.cancel_event)
            with self.lock:
                interrupted(self.cancel_event)
                self.plan = plan
                self.job.update(state="ready", phase="ready", message="Prüfung abgeschlossen. Die Einrichtung kann gestartet werden.", progress=100)
                if isinstance(plan, Plan) or data["mode"] == "install":
                    self.job["market"] = plan.inputs["market"]
                    if isinstance(plan, Plan):
                        self.job["prefix_bytes"] = plan.prefix_bytes
                    self.job["runtime_path"] = str(plan.destination)
                else:
                    self.job["runtime_path"] = str(plan)
        except Exception as error:
            self._failed(error)

    def _failed(self, error):
        from .backend import LauncherError
        from .game_install import GameInstallError, GameInstallCancelled
        bootstrap = bootstrap_module()
        known_errors = (SetupError, LauncherError, GameInstallError)
        if bootstrap is not None and isinstance(getattr(bootstrap, "BootstrapError", None), type):
            known_errors += (bootstrap.BootstrapError,)
        cancelled = isinstance(error, (SetupCancelled, GameInstallCancelled)) or self.cancel_event.is_set()
        message = "Einrichtung abgebrochen." if cancelled else error_message(error) if isinstance(error, known_errors) else "Die Einrichtung ist fehlgeschlagen. Bitte Dateien, Zugriffsrechte und freien Speicher prüfen."
        with self.lock:
            from .game_update import AuthRequired
            if self.job.get("mode") == "update":
                self.job["auth_required"] = isinstance(error, AuthRequired)
            self._update(state="cancelled" if cancelled else "failed", failure_phase=self.job["phase"],
                         phase="cancelled" if cancelled else "failed", message=message,
                         error=None if cancelled else message, progress=None, can_pause=False, can_resume=False)
        self.launcher.release_setup()

    def start(self, check_id):
        from .backend import LauncherError
        with self.lock:
            if not self.job or self.job["id"] != check_id or self.job["state"] != "ready":
                raise LauncherError("Bitte die ausgewählten Dateien zuerst erneut prüfen.")
            if self.job.get("mode") == "update":
                with self.launcher.lock:
                    if self.launcher.runtime != self.plan.runtime:
                        raise LauncherError("Das installierte Spiel hat sich geändert. Bitte Updates erneut prüfen.")
                    self.launcher.reserve_setup()
            self.job.update(state="installing", phase="recheck", message="Eingaben werden vor der Übernahme erneut geprüft …", progress=None)
            self.thread = threading.Thread(target=self._install, daemon=True)
            self.thread.start()
            return {"ok": True, "job": copy.deepcopy(self.job)}

    def _install(self):
        try:
            transfer = self._transfer_callback()
            if self.job["mode"] == "update":
                from . import game_update
                def committing():
                    with self.lock:
                        interrupted(self.cancel_event)
                        self.job.update(phase="switch_update", message="Die geprüfte Spielversion wird atomar aktiviert …", can_pause=False, can_resume=False)
                path = game_update.install(self.launcher, self.plan, notify=self._notify, cancel=self.cancel_event, control=self.download_control, committing=committing, transfer=transfer)
                with self.lock:
                    self.job.update(state="complete", phase="complete", message="Reparatur abgeschlossen. Das neu heruntergeladene Basispaket ist aktiv; der vorherige Stand bleibt erhalten." if self.job.get("operation") == "repair" else "Spielupdate abgeschlossen. Die vorherige Version bleibt für eine Rückkehr erhalten.", progress=100, can_pause=False, can_resume=False, update_available=False, transfer=None)
                self.launcher.release_setup()
                return
            if self.job["mode"] == "install":
                from .game_install import download_game
                bootstrap = bootstrap_module()
                plan = bootstrap.preflight(self.plan.inputs, source_root=self.source_root, notify=self._notify, cancel=self.cancel_event)
                self._notify("bootstrap", "Die Laufzeitkomponenten werden automatisch vorbereitet …")
                components = bootstrap.bootstrap(plan, notify=self._notify, cancel=self.cancel_event, transfer=transfer)
                workspace = Path(components["workspace"])
                self._update(workspace_path=str(workspace))
                xdg = workspace / "xdg"
                game = download_game(components["cli"], components["cli_sha256"], workspace / "game", plan.inputs["market"],
                                     cancel=self.cancel_event, notify=self._notify, xdg_root=xdg,
                                     control=self.download_control, cli_features=components.get("cli_features", []), transfer=transfer)
                self._notify("provision", "Das heruntergeladene Spiel wird für den Start eingerichtet …")
                inputs = {key: str(components[key]) for key in ("artifacts_path", "runner_path", "prefix_path")}
                inputs.update(game_path=str(game), destination_path=str(plan.destination), market=plan.inputs["market"],
                              local_saves=plan.inputs.get("local_saves", False), mode="prepare")
                prepared = preflight(inputs, source_root=self.source_root, notify=self._notify, cancel=self.cancel_event)
                path = prepare(prepared, notify=self._notify, cancel=self.cancel_event, xdg_root=xdg)
            elif isinstance(self.plan, Plan):
                # A ready job can remain open; recheck every manifest hash before
                # creating anything, then verify the copied bytes once more.
                plan = preflight(self.plan.inputs, source_root=self.source_root, notify=self._notify, cancel=self.cancel_event)
                path = prepare(plan, notify=self._notify, cancel=self.cancel_event)
            else:
                path = self._check_existing({"runtime_path": str(self.plan)})
                interrupted(self.cancel_event)
            self.launcher.finish_setup(str(path))
            self._update(state="complete", phase="complete", message="Runtime eingerichtet und im Launcher ausgewählt.", progress=100, runtime_path=str(path))
        except Exception as error:
            self._failed(error)

    def cancel(self, job_id):
        from .backend import LauncherError
        with self.lock:
            if not self.job or self.job["id"] != job_id or self.job["state"] not in ACTIVE:
                raise LauncherError("Es gibt keine passende laufende Einrichtung.")
            if self.job.get("mode") == "update" and self.job["phase"] == "switch_update":
                raise LauncherError("Der atomare Spielwechsel wird gerade abgeschlossen. Bitte kurz warten.")
            self.cancel_event.set()
            if self.job["state"] == "ready":
                self._failed(SetupCancelled("Einrichtung abgebrochen."))
            else:
                self.job.update(message="Einrichtung wird abgebrochen. Bereits heruntergeladene Installationsdateien bleiben im privaten Arbeitsordner erhalten.",
                                can_pause=False, can_resume=False)
            return {"ok": True, "job": copy.deepcopy(self.job)}

    def _download_changed(self, state):
        with self.lock:
            if self.cancel_event.is_set() or not self.job or self.job["state"] != "installing":
                return
            self.job.update(can_pause=state == "download", can_resume=state == "paused")
            if state in {"pausing", "paused"}:
                self.job.update(phase=state, progress=None, message=(
                    "Download wird pausiert. Der laufende Teil wird sicher beendet …" if state == "pausing" else
                    "Download pausiert. Flightdeck geöffnet lassen. Vollständige Dateien bleiben erhalten; die unvollständige Datei beginnt beim Fortsetzen erneut."))
            elif state == "resuming":
                self.job.update(phase="download", message="Der Download wird fortgesetzt …")

    def download_action(self, job_id, operation):
        from .backend import LauncherError
        from .game_install import GameInstallError
        with self.lock:
            if (not self.job or self.job["id"] != job_id or self.job["state"] != "installing"
                    or self.cancel_event.is_set() or operation not in {"pause", "resume"}):
                raise LauncherError("Es gibt keine passende laufende Einrichtung.")
            control = self.download_control
        # Worker callbacks acquire self.lock: never hold it while entering the
        # control lock. HTTP pause/cancel stay concurrent with child reaping.
        try:
            getattr(control, operation)()
        except GameInstallError as error:
            raise LauncherError(error_message(error)) from None
        with self.lock:
            return {"ok": True, "job": copy.deepcopy(self.job)}

    def close(self):
        """Stop only setup work when the local HTTP service shuts down."""
        with self.lock:
            self.cancel_event.set()
            thread = self.thread
            if self.job and self.job["state"] == "ready":
                self._failed(SetupCancelled("Einrichtung abgebrochen."))
        if thread and thread is not threading.current_thread():
            thread.join(timeout=5)
