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
if [[ ! -x "$flightdeck_source/bin/flightdeck" ]]; then
  if [[ "$flightdeck_language" == de ]]; then
    printf '%s\n' 'Das native Flightdeck-Programm fehlt. Bitte das vollständige Linux-Paket entpacken; aus dem Quellcode zuerst mit cargo build --release bauen und paketieren.' >&2
  else
    printf '%s\n' 'The native Flightdeck program is missing. Extract the complete Linux package; for a source build, first run cargo build --release and package it.' >&2
  fi
  exit 1
fi
exec "$flightdeck_source/bin/flightdeck" install --source "$flightdeck_source" "$@"
