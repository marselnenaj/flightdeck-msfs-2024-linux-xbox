#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
umask 077
MSFS_LINUX_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
export MSFS_LINUX_ROOT
: "${XDG_RUNTIME_DIR:?Start from your graphical Linux session}"
flightdeck_socket_id=$(printf '%s' "$MSFS_LINUX_ROOT" | sha256sum | cut -c1-16)
export XODUS_USER_SOCKET_SUFFIX="flightdeck-$flightdeck_socket_id/xodus.sock"
export FLIGHTDECK_SOCKET_DIR="$XDG_RUNTIME_DIR/flightdeck-$flightdeck_socket_id"
mkdir -p -m 700 "$FLIGHTDECK_SOCKET_DIR" "$MSFS_LINUX_ROOT/private"
export XDG_CONFIG_HOME="$MSFS_LINUX_ROOT/private/xdg/config"
export XDG_DATA_HOME="$MSFS_LINUX_ROOT/private/xdg/data"
export XDG_CACHE_HOME="$MSFS_LINUX_ROOT/private/xdg/cache"
export XDG_STATE_HOME="$MSFS_LINUX_ROOT/private/xdg/state"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
export RUST_LOG=warn XODUS_LOG=warn
