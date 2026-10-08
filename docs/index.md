# Flightdeck documentation

[Overview and quick start](../README.md) · [Deutsch](readme.de.md)

This documentation covers the current stable release, **0.2.7**. The
[changelog](changelog.md) records its changes and earlier releases.
Hardware reports describe the tested setup, not universal compatibility.

## Using Flightdeck

| Guide | Start here when you need to… |
| --- | --- |
| [Install and update](install.md) | Check Linux requirements, install, update, roll back or uninstall the launcher. |
| [Game updates and repair](game-updates.md) | Update MSFS, verify files, repair the full package or restore a previous version. |
| [Download pause/resume](download-pause.md) | Understand retained files, progress and restart limitations. |
| [Graphics](graphics.md) · [Deutsch](graphics.de.md) | Set up AMD/NVIDIA graphics and investigate rendering issues. |
| [Known issues](known-issues.md) · [Deutsch](known-issues.de.md) | Check current limitations before troubleshooting. |
| [Cloud saves](cloud-saves.md) · [Deutsch](cloud-saves.de.md) | Use sync, local backups, conflict recovery and interrupted-session recovery. |
| [Add-ons](addons.md) · [Deutsch](addons.de.md) | Set up Community packages, Fenix, FlyByWire, SimBridge or experimental GSX. |
| [VR](vr.md) · [Deutsch](vr.de.md) | Prepare OpenXR and check the current headset validation scope. |
| [Problem reports](problem-reports.md) · [Deutsch](problem-reports.de.md) | Review diagnostics and prepare a useful report. |
| [Languages](localization.md) | Choose German or English for the desktop, installer and CLI. |
| [Existing runtimes](runtime.md) | Connect an already prepared installation. |
| [Marketplace](marketplace-collections.md) | Understand account-owned content, licensing, purchases and their limits. |
| [Multiplayer](multiplayer.md) | Follow the current test procedure and reported results. |

## Development and releases

- [Build and test](../BUILDING.md): toolchains, native UI checks, compatibility
  components and release packaging.
- [Contribute](contributing.md): code ownership, isolated tests, review and
  evidence required for compatibility claims.
- [Native desktop](native-ui.md): UI behavior, interaction checks and validation.
- [Binary package provenance](binary-release.md): pinned inputs, notices and
  corresponding-source obligations.
- [Third-party notices](../compat/THIRD_PARTY_NOTICES.md), [license](../LICENSE)
  and [artwork provenance](artwork.md).
- [Release history](changelog.md): version-specific changes.
- [Rust transition](rust-transition.md) and [migration status](rust-migration.md):
  historical updater ordering and the transition from Python/web to Rust.

## Technical background

These pages describe implementation and test evidence rather than setup steps.

- [Performance](performance.md): measured launcher/UI behavior and test methods.
- [NVIDIA renderer](nvidia-renderer.md) and [Steam/Proton comparison](nvidia-steam-parity.md):
  renderer provenance, compatibility probes and remaining differences.
- [Connected Storage protocol](connected-storage-protocol.md): native save operations.
- [Store session refresh](store-session-refresh.md): session-lifetime behavior.
- [Diagnostic follow-up](diagnostic-follow-up.md): interpreting remaining diagnostic cases.
- [VR validation](vr-validation.md): isolated graphics and headset-test scope.
- [Experimental neural rendering](neural-rendering.md): separate experimental
  work; not an established NVIDIA feature or general compatibility guarantee.
