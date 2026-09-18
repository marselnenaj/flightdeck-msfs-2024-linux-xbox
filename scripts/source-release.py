#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Create an inspected sources-only archive; never upload or publish it."""
import argparse
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("source_check", ROOT / "scripts/check-source-export.py")
check = importlib.util.module_from_spec(spec); spec.loader.exec_module(check)


def create(output):
    manifest = check.inspect()
    output = output.resolve()
    if output.exists():
        raise ValueError("Output exists; choose a new source archive path")
    if not output.is_relative_to(ROOT / "build"):
        raise ValueError("Source archives must be written under the ignored build directory")
    timestamp = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    # Freeze and verify every source before creating any archive output. Only
    # these checked bytes are written, even if a source changes afterwards.
    contents = {}
    for name, expected in manifest["files"].items():
        data = (ROOT / name).read_bytes()
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError("Source changed after inspection: " + name)
        contents[name] = data
    contents["SOURCE-MANIFEST.json"] = (json.dumps(manifest, indent=2) + "\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as stream, gzip.GzipFile(fileobj=stream, mode="wb", mtime=timestamp, filename="") as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as archive:
            for name, data in sorted(contents.items()):
                entry = tarfile.TarInfo("flightdeck-linux/" + name)
                entry.size = len(data); entry.mtime = timestamp
                entry.mode = 0o755 if data.startswith(b"#!") or name == "Install Flightdeck.desktop" else 0o644
                archive.addfile(entry, io.BytesIO(data))
    print(json.dumps({"status": "PASS", "source_files": len(manifest["files"]), "archive": str(output)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    try:
        create(parser.parse_args().output)
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + "\n")
