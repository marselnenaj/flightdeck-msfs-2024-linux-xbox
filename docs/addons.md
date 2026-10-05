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
Fenix and the experimental GSX Pro integration have separate setup workflows
below, currently for MSFS 2024 only.

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

### GSX Pro (experimental)

In **0.1.19**, **Mods → GSX Pro** opens preparation and setup in three steps.
Other add-on cards remain collapsed. The official FSDT installer has been tested
with native .NET 4.8 in a separate MSFS 2024 Wine profile, including opening,
closing and releasing the runtime. GSX installation, activation, Couatl/SimConnect,
the in-game menu and ground services still need testing with a licensed copy.

Starting with 0.1.20, FSDT setup uses the active Proton version and its installed
files carry forward when switching runners. Recover interrupted GSX preparation
before switching Proton versions.

Start MSFS 2024 once to create its package configuration, then close the simulator,
Fenix and other Windows applications in that profile.

1. **Prepare FSDT** downloads the SHA-256-pinned official installer, copies the
   Windows profile and installs .NET 4.8 if needed. Flightdeck registers FSDT's
   licensing component and retains the original profile as a backup. The game
   runner is unchanged. Preparation can take several minutes.
2. **Open FSDT installer**, select GSX Pro and follow FSDT's installation and
   activation. Check that the simulator and Community path match the selected
   Flightdeck installation. Let downloads finish, then close the installer or
   select **Close FSDT** in Flightdeck. FSDT manages its package links; do not copy
   its entire installation into Community manually.
3. **Set up automatic startup** becomes available after Flightdeck detects the
   GSX Community package and FSDT's Couatl entry in `exe.xml`. It enables that
   existing entry, preserving FSDT's arguments and other add-ons. If the entry is
   missing, update through FSDT and reload the status. Start MSFS and test the GSX
   menu and ground services. **GSX configured · Flight test pending** confirms
   local setup only, not license validity or Linux compatibility.

GSX is paid software. According to the [official GSX manual](https://www.fsdreamteam.com/gsx_manual_msfs.pdf),
installation requires a GSX license or an eligible activated FSDT airport. The
latter enables use at FSDT airports and a limited trial at KSFO, LIMC and EDDM.
Preparing FSDT in Flightdeck does not unlock GSX. Purchases and activation stay
with [FSDreamTeam](https://www.fsdreamteam.com/products_gsxpro.html).

**Disable GSX autostart** changes only the Couatl entry; it does not uninstall
packages. With GSX startup configured, Flightdeck also closes the named Couatl
companions in that profile at game exit, crash or stop. Other profiles and FSDT's
installer remain outside that session cleanup.

If preparation is interrupted, game launch remains blocked until **Recover GSX
preparation** restores the previous profile. Completed preparation keeps a
`local/msfs-prefix.before-gsx-*` backup. Recovery is for an interrupted
preparation, not for undoing a later GSX installation. Diagnostic logs are in
`private/gsx-setup-*.log` and `private/gsx-manager.log`. If FSDT replaces its
installer download, a checksum mismatch stops preparation until Flightdeck's
pin has been reviewed and updated. No FSDT or Microsoft binaries are bundled.

### Fenix A320

The 0.2.3 source branch also addresses the **"application install hook failed"**
warning for the known FenixApp ICU failure. It applies .NET settings inside the
Wine profile automatically and waits for installer children before checking the
result. Run the official EXE through Flightdeck's Fenix installer action, then
close its windows. [Verification and limits](native-ui.md#fenix-install-hook-warning).

Open **Mods → Fenix A320** to use the optional compatibility installer in
Flightdeck **0.1.4**. Update older launchers using the [full package](install.md).
The same patch is also available through the
[standalone patch installer](https://github.com/marselnenaj/fenix-a320-linux-patch/releases).

It supports MSFS 2024 with the pinned Xodus Wine runner. It creates an independent
runner/profile, keeps a backup, verifies release hashes and configures CPU displays,
Legacy readouts and Fenix autostart. Run MSFS 2024 once to create its user settings,
then close MSFS and all Fenix applications before setup.

Existing preview.1/preview.2 patch installations update automatically before opening Fenix
with this launcher. **Update patch** also starts the update directly. Aircraft,
settings and the original restore point are retained; Fenix need not be reinstalled.

1. Choose **Install patch**. Flightdeck downloads the Linux ZIP from the public
   Fenix patch GitHub release and verifies its SHA-256. Native Microsoft .NET
   Framework 4.8 is installed when needed. Flightdeck checks both 32-bit and
   64-bit CLR startup, completes pending Wine restart work and automatically
   repairs an incomplete installation. If MSI repair leaves missing files,
   Flightdeck makes one final reinstall attempt inside the copied profile.
2. Download the official installer from your [Fenix account](https://fenixsim.com/dashboard/),
   select its EXE and choose **Run installer**. Complete its normal prerequisite and
   aircraft installation in the selected simulator profile, then close the installer.
3. Choose **Open Fenix**, sign in/activate normally, then close the application
   or use **Stop Fenix** in Flightdeck.
4. Choose **Finish setup**. When the green **Fenix is ready to fly** message
   appears, return to the overview and start MSFS normally.

Flightdeck marks completed steps and highlights the next action. Finishing the
official installer alone does not complete setup. If a Windows application is
still open, quit the Fenix installer and Fenix completely; the panel refreshes
automatically. Fenix itself checks your sign-in and license. On a German keyboard,
if `AltGr+Q` does not enter `@` in its login window, try `Ctrl+Alt+Q` or paste `@`
with `Ctrl+V`.

**Stop Fenix** appears when Fenix is running in the selected profile. It closes
Fenix, its helpers and the official manager, and waits before unlocking the
next setup step. It is disabled while MSFS or an installer is running. Complete
any installation or livery download in the official manager before stopping it.

After setup, Fenix starts automatically with MSFS; you do not need to start it
separately. Flightdeck closes the session's Fenix companions after normal exit,
a game crash or **Stop**, with a bounded fallback for stuck processes. The
official Fenix installer and other Wine profiles are excluded from that cleanup.

Flightdeck uses patch **0.1.0-preview.3**. Newer patch versions are adopted
through a Flightdeck update; the launcher does not independently check for the
latest Fenix patch on GitHub. Supported earlier patches update to Flightdeck's
included version when opening Fenix, its installer or manager, or finishing setup.
Installing Flightdeck itself does not install Fenix automatically.
No GitHub login is needed for the public patch download. Download and activation
of the purchased aircraft use the official Fenix software and your Fenix account.

For a local copy, extract the **Linux installer ZIP** and select the directory
containing `bundle.json` under **Local patch bundle and restore**. A GitHub source
checkout or the source-only archive lacks the Wine payload. Leaving this field
blank uses the verified download/cache. The standalone `install.sh` provides the
same setup without Flightdeck's panel and needs Python Tk for its graphical UI;
the Flightdeck panel does not require Tk.

#### Liveries

**Installer & Liveries** opens the already installed official manager for
aircraft installation, updates and liveries. **Open Fenix** opens the main Fenix application.
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
glibc 2.39+. Starting with 0.1.20, matched Fenix overlays are available for Proton
Experimental 11.0 (20260924) and CachyOS Proton 10.0 sunset. Their native probes
pass; a full flight on these runners remains unverified. Other builds need a
matching overlay. Steam prefixes and MSFS 2020 remain unsupported. The optional window guard runs with the simulator and hides
matching service/display windows. The X11 driver now keeps them off the desktop
while preserving internal visibility; the main Fenix application remains accessible.
Opening Fenix manually does not start that guard.

**Existing local Fenix patch** means a previous development setup is detected.
That setup stays active until a supported Proton version is selected in
Flightdeck. The switch migrates recognized launch scripts and applies the matching
Fenix overlay while preserving the current profile. Modified scripts are retained
and block automatic replacement. Disabled steps can also mean MSFS, Fenix or another setup job is still
running, or the previous step has not finished. Close those applications and use
**Reload status**. The official installer step also needs a selected EXE.

**Restore original profile** restores the pre-patch runner, scripts and Windows
profile. Settings and packages added inside the profile after patch installation
stay in the retained newer profile; they are not merged into the restored one.
External Community packages and
Flightdeck's separate Xbox save storage are not removed. Interrupted patch setup
blocks game launch. If setup stopped before changing the active profile, choose
**Repair setup** to retry directly; a host reboot or manual restore is unnecessary.
Flightdeck verifies that the original profile, runner and launch scripts are
unchanged, retains the failed copy and logs, and prepares a fresh copy. If the
transaction already started publishing files, or those originals changed, use
the existing restore action. A failed repair never reports the setup as ready.
The separate patch project contains full
source/build instructions and a standalone installer:
[fenix-a320-linux-patch](https://github.com/marselnenaj/fenix-a320-linux-patch).

Fenix/Microsoft software and account data are not part of Flightdeck or the patch
release. The official Fenix login and license activation remain required.

Patch preview.3 repairs missing routes and excessive stroke joins, installs the
pinned geometry dependency, hides matching helper windows and automatically
refreshes stale MCDU startup images after a Display restart. The refresh briefly
changes the pop-out display preference, then restores it. At maximum brightness
it uses a DIM/BRT round-trip; MCDU pages and flight-plan data stay unchanged.
The official live restart check passed. See the patch's
[display refresh notes](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/main/docs/mcdu-restart.md).
