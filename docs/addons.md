# Community add-ons

[Deutsche Anleitung](addons.de.md)

Open **Add-ons** in Flightdeck to see the packages in your simulator's Community
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

1. Close MSFS. Download the add-on's **MSFS 2024** version from its developer.
2. In Flightdeck, select **Add-ons → Open Community folder**.
3. Extract the package into that folder. The package's `manifest.json` should be
   directly inside its own directory, for example
   `Community/example-aircraft/manifest.json`. Avoid an extra outer ZIP directory.
4. Select **Refresh** in Flightdeck and check the displayed name and version.
5. Start MSFS and check that the add-on appears in the simulator's library or
   aircraft selection. Presence in Flightdeck alone does not establish that it
   loads correctly in the simulator.

Follow the developer's own installer when one is supplied. Keep existing
settings and liveries before replacing a package. Flightdeck currently lists
packages and opens their folder; it does not install, update, enable or delete
individual add-ons for you.

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

### Fenix

The [Fenix installer](https://support.fenixsim.com/hc/en-us/articles/12459059815823-New-Fenix-Installer)
requires your Fenix login and installs a separate companion application. Fenix
instructs users to close MSFS and the Fenix application before installation.
The presence of its aircraft package alone does not establish working displays,
systems or communication with MSFS under Wine.

Get the current installer from your [Fenix customer dashboard](https://fenixsim.com/dashboard/downloads/).
Use the account that owns your aircraft. Fenix installs its own prerequisites;
the tested installer 1.0.286 requested WebView2, .NET 8 Windows Desktop x64 and
the Visual C++ x64 runtime.

The Wine test completed the official aircraft download and installation of
Fenix Airbus A320 **2.4.0.4720**. These settings were needed in its **separate
test prefix**:

- `DOTNET_SYSTEM_GLOBALIZATION_USENLS=1` selects Windows NLS for .NET. Without it,
  this runner failed to load an ICU symbol.
- `HKCU\Software\Microsoft\Avalon.Graphics\DisableHWAcceleration`, a DWORD set
  to `1`, enables WPF software rendering. Without it, the installer window was
  blank. Keep Wine's `mscoree` loader enabled.
- `DOTNET_ReadyToRun=0` disables precompiled .NET code for the Fenix process.
  Without it, the download planner failed with `SQLite Error 1`, either
  `no more rows available` or `SQL logic error`. With it, the official installer
  completed the download and installation. The precise runtime defect is still
  unknown; replacing or deleting the database was not required.

These are documented [.NET globalization](https://learn.microsoft.com/en-us/dotnet/core/runtime-config/globalization)
and [WPF rendering](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/graphics-multimedia/graphics-rendering-registry-settings)
settings; Microsoft also documents the [ReadyToRun switch](https://learn.microsoft.com/en-us/dotnet/core/runtime-config/compilation).
Set the environment variables on the Fenix launch command, not globally for MSFS.
Flightdeck does not currently apply these settings or start Fenix for you.

For an invisible cursor, an app-local test is
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-features=HideCursorWhileTyping`.
Preserve any existing browser arguments. This targets a documented
[WebView2 cursor regression](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5687);
its individual effect has not yet been confirmed in our Wine test.

If Fenix cannot find MSFS, first check which Wine prefix launched the installer.
A fresh prefix does not contain your simulator profile. Fenix reads `UserCfg.opt`
and also checks the surrounding profile files; copying only that file can still
fail detection. `InstalledPackagesPath` must resolve through that prefix's Wine
drive mappings to the actual Packages folder. Use separate copies for initial
tests instead of giving an untested installer write access to the live profile.
Do not create empty files just to satisfy the detection check.

If the installer used an isolated profile, its Community directory is also
separate. With MSFS closed, copy the completed `fnx-aircraft-320` package into the
Community folder shown by Flightdeck, without overwriting another installation.
Refresh the mod list to verify the title and version. The test verified all
2,076 copied files, and Flightdeck recognized the package. Future changes made
by an installer still pointed at the test profile do not update that copy.

Aircraft license activation, Fenix companion startup and its connection to MSFS
remain separate live tests. A completed installation does not establish that
cockpit displays or aircraft systems work under Wine. The Community package
alone does not include the separate companion installation.

Obtain add-ons and their installers from their developers. Flightdeck's releases
do not include aircraft packages, paid installers or account data.
