#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/runtime-env.sh"
export XDG_RUNTIME_DIR="$FLIGHTDECK_SOCKET_DIR"
exec "$MSFS_LINUX_ROOT/bin/xodus-service" "$@"
