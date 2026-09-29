# Known issues

[Deutsch](known-issues.de.md)

Status: Flightdeck 0.1.10, 29 September 2026.

**Component updates:** 0.1.10 fixes older Store binaries being left behind when
launch scripts were customized. Update and reopen Flightdeck with the simulator
closed; the Store check should then pass its component step. Customized scripts
are retained. This does not by itself confirm the online checks below.

The following reported issues are not yet confirmed resolved and do not affect every
installation. The NVIDIA black main view is the current priority, followed by
cloud-sync and Marketplace session reliability.

## Current scope

- **NVIDIA:** MSFS 2024 can show working menus and overlays while the world map
  and main 3D view remain black. Neither graphics mode is a confirmed fix, and
  a second render window can cause a crash. The 0.1.9 renderer contains upstream fixes,
  but has not yet been verified to resolve the black main view on NVIDIA.
  A working second window does not resolve a black primary window.
  [Graphics modes and current status](graphics.md).
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
