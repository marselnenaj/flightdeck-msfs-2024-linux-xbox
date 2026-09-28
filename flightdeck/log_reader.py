# SPDX-License-Identifier: MIT
"""Read diagnostic logs with bounded memory, work and explicit coverage.

Consumers receive complete lines, never pieces joined across an omitted range.
Raw log text stays inside the local diagnostic parser.
"""
import os
import stat
import time

BLOCK = 256 * 1024
MAX_LINE = 16 * 1024


def scan(path, consume, *, maximum=64 * 1024 * 1024, tail=512 * 1024, seconds=3):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode):
            raise OSError("not regular")
        deadline = time.monotonic() + seconds
        read = 0
        oversized = 0
        pending = b""
        discarding = False

        def feed(data, *, final=False):
            nonlocal pending, discarding, oversized
            data = pending + data
            if final and not data:
                return
            parts = data.split(b"\n")
            pending = b"" if final else parts.pop()
            lines = []
            for part in parts:
                if discarding:
                    discarding = False
                elif len(part) > MAX_LINE:
                    oversized += 1
                else:
                    lines.append(part)
            if len(pending) > MAX_LINE:
                if not discarding:
                    oversized += 1
                discarding = True
                pending = b""
            if lines:
                consume(b"\n".join(lines).decode("utf-8", errors="replace") + "\n")

        end = min(info.st_size, maximum)
        while stream.tell() < end and time.monotonic() < deadline:
            block = stream.read(min(BLOCK, end - stream.tell()))
            if not block:
                break
            read += len(block)
            feed(block)
        position = stream.tell()
        # If capped, still retain session shutdown and its final error codes.
        offset = max(position, info.st_size - tail)
        if position < info.st_size:
            if offset > position:
                pending = b""
                discarding = True  # skip the possibly incomplete first tail line
            stream.seek(offset)
            while stream.tell() < info.st_size:
                block = stream.read(min(BLOCK, info.st_size - stream.tell()))
                if not block:
                    break
                read += len(block)
                feed(block)
        feed(b"", final=True)
        after = os.fstat(stream.fileno())
        changed = (info.st_size, info.st_mtime_ns, info.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns)
        coverage = {"scope": "bounded_scan", "bytes_total": info.st_size, "bytes_read": read,
                    "omitted_bytes": max(0, info.st_size - read), "oversized_lines": oversized,
                    "changed_during_read": changed,
                    "complete": read == info.st_size and not oversized and not changed}
        return coverage, info
