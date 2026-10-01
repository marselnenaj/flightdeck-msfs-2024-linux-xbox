#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Reproduce the maintenance9 3D-copy layout bug in an isolated Wine prefix."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from flightdeck import bootstrap, graphics, renderer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True, help="Previously released graphics bundle")
    parser.add_argument("--bundle", type=Path, required=True, help="Bundle containing the layout fix")
    parser.add_argument("--output", type=Path, required=True, help="New directory for the private test prefix and results")
    parser.add_argument("--validation-layer-path", type=Path, help="Directory containing the Khronos validation layer JSON")
    args = parser.parse_args()
    runner = args.runner.resolve(strict=True)
    baseline, bundle = args.baseline.resolve(strict=True), args.bundle.resolve(strict=True)
    layer_path = args.validation_layer_path.resolve(strict=True) if args.validation_layer_path else None
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    runtime = output / "runtime"
    (runtime / "private").mkdir(parents=True)
    (runtime / "local").mkdir()
    (runtime / "runner").symlink_to(runner, target_is_directory=True)
    prefix = runtime / "local/msfs-prefix"
    devices = [d for d in graphics.probe(include_device_ids=True)["devices"] if d["type"] == 2]
    if len(devices) != 1 or not graphics._device_uuid(devices[0]):
        parser.error("Requires one discrete Vulkan GPU with a device UUID")
    gpu = devices[0]
    source = ROOT / "tests/graphics/volume-copy.c"
    binary = output / "volume-copy.exe"
    subprocess.run(["x86_64-w64-mingw32-gcc", "-O2", "-Wall", "-Wextra", "-Werror",
                    str(source), "-o", str(binary), "-ld3d12", "-ldxgi", "-ldxguid"], check=True)
    bootstrap.prepare_prefix(runner, prefix)
    environment = {k: v for k, v in os.environ.items()
                   if not k.startswith(("WINE", "DXVK", "VKD3D", "PROTON", "NVIDIA_WINE"))}
    environment.update(WINEPREFIX=str(prefix), WINEESYNC="0", WINEFSYNC="0", WINEDEBUG="-all",
                       WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3d11,d3d10core=n",
                       DXVK_FILTER_DEVICE_UUID=gpu["device_uuid"], DXVK_LOG_LEVEL="info",
                       VKD3D_DEBUG="warn", VKD3D_CONFIG="vk_debug", VKD3D_SHADER_CACHE_PATH="0",
                       VK_INSTANCE_LAYERS="VK_LAYER_KHRONOS_validation")
    if layer_path:
        environment["VK_LAYER_PATH"] = str(layer_path)
    results = []
    for name, payload, disable in (("released", baseline, False), ("fixed", bundle, False),
                                   ("released-without-maintenance9", baseline, True),
                                   ("fixed-without-maintenance9", bundle, True)):
        if renderer.install(runtime, bundle=payload) != "backport":
            raise ValueError("Bundle does not match the runner")
        env = dict(environment)
        if disable:
            env["VKD3D_DISABLE_EXTENSIONS"] = "VK_KHR_maintenance9"
        log_path = output / (name + ".log")
        try:
            with log_path.open("w") as log:
                run = subprocess.run([str(runner / "files/bin/wine"), str(binary)], cwd=output,
                                     env=env, stdout=log, stderr=subprocess.STDOUT, timeout=60)
            text = log_path.read_text(errors="replace")
            errors = Counter(re.findall(
                r"err:vkd3d-proton:vkd3d_debug_messenger_callback: ((?:VUID-|UNASSIGNED-)[^:\s]+)", text))
            # Pixel readback can succeed despite invalid Vulkan layouts. Require
            # the layer's own active check and evaluate its errors separately.
            results.append({"case": name, "exit_code": run.returncode,
                            "pixels_correct": "VOLUME_COPY: pixels=2048 mismatches=0" in text,
                            "validation_active": "Validation layers seem to be loaded correctly." in text,
                            "validation_errors": dict(errors),
                            "volume_layout_error": bool(errors) and bool(re.search(
                                r"(?:layer [1-3], mip 0|mipLevel = 0, arrayLayer = [1-3])", text)),
                            "renderer_sha256": {n: graphics._digest(payload / n) for n in renderer.FILES}})
        finally:
            for option in ("-k", "-w"):
                subprocess.run([str(runner / "files/bin/wineserver"), option], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
    valid = all(r["exit_code"] == 0 and r["pixels_correct"] and r["validation_active"] for r in results)
    reproduced = (results[0]["volume_layout_error"] and
                  "VUID-VkCopyImageInfo2-srcImageLayout-00128" in results[0]["validation_errors"])
    fixed = all(not r["validation_errors"] for r in results[1:])
    status = "PASS" if valid and reproduced and fixed else "FAIL"
    if valid and not reproduced and all(not r["validation_errors"] for r in results):
        status = "INCONCLUSIVE"  # Unsupported/disabled maintenance9 must not look like a verified fix.
    report = {"status": status, "scope": "3D texture upload/copy/readback with Vulkan layout validation",
              "gpu": gpu["name"], "nvidia_hardware": gpu["vendor_id"] == 0x10de,
              "probe_sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "cases": results}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return {"PASS": 0, "FAIL": 1, "INCONCLUSIVE": 2}[status]


if __name__ == "__main__":
    raise SystemExit(main())
