#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Install the source-only Flightdeck launcher for one Linux user, without pip."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile

APP = "flightdeck-source-launcher"
FORMAT = 1
HASH = re.compile(r"[0-9a-f]{64}\Z")
MAX_FILE = 2 * 1024 * 1024
MAX_SOURCE = 16 * 1024 * 1024
NATIVE_FILE_MAX = 128 * 1024 * 1024
NATIVE_TOTAL_MAX = 256 * 1024 * 1024
NATIVE_NOTICE_MAX = 16 * 1024 * 1024
NATIVE_FEATURES = ("connected-storage-read-v1", "connected-storage-sync-v1")
NATIVE_FILES = ("bin/xodus-cli", "bin/xodus-service", "bin/flightdeck-connected-storage.exe", "runtime/xgameruntime.dll",
                "builtin/x86_64-windows/xodus_store_test.dll", "builtin/x86_64-unix/xodus_store_test.so")
PUBLIC_IMAGES = {"ui/flight-panorama.png": "f64fc375e0aaa806c4de91cd91ec8f18994a06ff64aa80d8b6d95f7c463c9304",
                 "ui/flight-panorama-2020.png": "cd81013284d468fe6d306438e66d9a0d7ee35d6bda02ec2b44dea010f8cf8344"}
PUBLIC_FONT = {"ui/manrope-variable.woff2": "30b83738add8c9edd9e3450b98036a9a8fb5668d0cbd4eb0ce5fe6761197f21f",
               "ui/OFL-Manrope.txt": "58172e0c0fac2cda8a37b348164bb55e44b0e69051e557e92b1d3f6910141f7b"}
SOURCE_ROOT = Path(__file__).resolve().parents[1]


def detect_language(environ=None) -> str:
    """Read locale preferences without modifying the process environment."""
    values = os.environ if environ is None else environ
    locale = next((values.get(key) for key in ("LC_ALL", "LC_MESSAGES", "LANG") if values.get(key)), "")
    return "de" if re.match(r"de(?:[_@.\-]|$)", locale, re.IGNORECASE) else "en"


LANGUAGE = detect_language()
# German source messages and English translations share named placeholders.
ENGLISH = {
    'Ungültige Schriftdatei oder Lizenz: {name}': 'Invalid font file or license: {name}',
    'Installationspfade dürfen keine Zeilenumbrüche enthalten.': 'Installation paths must not contain line breaks.',
    'Symbolischer Link im Installationspfad: {part}': 'Symbolic link in installation path: {part}',
    'Kein Verzeichnis: {path}': 'Not a directory: {path}',
    'Datei kann nicht sicher gelesen werden: {path}': 'Cannot safely read file: {path}',
    'Keine reguläre oder zu große Quelldatei: {path}': 'Source file is not regular or is too large: {path}',
    'Datei wurde während des Lesens geändert: {path}': 'File changed while being read: {path}',
    'Unvollständiges Quellpaket: {folder} fehlt.': 'Incomplete source package: {folder} is missing.',
    'Ungültiges PNG-Bild: {name}': 'Invalid PNG image: {name}',
    'Binärdaten statt Launcher-Quelltext: {name}': 'Binary data instead of launcher source: {name}',
    'Ungültiger oder inkompatibler Quelltext: {name}': 'Invalid or incompatible source: {name}',
    'Das Launcher-Quellpaket überschreitet 16 MiB.': 'The launcher source package exceeds 16 MiB.',
    'Ungültige Installationsmetadaten: {path}': 'Invalid installation metadata: {path}',
    'Ungültige Release-ID.': 'Invalid release ID.',
    'Release-Metadaten stimmen nicht mit der Release-ID überein.': 'Release metadata does not match the release ID.',
    'Ungültiger Pfad in den Release-Metadaten.': 'Invalid path in release metadata.',
    'Fremde Datei in der installierten Version: {path}': 'Unrecognized file in the installed version: {path}',
    'Installierte Quelldatei wurde geändert: {name}': 'Installed source file was modified: {name}',
    'Die Installationsmetadaten sind ein symbolischer Link.': 'Installation metadata is a symbolic link.',
    'Dieses Verzeichnis gehört keiner gültigen Flightdeck-Installation.': 'This directory is not a valid Flightdeck installation.',
    'Ungültiger Launcher-Eintrag in den Installationsmetadaten.': 'Invalid launcher entry in installation metadata.',
    'Der Launcher-Eintrag fehlt in den Installationsmetadaten.': 'The launcher entry is missing from installation metadata.',
    'Ungültige Installationssperre.': 'Invalid installation lock.',
    'Eine andere Installation oder Deinstallation läuft bereits.': 'Another installation or uninstallation is already running.',
    'Vorhandene fremde oder geänderte Datei bleibt erhalten: {path}': 'Existing unrelated or modified file is preserved: {path}',
    'Der installierte Eintrag fehlt: {path}. Bitte zuerst die Installation prüfen.': 'Installed entry is missing: {path}. Please check the installation first.',
    'Das Zielverzeichnis enthält fremde Dateien. Bitte ein leeres Installationsziel wählen.': 'The destination contains unrelated files. Choose an empty installation directory.',
    'Keine vorherige Launcher-Version für ein Rollback vorhanden.': 'No previous launcher version is available for rollback.',
    'Hier ist kein Flightdeck-Launcher installiert.': 'No Flightdeck launcher is installed here.',
    'Benötigt Linux und Python 3.10 oder neuer.': 'Linux and Python 3.10 or newer are required.',
    'Bitte als normaler Benutzer ohne sudo starten.': 'Run as your normal user, without sudo.',
    'Bitte --language de oder --language en wählen.': 'Choose --language de or --language en.',
    'Flightdeck für das eigene Linux-Benutzerkonto installieren.': 'Install Flightdeck for your Linux user account.',
    'Pfad zum entpackten Quellpaket/Clone': 'Path to the extracted source package or clone',
    'Verzeichnis für verwaltete Launcher-Versionen': 'Directory for managed launcher versions',
    'Verzeichnis für den Launcher-Befehl': 'Directory for the launcher command',
    'Verzeichnis für den Menüeintrag': 'Directory for the application menu entry',
    'Sprache: de oder en (Standard: Systemsprache)': 'Language: de or en (default: system locale)',
    'Diese Hilfe anzeigen und beenden': 'Show this help message and exit',
    'Menüeintrag anlegen (Standard)': 'Create an application menu entry (default)',
    'Keinen neuen Menüeintrag anlegen': 'Do not create a new application menu entry',
    'Oberfläche nach Installation nicht öffnen': 'Do not open the interface after installation',
    'Launcher entfernen; Einstellungen und Spielstände behalten': 'Remove the launcher; preserve settings and saves',
    'Zur vorherigen Launcher-Version wechseln': 'Switch to the previous launcher version',
    'Aufruf: ': 'Usage: ',
    'Optionen': 'Options',
    'Argumente': 'Arguments',
    'Ungültige Argumente. Hilfe: {command} --help': 'Invalid arguments. For help: {command} --help',
    'Launcher entfernt. Einstellungen, Runtimes und Spielstände bleiben erhalten.': 'Launcher removed. Settings, runtimes and saves are preserved.',
    'Geänderte oder fremde Dateien wurden behalten:\n{paths}': 'Modified or unrelated files were preserved:\n{paths}',
    'Flightdeck installiert: {launcher}\nQuellstand: {release}': 'Flightdeck installed: {launcher}\nSource release: {release}',
    'Update: Installer aus einem neuen Quellpaket erneut ausführen.\nRollback: flightdeck --rollback · Entfernen: flightdeck --uninstall': 'Update: run the installer from a newer source package.\nRollback: flightdeck --rollback · Remove: flightdeck --uninstall',
    'Installation nicht abgeschlossen: {error}': 'Installation was not completed: {error}',
    'Dateisystemfehler {code}: {path}': 'Filesystem error {code}: {path}',
    'Unbekannter Pfad': 'Unknown path',
    'MSFS Xbox PC unter Linux starten': 'Launch MSFS Xbox PC on Linux',
    'Grafischen Installationsdialog öffnen': 'Open the graphical installer',
    'Die Beschreibung des Kompatibilitätspakets ist ungültig.': 'The compatibility package specification is invalid.',
    'Das mitgelieferte Kompatibilitätspaket ist unvollständig oder enthält fremde Dateien.': 'The bundled compatibility package is incomplete or contains unrelated files.',
    'Prüfsumme des Kompatibilitätspakets stimmt nicht: {name}': 'Compatibility package checksum does not match: {name}',
    'Das Kompatibilitätspaket überschreitet die erlaubte Größe.': 'The compatibility package exceeds its size limit.',
}


def tr(message: str, *, language=None, **values) -> str:
    selected = LANGUAGE if language is None else language
    return (message if selected == "de" else ENGLISH[message]).format_map(values)


def language_option(arguments) -> str:
    selected = detect_language()
    for index, argument in enumerate(arguments):
        if argument == "--language":
            value = arguments[index + 1] if index + 1 < len(arguments) else None
        elif argument.startswith("--language="):
            value = argument.partition("=")[2]
        else:
            continue
        if value not in {"de", "en"}:
            raise InstallError(tr("Bitte --language de oder --language en wählen.", language=selected), language=selected)
        selected = value
    return selected


class LocalizedParser(argparse.ArgumentParser):
    def format_help(self):
        return super().format_help().replace("usage: ", tr("Aufruf: "), 1)

    def format_usage(self):
        return super().format_usage().replace("usage: ", tr("Aufruf: "), 1)

    def error(self, message):
        self.print_usage(sys.stderr)
        self.exit(2, tr("Ungültige Argumente. Hilfe: {command} --help", command=self.prog) + "\n")


class InstallError(Exception):
    """An actionable installation failure; existing user data is retained."""

    def __init__(self, message, *, language=None):
        super().__init__(message)
        self.language = language


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def absolute(path: Path) -> Path:
    path = Path(os.path.abspath(path.expanduser()))
    if any(c in str(path) for c in "\r\n\0"):
        raise InstallError(tr('Installationspfade dürfen keine Zeilenumbrüche enthalten.'))
    return path


def no_links(path: Path) -> None:
    for part in [*reversed(path.parents), path]:
        if part.is_symlink():
            raise InstallError(tr('Symbolischer Link im Installationspfad: {part}', part=part))


def directory(path: Path) -> None:
    no_links(path)
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    if not path.is_dir():
        raise InstallError(tr('Kein Verzeichnis: {path}', path=path))


def read_regular(path: Path, limit: int | None = None) -> bytes:
    if limit is None:
        if path.name in {Path(name).name for name in NATIVE_FILES}:
            limit = NATIVE_FILE_MAX
        elif path.name == "THIRD-PARTY-NOTICES.txt":
            limit = NATIVE_NOTICE_MAX
        else:
            limit = 4 * 1024 * 1024 if path.name in {Path(name).name for name in PUBLIC_IMAGES} else MAX_FILE
    no_links(path)
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError as error:
        raise InstallError(tr('Datei kann nicht sicher gelesen werden: {path}', path=path)) from error
    with os.fdopen(fd, "rb") as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_size > limit:
            raise InstallError(tr('Keine reguläre oder zu große Quelldatei: {path}', path=path))
        data = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
    if len(data) > limit or (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
        raise InstallError(tr('Datei wurde während des Lesens geändert: {path}', path=path))
    return data


def json_bytes(value: dict) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode()


def atomic_write(path: Path, data: bytes, mode: int = 0o600) -> None:
    directory(path.parent)
    no_links(path)
    fd, temporary = tempfile.mkstemp(prefix=".flightdeck-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.lexists(temporary):
            os.unlink(temporary)


def source_snapshot(source: Path) -> dict[str, bytes]:
    source = absolute(source)
    no_links(source)
    required = {"flightdeck/__init__.py", "flightdeck/__main__.py", "flightdeck/backend.py",
                "flightdeck/server.py", "ui/index.html", "ui/app.js", "ui/styles.css",
                "ui/mark.svg", "LICENSE", "scripts/install-launcher.py", "scripts/install-launcher-gui.py"}
    paths = set(required)
    # No tests, build outputs, native runtimes or account files are install inputs.
    for folder, suffixes in (("flightdeck", {".py"}), ("ui", {".html", ".css", ".js", ".svg", ".png", ".py"})):
        root = source / folder
        no_links(root)
        if not root.is_dir():
            raise InstallError(tr('Unvollständiges Quellpaket: {folder} fehlt.', folder=folder))
        for path in root.iterdir():
            if path.suffix in suffixes:
                paths.add(str(path.relative_to(source)))
    # Only this reviewed font and its exact accompanying license are admitted.
    # Neither arbitrary fonts nor arbitrary binary/text assets are imported.
    if any((source / name).exists() or (source / name).is_symlink() for name in PUBLIC_FONT):
        paths.update(PUBLIC_FONT)
    inputs = {name: source / name for name in paths}
    for name in ("__init__.py", "core.py"):
        inputs["flightdeck/_fenix/" + name] = source / "flightdeck/_fenix" / name
    for name in ("launch-msfs.sh", "play-msfs.sh", "runtime-env.sh", "xodus.sh", "xodus-service.sh", "xodus-wine-launch"):
        inputs["flightdeck/resources/runtime/" + name] = source / "scripts/runtime" / name
    inputs["flightdeck/resources/upstreams.lock.json"] = source / "compat/upstreams.lock.json"
    inputs["flightdeck/resources/bootstrap.lock.json"] = source / "compat/bootstrap.lock.json"
    for name in ("bundle.json", "release.json", "LICENSE"):
        inputs["flightdeck/resources/fenix/" + name] = source / "compat/fenix" / name
    result = {}
    for name in sorted(inputs):
        data = read_regular(inputs[name])
        if name in PUBLIC_FONT:
            if digest(data) != PUBLIC_FONT[name] or (name.endswith(".woff2") and not data.startswith(b"wOF2")):
                raise InstallError(tr('Ungültige Schriftdatei oder Lizenz: {name}', name=name))
            result[name] = data
            continue
        if name.endswith(".png"):
            if PUBLIC_IMAGES.get(name) != digest(data) or not data.startswith(b"\x89PNG\r\n\x1a\n"):
                raise InstallError(tr('Ungültiges PNG-Bild: {name}', name=name))
            result[name] = data
            continue
        if b"\0" in data:
            raise InstallError(tr('Binärdaten statt Launcher-Quelltext: {name}', name=name))
        try:
            text = data.decode("utf-8")
            if name.endswith(".py"):
                compile(text, name, "exec")
        except (UnicodeError, SyntaxError) as error:
            raise InstallError(tr('Ungültiger oder inkompatibler Quelltext: {name}', name=name)) from error
        result[name] = data
    if sum(map(len, result.values())) > MAX_SOURCE:
        raise InstallError(tr('Das Launcher-Quellpaket überschreitet 16 MiB.'))
    result.update(native_snapshot(source, result["flightdeck/resources/bootstrap.lock.json"]))
    return result


def native_snapshot(source: Path, specification: bytes) -> dict[str, bytes]:
    """Admit only the optional, exactly pinned native bundle and its notices."""
    root = source / "flightdeck/resources/native"
    no_links(root)
    if not root.exists():
        return {}
    if not root.is_dir():
        raise InstallError(tr("Das mitgelieferte Kompatibilitätspaket ist unvollständig oder enthält fremde Dateien."))
    try:
        expected = json.loads(specification)["native"]
        hashes = expected["files"]
        notice_hash = expected["notice_sha256"]
        features = expected.get("features")
        if (not isinstance(features, list) or any(not isinstance(item, str) for item in features) or
                not set(NATIVE_FEATURES).issubset(features) or
                not isinstance(hashes, dict) or set(hashes) != set(NATIVE_FILES) or
                any(not isinstance(value, str) or not HASH.fullmatch(value) for value in hashes.values()) or
                not isinstance(notice_hash, str) or not HASH.fullmatch(notice_hash)):
            raise ValueError()
        manifest_bytes = read_regular(root / "manifest.json")
        manifest = json.loads(manifest_bytes)
        if set(manifest) != {"format", "files"} or manifest["format"] != 1 or manifest["files"] != hashes:
            raise ValueError()
    except (ValueError, UnicodeError, KeyError, TypeError):
        raise InstallError(tr("Die Beschreibung des Kompatibilitätspakets ist ungültig.")) from None
    allowed = set(NATIVE_FILES) | {"manifest.json", "THIRD-PARTY-NOTICES.txt"}
    found = set()
    for path in root.rglob("*"):
        no_links(path)
        if path.is_dir():
            continue
        relative = path.relative_to(root).as_posix()
        if relative not in allowed:
            raise InstallError(tr("Das mitgelieferte Kompatibilitätspaket ist unvollständig oder enthält fremde Dateien."))
        found.add(relative)
    if found != allowed:
        raise InstallError(tr("Das mitgelieferte Kompatibilitätspaket ist unvollständig oder enthält fremde Dateien."))
    values = {"manifest.json": manifest_bytes}
    for name, checksum in {**hashes, "THIRD-PARTY-NOTICES.txt": notice_hash}.items():
        data = read_regular(root / name, NATIVE_NOTICE_MAX if name == "THIRD-PARTY-NOTICES.txt" else NATIVE_FILE_MAX)
        if digest(data) != checksum:
            raise InstallError(tr("Prüfsumme des Kompatibilitätspakets stimmt nicht: {name}", name=name))
        values[name] = data
    if sum(map(len, values.values())) > NATIVE_TOTAL_MAX:
        raise InstallError(tr("Das Kompatibilitätspaket überschreitet die erlaubte Größe."))
    return {"flightdeck/resources/native/" + name: data for name, data in values.items()}


def release_identity(files: dict[str, bytes]) -> tuple[str, dict]:
    hashes = {name: digest(data) for name, data in sorted(files.items())}
    identity = digest(json_bytes(hashes))
    return identity, {"app": APP, "format": FORMAT, "release": identity, "files": hashes}


def load_json(path: Path) -> dict:
    try:
        value = json.loads(read_regular(path))
        if not isinstance(value, dict):
            raise ValueError()
        return value
    except (ValueError, UnicodeError) as error:
        raise InstallError(tr('Ungültige Installationsmetadaten: {path}', path=path)) from error


def release_manifest(root: Path, identity: str) -> tuple[Path, dict]:
    if not isinstance(identity, str) or not HASH.fullmatch(identity):
        raise InstallError(tr('Ungültige Release-ID.'))
    folder = root / "releases" / identity
    record = load_json(folder / "release.json")
    files = record.get("files")
    if (record.get("app") != APP or record.get("format") != FORMAT or
            record.get("release") != identity or not isinstance(files, dict) or not files or
            digest(json_bytes(files)) != identity):
        raise InstallError(tr('Release-Metadaten stimmen nicht mit der Release-ID überein.'))
    for name, checksum in files.items():
        path = Path(name)
        if (path.is_absolute() or ".." in path.parts or path.as_posix() != name or
                not isinstance(checksum, str) or not HASH.fullmatch(checksum)):
            raise InstallError(tr('Ungültiger Pfad in den Release-Metadaten.'))
    return folder, record


def remove_bytecode_caches(folder: Path, expected: set[str]) -> None:
    """Discard only generated caches for shipped Python modules before verifying."""
    for cache in folder.rglob("__pycache__"):
        no_links(cache)
        if not cache.is_dir():
            raise InstallError(tr('Fremde Datei in der installierten Version: {path}', path=cache))
        entries = list(cache.iterdir())
        for path in entries:
            no_links(path)
            match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)\.cpython-\d+(?:\.opt-\d+)?\.pyc", path.name)
            source = cache.parent / (match.group(1) + ".py") if match else None
            info = path.lstat()
            if (not match or source.relative_to(folder).as_posix() not in expected
                    or not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_nlink != 1):
                raise InstallError(tr('Fremde Datei in der installierten Version: {path}', path=path))
        for path in entries:
            path.unlink()
        cache.rmdir()


def verify_release(root: Path, identity: str) -> Path:
    folder, record = release_manifest(root, identity)
    expected = set(record["files"]) | {"release.json"}
    remove_bytecode_caches(folder, expected)
    for path in folder.rglob("*"):
        no_links(path)
        if not path.is_dir() and str(path.relative_to(folder)) not in expected:
            raise InstallError(tr('Fremde Datei in der installierten Version: {path}', path=path))
    for name, checksum in record["files"].items():
        if digest(read_regular(folder / name)) != checksum:
            raise InstallError(tr('Installierte Quelldatei wurde geändert: {name}', name=name))
    return folder


def load_installation(root: Path) -> dict | None:
    path = root / "installation.json"
    if not path.exists():
        if path.is_symlink():
            raise InstallError(tr('Die Installationsmetadaten sind ein symbolischer Link.'))
        return None
    value = load_json(path)
    if (value.get("app") != APP or value.get("format") != FORMAT or
            value.get("data_dir") != str(root) or not isinstance(value.get("entries"), dict) or
            not isinstance(value.get("language", "en"), str) or value.get("language", "en") not in {"de", "en"} or
            not isinstance(value.get("manager", value.get("current")), str) or
            not HASH.fullmatch(value.get("manager", value.get("current", ""))) or
            not isinstance(value.get("current"), str) or not HASH.fullmatch(value["current"]) or
            (value.get("previous") is not None and not HASH.fullmatch(str(value["previous"])))):
        raise InstallError(tr('Dieses Verzeichnis gehört keiner gültigen Flightdeck-Installation.'))
    for role, entry in value["entries"].items():
        if (role not in {"launcher", "desktop"} or not isinstance(entry, dict) or
                not isinstance(entry.get("path"), str) or not Path(entry["path"]).is_absolute() or
                Path(entry["path"]).name != ("flightdeck" if role == "launcher" else "flightdeck.desktop") or
                not isinstance(entry.get("sha256"), str) or not HASH.fullmatch(entry["sha256"])):
            raise InstallError(tr('Ungültiger Launcher-Eintrag in den Installationsmetadaten.'))
    if "launcher" not in value["entries"]:
        raise InstallError(tr('Der Launcher-Eintrag fehlt in den Installationsmetadaten.'))
    return value


@contextmanager
def installation_lock(root: Path):
    directory(root)
    lock = root / ".install.lock"
    no_links(lock)
    fd = os.open(lock, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise InstallError(tr('Ungültige Installationssperre.'))
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise InstallError(tr('Eine andere Installation oder Deinstallation läuft bereits.')) from error
        yield
    finally:
        os.close(fd)


def launcher_bytes(root: Path, language: str = "en") -> bytes:
    # The release is resolved once per launch. Language is carried as an argument,
    # never by modifying LC_*/LANG or another process-wide setting.
    return ('''#!/usr/bin/env python3
# Managed by Flightdeck source launcher; SPDX-License-Identifier: MIT
import json, os, pathlib, re, subprocess, sys
sys.dont_write_bytecode = True
root = pathlib.Path(ROOT_LITERAL)
language = LANGUAGE_LITERAL
messages = {
    "de": {"language": "Bitte --language de oder --language en wählen.",
           "invalid": "Ungültige Installation. Bitte den Quellinstaller erneut ausführen.",
           "update": "Aufruf: flightdeck --update QUELLVERZEICHNIS [--language de|en]",
           "extra": "Diese Aktion akzeptiert nur --language de|en als zusätzliche Option.",
           "start": "Flightdeck konnte nicht starten: {reason}",
           "os": "Dateisystemfehler {code}. Bitte Pfad und Zugriffsrechte prüfen."},
    "en": {"language": "Choose --language de or --language en.",
           "invalid": "Invalid installation. Run the source installer again.",
           "update": "Usage: flightdeck --update SOURCE_DIRECTORY [--language de|en]",
           "extra": "This action only accepts --language de|en as an additional option.",
           "start": "Flightdeck could not start: {reason}",
           "os": "Filesystem error {code}. Check the path and permissions."}}
def text(key, **values):
    return messages[language][key].format_map(values)
try:
    arguments = []
    explicit = False
    index = 1
    while index < len(sys.argv):
        value = sys.argv[index]
        if value == "--language":
            index += 1
            selected = sys.argv[index] if index < len(sys.argv) else None
        elif value.startswith("--language="):
            selected = value.partition("=")[2]
        else:
            arguments.append(value)
            index += 1
            continue
        if selected not in {"de", "en"}:
            raise ValueError(text("language"))
        language = selected
        explicit = True
        index += 1
    state = json.loads((root / "installation.json").read_text())
    identity = state["current"]
    saved = state.get("language", language)
    if (state.get("app") != "flightdeck-source-launcher" or
            not isinstance(identity, str) or not re.fullmatch(r"[0-9a-f]{64}", identity) or
            saved not in {"de", "en"}):
        raise ValueError(text("invalid"))
    if not explicit:
        language = saved
    release = root / "releases" / identity
    manager_id = state.get("manager", identity)
    if not isinstance(manager_id, str) or not re.fullmatch(r"[0-9a-f]{64}", manager_id):
        raise ValueError(text("invalid"))
    manager = root / "releases" / manager_id / "scripts" / "install-launcher.py"
    commands = {"--uninstall": "--uninstall", "--rollback": "--rollback", "--update": "--source"}
    if arguments and arguments[0] in commands:
        action = arguments[0]
        if action == "--update" and len(arguments) != 2:
            raise ValueError(text("update"))
        if action != "--update" and len(arguments) != 1:
            raise ValueError(text("extra"))
        managed = [commands[action]] + (arguments[1:] if action == "--update" else [])
        # An update must be installed by the selected *new* package. The old
        # manager cannot know future native features or package formats.
        # Rollback/uninstall still use the trusted installed manager.
        selected_manager = pathlib.Path(arguments[1]) / "scripts" / "install-launcher.py" if action == "--update" else manager
        selected_manager = pathlib.Path(os.path.abspath(selected_manager))
        if (not selected_manager.is_file() or
                any(part.is_symlink() for part in (selected_manager, *selected_manager.parents))):
            raise ValueError(text("update" if action == "--update" else "invalid"))
        invocation = [sys.executable, str(selected_manager), "--data-dir", str(root),
                      "--language", language, *managed]
        if action == "--uninstall":
            os.execv(sys.executable, invocation)
        # Keep management messages in the saved language without making that
        # preference override a later language selection inside the browser.
        completed = subprocess.run([*invocation, "--no-launch"], check=False)
        if completed.returncode:
            sys.exit(completed.returncode)
        forwarded = ["--language", language] if explicit else []
        os.execv(sys.executable, [sys.executable, sys.argv[0], *forwarded])
    os.chdir(release)
    sys.path.insert(0, str(release))
    from flightdeck import __main__ as application
    supported = getattr(application, "SUPPORTED_LANGUAGES", ())
    # Earlier launcher releases do not understand the language option. Keep
    # those rollbacks runnable while retaining this manager's translated UI.
    forwarded = ["--language", language] if explicit and language in supported else []
    desktop = (["--desktop"] if getattr(application, "SUPPORTED_DESKTOP", False) and
               not any(value in arguments for value in ("--desktop", "--no-browser", "-h", "--help")) else [])
    sys.argv = [sys.argv[0], *arguments, *forwarded, *desktop]
    application.main()
except OSError as error:
    print(text("start", reason=text("os", code=error.errno)), file=sys.stderr)
    sys.exit(1)
except (json.JSONDecodeError, KeyError, TypeError):
    print(text("start", reason=text("invalid")), file=sys.stderr)
    sys.exit(1)
except ValueError as error:
    print(text("start", reason=str(error)), file=sys.stderr)
    sys.exit(1)
'''.replace("LANGUAGE_LITERAL", repr(language)).replace("ROOT_LITERAL", repr(str(root)))).encode()


def desktop_bytes(launcher: Path, icon: Path, language: str = "en") -> bytes:
    def quoted(value: Path) -> str:
        text = str(value).replace("%", "%%")
        for char in ("\\", '"', "`", "$"):
            text = text.replace(char, "\\" + char)
        return '"' + text + '"'
    comment = "MSFS Xbox PC unter Linux starten"
    return ("[Desktop Entry]\nType=Application\nVersion=1.0\nName=Flightdeck\n"
            f"Comment={tr(comment, language=language)}\n"
            f"Comment[de]={tr(comment, language='de')}\nComment[en]={tr(comment, language='en')}\n"
            f"Exec={quoted(launcher)}\nIcon={icon}\nTerminal=false\n"
            "Categories=Game;Simulation;\nStartupNotify=false\n").encode()


def apply_entries(root: Path, old: dict | None, state: dict, entries: dict[str, tuple[Path, bytes, int]]) -> None:
    backups = []
    try:
        for role, (path, data, mode) in entries.items():
            directory(path.parent)
            no_links(path)
            before = None
            before_mode = mode
            previous = (old or {}).get("entries", {}).get(role)
            if path.exists():
                before = read_regular(path)
                before_mode = stat.S_IMODE(path.stat().st_mode)
                if not previous or previous["path"] != str(path) or digest(before) != previous["sha256"]:
                    raise InstallError(tr('Vorhandene fremde oder geänderte Datei bleibt erhalten: {path}', path=path))
            elif previous:
                raise InstallError(tr('Der installierte Eintrag fehlt: {path}. Bitte zuerst die Installation prüfen.', path=path))
            atomic_write(path, data, mode)
            backups.append((path, before, before_mode))
            state["entries"][role] = {"path": str(path), "sha256": digest(data)}
        # This single atomic pointer selects the complete source release.
        atomic_write(root / "installation.json", json_bytes(state))
    except Exception:
        for path, before, mode in reversed(backups):
            if before is None:
                path.unlink(missing_ok=True)
            else:
                atomic_write(path, before, mode)
        raise


def install(source: Path, root: Path, bin_dir: Path, applications_dir: Path, desktop: bool = True, *, language: str | None = None) -> dict:
    language = LANGUAGE if language is None else language
    if language not in {"de", "en"}:
        raise InstallError(tr("Bitte --language de oder --language en wählen."))
    files = source_snapshot(source)  # Freeze all input bytes before mutating installation paths.
    identity, record = release_identity(files)
    root, bin_dir, applications_dir = map(absolute, (root, bin_dir, applications_dir))
    with installation_lock(root):
        old = load_installation(root)
        if old:
            verify_release(root, old["current"])
            bin_dir = Path(old["entries"]["launcher"]["path"]).parent
            if "desktop" in old["entries"]:
                applications_dir = Path(old["entries"]["desktop"]["path"]).parent
                desktop = True
        elif any(p.name != ".install.lock" and not
                 (p.name == "releases" and p.is_dir() and not p.is_symlink() and not any(p.iterdir()))
                 for p in root.iterdir()):
            raise InstallError(tr('Das Zielverzeichnis enthält fremde Dateien. Bitte ein leeres Installationsziel wählen.'))
        releases = root / "releases"
        directory(releases)
        target = releases / identity
        no_links(target)
        created_release = not target.exists()
        if target.exists():
            verify_release(root, identity)
        else:
            staging = Path(tempfile.mkdtemp(prefix=".staging-", dir=releases))
            try:
                for name, data in files.items():
                    # Only the two hash-pinned native entrypoints are programs.
                    # Source files and runtime libraries remain non-executable.
                    mode = 0o755 if name in {
                        "flightdeck/resources/native/bin/xodus-cli",
                        "flightdeck/resources/native/bin/xodus-service",
                        "flightdeck/resources/native/bin/flightdeck-connected-storage.exe",
                    } else 0o644
                    atomic_write(staging / name, data, mode)
                atomic_write(staging / "release.json", json_bytes(record))
                os.replace(staging, target)
            finally:
                # Only files just created by this invocation may be removed.
                if staging.exists():
                    for path in sorted(staging.rglob("*"), key=lambda p: len(p.parts), reverse=True):
                        if path.is_file():
                            path.unlink()
                        elif path.is_dir():
                            path.rmdir()
                    staging.rmdir()
        previous = old.get("previous") if old and old["current"] == identity else (old["current"] if old else None)
        state = {"app": APP, "format": FORMAT, "data_dir": str(root), "current": identity,
                 "previous": previous, "manager": identity, "language": language,
                 "entries": dict((old or {}).get("entries", {}))}
        launcher = bin_dir / "flightdeck"
        entries = {"launcher": (launcher, launcher_bytes(root, language), 0o700)}
        if desktop:
            entries["desktop"] = (applications_dir / "flightdeck.desktop",
                                  desktop_bytes(launcher, target / "ui/mark.svg", language), 0o644)
        try:
            apply_entries(root, old, state, entries)
        except Exception:
            if created_release:
                for name in files:
                    (target / name).unlink()
                (target / "release.json").unlink()
                for path in sorted(target.rglob("*"), key=lambda p: len(p.parts), reverse=True):
                    if path.is_dir() and not path.is_symlink():
                        path.rmdir()
                target.rmdir()
            if not old:
                releases.rmdir()
            raise
        return state


def rollback(root: Path, *, language: str | None = None) -> dict:
    language = LANGUAGE if language is None else language
    if language not in {"de", "en"}:
        raise InstallError(tr("Bitte --language de oder --language en wählen."))
    root = absolute(root)
    with installation_lock(root):
        old = load_installation(root)
        if not old or not old.get("previous"):
            raise InstallError(tr('Keine vorherige Launcher-Version für ein Rollback vorhanden.'))
        target = verify_release(root, old["previous"])
        state = {**old, "current": old["previous"], "previous": old["current"], "language": language, "entries": dict(old["entries"])}
        launcher = Path(old["entries"]["launcher"]["path"])
        entries = {"launcher": (launcher, launcher_bytes(root, language), 0o700)}
        if "desktop" in old["entries"]:
            entries["desktop"] = (Path(old["entries"]["desktop"]["path"]),
                                  desktop_bytes(launcher, target / "ui/mark.svg", language), 0o644)
        apply_entries(root, old, state, entries)
        return state


def unlink_owned(path: Path, checksum: str, retained: list[str]) -> None:
    if not path.exists() and not path.is_symlink():
        return
    try:
        if digest(read_regular(path)) != checksum:
            raise InstallError("changed")
        path.unlink()
    except (OSError, InstallError):
        retained.append(str(path))


def uninstall(root: Path) -> list[str]:
    root = absolute(root)
    retained = []
    with installation_lock(root):
        state = load_installation(root)
        if not state:
            raise InstallError(tr('Hier ist kein Flightdeck-Launcher installiert.'))
        releases = root / "releases"
        no_links(releases)
        for entry in state["entries"].values():
            unlink_owned(Path(entry["path"]), entry["sha256"], retained)
        for folder in releases.iterdir():
            try:
                _, record = release_manifest(root, folder.name)
            except (InstallError, OSError):
                retained.append(str(folder))
                continue
            for name, checksum in record["files"].items():
                unlink_owned(folder / name, checksum, retained)
            # Foreign, changed and symbolic-link files are never recursively deleted.
            for directory_path in sorted(folder.rglob("*"), key=lambda p: len(p.parts), reverse=True):
                if directory_path.is_dir() and not directory_path.is_symlink():
                    try:
                        directory_path.rmdir()
                    except OSError:
                        pass
            remaining = [p for p in folder.iterdir() if p.name != "release.json"]
            if remaining:
                retained.append(str(folder))
            else:
                (folder / "release.json").unlink()
                folder.rmdir()
        (root / "installation.json").unlink()
        try:
            releases.rmdir()
        except OSError:
            pass
    # Retain the lock inode so concurrent callers can never lock a replacement
    # inode while the old one is still held. The small manager directory stays.
    return sorted(set(retained))


def main(argv=None) -> int:
    global LANGUAGE
    arguments = list(sys.argv[1:] if argv is None else argv)
    previous_language = LANGUAGE
    LANGUAGE = detect_language()
    try:
        LANGUAGE = language_option(arguments)
        parser = LocalizedParser(description=tr("Flightdeck für das eigene Linux-Benutzerkonto installieren."),
                                 add_help=False, allow_abbrev=False)
        parser._positionals.title = tr("Argumente")
        parser._optionals.title = tr("Optionen")
        parser.add_argument("-h", "--help", action="help", help=tr("Diese Hilfe anzeigen und beenden"))
        parser.add_argument("--language", choices=("de", "en"),
                            help=tr("Sprache: de oder en (Standard: Systemsprache)"))
        parser.add_argument("--source", type=Path, default=SOURCE_ROOT, help=tr("Pfad zum entpackten Quellpaket/Clone"))
        parser.add_argument("--data-dir", type=Path, default=Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "flightdeck-launcher",
                            help=tr("Verzeichnis für verwaltete Launcher-Versionen"))
        parser.add_argument("--bin-dir", type=Path, default=Path.home() / ".local/bin", help=tr("Verzeichnis für den Launcher-Befehl"))
        parser.add_argument("--applications-dir", type=Path, default=Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "applications",
                            help=tr("Verzeichnis für den Menüeintrag"))
        desktop = parser.add_mutually_exclusive_group()
        desktop.add_argument("--desktop", dest="desktop", action="store_true", default=True, help=tr("Menüeintrag anlegen (Standard)"))
        desktop.add_argument("--no-desktop", dest="desktop", action="store_false", help=tr("Keinen neuen Menüeintrag anlegen"))
        parser.add_argument("--no-launch", action="store_true", help=tr("Oberfläche nach Installation nicht öffnen"))
        parser.add_argument("--gui", action="store_true", help=tr("Grafischen Installationsdialog öffnen"))
        actions = parser.add_mutually_exclusive_group()
        actions.add_argument("--uninstall", action="store_true", help=tr("Launcher entfernen; Einstellungen und Spielstände behalten"))
        actions.add_argument("--rollback", action="store_true", help=tr("Zur vorherigen Launcher-Version wechseln"))
        args = parser.parse_args(arguments)
        if sys.platform != "linux" or sys.version_info < (3, 10):
            raise InstallError(tr("Benötigt Linux und Python 3.10 oder neuer."))
        if os.geteuid() == 0:
            raise InstallError(tr("Bitte als normaler Benutzer ohne sudo starten."))
        if args.gui:
            # Keep the optional desktop toolkit outside the CLI dependency path.
            helper = Path(__file__).with_name("install-launcher-gui.py")
            specification = importlib.util.spec_from_file_location("flightdeck_install_gui", helper)
            module = importlib.util.module_from_spec(specification)
            specification.loader.exec_module(module)
            def operation():
                if args.uninstall:
                    retained = uninstall(args.data_dir)
                    message = tr("Launcher entfernt. Einstellungen, Runtimes und Spielstände bleiben erhalten.")
                    if retained:
                        message += "\n" + tr("Geänderte oder fremde Dateien wurden behalten:\n{paths}", paths="\n".join(retained))
                    return message, None
                state = rollback(args.data_dir, language=LANGUAGE) if args.rollback else install(
                    args.source, args.data_dir, args.bin_dir, args.applications_dir, args.desktop, language=LANGUAGE)
                launcher = state["entries"]["launcher"]["path"]
                return tr("Flightdeck installiert: {launcher}\nQuellstand: {release}", launcher=launcher,
                          release=state["current"][:12]), None if args.no_launch else launcher
            def explain(error):
                reason = str(error) if isinstance(error, InstallError) else tr(
                    "Dateisystemfehler {code}: {path}", code=getattr(error, "errno", "?"),
                    path=getattr(error, "filename", None) or tr("Unbekannter Pfad"))
                return tr("Installation nicht abgeschlossen: {error}", error=reason)
            return module.run(operation, explain, LANGUAGE, args.language, uninstalling=args.uninstall)
        if args.uninstall:
            retained = uninstall(args.data_dir)
            print(tr("Launcher entfernt. Einstellungen, Runtimes und Spielstände bleiben erhalten."))
            if retained:
                print(tr("Geänderte oder fremde Dateien wurden behalten:\n{paths}", paths="\n".join(retained)))
            return 0
        state = rollback(args.data_dir, language=LANGUAGE) if args.rollback else install(
            args.source, args.data_dir, args.bin_dir, args.applications_dir, args.desktop, language=LANGUAGE)
        launcher = state["entries"]["launcher"]["path"]
        print(tr("Flightdeck installiert: {launcher}\nQuellstand: {release}", launcher=launcher, release=state["current"][:12]), flush=True)
        print(tr("Update: Installer aus einem neuen Quellpaket erneut ausführen.\nRollback: flightdeck --rollback · Entfernen: flightdeck --uninstall"), flush=True)
        if not args.no_launch:
            forwarded = ["--language", args.language] if args.language else []
            os.execv(sys.executable, [sys.executable, launcher, *forwarded])
        return 0
    except (InstallError, OSError) as error:
        LANGUAGE = getattr(error, "language", None) or LANGUAGE
        reason = str(error) if isinstance(error, InstallError) else tr(
            "Dateisystemfehler {code}: {path}", code=error.errno, path=error.filename or tr("Unbekannter Pfad"))
        print(tr("Installation nicht abgeschlossen: {error}", error=reason), file=sys.stderr)
        return 1
    finally:
        LANGUAGE = previous_language


if __name__ == "__main__":
    raise SystemExit(main())
