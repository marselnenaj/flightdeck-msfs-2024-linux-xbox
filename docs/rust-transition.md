# Python-to-Rust transition update

**0.2.5 is now GitHub Latest.** The native launcher and Python transition
version **0.1.22** discover it through their existing update interface.
Versions 0.2.1–0.2.4 update directly using **Updates → Flightdeck**, followed by
**Restart Flightdeck now**.

Users still on **0.1.21 or earlier** should run the full
[0.2.5 installer](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.2.5)
once. Their in-app updater reads `/releases/latest` and only accepts Python
packages; it cannot install the native archive now offered by that endpoint.
The full installer retains launcher settings, game installations and the
previous launcher for rollback.

The unchanged [0.1.22 transition package](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/v0.1.22)
is still available for an explicit two-step migration:

```text
0.1.21 → install 0.1.22 explicitly → in-app update to 0.2.5
```

The bridge verifies the native package before executing its installer, preserves
the existing local service endpoint and opens the native window. Prerelease
flags and tags, including the historical `0.2.0-dev.1`, are excluded from stable
update discovery.

The briefly published 0.2.0 was withdrawn after a public check exposed a native
HTTP body-read panic, also present in 0.2.0-dev.1. Users of either native version
need the complete 0.2.5 installer once; their affected updater cannot fetch its
own repair. Python 0.1.22 discovers and installs 0.2.5 normally. The repair
introduced in 0.2.1 uses blocking HTTP timeouts and includes streamed/stalled-body
regression tests.

The bridge needs Python 3.10 or later. Normal native operation no longer needs
Python. Restoring a retained Python version still needs its interpreter. A
launcher rollback does not restore game files or undo add-on changes.

## Release ordering

During the initial transition through native version 0.2.4, GitHub Latest stayed
on 0.1.22 so unchanged older clients received that bridge automatically. Starting
with 0.2.5, the stable native launcher is marked Latest. The 0.1.22 and native
updaters read `/releases?per_page=50` and choose the highest stable full package;
their discovery does not depend on the Latest badge.

1. Publish the stable native tag and full assets with `draft: false`,
   `prerelease: false`, **`make_latest: "true"`**.
2. Verify `/releases/latest` and the stable release feed both resolve to the
   intended native version. Verify 0.1.22 and prior supported native launchers
   offer and install it normally.
3. Keep the published 0.1.22 tag and asset bytes unchanged for explicit migration
   and rollback. Link old-client users to the full installer or that bridge.
4. Publish component-only releases with **`make_latest: "false"`**, retaining
   the Latest badge on the current full native launcher.

GitHub supports explicit control through the `make_latest` release parameter.
See the [GitHub release API](https://docs.github.com/en/rest/releases/releases#create-a-release).

The feed is bounded to 50 releases and 2 MiB. Drafts, prereleases, non-version
tags and entries without the full launcher asset are ignored. The newest
eligible release must have a unique version, exact repository download URL,
valid size and SHA256 digest; invalid metadata fails the check rather than
silently selecting an older package. Ensure the intended stable package stays
within that bounded feed before publishing large numbers of component releases.

These are release instructions, not a record of publication. No release is
created or changed by the build and validation scripts.

## Build and provenance

The bridge and its historical Python builder are preserved in tag `v0.1.22`
and its published source archive. The current branch contains only the Rust
application and no bridge builder. See [BUILDING.md](../BUILDING.md#python-transition-package-0122)
for the historical rebuild boundary. Existing published assets remain immutable.

The unchanged bridge contains the pinned 0.1.16 compatibility, 0.1.11 DXVK and
0.1.17 VKD3D artifacts and notices. Their complete corresponding-source archives
remain available with the release. Retaining those published packages is enough
to validate and support older clients; it does not require a second application
in the current source tree.

## Isolated migration check

Run against the real extracted old release, the bridge archive and a native
archive with a stable version. Use the actual release version in the candidate;
keep already published tags and artifact bytes unchanged.

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
the fixture Latest endpoint offers the bridge, and the release list includes
the native candidate. This checks the historical two-step transition, not the
current public Latest routing, which points at the native release. Product code has no update-URL override or test bypass.

Checks cover 0.1.21 → 0.1.22 → native → 0.1.22 → native; the same browser origin;
retained preference data; no extra browser during handoff; the bridge bootstrap;
terminal `flightdeck --update`; rejection of a modified native executable
without changing the installation; and a final wrapper that requires no Python.
All state and service processes belong to the temporary installation. The check
does not use a real account, game installation or simulator session.
