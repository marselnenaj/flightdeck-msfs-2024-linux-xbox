#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/runtime-env.sh"
export WINEPREFIX="$MSFS_LINUX_ROOT/local/msfs-prefix" WINEARCH=win64
export WINEESYNC=0 WINEFSYNC=0
export WINEDEBUG='-all,err+all,warn+gdkc,fixme+gdkc'
export DXVK_LOG_LEVEL=warn VKD3D_DEBUG=warn
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:+$WINEDLLOVERRIDES;}xgameruntime=n;xgameruntime_original=n,b;xodus_store_test=b"
export WINEDLLPATH="$MSFS_LINUX_ROOT/local/store-runtime${WINEDLLPATH:+:$WINEDLLPATH}"
export XODUS_USER_RUNTIME=1
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
game="$MSFS_LINUX_ROOT/games/MSFS2024"
test -f "$game/.xodus-streaming.msixvc" && test -f "$game/FlightSimulator2024.exe"
market=$(python3 - "$MSFS_LINUX_ROOT/private/runtime.json" <<'PY'
import json, re, sys
market = json.load(open(sys.argv[1]))['market']
if not isinstance(market, str) or not re.fullmatch('[A-Z]{2}', market):
    raise SystemExit('Invalid configured market')
print(market)
PY
)
# Use the same explicit market for catalog prices and game licensing.
export XODUS_STORE_MARKET="$market"
cd "$game"
exec "$MSFS_LINUX_ROOT/tools/xodus.sh" run "$game" \
    "$MSFS_LINUX_ROOT/tools/xodus-wine-launch" --exe FlightSimulator2024.exe --market "$market"
