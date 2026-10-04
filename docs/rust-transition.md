# Python-to-Rust transition update

**0.1.22 is the Python transition release.** It lets existing managed
installations receive the Rust launcher through their current update interface:

```text
0.1.21 → 0.1.22 → 0.2.1
           Python       Rust
```

Each step uses **Updates → Flightdeck → Download & install**, followed by
**Restart Flightdeck now**. The bridge preserves settings and game files,
verifies the native package before executing its installer, and starts the
native service at the existing browser address. The native release **0.2.1**
is offered by 0.1.22 after restarting and checking for updates. The earlier
`0.2.0-dev.1` preview is excluded because stable clients reject both prerelease
flags and prerelease version tags. The 0.2.1 tag, package manifest and executable
all carry the matching stable version. Direct installation remains available.

The briefly published 0.2.0 was withdrawn after a public check exposed a native
HTTP body-read panic, also present in 0.2.0-dev.1. Users of either native version
need the complete 0.2.1 installer once; their affected updater cannot fetch its
own repair. Python 0.1.22 discovers and installs 0.2.1 normally. Version 0.2.1
uses blocking HTTP timeouts and includes streamed/stalled-body regression tests.

The bridge needs Python 3.10 or later. Normal native operation no longer needs
Python. Restoring a retained Python version still needs its interpreter. A
launcher rollback does not restore game files or undo add-on changes.

## Release ordering

Versions with the in-app updater through 0.1.21 read GitHub's `/releases/latest`. They only accept Python
archives and cannot skip the bridge. Version 0.1.22 and the updated native
launcher instead read `/releases?per_page=50` and select the highest stable
version containing `Flightdeck-Linux-x86_64.tar.gz`.

An older launcher continues to receive **0.1.22 first**, even after 0.2.1 or a
later stable native release exists. Installing 0.1.22 and restarting loads the
new updater, which can then offer the native release. Versions before the
in-app updater was introduced in 0.1.4 need the full installer.

1. Publish the 0.1.22 full tar/ZIP, launcher source, required corresponding
   sources, checksums and release notes as tag `v0.1.22`, with `draft: false`,
   `prerelease: false`, **`make_latest: "true"`**.
2. Verify `/releases/latest` resolves to `v0.1.22` and an unchanged 0.1.21
   installation offers this update. Restart it after installation.
3. When the native release is ready, publish its stable tag and full assets
   with `draft: false`, `prerelease: false`, **`make_latest: "false"`**.
4. Verify the old endpoint still returns `v0.1.22`, while the release list
   contains the stable native release and 0.1.22 offers that version. Keep
   `make_latest: "false"` on subsequent native releases while older clients
   still need this migration path.

GitHub supports explicit control through the `make_latest` release parameter;
its default is `true`. Do not rely on creation order, a prerelease flag or
GitHub's default selection for this transition. See the
[GitHub release API](https://docs.github.com/en/rest/releases/releases#create-a-release).
GitHub's Latest badge and `/releases/latest/download/...` links therefore stay
on the bridge during the transition. Link directly to the native tag for
native downloads. Component-only releases must also retain Latest on the bridge.

The feed is bounded to 50 releases and 2 MiB. Drafts, prereleases, non-version
tags and entries without the full launcher asset are ignored. The newest
eligible release must have a unique version, exact repository download URL,
valid size and SHA256 digest; invalid metadata fails the check rather than
silently selecting an older package. Ensure the intended stable package stays
within that bounded feed before publishing large numbers of component releases.

These are release instructions, not a record of publication. No release is
created or changed by the build and validation scripts.

## Build and provenance

See [BUILDING.md](../BUILDING.md#python-transition-package-0122) for the command.
`scripts/transition-release.py` checks all source files, the pinned compatibility
archive and the graphics bundle. It emits:

- `Flightdeck-Linux-x86_64.tar.gz` and `Flightdeck-0.1.22-Linux-x86_64.zip`;
- `flightdeck-source-0.1.22.tar.gz`;
- `package-validation.json` and `SHA256SUMS`.

The repository's regular `install.sh` selects the native launcher. The bridge
builder substitutes the reviewed `scripts/install-python.sh` template in the
full packages and regenerates their source manifest. The separate source
archive contains the unchanged sources, both bootstraps and the builder;
`package-validation.json` records the substitution. The full package retains
the pinned 0.1.16 compatibility, 0.1.11 DXVK and 0.1.17 VKD3D artifacts and
notices. Distribute their complete corresponding-source archives alongside it.

## Isolated migration check

Run against the real extracted old release, the bridge archive and a native
archive with a stable version. A test candidate may use `0.2.1` in an isolated
source copy; keep the development checkout and published tags unchanged.

```sh
python3 scripts/check-transition-update.py \
  --legacy-package build/old-package/flightdeck-linux \
  --bridge-archive build/releases/flightdeck-0.1.22/Flightdeck-Linux-x86_64.tar.gz \
  --native-archive build/native-candidate/Flightdeck-Linux-x86_64.tar.gz \
  --output build/transition-check
```

The script installs the unchanged 0.1.21 package in a fresh directory, runs its
actual service and sends normal update/check/install/restart requests. A
test-only network adapter supplies local GitHub metadata and archive bytes:
the old Latest endpoint always offers the bridge, and the release list includes
the native candidate. Product code has no update-URL override or test bypass.

Checks cover 0.1.21 → 0.1.22 → native → 0.1.22 → native; the same browser origin;
retained preference data; no extra browser during handoff; the bridge bootstrap;
terminal `flightdeck --update`; rejection of a modified native executable
without changing the installation; and a final wrapper that requires no Python.
All state and service processes belong to the temporary installation. The check
does not use a real account, game installation or simulator session.
