#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run the complete Rust suite with private process visibility as the invoking user.

GitHub's runner has a persistent, unreadable sudo process with the runner's real
UID. The cloud guard correctly refuses that unknown writer. Isolate test process
visibility instead of weakening the application's guard or skipping tests.
Only namespace setup is privileged; Cargo and every test run without capabilities.
"""
import os
from pathlib import Path
import shutil
import subprocess


def command(environment, uid, gid, cargo):
    if uid == 0 or not cargo:
        raise ValueError("Run the tests as an unprivileged user with Cargo available")
    home = environment["HOME"]
    values = {
        "HOME": home,
        "PATH": environment["PATH"],
        "CARGO_HOME": environment.get("CARGO_HOME") or str(Path(home) / ".cargo"),
        "RUSTUP_HOME": environment.get("RUSTUP_HOME") or str(Path(home) / ".rustup"),
        "RUSTUP_TOOLCHAIN": environment.get("RUSTUP_TOOLCHAIN") or "1.98.0",
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
    }
    for name in ("ICED_TEST_BACKEND", "CARGO_TARGET_DIR"):
        if environment.get(name):
            values[name] = environment[name]
    return [
        "/usr/bin/sudo", "-n", "/usr/bin/unshare", "--mount", "--propagation", "private",
        "--pid", "--fork", "--kill-child=SIGKILL", "--mount-proc",
        "/usr/bin/setpriv", f"--reuid={uid}", f"--regid={gid}", "--init-groups",
        "--bounding-set=-all", "--inh-caps=-all", "--ambient-caps=-all", "--no-new-privs",
        "/usr/bin/env", "-i", *[f"{key}={value}" for key, value in values.items()],
        cargo, "test", "--locked", "--workspace",
    ]


if __name__ == "__main__":
    result = subprocess.run(
        command(os.environ, os.getuid(), os.getgid(), shutil.which("cargo")),
        env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"},
        check=False,
    )
    raise SystemExit(result.returncode)
