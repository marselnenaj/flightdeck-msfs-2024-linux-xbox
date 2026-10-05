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
        env = {**os.environ, **{"XDG_" + name + "_HOME": str(work / name.lower()) for name in ("CONFIG", "DATA", "CACHE", "STATE")}}
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
            def request(path, payload=None, headers=None, raw=None):
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                try:
                    body = json.dumps(payload).encode() if payload is not None else raw
                    supplied = {"Accept-Language": "en", **(headers or {})}
                    if body is not None:
                        supplied.setdefault("Content-Type", "application/json")
                    connection.request("POST" if body is not None else "GET", path, body=body, headers=supplied)
                    response = connection.getresponse()
                    content = response.read()
                    return response.status, {key.lower(): value for key, value in response.getheaders()}, json.loads(content) if response.getheader("Content-Type", "").startswith("application/json") else content
                finally:
                    connection.close()

            code, headers, initial = request("/api/status")
            assert code == 200 and initial["app"]["name"] == "Flightdeck"
            assert initial["runtime"]["configured"] is False and initial["game"]["can_start"] is False
            assert headers["cache-control"] == "no-store"
            token = initial["csrf_token"]
            safe = {"X-Flightdeck-Token": token}
            checks += 4
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
