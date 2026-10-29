# Compatibility component provenance

The top-level MIT license does not replace the licenses below. Original notices
are retained in source and upstream patch context. New native compatibility
files are provided under LGPL-2.1-or-later; new launcher/build tooling uses MIT.
The exported changes include modifications through 2026-10-22. `source-deltas.json` identifies
every changed upstream file and the resulting content hash.

| Component | Pinned source | License and treatment |
| --- | --- | --- |
| Wine builtin User/Store implementation | [Sightem/WineGDK b5d23b0](https://github.com/Sightem/WineGDK/tree/b5d23b074cfd5e28e79acceaaefaf41a26ce6272) | LGPL-2.1-or-later. Distributed as source patch; full upstream notices remain in staged source. |
| Queue/Async source and private interface headers | Same WineGDK revision | LGPL-2.1-or-later headers retained; generated headers are recreated using pinned WIDL/IDL. |
| Original Microsoft queue implementation ancestry | [Microsoft libHttpClient](https://github.com/microsoft/libHttpClient) | Original Microsoft MIT notice is retained separately in `LICENSES/libHttpClient-MIT.txt`, in addition to WineGDK LGPL notices. No claim that the adapted source is wholly MIT. |
| Store IDL | [xodus-gaming/xgameruntime 64aebca](https://github.com/xodus-gaming/xgameruntime/blob/64aebcabb8c66121eae25d3bf0ace4b582ebb0da/xstore.idl) | LGPL-2.1-or-later. Exact source SHA256 a7d02e5e9bbedb2f36824e5f9e52fca60378fc06dfcf77488ef830bb7b02dc17. |
| GameSave ABI | [same revision xgamesave.idl](https://github.com/xodus-gaming/xgameruntime/blob/64aebcabb8c66121eae25d3bf0ace4b582ebb0da/xgamesave.idl) | LGPL-2.1-or-later. Independent header was checked against all 30 methods, three interface IDs and structure fields in this free source. Native layout was separately tested; no proprietary SDK header is copied. |
| JSON for Modern C++ 3.6.1 | [nlohmann/json v3.6.1](https://github.com/nlohmann/json/tree/v3.6.1) | MIT. Complete original notice remains in `runtime/include/vendor/nlohmann_json.hpp`. |
| Xodus CLI/service modifications | [xodus-gaming/xodus 0670e25](https://github.com/xodus-gaming/xodus/tree/0670e25aeb0e0e9f800f8f2f4968ae3b681842a7) | Upstream GPLv3 text. Recorded as GPL-3.0-only without inferring an unspecified later-version grant. |
| XAL authentication library | [xodus-gaming/xal-rs 6aa2420](https://github.com/xodus-gaming/xal-rs/tree/6aa2420f8f0d5e6f7460ff13f399037583a942f4) | MIT. Library and required text fixture imported into the Xodus patch; original LICENSE retained. Local changes migrate Reqwest/HTTP/OAuth while preserving request signing and disabling OAuth redirects. Examples omitted. |
| EAPPX format library | [tuxuser/eappx-rs e392228](https://github.com/tuxuser/eappx-rs/tree/e39222821f8004c663f61a917304e8f5f679b224) | MIT. Library and required text fixtures imported into the Xodus patch; original LICENSE retained. Local changes update XML dependencies and add package-reader regressions. Example applications and binary sample packages omitted. |
| New native proxy, Store and local GameSave implementation | This repository | LGPL-2.1-or-later, except separately identified third-party code. |

License texts are in `compat/LICENSES`. The free GameSave IDL's original Wine
notice is reproduced in `LICENSES/GameSave-ABI-NOTICE.txt`. Microsoft Learn
pages were used as API references; page text/HTML archives are not included.
The purchase adapter also follows the public Microsoft-hosted dialog protocol
used by Xbox's web storefront. Microsoft's website scripts and payment UI are
not copied or distributed; the original local host and broker additions follow
the Xodus component's GPL-3.0-only license.

The original fallback runtime is a Wine builtin from the user-supplied pinned
Proton runner, not a bundled Microsoft GDK binary. Neither it nor other runner,
game or SDK binaries are distributed here. Runtime staging verifies its expected
ABI fingerprint and copies it only into a user-requested local runtime.

This provenance review covers this source export. Compiled Rust/native packages
also contain transitive dependencies and must receive a separate binary-release
review. Upstream Cargo.lock is preserved for reproducibility, not presented as a
complete binary license audit.

## MobiFlight HID compatibility payload

Development builds may embed a modified HidSharp 2.6.4 DLL (James F. Bellinger,
Apache-2.0) using the `bundled-mobiflight` feature. Original copyright, full license,
change description, exact package/output pins and Flightdeck's own MIT-licensed
builder are in [mobiflight/](mobiflight/README.md). Full packages append those
notices and distribute the pinned NuGet build inputs in `mobiflight-sources/` of
their corresponding source archive. Mono.Cecil 0.11.6 is MIT-licensed build tooling;
it is not loaded by the launcher. The official MIT-licensed MobiFlight application
and its WASM module are downloaded separately, with their original files preserved.

## Wine GStreamer system-memory module and queue tests

The separately loaded Wine media module includes OnlyCrayZ-Labs’ 2026
system-memory caps negotiation fix under LGPL-2.1-or-later. Exact upstream
revisions, source hashes and the adapted patch are recorded in `media.lock.json`
and [the media build/provenance instructions](media/README.md). Full binary
releases embed the module as data and supply its original source archives inside
the matching Flightdeck source release, with the patch and build instructions.
The module is rebuilt independently of the existing runner; no complete Proton
runner is distributed. Taskqueue lifecycle test scenarios adapted from
OnlyCrayZ-Labs/xgameruntime retain LGPL attribution in their test source.

## Bundled NVIDIA renderer corrections

Full installers include the matched DXVK and VKD3D libraries pinned in
`graphics.lock.json`. DXVK is zlib-licensed; VKD3D-Proton retains its LGPL and
component notices. Both source trees and their exact recursive submodules are
distributed as corresponding-source release assets. The full graphics bundle
contains the original license texts of those components and their included
dependencies. No proprietary NVIDIA driver or game DLL is included.
