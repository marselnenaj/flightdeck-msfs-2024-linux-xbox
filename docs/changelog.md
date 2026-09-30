# Changes and release status

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
