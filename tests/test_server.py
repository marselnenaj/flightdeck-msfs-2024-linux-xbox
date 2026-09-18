"""HTTP trust-boundary regressions. MIT licensed."""
import http.client
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

from flightdeck.backend import Launcher
from flightdeck.server import Handler, Server


class ServerTests(unittest.TestCase):
    def test_automatic_cloud_actions_are_bound_to_current_request_and_token(self):
        auto = self.server.launcher.cloud_saves.automation
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        for operation in ("retry", "play-local", "cancel-auto", "resolve"):
            route = "/api/cloud-saves/" + operation
            self.assertEqual(self.request("POST", route, "{}", {"Content-Type": "application/json"})[0], 403)
            with patch.object(auto, "action", return_value={"ok": True}) as action:
                self.assertEqual(self.request("POST", route,
                    '{"request_id":"current","choice":"cloud","runtime":"/foreign","url":"https://foreign.invalid"}', headers)[0], 200)
                action.assert_called_once_with(operation, "current", choice="cloud")
        self.assertEqual(self.request("POST", "/api/cloud-saves/play-local", '{"request_id":"stale"}', headers)[0], 409)
        with patch.object(auto, "launch") as launch:
            code, _, body = self.request("GET", "/api/status")
            self.assertEqual(code, 200)
            self.assertFalse(json.loads(body)["cloud"]["enabled"])
            launch.assert_not_called()

    def test_cloud_reads_are_explicit_token_protected_and_never_accept_urls(self):
        manager = self.server.launcher.cloud_saves
        with patch.object(manager, "start") as start:
            code, _, body = self.request("GET", "/api/cloud-saves")
            self.assertEqual(code, 200)
            status = json.loads(body)
            self.assertFalse(status["can_check"])
            self.assertFalse(status["sync_supported"])
            start.assert_not_called()
        for operation in ("check", "download", "cancel", "prepare-import", "import", "discard-plan", "upload", "restore"):
            self.assertEqual(self.request("POST", "/api/cloud-saves/" + operation, "{}",
                                          {"Content-Type": "application/json"})[0], 403)
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        for operation in ("check", "download", "prepare-import"):
            with patch.object(manager, "start", return_value={"ok": True}) as start:
                self.assertEqual(self.request("POST", "/api/cloud-saves/" + operation,
                                              '{"url":"https://untrusted.invalid/","runtime_path":"/foreign"}', headers)[0], 200)
                start.assert_called_once_with(operation)
        with patch.object(manager, "cancel", return_value={"ok": True}) as cancel:
            self.assertEqual(self.request("POST", "/api/cloud-saves/cancel", '{"job_id":"fixture"}', headers)[0], 200)
            cancel.assert_called_once_with("fixture")
        with patch.object(manager, "start", return_value={"ok": True}) as start:
            self.assertEqual(self.request("POST", "/api/cloud-saves/import",
                '{"plan_id":"fixture","choice":"cloud","path":"/foreign","scope":"foreign"}', headers)[0], 200)
            start.assert_called_once_with("import", plan_id="fixture", choice="cloud")
        with patch.object(manager, "discard_plan", return_value={"ok": True}) as discard:
            self.assertEqual(self.request("POST", "/api/cloud-saves/discard-plan", '{"plan_id":"fixture"}', headers)[0], 200)
            discard.assert_called_once_with("fixture")
        with patch.object(manager, "start", return_value={"ok": True}) as start:
            self.assertEqual(self.request("POST", "/api/cloud-saves/upload",
                '{"plan_id":"fixture","choice":"local","url":"https://foreign.invalid"}', headers)[0], 200)
            start.assert_called_once_with("upload", plan_id="fixture", choice="local")
        with patch.object(manager, "start", return_value={"ok": True}) as start:
            self.assertEqual(self.request("POST", "/api/cloud-saves/restore",
                '{"backup_id":"fixture","path":"/foreign"}', headers)[0], 200)
            start.assert_called_once_with("restore", backup_id="fixture")
        for operation in ("delete", "sync"):
            self.assertEqual(self.request("POST", "/api/cloud-saves/" + operation, "{}", headers)[0], 404)

    def test_update_routes_are_read_only_until_explicit_authenticated_action(self):
        from flightdeck import game_update
        code, _, body = self.request("GET", "/api/game-update", headers={"Accept-Language":"en"})
        self.assertEqual(code, 200)
        self.assertFalse(json.loads(body)["can_check"])
        self.assertIn("runtime", json.loads(body)["unavailable_reason"])
        headers = {"Content-Type":"application/json","X-Flightdeck-Token":self.server.token}
        for path in ("check", "start", "rollback", "verify", "repair/check"):
            self.assertEqual(self.request("POST", "/api/game-update/" + path, "{}", {"Content-Type":"application/json"})[0], 403)
        with patch.object(self.server.launcher.setup, "check", return_value={"ok":True}) as check:
            self.assertEqual(self.request("POST", "/api/game-update/check", '{"sign_in":true,"path":"untrusted"}', headers)[0], 200)
            check.assert_called_once_with({"mode":"update", "sign_in":True})
        with patch.object(self.server.launcher.setup, "check", return_value={"ok":True}) as check:
            self.assertEqual(self.request("POST", "/api/game-update/verify", '{"path":"untrusted","sign_in":true}', headers)[0],200)
            check.assert_called_once_with({"mode":"update","operation":"verify"})
        with patch.object(self.server.launcher.setup, "check", return_value={"ok":True}) as check:
            self.assertEqual(self.request("POST", "/api/game-update/repair/check", '{"sign_in":true,"path":"untrusted"}', headers)[0],200)
            check.assert_called_once_with({"mode":"update","operation":"repair","sign_in":True})
        self.assertEqual(self.request("POST", "/api/game-update/start", '{"check_id":"wrong"}', headers)[0], 409)
        with patch.object(game_update,"rollback",return_value={"ok":True}) as rollback:
            self.assertEqual(self.request("POST", "/api/game-update/rollback", "{}", headers)[0], 200)
            rollback.assert_called_once_with(self.server.launcher)

    def test_mod_inventory_localizes_messages_and_folder_open_is_token_protected(self):
        import subprocess
        from flightdeck import mods
        runtime = Path(self.temp.name) / "runtime"
        (runtime / "tools").mkdir(parents=True)
        (runtime / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 0\n")
        (runtime / "private").mkdir()
        community = runtime / "Community space & text"
        addon = community / "private-fixture-addon"
        addon.mkdir(parents=True)
        (addon / "manifest.json").write_text('{"title":"Private fixture addon title","package_version":"1.0"}')
        (runtime / "private/runtime.json").write_text(json.dumps({"community_path":str(community)}))
        self.server.launcher.configure(str(runtime))
        code, _, body = self.request("GET", "/api/mods", headers={"Accept-Language":"en"})
        self.assertEqual(code, 200)
        value = json.loads(body)
        self.assertEqual(value["state"], "ready")
        self.assertEqual(value["mods"][0]["name"], "Private fixture addon title")
        self.assertIn("Community folder", value["message"])
        _, _, exported = self.request("GET", "/api/diagnostics")
        self.assertNotIn(b"Private fixture addon title", exported)
        self.assertNotIn(str(community).encode(), exported)
        self.assertEqual(self.request("POST", "/api/mods/open-folder", "{}", {"Content-Type":"application/json"})[0],403)
        headers={"Content-Type":"application/json","X-Flightdeck-Token":self.server.token,"Accept-Language":"en"}
        with patch.object(mods,"_opener",return_value=["/usr/bin/xdg-open"]), patch.object(mods.subprocess,"run",return_value=subprocess.CompletedProcess([],0)) as run:
            code, _, body = self.request("POST", "/api/mods/open-folder", '{"path":"/untrusted-client-path"}', headers)
            self.assertEqual(code, 200)
            self.assertEqual(run.call_args.args[0], ["/usr/bin/xdg-open",community.as_uri()+"/"])
            self.assertIn("file manager",json.loads(body)["message"])
            self.server.launcher.setup_busy=True
            self.assertEqual(self.request("POST", "/api/mods/open-folder", "{}", headers)[0],409)
            self.assertEqual(run.call_count,1)

    def test_pause_resume_routes_are_authenticated_and_job_bound(self):
        from flightdeck.game_install import DownloadControl
        manager = self.server.launcher.setup
        self.server.launcher.reserve_setup()
        manager.job = {"id": "fixture", "mode": "install", "state": "installing", "phase": "download", "checks": []}
        manager.download_control = DownloadControl(manager._download_changed)
        manager.download_control.downloading(True)
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token, "Accept-Language": "en"}
        self.assertEqual(self.request("POST", "/api/setup/pause", '{"job_id":"fixture"}', {"Content-Type": "application/json"})[0], 403)
        self.assertEqual(self.request("POST", "/api/setup/pause", '{"job_id":"foreign"}', headers)[0], 409)
        code, _, body = self.request("POST", "/api/setup/pause", '{"job_id":"fixture"}', headers)
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["job"]["phase"], "pausing")
        self.assertIn("Pausing", json.loads(body)["job"]["message"])
        with manager.download_control.lock:
            manager.download_control._set("paused")
        code, _, body = self.request("POST", "/api/setup/resume", '{"job_id":"fixture"}', headers)
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["job"]["phase"], "download")
        self.assertFalse(json.loads(body)["job"]["can_resume"])
        self.assertTrue(self.server.launcher.setup_busy)
        self.assertEqual(self.request("POST", "/api/setup/resume", '{"job_id":"fixture"}', headers)[0], 409)
        self.server.launcher.release_setup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.server = Server(Launcher(Path(self.temp.name)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.temp.cleanup()

    def request(self, method, path, body=None, headers=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=3)
        connection.request(method, path, body=body, headers=headers or {})
        response = connection.getresponse()
        data = response.read()
        result = response.status, dict(response.getheaders()), data
        connection.close()
        return result

    def test_local_status_and_static_ui(self):
        code, headers, body = self.request("GET", "/api/status")
        self.assertEqual(code, 200)
        value = json.loads(body)
        self.assertFalse(value["runtime"]["configured"])
        self.assertEqual(value["csrf_token"], self.server.token)
        self.assertNotIn("Access-Control-Allow-Origin", headers)
        code, headers, body = self.request("GET", "/")
        self.assertEqual(code, 200)
        self.assertIn("text/html", headers["Content-Type"])
        self.assertIn("frame-ancestors 'none'", headers["Content-Security-Policy"])

    def test_dns_rebinding_and_cross_site_reads_blocked(self):
        for headers in ({"Host": "evil.example"}, {"Origin": "https://evil.example"}, {"Sec-Fetch-Site": "cross-site"}, {"Host": "localhost"}):
            self.assertEqual(self.request("GET", "/api/status", headers=headers)[0], 403)

    def test_post_requires_session_token(self):
        self.assertEqual(self.request("POST", "/api/stop", "{}", {"Content-Type": "application/json"})[0], 403)
        self.assertEqual(self.request("POST", "/api/stop", "{}", {"Content-Type": "application/json", "X-Flightdeck-Token": "wrong"})[0], 403)

    def test_desktop_refresh_is_authenticated_idle_only_and_localized(self):
        self.server.desktop_service = True
        launcher = self.server.launcher
        launcher.reserve_setup()
        path = "/api/desktop/refresh"
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        self.assertEqual(self.request("POST", path, "{}", {"Content-Type": "application/json"})[0], 403)
        self.assertFalse(self.server.update_pending)
        code, _, body = self.request("POST", path, "{}", headers)
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["refresh"], "busy")
        self.assertTrue(self.thread.is_alive())
        for language, snippet in (("de", "Launcher-Update"), ("en", "launcher update")):
            _, _, body = self.request("GET", "/api/status", headers={"Accept-Language": language})
            service = json.loads(body)["service"]
            self.assertEqual(service["release"], self.server.release_identity)
            self.assertTrue(service["update_pending"])
            self.assertIn(snippet, service["message"])
        launcher.release_setup()
        code, _, body = self.request("POST", path, "{}", headers)
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["refresh"], "restarting")
        self.thread.join(timeout=3)
        self.assertFalse(self.thread.is_alive())
        self.assertTrue(launcher.desktop_closing)

    def test_foreground_server_does_not_accept_desktop_refresh(self):
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        code, _, body = self.request("POST", "/api/desktop/refresh", "{}", headers)
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["refresh"], "unsupported")
        self.assertTrue(self.thread.is_alive())

    def test_reserved_refresh_stops_even_if_response_write_fails(self):
        self.server.desktop_service = True
        original = Handler.reply
        def fail_refresh_reply(handler, status, body, *args):
            if isinstance(body, dict) and body.get("refresh") == "restarting":
                raise BrokenPipeError("synthetic client disconnect")
            return original(handler, status, body, *args)
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        with patch.object(Handler, "reply", fail_refresh_reply):
            self.request("POST", "/api/desktop/refresh", "{}", headers)
        self.thread.join(timeout=3)
        self.assertFalse(self.thread.is_alive())
        self.assertTrue(self.server.launcher.desktop_closing)

    def test_json_bounds_and_action_errors(self):
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        for body in ("[1]", "{", "null"):
            self.assertEqual(self.request("POST", "/api/stop", body, headers)[0], 400)
        self.assertEqual(self.request("POST", "/api/stop", " " * 16385, headers)[0], 413)
        self.assertEqual(self.request("POST", "/api/stop", "{}", headers)[0], 409)
        headers["Content-Type"] = "text/plain"
        self.assertEqual(self.request("POST", "/api/stop", "{}", headers)[0], 415)

    def test_only_named_assets_served(self):
        for path in ("/../LICENSE", "/%2e%2e/LICENSE", "/flightdeck/backend.py", "/api/config", "/DESIGN.md"):
            self.assertEqual(self.request("GET", path)[0], 404)

    def test_diagnostic_export_has_no_session_token(self):
        code, _, body = self.request("GET", "/api/diagnostics")
        self.assertEqual(code, 200)
        self.assertNotIn(self.server.token.encode(), body)
        self.assertNotIn(str(Path(self.temp.name)).encode(), body)

    def test_setup_status_and_session_protected_actions(self):
        code, _, body = self.request("GET", "/api/setup")
        self.assertEqual(code, 200)
        value = json.loads(body)
        self.assertEqual(value["state"], "idle")
        self.assertIsNone(value["job"])
        self.assertEqual(value["defaults"]["mode"], "install")
        for path in ("check", "start", "cancel", "pick"):
            self.assertEqual(self.request("POST", "/api/setup/" + path, "{}", {"Content-Type": "application/json"})[0], 403)
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        for path, body in (("check", '{"mode":[]}'), ("start", '{"check_id":null}'), ("cancel", '{"job_id":[]}'), ("pick", '{"field":[]}')):
            self.assertEqual(self.request("POST", "/api/setup/" + path, body, headers)[0], 409)

    def test_setup_failed_check_is_observable(self):
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token}
        code, _, body = self.request("POST", "/api/setup/check", '{"mode":"existing","runtime_path":"/missing-synthetic-flightdeck-fixture"}', headers)
        self.assertEqual(code, 200)
        self.assertTrue(json.loads(body)["ok"])
        self.server.launcher.setup.thread.join(timeout=2)
        code, _, body = self.request("GET", "/api/setup")
        self.assertEqual(code, 200)
        value = json.loads(body)
        self.assertEqual(value["state"], "failed")
        self.assertTrue(value["job"]["error"])
        self.assertFalse(self.server.launcher.setup_busy)

    def test_accept_language_localizes_status_diagnostics_and_errors(self):
        default_code, default_headers, default_body = self.request("GET", "/api/status")
        default = json.loads(default_body)
        self.assertEqual(default_code, 200)
        self.assertEqual(default["runtime"]["checks"][0]["label"], "Startprogramm")
        for header in ("en", "en-US", "fr"):
            code, headers, body = self.request("GET", "/api/status", headers={"Accept-Language": header})
            value = json.loads(body)
            self.assertEqual(code, 200)
            self.assertEqual(value["runtime"]["checks"][0]["label"], "Launcher")
            self.assertEqual(value["runtime"]["checks"][0]["detail"], "Prepare the runtime or check the path")
            for key in ("game", "app", "support", "csrf_token"):
                self.assertEqual(value[key], default[key])
            self.assertEqual(headers["Content-Security-Policy"], default_headers["Content-Security-Policy"])
        code, _, body = self.request("GET", "/api/diagnostics", headers={"Accept-Language": "en"})
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["checks"][0]["label"], "Launcher")
        code, _, body = self.request("POST", "/api/stop", "{}", {"Accept-Language": "en", "Content-Type": "application/json"})
        self.assertEqual(code, 403)
        self.assertEqual(json.loads(body)["error"], "The session is invalid. Reload the page.")

    def test_language_switch_relocalizes_async_job_and_parallel_requests(self):
        headers = {"Content-Type": "application/json", "X-Flightdeck-Token": self.server.token, "Accept-Language": "en"}
        code, _, body = self.request("POST", "/api/setup/check", '{"mode":"existing","runtime_path":"/missing-synthetic-flightdeck-fixture"}', headers)
        self.assertEqual(code, 200)
        job_id = json.loads(body)["job"]["id"]
        self.server.launcher.setup.thread.join(timeout=2)
        def read(locale):
            status, _, body = self.request("GET", "/api/setup", headers={"Accept-Language": locale})
            return locale, status, json.loads(body)["job"]
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(read, ["en", "de"] * 4))
        for locale, status, job in results:
            self.assertEqual(status, 200)
            self.assertEqual(job["id"], job_id)
            self.assertEqual(job["state"], "failed")
            expected = "tools/play-msfs.sh is missing here. Prepare the runtime first." if locale == "en" else "Hier fehlt tools/play-msfs.sh. Bitte zuerst die Runtime vorbereiten."
            self.assertEqual(job["message"], expected)
            self.assertEqual(job["error"], expected)

    def test_discovery_lists_only_prepared_synthetic_runtime(self):
        root = Path(self.temp.name)
        folder = root / "data/flightdeck/runtimes/synthetic"
        for relative in ("tools/play-msfs.sh", "games/MSFS2024/FlightSimulator2024.exe", "local/msfs-prefix/system.reg", "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"):
            file = folder / relative
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text("synthetic")
            file.chmod(0o700)
        (folder / "private").mkdir()
        self.server.launcher.setup.source_root = root / "no-source-siblings"
        with patch("flightdeck.setup.data_home", return_value=root / "data"):
            code, _, body = self.request("GET", "/api/setup/discover", headers={"Accept-Language": "en"})
        self.assertEqual(code, 200)
        value = json.loads(body)
        self.assertEqual(len(value["runtimes"]), 1)
        self.assertEqual(value["runtimes"][0]["path"], str(folder))
        self.assertTrue(value["runtimes"][0]["ready"])
        self.assertEqual(value["runtimes"][0]["checks"][0]["label"], "Launcher")
        self.assertFalse((folder / "private/play.lock").exists())
        self.assertIsNone(self.server.launcher.runtime)


if __name__ == "__main__":
    unittest.main()
