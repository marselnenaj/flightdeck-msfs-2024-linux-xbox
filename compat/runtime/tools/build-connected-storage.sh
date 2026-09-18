#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
stage=$(realpath -- "${1:?staged compatibility build directory required}")
project=$(realpath -- "$source_dir/../../..")
output=${2:-"$stage/artifacts/bin/flightdeck-connected-storage.exe"}
"$stage/wine-build/tools/widl/widl" --win64 -D__WINESRC__ -h -I "$stage/wine-src/include" \
    -o "$stage/runtime/include/xgame.h" "$stage/wine-src/include/xgame.idl"
mkdir -p -- "$(dirname -- "$output")"
x86_64-w64-mingw32-g++ -std=c++17 -O2 -static -s \
    "-ffile-prefix-map=$project=/usr/src/flightdeck" "-ffile-prefix-map=$stage=/usr/src/flightdeck-build" \
    -I "$source_dir/../include" -I "$stage/runtime/include" \
    "$source_dir/connected-storage.cpp" "$source_dir/connected-storage-http.cpp" \
    -o "$output" -lwinhttp -luuid -lole32 -lbcrypt
