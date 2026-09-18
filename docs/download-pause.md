# Pausing a game download

During an installation, **Pause download** stops Flightdeck's own Xodus streaming
process. **Resume download** starts it again in the same private installation
folder. The launcher stays reserved for that installation while paused.

Keep the Flightdeck background service running. Reloading the launcher page or
changing its language is supported; restoring an installation job after the
background service or computer restarts is not yet supported. Cancelling keeps
the private downloaded files, but does not turn that cancelled job into a
resumable installation.

A resumed download checks every completed file against its stored SHA-256 digest
and exact output length. It downloads incomplete or changed files again. Up to
four files can be in progress at once, so pausing can discard part of each of
those four transfers. In the tested MSFS base package, the largest individual
file was approximately 1.19 GB.

Flightdeck shows received MB/GB, the full package's content size and percentage
when the installed components support numeric progress. The total includes files
retained and verified on resume. Active file transfers contribute to the display,
but 100% waits for their verification and completion. Metadata validation and
installation can continue after the content download reaches 100%.
This total covers the Store base-game files. Wine/Proton, caches and additional
content downloaded inside MSFS need extra space; see
[download size and storage](install.md#download-size-and-storage).

These counters describe game-file content, including required encrypted-file
padding, rather than HTTP traffic or an MSIXVC container's apparent disk size.
Retries do not inflate the total. Because an unfinished file restarts on resume,
its partial contribution can decrease when the new transfer begins. Paused
downloads retain the last received counters.

Component downloads also show received bytes and percentage when the server
provides a valid total. An unknown total shows the received amount without a
percentage. No progress is inferred from Xodus' terminal output.

Each invocation obtains new package access and a content license through Xodus'
existing authenticated operations. Expired device tickets use the existing
device authentication path. If the user's sign-in has expired, Microsoft sign-in
opens again; completed files remain. Flightdeck permits one such sign-in attempt
per resume, then reports failure instead of retrying indefinitely. Revoked
ownership, unavailable services or an updated package can still require a new
installation attempt. A changed package identity is rejected rather than mixing
files from two package versions.

## Component contract

The pause controls require a hash-verified CLI whose trusted component lock
contains `native.cli_features: ["streaming-resume-files-v1"]`. Older component
packages continue to install normally without pause controls. The API never
accepts capabilities or executable hashes from an installation request.

Numeric game progress additionally requires `streaming-progress-v1`. The launcher
passes a dedicated writable FIFO with `--progress-fd` alongside `--resume-files`.
Bounded JSON lines contain exactly `format: 1`, `received_bytes`, `verified_bytes`,
`total_bytes`, `completed_files` and `total_files`. No paths, names, URLs or account
data are accepted. Stdout and stderr remain discarded. The optional channel is
nonblocking and cannot mark an installation successful; the child exit status,
sealed download index and runtime validation still decide that. Older components
continue with an indeterminate progress bar.

The CLI option is `streaming --resume-files`. Its private `.xodus-resume`
directory contains an exclusive lock, a package-metadata fingerprint and
individual atomic completed-file receipts. Receipts contain only relative file
names, lengths and hashes; they contain no credentials, license keys or signed
URLs. Files and receipts are synchronized before publication. Descriptor-relative
operations reject symlinks, hardlinked file targets, nonregular files and paths
outside the download root.

The fingerprint also includes authenticated package and file version IDs,
package version, size and file hash. Rotating signed URLs and key blobs are
excluded. Direct URL or local-file sources do not support `--resume-files`
because they do not provide that authenticated revision identity.

When available, XVD data hashes are checked against each complete raw 4096-byte
page before decryption. Files without such metadata receive a local completed-file
hash. This is an integrity check for reuse, not a new package-signature or
ownership claim. Encrypted-on-disk executables retain their full padded pages.
The raw-page hash behavior is corroborated by the upstream
[XVD data hash implementation](https://github.com/emoose/xvdtool/blob/master/LibXboxOne/XVD/XVDFile.cs#L803).

The JSON API adds `job.can_pause`, `job.can_resume` and `job.failure_phase`.
`POST /api/setup/pause` and `POST /api/setup/resume` accept the current `job_id`
and require the existing CSRF token. `state` stays `installing`; `phase` changes
from `download` to `pausing`, then to `paused` only after the child has exited.
Resuming returns to `download`, or `authentication` if sign-in is needed.
Translations do not change these machine-readable values.

## Verification

No Microsoft login or game download is needed for the automated regression tests:

```sh
python3 -m unittest tests.test_game_install tests.test_setup tests.test_i18n tests.test_server
cargo test --locked --manifest-path build/compat/xodus-src/Cargo.toml -p msixvc
cargo test --locked --manifest-path build/compat/xodus-src/Cargo.toml -p xodus-cli
cargo test --locked --manifest-path build/compat/xodus-src/Cargo.toml -p xodus resume_expiry_tests
```

These exercise real synthetic child termination/restart, cancel while paused,
credential-expiry handling, response localization, byte checks, interrupted
commits and hostile file paths. Rust HTTP-reader tests use a local synthetic
server. They do not establish uninterrupted Microsoft service availability.
