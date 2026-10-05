# Known issues

[Deutsch](known-issues.de.md)

Status: Flightdeck 0.2.3, 5 October 2026.

**VR:** Optional OpenXR setup is available for WiVRn, SteamVR and Monado.
D3D11/D3D12 stereo frames pass on AMD with a simulated Monado headset. Physical
headsets, NVIDIA hardware and actual MSFS VR flights remain unverified.
[Setup and test scope](vr.md).

**Component updates:** 0.1.10 fixes older Store binaries being left behind when
launch scripts were customized. Update and reopen Flightdeck with the simulator
closed; the Store check should then pass its component step. Customized scripts
are retained. This does not by itself confirm the online checks below.

The following reported issues are not yet confirmed resolved and do not affect every
installation. A sign-in failure can prevent testing simulator rendering.

## Current scope

- **Microsoft sign-in:** 0.1.16 handles verification steps embedded in a
  multi-token response, shortened SOAP fault headers and encrypted replies
  that previously failed with code 74. It retains the session-cookie and
  callback fixes from 0.1.15. Update and restart before retrying. If sign-in
  still fails, report the displayed code and version; successful sign-in on
  every affected setup is not yet established. A connection to the NVIDIA
  issue is not established.
  [Correction and validation scope](store-session-refresh.md#soap-response-correction-0116).
- **NVIDIA:** MSFS 2024 can show working menus and overlays while the world map
  and primary 3D view stay black. 0.1.11 corrects DXVK's ignored Low Latency
  opt-out and automatically uses the complete DirectX 11/12 compatibility
  profile. DLSS, Reflex and NVIDIA Frame Generation are disabled by default.
  One user reported that the later startup fixes restored the main view, while
  the loading video remained black. This is user feedback, not confirmation
  across NVIDIA hardware and drivers. A working second window alone does not
  confirm the main view.
  [Graphics modes and current status](graphics.md).
- **Fenix installation:** 0.2.3 addresses the reproduced ICU install-hook
  failure and waits for detached installer children. The official install hook
  passes in an isolated prefix. The generic warning in the reported screenshot
  does not establish that user's exact cause. Retry through **Mods → Fenix →
  Start installer** and close its windows when finished.
  [Correction and validation](native-ui.md#fenix-install-hook-warning).
- **Cloud saves:** repeated sync errors remain reported after the 0.1.8 timeout
  corrections. Recovery and local-play options are described in the
  [cloud-save guide](cloud-saves.md). Local play does not confirm a successful
  cloud transfer.
- **Marketplace:** “Marketplace session expired” is reported when opening
  Marketplace inside the simulator. The message cannot be dismissed in the
  affected session. It is distinct from Flightdeck's purchase-window errors;
  restarting the simulator is a recovery step, not a permanent fix.
  Flightdeck 0.1.9 renews expiring tickets, preserves the broker
  context for the same account and provides interactive sign-in with subsequent
  verification. Recovery is available with the simulator closed. These fixes
  are not an end-to-end confirmation of this in-game symptom.
  [Store recovery scope](store-session-refresh.md).
  [Marketplace scope](marketplace-collections.md).

See [compatibility coverage](../README.md#compatibility) for other unverified
features, including MSFS 2020 startup and completed paid purchases.
