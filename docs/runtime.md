# Local runtime contract

For a first installation, use the full Flightdeck installer and choose **Install
MSFS** in the launcher. It downloads the owned game and prepares Wine/Proton
automatically after the Linux prerequisite checks. Follow the
[installation guide](install.md); no existing game, runner or Wine prefix is
required for that workflow.

This page covers the advanced import and developer staging of existing files.
The launcher accepts a prepared runtime directory. Its required entry point is
`tools/play-msfs.sh`, the isolated prefix is at `local/msfs-prefix`, and optional
local saves are at `private/local-saves`. Each simulator uses a separate runtime:

| `private/runtime.json` game ID | Game executable | Default runtime directory |
| --- | --- | --- |
| `msfs2024` | `games/MSFS2024/FlightSimulator2024.exe` | `~/.local/share/flightdeck/runtimes/msfs2024` |
| `msfs2020` | `games/MSFS2020/FlightSimulator.exe` | `~/.local/share/flightdeck/runtimes/msfs2020` |

The data root follows `XDG_DATA_HOME` when configured. Missing edition metadata
is treated as legacy MSFS 2024; invalid or conflicting identities are rejected.
Selecting an edition changes the runtime, Community inventory, update target and
save storage together. The encrypted package's `.xodus-streaming.msixvc` and
MicrosoftGame.Config must remain alongside the game. Xodus obtains and checks
the user's actual entitlement; the compatibility code does not grant licenses.

## Advanced: prepare a runtime from existing files

The installed launcher provides the same preparation logic under **Install MSFS
→ Prepare a new runtime**. Select the edition and input folders, review the checks and
start setup. Folder picker buttons appear when zenity or kdialog is available;
paths can always be entered directly. A new runtime is built in a temporary
directory and published only after successful verification. Cancelling removes
that operation's temporary copy. No source prefix, game or runner is overwritten.
The existing-runtime option checks an already prepared folder and connects it.

Both the guided interface and the command below use `flightdeck.setup`, so the
artifact checks, runner fingerprint and no-overwrite rules stay consistent.
Close any application using the source Wine prefix before copying it.

Supply your own Xodus-compatible Proton runner, game downloaded using your
purchased account, and initialized x64 Wine prefix with working graphics.
Runner revisions are pinned in `compat/upstreams.lock.json`. The tested
original `xgameruntime.dll` is a free Wine builtin from that runner; the source
repository deliberately does not distribute it. A different original DLL is
rejected because the proxy's private interface layout was checked against the
pinned version. A normal upstream Wine build lacks the required
`WINE_DLL_FILE_MAP` loader extension.

```sh
python3 scripts/stage-runtime.py \
  --artifacts build/compat/artifacts \
  --game "$OWNED_GAME_DIRECTORY" --runner "$RUNNER" \
  --prefix "$PREPARED_WINE_PREFIX" --destination "$NEW_RUNTIME_DIRECTORY" \
  --market "$STORE_COUNTRY_CODE" --local-saves
```

This command-line helper currently stages MSFS 2024. For a prepared MSFS 2020
runtime, select that edition in Flightdeck's setup form; the helper has no
`--game-id` option.

The command verifies artifact hashes and the runner ABI fingerprint, copies
the prefix using reflinks where available, links the supplied game and runner,
and installs the newly built components into the copy. It refuses an existing
destination. The original prefix, runner and game content are not rewritten.
The result is local user data, including the copied prefix; never publish it.
No login, game start, download or package installation occurs during staging.
The configured market is also used for public Store catalog prices. Product
text follows the Wine user locale; this setting does not select or change the
authenticated Microsoft Store account.

The staging command's `--local-saves` option creates
`private/local-saves.enabled` with mode0600 and its storage directory with
mode0700. The startup wrapper uses this gate to set the native provider option.
Normal launcher starts enable this provider automatically for cloud-backed
sessions. The launcher loads cloud saves before play and uploads local changes
after the game exits, keeping local backups. It does not merge live game writes
in the middle of a session; see [cloud saves](cloud-saves.md).
The implementation rejects `syncOnDemand=true`; the observed game uses false.
Keep the entire local-saves directory when backing up or moving this runtime.

The game requires native Vulkan/DXVK/VKD3D setup in the supplied prefix and
compatible GStreamer codecs for startup video. `--media-plugins DIRECTORY`
adds an already prepared plugin directory without modifying the system. Codec
ABI and driver availability are host-specific. The source builder does not
download or install graphics, multimedia or system packages.

## Authentication and launch

Flightdeck 0.1.6 provides a reversible environment reset and a
game uninstaller under **Setup**; see the
[maintenance instructions](install.md#manage-a-game-installation).
A reset atomically exchanges `local/msfs-prefix` with a freshly prepared prefix.
The old prefix remains at `local/environment-backup-…/prefix`; the journal in
`private/environment-reset.json` identifies the last reset that can be undone.
An interrupted journal publication is recoverable from
`private/environment-reset-pending.json`. Save storage under `private` is not
reset. These operations hold the same runtime lease used by game starts and
reject active Wine/Fenix processes.

### NVIDIA graphics in launcher-managed starts

Flightdeck starts Wine directly, so the launcher prepares the NVIDIA pieces
normally installed by [the pinned Proton launcher](https://github.com/xodus-gaming/Proton/blob/7c0b435495814349735c913fde78da906aecea52/proton)
before starting the game, while holding the runtime lock. It installs 32/64-bit
NVAPI and 64-bit optical flow from that runtime's runner, enables their native
DLL overrides and DXVK NVAPI, and disables GLVND dispatch-table patching.
NGX DLLs, when available, come from the installed NVIDIA driver via its loaded
GLX library directory (or an explicit `NVIDIA_WINE_DLL_DIR`). No driver is
downloaded or redistributed. These steps also apply to existing runtimes.

Managed copies follow runner and host-driver updates. Their hashes are recorded
in `private/nvidia-runtime.json`; differing user-supplied DLLs are preserved.
Missing NGX does not prevent ordinary rendering, but DLSS needs the host NGX
components. See [DXVK-NVAPI's requirements](https://github.com/jp7677/dxvk-nvapi).

On a machine with one discrete NVIDIA GPU plus integrated graphics, Flightdeck
selects that NVIDIA device for both DXVK/DXGI and VKD3D. Explicit device selections
are retained; multiple discrete GPUs are not automatically narrowed to one.
AMD/Intel-only launch environments are unchanged. A failed NVIDIA Vulkan check
stops launch with a driver message.

Flightdeck 0.1.6 adds **Setup → NVIDIA graphics**. Its
per-installation choice is stored in `private/graphics-settings.json` and read
for each launcher-managed start. See the [user guide](graphics.md).

- `auto`: prepare the runner's NVIDIA components and retain explicit environment
  overrides.
- `compatibility`: disable NVAPI, optical flow and NGX loading for the process;
  set `WINE_HIDE_NVIDIA_GPU=1` and append `dxgi.hideNvidiaGpu = True` to
  `DXVK_CONFIG`. This disables DLSS and NVIDIA Frame Generation. Existing DLLs
  remain on disk and are available again after switching back to Automatic.

Flightdeck also translates `PROTON_HIDE_NVIDIA_GPU=1` for Wine
and DXGI. `PROTON_DISABLE_NVAPI=1` or `DXVK_ENABLE_NVAPI=0` blocks NVAPI and
optical-flow loading even when an earlier start installed these libraries.
The saved Compatibility mode takes precedence over conflicting feature-enabling
variables. Automatic mode honors inherited variables; remove explicit disable
settings from the launcher's service environment to use NVIDIA features again.
Generic Steam launch options do not configure Flightdeck's existing background
service. Changing the mode in Flightdeck requires no service restart.

The Vulkan check, DXGI/D3D12 adapter selection and GLVND setup also run with
NVAPI disabled. A single GPU name filter that uniquely matches a Vulkan adapter
is completed for the other graphics API. Explicit filters for both APIs, UUIDs,
device indices and ambiguous matches are preserved without guessing.

**Diagnostics** reports Vulkan adapters, API/driver versions and the desktop
session type. `graphics.status: ready` means that the Linux probe can enumerate
a hardware adapter; it does not prove that Wine initialized Direct3D or that
the game rendered. A listed software adapter such as llvmpipe is not evidence
that the game selected it. The probe runs in a separate process with a deadline
and does not require `vulkaninfo`.

Flightdeck 0.1.6 provides diagnostics schema 2 with:

- `context`: launcher version, selected simulator, last run-log modification
  time and `cloud_sync_scope: current_service`. Cloud state describes the current
  service, not necessarily the recorded game run.
- `graphics.last_start_attempt`: sanitized launch settings persisted in
  `private/graphics-launch.json`, including attempt time, launcher version,
  simulator, preparation/spawn state, requested GPU-filter categories and
  NVIDIA mode, vendor-hiding flag and NVAPI/DLL override modes. Writing this record is best effort and does not
  prevent launch. It survives a service restart; the older `last_launch` field
  exists only in service memory. Neither field proves successful rendering.
- `graphics.prefix`: read-only comparisons of current graphics DLLs against
  the selected runner, distinguishing matching native files, known Wine
  builtins, different files and missing/unavailable files. Known global and
  game-specific registry override modes are included. NGX presence alone does
  not establish a working driver bridge or DLSS.
- `graphics.log`: known component markers and Vulkan/DXGI error symbols found
  in the bounded first/last log excerpt. Empty results do not rule out a graphics
  failure or prove a renderer was never loaded.

Compare the attempt and log times: a direct script start or later prefix reset
may leave an older launcher attempt beside newer logs/files. The export omits
paths, device UUIDs, arbitrary registry values and raw log lines. Direct
execution of `tools/play-msfs.sh` bypasses launcher preparation and attempt
recording and requires an already prepared graphics environment.

### Xbox sign-in and game process

Run the prepared `tools/xodus.sh login` for a normal interactive Microsoft
login if needed. The broker and CLI use Linux Secret Service on the current
desktop D-Bus session. No password/token file belongs in the source repository.
Run `tools/play-msfs.sh` from your graphical Linux session, or select the
prepared directory in Flightdeck. The wrapper creates an isolated per-runtime
IPC socket beneath XDG_RUNTIME_DIR, starts its broker, forwards termination
signals, and returns the actual game process exit status. Private logs remain
under the runtime's `private/` directory.

The loader wrapper preserves Xodus's inherited memory-file descriptors and the
simulator's executable basename in a temporary launch directory, with resource
aliases alongside it. The sparse executable contains only PE headers. It does not
write a decrypted executable or obtain a license itself. Temporary launch
headers and aliases are removed when the wrapper exits. The game and prefix paths must
match this runtime configuration.

## Optional Fenix profile

Fenix support is selected explicitly under **Mods → Fenix A320** for MSFS 2024.
The patch creates its own runner/profile copies and records install/restore
state in `private/fenix-linux-patch.json`. Its original runner, Windows profile
and launch scripts remain available for restore. Launch reads the completed
patch state before enabling the Fenix compatibility settings and window helper.
An interrupted transaction blocks game launch until recovered.

The `private/fenix-compat.json` marker belongs to earlier local development
setups. Flightdeck recognizes it and does not automatically replace that setup.
Do not remove a marker to bypass runner checks. For an installation test, use a
separate compatible runtime and separate launcher state:

```sh
flightdeck --runtime "$TEST_RUNTIME_DIRECTORY" --state-dir "$TEST_LAUNCHER_STATE"
```

The runtime must already be independently prepared; `--state-dir` alone does
not copy it. See [Fenix installation and recovery](addons.md#fenix-a320).

## Current limits

Native play has reached a cockpit in development. This is an experimental
compatibility layer with a bounded read-only Store integration: explicit queries
can return supported consumable/Durable products, public desktop prices and actual
Store-account collection data. Owned add-ons can be enumerated for the current
title and supported Durable handles require signed license grants.
The catalog must establish the game's association
and the supported product shape. Unverified ownership and unsupported product
shapes produce an error. See [the Collections contract](marketplace-collections.md)
for the scope and validation level.

Full DLC coverage remains unverified; purchase dialogs, device-shared DLC rights
and consumable fulfillment are unsupported. The launcher supports full-package updates, integrity checks and
repairs; see [game updates](game-updates.md). Cloud saves synchronize automatically
before a managed game starts and after it exits, with local backups and explicit
conflict choices. The local save provider does not synchronize during gameplay.
The actual runner, driver, codecs, game version and service contracts can change
independently of this repository.
