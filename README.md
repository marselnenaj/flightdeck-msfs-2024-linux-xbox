<p align="center">
  <img src="ui/mark.svg" width="76" alt="Flightdeck logo">
</p>
<h1 align="center">Flightdeck</h1>
<p align="center">
  A Linux launcher for the Xbox PC editions of Microsoft Flight Simulator 2024 and 2020.
</p>
<p align="center">
  <a href="#get-started"><strong>Get started</strong></a> &nbsp;·&nbsp;
  <a href="docs/index.md">Documentation</a> &nbsp;·&nbsp;
  <a href="docs/known-issues.md">Known issues</a> &nbsp;·&nbsp;
  <a href="docs/changelog.md">Changes</a> &nbsp;·&nbsp;
  <a href="docs/readme.de.md">Deutsch</a>
</p>

![Original Flightdeck aircraft artwork](ui/flight-panorama.png)

Flightdeck installs and launches your **purchased Xbox PC / Microsoft Store copy
of MSFS 2024 or 2020** through Wine/Proton. Sign in with your Microsoft account,
download the licensed game and start it from the native Rust desktop application.
Windows, the Xbox app and a previous MSFS installation are not required.

**Current stable release: [Flightdeck 0.2.6](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.6).**
The launcher, installer and updater need no Python.

**New in 0.2.6:** session recovery, launch/stop and edition-selection fixes,
clearer Fenix app detection and repair, and automatic update checks with readable
release notes. See the [changelog](docs/changelog.md) and [native UI guide](docs/native-ui.md).

## Get started

1. Download **Flightdeck-Linux-x86_64.tar.gz** from the
   [0.2.6 release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.6),
   extract it and open **Install Flightdeck.desktop**. Your file manager may ask
   you to trust this local launcher. Alternatively, run `./install.sh` there.
2. Choose **Install MSFS**, select 2024 or 2020 and a destination, then
   **Sign in & install** with the Microsoft account that owns the PC edition.
3. Open **Flightdeck** from your application menu and start the simulator.
   Select an installed edition on the overview to switch between separate
   runtimes, Wine prefixes, updates and local saves.

An Internet connection and a purchased PC license are required. Allow at least
**100 GiB free**, including room for streamed content, caches and retained game
versions. The displayed base-game download size is only part of that storage.

The binary package requires Linux x86-64 with **glibc 2.39+**, Vulkan drivers,
a Wayland or X11 desktop and a Linux Secret Service keyring. The full runtime
also needs GTK 3, WebKitGTK 4.1, OpenSSL 3 and GStreamer Good/Bad/Libav plugins;
the native window needs libxkbcommon. Setup checks missing prerequisites.
Arch Linux has been tested; other distributions need separate validation.
[Requirements and installation options](docs/install.md)

Install newer launchers through **Updates → Flightdeck** or their full installer.
For versions 0.1.21 and earlier, the 0.2.0 preview or withdrawn 0.2.0 release,
use the full installer once; existing runtimes and saves are preserved.
[Updates and rollback](docs/install.md#update-and-rollback) ·
[Older-version migration](docs/rust-transition.md)

## In the launcher

| Area | What it provides |
| --- | --- |
| **Install and updates** | Licensed downloads, pause/resume, game-file verification, full repair and rollback. [Details](docs/game-updates.md) |
| **Mods** | Community-folder management and optional Fenix/GSX setup through their official installers. [Guide](docs/addons.md) |
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
MSFS 2020 installation and license checks are implemented; successful simulator
startup and complete-flight compatibility remain unconfirmed.

Fenix cockpit rendering has been tested, while complete-flight coverage, GSX
operation, physical VR headsets and cross-device cloud-save gameplay still need
validation. Marketplace session errors and incomplete DLC/purchase coverage
remain documented separately. Installed files alone do not prove activation or
working aircraft systems.
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
