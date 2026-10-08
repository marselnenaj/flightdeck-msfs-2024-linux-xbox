#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
flightdeck_locale=${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}
case "$flightdeck_locale" in
  [dD][eE]|[dD][eE][_.@-]*) flightdeck_language=de ;;
  *) flightdeck_language=en ;;
esac
flightdeck_arguments=("$@")
flightdeck_gui=false
for ((flightdeck_index=0; flightdeck_index < ${#flightdeck_arguments[@]}; flightdeck_index++)); do
  case "${flightdeck_arguments[flightdeck_index]}" in
    --gui) flightdeck_gui=true; continue ;;
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
flightdeck_fail() {
  local flightdeck_message=$2
  local flightdeck_details=${3:-}
  local flightdeck_dialog
  flightdeck_details=${flightdeck_details:0:4096}
  if [[ "$flightdeck_language" == de ]]; then
    flightdeck_message=$1
  fi
  printf '%s\n' "$flightdeck_message" >&2
  flightdeck_dialog=$flightdeck_message
  if [[ -n "$flightdeck_details" ]]; then
    printf '%s\n' "$flightdeck_details" >&2
    flightdeck_dialog+=$'\n\n'"$flightdeck_details"
  fi
  # Keep loader details bounded and literal, including in graphical errors.
  if [[ "$flightdeck_gui" == true && ( -n ${DISPLAY:-} || -n ${WAYLAND_DISPLAY:-} ) ]] && command -v timeout >/dev/null 2>&1; then
    if command -v zenity >/dev/null 2>&1; then
      timeout -k 1 20 zenity --error --no-markup --title Flightdeck --text "$flightdeck_dialog" >/dev/null 2>&1 || true
    elif command -v kdialog >/dev/null 2>&1; then
      flightdeck_dialog=${flightdeck_dialog//&/\&amp;}
      flightdeck_dialog=${flightdeck_dialog//</\&lt;}
      flightdeck_dialog=${flightdeck_dialog//>/\&gt;}
      flightdeck_dialog=${flightdeck_dialog//$'\n'/<br/>}
      timeout -k 1 20 kdialog --title Flightdeck --error "<p>$flightdeck_dialog</p>" >/dev/null 2>&1 || true
    fi
  fi
  exit 1
}
flightdeck_source=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ ! -x "$flightdeck_source/bin/flightdeck" ]]; then
  flightdeck_fail \
    'Das native Flightdeck-Programm fehlt oder ist nicht ausführbar. Bitte das vollständige Linux-Paket mit Ausführungsrechten entpacken; aus dem Quellcode zuerst mit cargo build --release bauen und paketieren.' \
    'The native Flightdeck program is missing or is not executable. Extract the complete Linux package with executable permissions; for a source build, first run cargo build --release and package it.'
fi
if command -v uname >/dev/null 2>&1; then
  flightdeck_system=$(uname -s 2>/dev/null) || flightdeck_system=
  flightdeck_arch=$(uname -m 2>/dev/null) || flightdeck_arch=
  if [[ -n "$flightdeck_system" && "$flightdeck_system" != Linux ]] || [[ -n "$flightdeck_arch" && "$flightdeck_arch" != x86_64 && "$flightdeck_arch" != amd64 ]]; then
    flightdeck_fail \
      'Dieses Flightdeck-Paket benötigt Linux auf x86-64. Für andere Betriebssysteme oder Prozessorarchitekturen ist es nicht geeignet.' \
      'This Flightdeck package requires Linux on x86-64. It does not support other operating systems or processor architectures.'
  fi
fi
# Probe the actual package rather than a distribution name or ldconfig cache.
# Missing optional tools must not prevent an otherwise working installation.
if command -v timeout >/dev/null 2>&1; then
  if flightdeck_probe=$(LC_ALL=C timeout -k 1 5 "$flightdeck_source/bin/flightdeck" --version 2>&1 | {
    flightdeck_output=
    IFS= LC_ALL=C read -r -N 4096 flightdeck_output || true
    printf '%s' "$flightdeck_output"
  }); then
    :
  else
    flightdeck_probe_status=$?
    if [[ "$flightdeck_probe_status" == 124 || "$flightdeck_probe_status" == 137 ]]; then
      flightdeck_fail \
        'Die Startprüfung von Flightdeck wurde nicht rechtzeitig abgeschlossen. Es wurde nichts installiert. Bitte das Paket und seinen Speicherort prüfen.' \
        'The Flightdeck startup check did not finish in time. Nothing was installed. Check the package and its location.' "$flightdeck_probe"
    fi
    case "$flightdeck_probe" in
      *GLIBC_*'not found'*)
        flightdeck_fail \
          'Die glibc-Version dieses Systems ist für das Flightdeck-Binärpaket zu alt. Dieses Paket benötigt glibc 2.39 oder neuer. Bitte eine passende Linux-Version verwenden.' \
          'This system has an older glibc than the Flightdeck binary package requires. This package needs glibc 2.39 or newer. Use a compatible Linux version.' "$flightdeck_probe" ;;
      *'error while loading shared libraries'*|*'Error loading shared library'*)
        flightdeck_fail \
          'Eine Linux-Laufzeitbibliothek für Flightdeck fehlt oder kann nicht geladen werden. Bitte die unten genannte Bibliothek über die Softwareverwaltung prüfen. Es wurden keine Systempakete installiert.' \
          'A Linux runtime library required by Flightdeck is missing or cannot be loaded. Check the library named below using your software manager. No system packages were installed.' "$flightdeck_probe" ;;
      *'No such file or directory'*|*'required file not found'*)
        flightdeck_fail \
          'Der Linux-Programmloader für dieses Paket fehlt oder ist nicht verfügbar. Flightdeck benötigt eine glibc-basierte x86-64-Linux-Umgebung. Das unveränderte Paket läuft nicht direkt auf einem reinen musl-System.' \
          'The Linux program loader for this package is missing or unavailable. Flightdeck needs a glibc-based x86-64 Linux environment. The unmodified package does not run directly on a musl-only system.' "$flightdeck_probe" ;;
      *)
        flightdeck_fail \
          'Das native Flightdeck-Programm kann hier nicht starten. Es wurde nichts installiert. Bitte das vollständige Linux-x86-64-Paket, die Ausführungsrechte und die folgenden Details prüfen.' \
          'The native Flightdeck program cannot start here. Nothing was installed. Check the complete Linux-x86-64 package, executable permissions and the details below.' "$flightdeck_probe" ;;
    esac
  fi
fi
exec "$flightdeck_source/bin/flightdeck" install --source "$flightdeck_source" "$@"
