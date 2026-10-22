# Known issues

[Deutsch](known-issues.de.md) · [Documentation](index.md)

Status: 22 October 2026, covering the stable **0.2.17** release.
See the [release changes](changelog.md#0217--22-october-2026).

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
  [Issue #4](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/4).
- **VSync:** The one-third-refresh-rate limit exceeding 48 FPS on a 144 Hz
  screen, also reported in issue #4, remains unverified.
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
