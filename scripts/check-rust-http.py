#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the native service with synthetic profiles and real loopback HTTP."""
import argparse
import http.client
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    checks = 0
    with tempfile.TemporaryDirectory(prefix="flightdeck-native-http-") as temporary:
        work = Path(temporary)
        env = {key: value for key, value in os.environ.items() if not key.startswith("FLIGHTDECK_")}
        env.update(HOME=str(work), **{"XDG_" + name + "_HOME": str(work / name.lower()) for name in ("CONFIG", "DATA", "CACHE", "STATE")})
        child = subprocess.Popen([str(binary), "--state-dir", str(work / "state"), "--no-browser"],
                                 env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(child.stdout, selectors.EVENT_READ)
                assert selector.select(15), "Native service did not announce its loopback listener"
                line = child.stdout.readline().decode()
            match = re.fullmatch(r"Flightdeck: http://127\.0\.0\.1:(\d+)\n", line)
            assert match, "Unexpected service startup output"
            port = int(match[1])
            token = json.loads((work / "state/desktop-service.json").read_text())["token"]
            def request(path, payload=None, headers=None, raw=None, timeout=5):
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
                try:
                    body = json.dumps(payload).encode() if payload is not None else raw
                    supplied = {"Accept-Language": "en", **({"X-Flightdeck-Token": token} if payload is None and raw is None and headers is None else {}), **(headers or {})}
                    if body is not None:
                        supplied.setdefault("Content-Type", "application/json")
                    connection.request("POST" if body is not None else "GET", path, body=body, headers=supplied)
                    response = connection.getresponse()
                    content = response.read()
                    return response.status, {key.lower(): value for key, value in response.getheaders()}, json.loads(content) if response.getheader("Content-Type", "").startswith("application/json") else content
                finally:
                    connection.close()

            # The first status hashes the executable for the service identity.
            # Unoptimized native-GUI debug binaries exceed 400 MiB and need
            # around 16 seconds on CI. Only startup gets this larger budget;
            # the steady-state HTTP contract keeps its five-second timeout.
            code, headers, initial = request("/api/status", timeout=30)
            assert code == 200 and initial["app"]["name"] == "Flightdeck"
            assert initial["runtime"]["configured"] is False and initial["game"]["can_start"] is False
            assert headers["cache-control"] == "no-store"
            assert initial["csrf_token"] == token
            safe = {"X-Flightdeck-Token": token}
            checks += 4
            for path in ("/api/status", "/api/diagnostics", "/api/problem-reports", "/api/unknown"):
                for supplied in ({}, {"X-Flightdeck-Token": "invalid"}):
                    status, _, denied = request(path, headers=supplied)
                    assert status == 403 and "csrf_token" not in denied
                    checks += 1
            # Duplicate credentials must fail even when both values are correct.
            for method, path in (("GET", "/api/status"), ("POST", "/api/preferences")):
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                try:
                    connection.putrequest(method, path)
                    connection.putheader("X-Flightdeck-Token", token)
                    connection.putheader("X-Flightdeck-Token", token)
                    connection.putheader("Content-Type", "application/json")
                    connection.putheader("Content-Length", "2")
                    connection.endheaders(b"{}")
                    response = connection.getresponse()
                    assert response.status == 403
                    response.read()
                    checks += 1
                finally:
                    connection.close()
            for path in ("/api/cloud-saves", "/api/launcher-update", "/api/store-check", "/api/problem-reports", "/api/maintenance", "/api/proton", "/api/fenix", "/api/gsx", "/api/mods", "/api/game-update", "/api/diagnostics"):
                code, _, value = request(path)
                assert code == 200 and isinstance(value, dict), path
                checks += 1
            assert request("/api/launcher-update/install", {"check_id": "stale"}, safe)[0] == 409
            assert request("/api/launcher-update/rollback", {}, safe)[0] == 409
            assert request("/api/launcher-update/restart", {}, safe)[0] == 409
            assert request("/api/store-check/cancel", {"job_id": "stale"}, safe)[0] == 409
            assert request("/api/cloud-saves/cancel-auto", {"request_id": "stale"}, safe)[0] == 409
            assert request("/api/problem-reports/discard", {"report_id": "stale"}, safe)[0] == 409
            assert request("/api/config", headers=safe, raw=b'{"runtime_path":"a","runtime_path":"b"}')[0] == 400
            checks += 7
            for headers in ({"Host": "example.invalid"}, {"Origin": "https://example.invalid"}, {"Sec-Fetch-Site": "cross-site"}):
                assert request("/api/status", headers=headers)[0] == 403
                checks += 1
            assert request("/api/config", {"runtime_path": str(work)})[0] == 403
            assert request("/api/config", {}, {"X-Flightdeck-Token": "invalid"})[0] == 403
            assert request("/api/config", [], safe)[0] == 400
            assert request("/api/config", headers=safe, raw=b"{")[0] == 400
            assert request("/api/config", {}, {**safe, "Content-Type": "text/plain"})[0] == 415
            assert request("/api/config", headers=safe, raw=b" " * 16385)[0] == 413
            assert request("/private/config.json")[0] == 404
            assert request("/../Cargo.toml")[0] == 404
            checks += 8
            # The production service exposes API data, never executable web UI.
            for name in ("index.html", "app.js", "styles.css", "i18n.js"):
                assert request("/" + name)[0] == 404
                checks += 1
            runtime = work / "runtime with spaces"
            for relative in ("private", "tools", "games/MSFS2024", "local/msfs-prefix/drive_c/windows/system32"):
                (runtime / relative).mkdir(parents=True, mode=0o700)
            for relative in ("tools/play-msfs.sh", "games/MSFS2024/FlightSimulator2024.exe", "local/msfs-prefix/system.reg", "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"):
                (runtime / relative).write_text("synthetic fixture\n")
            (runtime / "tools/play-msfs.sh").chmod(0o700)
            assert request("/api/config", {"runtime_path": str(runtime)}, safe)[0] == 200
            configured = request("/api/status")[2]
            assert configured["runtime"]["configured"] and configured["runtime"]["ready"]
            assert configured["versions"]["msfs2024"]["path"] == str(runtime)
            # An installed edition with failed readiness checks must remain
            # selectable for repair, never be treated as a new installation.
            other = work / "msfs2020"
            for name in ("tools", "private"):
                (other / name).mkdir(parents=True, mode=0o700)
            (other / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 99\n")
            (other / "tools/play-msfs.sh").chmod(0o700)
            (other / "private/runtime.json").write_text(json.dumps({"game_id": "msfs2020"}))
            assert request("/api/config", {"runtime_path": str(other)}, safe)[0] == 200
            assert request("/api/game/select", {"game_id": "msfs2024"}, safe)[0] == 200
            assert request("/api/game/select", {"game_id": "msfs2020"}, safe)[0] == 200
            selected = request("/api/status")[2]
            assert selected["runtime"]["path"] == str(other)
            assert selected["versions"]["msfs2020"]["installed"] is True
            assert selected["runtime"]["ready"] is False
            assert selected["game"]["can_start"] is False
            assert request("/api/launch", {}, safe)[0] == 409
            assert json.loads((work / "state/config.json").read_text())["runtime_path"] == str(other)
            assert request("/api/game/select", {"game_id": "msfs2024"}, safe)[0] == 200
            checks += 10
            assert request("/api/graphics", {"runtime_path": str(work), "nvidia_mode": "auto"}, safe)[0] == 409
            assert request("/api/vr/configure", {"runtime_path": str(runtime), "mode": "off"}, safe)[0] == 200
            setup = request("/api/setup")[2]
            assert setup["prepare_available"] and setup["defaults"]["runtime_path"] == str(runtime)
            checked = request("/api/setup/check", {"mode": "existing", "runtime_path": str(runtime)}, safe)
            assert checked[0] == 200
            check_id = checked[2]["job"]["id"]
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                job = request("/api/setup")[2]["job"]
                if job["state"] != "checking":
                    break
                time.sleep(.05)
            assert job["state"] == "ready", job
            assert request("/api/config", {"runtime_path": str(runtime)}, safe)[0] == 409
            assert request("/api/setup/start", {"check_id": "stale"}, safe)[0] == 409
            assert request("/api/setup/start", {"check_id": check_id}, safe)[0] == 200
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                job = request("/api/setup")[2]["job"]
                if job["state"] != "installing":
                    break
                time.sleep(.05)
            assert job["state"] == "complete", job
            assert not request("/api/status")[2]["setup"]["busy"]
            checks += 8
            community = work / "Community"
            addon = community / "synthetic-addon"
            addon.mkdir(parents=True, mode=0o700)
            (addon / "manifest.json").write_text(json.dumps({"title": "Synthetic add-on", "package_version": "1.0"}))
            (runtime / "private/runtime.json").write_text(json.dumps({"game_id": "msfs2024", "community_path": str(community)}))
            for operation in ("preview-remove", "remove", "discard-remove"):
                assert request("/api/mods/" + operation, {})[0] == 403
                assert request("/api/mods/" + operation, {"runtime_path": str(runtime), "job_id": "stale", "addon_id": "../outside"}, safe)[0] == 409
                checks += 2
            result = request("/api/mods/preview-remove", {"runtime_path": str(runtime), "addon_id": addon.name}, safe)
            assert result[0] == 200, result
            def wait_mod(state):
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    job = request("/api/mods")[2]["job"]
                    if job["state"] == state:
                        return job
                    assert job["state"] != "failed", job
                    time.sleep(.02)
                raise AssertionError(job)
            reviewed = wait_mod("ready")
            assert addon.exists() and reviewed["entry_path"] == str(addon)
            assert request("/api/mods/remove", {"job_id": "different-review"}, safe)[0] == 409
            assert request("/api/mods/remove", {"job_id": reviewed["id"]}, safe)[0] == 200
            wait_mod("complete")
            assert not addon.exists() and community.is_dir()
            checks += 5
            saves = runtime / "private/local-saves/title"
            saves.mkdir(parents=True, mode=0o700)
            (runtime / "private/local-saves.enabled").touch()
            (saves / "state.bin").write_bytes(b"synthetic local save")
            (saves / "external").symlink_to(work / "state/config.json")
            result = request("/api/saves/backup", {}, safe)
            assert result[0] == 200 and result[2]["files"] == 1
            backup = runtime / "private/save-backups" / result[2]["backup"]["name"]
            assert (backup / "data/title/state.bin").read_bytes() == b"synthetic local save"
            assert not (backup / "data/title/external").exists()
            checks += 8
            print(f"Rust HTTP: {checks} service, origin, asset, configuration and backup checks passed.")
        finally:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                try:
                    child.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=5)
            child.stdout.close()
            child.stderr.close()
            assert child.returncode == 0, "Native service did not shut down cleanly"


if __name__ == "__main__":
    main()
