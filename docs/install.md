# Install the Flightdeck launcher

This guide covers **Flightdeck 0.1.3**, including simulator selection, the Fenix
panel and managed runtime component updates. See [changes](changelog.md).

Download **Flightdeck-Linux-x86_64.tar.gz** from the
[releases page](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases)
and extract it on your Linux computer. This full installer includes the six
pinned compatibility components and their license notices. It needs Python
3.10.12 or newer and the Linux libraries listed below. Open
**Install Flightdeck.desktop** in the extracted folder to use the graphical
installer. Depending on the file manager, first choose **Allow Launching** or
confirm that this is a trusted local launcher. Flightdeck does not change that
desktop security setting for you.

The optional dialog uses an existing Zenity, kdialog or Python Tk installation.
There is no added mandatory Python dependency. If none is available, install
from that directory with one command:

```sh
./install.sh
```

The equivalent command is `python3 scripts/install-launcher.py`. The installer
uses only Python's standard library. It does not use sudo, pip, a virtual
environment or a remotely executed shell script.

The installer creates a **Flightdeck** application-menu entry and opens the
launcher as an application window when a compatible browser is available,
otherwise in the default browser. On first start, follow the setup view. No
terminal has to remain open. Subsequent starts are available from the application
menu or `~/.local/bin/flightdeck`. Closing the interface leaves a running
simulator alone.

`./install.sh --gui` explicitly selects the optional install dialog. It confirms
the action, shows progress with Zenity or Tk and reports failures locally. It
does not silently install if no graphical toolkit or desktop session is available.

This installs the launcher and its setup resources. In setup, choose **New
installation**, select **Microsoft Flight Simulator 2024** or **Microsoft Flight Simulator 2020**,
select a new destination and follow the Microsoft sign-in and
download steps for your purchased Xbox PC edition. An existing game installation
is not required. A prepared Wine prefix and existing game directory remain an
advanced import option.

Each edition gets its own prepared runtime, Wine prefix, download, update history
and local saves. In **Overview**, choose **MSFS 2024** or **MSFS 2020** to switch
directly; a missing edition opens its setup form. Flightdeck
never converts a 2024 installation into 2020 or replaces one with the other.
Older runtimes without a recorded edition remain MSFS 2024. MSFS 2020's PC Store
package is available in the public Microsoft catalog. Edition workflows have
synthetic coverage and live game-license/package probes passed. A startup disc
prompt was reported; a complete end-to-end installation and flight remain open.
See the [MSFS 2020 test status](marketplace-collections.md#package-checks-and-the-msfs-2020-disc-prompt).

Microsoft sign-in opens in a separate GTK/WebKitGTK window provided by Xodus.
It does not use the browser showing Flightdeck's local interface. If the
sign-in window is not visible, check the desktop's open windows before retrying;
if setup reports an error, keep the displayed phase and code for diagnosis.

Choose your Microsoft account's Store region from the visible dropdown before
sign-in. Flightdeck suggests a country from the local timezone or regional
locale; if neither identifies one, choose it explicitly. The installation
location can be changed separately. During the game download, select **Pause** to stop
network activity and **Resume** to continue. Complete files are verified before
reuse. Up to four unfinished files may restart, so pausing does not preserve
every byte already transferred. If the Microsoft sign-in has expired, Flightdeck
asks you to sign in again and keeps the verified files.
The progress bar shows received MB/GB and percentage once the content total is
known. Paused downloads keep those values visible.
[Progress and resume details](download-pause.md)

You can close and reopen the launcher window while its background service stays
running. Recovery after stopping that service, logging out or restarting Linux
is not supported yet. **Cancel** ends the installation; use **Pause** when you
intend to continue. Older component packages without resume support do not show
these controls.

Every bundled compatibility component must match
`compat/bootstrap.lock.json` before installation; unrelated binaries are never
admitted. Source archives and repository clones are advanced alternatives: they
do not include this binary bundle and require the documented
[component build](../BUILDING.md) or bootstrap path. Neither package includes
game files, account credentials, a game license or proprietary SDK files, and
setup does not purchase the game.

## Download size and storage

The game progress bar counts the files in the Microsoft Store base package. In
the tested MSFS **1.8.16.0** package, that was **9,875,881,987 bytes**, displayed
as about **9.9 GB**. This is an example for that version, not a fixed install size.
It excludes the separately downloaded Wine/Proton runner, download metadata,
Wine prefixes, caches and additional content downloaded inside the simulator.

MSFS 2024 downloads textures, models and world data as needed; see the
[official FAQ](https://www.flightsimulator.com/microsoft-flight-simulator-2024-faq/).
The game itself runs locally through Wine/Proton. This content streaming does
not use Xbox Cloud Gaming. Downloaded add-ons and retained previous game
versions also use extra disk space.

Plan for at least **100 GiB of free space** in the chosen destination; the
simulator may need more for its downloaded and streamed content.

## Linux prerequisites

The native package requires Linux x86-64, glibc 2.39+, GTK3, WebKitGTK 4.1, OpenSSL 3 and a
working Vulkan driver. GStreamer with its Good, Bad and Libav plugin sets must
provide `qtdemux`, `h264parse` and `avdec_h264` for the simulator's video playback.
Setup checks these plugins before starting the new installation. Install missing
components through your distribution's software manager; package names vary.
Flightdeck checks these requirements but does not install Linux system packages.
Your software manager may request administrator rights for that step.
These are host prerequisites, not Python dependencies.
[Runtime documentation](runtime.md) and [build instructions](../BUILDING.md)
describe the platform components and advanced paths. For automatic Xbox cloud
saves and local backups, see [cloud saves](cloud-saves.md).

## Language

The installer, its help and follow-up commands support German and English:

```sh
./install.sh --language en
./install.sh --language de
~/.local/bin/flightdeck --language en
```

Without the flag, installer messages follow `LC_ALL`, then `LC_MESSAGES`, then
`LANG`: a German locale selects German; other locales select English. These
environment variables are only read, never changed. The menu description has
German and English translations. The installation records the chosen language
for update, rollback and uninstall messages:

```sh
~/.local/bin/flightdeck --update /absolute/path/to/new/source --language en
~/.local/bin/flightdeck --rollback --language de
~/.local/bin/flightdeck --uninstall --language en
```

An explicit `--language` also selects the interface language when the launcher
opens. A normal start, update or rollback without this flag leaves the browser's
saved language choice intact. Changing the installer language does not alter
your runtime configuration, country/Store market, account or saves.

Rollback keeps the current installation manager while selecting the previous
launcher source. If that older launcher predates language selection, it starts
without the unsupported flag; management commands remain translated.

## Installation options

```sh
# Install without opening a browser/server.
./install.sh --no-launch

# Omit the new application menu entry.
./install.sh --no-desktop
```

The default files are:

- Launcher command: `~/.local/bin/flightdeck`.
- Managed source releases: `${XDG_DATA_HOME:-~/.local/share}/flightdeck-launcher`.
- Optional menu entry: `${XDG_DATA_HOME:-~/.local/share}/applications/flightdeck.desktop`.
- Launcher settings: `${XDG_STATE_HOME:-~/.local/state}/flightdeck`.

If `~/.local/bin` is not on your PATH, use the full command path or the application
menu. The installer does not edit shell profiles, PATH, Omarchy or Hyprland
configuration. Desktop menus normally discover the new entry automatically;
reopen the menu if necessary.

`--data-dir`, `--bin-dir` and `--applications-dir` accept custom absolute paths.
For example, a contained test install can use three directories under a temporary
workspace. Symlinks in installation/source paths are rejected with an explanatory
message; use real directories. The installer will not overwrite a pre-existing
unrelated `flightdeck` command or `flightdeck.desktop` file.

For development only, `flightdeck --no-browser` runs the foreground local HTTP
service without opening a window. Stop that mode with Ctrl+C. Normal menu and
command starts use the detached desktop mode when the installed version supports
it; users do not need to manage the HTTP service themselves.

## Update and rollback

Choose the update that matches what you want to change:

| What to update | Where | What it changes |
| --- | --- | --- |
| Flightdeck launcher | Run the installer from a newer Flightdeck package | Interface, setup logic and bundled resources |
| Managed runtime components | Reopen the updated launcher while idle, or use `flightdeck --refresh-components` | Recognized Store/login components and runtime scripts |
| MSFS 2024 or 2020 | Select the edition, then **Updates** | Its Store base-game package; previous package retained |
| Fenix compatibility | **Mods → Fenix A320** | Optional, version-checked runner/profile setup; requires a separate explicit install |
| Fenix aircraft and liveries | Official Fenix installer/manager | Purchased Fenix software and matching liveries |

Flightdeck's **Updates** page does not update Flightdeck or Fenix. The Fenix
patch release is independent of the Flightdeck package and is downloaded only
when requested. A newer patch is adopted through a reviewed Flightdeck release;
the launcher does not automatically follow GitHub's latest tag.
[Fenix setup and restore](addons.md#fenix-a320).

Download and extract the newer **Flightdeck-Linux-x86_64.tar.gz**, then open its
**Install Flightdeck.desktop** or run `./install.sh` again. Source-build users
can update their prepared checkout instead.
Alternatively:

```sh
~/.local/bin/flightdeck --update /absolute/path/to/new/source
```

Launchers with the updated wrapper run the installer supplied by the
selected new package, so a newer package can introduce components that
the installed installer does not yet recognize. For older installed launchers,
use `./install.sh` from the new package. Use a package you trust.

On the next Flightdeck start, a newer **full package** automatically applies
its bundled Store and login components to an existing managed game runtime if
MSFS and setup are idle. If the game is still running, close it and reopen
Flightdeck. The runtime status shows a pending update until it succeeds. To
retry or apply the update from a terminal, run:

```sh
~/.local/bin/flightdeck --refresh-components
```

This checks every installed component against the runtime's import manifest,
checks the new bundle against its release hashes, and updates native files and
the six runtime scripts together under the runtime lock. Older manifests without
script hashes migrate only when all existing scripts match a pinned release.
Script-only updates are detected even if the native files are already current.
Game files, saves, account data and the Proton
runner are left in place. A recorded backup restores the old binaries, scripts
and import manifest if
you reopen Flightdeck or run the command again after an interruption. Until then, the launcher
blocks game start. If any old file was changed manually, Flightdeck
stops without overwriting it. An older development runtime without an import
manifest cannot be updated in place. Use **Prepare a new runtime** to import its
existing game files into a separate, managed runtime. A custom component set
is also preserved by an explicit `--refresh-components` command.
Launcher rollback does not automatically reverse
a completed runtime component update.

The source bytes, UI assets, six setup runtime scripts, both runtime lock files
and the vendored Fenix engine/manifests
form a reproducible SHA-256 release ID. All inputs are captured before installation.
Flightdeck installs into a new release directory and atomically switches its
small installation record after the files and entrypoints are ready. Reported
write failures restore earlier entrypoints and leave the previous active source
release intact. If the prepared package includes native components, their
verified bytes and notices are part of the same release hash. Interrupted processes/power loss are not a tested filesystem
recovery guarantee; retained staging files are never silently treated as an
installed release.

The previous release is retained:

```sh
~/.local/bin/flightdeck --rollback
```

Updates and rollbacks open the launcher by default. To manage it without launching,
run the installer directly with `--no-launch` (and `--rollback` if required).
Opening the updated launcher refreshes an idle background service automatically.
If the simulator, a download or another protected operation is active, the
running service keeps that operation. Open Flightdeck again after it finishes
to apply the update. Closing the window alone does not stop the service.
Older source releases remain available on disk and are cleaned up by uninstall.
Runtime folders, game files,
Microsoft credentials and saves are not updated by this launcher installer.

A changed installed source file or entrypoint blocks an update rather than
silently discarding local edits. Keep such edits in a source checkout, preserve
them before uninstalling, and install into a fresh managed directory if needed.

## Uninstall

```sh
~/.local/bin/flightdeck --uninstall
```

If the command has been moved or removed, run the source installer instead:

```sh
python3 scripts/install-launcher.py --uninstall
```

Use the original `--data-dir` when uninstalling a custom installation. Only
manifest-owned files whose hashes still match are removed. Changed files,
unrecognized files and symlinks are kept and their paths are reported. Settings,
prepared runtimes, account stores and local/cloud save data are left untouched.
The small manager directory and lock inode remain to avoid a concurrent-operation
race; they can be reused by a later install. No recursive deletion of foreign
folders is performed.

## Verification

Installer regression tests use temporary directories and synthetic path values,
with no real account, game or desktop changes:

```sh
python3 -m unittest discover -s tests -p 'test_installer.py' -v
```

The suite covers source/resource hashes, updates, rollback after write failures,
foreign files, symlinks, concurrent installers, preserved settings/saves and the
installed command's entrypoint. Language checks cover locale precedence,
translated help/errors, matching placeholders, explicit overrides, preserved
browser preferences and rollback to an older CLI. Source-install success does not establish
simulator compatibility or Marketplace purchase support.
