#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build deterministic Rust launcher packages from explicit, verified inputs.

Python is build tooling only. Nothing in the installed native payload uses it.
No game, Wine prefix or account directory is read by this command.
"""
import argparse
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


legacy = module("full-installer-release")


def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def notices(metadata):
    graph = legacy.read_json(legacy.read_regular(metadata, 32 * 1024 * 1024), "Cargo graph")
    locked = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    expected = {(p["name"], p["version"]): p for p in locked}
    inventory, texts, seen = [], [], set()
    for package in sorted(graph["packages"], key=lambda p: (p["name"], p["version"])):
        name, version = package["name"], package["version"]
        if (name, version) not in expected or (name, version) in seen:
            raise ValueError("Cargo metadata does not match the locked dependency graph")
        seen.add((name, version))
        if package["source"] != expected[(name, version)].get("source"):
            raise ValueError("Cargo dependency source does not match Cargo.lock")
        folder = Path(package["manifest_path"]).parent
        if package["source"] is None:
            if name != "flightdeck-linux" or folder.resolve() != ROOT:
                raise ValueError("Unexpected local package in Cargo metadata")
            paths = [ROOT / "LICENSE"]
        else:
            paths = sorted(p for p in folder.rglob("*") if p.is_file() and not p.is_symlink() and p.name.lower().startswith(("license", "licence", "copying", "notice", "copyright")))
        if package.get("license_file"):
            extra = folder / package["license_file"]
            if not extra.resolve().is_relative_to(folder.resolve()):
                raise ValueError("License path leaves its dependency")
            if extra not in paths:
                paths.append(extra)
        if not paths:
            raise ValueError("Missing dependency license text: " + name)
        included = []
        for path in paths:
            data = legacy.read_regular(path, 4 * 1024 * 1024)
            relative = str(path.relative_to(folder))
            texts.append(f"\n{'=' * 72}\n{name} {version} / {relative}\n{'=' * 72}\n" + data.decode())
            included.append({"name": relative, "sha256": digest(data)})
        inventory.append({"name": name, "version": version, "source": package["source"], "license": package["license"], "repository": package["repository"], "notices": included})
    if not seen or ("flightdeck-linux", VERSION) not in seen:
        raise ValueError("Missing Flightdeck in Cargo metadata")
    texts.append("\nManrope / SIL Open Font License\n" + (ROOT / "ui/OFL-Manrope.txt").read_text())
    header = (f"Flightdeck {VERSION} Rust launcher\n\nLauncher/UI: MIT. Dependency license texts follow.\n"
              "Bundled compatibility and graphics components retain their own licenses;\n"
              "their original notices are in resources/native and resources/graphics.\n"
              "The companion Flightdeck source archive contains the launcher sources and Cargo.lock.\n"
              "When distributing the full package, also distribute the unchanged corresponding\n"
              "Xodus/WineGDK, DXVK and VKD3D source archives listed in the release manifest.\n")
    return (header + "".join(texts)).encode(), inventory


def payload(binary, metadata, rust_notices, native=None, graphics=None):
    raw = legacy.read_regular(binary, 128 * 1024 * 1024)
    if len(raw) < 64 or raw[:6] != b"\x7fELF\x02\x01" or raw[18:20] != b"\x3e\x00":
        raise ValueError("Expected a Linux x86-64 release binary")
    reported = subprocess.run([str(binary.resolve()), "--version"], check=True, capture_output=True, timeout=10).stdout.decode().strip()
    if reported != "flightdeck " + VERSION:
        raise ValueError("Binary version differs from Cargo.toml")
    notice, inventory = notices(metadata)
    values = {"bin/flightdeck": raw, "THIRD-PARTY-NOTICES.txt": notice,
              "RUST-STANDARD-LIBRARY-NOTICES.html": legacy.read_regular(rust_notices, 16 * 1024 * 1024),
              "LICENSE": (ROOT / "LICENSE").read_bytes(), "ui/mark.svg": (ROOT / "ui/mark.svg").read_bytes()}
    for name in ("bootstrap", "graphics"):
        values[f"compat/{name}.lock.json"] = (ROOT / f"compat/{name}.lock.json").read_bytes()
    if native is not None:
        lock = json.loads(values["compat/bootstrap.lock.json"])["native"]
        archive = legacy.read_regular(native, legacy.NATIVE_ARCHIVE_MAX)
        if digest(archive) != lock["archive_sha256"]:
            raise ValueError("Compatibility archive differs from bootstrap lock")
        components = legacy.read_archive(archive, native=True)
        legacy.verify_native(components, lock)
        values.update({"resources/native/" + name: data for name, data in components.items()})
    if graphics is not None:
        inputs = {legacy.SOURCE_ROOT + "compat/graphics.lock.json": values["compat/graphics.lock.json"]}
        components = legacy.graphics_files(inputs, graphics)
        values.update({"resources/graphics/" + name.removeprefix(legacy.GRAPHICS_ROOT): data for name, data in components.items()})
    values["README.txt"] = (f"Flightdeck {VERSION}\n\nRun ./install.sh (terminal) or ./install.sh --gui.\n"
        "No Python, pip or Rust toolchain is required to install or run this package.\n"
        "The optional install dialog needs Zenity or KDialog.\n"
        "For a development service: ./bin/flightdeck --no-browser\n"
        "Uninstall: ~/.local/bin/flightdeck --uninstall\n"
        "Settings, game installations and saves remain separate from the launcher.\n").encode()
    values["FLIGHTDECK-PACKAGE.json"] = encoded({"schema": 1, "kind": "rust-launcher", "version": VERSION,
        "files": {name: digest(data) for name, data in sorted(values.items())}})
    values["install.sh"] = (ROOT / "install.sh").read_bytes()
    values["Install Flightdeck.desktop"] = (ROOT / "Install Flightdeck.desktop").read_bytes()
    if sum(map(len, values.values())) > 384 * 1024 * 1024 or len(values) > 4096:
        raise ValueError("Native package exceeds the installer limits")
    return values, inventory


def mode(name):
    return 0o755 if name in {"bin/flightdeck", "install.sh", "Install Flightdeck.desktop"} or name.startswith("resources/native/bin/") else 0o644


def create(args):
    if not args.launcher_only and (args.native is None or args.graphics is None):
        raise ValueError("Full packages require --native and --graphics; use --launcher-only for a development package")
    if args.launcher_only and (args.native is not None or args.graphics is not None):
        raise ValueError("Do not mix launcher-only and full-package inputs")
    values, inventory = payload(args.binary, args.cargo_metadata, args.rust_notices, args.native, args.graphics)
    args.output.mkdir(parents=True, exist_ok=False)
    base = "Flightdeck-Launcher-Linux-x86_64" if args.launcher_only else "Flightdeck-Linux-x86_64"
    archive = args.output / (base + ".tar.gz")
    with archive.open("xb") as out, gzip.GzipFile(fileobj=out, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for name, data in sorted(values.items()):
                entry = tarfile.TarInfo("flightdeck-linux/" + name)
                entry.size, entry.mode = len(data), mode(name)
                tar.addfile(entry, io.BytesIO(data))
    with zipfile.ZipFile(args.output / (base + ".zip"), "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as out:
        for name, data in sorted(values.items()):
            entry = zipfile.ZipInfo("flightdeck-linux/" + name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.create_system, entry.external_attr = 3, (0o100000 | mode(name)) << 16
            out.writestr(entry, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    (args.output / "launcher-dependencies.json").write_bytes(encoded(inventory))
    (args.output / "THIRD-PARTY-NOTICES.txt").write_bytes(values["THIRD-PARTY-NOTICES.txt"])
    (args.output / "SHA256SUMS").write_text("".join(digest(p.read_bytes()) + "  " + p.name + "\n" for p in sorted(args.output.iterdir()) if p.is_file()))
    print(json.dumps({"status": "PASS", "version": VERSION, "files": len(values), "dependencies": len(inventory), "full_package": not args.launcher_only, "archive": str(archive)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "cargo-metadata", "rust-notices", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--native", type=Path)
    parser.add_argument("--graphics", type=Path)
    parser.add_argument("--launcher-only", action="store_true")
    try:
        create(parser.parse_args())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, str(error) + "\n")
