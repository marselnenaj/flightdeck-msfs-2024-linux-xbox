# SPDX-License-Identifier: MIT
"""Install matched DXVK/VKD3D corrections while the runtime lease is held."""
from __future__ import annotations

import json
from pathlib import Path
import re

LEGACY_FILES = ("d3d12.dll", "d3d12core.dll")
FILES = (*LEGACY_FILES, "dxgi.dll", "d3d11.dll", "d3d10core.dll")
MARKER = "renderer-runtime.json"


def _hashes(value, files=None):
    allowed = (set(files),) if files is not None else (set(LEGACY_FILES), set(FILES))
    if (not isinstance(value, dict) or set(value) not in allowed
            or any(not isinstance(v, str) or not re.fullmatch(r"[0-9a-f]{64}", v)
                   for v in value.values())):
        raise ValueError("Invalid renderer file hashes")
    return value


def install(runtime, *, bundle=None):
    """All NVIDIA modes use the corrections; never replace a custom renderer.

    The shared runner and Fenix overlay remain untouched. All inputs and all
    destination files are checked before replacing any DLL. A failed copy
    stops launch; the next start recognizes and completes a partial update.
    """
    from .backend import atomic_json
    from .graphics import GraphicsError, _digest
    from .graphics_diagnostics import _read
    from .setup import SetupError, _copy_file, prefix_system32

    bundle = Path(bundle) if bundle is not None else Path(__file__).parent / "resources/graphics"
    private = runtime / "private"
    marker = private / MARKER
    if not bundle.exists() and not marker.exists() and not marker.is_symlink():
        return "runner"
    try:
        if private.is_symlink() or not private.is_dir() or bundle.is_symlink():
            raise ValueError()
        previous = _hashes(json.loads(_read(marker, 4096))) if marker.exists() or marker.is_symlink() else {}
        manifest = None
        files = tuple(previous) if previous else FILES
        if bundle.exists():
            manifest = json.loads(_read(bundle / "manifest.json", 8192))
            if not isinstance(manifest, dict) or manifest.get("schema") != 2:
                raise ValueError()
            files = FILES
        runner = runtime / "runner/files/lib/wine"
        sources = {name: runner / ("vkd3d-proton" if name in LEGACY_FILES else "dxvk")
                   / "x86_64-windows" / name for name in files}
        original = {name: _digest(source) for name, source in sources.items()}
        expected = original
        state = "runner"
        if manifest is not None:
            base = _hashes(manifest.get("base"), files)
            replacement = _hashes(manifest.get("files"), files)
            # Verify even an unused bundled payload, never silently use a
            # damaged package or combine libraries from different builds.
            for name in files:
                if (bundle / name).is_symlink() or _digest(bundle / name) != replacement[name]:
                    raise ValueError()
            if original == base:
                sources = {name: bundle / name for name in files}
                expected = replacement
                state = "backport"
        prefix = runtime / "local/msfs-prefix"
        if prefix.is_symlink() or not prefix.is_dir():
            raise ValueError()
        system = prefix_system32(prefix)
        current = {}
        for name in files:
            target = system / name
            current[name] = _digest(target) if target.exists() or target.is_symlink() else None
            if current[name] not in (None, original[name], expected[name], previous.get(name)):
                return "custom"
        # Also restores our old copies when the selected runner changes or a
        # source-only launcher replaces one that contained the backport.
        for name in files:
            if current[name] != expected[name]:
                _copy_file(sources[name], system / name)
        if state == "backport":
            atomic_json(marker, expected)
        else:
            marker.unlink(missing_ok=True)
        return state
    except (OSError, ValueError, TypeError, SetupError) as error:
        raise GraphicsError("Die Grafik-Laufzeit konnte nicht vorbereitet werden. Bitte Flightdeck erneut installieren.") from error
