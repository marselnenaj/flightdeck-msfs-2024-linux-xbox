#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Assemble the Python bridge package from the checked source and pinned bundles."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import zipfile


ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def create(args):
    full, export = module("full-installer-release"), module("source-release")
    version = re.search(r'^__version__ = "([0-9]+\.[0-9]+\.[0-9]+)"$',
                        (ROOT / "flightdeck/__init__.py").read_text(), re.M)[1]
    if version != "0.1.22":
        raise ValueError("This builder is specifically for the 0.1.22 Python transition release")
    args.output.mkdir(parents=True, exist_ok=False)
    archive = args.output / ("flightdeck-source-" + version + ".tar.gz")
    export.create(archive)
    sources = full.read_archive(full.read_regular(archive, full.SOURCE_ARCHIVE_MAX))
    lock, _ = full.verify_sources(sources)
    native_bytes = full.read_regular(args.native, full.NATIVE_ARCHIVE_MAX)
    if full.digest(native_bytes) != lock["archive_sha256"]:
        raise ValueError("Compatibility archive differs from the pinned source lock")
    components = full.read_archive(native_bytes, native=True)
    full.verify_native(components, lock)
    # The repository's normal bootstrap selects Rust. This transitional full
    # package uses the separately reviewed Python bootstrap until the next hop.
    sources[full.SOURCE_ROOT + "install.sh"] = sources[full.SOURCE_ROOT + "scripts/install-python.sh"]
    manifest_name = full.SOURCE_ROOT + "SOURCE-MANIFEST.json"
    manifest = {"format": 1, "status": "PASS", "files": {
        name.removeprefix(full.SOURCE_ROOT): full.digest(data)
        for name, data in sorted(sources.items()) if name != manifest_name}}
    sources[manifest_name] = (json.dumps(manifest, indent=2) + "\n").encode()
    full.verify_sources(sources)
    values = {**sources, **{full.NATIVE_ROOT + name: data for name, data in components.items()},
              **full.graphics_files(sources, args.graphics)}
    full.reject_parent_files(values)
    full.write_archive(args.output / "Flightdeck-Linux-x86_64.tar.gz", values)
    with zipfile.ZipFile(args.output / "Flightdeck-0.1.22-Linux-x86_64.zip", "x", compression=zipfile.ZIP_DEFLATED) as out:
        for name, data in sorted(values.items()):
            item = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            executable = name.removeprefix(full.NATIVE_ROOT) in full.EXECUTABLE_NATIVE if name.startswith(full.NATIVE_ROOT) else name.endswith(".desktop") or data.startswith(b"#!")
            item.create_system = 3
            item.external_attr = (0o100755 if executable else 0o100644) << 16
            out.writestr(item, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    report = {"status": "PASS", "version": version, "kind": "python-rust-transition",
              "files": len(values), "source_override": {"install.sh": "scripts/install-python.sh"},
              "published": False}
    (args.output / "package-validation.json").write_text(json.dumps(report, indent=2) + "\n")
    (args.output / "SHA256SUMS").write_text("".join(hashlib.sha256(p.read_bytes()).hexdigest() + "  " + p.name + "\n"
        for p in sorted(args.output.iterdir()) if p.is_file()))
    print(json.dumps(report))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("native", "graphics", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    try:
        create(parser.parse_args())
    except (OSError, ValueError) as error:
        parser.exit(1, str(error) + "\n")
