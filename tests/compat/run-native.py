#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compile and run account-free GameSave, Store and catalog tests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]


def run(args):
    os.umask(0o077)
    runtime = args.stage.resolve() / "runtime"
    sources = runtime / "src"
    work = Path(tempfile.mkdtemp(prefix="compat-tests-", dir=args.stage.resolve()))
    wine = args.wine.resolve(strict=True)
    report = {"format": 1, "suite": args.suite, "synthetic_only": True,
              "account_calls": False, "cases": {}}
    names = []
    if args.suite in ("gamesave", "all"):
        names.extend(("core", "bridge", "async", "save-interchange"))
    if args.suite in ("store", "all"):
        names.extend(("explicit-products", "durable-license", "package-updates"))
    if args.suite in ("store", "catalog", "all"):
        names.extend(("catalog", "catalog-batch", "catalog-coins"))
    queue_sources = ("XAsync.cpp", "XTaskQueue.cpp", "ThreadPool.cpp", "WaitTimer.cpp")
    for name in names:
        case = work / name; case.mkdir(); data = case / "synthetic-saves"; data.mkdir()
        if name in ("explicit-products", "durable-license", "package-updates"):
            selected = [sources / part for part in
                        ("StoreQueries.cpp", "StoreContext.cpp", "StoreDurableLicense.cpp", *queue_sources)]
        elif name.startswith("catalog"):
            selected = [sources / "StoreCatalog.cpp"]
            if name != "catalog": selected.append(sources / "StoreCatalogBatch.cpp")
            if name == "catalog-coins":
                selected += [sources / part for part in
                             ("StoreCatalogCoinMapper.cpp", "StoreCatalogProvider.cpp")]
        else:
            selected = [sources / "GameSaveLocalCore.cpp"]
            if name not in ("core", "save-interchange"): selected.append(sources / "GameSaveBridge.cpp")
            if name == "async":
                selected += [sources / part for part in ("GameSaveAsync.cpp", *queue_sources)]
        selected.append(REPO / "tests/compat" / (name + "-test.cpp"))
        binary = case / (name + "-test.exe")
        command = ["x86_64-w64-mingw32-g++", "-std=c++17", "-O2", "-static",
                   "-I", str(runtime / "include"), "-I", str(sources)]
        if name in ("core", "bridge", "async"): command.append("-DXODUS_GAMESAVE_TESTING")
        if name == "durable-license": command.append("-DSTORE_DURABLES_TESTING")
        if name in ("core", "async", "save-interchange"): command.append("-municode")
        libraries = ["-lwinhttp"] if name.startswith("catalog") else ["-lbcrypt", "-lole32", "-luuid"]
        subprocess.run(command + [str(p) for p in selected] +
                       ["-o", str(binary), *libraries], check=True)
        # The batch/provider tests reuse the embedded synthetic catalog fixture.
        # Include it in provenance without compiling a second entry point.
        hashed_sources = list(selected)
        if name in ("catalog-batch", "catalog-coins"):
            hashed_sources.append(REPO / "tests/compat/catalog-test.cpp")
        if name == "save-interchange":
            sys.path.insert(0, str(REPO))
            from flightdeck.save_state import State, Container, encode, decode
            hashed_sources.append(REPO / "flightdeck/save_state.py")
            namespace = data / ("a" * 64)
            namespace.mkdir()
            (namespace / "state.bin").write_bytes(encode(
                State(7, {"profile": Container("Pilot", 123, {"data": b"\0\xffB"})})))
        env = dict(os.environ)
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "WINE_DLL_FILE_MAP", "WINEDLLPATH",
                    "XODUS_USER_RUNTIME", "XODUS_USER_SOCKET_SUFFIX", "XODUS_LOCAL_GAMESAVE", "XODUS_LOCAL_GAMESAVE_ROOT",
                    "XODUS_STORE_MARKET", "XODUS_STORE_LANGUAGE"):
            env.pop(key, None)
        env.update(WINEPREFIX=str(case / "prefix"), WINEARCH="win64", WINEDEBUG="-all",
                   WINEESYNC="0", WINEFSYNC="0", WINEDLLOVERRIDES="winemenubuilder.exe,mscoree,mshtml=d")
        try:
            subprocess.run([str(wine), "wineboot", "-u"], env=env, capture_output=True, check=True, timeout=60)
            invocation = [str(wine), str(binary)]
            if name in ("core", "bridge", "async", "save-interchange"):
                invocation.append("Z:" + str(data).replace("/", "\\"))
            process = subprocess.run(invocation,
                                     env=env, capture_output=True, timeout=60)
            lines = process.stdout.decode(errors="replace").splitlines()
            passed = process.returncode == 0 and any("SUMMARY" in line or "failures=0" in line for line in lines)
            if name == "save-interchange" and passed:
                state = decode((namespace / "state.bin").read_bytes())
                passed = (state.generation == 8 and state.containers["profile"].display_name == "Native reply"
                          and state.containers["profile"].blobs == {"data": b"\0\xffB", "native_result": b"PE\0\xff"})
                lines.append("PASS native save read back by Python" if passed else "FAIL native save interchange")
            report["cases"][name] = {"exit_code": process.returncode, "passed": passed, "checks": lines,
                                    "source_sha256": {str(p.relative_to(runtime) if p.is_relative_to(runtime)
                                                         else p.relative_to(REPO)):
                                                      hashlib.sha256(p.read_bytes()).hexdigest() for p in hashed_sources},
                                    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
        finally:
            subprocess.run([str(wine.parent / "wineserver"), "-k"], env=env, capture_output=True, timeout=10)
    report["passed"] = all(case["passed"] for case in report["cases"].values())
    (work / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": report["passed"], "report": str(work / "results.json")}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--wine", type=Path, required=True)
    parser.add_argument("--suite", choices=("gamesave", "store", "catalog", "all"), default="all",
                        help="test family to run (default: all)")
    raise SystemExit(run(parser.parse_args()))
