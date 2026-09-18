# SPDX-License-Identifier: MIT
"""Bounded numeric progress over a dedicated pipe, never the CLI's logs."""
from __future__ import annotations

import json
import os
import time

MAX_INTEGER = 2**53 - 1
FIELDS = frozenset(("received_bytes", "verified_bytes", "total_bytes", "completed_files", "total_files"))


def normalize_transfer(value):
    """Only allow finite integer counters and a fixed, locally chosen kind."""
    if not isinstance(value, dict) or set(value) != FIELDS | {"kind"}:
        return None
    if value["kind"] not in ("game", "components"):
        return None
    for key in FIELDS:
        number = value[key]
        if number is None and key in ("total_bytes", "completed_files", "total_files"):
            continue
        if type(number) is not int or not 0 <= number <= MAX_INTEGER:
            return None
    received, verified, total = (value[key] for key in ("received_bytes", "verified_bytes", "total_bytes"))
    if verified > received or (total is not None and (total <= 0 or received > total)):
        return None
    done, count = value["completed_files"], value["total_files"]
    if (done is None) != (count is None) or (count is not None and (count <= 0 or done > count)):
        return None
    return dict(value)


def _unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate progress field")
        result[key] = value
    return result


class ProgressPipe:
    """An optional display channel cannot authorize or complete an install.

    Stdout/stderr remain discarded. Only this inherited write descriptor accepts
    the pinned streaming-progress-v1 protocol. Each poll has a fixed byte budget
    and overlong frames are discarded without recording their contents.
    """
    MAX_FRAME = 1024
    READ_BUDGET = 64 * 1024
    INTERVAL = .5

    def __init__(self, callback):
        self.callback = callback
        self.read_fd, self.write_fd = os.pipe2(os.O_CLOEXEC)
        os.set_blocking(self.read_fd, False)
        self.buffer = bytearray()
        self.discarding = False
        self.pending = None
        self.changed = False
        self.last_emitted = None

    def child_started(self):
        os.close(self.write_fd)
        self.write_fd = None

    def _frame(self, raw):
        try:
            value = json.loads(raw, object_pairs_hook=_unique_pairs)
            if (not isinstance(value, dict) or set(value) != FIELDS | {"format"}
                    or type(value["format"]) is not int or value["format"] != 1):
                raise ValueError("Unknown progress format")
            value.pop("format")
            self.pending = normalize_transfer({"kind": "game", **value})
        except (ValueError, TypeError, UnicodeError, RecursionError):
            self.pending = None
        self.changed = True

    def _consume(self, data):
        for part in data.splitlines(keepends=True):
            ended = part.endswith(b"\n")
            if not self.discarding:
                if len(self.buffer) + len(part) > self.MAX_FRAME:
                    self.buffer.clear()
                    self.discarding = True
                    self.pending = None
                    self.changed = True
                else:
                    self.buffer.extend(part)
            if ended:
                if not self.discarding:
                    self._frame(bytes(self.buffer))
                self.buffer.clear()
                self.discarding = False

    def poll(self, *, final=False):
        remaining = self.READ_BUDGET
        while remaining > 0:
            try:
                data = os.read(self.read_fd, min(4096, remaining))
            except BlockingIOError:
                break
            if not data:
                break
            remaining -= len(data)
            self._consume(data)
        now = time.monotonic()
        if self.changed and (final or self.last_emitted is None or now - self.last_emitted >= self.INTERVAL):
            # Never surface the content of an invalid frame, including errors.
            try:
                self.callback(self.pending)
            except Exception:
                pass  # Optional display feedback cannot corrupt the download.
            self.last_emitted = now
            self.changed = False

    def close(self):
        for name in ("read_fd", "write_fd"):
            fd = getattr(self, name)
            if fd is not None:
                os.close(fd)
                setattr(self, name, None)
