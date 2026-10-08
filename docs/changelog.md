# Changes and release status

## 0.2.7 — 8 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.6 launchers can use the in-app updater.

- Simplify Fenix and GSX setup to one highlighted next action. Summarize detected
  components and put additional installer, repair and recovery options under
  **Manage & repair** instead of repeating every completed setup step.
- Show current failures and progress beside the next action. Offer Fenix app
  repair after a failed install hook; retain explicit reinstallation when the
  manager is missing but the Fenix companion is present.
- Bind Fenix job history to the selected simulator installation and hide stale
  add-on feedback after a profile change or connection failure.
- Describe Fenix completion as local configuration. File detection does not
  verify an aircraft installation, account activation or license.
- Keep GSX explicitly experimental and its simulator functionality unconfirmed.
  Require each local prerequisite before reporting setup complete, and keep
  running jobs distinct from completed setup. This release does not establish
  licensed GSX operation, Couatl/SimConnect connectivity or working ground services.
- Update the English and German add-on guides to match the simplified controls.

## 0.2.6 — 8 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.5 launchers can use the in-app updater.
Older clients need the full installer or explicit bridge.

- Allow edition switching while idle even when cloud saves need attention.
  Keep switching blocked during synchronization and running work.
- Add **Check session** for an interrupted session: verify that its processes
  have ended and back up local saves before releasing the launch block.
  This action does not start the simulator or upload saves.
- Recover a stale, disconnected Xodus socket before launch while preserving
  sockets still owned by a running service.
- Make disabled controls and blocked launch states visibly distinct. Show an
  abnormal simulator exit with retry guidance instead of reporting readiness.
  Keep **Stopping simulator…** visible until the stop completes, and finish
  cleanup of orphaned Wine services only within the stopped runtime's profile.
- Honor Stop during startup preparation before spawning the service or simulator.
- Check for launcher and simulator updates periodically in the background,
  retry failed or deferred checks, and show their status on the overview.
  Completed, cancelled or failed update jobs no longer suppress newer checks.
- Format release notes with headings, lists, code blocks and explicit links in
  a scrollable panel. Keep update downloads under the user's control.
- Recognize the FenixApp manager separately from the aircraft. Add **Repair
  Fenix app** for retrying its official install hook with the existing Wine/.NET
  compatibility settings, bounded execution and cancellation.
- Include validated Fenix attempt results in support diagnostics so a generic
  hook warning can be investigated without exporting raw logs or account data.
- Keep the interrupted-session guard when runtime-bound processes remain after
  the supervisor exits; do not begin cloud backup or upload in that state.
- Handle a process exiting during its `/proc` status read without falsely
  retaining the session block. Unknown live writers remain blocked.
- Refuse nested mounted directories during managed tree removal, including bind
  mounts on the same filesystem, so removing a mod cannot traverse their contents.
- Preserve Store-check failures even when individual stages already passed.
  A failed helper process no longer enables automatic cloud retry.
- Use the detected Store region on first setup while preserving a later manual
  choice. Honor cancellation during VR preparation before changing its settings.
- Avoid serializing an existing local save a second time during cloud export.
- Require the local session token for API reads as well as actions, and update
  the locked rustls dependency.
- Coordinate update and rollback restarts from the running launcher so older
  versions can take over without relaxing the service's authentication.
- Shorten both README versions and add a documentation index. Update NVIDIA's
  normal-rendering status to user-confirmed, retaining known limitations such
  as missing startup video and separate validation requirements for VR/features.

## 0.2.5 — 5 October 2026

- Keep existing MSFS editions selectable when they need repair. Apply pending
  bundled component updates when selecting an inactive edition, fixing MSFS 2020
  incorrectly opening the new-installation flow after a launcher update.
- Acknowledge Start immediately and show cloud synchronization and final-save
  progress on the main button. Keep a running simulator's state visible.
- Switch pages using cached state while refreshing in the background. Cancel
  superseded reads and retain context and confirmation checks for mutations.
- Decode each panorama once and avoid unnecessary text clipping masks. Document
  the measured native UI improvement over 0.2.4 in [performance results](performance.md).

0.2.5 became GitHub Latest. Users still on 0.1.21 or earlier need the full
installer once, or an explicit 0.1.22 bridge install. Existing 0.1.22 and native
0.2.1–0.2.4 launchers update normally.

## 0.2.4 — 5 October 2026

- Restore automatic Proton discovery, readable runner labels and the current
  runner selection. Keep Flightdeck, installed Experimental/CachyOS and a custom
  path option; enforce Fenix compatibility when switching.
- Recognize legacy Fenix installs as existing setups and show managed setup
  steps according to their actual dependencies.
- Bring native pages and controls closer to the historical interface, restore
  missing region/help/status content and fix clipped or invisible software-rendered text.
- Add confirmed Community add-on uninstallation with path/size preview, exclusive
  reservation, changed-entry checks and retained external link targets. Fenix/GSX
  companion programs use their official installers.
- Remove the retired web implementation and Node build checks from the source
  tree. Keep artwork and synthetic migration fixtures; Python remains maintainer
  build/test tooling only. Remove fully merged development/release branches.

Existing native versions update directly. Older Python launchers still receive
0.1.22 first; GitHub Latest remains on that unchanged transition release.

## 0.2.3 — 5 October 2026

Update from 0.2.1 or 0.2.2 through **Updates → Flightdeck**, or use the
[full 0.2.3 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.3).
Older Python launchers still receive 0.1.22 first, then 0.2.3. GitHub Latest
remains on the unchanged bridge.

Replace the browser interface with a native Rust/iced desktop for all six views.
Keep the existing artwork, font, icons, DE/EN navigation and backend operations.
Persist native language preferences, verify service reconnections and bind
mutations to their selected installation and reviewed job/plan. Closing the
window leaves managed game and installation work with the existing service.
Narrow tiling below the requested minimum uses an icon rail and stacked cards;
setup edition selection is separate from the active game. The previous web
implementation remains only as a migration test reference.

Prepare Fenix's .NET compatibility settings in its Wine registry as well as the
launch environment. Wait for detached installer children and check new log output
for failed or unfinished Velopack hooks. The official FenixApp 1.0.286 install
hook passes in a new account-free prefix with the Unix .NET variables removed.
This reproduces and fixes the known ICU case; the reported screenshot alone does
not prove the remote user's exact cause. [Details](native-ui.md).

Keep the release ABI at glibc 2.39 with a pinned Ubuntu build image and a package
check for imported symbol versions. Native rendering/action tests, real service
client checks and isolated installer/update checks replace the production
browser contract suite.

The installed background service starts in 87.40 ms versus 148.48 ms for
Python 0.1.21, with 14.38 MiB versus 35.18 MiB resident memory in this workstation
run. These medians do not measure GUI performance or MSFS frame rate.
[Method and results](performance.md).

## 0.2.2 — 4 October 2026

Update from 0.2.1 through **Updates → Flightdeck**, or use the
[full 0.2.2 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.2).
Older Python launchers still receive 0.1.22 first, then the current native release.
The bridge remains GitHub Latest. This release finishes the Rust source transition;
it does not establish a new fix for the reported NVIDIA simulator failure.

Remove the Python launcher, vendored Fenix engine, Python installer, pip package
metadata and duplicated application tests from the current branch. Runtime
entry scripts now have one Rust-backed source. Native builds and packaging no
longer depend on the retired application or its installer. The published 0.1.22
bridge and previous release archives remain unchanged for migration and rollback.

Keep legacy save, .NET, graphics-undo and diagnostic behavior covered with frozen
synthetic fixtures and Rust tests. Browser checks now always start Rust. Graphics,
VR, Proton and C++ save probes invoke the production Rust implementations through
a development-only driver; Fenix no longer installs the unused Python display
helper. Remaining Python files are maintainer build/test tools only.

## 0.2.1 — 4 October 2026

Use **Updates → Flightdeck** in 0.1.22, or download the
[full 0.2.1 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.1).
Earlier launchers receive 0.1.22 first; install, restart and check again. GitHub
Latest remains on that bridge. Versions 0.2.0-dev.1 and the withdrawn 0.2.0 need
the full installer because their native update check has the defect fixed here.
The installer preserves the existing configuration and previous launcher.

Fix native update discovery and streamed component downloads. The HTTP clients
mixed an asynchronous read timer with blocking response reads; ordinary download
workers could panic when reading a response body without a Tokio runtime. Both
now use the blocking client's own connection/read/write timeouts. Slow reads
still time out, while complete downloads may span many individual read windows.
The release feed now also accepts GitHub’s top-level JSON array; the old
object-only decoder rejected it. Duplicate-key rejection is retained at every
level, and cloud APIs still require objects. HTTPS, redirect restrictions and
checksum validation remain enforced. Local
streamed-body and stalled-body tests reproduce the old defect and cover the fix.

This stable release also makes the native launcher available through 0.1.22;
preview flags and preview version tags had excluded 0.2.0-dev.1 from discovery.
It includes the launcher/UI/runtime changes below. NVIDIA/MSFS hardware limits
remain unchanged.

## 0.2.0 — 4 October 2026 (withdrawn)

Withdrawn immediately after the public migration check exposed an HTTP body-read
panic in the native updater. Existing 0.1.22 launchers could discover and install
it, but the resulting Rust service could not perform its next update check. Use
0.2.1. Its GitHub release entry has been removed; the source tag is retained
for historical reference.

## 0.2.0-dev.1 — 4 October 2026 (retired preview)

This preview has been superseded by 0.2.1 and removed from the GitHub release
list. Use the current 0.2.1 installer. GitHub's Latest entry stays on the
0.1.22 Python transition release so older launchers receive the bridge first.

- Share one UI polling timer, refresh idle views every ten seconds and retain
  faster updates during running jobs. Pause all scheduled reads while hidden,
  then revalidate on focus/visibility return. Explicit refresh remains immediate.
- Preserve unchanged check-list DOM nodes, reuse locale formatters and centralize
  job reservations so completing one operation cannot release another's lock.
  Strict JSDoc/TypeScript checks cover these shared UI modules in CI.
- Serve the Rust launcher's embedded images and UI assets without allocating
  another complete copy for each response. Origin, CSP and no-store protections
  remain enforced.

- Fix delayed mapped-DLL loading with the default Flightdeck runner. The
  native Wine mapping redirects the main EXE only; both bridges now expose
  mapped DLLs and the working directory through the same temporary memory-backed
  game view used by selected Proton. The default EXE retains its native mapping.
  Encrypted game files and ordinary resources are preserved. A real Windows
  probe reproduces error 193 before this correction.
- Start MSFS 2020/2024 with `-FastLaunch` at the final Wine boundary. This
  applies the reported intro-path workaround for a black main view with visible
  menus/secondary views to the default runner and selected Proton versions.
  Python and Rust preserve explicit arguments without duplication; managed
  older Python runtime scripts receive the change through component updates.
  Set `FLIGHTDECK_FAST_LAUNCH=0` in the launch environment to compare the full
  intro path. The affected Linux/NVIDIA game rendering still needs confirmation.
- Keep MSFS 2024's saved NVIDIA options consistent with the effective launch
  profile. Automatic/Compatibility replace saved DLSS with TAA, disable saved
  Reflex and NVIDIA frame generation, and retain a per-field undo record in the
  Wine profile. Features mode restores values still matching Flightdeck's
  changes. Other upscalers, resolution, quality, content and subsequent user
  edits are retained. The Python reference uses the same format and behavior.
  This prevents incompatible saved options; the affected RTX 4080's black
  primary view remains unverified on hardware.

Port the launcher backend, desktop service, installer, updater and runtime
helpers to Rust. The native package needs no Python interpreter. It preserves
the browser interface, runtime/save formats and Wine/Store ABI components.
Setup, cloud saves, add-ons, graphics/VR, diagnostics and recoverable maintenance
jobs use native implementations. The previous Python release remains available
for explicit launcher rollback and as the test reference.

The alternative Proton loader now uses the licensed temporary game view as its
working directory. Previously, delayed DLL loads relative to the working
directory could reach encrypted originals and fail with Windows error 193. The
expanded synthetic test reproduces that failure before the fix and passes with
Experimental 11.0 (20260924) and CachyOS Proton 10 sunset after it, including
D3D11/D3D12 rendering and return to the default runner. This fixes a loader defect;
confirmation of the reported MSFS menu failure on the affected system is still
outstanding. Later CachyOS repetitions timed out twice, then passed in a Wine
virtual desktop and again after returning to the host compositor. This
intermittent graphics-test issue remains unexplained. Known Fenix launch scripts
receive the correction through the
component updater, and selecting a Fenix overlay retains the current launcher
scripts instead of reinstalling older archive copies.

- Automatically repair incomplete Microsoft .NET Framework 4.8 setup for
  Fenix/GSX in the staged Windows profile. Verify both CLR architectures by
  starting the managed compiler; registry markers alone are insufficient.
  Complete pending Wine restart operations, handle Wine Mono's advertised
  release values and make a bounded reinstall attempt when MSI repair fails.
- Offer **Repair setup** for interrupted Fenix preparation whose original
  profile, runner and launch scripts remain unchanged. Retain failed copies
  and logs; committing transactions still require recovery.
- Check managed .NET evidence again before Fenix/MSFS startup. Repair an
  incomplete profile in a separate copy, verify it, and retain the previous
  profile when activating the repair. Interrupted repairs remain recoverable.
- Package a verified native runtime helper with the current launch scripts,
  preserving custom component sets and supporting idle launcher handoff.
- Add native transaction, HTTP and browser checks and an isolated
  [Python/Rust performance comparison](performance.md).

In the isolated full-package comparison, native startup takes 72 ms versus
134 ms and service RSS falls from 35.21 MiB to 10.48 MiB. The native package is
3.27 MiB larger. These results do not establish an MSFS frame-rate improvement.

Fresh .NET setup, repair of a deliberately removed x86 CLR (including matching
restored file hash), and an idempotent follow-up passed with the pinned Flightdeck
Wine runner in an isolated, account-free profile. The remote report's exact
failure still requires its setup log; this test does not establish every Proton
build or distribution. This native build is a prerelease; the Python-side
.NET repair and earlier selected-Proton corrections ship in 0.1.22.

## 0.1.22 — 3 October 2026

Prepare existing Python installations for the native launcher. This release
still runs on Python 3.10+ and can be installed by the 0.1.21 in-app updater.

- Recognize native packages, verify their manifest, x86-64 executable and
  SHA256 checksum, and run the incoming native installer with the expected
  current release bound to the update.
- Restart the native service at the existing browser address. Preserve the
  installation manager through rollback to Python and a subsequent return
  to Rust. Terminal updates and installer launches accept the new shell wrapper.
- Discover stable full packages from the release list. Keep GitHub's Latest
  entry on 0.1.22 so older clients receive this prerequisite first; later
  stable native releases remain discoverable by updated clients. Follow the
  [staged release procedure](rust-transition.md).
- Include the Python-side automatic .NET repair and Experimental/CachyOS
  loader corrections described below, with the same live-validation limits.

The real 0.1.21 package, transition package and a local native candidate pass
the isolated in-app update and rollback chain. The native candidate is a local
test build; this release installs the Python transition launcher.

## 0.1.21 — 3 October 2026

- Include **Flightdeck (Xodus, default)** directly in the Proton selector and
  preselect the active environment when available.
- Use the existing return-to-Flightdeck operation when applying that choice,
  preserving the current add-ons and settings. Keep recovery available when
  an interrupted switch or cloud-sync issue blocks other runner choices.

## 0.1.20 — 3 October 2026

- Carry the current Windows profile across Proton changes and back to Flightdeck,
  retaining aircraft, mods, sign-ins and settings instead of restarting each trial
  from the original profile. Retain earlier profiles as backups.
- Add exact-build Fenix overlays for Experimental 11.0 (20260924) and CachyOS 10.0
  sunset. Show compatible versions in the selector and migrate known legacy
  Fenix installations while preserving their current profile.
- Allow GSX/FSDT preparation and launch on the selected Proton. Handle Wine64
  consistently for classic runners and keep recovery journals coordinated.
- Keep Fenix geometry, helper-window and MCDU refresh features in the portable
  launcher. Reject unsupported Fenix runner combinations before changing files.

Native Fenix probes, fresh .NET/Fenix setup, official FSDT preparation and isolated
switch/loader/graphics checks pass for both builds. The launcher suite passes
832 tests, compatibility packaging 39 tests and Chromium 309 UI checks. Full Fenix/GSX flights on alternate Proton versions and the reported NVIDIA
black main view remain unverified.

## 0.1.19 — 3 October 2026

[Download Flightdeck 0.1.19](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.19).

This release combines experimental Proton selection and GSX Pro setup with a
more compact launcher interface. Update through **Updates → Flightdeck** or
the full installer. This is a regular release on the stable update channel.

- Select an installed Proton version under Setup, discovered in Steam libraries
  or chosen by folder. Each trial has its own Wine/DXVK/VKD3D copy and Windows
  profile, with recovery and a return to the original Flightdeck environment.
- Preserve normal Xbox entitlement checks and Store/GDK integration with the
  alternative launch bridge. Account/save helpers keep the original runner.
- Add GSX Pro preparation under Mods for MSFS 2024: prepare the pinned official
  FSDT installer and .NET, install and activate through FSDT, then enable its
  existing Couatl startup entry. Retain a profile backup and support recovery.
- Keep GSX setup and Proton recovery from replacing each other's profiles.
  Return to Flightdeck before changing FSDT setup; recover incomplete GSX setup
  before switching Proton. Internal add-on links stay inside copied profiles.
- Show Fenix and GSX as compact status cards, with one setup workflow expanded
  at a time. NVIDIA, VR, Proton and maintenance settings also open on demand.
  Keyboard access and German/English mobile layouts remain available.
- Update the managed runtime script checksums, including GSX session cleanup,
  and include the selected Proton version in diagnostics.

Proton Experimental 11.0 (20260924) and CachyOS Proton 10.0 sunset passed synthetic
executable/DLL loading, Store interface loading, mixed D3D11/D3D12 rendering and
return to the original profile on AMD. Full MSFS operation with these runners
and a fix for the NVIDIA black view remain unconfirmed. The 0.1.18 follow-up
confirms that corrected renderer build `628afa6f9cfece4` loads but the affected
system's globe/map and cockpit remain black.

FSDT installer preparation, opening and closing were tested under Wine. GSX
package installation, activation, Couatl/SimConnect and ground services still
need validation with a licensed copy. Fenix keeps its matched Flightdeck runner.
No native or renderer binaries change; their existing corresponding sources and
notices are retained. [GSX setup](addons.md#gsx-pro-experimental) ·
[Proton trials](runtime.md#experimental-proton-selection).

## 0.1.18 — 2 October 2026

[Download Flightdeck 0.1.18](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.18).

- Add per-installation VR settings for automatic OpenXR, WiVRn, SteamVR and
  Monado, off by default, with German and English setup instructions.
- Add an explicit, bounded headset/GPU check with clear failure states and
  protection against concurrent game starts, cloud sync and setup.
- Prepare separate Windows and Linux OpenXR manifests and Wine's required VR
  registry data for direct game starts, preserving the Xbox Store bridge.
- Keep DXGI on the compositor's GPU, preserve NVIDIA compatibility settings,
  and retain SteamVR registration across the game's isolated XDG directories.
- Include VR mode/check outcomes in diagnostics without raw driver output or
  identifying runtime paths and device UUIDs.
- Add real D3D11/D3D12 stereo-frame validation through Wine and retain the
  renderer correction from 0.1.17. No native renderer or Store binaries change.

Validated with a simulated Monado headset and AMD RX 6900 XT. Physical headsets,
NVIDIA hardware, WiVRn/SteamVR device operation and an MSFS VR flight remain
unverified. [Setup](vr.md) · [Test evidence and reproduction](vr-validation.md).
This is a regular release, available through the stable update channel.

## 0.1.17 — 2 October 2026

[Download Flightdeck 0.1.17](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.17).

- Backport the upstream VKD3D correction for full-depth 3D-texture copy barriers
  with `VK_KHR_maintenance9`. The previous barrier covered only the first depth
  slice of render-target-capable volume textures.
- Give the rebuilt D3D12 pair a source-derived build ID, pin the changed-source
  hashes, and retain the existing NVIDIA low-latency corrections and DXVK build.
- Add a real upload/copy/readback regression that requires active Vulkan
  validation, reproduces the released renderer's invalid layouts, and checks
  the corrected renderer plus controls with maintenance9 disabled.

The isolated regression passes on AMD. This establishes the volume-layout
correction; its connection to the reported black NVIDIA main view is unverified.
Available as a regular release through **Updates → Flightdeck** or the full installer. Close Flightdeck and MSFS before installing,
then reopen Flightdeck, use **Automatic** NVIDIA graphics and check the main map
and cockpit. The game log should identify renderer build `628afa6f9cfece4`.
No game reinstall or profile reset is needed. The Microsoft sign-in components
and their corresponding sources remain at the verified 0.1.16 versions.
[Implementation and validation](nvidia-renderer.md#3d-texture-layout-correction-0117).

## 0.1.16 — 1 October 2026

[Download Flightdeck 0.1.16](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.16).

- Preserve verification steps embedded in individual replies of a multi-scope
  Microsoft token exchange. A response can require Xbox verification while
  already issuing a Microsoft root token; the login now completes that step
  before storing credentials. The previous decoder rejected this response
  because the verification reply had no issued-token fields.
- Accept Microsoft's shortened SOAP fault headers so an accompanying supported
  verification link reaches the sign-in window instead of failing with code 74.
  Successful token responses still require their normal headers; signature
  verification and account/root-credential checks remain in place.
- Decode encrypted verification steps and token responses using bounded buffers
  and the XML Encryption CBC padding rules. Valid responses larger than 8 KB no
  longer overflow the old fixed buffer; malformed cipher data returns a typed
  failure instead of panicking.
- Replace the broad response-error category with fixed codes 80–93 for request
  construction, response size/encoding/XML, signatures, challenge/body decoding,
  incomplete responses, HTTP errors, root tokens, timeouts and rate limiting.
  The Store recovery report and game updater preserve these codes and their
  localized explanations, without including raw authentication output.
- Rebuild both Xodus components because they share the response parser. Managed
  component updates recognize the published 0.1.15 binaries.

Update through **Updates → Flightdeck** and restart Flightdeck before retrying
sign-in. See [sign-in recovery](store-session-refresh.md#soap-response-correction-0116)
for the diagnostic codes and validation scope. Other reported NVIDIA,
in-game Marketplace and cloud-sync problems still need separate confirmation.

## 0.1.15 — 30 September 2026

[Download Flightdeck 0.1.15](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.15).

- Keep one browser context throughout interactive Microsoft sign-in, preserving
  session cookies when opening a follow-up verification view.
- Ignore queued token callbacks from a previous view after the next step has
  opened. A duplicate callback can no longer repeat the old exchange and end
  the current verification step.
- Distinguish connection errors, invalid Microsoft responses, credential
  rejection, account mismatch and missing supported challenges using fixed,
  localized sign-in codes. Raw authentication responses are never displayed.
- Ship the rebuilt login component and recognize the 0.1.14 native component
  set for automatic updates of managed installations.

Native GTK/WebKit tests use a loopback password → email code → completion flow.
The 0.1.14 baseline loses cookies and fails on duplicate callbacks; the corrected
runtime passes both checks and the combined case under X11 and Wayland. This
does not yet confirm successful Microsoft email-code verification on an
affected account. Update and restart Flightdeck before retrying sign-in.

## 0.1.14 — 30 September 2026

[Download Flightdeck 0.1.14](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.14).

- Preserve a Microsoft follow-up sign-in URL when the token exchange also
  reports a credential fault. Since 0.1.9, this combination could end the
  interactive login before displaying the required next step. Silent renewal
  continues to require sign-in; a challenge does not count as successful login.
- Share the existing Microsoft HTTPS challenge validation between renewal and
  interactive login. Keep credential rejection without a valid challenge as a
  failure, and retain account binding and root-ticket validation.

- Ship rebuilt login and broker components and recognize the 0.1.13 component
  set for automatic updates of managed installations.

A synthetic regression test reproduces the discarded challenge in 0.1.13 and
passes with this correction. Authentication and session-renewal tests and the
native build pass. Confirmation of the reported immediately closing login
window on affected hardware remains pending; NVIDIA rendering is not changed.

## 0.1.13 — 30 September 2026

[Download Flightdeck 0.1.13](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.13).

- Accept Microsoft's fallback product translations in explicit Marketplace
  queries, as already supported for the owned library. Prefer the requested
  language and preserve the selected Store region, pricing and ownership checks.
- Include fixed mapping-failure categories in diagnostic summaries and session
  timelines, without product IDs, account data or raw catalog text. Existing
  reports with only `80004001` cannot identify the rejected product feature.
- Label successful Store diagnostics as **Basic Store checks passed** and
  explain that these checks do not test in-game Marketplace product queries.
- Ship the rebuilt Store runtime and recognize the previous native component
  set for automatic updates of managed installations.

The translation regression is reproduced and tested with synthetic catalog data.
Native catalog, diagnostic-export, browser and package-update checks passed.
This does not yet establish that the reported in-game Marketplace error is fixed
for every affected installation. German Store and game-language settings can be retained.

## 0.1.12 — 29 September 2026

[Download Flightdeck 0.1.12](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.12).

- Separate installed-resource verification from the source-import DLL name
  list and 2 MiB text-file limit. Preserve every checksum, regular-file and
  symlink check, with the existing 128 MiB binary limit.
- Cover a real service update, restart and rollback with a newly introduced
  resource larger than the source limit; retain the same browser origin.
- An already-running 0.1.10 process cannot receive this verifier correction
  before restarting. If it reports that the installed version could not be
  identified after an otherwise successful update, close and reopen Flightdeck
  once through the application menu. Game files and saves are unaffected.
- NVIDIA/Store binaries are unchanged from 0.1.11. This is a launcher update
  correction and does not add a new NVIDIA rendering claim.

## 0.1.11 — 29 September 2026

[Download Flightdeck 0.1.11](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.11).

- NVIDIA Automatic now selects compatibility for both DirectX 11 and 12, also
  for saved automatic preferences. DLSS, Reflex and NVIDIA Frame Generation
  require the explicit **NVIDIA features (experimental)** mode.
- Correct the ignored `dxvk.disableNvLowLatency2` option in the pinned DXVK.
  Ship matched DXGI/D3D11/D3D10 libraries with the existing VKD3D backport,
  checked before installation. Custom renderer files are preserved.
- Retain VKD3D warnings in the default game log and recognize patched renderer
  versions in diagnostic exports.
- Compact repeated report rows without dropping their values, making long
  reports more likely to fit in the email draft. Downloads remain available.
- Mixed D3D11/D3D12 clear, readback and multiwindow presentation passed on
  AMD hardware. This release corrects the incomplete NVIDIA opt-out; resolving
  the black MSFS main view still requires confirmation on NVIDIA hardware.

## 0.1.10 — 29 September 2026

[Download Flightdeck 0.1.10](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.10).

- Fix native component updates being skipped when a runtime has customized
  launch scripts. The launcher could show 0.1.9 while the older Store binaries
  remained installed, causing **Diagnostics → Check Store** to stop at the
  component check and preventing the new session fixes from taking effect.
- Update the recognized native component set independently. Preserve customized
  scripts and their provenance record; retain existing checks for custom or
  modified binaries, busy runtimes and interrupted-update recovery.

Native compatibility components and the NVIDIA renderer remain the same as in
0.1.9. This release fixes delivery of those Store components to affected managed
runtimes; it does not establish a fix for NVIDIA's black main view or every
reported in-game Marketplace/cloud error.

## 0.1.9 — 29 September 2026

[Download Flightdeck 0.1.9](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.9).

- Add local problem reports with a frozen diagnostic snapshot and an email
  draft addressed to `contact@flightdeck-app.com`. Description and diagnostics
  are included as plain text; no attachment, GitHub account or upload service
  is required. Provide complete copy/download fallbacks for webmail and long
  reports, with German and English UI.
- Extend bounded diagnostic coverage with authentication, networking-policy
  and audio error summaries, without including raw logs or account data.
- Preserve the Store account context across a same-account ticket-expiry gap,
  while rejecting expired credentials, account changes and logout. A failing
  regression reproduces the permanent rejection after ticket renewal in 0.1.8.
  See [Store session recovery](store-session-refresh.md) for scope and evidence.
- Renew expiring Microsoft user/device tickets before Store and Xbox requests,
  share concurrent renewal work, and allow enough IPC time for renewal plus the
  Store request. Keep network/keyring failures distinct from sign-in challenges.
- Add **Renew Microsoft sign-in** with same-account protection, cancellation and
  subsequent license/library verification. **Sign in again and sync** retries
  the captured cloud operation only after that verification succeeds.
- Backport three upstream VKD3D fixes for NVIDIA low-latency swapchain ownership
  and lifetime. Apply the matched D3D12 DLL pair to NVIDIA installations using
  the pinned runner; preserve custom renderers and the shared Wine/Fenix runner.
- Make Compatibility mode disable `VK_NV_low_latency2` as well as NVAPI/NGX.
  Disabling NVAPI alone did not disable VKD3D's NVIDIA-specific presentation path.
- Verify the renderer payload against source-release hashes during packaging,
  installation and launch. Complete interrupted copies before starting a game;
  restore managed copies from the runner when the runner changes.
- Add a native multiwindow rendering/readback test, including unused secondary
  windows, resize and destruction. This is not an MSFS or NVIDIA hardware test.
- Document the unresolved NVIDIA black main view, repeated cloud-sync failures
  and blocking in-game Marketplace session error in both languages. Replace
  repeated graphics-log requests with the current status and defined rendering
  checks.

The black main scene reported on RTX 4060/5060 Ti is **not yet confirmed fixed**.
See [renderer build and validation](nvidia-renderer.md). Native Store components are rebuilt as
**0.1.9**; the reported in-game Marketplace dialog and repeated cloud errors
are not yet confirmed resolved.

## 0.1.8 — 28 September 2026

- Give Store inventory, package-update queries and Microsoft account ticket
  requests enough time to receive server responses. These operations previously
  used a five-second local-request timeout, which could close the shared broker
  connection and abort subsequent license queries.
- Keep the broker connection usable after a completed inventory request reports
  a server timeout. A broken or unresponsive transport still closes safely so a
  late reply cannot be mistaken for another request's response.
- Align the nested cloud sign-in waits with the longer broker request deadline.
- Report native cloud timeouts, connection failures and server errors as
  connection problems, including during sign-in and title lookup. Actual rejected
  sign-ins still report authentication errors; numeric diagnostics are retained.

Native components are rebuilt as **0.1.8**. Delayed synthetic broker replies
reproduce the 0.1.7 inventory timeout and following license abort, and verify
the corrected inventory, sign-in, package-update and failure handling. These
checks use the built Wine component without accounts or real cloud data; they
do not establish recovery from every Microsoft outage or a successful live
cloud sync on an affected user's system.

## 0.1.7 — 28 September 2026

[Download Flightdeck 0.1.7](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.7).

- Select the sole discrete NVIDIA GPU by its Vulkan device UUID instead of its
  Linux display name. Wine can rename the adapter, especially in Compatibility
  mode; the previous name filter could exclude the intended GPU. DirectX 12
  retains the adapter selected by DXGI.
- Include graphics startup versions, adapter-filter failures and counts of
  presents without rendering in diagnostics. Hardware UUIDs and raw log lines
  are excluded from the exported report.
- Add **Diagnostics → Check Store** to check installed components, saved sign-in,
  catalog access, game licensing, owned content and local window display without
  opening a purchase page.
- Open Microsoft's Marketplace confirmation page in a dedicated window. Fix
  blank or gray windows and use Microsoft's Xbox layout with a compact frame
  that fits the available space.
- Show loading progress and distinct connection, timeout and expired-session
  errors. Keep failures visible until dismissed, support cancellation and allow
  only one purchase dialog at a time, without automatic retries.
- Improve owned-content queries for bundles, large libraries and fallback Store
  languages. Keep Store-account checks tied to the active sign-in and handle
  unrelated library items without rejecting the current game's content.
- Include timestamped Store events and the component versions used at game
  launch in diagnostics, excluding account data, product identifiers and raw logs.
- Reuse the existing launcher window after an update or rollback, retaining its
  local address and language. Handle update restarts from earlier releases
  without opening a second window.
- Update English and German guides with Store checks, purchase-dialog limits,
  cloud-save behavior and the separate Fenix patch update process.

Native components are rebuilt as **0.1.7**; the optional Fenix patch remains
**0.1.0-preview.3**. The confirmation page and its layout have been checked in
MSFS 2024 1.8.16.0. Automated checks cover dialog behavior, cancellation and
message validation. Completed paid transactions, delivery of purchased content
and layout across other systems remain unverified. Broader Marketplace
inventory compatibility also requires testing.
The adapter-selection regression has been reproduced and corrected with the
real runner on AMD hardware, including vendor hiding and both DirectX 12 device
creation paths. NVIDIA in-game rendering, including the reported RTX 5060 Ti
black scene, still needs hardware validation.

## 0.1.6 — 26 September 2026

[Download Flightdeck 0.1.6](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.6).

- Add **Setup → NVIDIA graphics** with Automatic and Compatibility modes,
  saved separately for each simulator installation and applied on its next
  start. Compatibility disables NVIDIA-specific features, including DLSS and
  NVIDIA Frame Generation, while retaining the physical GPU selection.
- Translate NVIDIA vendor-hiding options for direct Wine/DXGI starts and make
  NVAPI opt-outs effective even after an earlier NVIDIA-enabled launch.
- Complete an unambiguous GPU name filter for both DXGI and DirectX 12 while
  preserving explicit choices and avoiding guesses between multiple GPUs.
- Extend graphics diagnostics with persistent start settings, graphics-library
  checks and recognized error codes. These records describe configuration and
  errors, not successful rendering.
- Add **Setup → Manage installation** with removal previews, retained data by
  default, and a reversible Wine environment reset. Active games and setup
  operations remain protected; pending cloud recovery data is preserved.
- Update the English and German guides with graphics-mode instructions,
  installation maintenance and consistent compatibility requirements.

NVIDIA rendering and flight stability still require hardware validation.
Native components remain **0.1.1** and the optional Fenix patch remains
**0.1.0-preview.3**. See [NVIDIA graphics](graphics.md) and
[installation maintenance](install.md#manage-a-game-installation).

## 0.1.5 — 25 September 2026

[Download Flightdeck 0.1.5](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.5).

- Check for Flightdeck and selected-simulator updates automatically when the
  app opens. Show available updates on the overview; background discovery does
  not reserve the simulator or start downloads/sign-in. Reopening windows is
  throttled for 30 minutes per running service; manual checks remain available.
- Prepare NVIDIA graphics before launcher-managed starts: use NVAPI and optical
  flow from the matching runner, NGX from the installed driver, and Proton's
  corresponding DLL overrides. Existing managed files follow runner/driver
  updates; user-supplied DLLs and explicit GPU selections are retained.
- On NVIDIA plus integrated-graphics systems, select the sole discrete NVIDIA
  adapter consistently for DXGI and DirectX 12. Report unavailable NVIDIA Vulkan
  before launch. AMD/Intel-only launch environments are unchanged.
- Recover from a failed post-exit cloud upload with explicit local play. Keep
  backups and the pending transaction; require a version choice if later local
  progress differs from the interrupted upload's target.
- Include bounded GPU/Vulkan information and numeric cloud error details in
  diagnostics; distinguish connection, storage-lock and quota failures.

NVIDIA rendering is not yet verified across GPU and driver versions. Native
components remain at **0.1.1** and the optional Fenix patch remains
**0.1.0-preview.3**.

## 0.1.4 — 24 September 2026

- Update Flightdeck directly under **Updates → Flightdeck**: check stable GitHub
  releases, read release notes, download and install, then restart when ready.
  Downloads are checked against GitHub's SHA256 digest; the previous launcher
  version remains available for restoration. Installation requires an idle game.
- Brief notifications close automatically after six seconds (errors: twelve),
  pause while hovered or focused, and include a close button. Errors inside
  update and setup panels remain visible.
- Integrate Fenix patch **0.1.0-preview.3**: path metrics restore missing routes;
  correct stroke contours remove excessive lines at acute route joins.
- Install and verify the separately downloaded Microsoft geometry dependency
  in fresh profiles and updates from preview.1/preview.2.
- Keep matching Fenix helper windows off the X11/Xwayland desktop while
  preserving their internal visibility; retain the fallback window guard.
- Retain the aircraft, user settings and original restore point during updates.

- Automatically refresh stale MCDU images after Fenix Display restarts, preserving
  pages and restoring the original display preference and brightness.

Native components **0.1.1** are unchanged. The official Fenix display-restart
check passed with automatic MCDU recovery. This remains a community Fenix preview;
a full flight and other aircraft/simulator versions have not been verified.

## 0.1.3 — 24 September 2026

[Download Flightdeck 0.1.3](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.3).

- Repair missing UI-font registrations before opening Fenix applications,
  fixing installer startup crashes in affected Wine profiles.
- Restore visible arrow, text and link cursors in the official Fenix Installer
  and livery manager.
- Update supported preview.1 patch installations before opening Fenix, retaining
  aircraft, settings and the original restore point.
- One **Open Fenix** action opens the main application. The separate
  **Installer & Liveries** action opens the official installation/livery manager.
- **Open Fenix** is available for existing local Fenix installations.
- Closing Fenix also cleans up its WebView helpers, including after a manager
  crash, so a later start is not blocked by leftover processes.
- **Stop Fenix** stays visible and shows when Fenix has stopped.
- Opening Fenix no longer creates an extra blank Wine notification-area window.

Native components **0.1.1** are unchanged. Fenix patch **0.1.0-preview.2** adds
the cursor fix. Install the new full package to update Flightdeck; its Fenix panel
downloads and verifies the matching patch when needed.

## 0.1.2 — 23 September 2026

[Download Flightdeck 0.1.2](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.2).

- **Mods → Fenix A320 → Stop Fenix** closes the main Fenix application, its
  helpers and the official manager in the selected Wine profile. It first
  requests a normal close and uses a bounded fallback for stuck processes.
- The button works while Flightdeck's **Open Fenix** or livery-manager job is
  active. Setup and game launch stay reserved until shutdown completes. A running
  simulator or installer disables this action; other profiles and applications
  are excluded.

Validation includes process tests for graceful close, stuck helpers, profile
isolation and a running simulator; runtime-reservation tests; and German/English
desktop/mobile browser flows. Native components and patch **0.1.0-preview.1**
are unchanged. Install the new full package to update the launcher.

## 0.1.1 — 23 September 2026

[Download Flightdeck 0.1.1](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.1).
Run the installer from the new full package to update an existing launcher.
Game files, profiles, settings and saves remain in their existing locations.

### Fenix A320 in MSFS 2024

- **Mods → Fenix A320** downloads a fixed, checksummed release from the separate
  [Fenix patch project](https://github.com/marselnenaj/fenix-a320-linux-patch).
  Patch **0.1.0-preview.1** is released independently of Flightdeck's version.
- The four-step flow prepares a separate Wine runner/profile and .NET Framework
  4.8, starts the user's official Fenix installer, opens normal sign-in and
  applies CPU displays, Legacy readouts and autostart.
- Completed steps, the next action and a final ready message make setup progress
  explicit. An open Fenix/Windows application explains why setup is waiting.
  A keyboard hint covers entering `@` on German layouts in the Fenix login.
- After setup, Fenix starts with MSFS. Flightdeck closes its session companions
  when the game exits, crashes or is stopped, with a bounded fallback for stuck
  helpers. Other Wine profiles and the official installer remain independent.
- Wine fixes address cockpit rendering, FCU/radio text and Fenix helper windows.
  Setup retains the original profile and supports recovery after interruption.
  Existing local development patches are detected and kept in use.
- The installed official Fenix manager can be opened for liveries. Packages
  must match the purchased aircraft, engine, wing variant and simulator.

[Setup, liveries and restore](addons.md#fenix-a320) ·
[Deutsche Anleitung](addons.de.md#fenix-a320-einrichten)

### Separate MSFS editions

- Select **MSFS 2024** or **MSFS 2020** in the overview and installation form.
  Each edition has its own runtime, Wine profile, game package, Community
  location, update history and local saves.
- Package checks, game updates, integrity checks, repair, rollback and cloud
  helpers use the selected edition's identity. Existing runtimes without an
  explicit edition remain MSFS 2024.
- The overview changes its aircraft artwork with the selected simulator and
  preserves layout during switches, including reduced-motion settings.

### Launcher and compatibility updates

- A newer full Flightdeck package can refresh recognized managed native
  components and launch scripts when the runtime is idle. Interrupted component
  updates retain recovery information; custom components are preserved.
- Launcher updates use the installer included in the new package. An idle
  background service can pick up the updated launcher on the next open.
- Microsoft sign-in has its own GTK/WebKitGTK window. Store region selection,
  download size guidance and setup messages are clarified.

[Updating Flightdeck and its runtime](install.md#update-and-rollback)

### Marketplace compatibility

- Enumerate supported direct and satisfying account entitlements for the
  current game, including legacy MSFS 2020 application IDs.
- Acquire supported Durable license handles using verified Microsoft-signed
  grants. Handle expiry, query lifetime and release explicitly.
- Query the single registered base-game package's update state. Improve
  supported consumable balances, catalog images and search terms, and retry
  expired signed receipts within the original bounded request.

Paid checkout, package installation through the game's Store APIs and
device-shared DLC rights remain unsupported. These changes do not establish
that the MSFS 2024 Aviator Upgrade works. See the
[Marketplace scope and evidence](marketplace-collections.md).

### Validation and remaining limits

Fenix 2.4.0.4720 / MSFS 2024 1.8.16.0 cockpit displays were checked on Hyprland.
A complete Fenix flight remains unverified; CPU displays do not provide weather
radar. The first patch requires the pinned Xodus runner. MSFS 2020 can display a
startup disc prompt, and startup/full-flight compatibility remains unconfirmed.
Cloud saves remain experimental, with cross-device gameplay validation pending.

## 0.1.0 — 18 September 2026

[Initial experimental release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.0):
MSFS 2024 installation and launch, download pause/resume within the running
session, base-game update/repair/rollback, Community inventory, local backups,
experimental automatic Xbox cloud saves and a German/English interface.
