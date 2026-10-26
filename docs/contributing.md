# Working on Flightdeck

Flightdeck is an independent compatibility experiment. Keep changes small enough
to review and describe the game behavior they enable. A successful build or a
mocked request is not evidence that a feature works in the simulator.

## Code map

| Location | Responsibility |
| --- | --- |
| `native/` | Rust backend, CLI, desktop service, installer and runtime helpers |
| `native/ui/` | Native Rust/iced desktop, using the authenticated local API |
| `native/games.rs`, `native/components.rs` | Simulator identities and checked runtime updates |
| `native/fenix.rs`, `native/framework_maintenance.rs`, `native/gsx.rs` | Add-on setup, automatic .NET repair and recovery |
| `native/cloud_*.rs` | Cloud protocol, comparison, transactions and automatic sync |
| `tests/fixtures/legacy-python/` | Frozen synthetic pre-migration contracts, with source provenance |
| `examples/runtime-lab.rs` | Rust driver for isolated hardware and C++ save interchange probes |
| `ui/` | Shared artwork, icons and fonts for the native desktop |
| `compat/runtime/` | Native Wine/GDK bridge, asynchronous results and local saves |
| `compat/patches/` | Changes against the pinned WineGDK and Xodus sources |
| `compat/upstreams.lock.json` | Upstream revisions, checksums and license identification |
| `compat/fenix/` | Fenix payload manifest, release URL/checksum and installer license |
| `scripts/` | Maintainer source builds, test harnesses and release packaging |
| `tests/` | Synthetic launcher, packaging and native regression checks |

The launcher has no responsibility for authenticating with Microsoft or granting
game entitlements. Those operations belong to the native bridge and the Xodus
broker. Store collection results must remain bound to the selected Store account;
an unavailable response must not be converted into an empty successful query.

## Development loop

1. Reproduce the failing behavior. Use synthetic data for tests and record the
   relevant numeric status or API result rather than a full authenticated log.
2. Change the smallest component responsible for the behavior. Preserve license
   notices and use public documentation or freely licensed interface sources.
3. Run the affected checks from [BUILDING.md](../BUILDING.md). For desktop
   changes, run `cargo test --locked --workspace`, the native software-rendered
   screenshot suite and the real-service UI client test. Inspect both languages,
   the minimum window size, both simulator editions and confirmation states.
   Native UI tests include automatic discovery, legacy add-on state and historical synthetic fixtures.
4. When changing a compatibility patch, stage it from the pinned upstream commit
   into a new directory. Update the patch checksum and source-delta hashes, and
   build that staged source rather than an unrelated development worktree.
5. Record what was tested: synthetic unit checks, a real read-only service request,
   or the running simulator. Identify the game/runtime versions for integration
   observations and any behavior that remains untested.
6. Update [the change log](changelog.md), both README languages and the affected
   guides. Keep upcoming features separate from what the published installer
   actually includes; distinguish reported game behavior from isolated probes.

Public guides should describe user-visible behavior, requirements and known
limitations. Keep personal support conversations, local debugging history,
account observations, workstation paths and temporary test artifacts out of
README files and release notes. Technical references may document stable
interfaces and reproducible checks. Keep German and English instructions
consistent, and verify relative links after editing. Do not describe a GPU or
driver as working solely because configuration or unit tests pass; rendering
claims require a test on the relevant hardware.

New compatibility operations must define cancellation, result ownership and
error behavior. Keep personal inventory separate from the public catalog, and
keep Store consumable quantities separate from a game's own currency wallet.
Back up local saves before testing a changed save provider. Replace runtime
components only while the launcher lock is held and the simulator is idle.

## Review and release

Commit source and synthetic fixtures. User prefixes, saves, game packages,
account identifiers, authentication responses and raw game logs stay in local
private directories. Use Flightdeck's diagnostic export for a report that omits
those details; screenshots still need a separate check for personal information.

Run `cargo run --locked -p flightdeck-tools -- source-check` before committing. Source archives
use the same explicit allowlist and record every included file's hash:

```sh
cargo run --locked -p flightdeck-tools -- source-release --output build/flightdeck-source.tar.gz
```

Build and package the Rust launcher with the `build-native` and `package`
commands of the same tool as described in [BUILDING.md](../BUILDING.md#native-packages).
The full package combines the native launcher with the component archive
matching `compat/bootstrap.lock.json` and the [pinned renderer bundle](nvidia-renderer.md).
These commands create local archives. Binary distributions also require the
matching corresponding sources and original notices described in
[binary package provenance](binary-release.md). Preserve the component licenses
listed in [the provenance notes](../compat/THIRD_PARTY_NOTICES.md).

### Fenix patch releases

The separate [Fenix patch repository](https://github.com/marselnenaj/fenix-a320-linux-patch)
owns the Wine overlay and standalone installer. Flightdeck implements Xbox transactions in Rust and uses the pinned shared Rust
engine for Steam installations. The maintainer tool's `probe sync-fenix` command
imports only the payload and release manifests and license. Review protocol
changes against the Rust implementation; no Python engine is copied into
Flightdeck.

1. Build and test the patch with its documented upstream sources. Publish its
   Linux installer ZIP, complete corresponding sources and `SHA256SUMS` as a
   public GitHub release, including `flightdeck-update.json` from the release
   tool. Keep already pinned release assets immutable.
2. Run `cargo run --locked -p flightdeck-tools -- probe sync-fenix
   /absolute/path/to/patch-project` and review the Rust integration, license and
   both imported manifests. The import command copies files; it does not
   independently validate a published GitHub asset.
3. Verify the release can be downloaded without GitHub authentication and its
   SHA-256 matches `compat/fenix/release.json`. Run the native add-on/framework/Proton tests,
   installer checks and the native UI interaction tests.
4. Update the English/German add-on guides and changelog, then include the
   integration in the next Flightdeck source/full package as its verified
   fallback. Flightdeck 0.2.20+ independently adopts compatible published patch
   releases; a launcher release is needed when the installer contract, runner
   identities, supported paths or setup policy change. See the
   [update contract](fenix-updates.md).

For a release-payload regression check, supply the current public ZIP, the
preceding public ZIP and a JSON object mapping `xodus` and every manifest
variant to its original runner directory:

```sh
FLIGHTDECK_TEST_FENIX_ARCHIVE=/path/to/current-linux.zip \
FLIGHTDECK_TEST_FENIX_PREVIOUS_ARCHIVE=/path/to/previous-linux.zip \
FLIGHTDECK_TEST_FENIX_RUNNERS=/path/to/original-runners.json \
cargo test --locked --test native_fenix_release -- --ignored --nocapture
```

This explicitly enabled test uses the production archive reader and overlay
code with isolated runner files. It checks the preceding module hashes,
replacement of prefix DLL links and preservation of original runners. It
starts no Wine process and does not qualify the complete installation
transaction, the official application or simulator stability.

The native launcher implements the installer contract in Rust. The installer
extracts only fixed Wine/window-helper payloads named in the manifest and
deploys current launcher-owned scripts; downloaded Python code is not imported.
Fenix aircraft, Microsoft prerequisites, fonts,
accounts and Wine profiles must not enter either project's release archives.
