# Working on Flightdeck

Flightdeck is an independent compatibility experiment. Keep changes small enough
to review and describe the game behavior they enable. A successful build or a
mocked request is not evidence that a feature works in the simulator.

## Code map

| Location | Responsibility |
| --- | --- |
| `flightdeck/` | Local HTTP API, runtime checks, process ownership and save backups |
| `flightdeck/games.py` | Fixed simulator identities and edition-bound runtime paths |
| `flightdeck/runtime_components.py` | Checked native/script updates for idle managed runtimes |
| `flightdeck/fenix.py`, `flightdeck/_fenix/` | Fenix setup jobs and the vendored patch installer engine |
| `ui/` | Browser interface, using the local API |
| `compat/runtime/` | Native Wine/GDK bridge, asynchronous results and local saves |
| `compat/patches/` | Changes against the pinned WineGDK and Xodus sources |
| `compat/upstreams.lock.json` | Upstream revisions, checksums and license identification |
| `compat/fenix/` | Fenix payload manifest, release URL/checksum and installer license |
| `scripts/` | Isolated source builds, runtime staging and source releases |
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
3. Run the affected checks from [BUILDING.md](../BUILDING.md). For browser
   changes, run `node --test ui/tests/*.test.mjs` and
   `node ui/tests/browser-test.mjs` (Chromium required), then inspect both
   languages and narrow layouts using synthetic data.
4. When changing a compatibility patch, stage it from the pinned upstream commit
   into a new directory. Update the patch checksum and source-delta hashes, and
   build that staged source rather than an unrelated development worktree.
5. Record what was tested: synthetic unit checks, a real read-only service request,
   or the running simulator. Identify the game/runtime versions for integration
   observations and any behavior that remains untested.
6. Update [the change log](changelog.md), both README languages and the affected
   guides. Keep upcoming features separate from what the published installer
   actually includes; distinguish reported game behavior from isolated probes.

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

Run `python3 scripts/check-source-export.py` before committing. Source archives
use the same explicit allowlist and record every included file's hash:

```sh
python3 scripts/source-release.py --output build/flightdeck-source.tar.gz
```

To assemble an installer, combine that archive with the native component package
matching `compat/bootstrap.lock.json`:

```sh
python3 scripts/full-installer-release.py \
  --source build/flightdeck-source.tar.gz \
  --native build/flightdeck-compat-0.1.1-linux-x86_64.tar.gz \
  --output build/Flightdeck-Linux-x86_64.tar.gz
```

These commands create local archives. Binary distributions also require the
matching corresponding sources and original notices described in
[binary package provenance](binary-release.md). Preserve the component licenses
listed in [the provenance notes](../compat/THIRD_PARTY_NOTICES.md).

### Fenix patch releases

The separate [Fenix patch repository](https://github.com/marselnenaj/fenix-a320-linux-patch)
is the canonical source for the installer engine and Wine overlay. Change and
validate the engine there, then use `scripts/sync-fenix.py` to import it into
Flightdeck; do not let the two engine copies drift.

1. Build and test the patch with its documented upstream sources. Publish its
   Linux installer ZIP, complete corresponding sources and `SHA256SUMS` as a
   public GitHub release. Keep already pinned release assets immutable.
2. Run `python3 scripts/sync-fenix.py /absolute/path/to/patch-project` and review
   the engine, licenses and both imported manifests. The import command copies
   files; it does not independently validate a published GitHub asset.
3. Verify the release can be downloaded without GitHub authentication and its
   SHA-256 matches `compat/fenix/release.json`. Run the Fenix Python/UI tests,
   source-installer checks and the existing browser flow tests.
4. Update the English/German add-on guides and changelog, then include the
   integration in the next Flightdeck source/full package. Publishing the patch
   alone does not update an already installed Flightdeck launcher.

Flightdeck executes its reviewed, vendored Python engine. It extracts only the
fixed Wine/window-helper/launch-script payload named in the manifest; downloaded
Python code is not imported. Fenix aircraft, Microsoft prerequisites, fonts,
accounts and Wine profiles must not enter either project's release archives.
