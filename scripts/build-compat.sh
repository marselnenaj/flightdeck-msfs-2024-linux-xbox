#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Build only the staged compatibility components; no install or game launch.
set -euo pipefail
stage=$(realpath -- "${1:-$(dirname -- "$0")/../build/compat}")
jobs=${BUILD_JOBS:-$(getconf _NPROCESSORS_ONLN)}
test -f "$stage/source-manifest.json"
mkdir -p "$stage/wine-build" "$stage/artifacts/bin" \
    "$stage/artifacts/runtime" "$stage/artifacts/builtin/x86_64-windows" \
    "$stage/artifacts/builtin/x86_64-unix"
if [[ ! -f "$stage/wine-build/Makefile" ]]; then
    (cd "$stage/wine-build" && ../wine-src/configure --enable-win64 \
        --without-x --without-wayland --without-freetype --without-alsa \
        --without-pulse --without-dbus --without-gstreamer --disable-tests)
fi
make -C "$stage/wine-build" -j"$jobs" \
    dlls/xgameruntime/x86_64-windows/xodus_store_test.dll
cp -- "$stage/wine-build/dlls/xgameruntime/x86_64-windows/xodus_store_test.dll" \
    "$stage/artifacts/builtin/x86_64-windows/"
# Remove compiler debug paths without changing the exported runtime interface.
x86_64-w64-mingw32-strip --strip-debug \
    "$stage/artifacts/builtin/x86_64-windows/xodus_store_test.dll"
gcc -shared -fPIC -O2 -D__WINESRC__ -DWINE_NO_TRACE_MSGS -DWINE_NO_DEBUG_MSGS \
    -fshort-wchar -Wl,-z,defs -pthread -I"$stage/wine-src/include" \
    "$stage/wine-src/dlls/xgameruntime/GDKComponent/Xodus/Unix/socket.c" \
    -o "$stage/artifacts/builtin/x86_64-unix/xodus_store_test.so"
for header in xasync xgameerr xgameruntimetypes xtaskqueue; do
    cp -- "$stage/wine-src/include/$header.h" "$stage/runtime/include/"
done
for header in xasyncprovider xgameruntimefeature xnetworking xsystem xuser; do
    "$stage/wine-build/tools/widl/widl" --win64 -D__WINESRC__ -h \
        -I "$stage/wine-src/include" -o "$stage/runtime/include/$header.h" \
        "$stage/wine-src/include/$header.idl"
done
"$stage/wine-build/tools/widl/widl" --win64 -D__WINESRC__ -h \
    -I "$stage/wine-src/include" -o "$stage/runtime/include/xstore.h" \
    "$stage/runtime/idl/xstore.idl"
(cd "$stage/runtime" && bash build.sh)
cp -- "$stage/runtime/xgameruntime.dll" "$stage/artifacts/runtime/"
# Uses the same generated headers and pinned Wine ABI; never authenticates.
bash "$stage/runtime/tools/build-connected-storage.sh" "$stage"
export CARGO_TARGET_DIR="$stage/cargo-target"
# Rust embeds panic/source locations even in release builds. Keep personal
# checkout and Cargo-cache paths out of distributable binaries.
flightdeck_cargo_sources=${CARGO_HOME:-"$HOME/.cargo"}
export CARGO_ENCODED_RUSTFLAGS
CARGO_ENCODED_RUSTFLAGS=$(python3 - "$stage" "$flightdeck_cargo_sources" <<'PY'
import os, sys
# Cargo's encoded form preserves an installation path containing spaces.
flags = (os.environ['CARGO_ENCODED_RUSTFLAGS'].split('\x1f')
         if os.environ.get('CARGO_ENCODED_RUSTFLAGS') else os.environ.get('RUSTFLAGS', '').split())
flags += ['--remap-path-prefix=' + sys.argv[1] + '=/usr/src/flightdeck',
          '--remap-path-prefix=' + sys.argv[2] + '=/usr/src/cargo']
print('\x1f'.join(flags), end='')
PY
)
# CLI and service deliberately share default Linux Secret Service credentials.
cargo build --locked --release --manifest-path "$stage/xodus-src/Cargo.toml" \
    -p xodus-cli -p xodus-service --bin xodus-cli --bin xodus-service
cp -- "$CARGO_TARGET_DIR/release/xodus-cli" "$CARGO_TARGET_DIR/release/xodus-service" \
    "$stage/artifacts/bin/"
python3 - "$stage" <<'PY'
import hashlib, json, pathlib, sys
stage = pathlib.Path(sys.argv[1]); root = stage / 'artifacts'
files = {str(p.relative_to(root)): hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()
         for p in sorted(root.rglob('*')) if p.is_file() and p.name != 'manifest.json'}
(root / 'manifest.json').write_text(json.dumps({'format': 1, 'source_manifest_sha256':
    hashlib.file_digest((stage/'source-manifest.json').open('rb'), 'sha256').hexdigest(),
    'credential_backend': 'Linux Secret Service (D-Bus)',
    'features': ['connected-storage-read-v1', 'connected-storage-sync-v1'],
    'cli_features': ['streaming-resume-files-v1', 'package-info-json-v1',
                     'streaming-integrity-index-v1', 'streaming-progress-v1'], 'files': files}, indent=2)+'\n')
print('Built', len(files), 'artifacts; no installation performed.')
PY
