#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""One-time reuse of the unchanged, tested 0.2.6 executable after a smoke-test fix."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = "1f2f4d714d54d699144704a157904e70562bda5b"
RUN = 37697531609
ARTIFACT = 11516851227
DIGEST = "dd54a4a8c9c087325e35654f0d050835c18bdf06af47e0c764ab16a721331343"
SIZE = 115459529
ALLOW = {".github/workflows/release.yml", "scripts/release.py", "scripts/release-reuse.py",
         "scripts/check-native-package.py", "tests/compat/test_native_package_poll.py",
         "tests/compat/test_release_reuse.py"}
SPEC = importlib.util.spec_from_file_location("release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


def reusable():
    if release.version() != "0.2.6":
        return False
    if release.run(["git", "merge-base", "--is-ancestor", SOURCE, "HEAD"], check=False).returncode:
        return False
    changed = set(release.run(["git", "diff", "--name-only", "--no-renames", SOURCE, "HEAD"]).stdout.splitlines())
    return bool(changed) and changed <= ALLOW


def api(path):
    request = urllib.request.Request(f"https://api.github.com/repos/{release.REPOSITORY}/{path}",
                                     headers={"Authorization": "Bearer " + os.environ["GH_TOKEN"],
                                              "Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def validate_metadata(metadata, jobs):
    expected = {"id": ARTIFACT, "name": "release-evidence-" + SOURCE, "size_in_bytes": SIZE,
                "digest": "sha256:" + DIGEST, "expired": False}
    if any(metadata.get(key) != value for key, value in expected.items()):
        raise ValueError("Unexpected reuse artifact metadata")
    run = metadata.get("workflow_run", {})
    if any(run.get(key) != value for key, value in {
            "id": RUN, "repository_id": 1374954725, "head_repository_id": 1374954725,
            "head_branch": "main", "head_sha": SOURCE}.items()):
        raise ValueError("Artifact is not from the exact authorized source build")
    builds = [job for job in jobs["jobs"] if job["id"] == 113052882089 and job["name"] == "build"]
    required = {"Format, lint and test the locked workspace",
                "Check native HTTP contract and render both UI languages",
                "Fetch and validate pinned compatibility, graphics and corresponding sources",
                "Build and assemble the complete native package"}
    if len(builds) != 1 or not required <= {step["name"] for step in builds[0]["steps"] if step["conclusion"] == "success"}:
        raise ValueError("Original build lacks required successful validation")


def extract(archive, assets):
    if archive.stat().st_size != SIZE or release.checksum(archive) != DIGEST:
        raise ValueError("Reuse artifact bytes differ from the pinned SHA256")
    wanted = {release.PREVIOUS, "Flightdeck-Linux-x86_64.zip", "THIRD-PARTY-NOTICES.txt", "launcher-dependencies.json", "SHA256SUMS"}
    values = {}
    with zipfile.ZipFile(archive) as source:
        for item in source.infolist():
            if item.filename.startswith("release-assets/") and item.filename.removeprefix("release-assets/") in wanted:
                name = item.filename.removeprefix("release-assets/")
                if name in values or item.file_size > release.MAX_ARCHIVE:
                    raise ValueError("Unexpected reused asset")
                values[name] = source.read(item)
    if set(values) != wanted:
        raise ValueError("Reuse artifact lacks complete package assets")
    sums = values["SHA256SUMS"].decode().splitlines()
    for name in wanted - {"SHA256SUMS"}:
        expected = hashlib.sha256(values[name]).hexdigest() + "  " + name
        if sums.count(expected) != 1:
            raise ValueError("Reused package checksum mismatch")
    assets.mkdir(parents=True, exist_ok=False)
    for name, data in values.items():
        (assets / name).write_bytes(data)
    contents = release.package_contents(assets / release.PREVIOUS)
    package = release.support.read_json(contents["flightdeck-linux/FLIGHTDECK-PACKAGE.json"], "Package")
    if package.get("kind") != "rust-launcher" or package.get("version") != release.version():
        raise ValueError("Wrong reused package version")
    for name, digest in package["files"].items():
        if hashlib.sha256(contents["flightdeck-linux/" + name]).hexdigest() != digest:
            raise ValueError("Reused package manifest mismatch")
    for name in ("install.sh", "Install Flightdeck.desktop", "compat/bootstrap.lock.json", "compat/graphics.lock.json"):
        if contents["flightdeck-linux/" + name] != (ROOT / name).read_bytes():
            raise ValueError("Reused installer or compatibility lock differs")
    with zipfile.ZipFile(assets / "Flightdeck-Linux-x86_64.zip") as source:
        if len(source.infolist()) != len(contents) or set(source.namelist()) != set(contents):
            raise ValueError("Package ZIP and TAR inventory differs")
        if any(source.read(name) != data for name, data in contents.items()):
            raise ValueError("Package ZIP and TAR bytes differ")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("probe", "fetch"))
    args = parser.parse_args()
    if args.command == "probe":
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            stream.write(f"reuse={str(reusable()).lower()}\n")
        return
    if not reusable() or release.run(["git", "status", "--porcelain", "--untracked-files=no"]).stdout:
        raise ValueError("Production sources differ from the authorized build")
    validate_metadata(api(f"actions/artifacts/{ARTIFACT}"), api(f"actions/runs/{RUN}/jobs"))
    archive = ROOT / "build/reused-artifact.zip"
    with archive.open("xb") as stream:
        subprocess.run(["gh", "api", f"repos/{release.REPOSITORY}/actions/artifacts/{ARTIFACT}/zip"],
                       check=True, stdout=stream, timeout=180)
    extract(archive, ROOT / "build/release-assets")
    provenance = {"source_commit": SOURCE, "workflow_run": RUN, "artifact": ARTIFACT, "artifact_sha256": DIGEST}
    (ROOT / "build/release-build-provenance.json").write_text(json.dumps(provenance) + "\n")
    print("PASS: exact tested artifact and unchanged production sources")


if __name__ == "__main__":
    main()
