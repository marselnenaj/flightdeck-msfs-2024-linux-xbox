#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
flightdeck_locale=${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}
case "$flightdeck_locale" in
  [dD][eE]|[dD][eE][_.@-]*) flightdeck_language=de ;;
  *) flightdeck_language=en ;;
esac
flightdeck_arguments=("$@")
for ((flightdeck_index=0; flightdeck_index < ${#flightdeck_arguments[@]}; flightdeck_index++)); do
  case "${flightdeck_arguments[flightdeck_index]}" in
    --language)
      flightdeck_selected=${flightdeck_arguments[flightdeck_index+1]:-}
      ((flightdeck_index+=1))
      ;;
    --language=*) flightdeck_selected=${flightdeck_arguments[flightdeck_index]#--language=} ;;
    *) continue ;;
  esac
  if [[ "$flightdeck_selected" != de && "$flightdeck_selected" != en ]]; then
    if [[ "$flightdeck_language" == de ]]; then
      printf '%s\n' 'Bitte --language de oder --language en wählen.' >&2
    else
      printf '%s\n' 'Choose --language de or --language en.' >&2
    fi
    exit 2
  fi
  flightdeck_language=$flightdeck_selected
done
flightdeck_source=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if ! command -v python3 >/dev/null 2>&1 || ! python3 -c 'import sys; sys.exit(sys.version_info < (3, 10))'; then
  if [[ "$flightdeck_language" == de ]]; then
    printf '%s\n' 'Flightdeck benötigt Python 3.10 oder neuer. Bitte installiere Python über deine Linux-Paketverwaltung.' >&2
  else
    printf '%s\n' 'Flightdeck requires Python 3.10 or newer. Install Python using your Linux package manager.' >&2
  fi
  exit 1
fi
exec python3 "$flightdeck_source/scripts/install-launcher.py" "$@"
