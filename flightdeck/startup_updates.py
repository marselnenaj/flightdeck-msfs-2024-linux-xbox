# SPDX-License-Identifier: MIT
"""Non-blocking update discovery; installation still requires explicit preflight."""
import threading
import time

from . import game_update, games
from .backend import utc_now


class StartupUpdates:
    def __init__(self, launcher):
        self.launcher = launcher
        self.lock = threading.RLock()
        self.records = {}
        self.thread = None
        self.cancel = threading.Event()
        self.closed = False

    def check(self):
        launcher = self.launcher
        with launcher.lock, self.lock:
            launcher.require_open()
            if self.closed:
                return {"ok": True}
            launcher.launcher_updates.check_on_startup()
            runtime = launcher.runtime
            previous = self.records.get(runtime)
            if runtime is None:
                return {"ok": True}
            if (launcher.setup_busy or launcher.process is not None
                    or launcher._external(create_lock=False)
                    or self.thread is not None and self.thread.is_alive()):
                return {"ok": True, "deferred": True}
            if previous is not None and time.monotonic() - previous["attempt"] < 1800:
                return {"ok": True}
            # A manual comparison already supplies fresher information. Never
            # replace its selected package, login flow or install capability.
            with launcher.setup.lock:
                job = launcher.setup.job
                if job and job.get("mode") == "update" and job.get("runtime_path") == str(runtime):
                    return {"ok": True}
            self.cancel = threading.Event()
            self.records[runtime] = {"attempt": time.monotonic(), "state": "checking"}
            self.thread = threading.Thread(target=self._game, args=(runtime, self.cancel),
                                           name="flightdeck-update-discovery", daemon=True)
            self.thread.start()
            return {"ok": True}

    def _game(self, runtime, cancel):
        try:
            # Version discovery neither reserves the simulator nor creates an
            # install plan. The existing explicit preflight rechecks everything
            # under its runtime lease before offering a download.
            game_update._private(runtime / "private")
            spec = games.for_runtime(runtime)
            current = game_update.installed_identity(games.path(runtime), game_id=spec.id)
            cli, checksum, _ = game_update.tools(runtime, self.launcher.setup.source_root)
            market = game_update.configured_market(runtime)
            latest = game_update.package_info(cli, checksum, runtime, market, cancel)
            if game_update.version(latest["version"]) < game_update.version(current["version"]):
                raise game_update.UpdateError("Older Store version")
            newer = game_update.version(latest["version"]) > game_update.version(current["version"])
            result = {"state": "complete", "installed_version": current["version"],
                      "latest_version": latest["version"], "update_available": newer,
                      "checked_at": utc_now()}
        except game_update.AuthRequired:
            result = {"state": "failed", "auth_required": True,
                      "error": "Für die Updateprüfung ist eine erneute Microsoft-Anmeldung erforderlich."}
        except Exception:
            result = {"state": "failed", "error": "Die Updateprüfung konnte nicht abgeschlossen werden. Verbindung prüfen und erneut versuchen."}
        with self.lock:
            if not cancel.is_set() and not self.closed:
                self.records[runtime].update(result)

    def snapshot(self, runtime):
        with self.lock:
            return {key: value for key, value in self.records.get(runtime, {}).items() if key != "attempt"}

    def close(self):
        with self.lock:
            self.closed = True
            self.cancel.set()
            thread = self.thread
        if thread is not None and thread is not threading.current_thread():
            thread.join(timeout=3)
