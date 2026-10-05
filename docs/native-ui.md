# Native desktop and add-on management (0.2.4)

All six Flightdeck views now use Rust and iced 0.14: overview, setup, updates,
saves, mods and diagnostics. The previous artwork, Manrope font, icons, colors
and navigation remain. Layout and native controls are reproduced directly;
this is not a webview and does not promise identical browser text rasterization.
German and English work at the supported minimum of 960 × 700 and larger sizes.

The root executable opens the native window by default. The desktop still uses
the existing private local service so closing a window does not terminate a game
or installation. It verifies the service process, lease, release and session
before connecting. After a service replacement it verifies the new private
record again. Mutating commands include the selected installation and current
job/check/plan ID. Stale replies, changed confirmations and incomplete refreshes
cannot enable another action. Service-side reservations remain authoritative.

The UI uses asynchronous requests and a CPU renderer, with no Vulkan context
required for its own window. Core state refreshes every ten seconds while idle and every two seconds during
work or reconnection. Window focus and explicit actions revalidate immediately.
Expensive diagnostic and Community-folder scans run only on their page, at most
every ten seconds unless explicitly refreshed or an add-on removal is active. These checks do not measure
performance inside MSFS; that needs a separate simulator test.

The executable embeds the production interface in `native/ui/`. `ui/` contains
only shared artwork, font and icon assets. The old web implementation is available
in Git history (`v0.2.2:ui/`); it is no longer duplicated in the working tree.
Python remains only in maintainer build/test tools. The
Windows add-ons themselves still require their own .NET/Wine components.

## Native parity and add-on fixes in 0.2.4

Setup automatically discovers installed Steam and custom Proton runners, including
Experimental and CachyOS. The default Flightdeck runner remains selectable. Discovery
returns string labels; current selection is bound to the exact source path and does
not overwrite an unapplied custom choice. Fenix-incompatible runners are identified
and blocked while Fenix is enabled.

Persisted legacy Fenix installations are presented as existing setups. Managed
installations follow dependent patch, aircraft, sign-in and configuration steps.
The native layouts restore disclosure groups, help links, region choices, update
status titles and save tools. Raw text uses finite bounds and explicit clipping so
help text remains visible with the software renderer.

Community entries can be reviewed and removed individually. The backend reserves
the runtime, records the selected directory identity and inventory, and rechecks
it before and after an atomic move into an owned staging directory. Links are
unlinked without removing their targets. Changed entries, foreign trees and mounts
are refused. A failed removal exposes any retained staging path. Official companion
applications are uninstalled through their own installers; Community removal does
not uninstall them. See [add-on instructions](addons.md).

## Fenix install-hook warning

The reported screenshot says that file installation completed but the
application install hook failed. That is a Velopack warning about the application's
post-install command; a zero exit code from the outer installer is insufficient
to declare success. See the upstream [hook failure discussion](https://github.com/velopack/velopack/issues/297).

A local Fenix 1.0.286 log reproduced this warning with an ICU symbol-loading
failure. Flightdeck already supplied .NET compatibility variables to its own
Unix child processes. It now also persists `DOTNET_SYSTEM_GLOBALIZATION_USENLS=1`
and `DOTNET_ReadyToRun=0` in **that Wine profile's** `HKCU\Environment`, before
starting Fenix. This covers later Windows shortcut/Explorer launches and detached
installer children. The Microsoft [globalization setting](https://learn.microsoft.com/en-us/dotnet/core/run-time-config/globalization)
selects Windows NLS instead of ICU; the [runtime environment options](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-environment-variables)
document the ReadyToRun switch. Host environment files are unchanged.

Flightdeck also waits for installer children after the bootstrapper exits,
instead of immediately cleaning them up. Close all Fenix windows when finished,
or use **Fenix beenden / Stop Fenix**. It checks only the new part of the local
log for failed, timed-out or unfinished install/update hooks. An old failed log
entry does not turn a successful retry into a failure.

An explicit, isolated test passed the real official FenixApp 1.0.286
`--veloapp-install` hook with both compatibility variables removed from the Unix
launch environment. The variables persisted in the Wine registry were sufficient
in that test. It used a new prefix and copied application/runtime code, without
accounts, activated profiles, games or existing-prefix changes. This verifies
the known ICU failure; the screenshot alone cannot establish whether the remote
user encountered the same cause. It does not validate Fenix activation or a flight.

Reproduce with locally available official application/runtime code and a compatible
runner; the output directory must not already exist:

```sh
FLIGHTDECK_TEST_RUNNER="$RUNNER" \
FLIGHTDECK_TEST_FENIX_APP="$FENIX_APP_CURRENT" \
FLIGHTDECK_TEST_DOTNET="$DOTNET_DIRECTORY" \
FLIGHTDECK_TEST_OUTPUT="$PWD/build/fenix-hook-check" \
cargo test --locked --test native_live_fenix -- --ignored --nocapture
```

For a user who sees this warning, retry the official EXE through **Mods → Fenix →
Installer starten** using this build and finish by closing its windows. The
compatibility settings are applied automatically; resetting the whole profile is
not required for the verified ICU case.

## Verification

`cargo test --locked --workspace` covers action guards, runtime/plan binding,
cloud conflicts, maintenance previews, editor/export invalidation, native button
events and the real authenticated service client. The opt-in
`capture_all_native_screens` test renders every screen in German and English at
750 × 900, 960 × 700 and 1536 × 1024 using the production software renderer, without a GPU
or display server. [BUILDING.md](../BUILDING.md) lists the complete checks and
packaging commands. Real Wayland and X11/XWayland windows have also been opened against an isolated
service. Forced narrow tiling uses an icon rail and stacked cards; setup edition
selection remains separate from the active game. Complete simulator and add-on
workflows remain separate from these desktop and API checks.

The 0.2.3 release passed 180 Rust tests, a 40-image capture, 33 maintainer tests,
the official Fenix install-hook probe and isolated Wayland/X11, update and bridge
checks. Its [service performance measurements](performance.md#native-desktop-candidate-023--5-october-2026)
remain historical results, not a new 0.2.4 or simulator benchmark.

The 0.2.4 checks additionally exercise native Proton discovery over HTTP, legacy
Fenix presentation, dependent setup readiness and confirmed mod removal. Backend
removal tests cover neighbours, external link targets, changed/replaced trees and
cancelled previews. Native captures include the same historical synthetic fixture,
both languages, small windows, Fenix legacy/ready states and mod removal previews.
