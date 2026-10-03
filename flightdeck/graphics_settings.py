# SPDX-License-Identifier: MIT
"""Keep saved MSFS 2024 NVIDIA options consistent with the launch profile.

Called with the runtime lease held and Wine idle. Only known Video values are
changed; the per-field undo record travels with the prefix across Proton moves.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import stat
import uuid
from xml.etree.ElementTree import ParseError

from . import games

MARKER = ".flightdeck-nvidia-settings.json"
LIMIT = 1024 * 1024
OPTIONS = {
    "AntiAliasing": ({"DLSS"}, "TAA"),
    "Reflex": ({"ON", "BOOST", "ONBOOST", "ON+BOOST"}, "OFF"),
    "FrameGeneration": ({"DLSSG"}, "NONE"),
}
OPTIONS.update({key + "VR": value for key, value in list(OPTIONS.items())})


def _decode(data):
    for bom, codec in ((b"\xff\xfe", "utf-16-le"), (b"\xfe\xff", "utf-16-be"),
                       (b"\xef\xbb\xbf", "utf-8"), (b"", "utf-8")):
        if data.startswith(bom):
            return data[len(bom):].decode(codec), bom, codec


def transform(data, original, compatibility):
    """Return edited bytes, remaining undo values and actually changed keys."""
    if (not isinstance(original, dict) or any(key not in OPTIONS or not isinstance(value, str) or value not in OPTIONS[key][0]
                                            for key, value in original.items())):
        raise ValueError("Invalid NVIDIA settings backup")
    text, bom, codec = _decode(data)
    # Match the native parser: only LF/CRLF delimit settings, rather than
    # Python's additional Unicode/control-character line separators.
    lines = re.findall(r"[^\n]*\n|[^\n]+$", text)
    depth, video, found, fields = 0, False, False, {}
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("{"):
            if video:
                raise ValueError("Nested Video settings")
            if stripped == "{Video" and depth == 0:
                if found:
                    raise ValueError("Duplicate Video settings")
                video = found = True
            depth += 1
        elif stripped == "}":
            depth -= 1
            if depth < 0:
                raise ValueError("Unbalanced settings")
            video = False
        elif video and stripped.split(" ", 1)[0].split("\t", 1)[0] in OPTIONS:
            match = re.fullmatch(r"([ \t]*)(\w+)([ \t]+)([^ \t\r\n]{1,32})([ \t]*)(\r?\n)?", line)
            if match is None or match[2] in fields:
                raise ValueError("Ambiguous Video option")
            fields[match[2]] = (index, match)
    if not found or depth != 0:
        raise ValueError("Incomplete Video settings")
    saved, changed = {}, []
    for key, (index, match) in fields.items():
        value, replacement = match[4], match[4]
        unsafe, safe = OPTIONS[key]
        if compatibility:
            if value in unsafe:
                saved[key], replacement = value, safe
            elif value == safe and key in original:
                saved[key] = original[key]
        elif key in original and value == safe:
            replacement = original[key]
        if replacement != value:
            lines[index] = match[1] + key + match[3] + replacement + match[5] + (match[6] or "")
            changed.append(key)
    return bom + "".join(lines).encode(codec), saved, sorted(changed)


def _folder(path):
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise ValueError("Linked or unowned settings directory")


def _case_child(parent, name):
    # Windows paths are case-insensitive. Refuse ambiguous or unbounded trees.
    _folder(parent)
    with os.scandir(parent) as scan:
        entries = []
        for entry in scan:
            entries.append(entry.name)
            if len(entries) > 256:
                raise ValueError("Too many settings entries")
    matches = [entry for entry in entries if entry.casefold() == name.casefold()]
    if len(matches) > 1:
        raise ValueError("Ambiguous settings path")
    if not matches:
        raise FileNotFoundError()
    return parent / matches[0]


def _read(path, maximum):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or info.st_size > maximum:
            raise ValueError("Unsupported settings file")
        data = stream.read(maximum + 1)
        if len(data) > maximum:
            raise ValueError("Settings grew during read")
        return data


def _atomic(path, data):
    temporary = path.with_name(".flightdeck-" + uuid.uuid4().hex)
    try:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        temporary.unlink(missing_ok=True)


def prepare(runtime, compatibility):
    result = {"changed_files": 0, "changed_options": [], "skipped_files": 0}
    if games.for_runtime(runtime).id != "msfs2024":
        return result
    try:
        folder = runtime
        for name in ("local", "msfs-prefix", "drive_c", "users"):
            folder = _case_child(folder, name)
        _folder(folder)
        with os.scandir(folder) as scan:
            profiles = []
            for entry in scan:
                profiles.append(Path(entry.path))
                if len(profiles) > 16:
                    raise ValueError("Too many Wine profiles")
    except FileNotFoundError:
        return result
    except (OSError, ValueError):
        result["skipped_files"] += 1
        return result
    from .mods import _family
    try:
        family = _family(runtime)
    except (OSError, ValueError, ParseError):
        # Optional packaged-app location: damaged package metadata must not
        # prevent preparation of the independent Roaming configuration.
        family = None
    paths = [("AppData", "Roaming", "Microsoft Flight Simulator 2024", "UserCfg.opt")]
    if family:
        paths.append(("AppData", "Local", "Packages", family, "LocalCache", "UserCfg.opt"))
    for profile in sorted(profiles):
        for parts in paths:
            try:
                config = profile
                for part in parts:
                    config = _case_child(config, part)
                data = _read(config, LIMIT)
                marker = config.parent / MARKER
                try:
                    record = json.loads(_read(marker, 4096))
                    if (not isinstance(record, dict) or set(record) != {"schema", "original"}
                            or type(record["schema"]) is not int or record["schema"] != 1):
                        raise ValueError("Invalid settings backup")
                    original = record["original"]
                except FileNotFoundError:
                    original = {}
                edited, saved, changed = transform(data, original, compatibility)
            except FileNotFoundError:
                continue
            except (OSError, ValueError, TypeError):
                result["skipped_files"] += 1
                continue
            # Keep the undo record before replacing the config. A retry after
            # interruption recognizes both the original and the applied value.
            if _read(config, LIMIT) != data:
                raise ValueError("Graphics settings changed during preparation")
            if saved:
                if saved != original:
                    _atomic(marker, (json.dumps({"schema": 1, "original": saved}) + "\n").encode())
            if edited != data:
                _atomic(config, edited)
                result["changed_files"] += 1
                result["changed_options"] = sorted(set(result["changed_options"]) | set(changed))
            if not saved:
                marker.unlink(missing_ok=True)
    return result
