#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise real rendering in new prefixes; never launch a game or use an account."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from flightdeck import bootstrap, graphics, renderer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--visible", action="store_true", help="Keep both colored windows visible longer")
    args = parser.parse_args()
    runner, bundle, output = args.runner.resolve(strict=True), args.bundle.resolve(strict=True), args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    runtime = output / "runtime"
    (runtime / "private").mkdir(parents=True)
    (runtime / "local").mkdir()
    (runtime / "runner").symlink_to(runner, target_is_directory=True)
    prefix = runtime / "local/msfs-prefix"
    probe = graphics.probe(include_device_ids=True)
    devices = [d for d in probe["devices"] if d["type"] == 2]
    if len(devices) != 1 or not graphics._device_uuid(devices[0]):
        parser.error("Requires one discrete Vulkan GPU with a device UUID")
    gpu = devices[0]
    binary = output / "multiwindow.exe"
    subprocess.run(["x86_64-w64-mingw32-gcc", "-O2", "-Wall", "-Wextra", "-Werror",
                    str(ROOT / "tests/graphics/multiwindow.c"), "-o", str(binary),
                    "-ld3d12", "-ld3d11", "-ldxgi", "-ldxguid", "-lgdi32"], check=True)
    bootstrap.prepare_prefix(runner, prefix)
    environment = {k: v for k, v in os.environ.items()
                   if not k.startswith(("WINE", "DXVK", "VKD3D", "PROTON", "NVIDIA_WINE"))}
    environment.update(WINEPREFIX=str(prefix), WINEESYNC="0", WINEFSYNC="0", WINEDEBUG="-all",
                       DXVK_LOG_LEVEL="info", VKD3D_DEBUG="warn", DXVK_FILTER_DEVICE_UUID=gpu["device_uuid"])
    results = []
    for name, patched, mode in (("runner", False, "features"), ("backport", True, "features"),
                                ("backport-automatic", True, "auto")):
        (runtime / "private" / graphics.SETTINGS_FILE).write_text(json.dumps({"schema": 1, "nvidia_mode": mode}))
        env, _ = graphics.prepare(runtime, environment)
        # A packaged launcher can install its default bundle during prepare().
        # Restore the runner pair explicitly for the baseline comparison.
        state = renderer.install(runtime, bundle=bundle if patched else output / "no-bundle")
        if state != ("backport" if patched else "runner"):
            raise ValueError("The supplied bundle does not match the runner")
        # On non-NVIDIA hardware this also checks the explicit extension opt-out
        # but does not claim to exercise the NVIDIA driver or vendor hiding.
        if mode == "auto" and gpu["vendor_id"] != 0x10de:
            graphics._nvidia_mode(env, mode)
        command = [str(runner / "files/bin/wine"), str(binary)]
        if args.visible and name == "backport":
            command.append("--visible")
        try:
            with (output / (name + ".log")).open("w") as log:
                run = subprocess.run(command, cwd=output, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=90)
            text = (output / (name + ".log")).read_text(errors="replace")
            results.append({"case": name, "exit_code": run.returncode,
                            "passed": run.returncode == 0 and "PASS:" in text,
                            "blank_present_warning": "never been rendered to" in text})
        finally:
            for option in ("-k", "-w"):
                subprocess.run([str(runner / "files/bin/wineserver"), option], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
    report = {"scope": "Mixed D3D11/D3D12 clear/readback/present and swapchain lifetime",
              "gpu": gpu["name"], "nvidia_hardware": gpu["vendor_id"] == 0x10de, "cases": results}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return int(not all(row["passed"] for row in results))


if __name__ == "__main__":
    raise SystemExit(main())
