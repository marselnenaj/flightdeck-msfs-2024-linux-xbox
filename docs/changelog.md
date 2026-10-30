# Changes and release status

## Unreleased

- Use HubHop's current export endpoint directly. Its former URL redirects, which
  prevented missing MobiFlight WASM events from downloading. Resume interrupted
  setup without reinstalling the app and offer repair as the primary action when
  the app exists but its WASM module or events are missing. Report WASM setup as a
  separate phase from publishing the portable app.
  Accept legitimate long HubHop event scripts while keeping the complete export
  bounded to 4 MiB and validating its UTF-8/comment/record format.
- Retain an interrupted MobiFlight journal and its stage files when Fenix or
  Proton has replaced the Windows profile. A new attempt uses the current
  profile without treating the old journal as permission to remove its files.

## 0.3.7 — 30 October 2026

- Finish Proton switch recovery independently of optional backup reclamation.
  Preserve backups that fail the strict removal inventory, clear their stale
  cleanup fingerprints, and retain all ownership/mount/link deletion guards.
  Exercise both a new switch and an older already-published interrupted journal,
  then return through the public default-Proton action without losing add-ons.
- Update ZIP to the patched 9.x line and explicitly validate local/central header
  consistency in every ZIP reader. Verify ordinary extraction and reject
  contradictory names, compression and flags before publishing engine files.
- Update Tokio, Fenix 0.1.4 and its TLS/certificate dependencies. Rebuild native
  Xodus authentication with patched P-256/constant-time dependencies and the
  supported AWS-LC XML-signing provider, removing the affected RSA/old Bigint graph.
  Preserve device identity, signing formats, Secret Service and verified downloads.
- Replace copied OAuth token fixtures with synthetic values and reject literal
  or percent-encoded OAuth credentials in source exports. Retain zeroizing
  temporary private-key buffers; make a real upstream device network test opt-in
  and remove its token output.
- Record the #16 reporter's confirmation that 0.3.6 reaches the main menu with
  default Proton. Full flights and the reporter's alternate-runner retest remain
  separate validation; no FenixSystem memory fix is claimed.

## 0.3.6 — 30 October 2026

- Probe the same architecture-specific GStreamer registry used by Wine, with
  Wine's in-process plugin loading and disabled GStreamer signal trap. A healthy
  Linux-only registry no longer hides a stale Wine plugin blacklist.
- When a required demux/post-processing element cannot be constructed, rebuild
  only the owned regular Wine registry and probe once more. Preserve links,
  unrelated files, explicit plugin settings and hardware-only decoder choices.
  Keep the existing game prefix, runner, DLLs and saves.
- Reproduce the reported ready-probe/missing-videoconvert failure with a real
  GStreamer blacklist and the actual 4K loading video. The corrected driver
  recovers in the same isolated profile and completes both decode passes,
  rewind, EOS and shutdown. The full simulator outcome in #16 remains unconfirmed.
- Export fixed cache-recovery states, registry scope and missing-element bitness
  in safe diagnostics. Record the failed maintenance-preview phase and numeric
  operating-system error without paths or raw exception text. The reported reset
  failure is not reproduced; a real isolated Wine reset completed with its backup.
- Test packaged updating and rollback against the pinned published 0.3.5 archive.

## 0.3.5 — 29 October 2026

- Integrate Fenix patch 0.1.2 and its pinned source revision. Pair the loader
  repair across the standalone installer and Flightdeck; retain 0.1.1 restore
  identities and verify launcher updating/rollback against published 0.3.4.
- Keep native CLR DLL priority while permitting Wine builtin fallback during
  .NET Framework preparation. Before opening or repairing Fenix, migrate only
  native-only global `mscoree` values, retaining a private backup. Preserve
  disabled/builtin preferences, per-app overrides and existing DLL files.
- Reproduce issue #10's IL-only `System.Runtime.dll` load failure with the
  default Xodus runner and Microsoft-verified .NET 8.0.33 in an isolated, account-free prefix.
  Verify the actual Flightdeck preparation repairs the persisted override, loads
  the same assembly and is idempotent. This verifies the loader failure class;
  the complete Fenix installer remains untested.
- Prepare official MobiFlight 11.3.1, its WASM module and missing HubHop events in
  the selected Xbox or Steam profile. Preserve custom events and unknown DLLs;
  bundle a reproducible, hash-bound HidSharp fix with its license and sources.
  Merge WinWing device IDs only for the verified Winebus build, preserving explicit
  disable preferences. Diagnose Linux permissions without global input access.
- Repair missing GSX engine files from bounded, hash-pinned official archives in
  a staged profile with real recovery history. Keep engine health separate from
  full product installation and licensing. Do not rewrite Community entries.
- Check X11 with a bounded handshake before Xbox authentication and game launch.
  Recover the verified Hyprland 0.56.2 stale-listener case only when no X11 clients
  or XWayland process exist; restore temporary configuration changes on failure,
  cancellation and the next startup. Other desktops receive an actionable error.

## 0.3.4 — 29 October 2026

- Instantiate the MP4/H.264 decoder and Wine video post-processing elements in
  the isolated media probe. Cached factory metadata no longer counts as proof
  that a decoder loads. Verify the full chain before choosing a host fallback;
  recheck a failed fallback once, retaining explicit media settings and hardware
  fallback behavior.
- Record typed media readiness, selection reason, missing H.264 decoder/known
  elements and the applied compatibility overlay in safe diagnostics. Exclude
  raw plugin errors, private paths and video caps. Clear stale authentication
  failure phases after successful or retried setup while retaining actual
  cancelled-download failures.
- Retain the codec-selection trace in the isolated developer video probe,
  including the host Wine loader result. Wait through a transient profile
  rebinding snapshot in the Steam recovery test instead of ending its poll early.
- Reproduce a failed decode with the exact official CachyOS 20261015 runner and
  verify the corrected host fallback against the real 3840×2160 loading video:
  two 120-frame NV12 passes, rewind, EOS and shutdown. Cache-sensitive repeats
  selected the host path with both old drivers; this does not establish a
  deterministic source-version difference or confirm the full outcome in #16.
  See [the comparison and limits](media-validation.md).
- Add cached-but-unloadable factory, missing post-processing element and
  sanitized legacy-log regressions. Test packaged updating and rollback against
  the pinned published 0.3.3 package.

## 0.3.3 — 28 October 2026

- Add experimental Orbx Central 4.5.50, Navigraph Hub and Simlink setup. Extend
  GSX preparation and all managed companion apps to Xbox and Steam MSFS 2020/2024
  using the selected game's existing profile and Proton runner.
- Resolve the selected Steam compatibility tool, including global/default tools
  and secondary libraries. Bind actions to the installation, prefix generation
  and Proton revision. Keep saved Xbox settings unchanged and share Steam's
  profile lock during mutations.
- Stage Orbx and GSX on the real prefix filesystem, retain backups, recover
  interrupted GSX publication and block simulator launch until recovery finishes.
  Preserve Proton's shared Mono files during staged native .NET preparation.
- Forward Navigraph browser sign-in to the waiting application in its own profile,
  bypass Hub's failing Process Governor extraction and manage Simlink autostart.
  After a confirmed Steam game exit, clean up only its enabled Simlink/Couatl
  companions; closing Flightdeck leaves an active flight running.
- Verify Orbx's complete official payload, bypass its failing Wine web-installer
  close check and disable tray minimization and login startup. Guard its fixed
  worker port and keep cleanup confined to the selected profile.
- Add Steam runner, stale-action, locking, recovery, companion lifecycle and
  bilingual UI regressions. Verify the official Simlink installer in an isolated
  Steam-shaped profile with real CachyOS Proton 10, preserving profile identity
  and simulator settings. These integrations remain experimental; actual Steam
  simulator flights, licensed GSX services and full-flight Navigraph operation
  still require tester validation. Preserve the 0.3.2 Xbox download auth fix.

## 0.3.2 — 28 October 2026

- Pass the bare MSA ticket to Microsoft's content-license identity field. Strip
  the Xbox delegation marker only at the licensing boundary, for both content
  licenses and license tokens; preserve device authorization and Xbox SISU.
- Select the licensing ticket by its exact Microsoft audience. Report numeric
  licensing faults, entitlement denials and malformed license responses without
  panicking or including credentials or response bodies in diagnostics.
- Add HTTP request/response regressions and run the content-license tests from
  the exact corresponding source archive during release verification. Verify
  a real license request with the same ticket before and after normalization.

## 0.3.1 — 27 October 2026

- Replace the failing legacy Xbox package-authentication exchange with Windows
  SISU for installation, package metadata and service update checks. Preserve
  the signed-in user and existing device identity, using one proof key throughout
  device, SISU and XSTS authorization. Retain user-based package access.
- Return bounded authentication failures with download code 94 instead of a
  panic. Keep HTTP status and numeric Xbox codes available to CLI diagnostics;
  exclude tickets, response bodies and request URLs. Remove XSTS debug body logs.
- Verify the old HTTP 400 / X-Err 00000005 and the successful replacement on
  the same local account without a proxy. The integrated CLI retrieves MSFS 2024
  package metadata and starts the Microsoft CDN metadata transfer. The bounded
  transfer test stops intentionally and removes its temporary files; a complete
  new installation and the original reporter's retest remain unverified.

## 0.3.0 — 26 October 2026

- Detect MSFS 2020 and 2024 in native, Flatpak and Snap Steam libraries. Select
  Xbox and Steam installations separately, preserving the existing Xbox config
  for rollback. Bind actions to the selected installation and profile generation.
- Launch through the matching Steam client and observe the actual simulator,
  including external starts. Keep Xbox setup, Store updates and Xbox cloud sync
  out of Steam profiles. Steam retains game downloads, updates and synchronization.
- Use the pinned shared Fenix Rust engine for Steam setup, patch updates,
  compatibility-tool selection, display configuration and restoration. Reuse
  tools across supported profiles and keep cleanup confined to the selected game.
- Add Steam pages, installation selection, Community-folder discovery and typed
  Fenix setup diagnostics in English and German. Retain independent patch release
  metadata and show current failures and cancellation controls.
- Forward Steam's Windows-version requirement during .NET preparation. Preserve
  JSON report hashes, strict decoding and canonical maintainer output after
  enabling the shared engine's dependency features.
- Validate synthetic profile lifecycles, HTTP source boundaries, both-language
  rendering and an offline real-Wine setup → patch update → official Fenix hook →
  restore sequence. Real Steam simulator flights still require tester validation.

## 0.2.29 — 26 October 2026

- Recover interrupted Fenix update preparation by validating the preceding
  installation and reinstating its marker before a fresh staged update. Retain
  installed aircraft, settings and the original restore chain. Refuse recovery
  after publication starts, or when runners, patch modules or scripts changed.
- Keep failed and interrupted Fenix setup visible in Updates and on the overview,
  including after a service restart. Offer an idle, runtime-bound retry action.
  Clear completed download progress when profile preparation starts.
- Stop and wait for the staging Wine server before rebooting after a failed .NET
  health check; avoid aborting through the same unreachable server connection.
- Add recovery and UI regressions, plus an explicit real-Wine update probe using
  public old/new payloads, retained user data and rejection of changed modules.

## 0.2.28 — 25 October 2026

- Materialize readable Tahoma font symlinks as private files during Fenix setup
  and registration. Repair broken profile links from the runner, accept linked
  runner fonts and existing hard-linked fonts, and keep shared sources unchanged.
  Preserve the font-directory containment check and bounded regular-file reads.
- Integrate Fenix patch 0.1.1 as the verified fallback, with exact 0.1.0 upgrade
  snapshots for all four supported runners.
- Extend the explicit fresh-patch test with linked fonts, real Wine registration
  and shared-source hash checks before the complete patch setup.

## 0.2.27 — 25 October 2026

- Copy new private Wine profiles and runners without restoring foreign ownership,
  POSIX ACLs or extended attributes. Preserve contents, executable files,
  symbolic links, hard-link topology and timestamps. GNU cp archive mode could
  copy all files but still fail on unsupported permission metadata, leaving
  Fenix setup in its preparing stage (issue #7).
- Translate the profile/runner copy failure in the English launcher.
- Add a native copy regression test and explicit account-free tests for a full
  profile/runner copy on a chosen filesystem and fresh Fenix patch setup.

## 0.2.26 — 25 October 2026

- Add a dedicated Credits page in German and English, reachable from the sidebar
  and compact icon rail without a game installation or service connection. Name
  OnlyCrayZ-Labs’ media fix and taskqueue test contributions, gut-engel’s
  NVIDIA/Wayland sign-in correction, upstream projects, the UI stack, font
  and artwork.
- Open installed launcher, Rust, compatibility and graphics license documents
  from Credits. Link the complete dependency inventory and corresponding source
  releases for the running version.

## 0.2.25 — 25 October 2026

- Link the Discord community from the launcher diagnostics, English/German
  project pages and documentation.
- Integrate OnlyCrayZ-Labs’ Wine GStreamer system-memory negotiation fix for
  the matching Xodus runner. Reuse a small verified game-only loader view;
  preserve installed runners, prefixes and Fenix loader changes. Unknown Proton
  builds retain their own modules. Ship pinned corresponding sources and LGPL
  notices; require the matching payload in complete release packages.
- Configure media using the actual selected runner, including alternate Proton
  versions, and report media overlay selection in isolated decode probes.
- Add concurrent task-queue, active shutdown, duplicate-handle, composite-port
  and delayed-callback checks adapted from OnlyCrayZ-Labs.


## 0.2.24 — 25 October 2026

- Reuse one private runner per unchanged Proton build/overlay. Carry the latest
  profile forward, keep the last rollback profile, and automatically reclaim
  obsolete generated profiles and recognized legacy copies while idle. Protect
  active runners, other configured runtimes, Fenix restore references, modified
  backups, unknown content and running processes. Quarantine deletions without
  following links; defer cleanup when the audit is incomplete.
- Resume Proton profile switches on filesystems without `RENAME_EXCHANGE`,
  including ntfs3, using the durable journal and checked directory identities.
  Return to the default environment even if an old trial runner was deleted.
  Keep session recovery available when cloud transport prerequisites are broken.
- Remove missing persisted absolute .NET startup-hook paths before Fenix install
  and repair, retaining original values locally. Official FenixApp 1.0.359
  reproduced exit 82/FileNotFoundException before the fix and exit 0 after it.
- Verify bundled H.264 decoder loading in an isolated child; use a separately
  verified host codec environment when the bundled stack fails. Retain explicit
  media settings and keep 32-bit plugins out of the 64-bit registry.
- Accept root-managed system directory aliases when launching the desktop app,
  including Bazzite's `/home` alias. User-controlled state links remain rejected.

## 0.2.23 — 25 October 2026

- Include existing local Fenix installations in automatic update discovery.
  Previously they were skipped, while the UI could label the embedded available
  version as though it were installed. Show verified installed versions and
  available releases separately.
- Adopt known local patches through the existing staged profile/runner
  transaction, automatically before MSFS/Fenix launch or through **Adopt setup
  & update**. Check known native modules and launch scripts before copying and
  again before publication. Preserve aircraft, renderer/FCU preferences and a
  restore point. Unknown customizations remain protected. Restoring the local
  profile pauses automatic adoption until an explicit adoption succeeds, so the
  next launch preserves the user's rollback.
- Show Fenix updates and pending adoption on the overview, in the Updates page
  and with an update navigation indicator. Keep checks available during a game;
  adoption and installation require an idle selected runtime.

## 0.2.22 — 25 October 2026

- Integrate [Fenix patch 0.1.0](https://github.com/marselnenaj/fenix-a320-linux-patch/releases/tag/v0.1.0)
  as the verified fallback. Deploy its native Rust runtime with executable
  permissions, alongside the existing Windows window/display helpers.
- Support installer contract 2 for the new helper layout. Earlier launchers must
  update to 0.2.22 before adopting this patch; compatible future patch releases
  continue to update independently. Preserve all four pinned runner identities,
  archive checks, aircraft, renderer preferences and restore points.
- The Wine/DXVK fixes are unchanged from preview.11. This release does not
  resolve FenixSystem memory growth or establish Fenix v3 full-flight stability.

## 0.2.21 — 24 October 2026

Documentation follow-up: add a Bazzite/Fedora Atomic installation workaround
for the symbolic-link path error, checked with a synthetic home-directory
layout. The published installer binaries are unchanged; installation and
simulator operation on an actual Bazzite desktop remain unverified.

- Offer **Retry download** after a failed game download or Microsoft sign-in
  when the verified components support file reuse. Keep the original private
  workspace and job reserved; retry checks completed files through the existing
  resume protocol. Cancellation releases the reservation and retains files.
- Show retry, cancellation and numeric failure codes before the first runtime
  is configured. Retain bounded sign-in retries and package-revision checks;
  a changed pinned revision requires checking the update again.
- Include the latest setup phase, edition, attempt count and numeric sign-in/
  download exit codes in diagnostics and support drafts, surviving a service
  restart. Capture no raw child output, paths or credentials. An old report
  cannot recover missing historical error codes.
- Keep the existing limit that interrupted jobs cannot be resumed after a
  background-service or Linux restart. Closing the desktop window keeps the
  background service running. This release improves recovery and evidence; it
  does not establish the cause of an earlier generic download failure.

## 0.2.20 — 23 October 2026

Documentation follow-up, 24 October: record the issue #4 reporter's MSFS 2024
menu/scene startup with 0.2.20 and CachyOS 11.0-20261015 after a black loading
screen. Video playback, the one-third-rate VSync limit and complete flights
remain unverified. This follow-up did not change the published 0.2.20 binaries.

- Discover Fenix patch releases independently of Flightdeck releases, using the
  existing automatic startup checks and a manual check under **Updates** and
  **Mods → Fenix A320**. Retry failed checks after five minutes; successful
  checks are throttled to 30 minutes.
- Apply a discovered compatible patch before the next MSFS or Fenix launch,
  while both are closed, through the existing recoverable profile/runner
  transaction. Preserve aircraft, graphics preferences and restore points.
- Retain the installed manifest across service restarts and Proton switches.
  Prefer stable releases when leaving previews and prevent automatic downgrades
  or re-entry into previews after a stable installation.
- Verify repository-bound release metadata, archive size and SHA-256, every
  extracted payload and the supported installer contract. Incompatible runner
  or setup changes require a Flightdeck update. Keep preview.11 as the embedded
  fallback; official aircraft updates remain in the Fenix app.

## 0.2.19 — 23 October 2026

- Adopt [Fenix patch 0.1.0-preview.11](https://github.com/marselnenaj/fenix-a320-linux-patch/releases/tag/v0.1.0-preview.11),
  including @malik05051's Gateway loader-name correction and matching PE NTDLL
  for all four pinned runners. The reporter confirms a working Gateway/EFB and
  GPU displays with **USE DISPLAY SAFE MODE off** on the affected NVIDIA setup.
  Aircraft files, saved renderer preferences and restore points survive updates.
- Make Fenix game launches and helper commands explicitly select the tested
  server-synchronization backend on Wine 11 with `PROTON_NO_NTSYNC=1`, retaining
  CachyOS 10's separate fast-sync switch. Synchronization alone did not fix #7;
  the Gateway correction is the separate loader-name change above.
- Retain the official v3 installer/startup and Direct2D Display-memory fixes.
  Fenix v3 full-flight stability, an earlier Wine assertion and the separate
  Fenix 2 System-memory growth remain open. The regular Fenix 0.1.0 release
  remains in preparation.

## 0.2.18 — 23 October 2026

Stable launcher release with the experimental Fenix patch preview.10.
Existing native launchers can use the in-app updater. Close MSFS and Fenix,
update Flightdeck, then open Fenix under **Mods** to update an earlier patch.
Installing Flightdeck itself does not install the aircraft.

- Adopt [Fenix patch 0.1.0-preview.10](https://github.com/marselnenaj/fenix-a320-linux-patch/releases/tag/v0.1.0-preview.10).
  Fix Wine compatibility paths used by the new official installer, image
  validation, volume/current-directory lookup and scoped cryptographic APIs.
  Original vendor executables and authentication are unchanged. Verified
  preview.9 installations retain the aircraft, renderer preferences and restore
  point when updating; the earlier Direct2D memory-leak fix is retained.
- Check official Installer 1.0.359, login and working cockpit displays with Fenix
  3.0.0.7454, MSFS 2024 1.8.16.0 and Xodus 11.0 / 7c0b4354, using GPU displays
  and Legacy readouts. **Fenix v3 remains experimental:** one startup hit
  Wine's `create_view` assertion. A later fresh production-payload start and
  five warm backend restarts did not reproduce it; its cause remains unknown.
  Full flights, long-session stability, NVIDIA and real v3 cockpits on other
  runners remain unvalidated. See the [test sequence and limits](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0-preview.10/docs/fenix-v3.md).
- Document the [Nobara/RTX 5080 workaround from issue #3](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/3#issuecomment-6348916712):
  the reporter can launch with an alternate Proton such as Proton-GE. This is
  a user-confirmed workaround; the default Xodus exit-84 crash remains
  unresolved and its working alternate version has not yet been specified.

## 0.2.17 — 22 October 2026

Stable launcher release with Fenix patch preview.9. Existing native launchers
can use the in-app updater; update the Fenix patch separately under Mods.

- Adopt the Direct2D per-figure memory-leak fix contributed by M3rocto in
  [Fenix PR #10](https://github.com/marselnenaj/fenix-a320-linux-patch/pull/10).
  The rebuilt Xodus, Proton Experimental 11 and CachyOS 10/11 overlays passed
  geometry, rendering and heap-lifetime probes. Additional guards terminate
  synthetic degenerate paths; these do not establish a fix for displays
  freezing in the simulator. Long-flight memory behaviour remains unvalidated.
- Prepare the pinned Microsoft .NET 10.0.13 x64 Desktop Runtime when installing
  or updating Fenix, and check it before opening an official Fenix application.
  This addresses the missing Windows Desktop Runtime error from the v3
  installer. The runtime host and an isolated WPF window were tested; a full
  Fenix v3 flight has not been independently verified.
- Make the aircraft-installation step explicit: sign in to the official Fenix
  installer, install the A320 for the selected simulator, wait for completion
  and then close the installer.

## 0.2.16 — 22 October 2026

Security update with rebuilt native compatibility components and corresponding
sources. Existing native launchers can use the in-app updater.

- Migrate XAL authentication, OAuth and the Store policy client off Reqwest 0.11
  and h2 0.3, removing the HTTP/2 memory-exhaustion path in
  [RUSTSEC-2026-0258](https://rustsec.org/advisories/RUSTSEC-2026-0258.html).
  Preserve OAuth request-body authentication, PKCE and redirect rejection.
- Update EAPPX's XML stack to xmlserde 0.14 and quick-xml 0.41, addressing the
  quadratic attribute processing in
  [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194.html).
  Test all three package XML readers, escaped filenames, encrypted extraction
  and rejection of XML-encoded unsafe paths.
- Retain the exact upstream library revisions and MIT notices in the exported
  source patch and native source archive. These dependency fixes do not claim
  that every transitive advisory is resolved.

## 0.2.15 — 22 October 2026

Stable security update with rebuilt native compatibility components and full
corresponding source archives. Existing native launchers can use the in-app updater.

- Update Rustls and WebPKI in the launcher and native broker, including the
  certificate-distrust fix for [CVE-2026-111943](https://rustsec.org/advisories/RUSTSEC-2026-0382.html).
  Also update the broker's PKCS#11 dependency to its corrected patch release.
- Protect manual MSIXVC and EAPPX extraction against unsafe package paths,
  normalized aliases, symlinks, hardlinks and special-file targets. Validate
  the full output plan and anchor payload and metadata writes to the chosen
  destination. Preserve ordinary extraction permissions and resume behavior.
- Dependency review still identifies advisories in the legacy XAL/Reqwest stack
  and local EAPPX XML parser. This release does not claim that all dependencies
  are advisory-free; those components need further modernization.

## 0.2.14 — 21 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.13 launchers can use the in-app updater.

- Fix Xbox sign-in error `000-2000-0x80004001` after selecting another Proton
  build ([issue #4](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/4)).
  Retain Flightdeck's working JSON and PropertySet implementations with private
  names and scoped WinRT registrations; repair already-selected profiles before
  launch. Wine 11 also needs unique internal DLL names to prevent substitution
  by its JSON stubs. Real MSFS 2024 sign-in, profile loading and token/signature
  requests passed with Proton Experimental 11. Safe diagnostics now preserve
  startup and authentication-stage results. The account-free Proton probe
  checks runtime initialization, JSON response parsing and shutdown with a
  ping-only local broker. An isolated CachyOS 10 start still exited with code 84
  before authentication completed; that crash and the VSync report remain open.
- Adopt Fenix patch 0.1.0-preview.8 with GPU display sharing on the pinned Xodus
  and Proton 11 runners. Deploy the matched DXGI into the profile, migrate
  earlier automatic CPU settings and preserve explicit CPU fallbacks. CachyOS
  Proton 10 remains CPU-only. Xodus PFD, ND, ECAM and a moving airspeed tape
  were checked in the live cockpit; NVIDIA, GPU weather radar and full flights
  remain unvalidated. Readout setup can still complete after the first load.

## 0.2.13 — 20 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.12 launchers can use the in-app updater.

- Fix Proton selection for native installations without Fenix. Its compatibility
  check still expected an environment setting in the old shell launcher; the
  shipped wrapper delegates to Rust and no longer contains it. The actual native
  wrapper is now accepted, and the public selection path is tested with those
  shipped bytes instead of a synthetic marker.
- Configure media inside the game's Wine child. Selected Proton builds that
  bundle FFmpeg receive their libraries and GStreamer plugins; the default
  Xodus runner and stock Steam runners retain the system-codec path. NVIDIA
  starts prefer the available software H.264 decoder, preserving explicit
  decoder choices and hardware fallback. Bundled runners have separate media
  registries. Safe diagnostics distinguish a placeholder video from H.264
  decode failures. See [issue #3](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/3); the reported
  RTX 5080 game crash still requires confirmation on that machine.
- Add `probe media`: decode and rewind a local video through Media Foundation
  in an isolated Wine prefix. The expected dimensions, frame count and decoded
  byte count must match on both passes; a small fallback video cannot pass.
  The pinned Xodus runner with system codecs passed with the actual 4K MSFS
  loading video, including seeking, end of stream and clean shutdown.
- Bound the history kept around saves. Local backups before and after each
  session, downloaded cloud snapshots and the local states saved before each
  import gained an entry with every session and were never removed; one
  runtime held 436 of them, 2.7 GiB, after three weeks. Before a start, each
  kind now keeps its newest 20 entries, everything younger than 30 days and
  whatever the snapshot cache, a pending transfer, the comparison baseline or
  the running launcher still refers to. With the clock set 60 days ahead, a
  copy of that runtime's history went from 2.7 GiB to 366 MiB with every
  referenced entry in place. If a stored record cannot be read, nothing is
  removed.

## 0.2.12 — 13 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.11 launchers can use the in-app updater.

- Fix a profile staying locked after the first game session on desktops whose
  own services hide their environment, such as KDE Plasma's authentication
  agent. After a session ends, Flightdeck makes sure nothing of it still runs
  before it backs up and synchronizes saves. It counted every process of the
  user that it could not inspect as a possible leftover, apart from six known
  names, so `polkit-kde-authentication-agent-1` held the profile until the
  next boot: cloud saves reported "unsafe session", the simulator could not be
  started again and the recovery could not help, because such a process is
  never ended. A process that cannot be inspected now counts only if its
  command line refers to the runtime or if it is by its name a Wine, Xodus or
  Flightdeck process. After updating, a profile that is still locked is
  released by the recovery offered there, or by a reboot.
- Bound what Flightdeck leaves on disk. Session logs are kept for the newest
  ten sessions besides the current one; until now every session's directory
  stayed, and a single `game.log` reached 162 MiB. A log that a running game
  keeps filling keeps its first and newest 16 MiB and gives the middle back to
  the file system. Logs that runs append to start anew beyond 8 MiB. The cloud
  helper's Wine prefix, about 630 MiB, exists once per combination of runner
  and components, and the ones of earlier versions were never removed; they
  are now removed after a completed cloud run, unless a process still uses one
  or one was left in use since the last boot. Save backups, cloud snapshots
  and import backups are not limited by this change.
- Record whether a game ever set up its renderer. Until now only NVIDIA
  starts logged the renderer at its informative level; on other hardware the
  log of a Direct3D 11 game such as MSFS 2020 held warnings only, and a report
  of a black screen looked exactly like a healthy session. Every start now
  uses that level, and diagnostics count the created Direct3D 11 device, the
  created swapchains and their failures. For a small test program the
  renderer writes about 530 lines at this level.
- Move the checks, packaging and release steps, the developer probes and the
  compatibility test drivers from Python scripts to the Rust maintainer tool in
  `tools/`. Launcher and full packages built by both implementations from the
  same inputs are identical member for member, and the distribution checks run
  the same tool inside their containers, so the test images no longer install
  an interpreter. The native component build script needs no Python either.
  The installed launcher is not affected. The probes and drivers that need a
  GPU, a Wine runner or a display were translated and tested against stand-ins,
  not yet run on real hardware.
- Replace the last Python scripts and the Python stand-ins inside the Rust
  tests. The rendering check is `probe graphics-rendering`; the loader test and
  the test peers (a stand-in Xodus service, Wine, cloud helper and others) are
  Rust, in `tests/native_loader.rs` and `examples/test-programs.rs`; the whole
  test suite passes with no interpreter available. The transition check is
  `probe transition-update`. It gives the same result as the script for a
  0.2.3 candidate. For candidates from 0.2.8 on the script could no longer
  pass: those launchers end without a session before they start their
  service, so the check now starts the installed service itself. An
  interpreter is needed only where a Python release is the subject: the
  transition check, `probe benchmark-launchers` and a rollback to a retained
  0.1.x release.
- Store runtime sources, not yet in the shipped components: log each distinct
  user interface and each distinct query result once instead of on every
  call. These two lines made up 98 % of a 162 MiB session log. The rebuilt
  Store module writes 2 lines where the published one writes 50 for 50 calls;
  the thirteen synthetic component suites and the broker timeout checks pass
  with the rebuilt modules under the pinned runner. The pinned 0.1.16
  component archive is unchanged, so installations keep the old behaviour
  until a component release. The idea of logging per call only on request is
  from the macOS fork by ssyuk.

## 0.2.11 — 12 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.10 launchers can use the in-app updater.

- Use Fenix patch **0.1.0-preview.6**. Its patched `ntdll.so` no longer needs
  the host library `libunwind.so.8`, which the patch build had linked by
  accident. Without that library a patched runtime cannot load `ntdll.so` and
  no Windows program starts; hosts with GStreamer usually have it installed, so
  this mainly affected the patch's Steam installation. Every other overlay file
  is identical to preview.5, and verified preview.5 installations update
  without reinstalling Fenix.
- Let **Finish setup** for Fenix proceed once `fenixConfig.xml` exists. It
  required `persistancy.xml` as well; a user of the patch's Steam installation
  reported that Fenix creates that file only when the aircraft is first loaded,
  which would leave the setup waiting at sign-in. CPU displays and auto-start
  are now applied without it, and the panel asks to reapply the settings after
  the first load for the Legacy FCU readouts. Not reproduced in Flightdeck;
  installations that already have both files behave as before.

## 0.2.10 — 12 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.9 launchers can use the in-app updater.

- Keep Xodus windows open on systems with an NVIDIA GPU. WebKitGTK's DMA-BUF
  renderer aborted the Microsoft sign-in window right after it opened with
  “Error 71 (Protocol error) dispatching to Wayland display”. Flightdeck now
  starts sign-in, the in-game Store purchase window and the Store window check
  with `WEBKIT_DISABLE_DMABUF_RENDERER=1` when an NVIDIA GPU is present. A value
  set in the launcher's environment is kept. Reported and fixed for sign-in by
  @gut-engel on nvidia-open 615 with KDE Plasma Wayland. The other two windows
  use the same webview and setting but are not yet confirmed on an affected
  system.
- Repaint the whole window after a change. Scrolled panels such as
  **What's new?** showed stale or overlapping text when the pointer moved over
  them, because the software renderer repainted text regions without their
  scroll offset. In a local measurement of a 1280×900 window, one complete
  repaint took about 6 ms instead of 40–90 ms for the region-wise repaint.
- Keep hard-wrapped list items in release notes together. Continuation lines of
  a list item were shown as separate paragraphs.

## 0.2.9 — 12 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.8 launchers can use the in-app updater.

- End the remaining processes of an interrupted or stopped session instead of
  fencing the launch until a reboot. **Check session** and the automatic
  post-exit release send SIGTERM, then SIGKILL, to processes bound to the runtime,
  verify they are gone and back up local saves. Since 0.2.6 such a straggler
  blocked every launch until reboot.
- End all Fenix processes with the simulator, including the Fenix app and its
  WebView. An open Fenix app previously kept the next launch blocked.
- Leave another Windows application in the profile, such as an installer,
  running when the simulator exits and name it as the reason for the block;
  **Check session** ends it on request. Processes that cannot be attributed to
  the runtime are never signalled.
- Name a surviving process with its PID in the launch note and in diagnostics
  (`cloud_sync.error_details`). A process stuck in the kernel after a GPU device
  loss still requires a restart, and the message now says so.
- Ignore processes bound to a Wine prefix that no longer exists; they previously
  blocked the launch without a stated reason.
- Use Fenix patch **0.1.0-preview.5**. Its Flightdeck and earlier Proton
  overlays are identical to preview.4; verified preview.4 installations update
  without reinstalling Fenix. The patch adds an overlay for CachyOS Proton
  `cachyos-11.0-20261005-slr`, which the Proton menu now marks as compatible.

## 0.2.8 — 8 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.7 launchers can use the in-app updater.

- Prevent inherited Linux .NET loading/runtime overrides from reaching Windows
  installer and repair commands. A controlled startup-hook override reproduced
  Fenix App 1.0.286 exit 82; the same hook succeeds without that override. This
  addresses that failure path, not every possible cause of exit 82.
- Retain the latest recorded Fenix hook failure in the interface after reopening
  the launcher. A failed repair points to diagnostics instead of prioritizing
  another identical retry. Label the Linux-patch check separately from app setup.
- Include recognized managed exception types and an explicitly logged CLR
  exception code in Fenix diagnostics, without raw third-party messages or paths.
  An exit code by itself remains an unknown hook failure.
- Explain installer startup failures caused by an unsupported architecture,
  older glibc, a missing loader or unavailable libraries before invoking the
  native installer. Keep the optional probe bounded and show German or English
  guidance in an available graphical error dialog.
- Check exact runtime libraries by loading them in an isolated helper instead
  of matching `ldconfig` cache text. Retain configured GStreamer plugin paths
  while keeping the test registry separate from the user's cache.
- Report unavailable desktop sessions and failed or timed-out file dialogs
  clearly; distinguish a cancelled picker from an execution failure.
- Fix startup without a system CA certificate store by keeping the desktop
  service client, native interface and local Fenix display client independent
  of TLS trust roots for loopback HTTP. External HTTPS keeps normal certificate
  verification and its CA-store requirement.
- Add a container test matrix for launcher installation, local API and software
  X11/Wayland windows on Ubuntu 24.04, Debian 13, Fedora 44, Arch Linux and
  openSUSE Tumbleweed. All five passed locally with the packaged development
  candidate on 8 October 2026, including 84 API checks and the actual Wayland
  application surface. These tests do not validate MSFS, hardware graphics,
  sign-in or licensed add-ons. See the
  [scope and current status](install.md#linux-test-matrix).

## 0.2.7 — 8 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.6 launchers can use the in-app updater.

- Simplify Fenix and GSX setup to one highlighted next action. Summarize detected
  components and put additional installer, repair and recovery options under
  **Manage & repair** instead of repeating every completed setup step.
- Show current failures and progress beside the next action. Offer Fenix app
  repair after a failed install hook; retain explicit reinstallation when the
  manager is missing but the Fenix companion is present.
- Bind Fenix job history to the selected simulator installation and hide stale
  add-on feedback after a profile change or connection failure.
- Describe Fenix completion as local configuration. File detection does not
  verify an aircraft installation, account activation or license.
- Keep GSX explicitly experimental and its simulator functionality unconfirmed.
  Require each local prerequisite before reporting setup complete, and keep
  running jobs distinct from completed setup. This release does not establish
  licensed GSX operation, Couatl/SimConnect connectivity or working ground services.
- Keep the main launch button label centered on one line during save loading,
  startup, stopping and final save synchronization, in both interface languages.
- Update the English and German add-on guides to match the simplified controls.

## 0.2.6 — 8 October 2026

Stable release with full installer and corresponding source archives.
Existing 0.1.22 and native 0.2.1–0.2.5 launchers can use the in-app updater.
Older clients need the full installer or explicit bridge.

- Allow edition switching while idle even when cloud saves need attention.
  Keep switching blocked during synchronization and running work.
- Add **Check session** for an interrupted session: verify that its processes
  have ended and back up local saves before releasing the launch block.
  This action does not start the simulator or upload saves.
- Recover a stale, disconnected Xodus socket before launch while preserving
  sockets still owned by a running service.
- Make disabled controls and blocked launch states visibly distinct. Show an
  abnormal simulator exit with retry guidance instead of reporting readiness.
  Keep **Stopping simulator…** visible until the stop completes, and finish
  cleanup of orphaned Wine services only within the stopped runtime's profile.
- Honor Stop during startup preparation before spawning the service or simulator.
- Check for launcher and simulator updates periodically in the background,
  retry failed or deferred checks, and show their status on the overview.
  Completed, cancelled or failed update jobs no longer suppress newer checks.
- Format release notes with headings, lists, code blocks and explicit links in
  a scrollable panel. Keep update downloads under the user's control.
- Recognize the FenixApp manager separately from the aircraft. Add **Repair
  Fenix app** for retrying its official install hook with the existing Wine/.NET
  compatibility settings, bounded execution and cancellation.
- Include validated Fenix attempt results in support diagnostics so a generic
  hook warning can be investigated without exporting raw logs or account data.
- Keep the interrupted-session guard when runtime-bound processes remain after
  the supervisor exits; do not begin cloud backup or upload in that state.
- Handle a process exiting during its `/proc` status read without falsely
  retaining the session block. Unknown live writers remain blocked.
- Refuse nested mounted directories during managed tree removal, including bind
  mounts on the same filesystem, so removing a mod cannot traverse their contents.
- Preserve Store-check failures even when individual stages already passed.
  A failed helper process no longer enables automatic cloud retry.
- Use the detected Store region on first setup while preserving a later manual
  choice. Honor cancellation during VR preparation before changing its settings.
- Avoid serializing an existing local save a second time during cloud export.
- Require the local session token for API reads as well as actions, and update
  the locked rustls dependency.
- Coordinate update and rollback restarts from the running launcher so older
  versions can take over without relaxing the service's authentication.
- Shorten both README versions and add a documentation index. Update NVIDIA's
  normal-rendering status to user-confirmed, retaining known limitations such
  as missing startup video and separate validation requirements for VR/features.

## 0.2.5 — 5 October 2026

- Keep existing MSFS editions selectable when they need repair. Apply pending
  bundled component updates when selecting an inactive edition, fixing MSFS 2020
  incorrectly opening the new-installation flow after a launcher update.
- Acknowledge Start immediately and show cloud synchronization and final-save
  progress on the main button. Keep a running simulator's state visible.
- Switch pages using cached state while refreshing in the background. Cancel
  superseded reads and retain context and confirmation checks for mutations.
- Decode each panorama once and avoid unnecessary text clipping masks. Document
  the measured native UI improvement over 0.2.4 in [performance results](performance.md).

0.2.5 became GitHub Latest. Users still on 0.1.21 or earlier need the full
installer once, or an explicit 0.1.22 bridge install. Existing 0.1.22 and native
0.2.1–0.2.4 launchers update normally.

## 0.2.4 — 5 October 2026

- Restore automatic Proton discovery, readable runner labels and the current
  runner selection. Keep Flightdeck, installed Experimental/CachyOS and a custom
  path option; enforce Fenix compatibility when switching.
- Recognize legacy Fenix installs as existing setups and show managed setup
  steps according to their actual dependencies.
- Bring native pages and controls closer to the historical interface, restore
  missing region/help/status content and fix clipped or invisible software-rendered text.
- Add confirmed Community add-on uninstallation with path/size preview, exclusive
  reservation, changed-entry checks and retained external link targets. Fenix/GSX
  companion programs use their official installers.
- Remove the retired web implementation and Node build checks from the source
  tree. Keep artwork and synthetic migration fixtures; Python remains maintainer
  build/test tooling only. Remove fully merged development/release branches.

Existing native versions update directly. Older Python launchers still receive
0.1.22 first; GitHub Latest remains on that unchanged transition release.

## 0.2.3 — 5 October 2026

Update from 0.2.1 or 0.2.2 through **Updates → Flightdeck**, or use the
[full 0.2.3 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.3).
Older Python launchers still receive 0.1.22 first, then 0.2.3. GitHub Latest
remains on the unchanged bridge.

Replace the browser interface with a native Rust/iced desktop for all six views.
Keep the existing artwork, font, icons, DE/EN navigation and backend operations.
Persist native language preferences, verify service reconnections and bind
mutations to their selected installation and reviewed job/plan. Closing the
window leaves managed game and installation work with the existing service.
Narrow tiling below the requested minimum uses an icon rail and stacked cards;
setup edition selection is separate from the active game. The previous web
implementation remains only as a migration test reference.

Prepare Fenix's .NET compatibility settings in its Wine registry as well as the
launch environment. Wait for detached installer children and check new log output
for failed or unfinished Velopack hooks. The official FenixApp 1.0.286 install
hook passes in a new account-free prefix with the Unix .NET variables removed.
This reproduces and fixes the known ICU case; the reported screenshot alone does
not prove the remote user's exact cause. [Details](native-ui.md).

Keep the release ABI at glibc 2.39 with a pinned Ubuntu build image and a package
check for imported symbol versions. Native rendering/action tests, real service
client checks and isolated installer/update checks replace the production
browser contract suite.

The installed background service starts in 87.40 ms versus 148.48 ms for
Python 0.1.21, with 14.38 MiB versus 35.18 MiB resident memory in this workstation
run. These medians do not measure GUI performance or MSFS frame rate.
[Method and results](performance.md).

## 0.2.2 — 4 October 2026

Update from 0.2.1 through **Updates → Flightdeck**, or use the
[full 0.2.2 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.2).
Older Python launchers still receive 0.1.22 first, then the current native release.
The bridge remains GitHub Latest. This release finishes the Rust source transition;
it does not establish a new fix for the reported NVIDIA simulator failure.

Remove the Python launcher, vendored Fenix engine, Python installer, pip package
metadata and duplicated application tests from the current branch. Runtime
entry scripts now have one Rust-backed source. Native builds and packaging no
longer depend on the retired application or its installer. The published 0.1.22
bridge and previous release archives remain unchanged for migration and rollback.

Keep legacy save, .NET, graphics-undo and diagnostic behavior covered with frozen
synthetic fixtures and Rust tests. Browser checks now always start Rust. Graphics,
VR, Proton and C++ save probes invoke the production Rust implementations through
a development-only driver; Fenix no longer installs the unused Python display
helper. Remaining Python files are maintainer build/test tools only.

## 0.2.1 — 4 October 2026

Use **Updates → Flightdeck** in 0.1.22, or download the
[full 0.2.1 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.1).
Earlier launchers receive 0.1.22 first; install, restart and check again. GitHub
Latest remains on that bridge. Versions 0.2.0-dev.1 and the withdrawn 0.2.0 need
the full installer because their native update check has the defect fixed here.
The installer preserves the existing configuration and previous launcher.

Fix native update discovery and streamed component downloads. The HTTP clients
mixed an asynchronous read timer with blocking response reads; ordinary download
workers could panic when reading a response body without a Tokio runtime. Both
now use the blocking client's own connection/read/write timeouts. Slow reads
still time out, while complete downloads may span many individual read windows.
The release feed now also accepts GitHub’s top-level JSON array; the old
object-only decoder rejected it. Duplicate-key rejection is retained at every
level, and cloud APIs still require objects. HTTPS, redirect restrictions and
checksum validation remain enforced. Local
streamed-body and stalled-body tests reproduce the old defect and cover the fix.

This stable release also makes the native launcher available through 0.1.22;
preview flags and preview version tags had excluded 0.2.0-dev.1 from discovery.
It includes the launcher/UI/runtime changes below. NVIDIA/MSFS hardware limits
remain unchanged.

## 0.2.0 — 4 October 2026 (withdrawn)

Withdrawn immediately after the public migration check exposed an HTTP body-read
panic in the native updater. Existing 0.1.22 launchers could discover and install
it, but the resulting Rust service could not perform its next update check. Use
0.2.1. Its GitHub release entry has been removed; the source tag is retained
for historical reference.

## 0.2.0-dev.1 — 4 October 2026 (retired preview)

This preview has been superseded by 0.2.1 and removed from the GitHub release
list. Use the current 0.2.1 installer. GitHub's Latest entry stays on the
0.1.22 Python transition release so older launchers receive the bridge first.

- Share one UI polling timer, refresh idle views every ten seconds and retain
  faster updates during running jobs. Pause all scheduled reads while hidden,
  then revalidate on focus/visibility return. Explicit refresh remains immediate.
- Preserve unchanged check-list DOM nodes, reuse locale formatters and centralize
  job reservations so completing one operation cannot release another's lock.
  Strict JSDoc/TypeScript checks cover these shared UI modules in CI.
- Serve the Rust launcher's embedded images and UI assets without allocating
  another complete copy for each response. Origin, CSP and no-store protections
  remain enforced.

- Fix delayed mapped-DLL loading with the default Flightdeck runner. The
  native Wine mapping redirects the main EXE only; both bridges now expose
  mapped DLLs and the working directory through the same temporary memory-backed
  game view used by selected Proton. The default EXE retains its native mapping.
  Encrypted game files and ordinary resources are preserved. A real Windows
  probe reproduces error 193 before this correction.
- Start MSFS 2020/2024 with `-FastLaunch` at the final Wine boundary. This
  applies the reported intro-path workaround for a black main view with visible
  menus/secondary views to the default runner and selected Proton versions.
  Python and Rust preserve explicit arguments without duplication; managed
  older Python runtime scripts receive the change through component updates.
  Set `FLIGHTDECK_FAST_LAUNCH=0` in the launch environment to compare the full
  intro path. The affected Linux/NVIDIA game rendering still needs confirmation.
- Keep MSFS 2024's saved NVIDIA options consistent with the effective launch
  profile. Automatic/Compatibility replace saved DLSS with TAA, disable saved
  Reflex and NVIDIA frame generation, and retain a per-field undo record in the
  Wine profile. Features mode restores values still matching Flightdeck's
  changes. Other upscalers, resolution, quality, content and subsequent user
  edits are retained. The Python reference uses the same format and behavior.
  This prevents incompatible saved options; the affected RTX 4080's black
  primary view remains unverified on hardware.

Port the launcher backend, desktop service, installer, updater and runtime
helpers to Rust. The native package needs no Python interpreter. It preserves
the browser interface, runtime/save formats and Wine/Store ABI components.
Setup, cloud saves, add-ons, graphics/VR, diagnostics and recoverable maintenance
jobs use native implementations. The previous Python release remains available
for explicit launcher rollback and as the test reference.

The alternative Proton loader now uses the licensed temporary game view as its
working directory. Previously, delayed DLL loads relative to the working
directory could reach encrypted originals and fail with Windows error 193. The
expanded synthetic test reproduces that failure before the fix and passes with
Experimental 11.0 (20260924) and CachyOS Proton 10 sunset after it, including
D3D11/D3D12 rendering and return to the default runner. This fixes a loader defect;
confirmation of the reported MSFS menu failure on the affected system is still
outstanding. Later CachyOS repetitions timed out twice, then passed in a Wine
virtual desktop and again after returning to the host compositor. This
intermittent graphics-test issue remains unexplained. Known Fenix launch scripts
receive the correction through the
component updater, and selecting a Fenix overlay retains the current launcher
scripts instead of reinstalling older archive copies.

- Automatically repair incomplete Microsoft .NET Framework 4.8 setup for
  Fenix/GSX in the staged Windows profile. Verify both CLR architectures by
  starting the managed compiler; registry markers alone are insufficient.
  Complete pending Wine restart operations, handle Wine Mono's advertised
  release values and make a bounded reinstall attempt when MSI repair fails.
- Offer **Repair setup** for interrupted Fenix preparation whose original
  profile, runner and launch scripts remain unchanged. Retain failed copies
  and logs; committing transactions still require recovery.
- Check managed .NET evidence again before Fenix/MSFS startup. Repair an
  incomplete profile in a separate copy, verify it, and retain the previous
  profile when activating the repair. Interrupted repairs remain recoverable.
- Package a verified native runtime helper with the current launch scripts,
  preserving custom component sets and supporting idle launcher handoff.
- Add native transaction, HTTP and browser checks and an isolated
  [Python/Rust performance comparison](performance.md).

In the isolated full-package comparison, native startup takes 72 ms versus
134 ms and service RSS falls from 35.21 MiB to 10.48 MiB. The native package is
3.27 MiB larger. These results do not establish an MSFS frame-rate improvement.

Fresh .NET setup, repair of a deliberately removed x86 CLR (including matching
restored file hash), and an idempotent follow-up passed with the pinned Flightdeck
Wine runner in an isolated, account-free profile. The remote report's exact
failure still requires its setup log; this test does not establish every Proton
build or distribution. This native build is a prerelease; the Python-side
.NET repair and earlier selected-Proton corrections ship in 0.1.22.

## 0.1.22 — 3 October 2026

Prepare existing Python installations for the native launcher. This release
still runs on Python 3.10+ and can be installed by the 0.1.21 in-app updater.

- Recognize native packages, verify their manifest, x86-64 executable and
  SHA256 checksum, and run the incoming native installer with the expected
  current release bound to the update.
- Restart the native service at the existing browser address. Preserve the
  installation manager through rollback to Python and a subsequent return
  to Rust. Terminal updates and installer launches accept the new shell wrapper.
- Discover stable full packages from the release list. Keep GitHub's Latest
  entry on 0.1.22 so older clients receive this prerequisite first; later
  stable native releases remain discoverable by updated clients. Follow the
  [staged release procedure](rust-transition.md).
- Include the Python-side automatic .NET repair and Experimental/CachyOS
  loader corrections described below, with the same live-validation limits.

The real 0.1.21 package, transition package and a local native candidate pass
the isolated in-app update and rollback chain. The native candidate is a local
test build; this release installs the Python transition launcher.

## 0.1.21 — 3 October 2026

- Include **Flightdeck (Xodus, default)** directly in the Proton selector and
  preselect the active environment when available.
- Use the existing return-to-Flightdeck operation when applying that choice,
  preserving the current add-ons and settings. Keep recovery available when
  an interrupted switch or cloud-sync issue blocks other runner choices.

## 0.1.20 — 3 October 2026

- Carry the current Windows profile across Proton changes and back to Flightdeck,
  retaining aircraft, mods, sign-ins and settings instead of restarting each trial
  from the original profile. Retain earlier profiles as backups.
- Add exact-build Fenix overlays for Experimental 11.0 (20260924) and CachyOS 10.0
  sunset. Show compatible versions in the selector and migrate known legacy
  Fenix installations while preserving their current profile.
- Allow GSX/FSDT preparation and launch on the selected Proton. Handle Wine64
  consistently for classic runners and keep recovery journals coordinated.
- Keep Fenix geometry, helper-window and MCDU refresh features in the portable
  launcher. Reject unsupported Fenix runner combinations before changing files.

Native Fenix probes, fresh .NET/Fenix setup, official FSDT preparation and isolated
switch/loader/graphics checks pass for both builds. The launcher suite passes
832 tests, compatibility packaging 39 tests and Chromium 309 UI checks. Full Fenix/GSX flights on alternate Proton versions and the reported NVIDIA
black main view remain unverified.

## 0.1.19 — 3 October 2026

[Download Flightdeck 0.1.19](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.19).

This release combines experimental Proton selection and GSX Pro setup with a
more compact launcher interface. Update through **Updates → Flightdeck** or
the full installer. This is a regular release on the stable update channel.

- Select an installed Proton version under Setup, discovered in Steam libraries
  or chosen by folder. Each trial has its own Wine/DXVK/VKD3D copy and Windows
  profile, with recovery and a return to the original Flightdeck environment.
- Preserve normal Xbox entitlement checks and Store/GDK integration with the
  alternative launch bridge. Account/save helpers keep the original runner.
- Add GSX Pro preparation under Mods for MSFS 2024: prepare the pinned official
  FSDT installer and .NET, install and activate through FSDT, then enable its
  existing Couatl startup entry. Retain a profile backup and support recovery.
- Keep GSX setup and Proton recovery from replacing each other's profiles.
  Return to Flightdeck before changing FSDT setup; recover incomplete GSX setup
  before switching Proton. Internal add-on links stay inside copied profiles.
- Show Fenix and GSX as compact status cards, with one setup workflow expanded
  at a time. NVIDIA, VR, Proton and maintenance settings also open on demand.
  Keyboard access and German/English mobile layouts remain available.
- Update the managed runtime script checksums, including GSX session cleanup,
  and include the selected Proton version in diagnostics.

Proton Experimental 11.0 (20260924) and CachyOS Proton 10.0 sunset passed synthetic
executable/DLL loading, Store interface loading, mixed D3D11/D3D12 rendering and
return to the original profile on AMD. Full MSFS operation with these runners
and a fix for the NVIDIA black view remain unconfirmed. The 0.1.18 follow-up
confirms that corrected renderer build `628afa6f9cfece4` loads but the affected
system's globe/map and cockpit remain black.

FSDT installer preparation, opening and closing were tested under Wine. GSX
package installation, activation, Couatl/SimConnect and ground services still
need validation with a licensed copy. Fenix keeps its matched Flightdeck runner.
No native or renderer binaries change; their existing corresponding sources and
notices are retained. [GSX setup](addons.md#gsx-pro-experimental) ·
[Proton trials](runtime.md#experimental-proton-selection).

## 0.1.18 — 2 October 2026

[Download Flightdeck 0.1.18](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.18).

- Add per-installation VR settings for automatic OpenXR, WiVRn, SteamVR and
  Monado, off by default, with German and English setup instructions.
- Add an explicit, bounded headset/GPU check with clear failure states and
  protection against concurrent game starts, cloud sync and setup.
- Prepare separate Windows and Linux OpenXR manifests and Wine's required VR
  registry data for direct game starts, preserving the Xbox Store bridge.
- Keep DXGI on the compositor's GPU, preserve NVIDIA compatibility settings,
  and retain SteamVR registration across the game's isolated XDG directories.
- Include VR mode/check outcomes in diagnostics without raw driver output or
  identifying runtime paths and device UUIDs.
- Add real D3D11/D3D12 stereo-frame validation through Wine and retain the
  renderer correction from 0.1.17. No native renderer or Store binaries change.

Validated with a simulated Monado headset and AMD RX 6900 XT. Physical headsets,
NVIDIA hardware, WiVRn/SteamVR device operation and an MSFS VR flight remain
unverified. [Setup](vr.md) · [Test evidence and reproduction](vr-validation.md).
This is a regular release, available through the stable update channel.

## 0.1.17 — 2 October 2026

[Download Flightdeck 0.1.17](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.17).

- Backport the upstream VKD3D correction for full-depth 3D-texture copy barriers
  with `VK_KHR_maintenance9`. The previous barrier covered only the first depth
  slice of render-target-capable volume textures.
- Give the rebuilt D3D12 pair a source-derived build ID, pin the changed-source
  hashes, and retain the existing NVIDIA low-latency corrections and DXVK build.
- Add a real upload/copy/readback regression that requires active Vulkan
  validation, reproduces the released renderer's invalid layouts, and checks
  the corrected renderer plus controls with maintenance9 disabled.

The isolated regression passes on AMD. This establishes the volume-layout
correction; its connection to the reported black NVIDIA main view is unverified.
Available as a regular release through **Updates → Flightdeck** or the full installer. Close Flightdeck and MSFS before installing,
then reopen Flightdeck, use **Automatic** NVIDIA graphics and check the main map
and cockpit. The game log should identify renderer build `628afa6f9cfece4`.
No game reinstall or profile reset is needed. The Microsoft sign-in components
and their corresponding sources remain at the verified 0.1.16 versions.
[Implementation and validation](nvidia-renderer.md#3d-texture-layout-correction-0117).

## 0.1.16 — 1 October 2026

[Download Flightdeck 0.1.16](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.16).

- Preserve verification steps embedded in individual replies of a multi-scope
  Microsoft token exchange. A response can require Xbox verification while
  already issuing a Microsoft root token; the login now completes that step
  before storing credentials. The previous decoder rejected this response
  because the verification reply had no issued-token fields.
- Accept Microsoft's shortened SOAP fault headers so an accompanying supported
  verification link reaches the sign-in window instead of failing with code 74.
  Successful token responses still require their normal headers; signature
  verification and account/root-credential checks remain in place.
- Decode encrypted verification steps and token responses using bounded buffers
  and the XML Encryption CBC padding rules. Valid responses larger than 8 KB no
  longer overflow the old fixed buffer; malformed cipher data returns a typed
  failure instead of panicking.
- Replace the broad response-error category with fixed codes 80–93 for request
  construction, response size/encoding/XML, signatures, challenge/body decoding,
  incomplete responses, HTTP errors, root tokens, timeouts and rate limiting.
  The Store recovery report and game updater preserve these codes and their
  localized explanations, without including raw authentication output.
- Rebuild both Xodus components because they share the response parser. Managed
  component updates recognize the published 0.1.15 binaries.

Update through **Updates → Flightdeck** and restart Flightdeck before retrying
sign-in. See [sign-in recovery](store-session-refresh.md#soap-response-correction-0116)
for the diagnostic codes and validation scope. Other reported NVIDIA,
in-game Marketplace and cloud-sync problems still need separate confirmation.

## 0.1.15 — 30 September 2026

[Download Flightdeck 0.1.15](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.15).

- Keep one browser context throughout interactive Microsoft sign-in, preserving
  session cookies when opening a follow-up verification view.
- Ignore queued token callbacks from a previous view after the next step has
  opened. A duplicate callback can no longer repeat the old exchange and end
  the current verification step.
- Distinguish connection errors, invalid Microsoft responses, credential
  rejection, account mismatch and missing supported challenges using fixed,
  localized sign-in codes. Raw authentication responses are never displayed.
- Ship the rebuilt login component and recognize the 0.1.14 native component
  set for automatic updates of managed installations.

Native GTK/WebKit tests use a loopback password → email code → completion flow.
The 0.1.14 baseline loses cookies and fails on duplicate callbacks; the corrected
runtime passes both checks and the combined case under X11 and Wayland. This
does not yet confirm successful Microsoft email-code verification on an
affected account. Update and restart Flightdeck before retrying sign-in.

## 0.1.14 — 30 September 2026

[Download Flightdeck 0.1.14](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.14).

- Preserve a Microsoft follow-up sign-in URL when the token exchange also
  reports a credential fault. Since 0.1.9, this combination could end the
  interactive login before displaying the required next step. Silent renewal
  continues to require sign-in; a challenge does not count as successful login.
- Share the existing Microsoft HTTPS challenge validation between renewal and
  interactive login. Keep credential rejection without a valid challenge as a
  failure, and retain account binding and root-ticket validation.

- Ship rebuilt login and broker components and recognize the 0.1.13 component
  set for automatic updates of managed installations.

A synthetic regression test reproduces the discarded challenge in 0.1.13 and
passes with this correction. Authentication and session-renewal tests and the
native build pass. Confirmation of the reported immediately closing login
window on affected hardware remains pending; NVIDIA rendering is not changed.

## 0.1.13 — 30 September 2026

[Download Flightdeck 0.1.13](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.13).

- Accept Microsoft's fallback product translations in explicit Marketplace
  queries, as already supported for the owned library. Prefer the requested
  language and preserve the selected Store region, pricing and ownership checks.
- Include fixed mapping-failure categories in diagnostic summaries and session
  timelines, without product IDs, account data or raw catalog text. Existing
  reports with only `80004001` cannot identify the rejected product feature.
- Label successful Store diagnostics as **Basic Store checks passed** and
  explain that these checks do not test in-game Marketplace product queries.
- Ship the rebuilt Store runtime and recognize the previous native component
  set for automatic updates of managed installations.

The translation regression is reproduced and tested with synthetic catalog data.
Native catalog, diagnostic-export, browser and package-update checks passed.
This does not yet establish that the reported in-game Marketplace error is fixed
for every affected installation. German Store and game-language settings can be retained.

## 0.1.12 — 29 September 2026

[Download Flightdeck 0.1.12](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.12).

- Separate installed-resource verification from the source-import DLL name
  list and 2 MiB text-file limit. Preserve every checksum, regular-file and
  symlink check, with the existing 128 MiB binary limit.
- Cover a real service update, restart and rollback with a newly introduced
  resource larger than the source limit; retain the same browser origin.
- An already-running 0.1.10 process cannot receive this verifier correction
  before restarting. If it reports that the installed version could not be
  identified after an otherwise successful update, close and reopen Flightdeck
  once through the application menu. Game files and saves are unaffected.
- NVIDIA/Store binaries are unchanged from 0.1.11. This is a launcher update
  correction and does not add a new NVIDIA rendering claim.

## 0.1.11 — 29 September 2026

[Download Flightdeck 0.1.11](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.11).

- NVIDIA Automatic now selects compatibility for both DirectX 11 and 12, also
  for saved automatic preferences. DLSS, Reflex and NVIDIA Frame Generation
  require the explicit **NVIDIA features (experimental)** mode.
- Correct the ignored `dxvk.disableNvLowLatency2` option in the pinned DXVK.
  Ship matched DXGI/D3D11/D3D10 libraries with the existing VKD3D backport,
  checked before installation. Custom renderer files are preserved.
- Retain VKD3D warnings in the default game log and recognize patched renderer
  versions in diagnostic exports.
- Compact repeated report rows without dropping their values, making long
  reports more likely to fit in the email draft. Downloads remain available.
- Mixed D3D11/D3D12 clear, readback and multiwindow presentation passed on
  AMD hardware. This release corrects the incomplete NVIDIA opt-out; resolving
  the black MSFS main view still requires confirmation on NVIDIA hardware.

## 0.1.10 — 29 September 2026

[Download Flightdeck 0.1.10](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.10).

- Fix native component updates being skipped when a runtime has customized
  launch scripts. The launcher could show 0.1.9 while the older Store binaries
  remained installed, causing **Diagnostics → Check Store** to stop at the
  component check and preventing the new session fixes from taking effect.
- Update the recognized native component set independently. Preserve customized
  scripts and their provenance record; retain existing checks for custom or
  modified binaries, busy runtimes and interrupted-update recovery.

Native compatibility components and the NVIDIA renderer remain the same as in
0.1.9. This release fixes delivery of those Store components to affected managed
runtimes; it does not establish a fix for NVIDIA's black main view or every
reported in-game Marketplace/cloud error.

## 0.1.9 — 29 September 2026

[Download Flightdeck 0.1.9](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.9).

- Add local problem reports with a frozen diagnostic snapshot and an email
  draft addressed to `contact@flightdeck-app.com`. Description and diagnostics
  are included as plain text; no attachment, GitHub account or upload service
  is required. Provide complete copy/download fallbacks for webmail and long
  reports, with German and English UI.
- Extend bounded diagnostic coverage with authentication, networking-policy
  and audio error summaries, without including raw logs or account data.
- Preserve the Store account context across a same-account ticket-expiry gap,
  while rejecting expired credentials, account changes and logout. A failing
  regression reproduces the permanent rejection after ticket renewal in 0.1.8.
  See [Store session recovery](store-session-refresh.md) for scope and evidence.
- Renew expiring Microsoft user/device tickets before Store and Xbox requests,
  share concurrent renewal work, and allow enough IPC time for renewal plus the
  Store request. Keep network/keyring failures distinct from sign-in challenges.
- Add **Renew Microsoft sign-in** with same-account protection, cancellation and
  subsequent license/library verification. **Sign in again and sync** retries
  the captured cloud operation only after that verification succeeds.
- Backport three upstream VKD3D fixes for NVIDIA low-latency swapchain ownership
  and lifetime. Apply the matched D3D12 DLL pair to NVIDIA installations using
  the pinned runner; preserve custom renderers and the shared Wine/Fenix runner.
- Make Compatibility mode disable `VK_NV_low_latency2` as well as NVAPI/NGX.
  Disabling NVAPI alone did not disable VKD3D's NVIDIA-specific presentation path.
- Verify the renderer payload against source-release hashes during packaging,
  installation and launch. Complete interrupted copies before starting a game;
  restore managed copies from the runner when the runner changes.
- Add a native multiwindow rendering/readback test, including unused secondary
  windows, resize and destruction. This is not an MSFS or NVIDIA hardware test.
- Document the unresolved NVIDIA black main view, repeated cloud-sync failures
  and blocking in-game Marketplace session error in both languages. Replace
  repeated graphics-log requests with the current status and defined rendering
  checks.

The black main scene reported on RTX 4060/5060 Ti is **not yet confirmed fixed**.
See [renderer build and validation](nvidia-renderer.md). Native Store components are rebuilt as
**0.1.9**; the reported in-game Marketplace dialog and repeated cloud errors
are not yet confirmed resolved.

## 0.1.8 — 28 September 2026

- Give Store inventory, package-update queries and Microsoft account ticket
  requests enough time to receive server responses. These operations previously
  used a five-second local-request timeout, which could close the shared broker
  connection and abort subsequent license queries.
- Keep the broker connection usable after a completed inventory request reports
  a server timeout. A broken or unresponsive transport still closes safely so a
  late reply cannot be mistaken for another request's response.
- Align the nested cloud sign-in waits with the longer broker request deadline.
- Report native cloud timeouts, connection failures and server errors as
  connection problems, including during sign-in and title lookup. Actual rejected
  sign-ins still report authentication errors; numeric diagnostics are retained.

Native components are rebuilt as **0.1.8**. Delayed synthetic broker replies
reproduce the 0.1.7 inventory timeout and following license abort, and verify
the corrected inventory, sign-in, package-update and failure handling. These
checks use the built Wine component without accounts or real cloud data; they
do not establish recovery from every Microsoft outage or a successful live
cloud sync on an affected user's system.

## 0.1.7 — 28 September 2026

[Download Flightdeck 0.1.7](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.7).

- Select the sole discrete NVIDIA GPU by its Vulkan device UUID instead of its
  Linux display name. Wine can rename the adapter, especially in Compatibility
  mode; the previous name filter could exclude the intended GPU. DirectX 12
  retains the adapter selected by DXGI.
- Include graphics startup versions, adapter-filter failures and counts of
  presents without rendering in diagnostics. Hardware UUIDs and raw log lines
  are excluded from the exported report.
- Add **Diagnostics → Check Store** to check installed components, saved sign-in,
  catalog access, game licensing, owned content and local window display without
  opening a purchase page.
- Open Microsoft's Marketplace confirmation page in a dedicated window. Fix
  blank or gray windows and use Microsoft's Xbox layout with a compact frame
  that fits the available space.
- Show loading progress and distinct connection, timeout and expired-session
  errors. Keep failures visible until dismissed, support cancellation and allow
  only one purchase dialog at a time, without automatic retries.
- Improve owned-content queries for bundles, large libraries and fallback Store
  languages. Keep Store-account checks tied to the active sign-in and handle
  unrelated library items without rejecting the current game's content.
- Include timestamped Store events and the component versions used at game
  launch in diagnostics, excluding account data, product identifiers and raw logs.
- Reuse the existing launcher window after an update or rollback, retaining its
  local address and language. Handle update restarts from earlier releases
  without opening a second window.
- Update English and German guides with Store checks, purchase-dialog limits,
  cloud-save behavior and the separate Fenix patch update process.

Native components are rebuilt as **0.1.7**; the optional Fenix patch remains
**0.1.0-preview.3**. The confirmation page and its layout have been checked in
MSFS 2024 1.8.16.0. Automated checks cover dialog behavior, cancellation and
message validation. Completed paid transactions, delivery of purchased content
and layout across other systems remain unverified. Broader Marketplace
inventory compatibility also requires testing.
The adapter-selection regression has been reproduced and corrected with the
real runner on AMD hardware, including vendor hiding and both DirectX 12 device
creation paths. NVIDIA in-game rendering, including the reported RTX 5060 Ti
black scene, still needs hardware validation.

## 0.1.6 — 26 September 2026

[Download Flightdeck 0.1.6](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.6).

- Add **Setup → NVIDIA graphics** with Automatic and Compatibility modes,
  saved separately for each simulator installation and applied on its next
  start. Compatibility disables NVIDIA-specific features, including DLSS and
  NVIDIA Frame Generation, while retaining the physical GPU selection.
- Translate NVIDIA vendor-hiding options for direct Wine/DXGI starts and make
  NVAPI opt-outs effective even after an earlier NVIDIA-enabled launch.
- Complete an unambiguous GPU name filter for both DXGI and DirectX 12 while
  preserving explicit choices and avoiding guesses between multiple GPUs.
- Extend graphics diagnostics with persistent start settings, graphics-library
  checks and recognized error codes. These records describe configuration and
  errors, not successful rendering.
- Add **Setup → Manage installation** with removal previews, retained data by
  default, and a reversible Wine environment reset. Active games and setup
  operations remain protected; pending cloud recovery data is preserved.
- Update the English and German guides with graphics-mode instructions,
  installation maintenance and consistent compatibility requirements.

NVIDIA rendering and flight stability still require hardware validation.
Native components remain **0.1.1** and the optional Fenix patch remains
**0.1.0-preview.3**. See [NVIDIA graphics](graphics.md) and
[installation maintenance](install.md#manage-a-game-installation).

## 0.1.5 — 25 September 2026

[Download Flightdeck 0.1.5](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.5).

- Check for Flightdeck and selected-simulator updates automatically when the
  app opens. Show available updates on the overview; background discovery does
  not reserve the simulator or start downloads/sign-in. Reopening windows is
  throttled for 30 minutes per running service; manual checks remain available.
- Prepare NVIDIA graphics before launcher-managed starts: use NVAPI and optical
  flow from the matching runner, NGX from the installed driver, and Proton's
  corresponding DLL overrides. Existing managed files follow runner/driver
  updates; user-supplied DLLs and explicit GPU selections are retained.
- On NVIDIA plus integrated-graphics systems, select the sole discrete NVIDIA
  adapter consistently for DXGI and DirectX 12. Report unavailable NVIDIA Vulkan
  before launch. AMD/Intel-only launch environments are unchanged.
- Recover from a failed post-exit cloud upload with explicit local play. Keep
  backups and the pending transaction; require a version choice if later local
  progress differs from the interrupted upload's target.
- Include bounded GPU/Vulkan information and numeric cloud error details in
  diagnostics; distinguish connection, storage-lock and quota failures.

NVIDIA rendering is not yet verified across GPU and driver versions. Native
components remain at **0.1.1** and the optional Fenix patch remains
**0.1.0-preview.3**.

## 0.1.4 — 24 September 2026

- Update Flightdeck directly under **Updates → Flightdeck**: check stable GitHub
  releases, read release notes, download and install, then restart when ready.
  Downloads are checked against GitHub's SHA256 digest; the previous launcher
  version remains available for restoration. Installation requires an idle game.
- Brief notifications close automatically after six seconds (errors: twelve),
  pause while hovered or focused, and include a close button. Errors inside
  update and setup panels remain visible.
- Integrate Fenix patch **0.1.0-preview.3**: path metrics restore missing routes;
  correct stroke contours remove excessive lines at acute route joins.
- Install and verify the separately downloaded Microsoft geometry dependency
  in fresh profiles and updates from preview.1/preview.2.
- Keep matching Fenix helper windows off the X11/Xwayland desktop while
  preserving their internal visibility; retain the fallback window guard.
- Retain the aircraft, user settings and original restore point during updates.

- Automatically refresh stale MCDU images after Fenix Display restarts, preserving
  pages and restoring the original display preference and brightness.

Native components **0.1.1** are unchanged. The official Fenix display-restart
check passed with automatic MCDU recovery. This remains a community Fenix preview;
a full flight and other aircraft/simulator versions have not been verified.

## 0.1.3 — 24 September 2026

[Download Flightdeck 0.1.3](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.3).

- Repair missing UI-font registrations before opening Fenix applications,
  fixing installer startup crashes in affected Wine profiles.
- Restore visible arrow, text and link cursors in the official Fenix Installer
  and livery manager.
- Update supported preview.1 patch installations before opening Fenix, retaining
  aircraft, settings and the original restore point.
- One **Open Fenix** action opens the main application. The separate
  **Installer & Liveries** action opens the official installation/livery manager.
- **Open Fenix** is available for existing local Fenix installations.
- Closing Fenix also cleans up its WebView helpers, including after a manager
  crash, so a later start is not blocked by leftover processes.
- **Stop Fenix** stays visible and shows when Fenix has stopped.
- Opening Fenix no longer creates an extra blank Wine notification-area window.

Native components **0.1.1** are unchanged. Fenix patch **0.1.0-preview.2** adds
the cursor fix. Install the new full package to update Flightdeck; its Fenix panel
downloads and verifies the matching patch when needed.

## 0.1.2 — 23 September 2026

[Download Flightdeck 0.1.2](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.2).

- **Mods → Fenix A320 → Stop Fenix** closes the main Fenix application, its
  helpers and the official manager in the selected Wine profile. It first
  requests a normal close and uses a bounded fallback for stuck processes.
- The button works while Flightdeck's **Open Fenix** or livery-manager job is
  active. Setup and game launch stay reserved until shutdown completes. A running
  simulator or installer disables this action; other profiles and applications
  are excluded.

Validation includes process tests for graceful close, stuck helpers, profile
isolation and a running simulator; runtime-reservation tests; and German/English
desktop/mobile browser flows. Native components and patch **0.1.0-preview.1**
are unchanged. Install the new full package to update the launcher.

## 0.1.1 — 23 September 2026

[Download Flightdeck 0.1.1](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.1).
Run the installer from the new full package to update an existing launcher.
Game files, profiles, settings and saves remain in their existing locations.

### Fenix A320 in MSFS 2024

- **Mods → Fenix A320** downloads a fixed, checksummed release from the separate
  [Fenix patch project](https://github.com/marselnenaj/fenix-a320-linux-patch).
  Patch **0.1.0-preview.1** is released independently of Flightdeck's version.
- The four-step flow prepares a separate Wine runner/profile and .NET Framework
  4.8, starts the user's official Fenix installer, opens normal sign-in and
  applies CPU displays, Legacy readouts and autostart.
- Completed steps, the next action and a final ready message make setup progress
  explicit. An open Fenix/Windows application explains why setup is waiting.
  A keyboard hint covers entering `@` on German layouts in the Fenix login.
- After setup, Fenix starts with MSFS. Flightdeck closes its session companions
  when the game exits, crashes or is stopped, with a bounded fallback for stuck
  helpers. Other Wine profiles and the official installer remain independent.
- Wine fixes address cockpit rendering, FCU/radio text and Fenix helper windows.
  Setup retains the original profile and supports recovery after interruption.
  Existing local development patches are detected and kept in use.
- The installed official Fenix manager can be opened for liveries. Packages
  must match the purchased aircraft, engine, wing variant and simulator.

[Setup, liveries and restore](addons.md#fenix-a320) ·
[Deutsche Anleitung](addons.de.md#fenix-a320-einrichten)

### Separate MSFS editions

- Select **MSFS 2024** or **MSFS 2020** in the overview and installation form.
  Each edition has its own runtime, Wine profile, game package, Community
  location, update history and local saves.
- Package checks, game updates, integrity checks, repair, rollback and cloud
  helpers use the selected edition's identity. Existing runtimes without an
  explicit edition remain MSFS 2024.
- The overview changes its aircraft artwork with the selected simulator and
  preserves layout during switches, including reduced-motion settings.

### Launcher and compatibility updates

- A newer full Flightdeck package can refresh recognized managed native
  components and launch scripts when the runtime is idle. Interrupted component
  updates retain recovery information; custom components are preserved.
- Launcher updates use the installer included in the new package. An idle
  background service can pick up the updated launcher on the next open.
- Microsoft sign-in has its own GTK/WebKitGTK window. Store region selection,
  download size guidance and setup messages are clarified.

[Updating Flightdeck and its runtime](install.md#update-and-rollback)

### Marketplace compatibility

- Enumerate supported direct and satisfying account entitlements for the
  current game, including legacy MSFS 2020 application IDs.
- Acquire supported Durable license handles using verified Microsoft-signed
  grants. Handle expiry, query lifetime and release explicitly.
- Query the single registered base-game package's update state. Improve
  supported consumable balances, catalog images and search terms, and retry
  expired signed receipts within the original bounded request.

Paid checkout, package installation through the game's Store APIs and
device-shared DLC rights remain unsupported. These changes do not establish
that the MSFS 2024 Aviator Upgrade works. See the
[Marketplace scope and evidence](marketplace-collections.md).

### Validation and remaining limits

Fenix 2.4.0.4720 / MSFS 2024 1.8.16.0 cockpit displays were checked on Hyprland.
A complete Fenix flight remains unverified; CPU displays do not provide weather
radar. The first patch requires the pinned Xodus runner. MSFS 2020 can display a
startup disc prompt, and startup/full-flight compatibility remains unconfirmed.
Cloud saves remain experimental, with cross-device gameplay validation pending.

## 0.1.0 — 18 September 2026

[Initial experimental release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.0):
MSFS 2024 installation and launch, download pause/resume within the running
session, base-game update/repair/rollback, Community inventory, local backups,
experimental automatic Xbox cloud saves and a German/English interface.
