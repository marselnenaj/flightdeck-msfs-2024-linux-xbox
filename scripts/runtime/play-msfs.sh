#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Start one locally prepared Flightdeck runtime.
set -euo pipefail
MSFS_INHERITED_LOCK_FD=
if (( $# )); then
    if (( $# != 4 )) || [[ $1 != --runtime || $3 != --lock-fd || ! $4 =~ ^[1-9][0-9]{0,6}$ ]]; then
        printf '%s\n' 'Invalid managed launch arguments.' >&2
        exit 2
    fi
    MSFS_SELECTED_RUNTIME=$2
    MSFS_INHERITED_LOCK_FD=$4
    # Validate before sourcing the selected runtime. A matching inode alone
    # does not prove this descriptor owns the flock: another descriptor must
    # be excluded and this exact open-file-description must retain ownership.
    python3 - "$MSFS_SELECTED_RUNTIME" "$MSFS_INHERITED_LOCK_FD" <<'PY'
import fcntl, os, stat, sys
from pathlib import Path
try:
    root = Path(sys.argv[1])
    descriptor = int(sys.argv[2])
    if descriptor < 3 or not root.is_absolute() or root.resolve(strict=True) != root:
        raise ValueError()
    private = root / 'private'
    metadata = private.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
        raise ValueError()
    opened = os.fstat(descriptor)
    expected = (private / 'play.lock').lstat()
    if (not stat.S_ISREG(opened.st_mode) or not stat.S_ISREG(expected.st_mode)
            or opened.st_uid != os.getuid() or opened.st_mode & 0o077 or opened.st_nlink != 1
            or (opened.st_dev, opened.st_ino) != (expected.st_dev, expected.st_ino)):
        raise ValueError()
    probe = os.open(private / 'play.lock', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        try:
            fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            pass
        else:
            raise ValueError()
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    finally:
        os.close(probe)
    for name in ('runtime-env.sh', 'launch-msfs.sh', 'xodus-service.sh'):
        info = (root / 'tools' / name).lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
            raise ValueError()
except (OSError, ValueError):
    print('The runtime does not support this managed launch.', file=sys.stderr)
    raise SystemExit(2)
PY
    if (( MSFS_INHERITED_LOCK_FD != 9 )); then
        exec 9>&"$MSFS_INHERITED_LOCK_FD"
        exec {MSFS_INHERITED_LOCK_FD}>&-
    fi
    # Socket selection belongs to this runtime, not the launcher's inherited
    # environment. Modern runtime-env.sh declares both values itself.
    unset FLIGHTDECK_SOCKET_DIR XODUS_USER_SOCKET_SUFFIX
    source "$MSFS_SELECTED_RUNTIME/tools/runtime-env.sh"
    if [[ $MSFS_LINUX_ROOT != "$MSFS_SELECTED_RUNTIME" ]]; then
        printf '%s\n' 'The selected runtime environment does not match.' >&2
        exit 2
    fi
else
    source "$(dirname -- "${BASH_SOURCE[0]}")/runtime-env.sh"
    exec 9>"$MSFS_LINUX_ROOT/private/play.lock"
    if ! flock -n 9; then
        printf '%s\n' 'This MSFS launcher is already running.' >&2
        exit 1
    fi
fi
unset MSFS_INHERITED_LOCK_FD
: "${XDG_RUNTIME_DIR:?Start from your graphical Linux session}"
if [[ -z ${FLIGHTDECK_SOCKET_DIR:-} ]]; then
    # Older prepared runtimes use Xodus's default socket directly under XDG.
    # A custom suffix without its corresponding directory is not that legacy
    # contract; do not guess a path which could refer to another service.
    if [[ -n ${XODUS_USER_SOCKET_SUFFIX:-} ]]; then
        printf '%s\n' 'The runtime socket directory is unavailable.' >&2
        exit 2
    fi
    FLIGHTDECK_SOCKET_DIR=$XDG_RUNTIME_DIR
fi
MSFS_RUN_DIR=$(mktemp -d "$MSFS_LINUX_ROOT/private/run-$(date +%Y%m%d-%H%M%S)-XXXXXX")
MSFS_SERVICE_PID=
MSFS_GAME_PID=
MSFS_GAME_FINISHED=0
MSFS_WAIT_INTERRUPTED=0
start_child() {
    # Bash background jobs inherit ignored SIGINT. Reset it before exec, and
    # give this invocation its own process group for bounded cleanup.
    exec python3 -c 'import os, signal, sys
signal.signal(signal.SIGINT, signal.SIG_DFL)
signal.signal(signal.SIGTERM, signal.SIG_DFL)
os.setsid()
os.execv(sys.argv[1], sys.argv[1:])' "$@"
}
owned_process_alive() {
    kill -0 -- "-$1" 2>/dev/null || kill -0 "$1" 2>/dev/null
}
signal_owned_process() {
    kill -s "$2" -- "-$1" 2>/dev/null || kill -s "$2" "$1" 2>/dev/null || true
}
stop_owned_process() {
    local pid=$1 initial_signal=${2:-INT} attempt
    signal_owned_process "$pid" "$initial_signal"
    for ((attempt=0; attempt<40; attempt++)); do
        if ! owned_process_alive "$pid"; then break; fi
        sleep 0.1
    done
    if owned_process_alive "$pid"; then
        signal_owned_process "$pid" TERM
        for ((attempt=0; attempt<10; attempt++)); do
            if ! owned_process_alive "$pid"; then break; fi
            sleep 0.1
        done
    fi
    if owned_process_alive "$pid"; then signal_owned_process "$pid" KILL; fi
    wait "$pid" 2>/dev/null || true
}
stop_fenix_companions() {
    # Wine companions can detach from the game's Unix process group. Keep the
    # runtime lease until their bounded shutdown has finished. Never use pkill
    # or wineserver -k here: installers and other Wine profiles remain independent.
    python3 - "$MSFS_LINUX_ROOT" 9>&- <<'FENIX_CLEANUP'
import os, select, signal, subprocess, sys, time
from pathlib import Path

root = Path(sys.argv[1]).resolve()
prefix = (root / 'local/msfs-prefix').resolve()
names = {'fenix.exe', 'fenixbootstrapper.exe', 'fenixsystem.exe',
         'fenixdisplay.exe', 'fenixcdu.exe', 'fenixwizzard.exe',
         'fenix.gqlgateway.exe', 'fenixwindowguard.exe'}
# The official FenixApp/installer, other aircraft and generic Wine services are
# deliberately outside the session's companion list.
handles = {}
poller = select.poll()

def collect():
    for proc in Path('/proc').iterdir():
        if not proc.name.isdecimal() or int(proc.name) in handles:
            continue
        fd = None
        try:
            if proc.stat().st_uid != os.getuid():
                continue
            # Hold the process identity before inspecting it, so a recycled PID
            # can never receive a later signal intended for a departed helper.
            fd = os.pidfd_open(int(proc.name))
            values = (proc / 'environ').read_bytes().split(b'\0')
            found = next((v[11:] for v in values if v.startswith(b'WINEPREFIX=')), None)
            if found is None or Path(os.fsdecode(found)).resolve() != prefix:
                continue
            command = (proc / 'cmdline').read_bytes().split(b'\0', 1)[0]
            name = os.fsdecode(command).strip().strip('"').replace('\\', '/').rsplit('/', 1)[-1].casefold()
            if name not in names:
                continue
            poller.register(fd, select.POLLIN)
            handles[int(proc.name)] = (fd, name)
            fd = None
        except (OSError, ValueError):
            continue
        finally:
            if fd is not None:
                os.close(fd)

def live():
    exited = {fd for fd, _ in poller.poll(0)}
    return [(fd, name) for fd, name in handles.values() if fd not in exited]

def wait_for_exit(seconds):
    deadline = time.monotonic() + seconds
    while live() and time.monotonic() < deadline:
        time.sleep(.05)
        collect()

def send(number):
    for fd, _ in live():
        try:
            signal.pidfd_send_signal(fd, number)
        except ProcessLookupError:
            pass

try:
    collect()
    if handles:
        wine = root / 'runner/files/bin/wine'
        if wine.is_file():
            env = dict(os.environ, WINEPREFIX=str(prefix), WINEDEBUG='-all')
            env.pop('WINE_DLL_FILE_MAP', None)
            env.pop('WINEDLLPATH', None)
            env['WINEDLLOVERRIDES'] = 'winemenubuilder.exe=d'
            # Wine taskkill without /F posts WM_CLOSE. Give Fenix a chance to
            # persist settings before handling hidden/stuck companions below.
            arguments = [str(wine), 'taskkill.exe']
            for name in sorted({name for _, name in live()}):
                arguments.extend(('/IM', name))
            if len(arguments) > 2:
                try:
                    subprocess.run(arguments, env=env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                   close_fds=True, timeout=3, check=False)
                except (OSError, subprocess.TimeoutExpired):
                    pass
        wait_for_exit(2)
        collect()
        send(signal.SIGTERM)
        wait_for_exit(1)
        collect()
        send(signal.SIGKILL)
        wait_for_exit(1)
        print('Fenix session cleanup: %d companion processes, %d still running.' %
              (len(handles), len(live())), flush=True)
        if live():
            raise SystemExit(1)
finally:
    for fd, _ in handles.values():
        os.close(fd)
FENIX_CLEANUP
}
cleanup() {
    local status=$?
    trap '' INT TERM
    if [[ -n "$MSFS_GAME_PID" ]] && (( ! MSFS_GAME_FINISHED )); then
        stop_owned_process "$MSFS_GAME_PID"
    fi
    if [[ -n "$MSFS_GAME_PID" ]]; then
        stop_fenix_companions || printf '%s\n' 'Some Fenix companions could not be closed; check the private launcher log.' >&2
    fi
    if [[ -n "$MSFS_SERVICE_PID" ]]; then stop_owned_process "$MSFS_SERVICE_PID"; fi
    return "$status"
}
interrupt_game() {
    MSFS_WAIT_INTERRUPTED=1
    if [[ -n "$MSFS_GAME_PID" ]]; then
        stop_owned_process "$MSFS_GAME_PID" "$1"
    else
        if [[ $1 == TERM ]]; then exit 143; else exit 130; fi
    fi
}
trap cleanup EXIT
trap 'interrupt_game INT' INT
trap 'interrupt_game TERM' TERM
socket_ready() {
    python3 - "$FLIGHTDECK_SOCKET_DIR/xodus.sock" <<'PY'
import socket, struct, sys
try:
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(0.5)
        connection.connect(sys.argv[1])
        payload = b'MSFS launcher probe'
        connection.sendall(struct.pack('<IHH', 0x58445358, 1, len(payload)) + payload)
        expected = struct.pack('<IHH', 0x58445358, 2, len(payload)) + payload
        actual = b''
        while len(actual) < len(expected):
            part = connection.recv(len(expected) - len(actual))
            if not part:
                raise OSError('Incomplete Xodus ping')
            actual += part
        if actual != expected:
            raise OSError('Invalid Xodus ping')
except OSError:
    raise SystemExit(1)
PY
}
if ! socket_ready; then
    if [[ -e "$FLIGHTDECK_SOCKET_DIR/xodus.sock" ]]; then
        printf '%s\n' 'An Xodus socket exists but is not accepting connections.' >&2
        exit 1
    fi
    start_child "$MSFS_LINUX_ROOT/tools/xodus-service.sh" >"$MSFS_RUN_DIR/service.log" 2>&1 9>&- &
    MSFS_SERVICE_PID=$!
    MSFS_SERVICE_READY=0
    for ((attempt=0; attempt<300; attempt++)); do
        if socket_ready; then MSFS_SERVICE_READY=1; break; fi
        if ! kill -0 "$MSFS_SERVICE_PID" 2>/dev/null; then break; fi
        sleep 0.2
    done
    if (( ! MSFS_SERVICE_READY )); then
        printf 'Xodus service did not start. Private log: %s/service.log\n' "$MSFS_RUN_DIR" >&2
        exit 1
    fi
fi
printf 'Starting MSFS. Private logs: %s\n' "$MSFS_RUN_DIR"
start_child "$MSFS_LINUX_ROOT/tools/launch-msfs.sh" >"$MSFS_RUN_DIR/game.log" 2>&1 9>&- &
MSFS_GAME_PID=$!
MSFS_GAME_STATUS=0
while true; do
    MSFS_WAIT_INTERRUPTED=0
    wait "$MSFS_GAME_PID" && MSFS_GAME_STATUS=0 || MSFS_GAME_STATUS=$?
    if (( ! MSFS_WAIT_INTERRUPTED )); then break; fi
done
MSFS_GAME_FINISHED=1
printf 'MSFS process ended with status %s. Log: %s/game.log\n' "$MSFS_GAME_STATUS" "$MSFS_RUN_DIR"
exit "$MSFS_GAME_STATUS"
