# MSFS base-game updates

Select **MSFS 2024** or **MSFS 2020** in the overview before opening **Updates**.
Checks, repair and rollback apply to that edition's runtime and base-game
package. Switching editions clears the previous online check. Edition selection
is available in [Flightdeck 0.1.1](changelog.md).

The launcher can explicitly check the installed Microsoft Store / Xbox PC base
game against current authenticated Store package metadata. It reads the real
`MicrosoftGame.Config` identity, Store ID and four-part game version. Opening
the Updates page only reads local state; **Check for updates** makes the online
request. If sign-in has expired, a separate sign-in action is offered.

This covers the base package and executable version. Additional streamed game
content and Marketplace content continue to use the MSFS library. Community
add-ons and their installers have their own update mechanisms. No mandatory
update or add-on compatibility claim is inferred from a version comparison.
Flightdeck's own launcher/component updates are described in
[Update and rollback](install.md#update-and-rollback); Fenix uses its separate
[setup and official installer](addons.md#fenix-a320).

## Applying an update

The check does not prevent playing the existing game. Starting an update
requires the game to be stopped and acquires the same runtime lock as the
launcher. That lock remains held during download, pause and publication.

An update downloads the **full new package**, not a binary delta. It uses a new
private directory and fresh download journal. The downloader is bound to the
exact package revision selected by the check; a changed revision requires a
new check and cannot reuse the earlier revision's partial files. Signed CDN
URL rotation does not change the package identity.

The current package remains active until the new download completes and its
Store ID, application name, publisher and version pass validation. Linux
`renameat2(RENAME_EXCHANGE)` then exchanges the active directory entry in one
operation. The previous package is retained. **Restore previous version**
performs an explicit reverse exchange with the game stopped and clears the
stale online check. A new online check is required afterward.

The Wine prefix, configuration and saves remain in place. Community/user
packages must be outside the base-game package; if a configured package folder
is inside it, Flightdeck asks the user to move it through the game's settings
before updating. Flightdeck does not guess which user files should be copied.

Allow space for the full new package while keeping the old version. The
preflight estimate reserves twice the new package's download size plus 1 GiB
for extraction and metadata; actual requirements may differ. Failed or
cancelled downloads remain in private storage, and no automatic cleanup of
previous versions is performed.

Pause/resume has the same limits as [installation downloads](download-pause.md):
completed files are hash-verified, up to four interrupted files restart, and
the Flightdeck background-service session must remain running. Automatic job
recovery after restarting the service or computer is not implemented.
Downloads show received MB/GB and percentage when the content total is known;
pausing keeps the last values visible. These counters cover the base package,
not additional content downloaded inside MSFS. The same display is used for
full repairs. [Download size and storage](install.md#download-size-and-storage)

## Component and API contract

Only a hash-pinned CLI with both `streaming-resume-files-v1` and
`package-info-json-v1` is used. Older component bundles report updates as
unavailable instead of treating an unsupported command as success.

`package-info STORE_ID --market CC` returns a bounded JSON object containing
only schema, Store ID, content ID, version ID, four-part version, package
identity hash and package size. Credentials, license keys and CDN URLs are
excluded. Exit 77 requests explicit sign-in. Streaming uses
`--resume-files --expect-package IDENTITY_HASH`; exit 78 means the package
revision changed before download and must be checked again.

The HTTP interface uses `GET /api/game-update` for local state and explicit
authenticated `POST /api/game-update/check`, `/start` and `/rollback` actions.
Checks and downloads use the shared setup job with `mode: update`. Pause,
resume and cancellation retain the existing setup endpoints and job IDs.
Completed checks release their runtime reservation; start revalidates and
reserves the currently selected runtime. Public diagnostics do not include
private update paths or account data.

Validation uses synthetic child processes and package fixtures, actual Linux
atomic exchanges, cancellation, revision mismatch and rollback regressions.
It does not establish that a newly released live MSFS version has been
downloaded or tested in gameplay.

## Verify files and repair

**Verify game files** reads the complete verification index sealed by a
successful Flightdeck download. It compares original SHA-256 checksums and
lengths against the installed bytes, including encrypted executables and the
read-only `.xodus-streaming.msixvc` metadata cache needed at launch. It does not
log in, download anything or change the installed files. The check can be
cancelled and requires the game to be stopped.

The result reports total/checked files and separate missing, changed and
unreadable counts. `checked` includes all attempted files, including errors;
the three error counts are disjoint subsets. Only a completed check can report
healthy. The result describes that check, not continuous monitoring. Extra
files not in the base-package index are not removed or treated as game files.
The index detects local damage; it does not protect against malicious changes
by the same account to both the game and its locally owned verification data.

Legacy imports without a complete original download index cannot be verified
reliably. Flightdeck explicitly reports this limitation instead of hashing
existing files and assuming they are correct. **Prepare full repair** checks
fresh authenticated Store metadata; **Download and repair** then explicitly
downloads the full available base package, even when its version is unchanged.
A newer Store version may be selected and is shown before confirmation.
This is not a damaged-file-only download. The existing package remains active
until validation and atomic exchange succeed; it is retained for rollback.
Prefix, separate user packages, Community add-ons and saves are not repaired or
replaced by this operation.

New successful downloads preserve their original package identity alongside
the index so damage to `MicrosoftGame.Config` can be repaired too. An older
installation with neither a readable identity nor this original record needs
a fresh installation; Flightdeck will not guess its Store identity. Repair
requires the hash-pinned `streaming-integrity-index-v1` component capability.

The additive HTTP actions are `POST /api/game-update/verify {}` and
`POST /api/game-update/repair/check {}` (or explicit `{ "sign_in": true }`).
Repair uses the same checked-plan `/start` action. Shared jobs include
`operation: update | repair | verify`; verification uses `integrity_check` and
publishes aggregate `integrity_result` only after completion. Downloads retain
the existing session-bound pause/resume and cancellation contract.

For older imported runtimes without `private/runtime.json`, the update check
can read a single explicit literal `export XODUS_STORE_MARKET=CC` from the
existing launch script. It does not execute the script, expand variables,
write migration data or override an existing invalid JSON setting.

The authenticated package service also uses a build-qualified revision of the
form `major.minor.build.revision.GUID`. Flightdeck compares only the validated
four-part executable version while retaining the complete original revision
in `version_id` and the download identity. The compound form must agree across
the package and selected MSIXVC revision fields. An explicitly empty optional
file hash is accepted only for that strongly identified compound form; changed
build IDs, version or size still invalidate the checked download revision.
