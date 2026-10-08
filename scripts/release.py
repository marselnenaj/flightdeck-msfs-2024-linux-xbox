#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Prepare pinned full-release inputs, or publish a verified Actions build.

Only the explicit `publish` command writes to GitHub. It requires the exact
main-branch release commit in GitHub Actions and never replaces a tag or asset.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tarfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "marselnenaj/flightdeck-msfs-2024-linux-xbox"
RELEASE_BASE = f"https://github.com/{REPOSITORY}/releases/download/v0.2.7/"
PREVIOUS = "Flightdeck-Linux-x86_64.tar.gz"
PREVIOUS_SHA = "90459cf5bab7065bded336ea3d1ba0024f712403c26ae4749b7d88fefe212e02"
# These corresponding sources describe this exact native binary bundle.
NATIVE_SHA = "19d2e97e327315dde4563d299dab07d6ff64e09d6df8f51a3a94b26db94751b6"
SOURCES = {
    "flightdeck-native-sources-0.1.16.tar.gz": "bd9cab21421783b3f0cc2ce959aafa24522af17bbcfd2cbd41b2d74e3ab2e9c9",
    "flightdeck-dxvk-0.1.11-sources.tar.gz": "f4cc973e7fa3816fb040a92f32799e33d7a7fb98c9e5032152a903bb5b234234",
    "flightdeck-vkd3d-0.1.17-sources.tar.gz": "6f9f723b7c98ea01e42384f06fe36eef58d54ab32ea486cf8028d8c54029e07d",
}
MAX_ARCHIVE = 512 * 1024 * 1024
spec = importlib.util.spec_from_file_location("release_support", ROOT / "scripts/release-support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)


def version():
    value = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value):
        raise ValueError("Only stable numeric launcher versions can be released")
    return value


def run(command, *, github=False, check=True):
    environment = dict(os.environ)
    token = environment.pop("GH_TOKEN", None)
    environment.pop("GITHUB_TOKEN", None)
    if github:
        if not token:
            raise ValueError("Publishing requires the workflow's ephemeral GH_TOKEN")
        environment["GH_TOKEN"] = token
    return subprocess.run(command, cwd=ROOT, env=environment, check=check,
                          capture_output=True, text=True, timeout=900)


def eligible():
    """No commit-message interpolation into a shell; compare Git's actual subject."""
    if (os.environ.get("GITHUB_EVENT_NAME") != "push"
            or os.environ.get("GITHUB_REF") != "refs/heads/main"
            or os.environ.get("GITHUB_REPOSITORY") != REPOSITORY):
        return False
    head = run(["git", "rev-parse", "HEAD"]).stdout.strip()
    if head != os.environ.get("GITHUB_SHA"):
        return False
    subject = run(["git", "log", "-1", "--format=%s"]).stdout.strip()
    # A failed pre-publication check can be corrected by a new release commit
    # without incrementing an unpublished version. A real tag is never reused.
    current = version()
    return subject == f"Release Flightdeck {current}" and not tag_refs(f"v{current}")


def checksum(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_ARCHIVE:
            raise ValueError("Release input must be a bounded regular file")
        return hashlib.file_digest(stream, "sha256").hexdigest()


def download(url, destination, expected, offline):
    if destination.exists():
        if checksum(destination) != expected:
            raise ValueError(f"Cached release input differs: {destination.name}")
        return
    if offline:
        raise ValueError(f"Missing offline input: {destination.name}")
    if not url.startswith(f"https://github.com/{REPOSITORY}/releases/download/"):
        raise ValueError("Release input URL must be from this repository")
    temporary = destination.with_suffix(destination.suffix + ".part")
    with urllib.request.urlopen(url, timeout=60) as response, temporary.open("xb") as stream:
        if not response.url.startswith("https://"):
            raise ValueError("Release download redirected away from HTTPS")
        total = 0
        while block := response.read(1024 * 1024):
            total += len(block)
            if total > MAX_ARCHIVE:
                raise ValueError("Release download exceeds its size limit")
            stream.write(block)
    if checksum(temporary) != expected:
        raise ValueError(f"Release checksum differs: {destination.name}")
    temporary.rename(destination)


def package_contents(archive):
    """Validate every member before writing; never delegate to tar.extractall."""
    contents = {}
    total = 0
    with tarfile.open(archive, "r|gz") as source:
        for member in source:
            name = support.safe_name(member.name)
            if (not name.startswith("flightdeck-linux/") or name in contents
                    or len(contents) >= 4096 or member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE)
                    or member.sparse is not None
                    or any(key.startswith("GNU.sparse") for key in member.pax_headers)):
                raise ValueError("Unexpected full-package member")
            total += member.size
            if member.size < 0 or member.size > 128 * 1024 * 1024 or total > 384 * 1024 * 1024:
                raise ValueError("Full package exceeds expanded size limits")
            with source.extractfile(member) as stream:
                data = stream.read(member.size + 1)
            if len(data) != member.size:
                raise ValueError("Truncated full package")
            contents[name] = data
    support.reject_parent_files(contents)
    if "flightdeck-linux/FLIGHTDECK-PACKAGE.json" not in contents:
        raise ValueError("Missing native package manifest")
    return contents


def unpack(archive, destination):
    contents = package_contents(archive)
    destination.mkdir(parents=True, exist_ok=False)
    for name, data in contents.items():
        path = destination / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        relative = name.removeprefix("flightdeck-linux/")
        path.chmod(0o755 if relative in {"bin/flightdeck", "install.sh", "Install Flightdeck.desktop"}
                   or relative.startswith("resources/native/bin/") else 0o644)


def prepare(args):
    args.cache.mkdir(parents=True, exist_ok=True)
    native = support.read_json((ROOT / "compat/bootstrap.lock.json").read_bytes(), "Bootstrap lock")["native"]
    if native["archive_sha256"] != NATIVE_SHA:
        raise ValueError("Update the corresponding-source pins when changing native components")
    download(native["archive_url"], args.cache / "native.tar.gz", native["archive_sha256"], args.offline)
    support.verify_native(support.read_archive(support.read_regular(args.cache / "native.tar.gz", support.NATIVE_ARCHIVE_MAX)), native)
    for name, digest in {PREVIOUS: PREVIOUS_SHA, **SOURCES}.items():
        download(RELEASE_BASE + name, args.cache / name, digest, args.offline)
    unpack(args.cache / PREVIOUS, args.output)
    support.graphics_files((ROOT / "compat/graphics.lock.json").read_bytes(), args.output / "flightdeck-linux/resources/graphics")
    print("PASS: pinned native, graphics, rollback and corresponding-source inputs")


def asset_names():
    return {PREVIOUS, "Flightdeck-Linux-x86_64.zip", f"flightdeck-source-{version()}.tar.gz",
            "launcher-dependencies.json", "THIRD-PARTY-NOTICES.txt", *SOURCES}


def stage(args):
    if not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        raise ValueError("Expected an exact source commit")
    result = support.read_json(args.check.read_bytes(), "Installation test")
    if (result.get("status") != "PASS"
            or result.get("checks", {}).get("native_update_and_rollback") != "PASS"):
        raise ValueError("Native install/update/rollback check must pass before staging")
    binary = package_contents(args.assets / PREVIOUS).get("flightdeck-linux/bin/flightdeck", b"")
    if not binary or result.get("native_binary_sha256") != hashlib.sha256(binary).hexdigest():
        raise ValueError("Installation check tested a different packaged executable")
    for name, digest in SOURCES.items():
        source = args.cache / name
        if checksum(source) != digest:
            raise ValueError("Corresponding source archive differs")
        with (args.assets / name).open("xb") as destination, source.open("rb") as stream:
            shutil.copyfileobj(stream, destination)
    if {p.name for p in args.assets.iterdir()} != asset_names() | {"SHA256SUMS"}:
        raise ValueError("Release directory contains missing or unexpected assets")
    manifest = {"schema": 1, "version": version(), "commit": args.commit,
                "files": {name: checksum(args.assets / name) for name in sorted(asset_names())},
                "native_installation_check": result}
    provenance = ROOT / "build/release-build-provenance.json"
    if provenance.exists():
        manifest["binary_build"] = support.read_json(provenance.read_bytes(), "Build provenance")
    (args.assets / "RELEASE-MANIFEST.json").write_text(json.dumps(manifest, indent=2) + "\n")
    sums = "".join(f"{checksum(args.assets / name)}  {name}\n"
                   for name in sorted(asset_names() | {"RELEASE-MANIFEST.json"}))
    (args.assets / "SHA256SUMS").write_text(sums)
    verify(args.assets, args.commit)


def verify(directory, commit):
    expected = asset_names() | {"SHA256SUMS", "RELEASE-MANIFEST.json"}
    if {path.name for path in directory.iterdir()} != expected:
        raise ValueError("Unexpected release assets")
    manifest = support.read_json((directory / "RELEASE-MANIFEST.json").read_bytes(), "Release manifest")
    if (manifest.get("schema") != 1 or manifest.get("commit") != commit
            or manifest.get("version") != version() or set(manifest.get("files", {})) != asset_names()):
        raise ValueError("Release manifest does not match this version and commit")
    if manifest.get("native_installation_check", {}).get("checks", {}).get("native_update_and_rollback") != "PASS":
        raise ValueError("Release lacks a successful native update/rollback check")
    hashes = {name: checksum(directory / name) for name in expected}
    if any(hashes[name] != digest for name, digest in manifest["files"].items()):
        raise ValueError("Release assets changed after validation")
    if any(hashes[name] != digest for name, digest in SOURCES.items()):
        raise ValueError("Corresponding sources changed")
    wanted = "".join(f"{hashes[name]}  {name}\n" for name in sorted(expected - {"SHA256SUMS"}))
    if (directory / "SHA256SUMS").read_text() != wanted:
        raise ValueError("Release checksums do not describe exactly these assets")
    return hashes


def gh(*arguments, check=True):
    return run(["gh", "release", *arguments, "--repo", REPOSITORY], github=True, check=check)


def tag_refs(tag):
    return run(["git", "ls-remote", f"https://github.com/{REPOSITORY}.git",
                f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"]).stdout.strip()


def cleanup_merged_branch(commit):
    """One authorized 0.2.6 cleanup; compare-and-delete must reject a new tip."""
    if version() != "0.2.6":
        return
    branch = "fix/security-api-auth-rustls-20261006"
    expected = "6606305dd893176bd3218d00d149c572f8be3bd8"
    ref = "refs/heads/" + branch
    url = f"https://github.com/{REPOSITORY}.git"
    current = run(["git", "ls-remote", url, ref]).stdout.strip()
    if not current:
        return
    if current != f"{expected}\t{ref}":
        raise ValueError("Authorized cleanup branch changed; leave it untouched")
    comparison = json.loads(run(["gh", "api", f"repos/{REPOSITORY}/compare/{expected}...{commit}",
                                 "--jq", "{status: .status, base: .merge_base_commit.sha}"], github=True).stdout)
    if comparison["status"] not in {"ahead", "identical"} or comparison["base"] != expected:
        raise ValueError("Cleanup branch is not fully included in the release")
    # GitHub's DELETE ref endpoint has no compare-and-delete lease. Pass a
    # one-process HTTP credential through the environment, never URL/config/args.
    environment = dict(os.environ)
    token = environment.pop("GH_TOKEN")
    environment.pop("GITHUB_TOKEN", None)
    credential = base64.b64encode(("x-access-token:" + token).encode()).decode()
    environment.update(GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0="http.https://github.com/.extraheader",
                       GIT_CONFIG_VALUE_0="AUTHORIZATION: basic " + credential,
                       GIT_TERMINAL_PROMPT="0")
    subprocess.run(["git", "push", f"--force-with-lease={ref}:{expected}", url, ":" + ref],
                   cwd=ROOT, env=environment, check=True, capture_output=True, text=True, timeout=90)
    if run(["git", "ls-remote", url, ref]).stdout.strip():
        raise ValueError("Cleanup branch still exists; inspect without deleting another ref")


def publish(args):
    if not eligible() or run(["git", "status", "--porcelain", "--untracked-files=no"]).stdout:
        raise ValueError("Publishing requires a clean, exact main-branch release commit in Actions")
    commit = os.environ["GITHUB_SHA"]
    hashes = verify(args.assets, commit)
    tag = "v" + version()
    if tag_refs(tag):
        raise ValueError("Tag already exists; never replace a published target")
    existing = gh("view", tag, "--json", "tagName", check=False)
    if existing.returncode == 0 or existing.stderr.strip() != "release not found":
        raise ValueError("Release exists or its absence could not be verified")
    # POST /git/refs creates exactly this ref or fails (including HTTP 422/409
    # for an existing ref). No PATCH, force push, delete or clobber is allowed.
    run(["gh", "api", "--method", "POST", f"repos/{REPOSITORY}/git/refs",
         "--raw-field", f"ref=refs/tags/{tag}", "--raw-field", f"sha={commit}"], github=True)
    # Any later failure leaves the exact tag/draft for review. A rerun aborts;
    # it never replaces an existing tag, draft or asset.
    gh("create", tag, "--draft", "--latest=false", "--verify-tag", "--target", commit,
       "--title", f"Flightdeck {version()}", "--notes-file", str(args.notes))
    gh("upload", tag, *[str(args.assets / name) for name in sorted(hashes)])
    release = json.loads(gh("view", tag, "--json", "tagName,targetCommitish,isDraft,isPrerelease,assets").stdout)
    if (release["tagName"] != tag or release["targetCommitish"] != commit
            or not release["isDraft"] or release["isPrerelease"]
            or {asset["name"] for asset in release["assets"]} != set(hashes)
            or len(release["assets"]) != len(hashes)
            or any(asset["size"] != (args.assets / asset["name"]).stat().st_size for asset in release["assets"])):
        raise ValueError("Draft target or asset inventory changed")
    args.readback.mkdir(parents=True, exist_ok=False)
    gh("download", tag, "--dir", str(args.readback))
    if verify(args.readback, commit) != hashes:
        raise ValueError("Uploaded release bytes differ")
    # A concurrent tag creation must not redirect publication to another commit.
    refs = tag_refs(tag)
    if not refs or {line.split()[0] for line in refs.splitlines()} != {commit}:
        raise ValueError("Release tag changed during draft verification")
    gh("edit", tag, "--draft=false", "--prerelease=false", "--latest")
    published = json.loads(gh("view", tag, "--json", "tagName,targetCommitish,isDraft,isPrerelease").stdout)
    latest = json.loads(gh("view", "--json", "tagName").stdout)
    if (published["isDraft"] or published["isPrerelease"] or published["targetCommitish"] != commit
            or latest["tagName"] != tag):
        raise ValueError("Published release or Latest routing needs inspection")
    print(f"Published https://github.com/{REPOSITORY}/releases/tag/{tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("gate")
    commands.add_parser("cleanup")
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--cache", type=Path, required=True)
    prepare_parser.add_argument("--output", type=Path, required=True)
    prepare_parser.add_argument("--offline", action="store_true")
    unpack_parser = commands.add_parser("unpack")
    unpack_parser.add_argument("--archive", type=Path, required=True)
    unpack_parser.add_argument("--output", type=Path, required=True)
    stage_parser = commands.add_parser("stage")
    for name in ("assets", "cache", "check"):
        stage_parser.add_argument("--" + name, type=Path, required=True)
    stage_parser.add_argument("--commit", required=True)
    publish_parser = commands.add_parser("publish")
    for name in ("assets", "notes", "readback"):
        publish_parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    if args.command == "gate":
        output = f"eligible={str(eligible()).lower()}\nversion={version()}\n"
        if path := os.environ.get("GITHUB_OUTPUT"):
            with open(path, "a") as stream:
                stream.write(output)
        print(output, end="")
    elif args.command == "unpack":
        unpack(args.archive, args.output)
    elif args.command == "cleanup":
        if not eligible() or run(["git", "status", "--porcelain", "--untracked-files=no"]).stdout:
            raise ValueError("Cleanup requires the exact main-branch release commit in Actions")
        cleanup_merged_branch(os.environ["GITHUB_SHA"])
    else:
        {"prepare": prepare, "stage": stage, "publish": publish}[args.command](args)


if __name__ == "__main__":
    main()
