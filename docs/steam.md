# Steam installations (beta)

Flightdeck can select installed Steam editions of MSFS 2020 and MSFS 2024,
including installations in additional Steam libraries. Xbox and Steam copies of
the same edition have separate identities and can coexist. The existing Xbox
configuration remains available when switching back or rolling back Flightdeck.

Steam support is beta. Local checks cover discovery, native/Flatpak process
namespaces, profile selection, the shared Fenix installer, interrupted commits
and scoped companion cleanup. They do not establish successful simulator flights
on every Steam client, GPU or distribution. An offline real-Wine check also
prepares a fresh Steam profile, upgrades patch 0.1.0 to 0.1.1, runs the official
FenixApp 1.0.359 install hook and restores the original profile while preserving
the simulator Packages files. Actual Steam MSFS flights still need
tester validation; this development machine has no Steam MSFS installation.

## Select and start

1. Install MSFS through Steam. Flightdeck detects native Steam, Flatpak Steam and
   Snap Steam directories and their registered libraries. Use the installation
   selector to choose the exact game and storefront.
2. Start the game once through Steam and close it to initialize its Proton
   profile. Discovery works before this first start; Fenix preparation waits for
   the profile to exist.
3. In Flightdeck, choose **Start MSFS**. Flightdeck requests the selected game's
   AppID through its Steam client and observes the actual simulator process.
   A Steam command returning does not count as the simulator exiting.
4. **Stop MSFS** requests termination only of that simulator in its own profile.
   Steam and other games remain running. If termination cannot be confirmed,
   close the simulator directly in Steam.

Already running simulators are recognized after Flightdeck restarts. Missing or
moved libraries remain visible; scan again and select the newly discovered
location before changing it. A stale page cannot write to a replaced profile.
If automatic discovery misses a custom client directory, use **Setup → Xbox and
Steam installations → Choose Steam directory manually**. Choose the Steam client
directory; the client reads its library list from there. Flatpak and Snap roots
must belong to the selected sandboxed client. If the same AppID has manifests in
multiple libraries, resolve the outdated installation in Steam before selecting
it in Flightdeck; an AppID launch cannot identify a particular library.

Steam owns game downloads, licenses, game updates, Proton launch options and its
own synchronization. Flightdeck's Xbox setup, Microsoft Store checks and Xbox
cloud import/upload are unavailable for a selected Steam installation. Existing
Steam launch options and platform synchronization are not overwritten.

## Fenix

Open **Mods → Fenix Linux patch**. An existing patch made by the standalone
[Fenix Linux installer](https://github.com/marselnenaj/fenix-a320-linux-patch)
is recognized directly. Flightdeck uses that project's pinned Rust Steam engine
for installation, configuration, tool selection, updates and restoration.

1. Close MSFS, Fenix and Steam completely before installing/updating the patch or
   changing Steam's Proton selection. Steam can otherwise overwrite its config.
2. Set up the patch and choose **Select Fenix Proton for Steam**. Only that
   simulator's compatibility-tool mapping is changed. Steam may need restarting
   before a newly created compatibility tool appears in its UI.
3. Select the official Fenix installer EXE from your Fenix account. Wait for the
   installation and any child installer to finish, then close the official app.
4. Open Fenix and complete its own sign-in/activation. Close it, then choose
   **Configure displays and autostart**.

Flightdeck retains one managed compatibility tool per supported Proton variant;
MSFS 2020 and 2024 can share it. Repeating setup reuses it. An update checks other
profiles using that tool and refuses to change it while they are running.
Restoring one game does not delete a tool still used by the other game.

Updates retain the current profile. Initial preparation preserves the original
profile for restoration and moves large simulator Packages data instead of
duplicating it. Restoration retains the newer profile as a backup. Interrupted
preparation offers restoration; never remove an unknown profile or backup to
bypass a failed check.

If the official installer reports partial success, Flightdeck records the
current hook result. **Repair Fenix app** reruns the official post-install hook;
it does not install the aircraft. After a failed repair, inspect Diagnostics
before retrying. A `FileNotFoundException` without its message does not identify
the missing file. The exported report retains bounded exception identifiers and
exit codes; the private local log retains the actual third-party error.

Fenix window and MCDU refresh helpers follow an observed game using a verified
Fenix Proton. Closing Flightdeck leaves the Steam flight running. A confirmed
game exit cleans up Fenix companions in that profile only.

## GSX, Navigraph and Orbx

Since 0.3.3, **Mods** offers the same experimental GSX, Navigraph Hub/Simlink and
Orbx Central workflows for Steam MSFS 2020 and 2024. They use this installation's
real prefix and selected Proton, including custom tools and secondary libraries.
Choose the compatibility tool in Steam and launch MSFS once before setup. Close
MSFS and other Windows applications in that profile before changing add-ons.
No separate permanent Wine profile or new Proton download is created for these
apps. [Setup steps and tested scope](addons.md)

Flightdeck stores companion metadata/logs under
`~/.local/share/flightdeck/steam/<installation-id>/addon-apps/`. GSX preparation
stages a copy beside the real prefix and retains `pfx.before-gsx-*` for recovery.
A pending GSX transaction blocks game launch until restored. Stale actions after
changing Proton or replacing the prefix require a fresh status page. Simlink and
Couatl startup use the edition's real `exe.xml`; cleanup after a confirmed game
exit affects only enabled companions in that profile. Full Steam flights and
licensed add-on functionality still need tester validation.

## Community and saves

The Community inventory uses the selected Steam profile's `UserCfg.opt` and Wine
drive mappings. It opens the actual configured folder and reports conflicting
configurations rather than choosing one. Steam Community removal is not enabled in this beta. Use the add-on's
official installer for that task.

Steam saves keep their existing platform behavior. Flightdeck's Xbox cloud sync
and Xbox save backups do not run against Steam profiles. Fenix restore points
are separate from game-save synchronization.

Report reproducible problems in [GitHub Issues](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues)
or the [Discord community](https://discord.gg/fAhHR2DkqN), including the MSFS
edition, Steam client, distribution, GPU and failed action.
