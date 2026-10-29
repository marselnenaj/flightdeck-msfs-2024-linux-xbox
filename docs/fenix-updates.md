# Fenix patch update contract

## Flightdeck 0.3.5: native-only CLR loader override

[Issue #10](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/10#issuecomment-6500878026)
reports an install hook failing on an existing `System.Runtime.dll` with
`System.IO.FileNotFoundException`. A native-only `mscoree` override can prevent
Wine from loading an IL-only assembly when the prefix contains Wine's builtin
loader. Checking that the assembly or Framework CLR files exist does not detect
this loader failure.

The correction writes `native,builtin` during Framework preparation,
so an installed native CLR loader retains priority. Before Fenix installer/app
preparation, existing global `native`/`n` values migrate to that fallback policy
through Wine's registry commands. The previous value is retained in the prefix's
private `.flightdeck-fenix-environment.json` backup. Existing DLL files, per-app
settings and disabled/builtin preferences are preserved. Xbox preparation runs
before the desktop-runtime helper, matching the Steam preparation order.

An account-free test with the default Xodus Wine build and an unchanged .NET
8.0.31 `System.Runtime.dll` reproduces error 126 with the native-only policy.
The actual preparation routine fixes the persisted registry value; the same
assembly then loads successfully. A second call creates no additional backup
entry. `tests/windows/managed-load.c` is the small loader probe used for this
comparison. It does not execute a Fenix hook or start a simulator/account.
The reporter's .NET 8.0.33 and full Fenix installation still need validation.
This correction first ships in 0.3.5.

## Published update descriptor

Flightdeck 0.2.20 and later discover releases from the fixed public
`marselnenaj/fenix-a320-linux-patch` GitHub repository. The bounded release feed
must advertise one uploaded `flightdeck-update.json` asset with a GitHub SHA-256
digest. Older releases without that asset are handled through Flightdeck's
embedded fallback. No caller-provided download URL or downloaded installer
script enters the update path.

The Fenix release tool writes this descriptor alongside its ZIP and source
archive:

```json
{
  "format": 1,
  "installer_contract": 2,
  "minimum_flightdeck_version": "0.2.22",
  "release": {
    "version": "0.1.0",
    "url": "https://github.com/marselnenaj/fenix-a320-linux-patch/releases/download/v0.1.0/fenix-a320-linux-patch-0.1.0-linux-x86_64.zip",
    "archive_root": "fenix-a320-linux-patch-0.1.0",
    "sha256": "<archive SHA-256>",
    "size": 286218485
  },
  "bundle": "<the complete bundle.json object>"
}
```

The placeholder strings illustrate the fields; the actual asset contains the
complete manifest and a lowercase hexadecimal digest. Attach this file to the
same published release as the immutable archive. It may be added to an existing
release when generated from that release's exact manifest and unchanged ZIP;
do not regenerate or replace the existing archives.

Contract 2 introduces the native `fenix-runtime` helper and requires Flightdeck
0.2.22 or later. Older launchers must update first. Contract 1 describes the
previous helper layout. Compatible updates may replace hashes for the embedded
destinations; a contract number alone never permits arbitrary new paths. Each update
requires the embedded variant set, original runner identities, server protocol,
renderer policy and geometry dependency to remain compatible. Existing upgrade
snapshots must be preserved, including the preceding embedded patch. Changed
paths, runner builds or setup behavior require a Flightdeck integration update
and, where needed, a new contract/minimum version. Publishing an unsupported
descriptor produces an explicit launcher-update requirement.

Flightdeck 0.2.23 also checks existing local setups automatically. A known local
runner and known launch scripts can be adopted into a managed copy before launch
or through the explicit adoption action. The original profile, runner link and
scripts are backed up; renderer and FCU preferences are retained. Unknown
customizations are not overwritten. An explicit restoration pauses automatic
adoption until the user explicitly adopts the setup again. Available and installed versions are
separate UI facts; an embedded release alone is not installation evidence.

Checks run without taking the runtime lease. Installation takes the existing
launcher/runtime lease, confirms the Wine prefix is idle, verifies archive size,
SHA-256 and every allowlisted payload, then uses the existing staged profile and
runner transaction. The installed marker retains the exact manifest and release
coordinates for offline verification, reapplication and Proton switching. No
vendor aircraft, credentials or scripts are fetched by this contract.

Tests cover preview/stable ordering, foreign URLs, duplicate releases, altered
metadata, incompatible contracts, isolated archive extraction, cancellation,
retained versions after restart and all four original runners. The explicitly
enabled public metadata test uses no existing Wine profile:

```sh
cargo test --locked --test native_fenix_updates public_stable_descriptor -- --ignored
```
