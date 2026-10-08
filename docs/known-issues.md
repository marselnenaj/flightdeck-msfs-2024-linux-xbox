# Known issues

[Deutsch](known-issues.de.md) · [Documentation](index.md)

Status: 8 October 2026, covering the stable **0.2.7** release.
See the [release changes](changelog.md#027--8-october-2026).

- **NVIDIA:** Normal MSFS 2024 rendering is confirmed by user testing. Smaller
  issues remain possible, including a missing or black startup video. This does
  not qualify every GPU/driver combination, VR or NVIDIA's optional features.
  If the main map or cockpit is also black, report it separately.
  [Graphics setup and reporting](graphics.md).
- **Fenix installation:** The message “application install hook failed” can
  leave FenixApp files present without completing setup. A reproduced ICU/.NET
  case is repairable; the generic warning alone does not identify the cause.
  Close the installer, then use **Mods → Fenix → Repair Fenix app**, introduced
  in 0.2.6. When available, it appears in 0.2.7 as the next action after a failed
  hook, or under **Manage & repair**. This reruns the official setup hook for an
  existing app, and diagnostics record the latest attempt. Detecting FenixApp
  does not mean that the aircraft is installed or activated.
  [Fenix setup](addons.md#fenix-a320) · [Validation](native-ui.md#fenix-install-hook-warning).
- **GSX Pro:** Experimental. The official FSDT installer has been tested in an
  isolated Wine profile; licensed installation, activation, Couatl/SimConnect,
  the in-game menu and ground services remain unverified. **GSX locally
  configured · Functionality unconfirmed** describes local setup only.
  [Test boundaries and setup](addons.md#gsx-pro-experimental).
- **Interrupted sessions and controls:** In 0.2.6, safe session recovery,
  stale service-socket recovery and corrected Start/Stop/edition-selection states
  address launch blocks left after an interrupted run.
  [Details](native-ui.md#session-and-control-fixes-in-026).
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
