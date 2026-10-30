<p align="center">
  <img src="ui/mark.svg" width="76" alt="Flightdeck logo">
</p>
<h1 align="center">Flightdeck</h1>
<p align="center">
  A Linux launcher for Microsoft Flight Simulator 2024 and 2020 on Xbox PC and Steam.
</p>
<p align="center">
  <a href="#get-started"><strong>Get started</strong></a> &nbsp;·&nbsp;
  <a href="docs/index.md">Documentation</a> &nbsp;·&nbsp;
  <a href="docs/known-issues.md">Known issues</a> &nbsp;·&nbsp;
  <a href="docs/changelog.md">Changes</a> &nbsp;·&nbsp;
  <a href="https://discord.gg/fAhHR2DkqN">Discord</a> &nbsp;·&nbsp;
  <a href="docs/readme.de.md">Deutsch</a>
</p>

![Original Flightdeck aircraft artwork](ui/flight-panorama.png)

Flightdeck installs and launches your **purchased Xbox PC / Microsoft Store copy
of MSFS 2024 or 2020** through Wine/Proton. Sign in with your Microsoft account,
download the licensed game and start it from the native Rust desktop application.
Windows, the Xbox app and a previous MSFS installation are not required for Xbox PC setup.
Flightdeck also detects existing **Steam installations** and manages their optional
Fenix, GSX, Navigraph Hub/Simlink, Orbx Central and MobiFlight setup; Steam handles game
downloads, updates and account sign-in.

**Download the [latest stable release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/latest).**
The launcher, installer and updater need no Python.

**New in 0.3.7:** Complete Proton switching and recovery even when an old
profile cannot be safely reclaimed. Keep that backup and allow returning to the
default runner. Update ZIP/TLS/async dependencies and native authentication
cryptography, enable ZIP header consistency checks and integrate Fenix 0.1.4.

**Since 0.3.6:** validate the same GStreamer registry that Wine uses for Xbox
game launches.
A stale plugin blacklist is rebuilt once when a required video element fails,
without resetting the game profile. The reproduced false-positive probe and
failed 4K decode now recover in the same isolated profile. Diagnostics retain
cache recovery, missing-plugin architecture and maintenance-preview failure
phase/errno. The reporter now confirms reaching the main menu with the default
runner in 0.3.6; full flights remain unqualified.
[Reproduction and tested scope](docs/media-validation.md)

**New in 0.3.5:** [MobiFlight / WinWing setup](docs/mobiflight.md), GSX engine
repair, bounded X11 startup checks and a repair for the native-only CLR override
behind the Fenix loader failure in #10. Hardware connections and full-flight
addon behavior remain experimental. The loader correction was reproduced in an
account-free Wine profile; full Fenix installer validation remains outstanding.

**New in 0.3.4:** verify that video decoders and Wine's post-processing elements
actually load before selecting a Proton media stack. This fixes a cached-factory
validation defect and improves safe diagnostics for missing codecs. The corrected
CachyOS 20261015 fallback passed the real 4K loading-video test, including rewind
and completed shutdown. The reporter has since confirmed the default-runner
main-menu launch with 0.3.6; alternate-runner recovery is addressed in 0.3.7. [Reproduction and tested scope](docs/media-validation.md)

**New in 0.3.3:** optional Orbx Central, Navigraph Hub and Simlink setup, plus
GSX preparation for the selected **Xbox or Steam** MSFS 2020/2024 profile. These
experimental integrations use the game's existing Wine profile and selected
Proton. Navigraph browser sign-in returns to the waiting app; Orbx avoids the
failing web updater and Wine tray window. Full-flight add-on compatibility still
needs tester validation. [Setup and tested scope](docs/addons.md)

Xbox downloads retain the SISU authentication and content-licensing fixes from
0.3.2, verified with a complete isolated MSFS 2024 download of all 28,621 files.

**Steam support:** Select Steam MSFS 2020/2024 alongside Xbox installations and
manage Fenix through the shared standalone Rust engine. Steam support is beta;
actual simulator flights still need tester qualification. See the
[Steam guide](docs/steam.md) and [changelog](docs/changelog.md).

Join the [Flightdeck Discord community](https://discord.gg/fAhHR2DkqN) to ask
questions, share your Linux setup and help develop Flightdeck. For reproducible
bugs, use [GitHub Issues](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues).

[Credits and acknowledgements](docs/credits.md) name the collaborators and
projects behind Flightdeck. The launcher also has a **Credits** page.

## Get started

1. Download **Flightdeck-Linux-x86_64.tar.gz** from the
   [latest release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/latest),
   extract it and open **Install Flightdeck.desktop**. Your file manager may ask
   you to trust this local launcher. Alternatively, run `./install.sh` there.
2. For Xbox PC, choose **Install MSFS**, select 2024 or 2020 and a destination,
   then **Sign in & install** with the Microsoft account that owns the PC edition.
   For Steam, install the Windows edition through Steam, then select the detected
   installation in Flightdeck. [Steam setup](docs/steam.md)
3. Open **Flightdeck** from your application menu and start the simulator.
   Select an installed edition on the overview to switch between separate
   runtimes, Wine prefixes, updates and local saves.

**Bazzite:** Install in Desktop Mode. If the installer reports that its path
contains a symbolic link, follow the [installation workaround](docs/install.md#bazzite-and-symbolic-link-errors).

An Internet connection and a purchased PC license are required. Allow at least
**100 GiB free**, including room for streamed content, caches and retained game
versions. The displayed base-game download size is only part of that storage.

The binary package requires Linux x86-64 with **glibc 2.39+**, Vulkan drivers,
a Wayland or X11 desktop and a Linux Secret Service keyring. The full runtime
also needs GTK 3, WebKitGTK 4.1, OpenSSL 3 and GStreamer Good/Bad/Libav plugins;
the launcher also needs liblzma, libgcc_s and libxkbcommon, plus the selected
Wayland/X11 libraries. Setup checks missing prerequisites.
Launcher checks passed on Ubuntu 24.04, Debian 13, Fedora 44, Arch Linux and
openSUSE Tumbleweed on 8 October 2026. See the
[Linux test matrix](docs/install.md#linux-test-matrix) for the tested scope.
These container checks do not establish simulator or GPU-driver compatibility.
[Requirements and installation options](docs/install.md)

Install newer launchers through **Updates → Flightdeck** or their full installer.
For versions 0.1.21 and earlier, the 0.2.0 preview or withdrawn 0.2.0 release,
use the full installer once; existing runtimes and saves are preserved.
[Updates and rollback](docs/install.md#update-and-rollback) ·
[Older-version migration](docs/rust-transition.md)

## In the launcher

| Area | What it provides |
| --- | --- |
| **Install and updates** | Licensed downloads, retry and pause/resume, game-file verification, full repair and rollback. [Details](docs/game-updates.md) |
| **Mods** | Community-folder management and optional Fenix, GSX, Navigraph and Orbx setup in the selected profile. [Guide](docs/addons.md) |
| **Saves** | Local backups and experimental automatic Xbox cloud sync with conflict recovery. [Guide](docs/cloud-saves.md) |
| **Graphics and VR** | Automatic GPU setup, NVIDIA compatibility options and optional OpenXR setup. [Graphics](docs/graphics.md) · [VR](docs/vr.md) |
| **Diagnostics** | Selected checks and reviewable problem reports without raw game logs, tokens or save contents. [Guide](docs/problem-reports.md) |

The interface supports German and English. The local background service starts
automatically; no terminal needs to remain open. Closing Flightdeck leaves a
running simulator alone. Download recovery after a service or Linux restart is
not yet supported. [Pause/resume limits](docs/download-pause.md)

## Compatibility

**Experimental.** MSFS 2024 cockpit access and takeoff have been tested on AMD.
**NVIDIA rendering is confirmed by user testing**, with remaining minor issues
such as missing startup video. This does not establish support for every GPU or driver.
A user reports reaching the rendered MSFS 2024 menu and scene with CachyOS
11.0-20261015 on 0.2.20 after waiting through a black loading screen. Startup
video and the 48-FPS VSync limit remain open. [Reported scope](docs/known-issues.md).
MSFS 2020 installation and license checks are implemented; successful simulator
startup and complete-flight compatibility remain unconfirmed.

Fenix cockpit rendering has been tested, while complete-flight coverage, GSX
operation, physical VR headsets and cross-device cloud-save gameplay still need
validation. Marketplace session errors and incomplete DLC/purchase coverage
remain documented separately. Installed files alone do not prove activation or
working aircraft systems.
Fenix 0.1.0 retains GPU display sharing for the pinned Xodus and Proton 11
runners, with a CPU fallback, and includes the Direct2D memory-leak fix.
Fenix v3 remains experimental; see the [tested scope](docs/addons.md#compatibility-and-recovery).
[Known issues](docs/known-issues.md) · [Add-on scope](docs/addons.md) ·
[Marketplace scope](docs/marketplace-collections.md)

## Local by design

The launcher uses Rust with a native desktop interface and a loopback-only API.
Local-origin checks and a per-session token protect every API read and action.
Desktop and headless clients obtain the token from the owner-private
`desktop-service.json` in the selected state directory and send it in the
`X-Flightdeck-Token` header; unauthenticated status requests cannot bootstrap a
session. There is no telemetry or CDN dependency in the launcher.
Microsoft sign-in, downloads and the simulator's online content still use their
respective network services.

Runtime files, credentials, saves and logs stay outside source and release
archives. The interface can show local paths; review screenshots before sharing.

## Documentation and development

Start with the [documentation index](docs/index.md) for user guides, development
instructions and technical references. [BUILDING.md](BUILDING.md) contains the
build and test commands; [contributing](docs/contributing.md) covers review and
release checks. Tests use isolated fixtures. Historical version bulletins are
in the [changelog](docs/changelog.md), and the retired Python/web application
remains available in Git history.

## License

The launcher, UI and new integration tooling use [MIT](LICENSE). Xodus retains
GPL-3.0-only; Wine/GDK-derived components retain LGPL-2.1-or-later. Native packages
include dependency notices and complete corresponding sources. The separately
downloaded runner retains its own licenses; Manrope uses SIL Open Font License 1.1.
[Third-party notices](compat/THIRD_PARTY_NOTICES.md) · [Package provenance](docs/binary-release.md)

Flightdeck is independent and not endorsed by Microsoft, Xbox, Asobo or the
upstream projects. It distributes no MSFS files, paid add-ons, proprietary SDK
headers, credentials or game licenses. The cover is original Flightdeck artwork,
not a simulator screenshot. [Artwork provenance](docs/artwork.md)
