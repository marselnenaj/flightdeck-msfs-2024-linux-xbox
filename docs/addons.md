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
below, currently for MSFS 2024 only. The experimental Navigraph setup supports
MSFS 2024 and 2020.

Since 0.2.7, Flightdeck shows one highlighted next action for each add-on. Detected
components remain visible; additional actions are under **Manage & repair**.

## Uninstall a Community add-on

Close the simulator and other setup tasks. In **Mods**, choose **Review
uninstallation** beside the package. Check the exact folder and size, then choose
**Uninstall** and confirm. Removing a real folder permanently removes the package
and any settings stored inside it. Removing a link only removes its Community
entry; its original files stay in place. Cancel leaves the package unchanged.
Flightdeck refuses an outdated review if the folder or contents changed.

For a full Fenix or GSX uninstall, open the official installer/manager in its
add-on section. Navigraph Hub and Simlink remain in the Windows profile when
their Community packages are removed. Removing its Community package does not uninstall its Windows
companion applications.

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

Open **Mods → GSX Pro** and follow the highlighted next step. Completed steps
remain summarized; additional actions are under the collapsed **Manage & repair**
section. Close MSFS, Fenix and other Windows applications in the selected profile
before setup. MSFS 2024 must have been started once to create its package configuration.

GSX remains experimental. The official FSDT installer has been tested with native
.NET 4.8 in a separate MSFS 2024 Wine profile, including opening, closing and
releasing the runtime. GSX installation, activation, Couatl/SimConnect, the in-game
menu and ground services have not been verified with a licensed copy.

Starting with 0.1.20, FSDT setup uses the active Proton version and its installed
files carry forward when switching runners. Recover interrupted GSX preparation
before switching Proton versions.

1. **Prepare FSDT** downloads the SHA-256-pinned official installer, copies the
   Windows profile and installs .NET 4.8 if needed. Flightdeck registers FSDT's
   licensing component and retains the original profile as a backup. The game
   runner is unchanged. Preparation can take several minutes.
2. **Open FSDT installer**, select GSX Pro and follow FSDT's installation and
   activation. Check that the simulator and Community path match the selected
   Flightdeck installation. Let downloads finish, then close the installer or
   use Flightdeck's close action. FSDT manages its package links; do not copy
   its entire installation into Community manually.
3. **Set up automatic startup** becomes available after Flightdeck detects the
   GSX Community package and FSDT's Couatl entry in `exe.xml`. It enables that
   existing entry, preserving FSDT's arguments and other add-ons. If the entry is
   missing, update through FSDT and reload the status. Start MSFS and test the GSX
   menu and ground services. **GSX locally configured · Functionality unconfirmed**
   confirms local setup only, not license validity or Linux compatibility.

GSX is paid software. According to the [official GSX manual](https://www.fsdreamteam.com/gsx_manual_msfs.pdf),
installation requires a GSX license or an eligible activated FSDT airport. The
latter enables use at FSDT airports and a limited trial at KSFO, LIMC and EDDM.
Preparing FSDT in Flightdeck does not unlock GSX. Purchases and activation stay
with [FSDreamTeam](https://www.fsdreamteam.com/products_gsxpro.html).

**Open FSDT installer** appears as the current next step or under **Manage & repair**,
depending on setup progress. The latter also contains FSDT repair and autostart
controls. **Disable GSX autostart** changes only the
Couatl entry; it does not uninstall packages. With GSX startup configured,
Flightdeck also closes the named Couatl
companions in that profile at game exit, crash or stop. Other profiles and FSDT's
installer remain outside that session cleanup.

If preparation is interrupted, **Recover GSX preparation** appears directly as
the next action. It restores the previous profile; game launch remains blocked
until recovery completes. Completed preparation keeps a
`local/msfs-prefix.before-gsx-*` backup. Recovery is for an interrupted
preparation, not for undoing a later GSX installation. Diagnostic logs are in
`private/gsx-setup-*.log` and `private/gsx-manager.log`. If FSDT replaces its
installer download, a checksum mismatch stops preparation until Flightdeck's
pin has been reviewed and updated. No FSDT or Microsoft binaries are bundled.

### Navigraph Hub and Simlink (experimental)

Open **Mods → Navigraph** and follow the highlighted next step. Each MSFS edition
has its own Windows profile, so set up Navigraph separately for 2024 and 2020.
Close MSFS and other Windows applications in the selected profile first.
Navigraph does not officially support Linux; sign-in, subscription and navdata
stay with Navigraph.

1. **Set up Navigraph sign-in** registers a hidden `flightdeck-navigraph.desktop`
   entry as the handler for `navigraph-desktop://` (Hub) and
   `navigraph-traffic-desktop://` (Simlink) through `xdg-mime`. After you sign
   in through your Linux browser, the browser hands the result back to
   Flightdeck, which passes it to the waiting application in its profile. Without
   this handler, the sign-in completes in the browser but never reaches Hub or
   Simlink. `xdg-utils` is required.
2. **Install Navigraph Hub** downloads the official Hub 1.2.24 installer from
   Navigraph's update server, checks its pinned SHA-256 and installs it for the
   Windows user of this profile. Flightdeck moves Hub's bundled Process Governor
   (`procgov.exe`) aside, because its call fails under Wine with
   `Win32Exception (87)` and stops every package extraction. Hub then runs 7-Zip
   directly. Hub's updates restore the file; Flightdeck moves it aside again before
   every start. Flightdeck also turns off Hub's **run in background** and Windows
   login autostart settings, so closing Hub's window quits it.
3. **Open Navigraph Hub**, sign in and install the navdata. Hub finds the
   simulator through the same `UserCfg.opt` that Flightdeck uses; start MSFS once
   beforehand. Close Hub's window when the installation has finished, or use
   **Close Navigraph**. Flightdeck lists the `navigraph-*` packages it finds in the
   Community folder.
4. Optional, for the aircraft position in Navigraph Charts: download the Simlink
   installer from [Navigraph](https://navigraph.com/downloads), select it and
   choose **Start Simlink installer**. Keep the suggested folder
   (`C:\Program Files\Navigraph\Simlink`).
5. **Set up Simlink auto-start** adds one Simlink entry to the edition's
   `exe.xml`, next to its `UserCfg.opt`, and keeps other add-ons and a backup in
   `private/navigraph-exe-*.xml`. MSFS then starts Simlink in the same Wine session,
   so SimConnect is reached without network configuration. Flightdeck closes
   Simlink when the simulator session ends. Sign in to Simlink once through
   **Manage & repair → Open Simlink**.

**Navigraph configured locally · Functionality unconfirmed** confirms local files
only. Hub navdata installation, Simlink's SimConnect connection and Charts
position tracking have not yet been verified in a complete flight with
Flightdeck. Hub detects Flightdeck's installation like a Steam installation; it
does not see Marketplace aircraft in the Official folder, so aircraft-specific
navdata for those may be missing. Logs are in
`private/navigraph-*.log` of the runtime. No Navigraph binaries are bundled.

### Fenix A320

Open **Mods → Fenix A320** and follow the highlighted next step. Flightdeck
summarizes completed steps instead of presenting every setup action at once.
Additional actions are under **Manage & repair**; **Local patch bundle and
restore** is further below. The same patch is also available through the
[standalone patch installer](https://github.com/marselnenaj/fenix-a320-linux-patch/releases).

It supports MSFS 2024 with the pinned Xodus Wine runner. It creates an independent
runner/profile, keeps a backup, verifies release hashes and configures GPU displays
on supported overlays (CPU on CachyOS Proton 10),
Legacy readouts and Fenix autostart. Run MSFS 2024 once to create its user settings,
then close MSFS and all Fenix applications before setup.

1. Choose **Install patch**. Flightdeck downloads the Linux ZIP from the public
   Fenix patch GitHub release and verifies its SHA-256. Native Microsoft .NET
   Framework 4.8 is installed when needed. Flightdeck checks both 32-bit and
   64-bit CLR startup, completes pending Wine restart work and automatically
   repairs an incomplete installation. If MSI repair leaves missing files,
   Flightdeck makes one final reinstall attempt inside the copied profile.
   The pinned Microsoft .NET 10 Desktop Runtime is also installed for the
   Fenix v3 installer. A missing runtime is repaired before opening a Fenix app.
2. If **Open Fenix app** is offered, continue installation in the detected manager.
   Otherwise download the official installer from your
   [Fenix account](https://fenixsim.com/dashboard/), select its EXE and choose
   **Start installer**. Sign in and install the A320 for the selected simulator,
   including its prerequisites. Wait for aircraft installation to finish, then
   close the manager or installer.
3. Choose **Open Fenix**, sign in/activate normally, then close the application
   or use **Stop Fenix** in Flightdeck.
4. Choose **Finish setup** to configure displays, Legacy readouts and autostart.
   Once local setup is complete, return to the overview and start MSFS normally.
   If Fenix has not created its readout settings yet, the panel says so: load the
   aircraft once in the simulator, quit MSFS and choose **Reapply settings**.

**Fenix locally configured** confirms Flightdeck's settings only. It does not confirm
your Fenix license, activation or functioning cockpit. Fenix checks sign-in and
licensing; verify the aircraft in MSFS. A detected manager or successful install
hook alone does not establish that the aircraft is installed.

Finishing the official installer alone does not complete setup. If a Windows
application is still open, quit the Fenix installer and Fenix completely; the
panel refreshes automatically. On a German keyboard,
if `AltGr+Q` does not enter `@` in its login window, try `Ctrl+Alt+Q` or paste `@`
with `Ctrl+V`.

**Stop Fenix** appears when Fenix is running in the selected profile. It closes
Fenix, its helpers and the official manager, and waits before unlocking the
next setup step. It is disabled while MSFS or an installer is running. Complete
any installation or livery download in the official manager before stopping it.

Use **Manage & repair** for patch updates, reapplying settings and reinstalling
the official app. Manager and repair actions appear there when they are not
already the current next step.
Existing preview.1/preview.2 patches update automatically before opening Fenix; **Update
patch** also starts that update directly. Aircraft, settings and the original
restore point are retained.

If the official installer reports **“application install hook failed”**, close
all installer windows first. Starting its EXE through Flightdeck applies the
existing ICU/.NET workaround, also present in 0.2.5. After a failed installer,
**Repair Fenix app** appears as the next action when repair is available. It
reruns the official hook with valid package metadata, without resetting the
profile. Reopen the manager after a successful repair.

In **0.2.8**, the latest recorded failed attempt stays visible after a launcher
restart. After a failed repair, **Open diagnostics** becomes the next step.
Prepare a fresh report before trying again; an eligible repair remains under
**Manage & repair**. If the manager is missing or cannot be identified, the
official installer can also be selected there. The green **Fenix Linux patch**
check confirms only the Linux patch, not a successful app hook. A new recorded
attempt replaces the previous result.

Version 0.2.8 filters inherited Linux .NET runtime/loading overrides and startup
hooks from Windows installer and repair commands. In a controlled test with
official FenixApp 1.0.286, an invalid inherited `DOTNET_STARTUP_HOOKS` caused
exit 82; the same input passed with exit 0 after filtering. This fixes that
reproduced cause, without establishing that users’ reported exit-82 failures
are resolved. Exit 82 alone does not identify a cause. Diagnostic reports retain
recognized exception types and an explicitly observed CLR code, never raw
exception messages; `<runtime>/private/fenix-app.log` remains local.
[Verification and limits](native-ui.md#fenix-install-hook-warning).

The launcher also checks persisted `DOTNET_STARTUP_HOOKS` in the Wine profile's
user and machine environment. A missing absolute `C:\…` hook can reintroduce
exit 82/`FileNotFoundException` after the Unix environment was filtered. Only
demonstrably missing paths are removed; existing hooks and relative, expanded
or external-drive entries are retained. Original values are saved locally in
`<prefix>/.flightdeck-fenix-environment.json` before changing the registry.
This fixes that reproduced failure, not every missing-assembly exception.

After setup, Fenix starts automatically with MSFS; you do not need to start it
separately. Flightdeck closes the session's Fenix companions after normal exit,
a game crash or **Stop**, with a bounded fallback for stuck processes. Since
0.2.9 this includes the official Fenix app when it is still open, so finish any
installation or livery download there before quitting MSFS. Other Wine profiles
are excluded from that cleanup.

Flightdeck 0.2.23 includes patch **0.1.0** as its verified fallback
and independently checks GitHub for compatible Fenix patch releases at startup
and every 30 minutes. Failed checks retry after five minutes and do not prevent
using the installed patch. **Updates** and **Mods → Fenix A320 → Manage & repair**
show the installed version, available update and **Check Fenix updates**.

A discovered compatible update installs automatically before starting MSFS,
opening Fenix, its installer or manager, or finishing setup. You can also choose
**Update patch** yourself. MSFS and Fenix must be closed; the runtime lock and
existing profile/runner backups protect the installation. Aircraft, renderer
preferences and restore points are retained. A launcher restart or offline check
does not downgrade a newer independently installed patch. Preview installations
prefer a stable release when one becomes available; stable installations follow
stable releases. Releases requiring a new installer contract or different runner
integration require a Flightdeck update first.

Since 0.2.23, a known older local setup can be adopted automatically before
launch or through **Adopt setup & update**. Flightdeck verifies its files,
copies the profile and runner, and retains the previous installation for
restoration. Existing aircraft, renderer and FCU preferences are retained.
Unknown customized files stop adoption. Restoring the previous local profile pauses automatic adoption; use the adoption action explicitly to enable it again. The overview and update icon show
pending adoption; an **available** version is not an installed-version claim.

Patch 0.1.0 replaces the remaining owned Linux Python helpers with Rust.
It requires Flightdeck **0.2.22 or later**; update the launcher first. Wine,
DXVK and the Windows ABI helpers remain native C/C++. Full-flight stability and
the separate FenixSystem memory growth remain unresolved.

Installing Flightdeck itself does not install the Fenix aircraft. The official
Fenix app remains responsible for aircraft updates and licensing.
No GitHub login is needed for the public patch download. Download and activation
of the purchased aircraft use the official Fenix software and your Fenix account.

For a local copy, extract the **Linux installer ZIP** and select the directory
containing `bundle.json` under **Local patch bundle and restore**, below
**Manage & repair**. A GitHub source checkout or the source-only archive lacks
the Wine payload. Leaving this field
blank uses the verified download/cache. The standalone `install.sh` provides the
same setup without Flightdeck's panel, as a guided setup in the web browser.

#### Liveries

The installed official manager opens through **Open Fenix app** in the current
setup step or **Installer & Liveries** under **Manage & repair**. It handles
aircraft installation, updates and liveries. **Open Fenix** opens the main Fenix
application.
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

**Fenix v3 is experimental.** Preview.11 fixes the Gateway loader-name
allowlist on all four pinned runners. On the reported **Fenix 3.0.0.7454 /
Steam MSFS 2020 / CachyOS 11 / NVIDIA-PRIME** setup, the reporter confirms
Gateway logging, port 8083 and a working EFB after this fix. GPU displays work
with **USE DISPLAY SAFE MODE off** in the Fenix application; CPU mode remains
an available fallback. These are community runtime results, backed by 64
independent native process-name comparisons across the four patched runners.

The earlier maintainer Xodus/MSFS 2024 test reached working cockpit displays
and passed a fresh start plus five backend restarts. One earlier Wine
`create_view` assertion remains unexplained, and full-flight v3 stability is
unvalidated. The separate Fenix 2 System-memory growth remains open.
[Gateway fix and scope](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0/docs/gateway-loader-case.md).

Earlier tested cockpit: Fenix 2.4.0.4720, MSFS 2024 1.8.16.0 and Hyprland. PFD/ND/ECAM,
MCDU, clock, FCU and radio rendering were verified. Full-flight testing remains
outstanding. The Xodus GPU path was checked with PFD, ND, ECAM and a moving
airspeed tape on AMD. NVIDIA support beyond the reported setup and GPU weather radar remain unvalidated; weather
radar is unavailable in the CPU renderer. Earlier automatic CPU settings migrate
to GPU on capable overlays, while a saved CPU preference survives updates.
See the patch's [GPU scope and fallback instructions](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0/docs/gpu-displays.md).
The binary patch
requires x86_64 Linux and glibc 2.38+; Flightdeck's full native package still requires
glibc 2.39+. Starting with 0.1.20, matched Fenix overlays are available for Proton
Experimental 11.0 (20260924), CachyOS Proton 11.0 (20261005) and CachyOS Proton
10.0 sunset. Their native probes
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

**Restore original profile** under **Local patch bundle and restore** restores
the pre-patch runner, scripts and Windows
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
