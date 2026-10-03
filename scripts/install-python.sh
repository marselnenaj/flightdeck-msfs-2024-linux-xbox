#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Packaging template: transition-release.py installs this as the root install.sh.
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
if ! command -v python3 >/dev/null 2>&1 || ! python3 -c 'import sys; sys.exit(sys.version_info < (3, 10))' >/dev/null 2>&1; then
  if [[ "$flightdeck_language" == de ]]; then
    printf '%s\n' 'Dieses Zwischenupdate benötigt Python 3.10 oder neuer.' >&2
  else
    printf '%s\n' 'This transition update requires Python 3.10 or newer.' >&2
  fi
  exit 1
fi
exec python3 "$flightdeck_source/scripts/install-launcher.py" --source "$flightdeck_source" "$@"
