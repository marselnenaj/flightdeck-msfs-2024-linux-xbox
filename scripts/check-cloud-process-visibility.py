#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Report bounded process visibility diagnostics without environment values."""
import errno
import json
import os
from pathlib import Path
import re
import select

KEYS = (b"WINEPREFIX", b"MSFS_LINUX_ROOT", b"FLIGHTDECK_HELPER_RUNTIME")


def read(path, maximum):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    with os.fdopen(descriptor, "rb") as stream:
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise OSError(errno.EFBIG, "bounded read exceeded")
    return data


def alive(descriptor):
    poll = select.poll()
    poll.register(descriptor, select.POLLIN)
    return not poll.poll(0)


def inspect():
    output = {"schema": 1, "uid": os.getuid(), "processes": [], "truncated": False}
    for index, process in enumerate(Path("/proc").iterdir()):
        if index >= 131072 or len(output["processes"]) >= 256:
            output["truncated"] = True
            break
        if not process.name.isdecimal() or int(process.name) == os.getpid():
            continue
        pid = int(process.name)
        try:
            status = read(process / "status", 65536)
        except OSError as error:
            if error.errno not in (errno.ENOENT, errno.ESRCH):
                output["processes"].append({"pid": pid, "stage": "status", "errno": error.errno})
            continue
        ids = next((line.split()[1:3] for line in status.splitlines() if line.startswith(b"Uid:")), [])
        if len(ids) != 2 or not all(value.isdigit() for value in ids):
            output["processes"].append({"pid": pid, "stage": "status_uid", "errno": None})
            continue
        if os.getuid() not in map(int, ids):
            continue
        row = {"pid": pid, "uids": list(map(int, ids))}
        try:
            descriptor = os.pidfd_open(pid, 0)
        except OSError as error:
            if error.errno != errno.ESRCH:
                output["processes"].append({**row, "stage": "pidfd", "errno": error.errno})
            continue
        try:
            if not alive(descriptor):
                continue
            try:
                command = read(process / "cmdline", 1024 * 1024)
                name = command.split(b"\0", 1)[0].rsplit(b"/", 1)[-1].decode("ascii", "replace")
                row["name"] = re.sub(r"[^A-Za-z0-9._()-]", "?", name[:64])
            except OSError as error:
                row["name_errno"] = error.errno
            try:
                environment = read(process / "environ", 2 * 1024 * 1024)
            except OSError as error:
                if alive(descriptor):
                    output["processes"].append({**row, "stage": "environment", "errno": error.errno})
                continue
            if not alive(descriptor):
                continue
            row.update(stage="readable", tracked=[])
            for variable in environment.split(b"\0"):
                key, separator, value = variable.partition(b"=")
                if separator and key in KEYS:
                    item = {"key": key.decode("ascii"), "empty": not value}
                    try:
                        Path(os.fsdecode(value)).resolve(strict=True)
                        item["canonical"] = True
                    except (OSError, ValueError) as error:
                        item.update(canonical=False, errno=getattr(error, "errno", None))
                    row["tracked"].append(item)
            output["processes"].append(row)
        finally:
            os.close(descriptor)
    return output


if __name__ == "__main__":
    print(json.dumps(inspect(), indent=2))
