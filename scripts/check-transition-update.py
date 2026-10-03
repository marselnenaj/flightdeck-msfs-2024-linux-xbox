#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise legacy -> bridge -> native updates with real packages and local GitHub fixtures."""
import argparse
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("package_check", ROOT / "scripts/check-native-package.py")
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)

# This hook exists only in the private test directory. It replaces GitHub reads
# in the isolated Python services, including the legacy service before upgrading.
# Normal product code has no test routes, custom update URLs or bypass switches.
HOOK = '''import io,json,os,sys
from pathlib import Path
fixture=Path(os.environ["FLIGHTDECK_TRANSITION_FIXTURE"])
def local_url(url):
    values=json.loads((fixture/"offer.json").read_text())
    if url.endswith("/releases/latest"):
        return io.BytesIO(json.dumps(values["bridge"]).encode())
    if url.endswith("/releases?per_page=50"):
        return io.BytesIO(json.dumps([values["bridge"],values["native"]]).encode())
    for name in ("bridge","native"):
        if url==values[name]["assets"][0]["browser_download_url"]:
            return Path(values[name+"_archive"]).open("rb")
    raise AssertionError("Unexpected network request in isolated transition check")
if (Path.cwd()/"flightdeck/__init__.py").is_file():
    sys.path.insert(0,str(Path.cwd()))
    from flightdeck import launcher_update as updates
    updates.open_url=local_url
'''


def metadata(version, path):
    project = "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox"
    return {"draft": False, "prerelease": False, "tag_name": "v" + version, "body": "Local fixture; not published",
            "assets": [{"name": "Flightdeck-Linux-x86_64.tar.gz", "state": "uploaded", "size": path.stat().st_size,
                        "digest": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest(),
                        "browser_download_url": project + "/releases/download/v" + version + "/Flightdeck-Linux-x86_64.tar.gz"}]}


def version(path, native=False):
    with tarfile.open(path) as archive:
        if native:
            result = json.load(archive.extractfile("flightdeck-linux/FLIGHTDECK-PACKAGE.json"))["version"]
        else:
            text = archive.extractfile("flightdeck-linux/flightdeck/__init__.py").read().decode()
            result = re.search(r'^__version__\s*=\s*"([^"]+)"', text, re.M)[1]
    assert re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", result), "Use an isolated stable-version native candidate for the stable-channel test"
    return result


def main(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    legacy = args.legacy_package.resolve(strict=True)
    bridge, native = args.bridge_archive.resolve(strict=True), args.native_archive.resolve(strict=True)
    bridge_version, native_version = version(bridge), version(native, native=True)
    assert bridge_version == "0.1.22"
    (output / "offer.json").write_text(json.dumps({"bridge": metadata(bridge_version, bridge), "native": metadata(native_version, native),
        "bridge_archive": str(bridge), "native_archive": str(native)}))
    hook = output / "hook"
    hook.mkdir()
    (hook / "sitecustomize.py").write_text(HOOK)
    env = {**os.environ, "PYTHONPATH": str(hook), "FLIGHTDECK_TRANSITION_FIXTURE": str(output),
           "HTTPS_PROXY": "http://127.0.0.1:1", "HTTP_PROXY": "http://127.0.0.1:1", "NO_PROXY": "127.0.0.1,localhost"}
    env.pop("PYTHONHOME", None)
    for name in ("DATA", "STATE", "CONFIG", "CACHE"):
        env["XDG_" + name + "_HOME"] = str(output / name.lower())
    browser_bin = output / "fake-browser"
    browser_bin.mkdir()
    (browser_bin / "chromium").write_text('#!/bin/sh\nprintf opened > "$FLIGHTDECK_TEST_BROWSER_LOG"\n')
    (browser_bin / "chromium").chmod(0o700)
    env["FLIGHTDECK_TEST_BROWSER_LOG"] = str(output / "unexpected-browser")
    env["PATH"] = str(browser_bin) + os.pathsep + env.get("PATH", "")
    state, data, bin_dir = output / "state/flightdeck", output / "installed", output / "bin"
    children, states = [], [state]

    def installed():
        return json.loads((data / "installation.json").read_text())

    def current(previous=None, wanted=None):
        def ready():
            record = check.record_at(state)
            if previous and record["token"] == previous["token"]:
                return None
            if previous:
                assert record["port"] == previous["port"]
            if wanted:
                assert check.request(record, "/api/status")["app"]["version"] == wanted
            return record
        return check.wait_for(ready)

    def finished(record):
        def ready():
            status = check.request(record, "/api/launcher-update")
            assert status["job"]["state"] != "failed", status["job"]
            return status if status["job"]["state"] == "complete" else None
        return check.wait_for(ready)

    def restart(record, wanted):
        check.request(record, "/api/launcher-update/restart", {})
        return current(record, wanted)

    try:
        # The initial installation is the real old package, not the current
        # Python source with an artificially changed version string.
        clean = {k: v for k, v in env.items() if k != "PYTHONPATH"}
        subprocess.run([sys.executable, str(legacy / "scripts/install-launcher.py"), "--source", str(legacy),
            "--data-dir", str(data), "--bin-dir", str(bin_dir), "--applications-dir", str(output / "applications"),
            "--no-desktop", "--no-launch", "--language", "en"], env=clean, check=True, capture_output=True)
        state.mkdir(parents=True, exist_ok=True, mode=0o700)
        marker = state / "retained-preference.txt"
        marker.write_text("synthetic browser preference")
        initial = installed()
        release = data / "releases" / initial["current"]
        with (output / "service.log").open("wb") as log:
            child = subprocess.Popen([sys.executable, "-B", "-m", "flightdeck", "--desktop-service", "--state-dir", str(state)],
                                     cwd=release, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        children.append(child)
        record = current(wanted="0.1.21")
        hops = []
        for target in (bridge_version, native_version):
            before = installed()["current"]
            check.request(record, "/api/launcher-update/check", {})
            status = finished(record)
            assert status["latest_version"] == target and status["can_install"], status
            check.request(record, "/api/launcher-update/install", {"check_id": status["check_id"]})
            status = finished(record)
            assert status["pending_restart"] and status["can_restart"]
            assert installed()["previous"] == before
            record = restart(record, target)
            hops.append(target)
        # Crossing back to Python and then forward to native must retain the
        # native manager and a shell wrapper that no longer needs Python.
        for target in (bridge_version, native_version):
            check.request(record, "/api/launcher-update/rollback", {})
            status = finished(record)
            assert status["pending_restart"] and status["can_restart"]
            record = restart(record, target)
            hops.append(target)
        assert (bin_dir / "flightdeck").read_text().startswith("#!/bin/sh\n")
        assert marker.read_text() == "synthetic browser preference"
        assert not (output / "unexpected-browser").exists()
        check.request(record, "/api/desktop/refresh", {})

        # Exercise the downloaded bootstrap and terminal update route as well.
        # The initial command is Python; successful update must exec a shell
        # wrapper, then start the actual native desktop service.
        cli = output / "cli"
        cli.mkdir()
        for name, archive_path in (("bridge", bridge), ("native", native)):
            with tarfile.open(archive_path) as archive:
                archive.extractall(cli / name, filter="data")
        bridge_source, native_source = (cli / name / "flightdeck-linux" for name in ("bridge", "native"))
        cli_env = {k: v for k, v in env.items() if k not in {"PYTHONPATH", "FLIGHTDECK_TRANSITION_FIXTURE"}}
        for name in ("DATA", "STATE", "CONFIG", "CACHE"):
            cli_env["XDG_" + name + "_HOME"] = str(cli / name.lower())
        cli_env["FLIGHTDECK_TEST_BROWSER_LOG"] = str(cli / "browser-opened")
        cli_state, cli_data, cli_bin = cli / "state/flightdeck", cli / "installed", cli / "bin"
        states.append(cli_state)
        subprocess.run([str(bridge_source / "install.sh"), "--data-dir", str(cli_data),
            "--bin-dir", str(cli_bin), "--applications-dir", str(cli / "applications"),
            "--no-desktop", "--no-launch", "--language", "en"], cwd=cli, env=cli_env,
            check=True, capture_output=True, timeout=90)
        before = (cli_data / "installation.json").read_bytes()
        binary = native_source / "bin/flightdeck"
        original = binary.read_bytes()
        binary.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        rejected = subprocess.run([str(cli_bin / "flightdeck"), "--update", str(native_source)],
            cwd=cli, env=cli_env, capture_output=True, timeout=90)
        assert rejected.returncode != 0, "Reject the modified native executable before execution"
        assert (cli_data / "installation.json").read_bytes() == before
        assert not (cli / "browser-opened").exists()
        binary.write_bytes(original)
        subprocess.run([str(cli_bin / "flightdeck"), "--update", str(native_source)], cwd=cli,
            env=cli_env, check=True, capture_output=True, timeout=90)
        cli_record = check.wait_for(lambda: check.record_at(cli_state))
        assert check.request(cli_record, "/api/status")["app"]["version"] == native_version
        assert json.loads((cli_data / "installation.json").read_text())["previous"] == json.loads(before)["current"]
        assert (cli_bin / "flightdeck").read_text().startswith("#!/bin/sh\n")
        check.wait_for(lambda: (cli / "browser-opened").exists())
        check.request(cli_record, "/api/desktop/refresh", {})
        report = {"status": "PASS", "initial_version": "0.1.21", "hops": hops, "same_browser_origin": True,
                  "settings_preserved": True, "no_additional_browser": True, "final_wrapper_requires_python": False,
                  "terminal_update": "PASS", "tampered_native_binary_rejected": True, "bridge_bootstrap": "PASS",
                  "bridge_archive_sha256": hashlib.sha256(bridge.read_bytes()).hexdigest(),
                  "native_archive_sha256": hashlib.sha256(native.read_bytes()).hexdigest(),
                  "network": "Private GitHub fixtures; no account or game operations", "published": False}
        (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report))
    finally:
        for directory in states:
            try:
                record = check.wait_for(lambda: check.record_at(directory), timeout=2)
                check.request(record, "/api/desktop/refresh", {})
            except (OSError, ValueError, AssertionError, http.client.HTTPException):
                pass
        for child in children:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=20)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("legacy-package", "bridge-archive", "native-archive", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    main(parser.parse_args())
