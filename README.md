<p align="center">
  <img src="ui/mark.svg" width="76" alt="Flightdeck logo">
</p>
<h1 align="center">Flightdeck</h1>
<p align="center">
  A Linux launcher for the Xbox PC edition of Microsoft Flight Simulator 2024.
</p>
<p align="center">
  <a href="#get-started"><strong>Get started</strong></a> &nbsp;·&nbsp;
  <a href="docs/addons.md">Add-ons</a> &nbsp;·&nbsp;
  <a href="docs/game-updates.md">Game updates</a> &nbsp;·&nbsp;
  <a href="BUILDING.md">Build from source</a> &nbsp;·&nbsp;
  <a href="docs/readme.de.md">Deutsch</a>
</p>

![Original Flightdeck aircraft artwork](ui/flight-panorama.png)

Flightdeck installs and launches your **purchased Xbox PC / Microsoft Store copy
of MSFS 2024** on your Linux computer through Wine/Proton. Sign in with your
Microsoft account, download the game and start it from one application.

**Experimental.** Local simulator execution and a controlled takeoff have been
observed, and online multiplayer has been reported working on Linux. Automatic Xbox cloud saves are experimental. [See the current evidence below.](#compatibility)

## Get started

**1. Install Flightdeck**

Get **Flightdeck-Linux-x86_64.tar.gz** from the
[releases page](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases),
extract it and double-click **Install Flightdeck.desktop**. Your file manager
may ask you to trust this local launcher.

**2. Install your game**

Choose **Install MSFS**, check the destination and select **Sign in & install**.
Use the Microsoft account that owns the PC edition. Flightdeck prepares the
Wine environment, downloads the licensed game through Xodus and connects it to
the launcher.

**3. Start from your application menu**

Open **Flightdeck** and launch MSFS. The local background service starts
automatically. You do not need to start a server or leave a terminal open.

Windows, the Xbox app, Microsoft Store and a previous MSFS installation are
not required. Your purchased PC license and an Internet connection are required.

<details>
<summary><strong>System requirements and other installation options</strong></summary>

The current binary package targets Linux x86-64 with **glibc 2.39+**, Python
3.10.12+, Vulkan graphics drivers and a graphical desktop with a Linux Secret
Service keyring. GTK 3, WebKitGTK 4.1, OpenSSL 3 and the GStreamer Good/Bad/Libav
media plugins are required. Allow at least **100 GiB free** for a first install;
an update or repair also keeps the previous game package.

Arch Linux has been tested. Other distributions need compatible libraries and
remain unverified. Setup reports missing prerequisites before installation.
Install any missing system packages through your distribution's software manager.
Chromium provides the application window when available; the default browser
is the fallback.

- [Installation details](docs/install.md)
- [Connect an existing prepared runtime](docs/runtime.md)
- [Build your own components](BUILDING.md)

`./install.sh` performs the same user-local installation from a terminal. Run
it from a newer extracted package to update Flightdeck. `flightdeck --rollback`
restores the previous **launcher**; `flightdeck --uninstall` removes its managed
files while preserving the runtime, settings and saves. No `sudo` is needed.
Installing missing Linux system packages may require administrator rights.

</details>

## In the launcher

![Flightdeck launcher overview](docs/images/launcher-overview.png)

<sub>Flightdeck launcher shown with example installation data.</sub>

| Area | What you can do |
| :--- | :--- |
| **Install** | Sign in, choose a destination and download your purchased PC edition, with received MB/GB and percentage when the total is known. |
| **Pause & resume** | Keep completed, verified download files and resume the active installation session. |
| **Game updates** | Compare the installed Store package with the current release, then download and activate a checked version. Keep the previous version for rollback. |
| **Verify & repair** | Check game files against the original download checksums. Prepare a full repair when files are missing or damaged, including at the same game version. |
| **Mods** | Open the real Community folder and see installed package names, versions and creators. |
| **Local saves** | Keep local save data and create backups while the simulator is stopped. |
| **Xbox cloud saves** | Use the cloud state before play and upload changes after exit, with local backups and conflict recovery. Experimental. |
| **Diagnostics** | Export selected checks without raw game logs, account tokens or save contents. |
| **Deutsch / English** | Switch the interface language; installer and command-line localization are available too. |

Pausing may restart up to four unfinished files. Download recovery after a
service or system restart is not implemented. Updates download the full Store
base-game package; additional content remains managed by MSFS and each add-on's
own installer. [Download behavior](docs/download-pause.md) ·
[Updates, verification and repair](docs/game-updates.md)

The download total covers the Store base-game files. For MSFS 1.8.16.0 these
totalled about **9.9 GB**; other versions may differ. Wine/Proton, additional
content downloaded inside MSFS, caches and add-ons need separate space. The
simulator runs locally while MSFS streams additional content as needed. Keep the
**100 GiB free-space requirement** even when the progress bar shows a smaller
download. [Download size and storage](docs/install.md#download-size-and-storage)

File verification requires a complete index from a successful Flightdeck
download. Older installations without this index need a full repair first.
Repair downloads the complete available base package and retains the previous
installation; it does not replace your separate Community folder or saves.

## Compatibility

| Area | Current evidence |
| :--- | :--- |
| **Simulator** | Cockpit reached and one controlled takeoff completed on a development system. |
| **Local saves** | Persistence across restarts and local backup tested. |
| **Free Store content** | Free-content downloads succeeded in a user test. |
| **Paid Marketplace purchases** | Checkout and complete DLC inventory are not yet verified. |
| **Multiplayer** | Online multiplayer reported working on Linux. Group invitations still need separate testing. |
| **Xbox cloud saves** | Automatic start/exit sync, local backups and conflict recovery implemented. Native cloud read/write tested; cross-device gameplay verification remains pending. [Details](docs/cloud-saves.md) |
| **FlyByWire A32NX** | MSFS 2024 Stable 2024.1.0 installed and recognized. In-game flight test remains open. |
| **SimBridge** | HTTP health, Web MCDU, WebSocket and terrain initialization tested under Wine. Simulator connection remains unverified. |
| **Fenix A320** | Official installation of 2.4.0.4720 completed with Wine/.NET adjustments; the Community package is recognized. Companion startup and aircraft operation remain unverified. |

The first-install and update workflows have component, synthetic full-flow and
browser tests. A complete fresh MSFS download, runtime setup, pause/resume and
full file verification have also passed on an Arch Linux workstation using
separate launcher and account storage. Installation on a fresh operating system
and reaching the main menu after that fresh game installation remain unverified,
as does a flight after a real Store update. A package in the mod
list confirms its local files, not working aircraft systems or activation.

[Add-on installation guide](docs/addons.md) ·
[Marketplace scope](docs/marketplace-collections.md) ·
[Multiplayer test steps](docs/multiplayer.md)

## Local by design

The launcher uses Python's standard library and local HTML, CSS and JavaScript.
Its service listens only on loopback. Local-origin checks and a per-session token
protect actions. There is no telemetry or CDN dependency in the launcher.
Microsoft sign-in, downloads and the simulator's online content still use their
respective network services.

Runtime files, credentials, saves and logs stay outside the source and release
archives. The interface can show installation paths locally; review screenshots
before sharing them. Closing Flightdeck leaves an already running simulator
alone.

## Documentation and development

| Guide | Covers |
| :--- | :--- |
| [Install](docs/install.md) | Requirements, setup and launcher maintenance |
| [Cloud saves](docs/cloud-saves.md) · [Deutsch](docs/cloud-saves.de.md) | Automatic sync, conflict recovery, backups and current limits |
| [Game maintenance](docs/game-updates.md) | Updates, file verification, full repair and rollback |
| [Add-ons](docs/addons.md) · [Deutsch](docs/addons.de.md) | Community packages, FlyByWire, SimBridge and Fenix |
| [Languages](docs/localization.md) | Interface, installer and command-line languages |
| [Build](BUILDING.md) | Native components and corresponding sources |
| [Contribute](docs/contributing.md) | Development and useful bug reports |

<details>
<summary><strong>Run the checks locally</strong></summary>

```sh
python3 -m unittest discover -s tests -p 'test_*.py' -v
python3 -m unittest discover -s tests/compat -p 'test_*.py' -v
node --test ui/tests/*.test.mjs
node ui/tests/browser-test.mjs
python3 scripts/check-source-export.py
```

The browser suite needs Chromium. HTTP and browser tests bind temporary local
sockets and use isolated fixtures, not your account or live game. Native
compatibility checks require the toolchain described in [BUILDING.md](BUILDING.md).
For a foreground development service, use `python3 -m flightdeck --no-browser`.

</details>

## License

The launcher, UI and new integration tooling use the [MIT license](LICENSE).
Xodus retains GPL-3.0-only; Wine/GDK-derived components retain LGPL-2.1-or-later.
Native packages include dependency notices and complete corresponding sources;
the separately downloaded runner retains its own licenses. The bundled Manrope
font uses the SIL Open Font License 1.1.

[Third-party notices](compat/THIRD_PARTY_NOTICES.md) ·
[Native package provenance](docs/binary-release.md) ·
[Artwork provenance](docs/artwork.md)

Flightdeck is an independent project, not endorsed by Microsoft, Xbox, Asobo or
the upstream projects. It distributes no MSFS files, paid add-ons, proprietary
SDK headers, account credentials or game licenses. The cover is original
Flightdeck artwork, not a simulator screenshot.
