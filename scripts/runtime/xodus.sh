#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/runtime-env.sh"
exec "$MSFS_LINUX_ROOT/bin/xodus-cli" "$@"
