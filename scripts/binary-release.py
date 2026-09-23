#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Package pinned native components with notices and full rebuild sources.

This tool never scans a game, prefix, home directory or account store. The stage,
Cargo-vendor directory, resolved build graph and extra pinned license texts are
explicit inputs. It never uploads, creates a tag or modifies installed files.
"""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["version"]
NATIVE_FEATURES = ("connected-storage-read-v1", "connected-storage-sync-v1")
CLI_FEATURES = ("streaming-resume-files-v1", "package-info-json-v1",
                "streaming-integrity-index-v1", "streaming-progress-v1")
NATIVE_FILES = frozenset(("bin/xodus-cli", "bin/xodus-service", "bin/flightdeck-connected-storage.exe",
                          "runtime/xgameruntime.dll", "builtin/x86_64-windows/xodus_store_test.dll",
                          "builtin/x86_64-unix/xodus_store_test.so"))
HELPER_SOURCES = tuple("runtime/tools/" + name for name in
                       ("ConnectedStorageProtocol.h", "ConnectedStorageWrite.h", "connected-storage.cpp",
                        "connected-storage-http.cpp", "build-connected-storage.sh"))


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def frozen(path, expected):
    if path.is_symlink() or not path.is_file():
        raise ValueError("Expected a regular build input")
    content = path.read_bytes()
    if hashlib.sha256(content).hexdigest() != expected:
        raise ValueError("Build input changed before packaging: " + path.name)
    return content


def license_files(folder):
    return sorted(p for p in folder.rglob("*") if p.is_file() and not p.is_symlink()
                  and any(word in p.name.lower() for word in ("license", "licence", "copyright", "copying", "notice")))


def notices(args):
    graph = json.loads(args.graph.read_text())
    supplements = json.loads((args.supplements / "manifest.json").read_text())["files"]
    sections = [f"Flightdeck {VERSION} native compatibility components\n\n"
                "The launcher is MIT; native components retain their own licenses.\n"
                "Xodus CLI/service: GPL-3.0-only. WineGDK builtin, Flightdeck proxy and ConnectedStorage helper: LGPL-2.1-or-later.\n"
                "Complete corresponding sources, Cargo dependencies and rebuild scripts are supplied in\n"
                f"flightdeck-native-sources-{VERSION}.tar.gz alongside this release.\n"
                "You may modify and replace these components, including for debugging modifications.\n"
                "The Proton runner, Linux system libraries and game are not contained in this package.\n"]
    inventory = []

    def append(label, path):
        text = path.read_text(encoding="utf-8")
        sections.append("\n" + "=" * 72 + "\n" + label + "\n" + "=" * 72 + "\n" + text)

    for path in sorted((ROOT / "compat/LICENSES").glob("*")):
        append("Flightdeck compatibility / " + path.name, path)
    append("Component provenance", ROOT / "compat/THIRD_PARTY_NOTICES.md")
    json_header = (ROOT / "compat/runtime/include/vendor/nlohmann_json.hpp").read_text()
    sections.append("\nnlohmann JSON 3.6.1\n" + json_header.split("*/", 1)[0] + "*/\n")
    for name in ("LICENSE", "libs/xml2/COPYING", "libs/c++/LICENSE.TXT", "libs/c++abi/LICENSE.TXT", "libs/compiler-rt/LICENSE.TXT"):
        append("Wine and bundled library / " + name, args.stage / "wine-src" / name)
    for path in args.toolchain_notices:
        append("Build runtime / " + path.name, path)
    for package in graph:
        # The resolved graph includes build dependencies; retaining their
        # notices is intentionally conservative. Other-target sources are
        # also vendored for the complete locked rebuild environment.
        name, version = package["name"], package["version"]
        label = name + " " + version
        folder = Path(package["manifest_path"]).parent
        files = license_files(folder) if package["source"] else [args.stage / "xodus-src/LICENSE"]
        extras = [item for item in supplements if item["package"] == name and item["version"] == version]
        for item in extras:
            path = args.supplements / item["path"]
            if not path.resolve().is_relative_to(args.supplements.resolve()) or sha(path) != item["sha256"]:
                raise ValueError("Invalid supplementary license: " + label)
            files.append(path)
        if not files:
            raise ValueError("Missing original license text: " + label)
        for path in files:
            append(label + " / " + path.name, path)
        inventory.append({"name": name, "version": version, "source": package["source"],
                          "repository": package["repository"], "license": package["license"] or
                          ("GPL-3.0-only" if package["source"] is None else "MIT (repository LICENSE)"),
                          "notices": [{"name": path.name, "sha256": sha(path)} for path in files]})
    return "\n".join(sections).encode(), inventory


def archive(output, files):
    """Stable names, timestamps and ownership; no absolute paths or links."""
    with output.open("xb") as stream, gzip.GzipFile(fileobj=stream, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as tar:
            for name, value in sorted(files.items()):
                if name.startswith("/") or ".." in Path(name).parts:
                    raise ValueError("Invalid archive member")
                entry = tarfile.TarInfo(name)
                entry.mode = 0o644
                if isinstance(value, bytes):
                    entry.size = len(value)
                    entry.mode = 0o755 if value.startswith(b"#!") or name.startswith("bin/") else 0o644
                    tar.addfile(entry, io.BytesIO(value))
                else:
                    if value.is_symlink() or not value.is_file():
                        raise ValueError("Unexpected source symlink or non-file: " + name)
                    entry.size = value.stat().st_size
                    entry.mode = 0o755 if value.stat().st_mode & 0o111 else 0o644
                    with value.open("rb") as source:
                        tar.addfile(entry, source)


def tree(root, prefix, result):
    for path in sorted(root.rglob("*")):
        if ".git" in path.relative_to(root).parts:
            raise ValueError("Do not package a mutable Git checkout")
        if path.is_symlink():
            raise ValueError("Unexpected source link: " + str(path.relative_to(root)))
        if path.is_file():
            result[prefix + "/" + str(path.relative_to(root))] = path


def create(args):
    args.output.mkdir(parents=True, exist_ok=False)
    lock = json.loads((ROOT / "compat/bootstrap.lock.json").read_text())
    manifest = json.loads((args.stage / "artifacts/manifest.json").read_text())
    features = lock["native"].get("features")
    built_features = manifest.get("features")
    if (not isinstance(built_features, list) or any(not isinstance(item, str) for item in built_features)
            or not set(NATIVE_FEATURES).issubset(built_features) or not isinstance(features, list) or any(not isinstance(item, str) for item in features)
            or not set(NATIVE_FEATURES).issubset(features)):
        raise ValueError("Native build/lock is missing a required ConnectedStorage capability")
    for cli_features in (manifest.get("cli_features"), lock["native"].get("cli_features")):
        if (not isinstance(cli_features, list) or any(not isinstance(item, str) for item in cli_features)
                or not set(CLI_FEATURES).issubset(cli_features)):
            raise ValueError("Native build/lock is missing a required CLI capability")
    if set(manifest["files"]) != NATIVE_FILES or manifest["files"] != lock["native"]["files"]:
        raise ValueError("Build artifacts differ from the selected release")
    if sha(args.stage / "source-manifest.json") != manifest["source_manifest_sha256"]:
        raise ValueError("Source manifest does not belong to this build")
    source_manifest = json.loads((args.stage / "source-manifest.json").read_text())
    if not set(HELPER_SOURCES).issubset(source_manifest["files"]):
        raise ValueError("Corresponding ConnectedStorage helper sources are missing")
    checked_sources = {name: frozen(args.stage / name, checksum)
                       for name, checksum in source_manifest["files"].items()}
    native_files = {}
    for name, checksum in manifest["files"].items():
        path = args.stage / "artifacts" / name
        native_files[name] = frozen(path, checksum)
    notice, inventory = notices(args)
    native_files["THIRD-PARTY-NOTICES.txt"] = notice
    native_files["manifest.json"] = (json.dumps({"format": 1, "files": manifest["files"]}, indent=2) + "\n").encode()
    artifact = args.output / f"flightdeck-compat-{VERSION}-linux-x86_64.tar.gz"
    archive(artifact, native_files)
    (args.output / "THIRD-PARTY-NOTICES.txt").write_bytes(notice)
    (args.output / "dependency-inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    sources = {}
    tree(args.stage / "wine-src", "wine-src", sources)
    tree(args.stage / "xodus-src", "xodus-src", sources)
    # Select only original runtime inputs from this exact build's manifest;
    # generated DLLs and later edits to the working tree are not source inputs.
    for name in checked_sources:
        if name.startswith("runtime/"):
            sources[name] = checked_sources[name]
    tree(args.vendor, "vendor", sources)
    tree(ROOT / "compat/LICENSES", "LICENSES", sources)
    tree(args.supplements, "license-supplements", sources)
    sources.update(checked_sources)
    sources["source-manifest.json"] = args.stage / "source-manifest.json"
    sources["build-compat.sh"] = ROOT / "scripts/build-compat.sh"
    config = args.vendor_config.read_text()
    config = re.sub(r'directory = "[^"]+"', 'directory = "vendor"', config)
    sources[".cargo/config.toml"] = config.encode()
    sources["THIRD-PARTY-NOTICES.txt"] = notice
    sources["dependency-inventory.json"] = args.output / "dependency-inventory.json"
    sources["BUILDING.md"] = ROOT / "BUILDING.md"
    sources["README.md"] = f"""# Flightdeck {VERSION} corresponding native sources

This archive supplies the patched WineGDK source, patched Xodus CLI/service,
Flightdeck native proxy and ConnectedStorage helper, all locked Cargo
dependency sources and original notices.
There are no game files, credentials, Wine prefixes or proprietary SDK inputs.

Install the toolchain described in BUILDING.md. From this archive's root run:

    CARGO_NET_OFFLINE=true BUILD_JOBS=8 bash ./build-compat.sh .

The root .cargo/config.toml uses the included vendor directory. Host development
packages and compilers remain external system prerequisites. The original build
used Rust 1.98, GCC/MinGW 16.2 and WIDL from this Wine source. Output is artifacts/.
The build produces six binaries, including bin/flightdeck-connected-storage.exe.
Its complete sources and build script are under runtime/tools/. It performs no
authentication or cloud requests during a build. The helper provides scoped
ConnectedStorage reads and lease-bound writes. The launcher coordinates automatic
before-play/after-exit sync and advanced manual comparison, import and upload;
the helper itself does not run an autonomous background sync service.

You can modify/rebuild/replace the LGPL libraries, proxy, helper and GPL CLI/service;
Flightdeck's advanced preparation accepts your rebuilt artifact manifest. No
signature restriction prevents modified components. Keep applicable notices.

The native executable build requires glibc 2.39 or newer, GTK 3, WebKitGTK 4.1,
OpenSSL 3 and a Linux Secret Service provider. Linux system libraries and the
separately downloaded upstream Proton runner are not distributed in this archive.
""".encode()
    source_archive = args.output / f"flightdeck-native-sources-{VERSION}.tar.gz"
    archive(source_archive, sources)
    result = {"format": 1, "native_sha256": sha(artifact), "notice_sha256": hashlib.sha256(notice).hexdigest(),
              "source_sha256": sha(source_archive), "source_files": len(sources), "dependency_packages": len(inventory),
              "artifacts": manifest["files"]}
    (args.output / "validation.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("stage", "vendor", "vendor-config", "graph", "supplements", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--toolchain-notices", type=Path, nargs="+", required=True)
    create(parser.parse_args())
