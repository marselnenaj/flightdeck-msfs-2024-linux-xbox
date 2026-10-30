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

## Experimental Proton selection

Open **Setup → Proton version (experimental)**. Flightdeck discovers installed
Proton builds in Steam libraries, including Flatpak Steam and
`compatibilitytools.d`. You can also enter a complete Wine/DXVK/VKD3D runner
folder. Steam or your existing Proton installer manages its downloads.
The selector includes **Flightdeck (Xodus, default)** and preselects the active
environment when available. Choose the default entry to return from another
Proton build; the separate recovery button remains available as well.

Switching carries the **current** Windows profile forward, including new
aircraft, add-ons, sign-ins and settings. Returning to Flightdeck does the same.
Each unchanged Proton build now has one reusable private runner copy under
`local/proton-tests/`; selecting the already active build is a no-op. Updating
that build's files, or changing its Fenix overlay, creates a separate cache
entry. The base game and Flightdeck-managed saves remain in place, and
Store/account helpers retain the original Flightdeck runner.

Preparation still stages the current profile so a failed preparation cannot
damage the active one. After a successful switch, Flightdeck keeps the last
profile backup and reclaims older, unreferenced generated profiles. It also
audits recognized copies from earlier releases when idle at startup. Fenix
restore points and references from other configured simulators remain
protected. Changed profiles, unknown files, mounted subdirectories and an
incomplete process/reference audit defer cleanup. Deletion uses a private
quarantine and never follows package-directory links. Never delete
`proton-tests` wholesale: active runners or restore points may still be inside.

From 0.3.7, an old profile that fails the deletion inventory is retained without
a cleanup fingerprint. That failure no longer strands a completed switch or
journal recovery. Returning to Flightdeck remains possible; the unverified
backup is excluded from automatic reclamation. Ownership and mount checks on
active profile operations remain in force.

Preparation verifies the selected files, updates Wine and installs that runner's
graphics libraries for both architectures. Steam updates cannot modify the
private runner copy; select the updated build again to adopt it. Failure or
cancellation during preparation leaves the active profile untouched. A journal
blocks launch after an interrupted commit; **Return to Flightdeck environment**
recovers it. Filesystems such as `ntfs3` that reject atomic directory exchange
use three journaled, identity-checked renames instead. An interruption after any
rename can be resumed without discarding either profile. **Check session**
remains available when missing runner files disable the cloud transport; then
use **Return to Flightdeck environment** to repair the runner selection.
Close MSFS and all profile applications before switching.

For bundled media stacks, Flightdeck checks whether the H.264 decoder loads in
a separate process. If it fails, a working host decoder and the runner's ability
to start with that environment must both be verified before using the host
stack. Explicit GStreamer plugin/rank settings are preserved. This checks
loading dependencies, not every video or GPU configuration.

The Xbox user bridge also needs working WinRT JSON parsing. Some stock Proton
builds expose its factories but return `E_NOTIMPL` from `Parse` or `GetObject`,
causing sign-in error `000-2000-0x80004001` even after a successful HTTP response.
Flightdeck copies the matching default runner's `windows.web.dll` and
`wintypes.dll` into the private profile as `fd-json.dll` and `fd-types.dll`,
including unique internal PE export names. Only the three JSON classes and
`Windows.Foundation.Collections.PropertySet` are registered to those copies.
The chosen runner's system libraries remain intact. Existing selected profiles
receive this repair before the next launch; source hashes, private-copy hashes
and registrations are checked so interrupted preparation or a Wine registry
update can be repaired. Real MSFS 2024 sign-in and token/signature requests were
verified with Experimental `experimental-11.0-20261001-x86_64`; this does not
establish a complete flight. The isolated CachyOS 10 game test still exited
with code 84 before authentication completed, despite passing the synthetic
JSON, graphics and profile-return checks.

With Flightdeck **0.2.20** and CachyOS **11.0-20261015**, the
[issue #4 reporter](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/4#issuecomment-6377721086)
reaches Start after waiting through a black loading screen with music, then
sees the rendered menu and a scene in the simulator. The earlier log from that
runner reported missing H.264 decoding and unresolved dependencies for bundled
media plugins. A current media diagnosis is still needed to establish whether
the same failures persist. Waiting is a reported startup workaround; it does
not repair video playback, qualify a complete flight or resolve the separate
one-third-rate VSync report. See [known issues](known-issues.md).

Fenix uses a separate overlay compiled for each exact supported Wine build.
The additional builds are Experimental `experimental-11.0-20260924-x86_64`,
CachyOS `cachyos-10.0-sunset-slr` and CachyOS `cachyos-11.0-20261005-slr`. The
last one passed the overlay's regression probes and a fresh profile setup; its
switch inside Flightdeck has not been run with the simulator. The menu marks compatible versions and
Flightdeck applies their patch automatically. An unknown or modified Wine build
is rejected before switching a Fenix installation. Known legacy Fenix profiles
can migrate without reinstalling aircraft. GSX setup and its official installer
can run on the selected Proton; an interrupted GSX setup must be recovered first.

The alternative launch bridge exposes Xodus' already-open, licensed image
descriptors through temporary `/proc` symlinks. Executables and DLLs remain in
memory; encrypted game files are not rewritten. This removes the need for the
custom `WINE_DLL_FILE_MAP` Wine extension for these trials. The executable and
working directory both use that view so working-directory DLL loads can access
the licensed images too. Xodus still performs
normal licensing, and Flightdeck retains its Store/GDK libraries. Flightdeck
starts the chosen Wine directly; Steam's Proton launcher and its game-specific
launch options are not invoked. Unsupported host dependencies fail preparation.

The 0.2.1 default loader also exposes mapped DLLs through this view and
uses it as the working directory. Its executable still uses the native Wine
mapping and a header-only stub. The pinned native mapping handles the main
image, so ordinary delayed DLL loads cannot rely on map aliases alone.
Maintainers can exercise that exact path with `cargo run --locked -p
flightdeck-tools -- probe proton --runtime /path/to/runtime --default-runner
--native-launcher target/debug/flightdeck-rust --output /path/to/new-results`;
use `--runner /path/to/proton` instead for selection and return tests. Both use
own synthetic programs in fresh prefixes and leave the source runtime unchanged.
Build the launcher and the `runtime-lab` executable first, as described in
[BUILDING.md](../BUILDING.md#synthetic-native-checks).

This is experimental compatibility support. Native overlay, loader, graphics and
profile-switch checks do not establish a complete Fenix/GSX flight or fix the
reported NVIDIA black view. Return to Flightdeck before resetting or removing an
environment. Diagnostics record the chosen version and loader. Maintainers can
add `--fenix-bundle /path/to/patch` to that `probe proton` command to verify the
matching overlay, synthetic Store loading, rendering and add-on-preserving
return without starting a game or signing in.

The Proton probe also initializes the real Xbox runtime with synthetic title
metadata, obtains its user interface, parses synthetic Xbox response JSON
(including nested identity arrays) and shuts it down. A local broker accepts
only ping requests; the probe does not sign in or contact Xbox services. Its
`xbox_initialization_passed` and `xbox_json_passed` results do not establish
in-game authentication.
Safe diagnostics schema 7 preserves runtime initialization, user-load stages
and authentication-stage/user API result codes in exported JSON and problem reports. Existing game
logs can be re-exported with `flightdeck diagnose-run --runtime /path/to/runtime`.
These are distinct observed results, not a chronological event trace; missing
entries do not establish that an API was never called.

## Advanced: prepare a runtime from existing files

The installed launcher provides the same preparation logic under **Install MSFS
→ Prepare a new runtime**. Select the edition and input folders, review the checks and
start setup. Folder picker buttons appear when zenity or kdialog is available;
paths can always be entered directly. A new runtime is built in a temporary
directory and published only after successful verification. Cancelling removes
that operation's temporary copy. No source prefix, game or runner is overwritten.
The existing-runtime option checks an already prepared folder and connects it.

The guided interface uses `native/setup.rs` for artifact checks, the runner
fingerprint and the rule that existing destinations cannot be overwritten.
Close any application using the source Wine prefix before copying it.

Supply your own Xodus-compatible Proton runner, game downloaded using your
purchased account, and initialized x64 Wine prefix with working graphics.
Runner revisions are pinned in `compat/upstreams.lock.json`. The tested
original `xgameruntime.dll` is a free Wine builtin from that runner; the source
repository deliberately does not distribute it. A different original DLL is
rejected because the proxy's private interface layout was checked against the
pinned version. A normal upstream Wine build lacks the required
`WINE_DLL_FILE_MAP` loader extension.

Start the Rust launcher with `target/debug/flightdeck-rust` and choose
**Install MSFS → Prepare a new runtime**. Supply the artifact directory,
owned game directory, runner, prepared Wine prefix, new destination and Store
country. The form supports both MSFS 2024 and MSFS 2020. The obsolete Python
staging command has been removed.

Setup verifies artifact hashes and the runner ABI fingerprint, copies
the prefix using reflinks where available, links the supplied game and runner,
and installs the newly built components into the copy. It refuses an existing
destination. The original prefix, runner and game content are not rewritten.
The result is local user data, including the copied prefix; never publish it.
No login, game start, download or package installation occurs during staging.
The configured market is also used for public Store catalog prices. Product
text follows the Wine user locale; this setting does not select or change the
authenticated Microsoft Store account.

The local-save option creates
`private/local-saves.enabled` with mode0600 and its storage directory with
mode0700. The startup wrapper uses this gate to set the native provider option.
Normal launcher starts enable this provider automatically for cloud-backed
sessions. The launcher loads cloud saves before play and uploads local changes
after the game exits, keeping local backups. It does not merge live game writes
in the middle of a session; see [cloud saves](cloud-saves.md).
The implementation rejects `syncOnDemand=true`; the observed game uses false.
Keep the entire local-saves directory when backing up or moving this runtime.

The game requires native Vulkan/DXVK/VKD3D setup in the supplied prefix and
compatible GStreamer codecs for startup video. The media-plugin directory field
adds an already prepared plugin directory without modifying the system. Codec
ABI and driver availability are host-specific. The source builder does not
download or install graphics, multimedia or system packages.

Flightdeck applies the game's media environment to its Wine child and disposable
verification children. Account and cloud helpers use their separate environment. The default
Xodus runner uses system GStreamer codecs: MP4 needs the ISO/QuickTime demuxer
and a working H.264 decoder, normally `qtdemux` and `avdec_h264`. Its bundled
Steam media converter depends on a transcoded-video cache that Flightdeck does
not have; adding its plugin paths can replace real video with a small test
pattern. The optional local plugin directory is used only with this host-codec
path and must match the system GStreamer ABI.

A selected Proton that supplies its own FFmpeg codec library receives its
library directories in `LD_LIBRARY_PATH` and its GStreamer plugin directories
when present. This
also makes GE-Proton's FFmpeg-backed Media Foundation implementation available.
Stock Steam builds without this library retain the host-codec path. The
original library path is retained for Wine's native-program handoff. These
settings do not reach the Store service, broker or companion applications.

On NVIDIA, H.264 decoding prefers `avdec_h264` with rank 512 when no explicit
H.264 feature rank was supplied. It leaves hardware decoders available if the
software decoder is missing. This changes video decoding, independently of the
simulator's NVIDIA graphics mode. Explicit media paths, debug levels and decoder
choices are retained. GStreamer registries are separate for host codecs and
each bundled runner, so switching cannot reuse another build's registry.

The local game log records the intended media source and decoder preference.
Safe diagnostics export only these categories and counts of known decode,
missing-plugin and placeholder-video messages; they contain no raw media paths
or codec data. Configuration is not proof of decoded or displayed frames. The
maintainer `probe media` check described in `BUILDING.md` reads a real local
video, validates its decoded dimensions and rewinds it without starting MSFS.

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

The Rust Wine bridge adds `-FastLaunch` when starting MSFS
2020/2024, independently of the graphics mode or selected runner. This uses the
[reported intro-path workaround](nvidia-renderer.md#intro-startup-workaround).
An explicitly supplied spelling is retained once; other arguments are preserved.
`FLIGHTDECK_FAST_LAUNCH=0` in the launch environment disables automatic insertion
for troubleshooting. With the desktop launcher this variable must be set in the
service's environment; unrelated terminal/Steam launch options do not change a
running service. Normal users receive the default automatically after updating.

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
0.1.7 selects that NVIDIA device using `DXVK_FILTER_DEVICE_UUID`. The native
Vulkan device UUID survives Wine's translation of display names and vendor
identification. DirectX 12 receives the physical adapter from DXGI, including
when the application requests its default adapter. Flightdeck does not add a
VKD3D host-name filter or assume that Linux and Wine enumerate GPUs in the same
order. If a unique device UUID is unavailable, it leaves adapter selection to
the runner. Explicit device selections are retained; multiple discrete GPUs are
not automatically narrowed to one. Hardware UUIDs are never exported in
diagnostics or persisted in the launch-settings record.
AMD/Intel-only launch environments are unchanged. A failed NVIDIA Vulkan check
stops launch with a driver message.

Flightdeck 0.1.6 adds **Setup → NVIDIA graphics**. Its
per-installation choice is stored in `private/graphics-settings.json` and read
for each launcher-managed start. See the [user guide](graphics.md).

- `auto`: from 0.1.11, use the same conservative profile as `compatibility`.
  Existing automatic preferences adopt this profile on the next start.
- `compatibility`: disable NVAPI, optical flow and NGX loading; hide the NVIDIA
  vendor in Wine and DXGI; exclude `VK_NV_low_latency2` in VKD3D; set
  `dxvk.disableNvLowLatency2 = True` and `dxvk.latencySleep = False` in DXVK.
  DLSS, Reflex and NVIDIA Frame Generation are unavailable. The bundled DXVK
  corrects the ignored extension opt-out in the pinned upstream revision.
- `features`: prepare the runner's NVIDIA components and host NGX, retaining
  explicit environment overrides. This is the previous automatic behavior,
  now an explicit experimental choice. Existing DLLs remain on disk.

Both automatic and compatibility settings take precedence over conflicting
feature-enabling variables. Unrelated options and explicit device selections
remain intact. Features mode honors inherited variables; remove explicit disable
settings from the launcher's service environment to enable NVIDIA features.
Flightdeck translates `PROTON_HIDE_NVIDIA_GPU=1` for Wine and DXGI.
`PROTON_DISABLE_NVAPI=1` or `DXVK_ENABLE_NVAPI=0` blocks NVAPI and optical-flow
loading even when an earlier start installed these libraries.
Generic Steam launch options do not configure Flightdeck's existing background
service. Changing the mode in Flightdeck requires no service restart.

The 0.2.1 launcher also reconciles MSFS 2024 `UserCfg.opt` Video options
before starting Wine, under the runtime lease. NVAPI-disabled starts replace
DLSS with TAA, Reflex with OFF and DLSSG with NONE, including the VR variants.
Only known values in one unambiguous Video block are eligible. The per-field
undo record, `.flightdeck-nvidia-settings.json` next to the configuration,
contains only the original option values and travels with Proton profile
copies. Enabling features restores only values still matching the managed
replacement. The write is atomic and follows the durable undo record; retry
recovers an interrupted apply or restore. Unknown or linked files are retained.

The Vulkan check, DXGI/D3D12 adapter selection and GLVND setup also run with
NVAPI disabled. A single GPU name filter that uniquely matches a Vulkan adapter
is completed for the other graphics API. Explicit filters for both APIs, UUIDs,
device indices and ambiguous matches are preserved without guessing.
Explicit name filters must match the names visible inside Wine. Remove custom
name filters before comparing modes if diagnostics reports that the
desired adapter was excluded.
Every start defaults to `DXVK_LOG_LEVEL=info` and `VKD3D_DEBUG=warn` so the local
game log includes renderer versions, adapter decisions, the created Direct3D 11
device and each swapchain. Until 0.2.11 only NVIDIA starts did, and the log of a
Direct3D 11 game on other hardware could not show whether anything was drawn.
VKD3D orders `info` below `warn`; earlier launcher defaults suppressed warnings. Explicit logging
preferences remain effective. These settings do not enable per-call tracing.

**Diagnostics** reports Vulkan adapters, API/driver versions and the desktop
session type. `graphics.status: ready` means that the Linux probe can enumerate
a hardware adapter; it does not prove that Wine initialized Direct3D or that
the game rendered. A listed software adapter such as llvmpipe is not evidence
that the game selected it. The probe runs in a separate process with a deadline
and does not require `vulkaninfo`.

Flightdeck 0.1.7 provides diagnostics schema 4, retaining these graphics fields:

Flightdeck 0.1.9's schema 5 correction additionally scans the middle of logs and
exports token/signature, networking-policy and audio evidence. See
[the 0.1.8 follow-up and offline exporter](diagnostic-follow-up.md) for bounds,
privacy, and the symptoms that still require tester evidence.

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
- `graphics.log`: known component markers, bounded renderer versions and
  Vulkan/DXGI error symbols found in the first/last log excerpt. `observations`
  counts presents without rendering, name/UUID filter skips and missing DXVK
  adapters in that excerpt, not throughout the whole run. An occasional blank
  present can be valid; its presence alone does not identify a driver fault.
  Filter skips can also be intentional, such as excluding integrated graphics;
  they are distinct from a failure to find any usable adapter.
  Empty results do not rule out a graphics failure or prove a renderer was never loaded.

Schema 4 also includes the latest Store check and timestamped Store-session
events with component hashes recorded at game launch. See
[Store diagnostics](marketplace-collections.md) for scope and interpretation.

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
compatibility layer. Its read-only Store queries
can return supported consumable/Durable products, public desktop prices and actual
Store-account collection data. Owned add-ons can be enumerated for the current
title and supported Durable handles require signed license grants.
The catalog must establish the game's association
and the supported product shape. Unverified ownership and unsupported product
shapes produce an error. See [the Collections contract](marketplace-collections.md)
for the scope and validation level.

Full DLC coverage remains unverified. Flightdeck 0.1.7 adds a
Microsoft-hosted purchase dialog whose display has been checked in MSFS 2024.
Completed purchases and delivery of purchased content remain unverified;
device-shared DLC rights and the Store consumable-fulfillment API remain
unsupported. See [purchase-dialog scope](marketplace-collections.md#purchase-dialog).
The launcher supports full-package updates, integrity checks and
repairs; see [game updates](game-updates.md). Cloud saves synchronize automatically
before a managed game starts and after it exits, with local backups and explicit
conflict choices. The local save provider does not synchronize during gameplay.
The actual runner, driver, codecs, game version and service contracts can change
independently of this repository.
