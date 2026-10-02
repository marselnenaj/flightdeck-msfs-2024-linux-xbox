#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Optional real OpenXR stereo test using an isolated, disposable Wine prefix."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from flightdeck import bootstrap, renderer, vr


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--runtime-json", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, help="Also test the verified packaged graphics bundle")
    parser.add_argument("--headers", type=Path, default=Path("/usr/include/openxr"))
    parser.add_argument("--simulate", action="store_true", help="Enable Monado's simulated headset and null compositor")
    parser.add_argument("--output", type=Path, required=True, help="New directory; never an existing game runtime")
    args = parser.parse_args()
    runner = args.runner.resolve(strict=True)
    manifest = args.runtime_json.resolve(strict=True)
    if not all((runner / "files/bin" / name).is_file() for name in ("wine", "wineserver")):
        parser.error("--runner must name a Proton-style runner")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    runtime = output / "runtime"
    (runtime / "private").mkdir(parents=True, mode=0o700)
    (runtime / "local").mkdir()
    (runtime / "runner").symlink_to(runner, target_is_directory=True)
    (runtime / "private" / vr.SETTINGS).write_text('{"schema":1,"mode":"auto"}')
    include = output / "include"
    shutil.copytree(args.headers, include / "openxr")
    binary = output / "openxr-stereo.exe"
    subprocess.run(["x86_64-w64-mingw32-gcc", "-O2", "-Wall", "-Wextra", "-Werror", "-Wno-missing-field-initializers",
                    "-I" + str(include), str(ROOT / "tests/graphics/openxr-stereo.c"), "-o", str(binary),
                    "-ld3d11", "-ld3d12", "-ldxgi", "-ldxguid"], check=True)
    prefix = runtime / "local/msfs-prefix"
    bootstrap.prepare_prefix(runner, prefix)
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("WINE", "DXVK", "VKD3D", "PROTON", "NVIDIA_WINE"))}
    env.update(WINEPREFIX=str(prefix), WINEDEBUG="-all", WINEESYNC="0", WINEFSYNC="0",
               XR_RUNTIME_JSON=str(manifest), DXVK_LOG_LEVEL="info", VKD3D_DEBUG="warn")
    if args.simulate:
        env.update(SIMULATED_ENABLE="1", XRT_COMPOSITOR_NULL="1")
    cases = [("runner", output / "no-bundle")]
    if args.bundle:
        cases.append(("backport", args.bundle.resolve(strict=True)))
    results = []
    for name, bundle in cases:
        if renderer.install(runtime, bundle=bundle) != name:
            raise ValueError("The renderer bundle does not match the runner")
        prepared = vr.prepare(runtime, env)
        for key, part in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                          ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state")):
            directory = runtime / "private/xdg" / part
            directory.mkdir(parents=True, exist_ok=True)
            prepared[key] = str(directory)
        for api in ("d3d11", "d3d12"):
            path = output / (name + "-" + api + ".log")
            try:
                with path.open("w") as log:
                    run = subprocess.run([str(runner / "files/bin/wine"), str(binary), api], cwd=output,
                                         env=prepared, stdout=log, stderr=subprocess.STDOUT, timeout=60)
                passed = run.returncode == 0 and "PASS:" in path.read_text(errors="replace")
                results.append({"renderer": name, "api": api, "passed": passed, "exit_code": run.returncode})
            finally:
                for option in ("-k", "-w"):
                    subprocess.run([str(runner / "files/bin/wineserver"), option], env=prepared,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
    report = {"scope": "Wine OpenXR stereo swapchains, tracking and frame submission",
              "simulated": args.simulate, "physical_headset_validated": False, "cases": results}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return int(not all(row["passed"] for row in results))


if __name__ == "__main__":
    raise SystemExit(main())
