#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run a packaged launcher on a pinned distribution, without host desktop access.

Container tests establish installation, HTTP and software desktop startup only.
They do not establish GPU, Microsoft sign-in, Wine or simulator compatibility.
"""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def run(command, **options):
    return subprocess.run(command, check=True, capture_output=True, text=True,
                          timeout=options.pop("timeout", 60), **options).stdout


def wait_for(check, children, timeout=75):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        assert all(child.poll() is None for child in children), "A desktop process exited before readiness"
        value = check()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError("Desktop readiness timed out")


def stop(child):
    if child.poll() is None:
        child.send_signal(signal.SIGTERM)
    try:
        child.wait(timeout=10)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=5)


def wayland_painted(text):
    surfaces, toplevels, titles = {}, {}, {}
    configured, buffered = set(), set()
    # Decorations use separate wl_surfaces and can paint before the content.
    # Follow the titled toplevel's role chain to its actual main surface.
    for line in text.splitlines():
        if event := re.search(r"\.get_xdg_surface\(new id xdg_surface[@#](\d+), wl_surface[@#](\d+)\)", line):
            surfaces[event[1]] = event[2]
            configured.discard(event[1])
            buffered.discard(event[2])
        elif event := re.search(r"xdg_surface[@#](\d+)\.get_toplevel\(new id xdg_toplevel[@#](\d+)\)", line):
            toplevels[event[2]] = event[1]
            titles.pop(event[2], None)
        elif event := re.search(r'xdg_toplevel[@#](\d+)\.set_title\("([^"\n]*)"\)', line):
            titles[event[1]] = event[2]
        elif event := re.search(r"xdg_surface[@#](\d+)\.ack_configure\(", line):
            configured.add(event[1])
        elif event := re.search(r"wl_surface[@#](\d+)\.attach\(([^,]*)", line):
            if re.fullmatch(r"wl_buffer[@#]\d+", event[2]):
                buffered.add(event[1])
            else:
                buffered.discard(event[1])
        elif event := re.search(r"wl_surface[@#](\d+)\.commit\(", line):
            if event[1] in buffered and any(
                title == "Flightdeck" and toplevels.get(top) in configured
                and surfaces.get(toplevels.get(top)) == event[1]
                for top, title in titles.items()
            ):
                return True
    return False


def window(binary, backend, work, output):
    environment = dict(os.environ)
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "WAYLAND_DEBUG"):
        environment.pop(key, None)
    runtime = work / (backend + "-runtime")
    runtime.mkdir(mode=0o700)
    environment["XDG_RUNTIME_DIR"] = str(runtime)
    if backend == "x11":
        environment["DISPLAY"] = ":99"
        server_command = ["Xvfb", ":99", "-screen", "0", "1280x900x24", "-nolisten", "tcp", "-noreset"]
        ready = lambda: Path("/tmp/.X11-unix/X99").exists()
    else:
        environment["WAYLAND_DISPLAY"] = "flightdeck-test"
        help_text = run(["weston", "--help"], env=environment)
        renderer = "--renderer=pixman" if "--renderer" in help_text else "--use-pixman"
        server_command = ["weston", "--backend=headless-backend.so", renderer,
                          "--socket=flightdeck-test", "--idle-time=0", "--no-config"]
        ready = lambda: (runtime / "flightdeck-test").is_socket()
    children = []
    state = work / (backend + "-state")
    with (output / (backend + "-server.log")).open("w") as server_log, \
            (output / (backend + "-window.log")).open("w") as window_log:
        try:
            server = subprocess.Popen(server_command, env=environment, stdout=server_log, stderr=server_log)
            children.append(server)
            wait_for(ready, children)
            if backend == "wayland":
                environment["WAYLAND_DEBUG"] = "client"
            # --desktop creates only an isolated, unconfigured service. Outbound
            # networking is disabled by the container; there is no host display.
            gui = subprocess.Popen([str(binary), "--desktop", "--state-dir", str(state), "--language", "en"],
                                   env=environment, stdout=window_log, stderr=window_log)
            children.append(gui)
            if backend == "x11":
                def painted():
                    text = run(["xwininfo", "-root", "-tree"], env=environment, timeout=5)
                    (output / "x11-windows.txt").write_text(text)
                    return '"Flightdeck"' in text
            else:
                def painted():
                    text = (output / "wayland-window.log").read_text()
                    return wayland_painted(text)
            wait_for(painted, children)
            record = json.loads((state / "desktop-service.json").read_text())
            connection = http.client.HTTPConnection("127.0.0.1", record["port"], timeout=10)
            try:
                connection.request("GET", "/api/status", headers={"X-Flightdeck-Token": record["token"]})
                response = connection.getresponse()
                status = json.loads(response.read())
                assert response.status == 200 and status["runtime"]["configured"] is False
            finally:
                connection.close()
        finally:
            # Retain startup evidence without credentials, even on failed startup.
            record_path = state / "desktop-service.json"
            try:
                record = json.loads(record_path.read_text())
            except (OSError, ValueError):
                record = None
            try:
                if isinstance(record, dict):
                    (output / (backend + "-service.json")).write_text(json.dumps(
                        {key: record.get(key) for key in ("pid", "port", "release")}, indent=2) + "\n")
            finally:
                for child in reversed(children):
                    stop(child)
                # The service deliberately outlives the window. This PID belongs
                # to this test's private container/state, never a host session.
                if isinstance(record, dict) and type(record.get("pid")) is int and record["pid"] > 1:
                    try:
                        os.kill(record["pid"], signal.SIGINT)
                    except ProcessLookupError:
                        pass


def inside(package, output):
    assert os.geteuid() != 0, "The launcher must be tested as an unprivileged user"
    binary = package / "bin/flightdeck"
    manifest = json.loads((package / "FLIGHTDECK-PACKAGE.json").read_text())
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    assert digest == manifest["files"]["bin/flightdeck"]
    assert run([str(binary), "--version"]).strip() == "flightdeck " + manifest["version"]
    with tempfile.TemporaryDirectory(prefix="flightdeck-linux-") as temporary:
        work = Path(temporary)
        root, bin_dir = work / "installed launcher ä", work / "commands"
        run(["bash", str(package / "install.sh"), "--data-dir", str(root), "--bin-dir", str(bin_dir),
             "--applications-dir", str(work / "applications"), "--no-desktop", "--no-launch", "--language", "en"])
        installed = bin_dir / "flightdeck"
        assert run([str(installed), "--version"]).strip() == "flightdeck " + manifest["version"]
        http = run(["python3", "/checks/check-rust-http.py", "--binary", str(installed)], timeout=120)
        (output / "http.log").write_text(http)
        for backend in ("x11", "wayland"):
            window(installed, backend, work, output)
        run([str(installed), "install", "--uninstall", "--data-dir", str(root), "--no-launch", "--language", "en"])
        assert not installed.exists(), "Uninstall retained the managed command"
    report = {"status": "PASS", "version": manifest["version"], "binary_sha256": digest,
              "system": Path("/etc/os-release").read_text(),
              "glibc": run(["getconf", "GNU_LIBC_VERSION"]).strip(),
              "checks": ["native_version", "install_unicode_path", "installed_command", "http_contract",
                         "x11_window", "wayland_buffer_commit", "uninstall"],
              "scope": "unprivileged, isolated container; no GPU, login, Wine or simulator tests"}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


def outside(args):
    matrix = json.loads((ROOT / "compat/linux-test-images.json").read_text())
    spec = matrix[args.distribution]
    assert re.fullmatch(r"[a-z0-9./:_-]+@sha256:[0-9a-f]{64}", spec["image"])
    package = args.package.resolve(strict=True)
    assert (package / "FLIGHTDECK-PACKAGE.json").is_file()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    dockerfile = (f"FROM {spec['image']}\nRUN {spec['install']}\n"
                  "RUN mkdir -p /checks /results /package\n")
    tag = "flightdeck-linux-check:" + args.distribution
    with (output / "build.log").open("w") as build_log:
        subprocess.run(["docker", "build", "--tag", tag, "-"], input=dockerfile, text=True,
                       stdout=build_log, stderr=subprocess.STDOUT, check=True, timeout=1200)
    image = run(["docker", "image", "inspect", "--format", "{{.Id}}", tag]).strip()
    (output / "image.json").write_text(json.dumps({**spec, "test_image": image}, indent=2) + "\n")
    # No sockets, host PID namespace, host networking, devices, credentials or
    # writable source mounts. Package and the two reviewed check scripts are RO.
    container_name = "flightdeck-linux-check-" + uuid.uuid4().hex
    command = ["docker", "run", "--name", container_name, "--rm", "--init", "--network=none", "--read-only", "--cap-drop=ALL",
               "--security-opt=no-new-privileges", "--user", f"{os.getuid()}:{os.getgid()}",
               "--tmpfs", "/tmp:rw,exec,mode=1777,size=768m", "--shm-size=128m",
               "--env", "HOME=/tmp", "--env", "XDG_CONFIG_HOME=/tmp/config", "--env", "XDG_DATA_HOME=/tmp/data",
               "--env", "XDG_STATE_HOME=/tmp/state", "--env", "XDG_CACHE_HOME=/tmp/cache",
               "--mount", f"type=bind,source={package},target=/package,readonly",
               "--mount", f"type=bind,source={output},target=/results"]
    for name in ("check-linux-distro.py", "check-rust-http.py"):
        command += ["--mount", f"type=bind,source={ROOT / 'scripts' / name},target=/checks/{name},readonly"]
    try:
        subprocess.run(command + [image, "python3", "/checks/check-linux-distro.py", "--inside",
                                 "--package", "/package", "--output", "/results"], check=True, timeout=420)
    finally:
        # A docker-client timeout does not stop the daemon-owned container.
        subprocess.run(["docker", "rm", "--force", container_name], capture_output=True, timeout=30)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--distribution", choices=("ubuntu", "debian", "fedora", "arch", "opensuse"))
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside:
        inside(args.package, args.output)
    else:
        if not args.distribution:
            parser.error("--distribution is required outside the test container")
        outside(args)
