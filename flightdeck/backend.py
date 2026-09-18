"""Local launcher API. No account tokens or raw game logs leave this process."""
from __future__ import annotations

from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import tempfile
import threading
import time

from . import __version__


class LauncherError(Exception):
    pass


def utc_now():
    return datetime.now(timezone.utc).isoformat()


def atomic_json(path: Path, value):
    fd, name = tempfile.mkstemp(prefix=".flightdeck-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, ensure_ascii=False)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


def regular_files(root: Path):
    """Do not follow save-folder symlinks, devices or sockets."""
    if not root.is_dir() or root.is_symlink():
        return
    for directory, dirs, names in os.walk(root, followlinks=False):
        dirs[:] = [n for n in dirs if not (Path(directory) / n).is_symlink()]
        for name in names:
            path = Path(directory) / name
            try:
                if stat.S_ISREG(path.lstat().st_mode):
                    yield path
            except FileNotFoundError:
                continue


def open_save_file(root_fd, relative):
    """Resolve every component relative to held descriptors, without symlinks."""
    parts = relative.parts
    if not parts or any(x in {"", ".", ".."} for x in parts):
        raise LauncherError("Ungültiger Speicherpfad.")
    directory_fd = os.dup(root_fd)
    try:
        for name in parts[:-1]:
            next_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory_fd)
            os.close(directory_fd)
            directory_fd = next_fd
        return os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory_fd)
    finally:
        os.close(directory_fd)


class Launcher:
    def __init__(self, state_dir: Path, runtime: str | None = None):
        self.state_dir = state_dir.expanduser().resolve()
        self.state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        if self.state_dir.stat().st_uid != os.getuid():
            raise LauncherError("Der Einstellungsordner gehört einem anderen Benutzer.")
        self.state_dir.chmod(0o700)
        self.config_file = self.state_dir / "config.json"
        self.lock = threading.RLock()
        self.process = None
        # The automatic cloud coordinator owns this exact session through
        # pre-start sync, process wait and post-exit sync. Status polling must
        # not consume its exit notification or release its reservation.
        self.managed_session = None
        self.started_at = None
        self.started_monotonic = None
        self.exit_code = None
        self.stopping = False
        self.setup_busy = False
        self._owned_runtime_operation = None
        self.desktop_closing = False
        self.runtime = None
        if self.config_file.exists():
            try:
                value = json.loads(self.config_file.read_text(encoding="utf-8"))
                self.runtime = self.validate_runtime(value["runtime_path"])
            except (OSError, ValueError, KeyError, TypeError, LauncherError):
                self.runtime = None
        if runtime is not None:
            self.configure(runtime)
        from .setup import SetupManager
        self.setup = SetupManager(self)
        from .cloud_sync import CloudSaveManager
        self.cloud_saves = CloudSaveManager(self)

    @staticmethod
    def validate_runtime(value):
        if not isinstance(value, str) or not value.strip() or len(value) > 4096:
            raise LauncherError("Bitte einen vorbereiteten Runtimeordner auswählen.")
        path = Path(value).expanduser()
        if not path.is_absolute():
            raise LauncherError("Der Runtimepfad muss absolut sein.")
        path = path.resolve()
        if not path.is_dir() or not (path / "tools/play-msfs.sh").is_file():
            raise LauncherError("Hier fehlt tools/play-msfs.sh. Bitte zuerst die Runtime vorbereiten.")
        if path.stat().st_uid != os.getuid():
            raise LauncherError("Bitte eine Runtime im eigenen Benutzerkonto auswählen.")
        return path

    def _poll(self):
        if self.process is not None and self.managed_session is None:
            code = self.process.poll()
            if code is not None:
                self.exit_code = code
                self.process = None
                self.stopping = False

    @contextmanager
    def runtime_lock(self, *, create=True, operation=None):
        if self.runtime is None:
            raise LauncherError("Zuerst eine Runtime auswählen.")
        private = self.runtime / "private"
        if not private.is_dir() or private.is_symlink():
            raise LauncherError("Der private Runtimeordner fehlt oder ist ungültig.")
        flags = (os.O_RDWR | os.O_CREAT if create else os.O_RDONLY) | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0)
        try:
            fd = os.open(private / "play.lock", flags, 0o600)
            with os.fdopen(fd, "a" if create else "r") as stream:
                if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                    raise LauncherError("Die Runtime-Sperrdatei ist keine reguläre Datei.")
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                # Status may distinguish this exact acquired lease from another
                # process. Safety probes still attempt flock normally.
                with self.lock:
                    owner = (self.runtime, operation) if operation else None
                    if owner:
                        self._owned_runtime_operation = owner
                try:
                    yield stream.fileno()
                finally:
                    with self.lock:
                        if owner and self._owned_runtime_operation == owner:
                            self._owned_runtime_operation = None
        except BlockingIOError:
            raise LauncherError("Das Spiel oder ein anderer Runtimevorgang läuft bereits.") from None

    def _external(self, *, create_lock=True):
        if self.runtime is None or self.process is not None:
            return False
        try:
            with self.runtime_lock(create=create_lock):
                return False
        except FileNotFoundError:
            return create_lock
        except LauncherError:
            return (self.runtime / "private").is_dir()
        except OSError:
            # An unreadable or substituted lock is not evidence of idleness.
            return True

    def configure(self, runtime_path):
        with self.lock:
            self.require_open()
            if self.setup_busy:
                raise LauncherError("Bitte die laufende Einrichtung abschließen oder abbrechen.")
            self._poll()
            if self.process is not None or self._external():
                raise LauncherError("Die Runtime kann während eines Spielstarts nicht gewechselt werden.")
            selected = self.validate_runtime(runtime_path)
            atomic_json(self.config_file, {"schema": 1, "runtime_path": str(selected)})
            self.runtime = selected
            self.exit_code = None
            return {"ok": True}

    def reserve_setup(self):
        with self.lock:
            self.require_open()
            self._poll()
            if self.setup_busy or self.process is not None or self._external(create_lock=False):
                raise LauncherError("Die Einrichtung benötigt ein beendetes Spiel und darf nur einmal laufen.")
            self.setup_busy = True

    def require_open(self):
        if self.desktop_closing:
            raise LauncherError("Der Launcher wird für ein Update neu gestartet. Bitte gleich erneut versuchen.")

    def reserve_desktop_refresh(self):
        """Close only an idle UI, excluding a concurrent launch/setup/backup.

        All mutating launcher operations share this lock. Once reserved, setup
        cannot start and no new game or backup can be created by this instance.
        External games are observed through the existing private runtime lock.
        """
        with self.lock:
            self._poll()
            if self.setup_busy or self.process is not None or self._external(create_lock=False):
                return False
            if self.setup.picker_lock.locked():
                return False
            self.desktop_closing = True
            return True

    def release_setup(self):
        with self.lock:
            self.setup_busy = False

    def finish_setup(self, path):
        with self.lock:
            self.setup_busy = False
            return self.configure(path)

    def checks(self):
        root = self.runtime
        definitions = (
            ("launcher", "Startprogramm", "tools/play-msfs.sh", True),
            ("game", "Eigenes MSFS-PC-Spielpaket", "games/MSFS2024/FlightSimulator2024.exe", False),
            ("prefix", "Wine-Umgebung", "local/msfs-prefix/system.reg", False),
            ("bridge", "Kompatibilitätsbibliothek", "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll", False),
        )
        result = []
        for key, label, name, executable in definitions:
            path = root / name if root else None
            ok = bool(path and path.is_file() and (not executable or os.access(path, os.X_OK)))
            result.append({"id": key, "label": label, "ok": ok,
                           "detail": "Vorhanden" if ok else "Runtime vorbereiten oder Pfad prüfen"})
        private_ok = bool(root and (root / "private").is_dir() and not (root / "private").is_symlink())
        if private_ok:
            lock = root / "private/play.lock"
            private_ok = not lock.is_symlink() and (not lock.exists() or lock.is_file())
        result.append({"id": "private", "label": "Privater Datenordner", "ok": private_ok,
                       "detail": "Vorhanden" if private_ok else "Privater Datenordner fehlt"})
        return result

    def saves(self, idle):
        root = self.runtime
        selected = bool(root and (root / "private/local-saves.enabled").is_file())
        folder = root / "private/local-saves" if root else None
        available = bool(selected and folder and folder.is_dir() and not folder.is_symlink())
        files, size = 0, 0
        if available:
            for path in regular_files(folder):
                if path.name.endswith(".lock") or path.name.startswith(".tmp"):
                    continue
                try:
                    size += path.stat().st_size
                    files += 1
                except FileNotFoundError:
                    continue
        backups = []
        if root:
            backup_root = root / "private/save-backups"
            if backup_root.is_dir() and not backup_root.is_symlink():
                for directory in backup_root.glob("backup-*"):
                    if directory.is_dir() and not directory.is_symlink() and (directory / "manifest.json").is_file():
                        backups.append(directory)
        backups.sort(key=lambda x: x.name)
        last = backups[-1] if backups else None
        return {"mode": "local" if available else "unavailable", "available": available,
                "bytes": size, "files": files, "backups": len(backups),
                "can_backup": bool(available and idle and files),
                "last_backup": {"name": last.name, "created_at": datetime.fromtimestamp(last.stat().st_mtime, timezone.utc).isoformat()} if last else None}

    def status(self):
        with self.lock:
            self._poll()
            owned = self._owned_runtime_operation
            external = False if owned and owned[0] == self.runtime else self._external()
            state = "stopped"
            if self.process is not None:
                state = "stopping" if self.stopping else ("starting" if time.monotonic() - self.started_monotonic < 15 else "running")
            elif external:
                state = "external"
            checks = self.checks()
            ready = all(x["ok"] for x in checks)
            return {"app": {"name": "Flightdeck", "version": __version__},
                    "runtime": {"configured": self.runtime is not None, "path": str(self.runtime) if self.runtime else "", "ready": ready, "checks": checks},
                    "game": {"state": state, "managed": self.process is not None,
                             "can_start": ready and state == "stopped" and not self.setup_busy and not self.desktop_closing, "can_stop": self.process is not None and not self.stopping,
                             "started_at": self.started_at, "exit_code": self.exit_code},
                    "saves": self.saves(state == "stopped" and not self.setup_busy and not self.desktop_closing),
                    "setup": {"busy": self.setup_busy},
                    "cloud": self.cloud_saves.automation.snapshot(),
                    "support": {"level": "experimental", "cloud_saves": True, "cloud_sync": "automatic", "automatic_cloud_sync": True}}

    def launch(self):
        automatic = getattr(self.cloud_saves, "launch", None)
        if callable(automatic):
            result = automatic()
            if result is not None:
                return result
        with self.lock:
            if not self.status()["game"]["can_start"]:
                raise LauncherError("Die Runtime ist nicht startbereit oder das Spiel läuft bereits.")
            # The launcher atomically owns play.lock; a competing launcher exits
            # harmlessly. Do not inherit that lock into our subprocess.
            with self.runtime_lock():
                pass
            self._spawn_reserved()
            return {"ok": True}

    def _require_managed_lease(self, runtime_lock_fd):
        if not self.setup_busy or self.managed_session is None:
            raise LauncherError("Die Runtime ist nicht für diesen Spielstart reserviert.")
        try:
            if type(runtime_lock_fd) is not int or runtime_lock_fd < 3:
                raise OSError()
            info = os.fstat(runtime_lock_fd)
            path = self.runtime / "private/play.lock"
            expected = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_nlink != 1 or info.st_mode & 0o077
                    or not stat.S_ISREG(expected.st_mode)
                    or (info.st_dev, info.st_ino) != (expected.st_dev, expected.st_ino)):
                raise OSError()
            # Another open-file-description must be excluded, while this
            # exact descriptor must already own the exclusive flock.
            probe = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
            try:
                try:
                    fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    pass
                else:
                    raise OSError()
                fcntl.flock(runtime_lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            finally:
                os.close(probe)
        except (OSError, ValueError, TypeError):
            raise LauncherError("Die Runtime unterstützt den gesperrten Spielstart nicht.") from None

    def _spawn_reserved(self, runtime_lock_fd=None):
        """Spawn while the caller holds self.lock and any session reservation.

        An automatic session lends its held runtime descriptor to the trusted
        packaged supervisor. The parent keeps ownership across the game and
        post-exit synchronization; game/service grandchildren never inherit it.
        The coordinator alone waits for and retires a managed Popen instance.
        """
        self.require_open()
        if self.runtime is None or self.process is not None:
            raise LauncherError("Die Runtime ist nicht startbereit oder das Spiel läuft bereits.")
        command = [str(self.runtime / "tools/play-msfs.sh")]
        inherited = ()
        if runtime_lock_fd is not None:
            self._require_managed_lease(runtime_lock_fd)
            try:
                from .setup import resource_paths
                tools, _ = resource_paths(self.setup.source_root)
                supervisor = tools / "play-msfs.sh"
                for file in (supervisor, self.runtime / "tools/runtime-env.sh",
                             self.runtime / "tools/launch-msfs.sh", self.runtime / "tools/xodus-service.sh"):
                    metadata = file.lstat()
                    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid():
                        raise OSError()
                command = ["bash", str(supervisor), "--runtime", str(self.runtime),
                           "--lock-fd", str(runtime_lock_fd)]
                inherited = (runtime_lock_fd,)
            except (OSError, ValueError, TypeError):
                raise LauncherError("Die Runtime unterstützt den gesperrten Spielstart nicht.") from None
        log = self.state_dir / "launcher.log"
        flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND | getattr(os, "O_NOFOLLOW", 0)
        fd = os.open(log, flags, 0o600)
        try:
            self.process = subprocess.Popen(command, cwd=self.runtime,
                                            stdin=subprocess.DEVNULL, stdout=fd, stderr=subprocess.STDOUT,
                                            start_new_session=True, close_fds=True, pass_fds=inherited, umask=0o077)
        except OSError:
            raise LauncherError("Das Startprogramm konnte nicht ausgeführt werden.") from None
        finally:
            os.close(fd)
        self.started_at = utc_now()
        self.started_monotonic = time.monotonic()
        self.exit_code = None
        self.stopping = False
        return self.process

    def stop(self):
        with self.lock:
            self._poll()
            if self.process is None or self.process.poll() is not None:
                raise LauncherError("Es läuft kein von Flightdeck gestartetes Spiel.")
            # Signal only the launcher we own. Its bounded cleanup handles only
            # its game/service children; unrelated Wine sessions are untouched.
            try:
                self.process.send_signal(signal.SIGTERM)
            except ProcessLookupError:
                self._poll()
            self.stopping = self.process is not None
            return {"ok": True}

    def backup(self):
        with self.lock:
            if not self.status()["saves"]["can_backup"]:
                raise LauncherError("Ein Backup benötigt vorhandene lokale Spielstände und ein beendetes Spiel.")
            with self.runtime_lock():
                return self._backup_local()

    def _backup_reserved(self, runtime_lock_fd):
        """Back up all existing local namespaces without an authenticated scope.

        Used for an explicitly chosen offline session, before and after play.
        The coordinator holds self.lock, its session reservation and runtime FD.
        No backup attempts to infer an Xbox identity or become a sync baseline.
        """
        self._require_managed_lease(runtime_lock_fd)
        if self.process is not None and self.process.poll() is None:
            raise LauncherError("Ein Backup benötigt vorhandene lokale Spielstände und ein beendetes Spiel.")
        return self._backup_local(allow_empty=True)

    def _backup_local(self, *, allow_empty=False):
        source = self.runtime / "private/local-saves"
        if allow_empty and not source.exists() and not source.is_symlink():
            return None
        destination = self.runtime / "private/save-backups"
        if destination.is_symlink():
            raise LauncherError("Der Backupordner darf kein symbolischer Link sein.")
        destination.mkdir(mode=0o700, exist_ok=True)
        name = datetime.now(timezone.utc).strftime("backup-%Y%m%d-%H%M%S-%f")
        stage = Path(tempfile.mkdtemp(prefix=".backup-", dir=destination))
        root_fd = destination_fd = None
        try:
            destination_fd = os.open(destination, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            root_fd = os.open(source, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            hashes = {}
            for path in regular_files(source):
                if path.name.endswith(".lock") or path.name.startswith(".tmp"):
                    continue
                relative = path.relative_to(source)
                target = stage / "data" / relative
                target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                fd = open_save_file(root_fd, relative)
                with os.fdopen(fd, "rb") as src, target.open("xb") as dst:
                    if not stat.S_ISREG(os.fstat(src.fileno()).st_mode):
                        raise LauncherError("Unerwartete Datei im Speicherordner.")
                    digest = hashlib.sha256()
                    while chunk := src.read(1024 * 1024):
                        digest.update(chunk)
                        dst.write(chunk)
                    dst.flush()
                    os.fsync(dst.fileno())
                target.chmod(0o600)
                hashes[str(relative)] = digest.hexdigest()
            if not hashes:
                if allow_empty:
                    shutil.rmtree(stage)
                    return None
                raise LauncherError("Es sind noch keine lokalen Spielstände vorhanden.")
            atomic_json(stage / "manifest.json", {"schema": 1, "created_at": utc_now(), "files": hashes})
            # Persist directory entries as well as file contents before publish.
            for directory, _, _ in os.walk(stage, topdown=False):
                directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
                try:
                    os.fsync(directory_fd)
                finally:
                    os.close(directory_fd)
            from .cloud_storage import CloudStorageError, _rename_noreplace
            try:
                _rename_noreplace(destination_fd, stage.name, name)
            except CloudStorageError:
                raise OSError("The local save backup could not be published safely.") from None
            os.fsync(destination_fd)
        except BaseException:
            shutil.rmtree(stage, ignore_errors=True)
            raise
        finally:
            for descriptor in (root_fd, destination_fd):
                if descriptor is not None:
                    os.close(descriptor)
        return {"ok": True, "backup": {"name": name, "created_at": utc_now()}, "files": len(hashes)}

    def diagnostics(self):
        """Extract numeric allowlisted outcomes, never raw lines or identities."""
        summary = {"run_found": False, "auth_http": [], "local_save_init": [], "exit": None}
        root = self.runtime
        if root:
            runs = [p for p in (root / "private").glob("run-*") if re.fullmatch(r"run-\d{8}-\d{6}-[A-Za-z0-9]+", p.name) and p.is_dir() and not p.is_symlink()]
            if runs:
                run = max(runs, key=lambda p: p.name)
                path = run / "game.log"
                try:
                    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                    with os.fdopen(fd, "rb") as stream:
                        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                            raise OSError("not regular")
                        # Initialization lives near the start; recent exit near
                        # the end. Never load an unbounded private log into RAM.
                        first = stream.read(1024 * 1024)
                        stream.seek(max(0, os.fstat(stream.fileno()).st_size - 512 * 1024))
                        text = (first + b"\n" + stream.read(512 * 1024)).decode("utf-8", errors="replace")
                    summary["run_found"] = True
                    summary["auth_http"] = sorted(set(int(x) for x in re.findall(r"xodus-title-auth: host=(?:user|device|title|xsts)\.auth\.xboxlive\.com status=(\d{3})\b", text)))
                    summary["local_save_init"] = [{"enabled": int(e), "sync_on_demand": int(s), "hresult": h.lower()} for e, s, h in dict.fromkeys(re.findall(r"\[xodus-gamesave\] local_init enabled=([01]) sync_on_demand=([01]) hr=([0-9a-fA-F]{8})\b", text))]
                    exits = re.findall(r"xodus-wine-launch: wine_pid=\d+ exit_code=(\d+) elapsed_seconds=(\d+(?:\.\d+)?)(?=\s|$)", text)
                    if exits:
                        summary["exit"] = {"code": int(exits[-1][0]), "seconds": float(exits[-1][1])}
                except OSError:
                    pass
        return {"generated_at": utc_now(), "summary": summary, "checks": self.checks()}
