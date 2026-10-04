#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
flightdeck_tools=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$flightdeck_tools/flightdeck-helper" play --runtime "$flightdeck_tools/.." "$@"
