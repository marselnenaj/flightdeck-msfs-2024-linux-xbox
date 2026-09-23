#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Verify pinned upstream inputs and apply the reviewed compatibility patches.

This only writes a new source staging directory. It does not run a game,
authenticate, install packages, or modify the supplied upstream repository.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

REPO = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def unpack(stream, destination, strip_root=False):
    with tarfile.open(fileobj=stream, mode="r:*") as archive:
        members = archive.getmembers()
        if strip_root:
            roots = {m.name.split("/")[0] for m in members}
            if len(roots) != 1:
                raise ValueError("Archive must contain exactly one root directory")
            root = roots.pop()
            for member in members:
                member.name = member.name.removeprefix(root).lstrip("/")
            members = [m for m in members if m.name]
        # Python's data filter rejects traversal, absolute paths and unsafe links.
        archive.extractall(destination, members=members, filter="data")


def stage(args):
    lock = json.loads((REPO / "compat/upstreams.lock.json").read_text())
    destination = args.destination.resolve()
    if destination.exists():
        raise ValueError("Destination already exists; choose a new directory")
    wine_archive = args.winegdk_archive.resolve(strict=True)
    if digest(wine_archive) != lock["winegdk"]["archive_sha256"]:
        raise ValueError("WineGDK archive checksum mismatch")
    xodus_repo = args.xodus_repo.resolve(strict=True)
    revision = lock["xodus"]["revision"]
    actual = subprocess.check_output(
        ["git", "-C", str(xodus_repo), "rev-parse", revision + "^{commit}"], text=True
    ).strip()
    if actual != revision:
        raise ValueError("Xodus revision does not match lock")
    # git archive reads the exact commit, never the caller's mutable worktree.
    xodus_archive = subprocess.check_output(["git", "-C", str(xodus_repo), "archive", revision])
    destination.mkdir(parents=True)
    wine_source, xodus_source = destination / "wine-src", destination / "xodus-src"
    wine_source.mkdir(); xodus_source.mkdir()
    with wine_archive.open("rb") as stream:
        unpack(stream, wine_source, strip_root=True)
    unpack(io.BytesIO(xodus_archive), xodus_source)
    for component, target in (("winegdk", wine_source), ("xodus", xodus_source)):
        patch = REPO / lock[component]["patch"]
        if digest(patch) != lock[component]["patch_sha256"]:
            raise ValueError(component + " patch checksum mismatch")
        # Do not inherit an enclosing checkout when staging under its build/.
        environment = dict(os.environ, GIT_CEILING_DIRECTORIES=str(target.parent))
        # The small login patch uses zero-context hunks to avoid whitespace-only
        # context lines in the shipped patch. Both the pristine upstream revision
        # and every resulting source file are checked against pinned digests.
        subprocess.run(["git", "apply", "--unidiff-zero", "--check", str(patch)], cwd=target, env=environment, check=True)
        subprocess.run(["git", "apply", "--unidiff-zero", str(patch)], cwd=target, env=environment, check=True)
    runtime = destination / "runtime"
    shutil.copytree(REPO / "compat/runtime", runtime)
    expected = json.loads((REPO / "compat/source-deltas.json").read_text())
    # The helper ships as a sixth binary; its full source/build inputs must
    # belong to the reviewed source delta, not merely an untracked worktree.
    for name in ("ConnectedStorageProtocol.h", "ConnectedStorageWrite.h", "connected-storage.cpp",
                 "connected-storage-http.cpp", "build-connected-storage.sh"):
        if "tools/" + name not in expected["runtime_source_files"]:
            raise ValueError("ConnectedStorage helper source is missing from the reviewed delta: " + name)
    for key, target in (("winegdk_patched_files", wine_source),
                        ("xodus_patched_files", xodus_source), ("runtime_source_files", runtime)):
        for relative, checksum in expected[key].items():
            if digest(target / relative) != checksum:
                raise ValueError("Staged source mismatch: " + relative)
    manifest = {"format": 1, "upstreams": lock, "files": {}}
    for component in ("wine-src/dlls/xgameruntime", "xodus-src", "runtime"):
        for path in sorted((destination / component).rglob("*")):
            if path.is_file() and not path.is_symlink():
                manifest["files"][str(path.relative_to(destination))] = digest(path)
    (destination / "source-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"status": "PASS", "files": len(manifest["files"]), "manifest": str(destination / "source-manifest.json")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--winegdk-archive", type=Path, required=True)
    parser.add_argument("--xodus-repo", type=Path, required=True)
    parser.add_argument("--destination", type=Path, default=REPO / "build/compat")
    try:
        stage(parser.parse_args())
    except (ValueError, OSError, subprocess.CalledProcessError, tarfile.TarError) as error:
        parser.exit(1, str(error) + "\n")
