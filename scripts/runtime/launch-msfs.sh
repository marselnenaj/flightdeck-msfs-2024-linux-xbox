#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/runtime-env.sh"
export WINEPREFIX="$MSFS_LINUX_ROOT/local/msfs-prefix" WINEARCH=win64
export WINEESYNC=0 WINEFSYNC=0
export WINEDEBUG='-all,err+all,warn+gdkc,fixme+gdkc,warn+mmdevapi,warn+pulse,warn+alsa,warn+xaudio2,warn+dsound,warn+winegstreamer'
export DXVK_LOG_LEVEL="${DXVK_LOG_LEVEL:-warn}" VKD3D_DEBUG="${VKD3D_DEBUG:-warn}"
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:+$WINEDLLOVERRIDES;}xgameruntime=n;xgameruntime_original=n,b;xodus_store_test=b"
export WINEDLLPATH="$MSFS_LINUX_ROOT/local/store-runtime${WINEDLLPATH:+:$WINEDLLPATH}"
export XODUS_USER_RUNTIME=1
# Enabled only by the optional, version-checked Fenix installer.
if [[ -f "$MSFS_LINUX_ROOT/private/fenix-linux-patch.json" ]]; then
    python3 - "$MSFS_LINUX_ROOT/private/fenix-linux-patch.json" <<'FENIX'
import json, sys
if json.load(open(sys.argv[1])).get('state') != 'installed':
    raise SystemExit('Fenix setup is incomplete. Restore or finish it in Flightdeck before starting MSFS.')
FENIX
    export WINE_TRACK_WRITECOPY='apps:Fenix.exe,FenixSystem.exe,FenixDisplay.exe,FenixCDU.exe,FlightSimulator2024.exe'
    export WINE_D2D1_DISPLAY_EFFECTS='FenixDisplay.exe;FenixCDU.exe'
    export WINE_D2D1_GEOMETRY_PROVIDER='FenixDisplay.exe'
    export WINE_DWRITE_UNHINTED_OUTLINES='FenixDisplay.exe;FenixCDU.exe'
    export DOTNET_SYSTEM_GLOBALIZATION_USENLS=1 DOTNET_ReadyToRun=0
    export WINE_FENIX_WINDOW_GUARD=1
    export WINE_FENIX_HELPER_WINDOWS=1
    export WINE_FENIX_DISPLAY_REFRESH=1
fi
export XODUS_WINE_RUNNER="$MSFS_LINUX_ROOT/runner/files/bin/wine"
export MEDIACONV_BLANK_VIDEO_FILE="$MSFS_LINUX_ROOT/runner/files/share/media/blank.mkv"
export MEDIACONV_BLANK_AUDIO_FILE="$MSFS_LINUX_ROOT/runner/files/share/media/blank.ptna"
if [[ -d "$MSFS_LINUX_ROOT/local/media-plugins" ]]; then
    export GST_PLUGIN_PATH_1_0="$MSFS_LINUX_ROOT/local/media-plugins${GST_PLUGIN_PATH_1_0:+:$GST_PLUGIN_PATH_1_0}"
fi
export WINE_GST_REGISTRY_DIR="$MSFS_LINUX_ROOT/private/gstreamer"
export GST_REGISTRY_1_0="$MSFS_LINUX_ROOT/private/gstreamer/registry.bin"
mkdir -p "$WINE_GST_REGISTRY_DIR"
if [[ -f "$MSFS_LINUX_ROOT/private/local-saves.enabled" && -d "$MSFS_LINUX_ROOT/private/local-saves" ]]; then
    export XODUS_LOCAL_GAMESAVE=1
    XODUS_LOCAL_GAMESAVE_ROOT="Z:${MSFS_LINUX_ROOT//\//\\}\\private\\local-saves"
    export XODUS_LOCAL_GAMESAVE_ROOT
else
    unset XODUS_LOCAL_GAMESAVE XODUS_LOCAL_GAMESAVE_ROOT
fi
configuration=$(python3 - "$MSFS_LINUX_ROOT/private/runtime.json" <<'PY'
import json, re, sys
from pathlib import Path
settings = json.load(open(sys.argv[1]))
market = settings['market']
game_id = settings.get('game_id', 'msfs2024')
if not isinstance(market, str) or not re.fullmatch('[A-Z]{2}', market):
    raise SystemExit('Invalid configured market')
if game_id not in ('msfs2020', 'msfs2024'):
    raise SystemExit('Invalid configured game')
private = Path(sys.argv[1]).parent
if (private / 'proton-switch.json').exists():
    raise SystemExit('Interrupted Proton switch. Restore Flightdeck from the Proton settings first.')
proton = private / 'proton-selection.json'
loader = 'native'
if proton.exists() or proton.is_symlink():
    if proton.is_symlink() or not proton.is_file() or proton.stat().st_size > 65536:
        raise SystemExit('Invalid Proton selection')
    selected = json.loads(proton.read_text())
    runner = selected.get('runner', '')
    if (selected.get('schema') not in (1, 2) or not isinstance(runner, str)
            or not re.fullmatch(r'local/(?:proton-tests/[0-9a-f]{32}|fenix-patch-[0-9T]+-[0-9a-f]{8})/runner', runner)
            or (private.parent / 'runner').resolve() != private.parent / runner):
        raise SystemExit('Invalid Proton runner selection')
    loader = 'portable'
print(game_id, market, loader)
PY
)
read -r game_id market FLIGHTDECK_PROTON_LOADER <<< "$configuration"
export FLIGHTDECK_PROTON_LOADER
if [[ "$FLIGHTDECK_PROTON_LOADER" == portable ]]; then
    export WINE_DISABLE_FAST_SYNC=1
    if [[ -x "$MSFS_LINUX_ROOT/runner/files/bin/wine64" ]]; then
        export XODUS_WINE_RUNNER="$MSFS_LINUX_ROOT/runner/files/bin/wine64"
    fi
    export WINELOADER="$XODUS_WINE_RUNNER"
    export WINESERVER="$MSFS_LINUX_ROOT/runner/files/bin/wineserver"
fi
case "$game_id" in
    msfs2020) directory=MSFS2020; executable=FlightSimulator.exe ;;
    msfs2024) directory=MSFS2024; executable=FlightSimulator2024.exe ;;
esac
game="$MSFS_LINUX_ROOT/games/$directory"
test -f "$game/.xodus-streaming.msixvc" && test -f "$game/$executable"
# Use the same explicit market for catalog prices and game licensing.
export XODUS_STORE_MARKET="$market"
cd "$game"
exec "$MSFS_LINUX_ROOT/tools/xodus.sh" run "$game" \
    "$MSFS_LINUX_ROOT/tools/xodus-wine-launch" --exe "$executable" --market "$market"
