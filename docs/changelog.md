# Changes and release status

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

NVIDIA integration and interrupted-sync recovery have isolated regression
tests. Rendering on the reported RTX 4060/Zorin system still needs verification.
Validation includes Python regression tests, compatibility packaging tests,
JavaScript tests and desktop/mobile Chromium checks. Native components remain at
**0.1.1** and the optional Fenix patch remains **0.1.0-preview.3**.

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
The packaged Wine modules, clean .NET setup, matching/nonmatching helper windows,
installer recovery and desktop/mobile UI flows have separate tests. The Fenix
release's GitHub checks and an anonymous download through Flightdeck passed.
A complete Fenix flight and other desktops remain unverified; CPU displays do
not provide weather radar. The first patch requires the pinned Xodus runner.

MSFS 2020 workflows have synthetic tests and live license/package probes. A
startup disc prompt was reported; those probes do not prove its resolution or
a successful flight. A complete end-to-end MSFS 2020 installation remains open.
Cloud saves remain experimental, with cross-device gameplay validation pending.

## 0.1.0 — 18 September 2026

[Initial experimental release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.0):
MSFS 2024 installation and launch, download pause/resume within the running
session, base-game update/repair/rollback, Community inventory, local backups,
experimental automatic Xbox cloud saves and a German/English interface.
