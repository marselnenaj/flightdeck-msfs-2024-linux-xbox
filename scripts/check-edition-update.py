#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Verify automatic inactive-edition migration using an isolated synthetic game."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--previous-package", type=Path, required=True)
    args = parser.parse_args()
    package = args.package.resolve(strict=True)
    previous = args.previous_package.resolve(strict=True)
    lock = json.loads((package / "compat/bootstrap.lock.json").read_text())
    old_native = previous / "flightdeck/resources/native"
    manifest = json.loads((old_native / "manifest.json").read_text())
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    mapping = {
        "bin/xodus-cli": ["bin/xodus-cli"],
        "bin/xodus-service": ["bin/xodus-service"],
        "bin/flightdeck-connected-storage.exe": ["bin/flightdeck-connected-storage.exe"],
        "runtime/xgameruntime.dll": ["local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"],
        "builtin/x86_64-windows/xodus_store_test.dll": ["local/store-runtime/x86_64-windows/xodus_store_test.dll", "local/msfs-prefix/drive_c/windows/system32/xodus_store_test.dll"],
        "builtin/x86_64-unix/xodus_store_test.so": ["local/store-runtime/x86_64-unix/xodus_store_test.so"],
    }
    with tempfile.TemporaryDirectory(prefix="flightdeck-edition-update-") as temporary:
        work = Path(temporary)
        older = work / "msfs2020"
        active = work / "msfs2024"
        state = work / "state"
        for root, edition, executable in ((older, "msfs2020", "FlightSimulator.exe"), (active, "msfs2024", "FlightSimulator2024.exe")):
            for name in ("private", "tools", "games/" + edition.upper(), "local/msfs-prefix/drive_c/windows/system32"):
                (root / name).mkdir(parents=True, mode=0o700)
            (root / "private/runtime.json").write_text(json.dumps({"game_id": edition}))
            (root / "tools/play-msfs.sh").write_text("#!/bin/sh\nexit 99\n")
            (root / "tools/play-msfs.sh").chmod(0o700)
            (root / "games" / edition.upper() / executable).write_text("synthetic game; never executed\n")
            (root / "local/msfs-prefix/system.reg").write_text("synthetic registry\n")
            (root / "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll").write_text("synthetic unmanaged component\n")
        old_scripts = {}
        for name in lock["runtime_scripts"]["files"]:
            source = previous / "scripts/runtime" / name
            old_scripts[name] = digest(source)
            shutil.copyfile(source, older / "tools" / name)
            (older / "tools" / name).chmod(0o700)
        assert old_scripts in lock["runtime_scripts"]["upgrade_from"], "previous scripts must be a supported migration"
        for name, destinations in mapping.items():
            assert digest(old_native / name) == manifest["files"][name]
            for destination in destinations:
                target = older / destination
                target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                shutil.copyfile(old_native / name, target)
                target.chmod(0o700)
        (older / "private/import-manifest.json").write_text(json.dumps({"format": 1, "artifacts": manifest, "runtime_files": old_scripts}))
        (older / "private/synthetic-save.bin").write_bytes(b"keep synthetic save")
        state.mkdir(mode=0o700)
        (state / "config.json").write_text(json.dumps({"schema": 1, "runtime_path": str(active), "runtimes": {"msfs2020": str(older), "msfs2024": str(active)}}))
        env = dict(os.environ, HOME=str(work))
        for name in ("CONFIG", "DATA", "CACHE", "STATE"):
            env["XDG_" + name + "_HOME"] = str(work / name.lower())
        with (work / "service.log").open("wb") as log:
            child = subprocess.Popen([str(package / "bin/flightdeck"), "--state-dir", str(state), "--no-browser"], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=log)
            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(child.stdout, selectors.EVENT_READ)
                    assert selector.select(20), "service startup"
                    port = int(child.stdout.readline().decode().strip().rsplit(":", 1)[1])

                def request(path, payload=None, token=None):
                    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
                    try:
                        headers = {"Content-Type": "application/json"}
                        if token:
                            headers["X-Flightdeck-Token"] = token
                        connection.request("GET" if payload is None else "POST", "/api/" + path, None if payload is None else json.dumps(payload), headers)
                        response = connection.getresponse()
                        value = json.loads(response.read())
                        assert response.status == 200, value
                        return value
                    finally:
                        connection.close()

                before = request("status")
                assert before["runtime"]["game_id"] == "msfs2024"
                assert before["versions"]["msfs2020"]["installed"] is True
                assert before["versions"]["msfs2020"]["ready"] is False
                started = time.monotonic()
                request("game/select", {"game_id": "msfs2020"}, before["csrf_token"])
                elapsed = time.monotonic() - started
                after = request("status")
                assert after["runtime"]["game_id"] == "msfs2020"
                assert after["runtime"]["ready"] is True, after["runtime"]["checks"]
                for name, expected in lock["runtime_scripts"]["files"].items():
                    assert digest(older / "tools" / name) == expected
                for name, destinations in mapping.items():
                    assert all(digest(older / target) == lock["native"]["files"][name] for target in destinations)
                assert digest(older / "tools/flightdeck-helper") == digest(package / "bin/flightdeck")
                assert (older / "private/synthetic-save.bin").read_bytes() == b"keep synthetic save"
                assert not (older / "private/component-update.json").exists()
                request("game/select", {"game_id": "msfs2024"}, before["csrf_token"])
                request("game/select", {"game_id": "msfs2020"}, before["csrf_token"])
                assert request("status")["runtime"]["ready"] is True
                print(json.dumps({"status": "PASS", "select_and_migrate_seconds": elapsed, "migration": "inactive MSFS 2020 becomes ready on selection", "game_or_account_calls": False}))
            finally:
                child.send_signal(signal.SIGINT)
                child.wait(timeout=10)


if __name__ == "__main__":
    main()
