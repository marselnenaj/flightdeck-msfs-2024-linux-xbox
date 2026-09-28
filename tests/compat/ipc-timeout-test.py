#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the built Wine IPC layer with delayed, account-free broker replies."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
CASES = ("inventory-ok", "inventory-timeout", "auth", "updates", "local-timeout")


def exact(stream, size):
    data = b""
    while len(data) < size:
        part = stream.recv(size - len(data))
        if not part:
            raise EOFError()
        data += part
    return data


def respond(kind, request, scenario):
    if kind == 1:
        return request
    if kind == 5:
        if scenario == "local-timeout":
            time.sleep(6)
        return b"<StoreAccountContextResponse><Status>Available</Status><Context>" + b"a" * 64 + b"</Context></StoreAccountContextResponse>"
    if kind == 3:
        time.sleep(6)
        return b"<InvalidSyntheticMsaResponse/>"
    if kind == 15:
        time.sleep(6)
        if scenario == "inventory-timeout":
            return b"<StoreCollectionsResponse><Status>Timeout</Status></StoreCollectionsResponse>"
        now = int(time.time())
        return ("<StoreCollectionsResponse><Status>Available</Status><ContextBound>true</ContextBound>"
                "<Complete>true</Complete><DirectCoverage>true</DirectCoverage><SatisfyingCoverage>true</SatisfyingCoverage>"
                "<SharedCoverage>false</SharedCoverage><ParentControlActive>true</ParentControlActive>"
                f"<ObservedAt>{now}</ObservedAt><ExpiresAt>{now + 30}</ExpiresAt>"
                "<Items/><AbsentIds/><UnknownIds/><Continuation/></StoreCollectionsResponse>").encode()
    if kind == 17:
        time.sleep(6)
        return b"<StorePackageUpdatesResponse><Status>Current</Status></StorePackageUpdatesResponse>"
    if kind == 7:
        return ("<StoreGameLicenseResponse><Status>Available</Status><SkuStoreId>GAME1234EFGH/0001</SkuStoreId>"
                "<IsActive>true</IsActive><IsTrialOwnedByThisUser>false</IsTrialOwnedByThisUser>"
                "<IsDiscLicense>false</IsDiscLicense><IsTrial>false</IsTrial><TrialTimeRemainingInSeconds>0</TrialTimeRemainingInSeconds>"
                f"<TrialUniqueId/><ExpirationDate>{int(time.time()) + 60}</ExpirationDate><Revision>" + "b" * 64 +
                "</Revision></StoreGameLicenseResponse>").encode()
    raise AssertionError(f"Unexpected synthetic broker request: {kind}")


def run(args):
    stage, wine = args.stage.resolve(), args.wine.resolve(strict=True)
    work = Path(tempfile.mkdtemp(prefix="ipc-timeout-tests-", dir=stage))
    runtime = stage / "runtime"
    sources = [runtime / "src" / name for name in ("XAsync.cpp", "XTaskQueue.cpp", "ThreadPool.cpp", "WaitTimer.cpp", "XThreading.cpp")]
    sources.append(ROOT / "tests/compat/ipc-timeout-test.cpp")
    binary = work / "ipc-timeout-test.exe"
    subprocess.run(["x86_64-w64-mingw32-g++", "-std=c++17", "-O2", "-static", "-I", str(runtime / "include"),
                    "-I", str(runtime / "src"), "-I", str(stage / "wine-src/dlls/xgameruntime/GDKComponent/Xodus"),
                    *map(str, sources), "-o", str(binary), "-lbcrypt", "-lole32", "-luuid"], check=True)
    report = {"synthetic_only": True, "account_calls": False, "cases": {}, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "builtin_sha256": hashlib.sha256((stage / "artifacts/builtin/x86_64-windows/xodus_store_test.dll").read_bytes()).hexdigest()}
    # One isolated prefix; each scenario gets a fresh process and broker socket.
    env = dict(os.environ)
    for key in ("DISPLAY", "WAYLAND_DISPLAY", "WINE_DLL_FILE_MAP", "XODUS_LOCAL_GAMESAVE", "XODUS_LOCAL_GAMESAVE_ROOT"):
        env.pop(key, None)
    env.update(WINEPREFIX=str(work / "prefix"), WINEARCH="win64", WINEDEBUG="-all", WINEESYNC="0", WINEFSYNC="0",
               WINEDLLOVERRIDES="winemenubuilder.exe,mscoree,mshtml=d;xodus_store_test=b",
               WINEDLLPATH=str(stage / "artifacts/builtin"), XODUS_STORE_MARKET="AT", XODUS_USER_SOCKET_SUFFIX="broker.sock",
               XODUS_STORE_PACKAGE_SCOPE="FlightdeckBaseGameOnlyV1")
    try:
        subprocess.run([str(wine), "wineboot", "-u"], env=env, capture_output=True, check=True, timeout=60)
        shutil.copy2(stage / "artifacts/builtin/x86_64-windows/xodus_store_test.dll", work / "prefix/drive_c/windows/system32")
        for scenario in args.case or CASES:
            # sockaddr_un is shorter than most full build-directory paths.
            with tempfile.TemporaryDirectory(prefix="flightdeck-ipc-") as socket_dir, socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(Path(socket_dir) / "broker.sock")); listener.listen(); listener.settimeout(30)
                seen, errors = [], []
                def serve():
                    try:
                        connection, _ = listener.accept()
                        with connection:
                            connection.settimeout(60)
                            while True:
                                magic, kind, length = struct.unpack("<IHH", exact(connection, 8))
                                assert magic == 0x58445358
                                request = exact(connection, length)
                                seen.append(kind)
                                response = respond(kind, request, scenario)
                                connection.sendall(struct.pack("<IHH", magic, kind + 1, len(response)) + response)
                    except (EOFError, BrokenPipeError, ConnectionResetError):
                        pass
                    except Exception as error:
                        errors.append(type(error).__name__ + ": " + str(error))
                thread = threading.Thread(target=serve, daemon=True); thread.start()
                result = subprocess.run([str(wine), str(binary), scenario], env=dict(env, XDG_RUNTIME_DIR=socket_dir), capture_output=True, timeout=60)
                thread.join(timeout=10)
                output = result.stdout.decode(errors="replace")
                (work / (scenario + ".log")).write_bytes(result.stdout + result.stderr)
                report["cases"][scenario] = {"passed": result.returncode == 0 and "SUMMARY failures=0" in output and not errors and not thread.is_alive(),
                                              "exit_code": result.returncode, "requests": seen, "checks": output.splitlines(), "errors": errors}
                print(scenario, "PASS" if report["cases"][scenario]["passed"] else "FAIL", flush=True)
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
    parser.add_argument("--case", choices=CASES, action="append")
    raise SystemExit(run(parser.parse_args()))
