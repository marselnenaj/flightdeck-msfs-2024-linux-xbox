# Known issues

[Deutsch](known-issues.de.md) · [Documentation](index.md)

Status: 27 October 2026, including the **0.3.1** fix.
See the [release changes](changelog.md#031--27-october-2026).

- **Bazzite installation:** The installer rejects symbolic links in its source
  and destination paths, including Fedora Atomic's `/home` alias. Install from
  Desktop Mode using the [physical-path workaround](install.md#bazzite-and-symbolic-link-errors).
  The workaround passed a synthetic installer test; Bazzite simulator
  compatibility remains unverified.

- **Game downloads:** Version 0.2.21 offers **Retry download** after a failed
  resumable download or sign-in while its background service remains running.
  The existing folder is reused with completed-file checks. Numeric setup
  failures are retained in diagnostics even before the first installation.
  Older reports cannot establish missing error codes, and jobs ended by an
  older version or a service restart cannot be recovered through this button.
  [Recovery and limits](download-pause.md).

- **Xbox download authentication (reported on 0.3.0):** Browser sign-in can
  succeed while the subsequent Xbox user-token request returns HTTP 400 and
  `X-Err: 00000005`. The bundled Xodus then panics with download code 101.
  This was reproduced locally without a proxy. Version **0.3.1** uses a Windows
  SISU replacement that
  succeeds with the same account: authenticated MSFS package metadata returns
  HTTP 200 and the Microsoft CDN metadata transfer starts. The replacement
  retains user, device and application authentication, including the same device
  proof key. Update Flightdeck to obtain the corrected native components.
  A complete new installation and the original reporter's retest remain pending.
  The code does not establish an account ban or missing purchase.
  [Upstream PR #201](https://github.com/xodus-gaming/xodus/pull/201) is a separate
  device-authentication experiment; the Flightdeck change requires a user token.

- **NVIDIA:** Normal MSFS 2024 rendering is confirmed by user testing. Smaller
  issues remain possible, including a missing or black startup video. This does
  not qualify every GPU/driver combination, VR or NVIDIA's optional features.
  Version 0.2.13 prefers software H.264 decoding and fixes codec selection for
  supported Proton builds. For the Nobara/RTX 5080 launch crash, the reporter
  [confirms that an alternative Proton such as Proton-GE works](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/3#issuecomment-6348916712).
  The default Xodus crash remains unresolved; the working Proton and Flightdeck
  versions have not yet been supplied.
  If the main map or cockpit is also black, report it separately.
  [Graphics setup and reporting](graphics.md).
- **Alternate Proton:** Version 0.2.14 repairs Xbox sign-in error
  `000-2000-0x80004001`, confirmed with Proton Experimental 11 in MSFS 2024.
  CachyOS 10 sunset still exits with code 84 before authentication completes;
  use the default Flightdeck environment or another tested runner.
  With **0.2.20 and CachyOS 11.0-20261015**, the reporter reaches Start after
  waiting through the black loading screen with music; choosing Start opens
  the rendered menu and the simulator renders a scene. This is a reported
  startup workaround, not a video fix or complete-flight qualification.
  [Issue #4 follow-up](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/4#issuecomment-6377721086).
- **VSync:** The one-third-refresh-rate limit exceeding 48 FPS on a 144 Hz
  screen is still reported on **0.2.20**. The latest issue #4 screenshot shows
  about 56 FPS with “VSync One-third rate”. The cause and a correction remain
  unverified; successful startup with another runner does not resolve it.
- **Fenix v3:** Patch 0.1.0 is experimental. Official installation, login
  and cockpit displays were checked on Xodus with Fenix 3.0.0.7454. One startup
  hit Wine's `create_view` assertion; six later starts, including five warm
  backend restarts, did not reproduce it. The cause, full-flight stability and
  the earlier simulator display freeze remain unresolved. Preview.11 fixes the
  Gateway loader-name check; the Steam MSFS 2020 / CachyOS 11 / NVIDIA reporter
  confirms working Gateway/EFB and GPU displays with **USE DISPLAY SAFE MODE
  off**. Other runners have matching native module tests; this report does not
  establish full-flight stability.
  [Test sequence and limits](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0/docs/fenix-v3.md).
- **Fenix profile/runner copy:** Version 0.2.27 fixes an archive-metadata failure
  reproduced on a fixed-permission NTFS mount: all files were copied, but restoring
  Linux permissions caused setup to stop in its preparing stage. The new copy
  passed full file/hash and link comparisons and a fresh Fenix patch setup.
  A missing `previous_prefix` directory during this stage is expected: the
  original profile moves to that restore point only when setup commits.
  [Issue #7](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/7)
  needs a reporter retest; its diagnostic export does not identify the filesystem
  or the underlying copy error.
- **Fenix fonts:** Flightdeck 0.2.28 and standalone Fenix patch 0.1.1 handle
  Tahoma symlinks in Wine profiles and runners. Earlier installers can stop
  with “Expected a regular file: tahoma.ttf”. In the standalone installer, use
  **Restore original profile**, then retry with 0.1.1. Flightdeck users need
  the launcher update as well as the patch update.
- **Interrupted Fenix patch updates:** Flightdeck 0.2.29 keeps setup failures
  visible under **Updates** and offers **Retry Fenix setup** for an unchanged
  preparation stage, including after restarting the launcher. Close MSFS and
  Fenix first. The retry verifies the preceding patch and retains its aircraft,
  settings and restore chain; it reuses the verified download. Changed files or
  a transaction already publishing its profile require recovery under **Mods**.
- **Fenix installation:** The message “application install hook failed” can
  leave FenixApp files present without completing setup. A reproduced ICU/.NET
  case is repairable; the generic warning alone does not identify the cause.
  Close the installer, then use **Mods → Fenix → Repair Fenix app** when offered.
  If repair fails, **Open diagnostics** is the next step; further repair or
  reinstallation is under **Manage & repair**. The latest recorded failure
  survives a launcher restart. The green **Fenix Linux patch** check confirms
  only the patch, not the application hook, aircraft installation or activation.
  Version 0.2.8 filters inherited host .NET overrides: a controlled FenixApp
  1.0.286 startup-hook test changed from exit 82 to exit 0. This does not establish
  that users’ reported exit-82 failures are resolved; the code alone does not
  identify the cause.
  [Fenix setup](addons.md#fenix-a320) · [Validation](native-ui.md#fenix-install-hook-warning).
- **GSX Pro:** Experimental. The official FSDT installer has been tested in an
  isolated Wine profile; licensed installation, activation, Couatl/SimConnect,
  the in-game menu and ground services remain unverified. **GSX locally
  configured · Functionality unconfirmed** describes local setup only.
  [Test boundaries and setup](addons.md#gsx-pro-experimental).
- **Interrupted sessions and controls:** Since 0.2.6 the launch stayed blocked
  until a reboot when a process of the previous session survived. Version 0.2.9
  ends such processes and names a survivor in the launch note; a process stuck
  in the kernel after a GPU device loss still needs a restart.
  [Details](cloud-saves.md#backups-and-interruptions).
- **Cloud saves:** Repeated sync errors remain reported. Use the recovery and
  local-play options in the [cloud-save guide](cloud-saves.md). Local play does
  not confirm a successful cloud transfer.
- **Microsoft sign-in:** Update and restart Flightdeck before retrying. If it
  still fails, include the displayed error code and launcher version.
  [Sign-in and Store recovery](store-session-refresh.md).
- **Marketplace:** A blocking “Marketplace session expired” message remains
  reported inside MSFS. Restarting the simulator is a recovery step; the issue
  is not confirmed permanently fixed. Completed purchases and delivery of paid
  content remain unverified. [Marketplace scope](marketplace-collections.md).
- **VR:** OpenXR setup is available for WiVRn, SteamVR and Monado. Stereo tests
  pass on AMD with a simulated headset; physical headsets, NVIDIA VR and actual
  MSFS VR flights need separate validation. [VR setup](vr.md).

See [compatibility](../README.md#compatibility) for simulator and add-on coverage.
Use [problem reports](problem-reports.md) to share selected diagnostics.
