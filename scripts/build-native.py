#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build the native release with checkout, Cargo and toolchain paths remapped."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--toolchain", help="An installed Rust toolchain, for example 1.98.0")
    args = parser.parse_args()
    selector = ["+" + args.toolchain] if args.toolchain else []
    sysroot = Path(subprocess.check_output(["rustc", *selector, "--print", "sysroot"], text=True).strip())
    environment = dict(os.environ)
    flags = (environment["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
             if environment.get("CARGO_ENCODED_RUSTFLAGS") else shlex.split(environment.get("RUSTFLAGS", "")))
    for source, replacement in ((ROOT, "/usr/src/flightdeck"),
                                (Path(environment.get("CARGO_HOME", Path.home() / ".cargo")), "/usr/src/cargo"),
                                (sysroot, "/usr/src/rust")):
        flags.append("--remap-path-prefix=" + str(source.resolve()) + "=" + replacement)
    environment.pop("RUSTFLAGS", None)
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    subprocess.run(["cargo", *selector, "build", "--locked", "--release", *(["--offline"] if args.offline else [])],
                   cwd=ROOT, env=environment, check=True)


if __name__ == "__main__":
    main()
