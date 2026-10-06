#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compare isolated launchers and identical save workloads; never start Wine/MSFS."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import random
import selectors
import signal
import statistics
import subprocess
import sys
import tempfile
import time


def summary(samples):
    ordered = sorted(samples)
    return {"samples": len(samples), "median": statistics.median(samples), "min": ordered[0],
            "p95": ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))], "max": ordered[-1]}


def request(port, path, token=None):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
    try:
        start = time.perf_counter_ns()
        connection.request("GET", path, headers={"Accept-Language": "en", **({"X-Flightdeck-Token": token} if token else {})})
        response = connection.getresponse()
        raw = response.read()
        elapsed = (time.perf_counter_ns() - start) / 1_000_000
        assert response.status == 200, path
        return elapsed, json.loads(raw)
    finally:
        connection.close()


def process_memory(pid):
    values = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in {"VmRSS", "VmHWM"}:
            values[key + "_kib"] = int(value.split()[0])
    return values


def service(command, directory, environment, samples):
    started = time.perf_counter_ns()
    child = subprocess.Popen([*command, "--state-dir", str(directory / "state"), "--no-browser"],
        cwd=directory, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    try:
        with selectors.DefaultSelector() as select:
            select.register(child.stdout, selectors.EVENT_READ)
            assert select.select(30), "Service did not start"
            line = child.stdout.readline().decode()
        import re
        match = re.search(r"http://127\.0\.0\.1:(\d+)", line)
        assert match, line
        port = int(match[1])
        record = directory / "state/desktop-service.json"
        token = json.loads(record.read_text())["token"] if record.exists() else None
        _, state = request(port, "/api/status", token)
        assert not state["runtime"]["configured"] and not state["game"]["can_start"]
        startup = (time.perf_counter_ns() - started) / 1_000_000
        timings = {}
        for path in ["/api/status", "/api/setup", "/api/cloud-saves", "/api/launcher-update"]:
            for _ in range(5):
                request(port, path, token)
            timings[path] = [request(port, path, token)[0] for _ in range(samples)]
        return {"startup_ms": startup, "memory": process_memory(child.pid), "api_ms": timings}
    finally:
        os.killpg(child.pid, signal.SIGINT)
        try:
            child.wait(timeout=30)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        child.stdout.close()
        child.stderr.close()


PYTHON_SAVE = """import hashlib,json,sys
sys.path.insert(0,sys.argv[1])
from flightdeck import save_state
raw=open(sys.argv[2],'rb').read()
state=save_state.decode(raw)
print(json.dumps({'canonical_sha256':hashlib.sha256(save_state.encode(state)).hexdigest(),
 'content_sha256':save_state.content_digest(state),'containers':len(state.containers),
 'blobs':sum(len(c.blobs) for c in state.containers.values()),
 'bytes':sum(len(b) for c in state.containers.values() for b in c.blobs.values())}))
"""


def saves(binary, python_source, path, environment, repeats):
    result = {"rust": [], "python": []}
    commands = {"rust": [str(binary), "save-check", str(path)],
                "python": [sys.executable, "-c", PYTHON_SAVE, str(python_source), str(path)]}
    expected = None
    for repeat in range(repeats + 1):
        for name in (["rust", "python"] if repeat % 2 else ["python", "rust"]):
            start = time.perf_counter_ns()
            completed = subprocess.run(commands[name], env=environment, capture_output=True, timeout=120, check=True)
            elapsed = (time.perf_counter_ns() - start) / 1_000_000
            value = json.loads(completed.stdout)
            value.pop("valid", None)
            if expected is None:
                expected = value
            assert value == expected, "Save workload results differ"
            if repeat:
                result[name].append(elapsed)
    return {"bytes": path.stat().st_size, "identical_results": True,
            "process_wall_ms": {name: summary(values) for name, values in result.items()}}


def main(args):
    binary, python_source = args.binary.resolve(strict=True), args.python_source.resolve(strict=True)
    sys.path.insert(0, str(python_source))
    from flightdeck import save_state
    with tempfile.TemporaryDirectory(prefix="flightdeck-benchmark-") as temporary:
        root = Path(temporary)
        environment = dict(os.environ)
        (root / "home").mkdir()
        environment["HOME"] = str(root / "home")
        for key in ("CONFIG", "DATA", "CACHE", "STATE"):
            environment[f"XDG_{key}_HOME"] = str(root / key.lower())
        environment["PYTHONPATH"] = str(python_source)
        environment["PYTHONUNBUFFERED"] = "1"
        direct = {"rust": [str(binary)], "python": [sys.executable, "-m", "flightdeck"]}
        commands = {"direct": direct}
        if args.rust_launcher and args.python_launcher:
            commands["installed"] = {"rust": [str(args.rust_launcher.resolve())],
                                     "python": [sys.executable, str(args.python_launcher.resolve())]}
        results = {}
        for mode, launchers in commands.items():
            runs = {name: [] for name in launchers}
            for repeat in range(args.repeats + 1):
                for name in (["rust", "python"] if repeat % 2 else ["python", "rust"]):
                    directory = root / f"{mode}-{repeat}-{name}"
                    directory.mkdir()
                    value = service(launchers[name], directory, environment, args.requests)
                    if repeat:  # An explicit discarded warm-up for each launcher.
                        runs[name].append(value)
            results[mode] = {name: {
                "startup_ms": summary([r["startup_ms"] for r in values]),
                "rss_kib": summary([r["memory"]["VmRSS_kib"] for r in values]),
                "peak_rss_kib": summary([r["memory"]["VmHWM_kib"] for r in values]),
                "api_ms": {path: summary([v for r in values for v in r["api_ms"][path]]) for path in values[0]["api_ms"]}
            } for name, values in runs.items()}
        rng = random.Random(20261003)
        workloads = {}
        for label, count, blob_size in [("small_128_kib", 32, 4096), ("many_4096_blobs", 4096, 256), ("large_32_mib", 32, 1024 * 1024)]:
            state = save_state.State(42, {"synthetic": save_state.Container("Synthetic benchmark", 1,
                {f"blob{i:05d}": rng.randbytes(blob_size) for i in range(count)})})
            path = root / (label + ".bin")
            path.write_bytes(save_state.encode(state))
            workloads[label] = saves(binary, python_source, path, environment, args.repeats)
        cpu = next((line.partition(":")[2].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), "unknown")
        output = {"schema": 1, "method": "alternating fresh processes after one discarded warm-up; warm filesystem cache; no game, account, cloud network or Wine",
            "scope": "launcher service only; no GUI, simulator FPS or flight-loading measurement", "python": sys.version.split()[0],
            "python_source_commit": args.python_commit, "rust_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "machine": {"cpu": cpu, "kernel": os.uname().release, "logical_cpus": os.cpu_count()},
            "services": results, "saves": workloads}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open("x") as stream:
            json.dump(output, stream, indent=2)
            stream.write("\n")
        print(json.dumps({"status": "PASS", "report": str(args.output), "modes": list(results), "workloads": list(workloads)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--python-source", type=Path, required=True)
    parser.add_argument("--python-commit", required=True)
    parser.add_argument("--rust-launcher", type=Path)
    parser.add_argument("--python-launcher", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--requests", type=int, default=50)
    main(parser.parse_args())
