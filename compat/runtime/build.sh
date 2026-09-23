#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
sources=(
    src/proxy.cpp
    src/GameSaveLocalCore.cpp src/GameSaveBridge.cpp src/GameSaveAsync.cpp
    src/RuntimeDiagnostics.cpp src/UserBridge.cpp
    src/StoreBridge.cpp src/StoreContext.cpp src/StoreQueries.cpp
    src/StoreLicenseEvents.cpp src/StoreDurableLicense.cpp src/StoreCatalog.cpp src/StoreCatalogBatch.cpp
    src/StoreCatalogCoinMapper.cpp src/StoreCatalogProvider.cpp
    src/NetworkingState.cpp src/NetworkSecurity.cpp src/XThreading.cpp
    src/XAsync.cpp src/XTaskQueue.cpp src/ThreadPool.cpp src/WaitTimer.cpp
)
x86_64-w64-mingw32-g++ -std=c++17 -O2 -shared -static -I include \
    "${sources[@]}" xgameruntime.def -o xgameruntime.dll \
    -lole32 -luuid -liphlpapi -lws2_32 -lwinhttp -lcrypt32 -lbcrypt
