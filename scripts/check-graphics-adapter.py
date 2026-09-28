#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Optional GPU integration check. Always create a new, isolated Wine prefix."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from flightdeck import bootstrap, graphics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--output", type=Path, help="New directory for the isolated prefix and results")
    args = parser.parse_args()
    runner = args.runner.resolve(strict=True)
    if not all((runner / "files/bin" / name).is_file() for name in ("wine", "wineserver")):
        parser.error("--runner must name a Proton-style runner")
    report = graphics.probe(include_device_ids=True)
    devices = [d for d in report["devices"] if d["type"] == 2]
    if len(devices) != 1 or not graphics._device_uuid(devices[0]):
        parser.error("This regression check requires one discrete Vulkan GPU with a device UUID")
    device = devices[0]
    output = args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix="flightdeck-graphics-"))
    if args.output:
        output.mkdir(parents=True, exist_ok=False)
    runtime = output / "runtime"
    (runtime / "private").mkdir(parents=True, mode=0o700)
    (runtime / "local").mkdir()
    (runtime / "runner").symlink_to(runner, target_is_directory=True)
    prefix = runtime / "local/msfs-prefix"
    binary = output / "adapter-probe.exe"
    subprocess.run(["x86_64-w64-mingw32-gcc", "-O2", "-Wall", "-Wextra", "-Werror",
                    str(ROOT / "tests/graphics/adapter-probe.c"), "-o", str(binary),
                    "-ld3d12", "-ldxgi", "-ldxguid"], check=True)
    bootstrap.prepare_prefix(runner, prefix)
    environment = {k: v for k, v in os.environ.items()
                   if not k.startswith(("WINE", "DXVK", "VKD3D", "PROTON", "NVIDIA_WINE"))}
    environment.update(WINEPREFIX=str(prefix), WINEDEBUG="-all", WINEESYNC="0", WINEFSYNC="0",
                       DXVK_LOG_LEVEL="info", VKD3D_DEBUG="info")
    selection = {"DXVK_FILTER_DEVICE_UUID": device["device_uuid"]}
    hide = {0x10de: "WINE_HIDE_NVIDIA_GPU", 0x1002: "WINE_HIDE_AMD_GPU", 0x8086: "WINE_HIDE_INTEL_GPU"}
    cases = [("legacy-host-name", {"DXVK_FILTER_DEVICE_NAME": device["name"],
                                 "VKD3D_FILTER_DEVICE_NAME": device["name"]}, None),
             ("uuid", selection, True),
             ("invalid-uuid", {"DXVK_FILTER_DEVICE_UUID": "0" * 32}, False)]
    if device["vendor_id"] in hide:
        cases.append(("uuid-hidden-vendor", {**selection, hide[device["vendor_id"]]: "1"}, True))
    if device["vendor_id"] == 0x10de:
        for mode in ("auto", "compatibility"):
            (runtime / "private" / graphics.SETTINGS_FILE).write_text(json.dumps({"schema": 1, "nvidia_mode": mode}))
            prepared, _ = graphics.prepare(runtime, environment)
            cases.append(("flightdeck-" + mode, prepared, True))
    results = []
    for name, overrides, expected in cases:
        env = environment | overrides
        try:
            with (output / (name + ".log")).open("w") as log:
                completed = subprocess.run([str(runner / "files/bin/wine"), str(binary)],
                    env=env, cwd=output, stdout=log, stderr=subprocess.STDOUT, timeout=45)
            results.append({"case": name, "exit_code": completed.returncode,
                            "expected_success": expected, "success": completed.returncode == 0})
        finally:
            for option in ("-k", "-w"):
                subprocess.run([str(runner / "files/bin/wineserver"), option], env=env,
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
    result = {"scope": "adapter_selection_and_device_creation", "gpu": device["name"], "cases": results}
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    print("Results:", output)
    return int(any(row["expected_success"] is not None and row["success"] != row["expected_success"] for row in results))


if __name__ == "__main__":
    raise SystemExit(main())
