# Preserved Xodus Proton runner

Flightdeck mirrors the exact upstream ZIP from Xodus Proton commit
`7c0b435495814349735c913fde78da906aecea52`, without repackaging or changing its
contents. The upstream Actions artifact expires on 5 November 2026; the
[dedicated runtime release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/tag/runner-xodus-7c0b4354)
has no Actions retention deadline. This is a component release, not the latest
launcher release. New launcher versions reference its immutable asset name.

Both the outer ZIP and its contained tar.xz retain the SHA-256 values in
`../bootstrap.lock.json`. Flightdeck verifies both before installing the runner.
The installed runtime may subsequently receive Flightdeck's separately tracked
compatibility patches; those patches are not part of this upstream mirror.

## Sources and notices

The release provides `flightdeck-proton-xodus-7c0b4354-sources.tar.gz`,
`sources.lock.json`, `THIRD-PARTY-NOTICES.txt` and `SHA256SUMS` alongside the ZIP.
The source bundle preserves original archives for:

- The pinned Proton repository and its recursive Git submodules, including Wine,
  DXVK, VKD3D, GStreamer, FFmpeg and their dependencies.
- Additional Git dependencies and the registry crate archives identified by the
  collected Cargo lockfiles, verified against their recorded checksums.
- Xalia 0.4.9, Wine Mono 11.2.0, Wine Gecko 2.47.4 and ICU 68.2 sources matching
  the bundled components.

The inventory deliberately retains upstream submodules even when a particular
architecture does not build them. `repositories` identifies exact revisions,
archive URLs, SHA-256 values and original notice paths. `placements` maps them
back to the upstream source layout, including nested submodules. `cargo-git/`
placements identify Cargo source dependencies; they are not Git submodules.
`registry_crates` and `vendor_sources` describe the other original source inputs.
Each archive retains its original license texts. The three notice files here
are copied directly from the pinned binary distribution. Their presence does
not relicense Proton or its components under Flightdeck's MIT license.

To recover the source tree, verify `SHA256SUMS`, unpack the source bundle into a
new directory and verify each `archives/` file against `sources.lock.json`.
Extract each repository archive into its listed placement with its single
archive-root directory stripped. Populate nested submodules after their parents.
The root archive contains the original configure scripts, Makefiles, patches,
workflow and build instructions; use those upstream recipes and the versions in
the source inventory. Cargo registry/Git inputs can be supplied from the stored
archives using Cargo's source replacement mechanism. The Mono/Gecko/ICU archives
contain their own source and build instructions. Build tools, the Steam SDK
container and other host prerequisites specified by upstream are separate inputs.
This preserves the source inputs and build recipes; it does not claim a hermetic
or byte-for-byte reproducible rebuild of the upstream binaries.

## Maintaining the mirror

Never replace this release's assets or silently change its checksums. A different
runner requires a new component tag, a new source inventory and independent
validation. Keep the component release excluded from GitHub's Latest selection,
which belongs to launcher releases. A replacement launcher must first verify a
cold download from the published release and then check the inner archive hash.
Preserve these source assets for as long as the mirrored binary is distributed.
