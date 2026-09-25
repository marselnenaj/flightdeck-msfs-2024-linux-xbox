"""Loopback-only HTTP interface for the local launcher (MIT)."""
from __future__ import annotations

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import secrets
import threading
from urllib.parse import urlsplit

from .backend import Launcher, LauncherError
from .i18n import error_message, language, localize


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, launcher: Launcher, port=0, ui_root=None):
        from .desktop import release_identity
        from .runtime_components import refresh_on_startup
        try:
            refresh_on_startup(launcher)
        except (LauncherError, OSError, ValueError) as error:
            # Keep the local UI available to explain the failed update. The
            # runtime readiness check prevents a partial component set from
            # being started, and a later launch can retry the journal.
            launcher.component_update_error = error_message(error)
        self.launcher = launcher
        self.token = secrets.token_urlsafe(32)
        self.release_identity = release_identity()
        self.desktop_service = False
        self.update_pending = False
        packaged_ui = Path(__file__).resolve().parent / "ui"
        source_ui = Path(__file__).resolve().parents[1] / "ui"
        self.ui_root = Path(ui_root) if ui_root else (packaged_ui if packaged_ui.is_dir() else source_ui)
        super().__init__(("127.0.0.1", port), Handler)
        self.authorities = {f"127.0.0.1:{self.server_port}", f"localhost:{self.server_port}"}

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server_port}"

    def server_close(self):
        self.launcher.startup_updates.close()
        self.launcher.launcher_updates.close()
        self.launcher.cloud_saves.close()
        self.launcher.setup.close()
        self.launcher.fenix.close()
        super().server_close()

    def desktop_refresh(self):
        if not self.desktop_service:
            return {"ok": True, "refresh": "unsupported"}
        self.update_pending = True
        return {"ok": True, "refresh": "restarting" if self.launcher.reserve_desktop_refresh() else "busy"}


class Handler(BaseHTTPRequestHandler):
    server_version = "Flightdeck"
    sys_version = ""

    def setup(self):
        super().setup()
        self.connection.settimeout(10)

    def log_message(self, *args):
        # Request strings can include local paths; no access log is needed.
        pass

    def reply(self, status, body, content_type="application/json; charset=utf-8"):
        if isinstance(body, (dict, list)):
            body = json.dumps(localize(body, language(self.headers.get("Accept-Language"))), ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Cross-Origin-Resource-Policy", "same-origin")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")
        self.end_headers()
        self.wfile.write(body)

    def error(self, code, message):
        self.reply(code, {"ok": False, "error": message})

    def trusted_request(self):
        hosts = self.headers.get_all("Host", [])
        if len(hosts) != 1 or hosts[0] not in self.server.authorities:
            self.error(403, "Nur lokaler Zugriff ist erlaubt.")
            return False
        origins = self.headers.get_all("Origin", [])
        if origins and (len(origins) != 1 or origins[0] not in {f"http://{a}" for a in self.server.authorities}):
            self.error(403, "Diese Anfrage kommt nicht von Flightdeck.")
            return False
        if self.headers.get("Sec-Fetch-Site") == "cross-site":
            self.error(403, "Zugriff von einer anderen Website ist gesperrt.")
            return False
        return True

    def do_GET(self):
        if not self.trusted_request():
            return
        path = urlsplit(self.path).path
        try:
            if path == "/api/status":
                value = self.server.launcher.status()
                value["csrf_token"] = self.server.token
                value["service"] = {"release": self.server.release_identity, "desktop": self.server.desktop_service,
                                    "update_pending": self.server.update_pending,
                                    "message": "Ein Launcher-Update ist bereit. Bitte Spiel oder Einrichtung abschließen und Flightdeck erneut öffnen. Die aktuelle Sitzung läuft weiter." if self.server.update_pending else ""}
                self.reply(200, value)
            elif path == "/api/diagnostics":
                self.reply(200, self.server.launcher.diagnostics())
            elif path == "/api/game-update":
                from .game_update import snapshot
                self.reply(200, snapshot(self.server.launcher))
            elif path == "/api/launcher-update":
                self.reply(200, self.server.launcher.launcher_updates.snapshot())
            elif path == "/api/cloud-saves":
                self.reply(200, self.server.launcher.cloud_saves.snapshot())
            elif path == "/api/fenix":
                self.reply(200, self.server.launcher.fenix.snapshot())
            elif path == "/api/mods":
                from .mods import snapshot
                self.reply(200, snapshot(self.server.launcher))
            elif path == "/api/setup":
                self.reply(200, self.server.launcher.setup.snapshot())
            elif path == "/api/setup/discover":
                self.reply(200, self.server.launcher.setup.discover())
            elif path in {"/", "/index.html", "/app.js", "/setup.js", "/mods.js", "/fenix.js", "/updates.js", "/launcher-updates.js", "/notices.js", "/cloud-saves.js", "/i18n.js", "/state.js", "/styles.css", "/mark.svg", "/flight-panorama.png", "/flight-panorama-2020.png", "/manrope-variable.woff2"}:
                name = "index.html" if path == "/" else path[1:]
                types = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2"}
                file = self.server.ui_root / name
                self.reply(200, file.read_bytes(), types[file.suffix])
            else:
                self.error(404, "Nicht gefunden.")
        except (OSError, LauncherError, ValueError):
            self.error(500, "Lokale Daten konnten nicht gelesen werden. Bitte die Runtime prüfen.")

    def do_POST(self):
        if not self.trusted_request():
            return
        supplied = self.headers.get_all("X-Flightdeck-Token", [])
        if len(supplied) != 1 or not secrets.compare_digest(supplied[0].encode(), self.server.token.encode()):
            self.error(403, "Die Sitzung ist ungültig. Bitte die Seite neu laden.")
            return
        if self.headers.get_content_type() != "application/json" or self.headers.get("Transfer-Encoding"):
            self.error(415, "Eine JSON-Anfrage ist erforderlich.")
            return
        try:
            lengths = self.headers.get_all("Content-Length", [])
            length = int(lengths[0]) if len(lengths) == 1 else -1
        except ValueError:
            length = -1
        if not 0 <= length <= 16384:
            self.error(413, "Die Anfrage ist zu groß oder unvollständig.")
            return
        try:
            data = json.loads(self.rfile.read(length))
            if not isinstance(data, dict):
                raise ValueError()
        except (ValueError, UnicodeError, TimeoutError):
            self.error(400, "Die Anfrage enthält kein gültiges JSON-Objekt.")
            return
        path = urlsplit(self.path).path
        try:
            launcher = self.server.launcher
            if path != "/api/desktop/refresh":
                with launcher.lock:
                    launcher.require_open()
            if path == "/api/desktop/refresh":
                result = self.server.desktop_refresh()
            elif path == "/api/updates/check-startup":
                result = launcher.startup_updates.check()
            elif path in {"/api/launcher-update/check", "/api/launcher-update/install", "/api/launcher-update/rollback"}:
                result = launcher.launcher_updates.start(path.rsplit("/", 1)[-1], data.get("check_id"))
            elif path == "/api/launcher-update/cancel":
                result = launcher.launcher_updates.cancel(data.get("job_id"))
            elif path == "/api/launcher-update/restart":
                result = launcher.launcher_updates.restart(language(self.headers.get("Accept-Language")))
            elif path == "/api/config":
                result = launcher.configure(data.get("runtime_path"))
            elif path == "/api/game/select":
                result = launcher.select_game(data.get("game_id"))
            elif path == "/api/game/register":
                result = launcher.register_runtime(data.get("runtime_path"))
            elif path == "/api/launch":
                result = launcher.launch()
            elif path == "/api/stop":
                result = launcher.stop()
            elif path == "/api/saves/backup":
                result = launcher.backup()
            elif path in {"/api/cloud-saves/retry", "/api/cloud-saves/play-local", "/api/cloud-saves/cancel-auto", "/api/cloud-saves/resolve"}:
                result = launcher.cloud_saves.automation.action(path.rsplit("/", 1)[-1],
                    data.get("request_id"), choice=data.get("choice"))
            elif path in {"/api/cloud-saves/check", "/api/cloud-saves/download", "/api/cloud-saves/prepare-import"}:
                result = launcher.cloud_saves.start(path.rsplit("/", 1)[-1])
            elif path == "/api/cloud-saves/import":
                result = launcher.cloud_saves.start("import", plan_id=data.get("plan_id"), choice=data.get("choice"))
            elif path == "/api/cloud-saves/upload":
                result = launcher.cloud_saves.start("upload", plan_id=data.get("plan_id"), choice=data.get("choice"))
            elif path == "/api/cloud-saves/restore":
                result = launcher.cloud_saves.start("restore", backup_id=data.get("backup_id"))
            elif path == "/api/cloud-saves/discard-plan":
                result = launcher.cloud_saves.discard_plan(data.get("plan_id"))
            elif path == "/api/cloud-saves/cancel":
                result = launcher.cloud_saves.cancel(data.get("job_id"))
            elif path == "/api/fenix/pick":
                result = launcher.fenix.pick(data.get("kind"))
            elif path.startswith("/api/fenix/"):
                result = launcher.fenix.start(path.rsplit("/", 1)[-1], data)
            elif path == "/api/mods/open-folder":
                from .mods import open_folder
                result = open_folder(launcher)
            elif path == "/api/game-update/check":
                result = launcher.setup.check({"mode": "update", "sign_in": data.get("sign_in", False)})
            elif path == "/api/game-update/verify":
                result = launcher.setup.check({"mode": "update", "operation": "verify"})
            elif path == "/api/game-update/repair/check":
                result = launcher.setup.check({"mode": "update", "operation": "repair", "sign_in": data.get("sign_in", False)})
            elif path == "/api/game-update/start":
                with launcher.setup.lock:
                    if not launcher.setup.job or launcher.setup.job.get("mode") != "update":
                        raise LauncherError("Bitte Updates zuerst erneut prüfen.")
                    result = launcher.setup.start(data.get("check_id"))
            elif path == "/api/game-update/rollback":
                from .game_update import rollback
                result = rollback(launcher)
            elif path == "/api/setup/check":
                result = launcher.setup.check(data)
            elif path == "/api/setup/start":
                result = launcher.setup.start(data.get("check_id"))
            elif path == "/api/setup/cancel":
                result = launcher.setup.cancel(data.get("job_id"))
            elif path in {"/api/setup/pause", "/api/setup/resume"}:
                result = launcher.setup.download_action(data.get("job_id"), path.rsplit("/", 1)[-1])
            elif path == "/api/setup/pick":
                result = launcher.setup.pick(data.get("field"), data.get("initial"), language=language(self.headers.get("Accept-Language")))
            else:
                self.error(404, "Nicht gefunden.")
                return
            restarting = path == "/api/desktop/refresh" and result.get("refresh") == "restarting"
            try:
                self.reply(200, result)
            finally:
                if restarting:
                    # Once idle shutdown is reserved, even a disconnected
                    # client must not leave the old service permanently closed
                    # to new work. shutdown runs off serve_forever's thread.
                    threading.Thread(target=self.server.shutdown, daemon=True).start()
        except LauncherError as error:
            self.error(409, error_message(error))
        except OSError:
            self.error(500, "Der lokale Vorgang ist fehlgeschlagen. Bitte Pfad und Schreibrechte prüfen.")
