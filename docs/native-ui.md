# Native desktop and add-on management (0.2.7)

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

## Compact add-on controls in 0.2.7

Fenix and GSX show the current status, detected components and one highlighted
next action. Completed setup steps no longer repeat their full instructions
and controls. Repair, reinstallation, updates and restore tools remain under
**Manage & repair**. A missing Fenix manager can still be reinstalled when the
companion runtime is present. An eligible repair appears directly after a failed
Fenix install hook.

Current setup errors and running operations appear beside the next action.
Feedback is bound to the selected runtime; unavailable or stale snapshots cannot
show another profile's old job or enable its actions. Running applications retain
their permitted stop control. Existing runtime, idle and confirmation checks
continue to govern every action.

**Fenix locally configured** describes the local patch, runtime files and
settings, without confirming an aircraft package, license or functioning cockpit.
GSX always shows that simulator functionality is unconfirmed, including after
local setup. FSDT installer checks do not validate licensed GSX installation,
activation, Couatl/SimConnect, the in-game menu or ground services.

State and pointer-action regressions cover these distinctions, missing managers,
blocked actions and foreign or stale snapshots. `capture_addon_states` renders
eight Fenix/GSX states in German and English at 750 × 900 and 1280 × 900 using
the production software renderer. These captures do not exercise paid add-ons
or a running simulator. See the [add-on guide](addons.md) for the setup flow.

## Session and control fixes in 0.2.6

Version 0.2.6 adds the following session and control corrections.

When cloud saves need attention but no operation is running, the overview can
switch between installed MSFS editions. Starting the affected edition remains
blocked until its warning is resolved. Synchronization, recovery and running
work continue to prevent edition switching.

An interrupted session shows **Sitzung prüfen / Check session** beside the launch
button. This checks that the previous session's processes have ended and backs
up local saves before clearing the block. It does not start the game or upload
saves. While the check runs, the button shows its progress and launch and edition
selection remain unavailable. If a check or backup fails, the block stays in
place and the reason remains visible.

Disabled controls have a muted appearance. A blocked launch shows a warning
instead of a ready checkmark. An abnormal simulator exit shows a failure hint
and directs users to Diagnostics; the launch button remains available to retry
when other checks pass. Normal exits and controlled stops do not show this
failure hint. **Stopping simulator…** remains visible until stopping finishes.
Cleanup can terminate orphaned Wine services in the stopped runtime's own
profile, with bounded waits and repeated checks that no applications remain.

Startup also replaces a stale Xodus socket left after its service has ended.
Flightdeck checks ownership and the runtime lease before removal; a socket
still bound by a process or a live service that does not respond is preserved.

Regression checks cover real pointer actions, recovery progress, retryable
failures, disabled-control pixels and cached page/size changes with the software
renderer. Backend tests cover save-preserving recovery, stale versus live
sockets and Wine cleanup without terminating another profile or an active
application. These checks do not establish a fix for an unspecified rendering
glitch or for simulator graphics.

## Edition detection and responsiveness in 0.2.5

Installed editions remain selectable when a readiness check fails. Previously,
an inactive MSFS 2020 runtime with pending bundled components was sent to the
new-installation flow: startup had refreshed only the selected MSFS 2024 runtime.
Selecting an existing edition now applies a pending component update under the
existing lease and recovery journal. Other failed checks remain visible and
continue to block launching; they do not imply a missing installation.

The launch button acknowledges a click before any HTTP response and keeps that
feedback until fresh status arrives. Before-start cloud synchronization and
after-exit saving have distinct labels and show the current cloud message near
the button. A running simulator is no longer labelled as a pending cloud sync.

Page navigation displays cached data immediately and refreshes in the background,
without temporarily declaring the service disconnected. Superseded read tasks
are cancelled; submitted mutations, runtime context checks and generation guards
are retained. The panorama is decoded once per edition, and fully visible text
avoids a separate framebuffer-sized clipping mask. Partially clipped text keeps
the explicit mask needed by the software renderer.

See the [measured 0.2.4/0.2.5 UI comparison](performance.md#native-page-switching-025).
`scripts/check-edition-update.py` also exercises a real service against a
synthetic inactive edition built with the historical package's pinned components;
it verifies automatic migration, the new helper, repeat switching and retained
synthetic saves without launching a game or accessing an account.

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

“Application install hook failed” means that the official application's
post-install command failed; an outer installer exit of zero does not establish
success. [Velopack hook behavior](https://github.com/velopack/velopack/issues/297).
The warning can have different causes.

Since 0.2.3, Flightdeck sets `DOTNET_SYSTEM_GLOBALIZATION_USENLS=1` and
`DOTNET_ReadyToRun=0` in both its scoped Wine launch environment and that profile's
`HKCU\Environment`. This addresses the reproduced ICU symbol-loading failure and
covers detached Windows children. The [globalization setting](https://learn.microsoft.com/en-us/dotnet/core/run-time-config/globalization)
selects Windows NLS; the [runtime options](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-environment-variables)
document ReadyToRun. Flightdeck waits for detached installer children and checks
only the current invocation's log slice.

**In 0.2.6:** Flightdeck distinguishes the detected **FenixApp
manager** from the **Fenix companion runtime files**. The former can exist after a failed
hook. **Repair Fenix app** validates the contained app and its `sq.version`, reapplies
the settings above and reruns the official `--veloapp-install` command with a
bounded timeout and cancellation. It does not reinstall the aircraft or reset the
Wine profile. Success enables continuing setup in the official manager; it does
not establish activation or working aircraft systems.

Diagnostics include the latest recorded Fenix attempt independently of the
simulator log: operation, timestamps, validated package version, runner category,
exit codes and finite failure signatures. Raw logs, paths and account data are
excluded. Missing evidence stays unknown; a new attempt replaces the old result.

**Unreleased:** Install and repair commands discard inherited host .NET runtime
paths, dependency/loading overrides and startup hooks before launching Wine. In
a controlled test with official FenixApp 1.0.286, an invalid inherited
`DOTNET_STARTUP_HOOKS` caused exit 82 before the application hook ran. The same
input succeeds with exit 0 through the updated Wine environment. This establishes
one preventable cause, not the cause of every reported exit 82. Prefix registry
values are separate and are not cleared by this filter.

Diagnostics now retain up to eight recognized .NET exception type identifiers
and the CLR exception code only when explicitly observed in the current log.
Messages, stack traces and paths stay local. Exit 82 alone remains `hook_nonzero`.
The latest validated failed attempt remains visible after a launcher restart;
a failed repair leads to diagnostics before another attempt. The green
**Fenix Linux patch** check validates that patch, not the official app's install
hook.

Validation used official FenixApp 1.0.286 in isolated, account-free profiles:

- A fresh full bootstrapper install with `--silent` and newly installed .NET,
  Visual C++ and WebView2 completed its install hook successfully.
- Removing the .NET settings reproduced ICU failure with exit code 3. Reapplying
  them and rerunning the hook succeeded with exit code 0, including when the Unix
  overrides were removed and only the registry supplied them.
- The actual backend repair action completed against that official application
  and persisted its successful versioned result in the isolated runtime.

These checks do not diagnose every remote install-hook failure or validate
activation and simulator flights. To reproduce with official local app/runtime
code, use a compatible runner and a new output directory for each check:

```sh
FLIGHTDECK_TEST_RUNNER="$RUNNER" \
FLIGHTDECK_TEST_FENIX_APP="$FENIX_APP_CURRENT" \
FLIGHTDECK_TEST_DOTNET="$DOTNET_DIRECTORY" \
FLIGHTDECK_TEST_OUTPUT="$PWD/build/fenix-hook-check" \
cargo test --locked --test native_live_fenix \
  official_fenix_hook_with_persisted_wine_environment -- --ignored --nocapture
```

To exercise the inherited-environment regression, set
`DOTNET_STARTUP_HOOKS='C:\flightdeck-repro\missing.dll'` on that test command and
use another fresh output directory. The application should still exit 0. Keep
these opt-in checks in an isolated process and network namespace; they need only
the cached official files and do not perform activation or login.

The separate `official_fenix_hook_failure_is_repaired_by_persisted_compatibility_settings`
check exercises failure and recovery; give it a different, unused output directory.

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
