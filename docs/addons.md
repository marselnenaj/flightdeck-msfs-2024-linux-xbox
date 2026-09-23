# Community add-ons

[Deutsche Anleitung](addons.de.md)

Open **Mods** in Flightdeck to see the packages in your selected simulator's Community
folder. **Open Community folder** opens that location in the Linux file manager;
**Refresh** reads the inventory again after an installation.

Flightdeck reads the actual package location from the selected runtime or the
simulator's `UserCfg.opt`, including Windows drive mappings in Wine. It does not
create a guessed Community directory. If no location is configured, start MSFS
once and check the package location in the simulator. Multiple conflicting
locations are reported for you to resolve.

Names, versions and creators come from each package's `manifest.json`. Linked
package directories are supported. Missing, malformed and unreadable manifests
are marked. Scans and file reads are bounded; the interface indicates a limited
inventory. This list does not confirm that a package is enabled in MSFS, licensed
through the Marketplace or compatible with Linux. Flightdeck does not execute
add-on programs or change package contents when reading this list.

## Install a normal Community package

1. Select **MSFS 2024** or **MSFS 2020** in Flightdeck, then close the simulator.
   Download the add-on for that edition from its developer.
2. In Flightdeck, select **Mods → Open Community folder**.
3. Extract the package into that folder. The package's `manifest.json` should be
   directly inside its own directory, for example
   `Community/example-aircraft/manifest.json`. Avoid an extra outer ZIP directory.
4. Select **Refresh** in Flightdeck and check the displayed name and version.
5. Start MSFS and check that the add-on appears in the simulator's library or
   aircraft selection. Presence in Flightdeck alone does not establish that it
   loads correctly in the simulator.

Follow the developer's own installer when one is supplied. Keep existing
settings and liveries before replacing a package. The general Community inventory lists packages and opens their folder.
Fenix has a separate setup workflow below, currently for MSFS 2024 only.

## Aircraft with companion applications

### FlyByWire A32NX and SimBridge

1. Obtain the **Linux installer** from [FlyByWire](https://flybywiresim.com/downloads/).
   The AppImage is the portable option; no Windows Xbox app is required.
2. Configure its Community location to match the exact path shown by Flightdeck.
   Linux cannot discover your Wine installation through the normal Windows
   application locations automatically in every setup.
3. Select **A32NX**, **Microsoft Flight Simulator 2024**, then **Stable** and
   install. MSFS 2020 and 2024 packages are separate downloads.
4. The expected aircraft folder is `flybywire-aircraft-a320-neo`. Refresh the
   Flightdeck inventory when the FlyByWire installer reports completion.
5. If you want features such as terrain data and the remote MCDU, install
   [SimBridge](https://docs.flybywiresim.com/tools/simbridge/install-configure/installation/)
   through the FlyByWire installer as well.

The checked SimBridge 0.7.0 package supplies `fbw-simbridge.exe`, a Windows
application, even when downloaded by the Linux installer. Its Wine startup and
connection to the simulator need their own verification. Do not assume that
installing the aircraft also starts or validates the companion application.

The current isolated SimBridge test reached HTTP health and terrain-map
initialization. It required the runner's `libvkd3d-1.dll`,
`libvkd3d-shader-1.dll` and `libvkd3d-utils-1.dll` in that test prefix. These are
already runner components, not modified FlyByWire files. A connection to a
running simulator has not yet been established by this test.

### Fenix A320

Open **Mods → Fenix A320** to use the optional compatibility installer in
Flightdeck **0.1.1**. Update older launchers using the [full package](install.md).
The same patch is also available through the
[standalone patch installer](https://github.com/marselnenaj/fenix-a320-linux-patch/releases/tag/v0.1.0-preview.1).

It supports MSFS 2024 with the pinned Xodus Wine runner. It creates an independent
runner/profile, keeps a backup, verifies release hashes and configures CPU displays,
Legacy readouts and Fenix autostart. Run MSFS 2024 once to create its user settings,
then close MSFS and all Fenix applications before setup.

1. Choose **Install patch**. Flightdeck downloads the Linux ZIP from the public
   Fenix patch GitHub release and verifies its SHA-256. Native Microsoft .NET
   Framework 4.8 is installed when needed.
2. Download the official installer from your [Fenix account](https://fenixsim.com/dashboard/),
   select its EXE and choose **Run installer**. Complete its normal prerequisite and
   aircraft installation in the selected simulator profile, then close the installer.
3. Choose **Open Fenix**, sign in/activate normally, then close the application.
4. Choose **Finish setup**. When the green **Fenix is ready to fly** message
   appears, return to the overview and start MSFS normally.

Flightdeck marks completed steps and highlights the next action. Finishing the
official installer alone does not complete setup. If a Windows application is
still open, quit the Fenix installer and Fenix completely; the panel refreshes
automatically. Fenix itself checks your sign-in and license. On a German keyboard,
if `AltGr+Q` does not enter `@` in its login window, try `Ctrl+Alt+Q` or paste `@`
with `Ctrl+V`.

After setup, Fenix starts automatically with MSFS; you do not need to start it
separately. Flightdeck closes the session's Fenix companions after normal exit,
a game crash or **Stop**, with a bounded fallback for stuck processes. The
official Fenix installer and other Wine profiles are excluded from that cleanup.

The source checkout currently pins patch **0.1.0-preview.1**. Flightdeck downloads
the reviewed version recorded in `compat/fenix/release.json`, not an arbitrary
latest release. Installing Flightdeck itself does not install Fenix automatically.
No GitHub login is needed for the public patch download. Download and activation
of the purchased aircraft use the official Fenix software and your Fenix account.

For a local copy, extract the **Linux installer ZIP** and select the directory
containing `bundle.json` under **Local patch bundle and restore**. A GitHub source
checkout or the source-only archive lacks the Wine payload. Leaving this field
blank uses the verified download/cache. The standalone `install.sh` provides the
same setup without Flightdeck's panel and needs Python Tk for its graphical UI;
the Flightdeck panel does not require Tk.

#### Liveries

**Open Fenix Installer / Liveries** opens the already installed official manager.
Close the simulator and other Fenix applications first. The button becomes
available when Flightdeck detects that manager in the selected Wine profile.
Choose liveries for the exact aircraft, engine and wing variant. A321 liveries do
not appear for A320; CFM/IAE and Sharklets variants can also have separate selection
requirements. For example, an **A320 CFM SL** livery belongs to that Sharklets
variant, not the A320 IAE. A livery does not add a separately sold aircraft.

For a third-party download, check support for your Fenix version and simulator,
extract the actual package into **Mods → Open Community folder**, then refresh the
inventory. Its `manifest.json` must be directly inside the package folder.
Restart MSFS and select the matching aircraft variant before choosing its livery.
If it is listed in Flightdeck but absent in the simulator, recheck the aircraft,
engine/wing variant, simulator version and extra archive directory level.

#### Compatibility and recovery

Tested cockpit: Fenix 2.4.0.4720, MSFS 2024 1.8.16.0 and Hyprland. PFD/ND/ECAM,
MCDU, clock, FCU and radio rendering were verified. Full-flight testing remains
outstanding. Weather radar is unavailable in the CPU renderer. The binary preview
requires x86_64 Linux and glibc 2.38+; Flightdeck's full native package still requires
glibc 2.39+. Other Wine/Proton builds, Steam prefixes and MSFS 2020 are outside this
first patch's scope. The optional window guard hides matching
service/display windows; the main Fenix application remains accessible.

**Existing local Fenix patch** means a previous development setup is detected.
That setup stays active and the new install button is disabled. There is no
automatic migration; test a fresh install with a separate compatible runtime if
needed. Disabled steps can also mean MSFS, Fenix or another setup job is still
running, or the previous step has not finished. Close those applications and use
**Reload status**. The official installer step also needs a selected EXE.

**Restore original profile** restores the pre-patch runner, scripts and Windows
profile. Settings and packages added inside the profile after patch installation
stay in the retained newer profile; they are not merged into the restored one.
External Community packages and
Flightdeck's separate Xbox save storage are not removed. Interrupted patch setup
blocks game launch and offers restore. The separate patch project contains full
source/build instructions and a standalone installer:
[fenix-a320-linux-patch](https://github.com/marselnenaj/fenix-a320-linux-patch).

Fenix/Microsoft software and account data are not part of Flightdeck or the patch
release. The official Fenix login and license activation remain required.
