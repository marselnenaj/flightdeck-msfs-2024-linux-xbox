# SPDX-License-Identifier: MIT
"""Development probe transport only; all Flightdeck behavior is implemented in Rust.

Build first: cargo build --locked --bin flightdeck-rust --example runtime-lab
"""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def call(operation, **inputs):
    binary = Path(os.environ.get("FLIGHTDECK_PROBE_BINARY", ROOT / "target/debug/examples/runtime-lab"))
    if not binary.is_file():
        raise ValueError("Build the Rust probes first: cargo build --locked --bin flightdeck-rust --example runtime-lab")
    result = subprocess.run([str(binary.resolve())], input=json.dumps({"operation": operation, **inputs}, default=str),
                            capture_output=True, text=True, timeout=900)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "Rust probe failed")
    return json.loads(result.stdout)


def mark(runtime):
    """Called only just after the harness creates its new private runtime."""
    (runtime / "private/synthetic-probe").write_text("Flightdeck isolated test\n")


def isolated_environment(runtime):
    env = call("wine-env", root=runtime)
    for category in ("CONFIG", "DATA", "CACHE", "STATE"):
        path = runtime / "private/xdg" / category.lower()
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
        env["XDG_" + category + "_HOME"] = str(path)
    return env
