# Compatibility component provenance

The top-level MIT license does not replace the licenses below. Original notices
are retained in source and upstream patch context. New native compatibility
files are provided under LGPL-2.1-or-later; new launcher/build tooling uses MIT.
The exported changes were made on 2026-09-17. `source-deltas.json` identifies
every changed upstream file and the resulting content hash.

| Component | Pinned source | License and treatment |
| --- | --- | --- |
| Wine builtin User/Store implementation | [Sightem/WineGDK b5d23b0](https://github.com/Sightem/WineGDK/tree/b5d23b074cfd5e28e79acceaaefaf41a26ce6272) | LGPL-2.1-or-later. Distributed as source patch; full upstream notices remain in staged source. |
| Queue/Async source and private interface headers | Same WineGDK revision | LGPL-2.1-or-later headers retained; generated headers are recreated using pinned WIDL/IDL. |
| Original Microsoft queue implementation ancestry | [Microsoft libHttpClient](https://github.com/microsoft/libHttpClient) | Original Microsoft MIT notice is retained separately in `LICENSES/libHttpClient-MIT.txt`, in addition to WineGDK LGPL notices. No claim that the adapted source is wholly MIT. |
| Store IDL | [xodus-gaming/xgameruntime 64aebca](https://github.com/xodus-gaming/xgameruntime/blob/64aebcabb8c66121eae25d3bf0ace4b582ebb0da/xstore.idl) | LGPL-2.1-or-later. Exact source SHA256 a7d02e5e9bbedb2f36824e5f9e52fca60378fc06dfcf77488ef830bb7b02dc17. |
| GameSave ABI | [same revision xgamesave.idl](https://github.com/xodus-gaming/xgameruntime/blob/64aebcabb8c66121eae25d3bf0ace4b582ebb0da/xgamesave.idl) | LGPL-2.1-or-later. Independent header was checked against all30 methods, three interface IDs and structure fields in this free source. Native layout was separately tested; no proprietary SDK header is copied. |
| JSON for Modern C++3.6.1 | [nlohmann/json v3.6.1](https://github.com/nlohmann/json/tree/v3.6.1) | MIT. Complete original notice remains in `runtime/include/vendor/nlohmann_json.hpp`. |
| Xodus CLI/service modifications | [xodus-gaming/xodus 0670e25](https://github.com/xodus-gaming/xodus/tree/0670e25aeb0e0e9f800f8f2f4968ae3b681842a7) | Upstream GPLv3 text. Recorded as GPL-3.0-only without inferring an unspecified later-version grant. |
| New native proxy, Store and local GameSave implementation | This repository | LGPL-2.1-or-later, except separately identified third-party code. |

License texts are in `compat/LICENSES`. The free GameSave IDL's original Wine
notice is reproduced in `LICENSES/GameSave-ABI-NOTICE.txt`. Microsoft Learn
pages were used as API references; page text/HTML archives are not included.

The original fallback runtime is a Wine builtin from the user-supplied pinned
Proton runner, not a bundled Microsoft GDK binary. Neither it nor other runner,
game or SDK binaries are distributed here. Runtime staging verifies its expected
ABI fingerprint and copies it only into a user-requested local runtime.

This provenance review covers this source export. Compiled Rust/native packages
also contain transitive dependencies and must receive a separate binary-release
review. Upstream Cargo.lock is preserved for reproducibility, not presented as a
complete binary license audit.
