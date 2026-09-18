# Working on Flightdeck

Flightdeck is an independent compatibility experiment. Keep changes small enough
to review and describe the game behavior they enable. A successful build or a
mocked request is not evidence that a feature works in the simulator.

## Code map

| Location | Responsibility |
| --- | --- |
| `flightdeck/` | Local HTTP API, runtime checks, process ownership and save backups |
| `ui/` | Browser interface, using the local API |
| `compat/runtime/` | Native Wine/GDK bridge, asynchronous results and local saves |
| `compat/patches/` | Changes against the pinned WineGDK and Xodus sources |
| `compat/upstreams.lock.json` | Upstream revisions, checksums and license identification |
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
  --native build/flightdeck-compat-0.1.0-linux-x86_64.tar.gz \
  --output build/Flightdeck-Linux-x86_64.tar.gz
```

These commands create local archives. Binary distributions also require the
matching corresponding sources and original notices described in
[binary package provenance](binary-release.md). Preserve the component licenses
listed in [the provenance notes](../compat/THIRD_PARTY_NOTICES.md).
