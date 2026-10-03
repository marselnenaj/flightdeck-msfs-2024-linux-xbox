#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compare native formats with the legacy implementation, using synthetic data only.

Python is a development oracle, never a dependency of flightdeck-rust.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from flightdeck import save_state
from flightdeck._fenix import core


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    cases = 0

    def invoke(*argv, ok=True):
        result = subprocess.run([str(binary), *map(str, argv)], capture_output=True, timeout=30)
        if (result.returncode == 0) != ok:
            raise AssertionError("Native contract outcome differs: " + str(argv[0]))
        return json.loads(result.stdout) if result.stdout else None

    with tempfile.TemporaryDirectory(prefix="flightdeck-rust-parity-") as directory:
        root = Path(directory)
        rng = random.Random(20261003)
        for n in range(32):
            state = save_state.State(n, {})
            for i in range(n % 7):
                state.containers[f"folder/profile{i}"] = save_state.Container(
                    "Flugzeug ✈ " + str(i), rng.randrange(2**31),
                    {f"blob{j}": rng.randbytes(rng.randrange(2048)) for j in range(i + 1)})
            raw = save_state.encode(state)
            path = root / "state.bin"
            path.write_bytes(raw)
            result = invoke("save-check", path)
            assert result["canonical_sha256"] == hashlib.sha256(raw).hexdigest()
            assert result["content_sha256"] == save_state.content_digest(state)
            assert result["containers"] == len(state.containers)
            assert result["blobs"] == sum(len(v.blobs) for v in state.containers.values())
            cases += 1
            for invalid in (raw[:-1], raw + b"x", raw[:20]):
                path.write_bytes(invalid)
                invoke("save-check", path, ok=False)
                cases += 1
        for lowercase in (False, True):
            for x86, x64, release in ((True, True, 528049), (False, True, 528049),
                                      (True, False, 528049), (True, True, 461808), (True, True, 533320)):
                prefix = root / f"prefix-{cases}"
                prefix.mkdir()
                lines = []
                for present, middle, architecture in ((x64, "", "Framework64"), (x86, r"Wow6432Node\\", "Framework")):
                    if not present:
                        continue
                    key = r"Software\\" + middle + r"Microsoft\\NET Framework Setup\\NDP\\v4\\Full"
                    lines.append('[%s] 123\n"Release"=dword:%08x\n' % (key, release))
                    relative = f"drive_c/windows/Microsoft.NET/{architecture}/v4.0.30319"
                    folder = prefix / (relative.lower() if lowercase else relative)
                    folder.mkdir(parents=True)
                    (folder / "clr.dll").write_bytes(b"MZfixture")
                text = "\n".join(lines)
                (prefix / "system.reg").write_text(text.lower() if lowercase else text)
                expected = core.framework_status(prefix)
                assert invoke("framework-check", "--prefix", prefix, ok=expected["ready"]) == expected
                cases += 1
    print(f"Rust/Python parity: {cases} synthetic format and .NET checks passed.")


if __name__ == "__main__":
    main()
