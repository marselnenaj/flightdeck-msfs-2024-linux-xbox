# Native desktop and Fenix installer changes (0.2.3)

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
every ten seconds unless explicitly refreshed. These checks do not measure
performance inside MSFS; that needs a separate simulator test.

The executable embeds the production interface in `native/ui/`. `ui/` contains
only shared artwork, font and icon assets. `tests/reference-web/` retains the
previous implementation exclusively for visual and behavior comparisons; it is
not shipped or served. Python remains only in maintainer build/test tools. The
Windows add-ons themselves still require their own .NET/Wine components.

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

The final candidate passes 180 Rust tests (eight opt-in tests ignored by the
default suite), the 40-image native capture and 33 maintainer packaging tests.
The separately enabled official Fenix install-hook test also passes. Real-package
tests cover native update/rollback and the unchanged Python bridge sequence
0.1.21 → 0.1.22 → 0.2.3, rollback and repeat upgrade, with settings preserved.
The final executable also starts in the Ubuntu 24.04 baseline container.
[Performance measurements](performance.md#native-desktop-candidate-023--5-october-2026)
compare the final background service with the old Python release.
