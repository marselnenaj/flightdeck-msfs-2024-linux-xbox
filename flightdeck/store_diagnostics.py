# SPDX-License-Identifier: MIT
"""Credential-free Store event history and the native files present at launch."""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import stat

from . import __version__

COMPONENTS = {
    "cli": "bin/xodus-cli", "broker": "bin/xodus-service", "storage": "bin/flightdeck-connected-storage.exe",
    "proxy": "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll",
    "builtin": "local/store-runtime/x86_64-windows/xodus_store_test.dll",
    "prefix_builtin": "local/msfs-prefix/drive_c/windows/system32/xodus_store_test.dll",
    "unix": "local/store-runtime/x86_64-unix/xodus_store_test.so",
}
METHODS = ("XStoreQueryGameLicenseAsync", "XStoreQueryEntitledProductsAsync", "XStoreProductsQueryNextPageAsync",
           "XStoreQueryLicenseTokenAsync", "XStoreQueryProductsAsync", "XStoreQueryConsumableBalanceRemainingAsync",
           "XStoreAcquireLicenseForDurablesAsync", "XStoreQueryGameAndDlcPackageUpdatesAsync", "XStoreShowPurchaseUIAsync",
           "XStoreQueryProductForCurrentGameAsync", "XStoreCanAcquireLicenseForStoreIdAsync")
PHASES = {"prepare", "catalog", "authentication", "window_open", "bootstrap_started", "bootstrap_ready", "bootstrap_error", "window_ready", "checkout_ready", "load_timeout", "session_timeout", "expired", "error", "complete", "cancel"}
OUTCOMES = {"started", "passed", "failed", "cancelled", "expired", "timeout", "unsupported", "busy", "succeeded"}
CATALOG = {"inventory", "inventory-catalog", "inventory-mapping", "inventory-page", "catalog", "mapping", "collections", "page", "result"}
ASYNC = {"schedule", "work_enter", "cancel", "cleanup", "begin_return", "context_retain"}


def launch_record(runtime):
    """Hash actual native files, not the mutable import manifest. Never guess."""
    files = {}
    try:
        for name, relative in COMPONENTS.items():
            path = Path(runtime) / relative
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(fd, "rb") as stream:
                info = os.fstat(stream.fileno())
                if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= 128 * 1024 * 1024:
                    return None
                digest = hashlib.sha256()
                remaining = info.st_size
                while remaining:
                    block = stream.read(min(1024 * 1024, remaining))
                    if not block:
                        return None
                    digest.update(block)
                    remaining -= len(block)
                after = os.fstat(stream.fileno())
                if (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns) != (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns):
                    return None
                files[name] = digest.hexdigest()
        return {"launcher": __version__, "files": files}
    except OSError:
        return None


def _build(value):
    if (not isinstance(value, dict) or set(value) != {"launcher", "files"}
            or not isinstance(value["launcher"], str) or not re.fullmatch(r"[A-Za-z0-9.+-]{1,39}", value["launcher"])
            or not isinstance(value["files"], dict) or set(value["files"]) != set(COMPONENTS)
            or any(not isinstance(v, str) or not re.fullmatch(r"[a-f0-9]{64}", v) for v in value["files"].values())):
        return None
    return value


def bounded_log(path, head=1024 * 1024, tail=512 * 1024):
    """Non-overlapping, bounded ranges; discard incomplete lines at a gap."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode):
            raise OSError("not regular")
        first = stream.read(min(head, info.st_size))
        offset = max(len(first), info.st_size - tail)
        clipped = offset > len(first)
        stream.seek(offset)
        last = stream.read(tail)
        if clipped:
            first = first.rsplit(b"\n", 1)[0]
            last = last.split(b"\n", 1)[-1] if b"\n" in last else b""
        text = (first + (b"\n" if clipped else b"") + last).decode("utf-8", errors="replace")
        return text, clipped, info


def session(run):
    result = {"components_at_launch": None, "events": [], "partial": False, "sources": {}}
    for source in ("game", "service"):
        try:
            text, clipped, _ = bounded_log(run / (source + ".log"))
        except OSError:
            result["sources"][source] = "unavailable"
            continue
        result["sources"][source] = "partial" if clipped else "complete"
        result["partial"] |= clipped or "[flightdeck-store-events-truncated]" in text
        for number, line in enumerate(text.splitlines()):
            # Native timestamps permit comparison across the game and broker.
            # Legacy lines remain in the existing summary, never a guessed time.
            if source == "service" and line.startswith("[flightdeck-store-build] ") and len(line) < 4096:
                try:
                    build = _build(json.loads(line.split("] ", 1)[1]))
                    if build is not None:
                        result["components_at_launch"] = build
                except (ValueError, TypeError):
                    pass
            timestamp = re.search(r"\btime_ms=(\d{13})(?=\s|$)", line)
            if not timestamp or not 946684800000 <= int(timestamp[1]) < 7258118400000:
                continue
            row = {"time_ms": int(timestamp[1]), "source": source}
            match = re.search(r"\[xodus-store-query\] kind=(\d{1,2}) hr=([a-fA-F0-9]{8})\b", line)
            phase = re.search(r"\[xodus-store-async\] kind=(\d{1,2}) stage=([a-z_]{1,20}) hr=([a-fA-F0-9]{8})\b", line)
            catalog = re.search(r"\[xodus-store-catalog\] stage=([a-z-]{1,32}) hr=([a-fA-F0-9]{8})\b", line)
            event = re.search(r"\[flightdeck-store-event\] time_ms=\d{13} seq=\d{1,6} phase=([a-z_]{1,24}) outcome=([a-z]{1,16})(?=\s|$)", line)
            if match and int(match[1]) < len(METHODS):
                row.update(method=METHODS[int(match[1])], phase="result", hresult=match[2].lower())
            elif phase and int(phase[1]) < len(METHODS) and phase[2] in ASYNC:
                row.update(method=METHODS[int(phase[1])], phase=phase[2], hresult=phase[3].lower())
            elif catalog and catalog[1] in CATALOG:
                row.update(phase=catalog[1], hresult=catalog[2].lower())
            elif event and event[1] in PHASES and event[2] in OUTCOMES:
                row.update(phase=event[1], outcome=event[2])
            else:
                continue
            result["events"].append(row)
    result["events"].sort(key=lambda r: r["time_ms"])
    if len(result["events"]) > 256:
        result["events"] = result["events"][-256:]
        result["partial"] = True
    return result
