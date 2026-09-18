# Build the compatibility components

The source export contains a native runtime proxy, local GameSave provider,
patches for a WineGDK builtin module and patches for the Xodus broker/CLI. It
does not contain a game, runner, Windows SDK, compiled library, credentials or
authenticated service response. Linux x86-64 is the tested host architecture.

## Toolchain

Use Python 3.12+, Git, GNU Make, GCC/G++, a POSIX-threaded x86-64 MinGW toolchain,
Rust/Cargo 1.98+, protobuf `protoc`, pkg-config and the development packages
required by Xodus's GTK/WebKit login window and OpenSSL. The original native
build used MinGW GCC 16.2 and the pinned Wine source's WIDL 11.8. The builtin
uses Wine's bundled libc++, libxml2 and import libraries; it does not require
installing a replacement host Wine. Ensure `cargo` and `protoc` are on PATH.

## Obtain pinned sources

`compat/upstreams.lock.json` records revisions, the Wine archive hash and patch
hashes. Inputs can be downloaded independently and reused offline:

```sh
mkdir -p build/downloads
curl --fail --location \
  https://codeload.github.com/Sightem/WineGDK/tar.gz/b5d23b074cfd5e28e79acceaaefaf41a26ce6272 \
  --output build/downloads/winegdk.tar.gz
git clone https://github.com/xodus-gaming/xodus.git build/downloads/xodus
git -C build/downloads/xodus fetch origin 0670e25aeb0e0e9f800f8f2f4968ae3b681842a7
python3 scripts/stage-compat.py \
  --winegdk-archive build/downloads/winegdk.tar.gz \
  --xodus-repo build/downloads/xodus \
  --destination build/compat
```

Staging verifies the archive, exports the exact Xodus commit (ignoring any
mutable checkout), checks both patches before applying them, and creates a
source manifest. It only writes a new destination. `compat/source-deltas.json`
records the intended changed files. The archived upstream sources retain their
licenses. Store IDL is separately pinned; other private interface headers are
generated from the pinned WineGDK IDLs during the build.

```sh
BUILD_JOBS=8 bash scripts/build-compat.sh build/compat
python3 -m unittest discover -s tests/compat -p 'test_*.py'
```

The build produces `build/compat/artifacts/manifest.json` and six files:

| Path | Purpose |
| --- | --- |
| `bin/xodus-cli` | Login, licensed package access and process launch |
| `bin/xodus-service` | Account and authenticated Store broker |
| `bin/flightdeck-connected-storage.exe` | Isolated Xbox cloud-save reader and sync client |
| `runtime/xgameruntime.dll` | Native queue/network/Store/GameSave proxy |
| `builtin/x86_64-windows/xodus_store_test.dll` | Wine builtin User/Store module |
| `builtin/x86_64-unix/xodus_store_test.so` | Unix IPC transport |

`xodus_store_test` is the compatibility module's actual embedded loader name,
retained for ABI compatibility; renaming the file is insufficient. CLI and
service are built together with the default Linux Secret Service backend. Do
not mix a file-keyring build with a Secret Service build.

## Synthetic native checks

With a separately supplied compatible Wine runner:

```sh
python3 tests/compat/run-native.py --stage build/compat \
  --wine "$RUNNER/files/bin/wine"
# Run just one family when investigating a change:
python3 tests/compat/run-native.py --suite store --stage build/compat \
  --wine "$RUNNER/files/bin/wine"
# --suite gamesave selects local storage and Python/native interchange tests.
# --suite catalog selects only the three catalog/mapper tests.
```

The default `--suite all` builds and runs the core, synchronous bridge and
asynchronous GameSave tests, the Python/native save-format interchange test,
plus four Store tests. Each test uses its own new
Wine prefix. `--suite gamesave` selects storage and interchange tests; `--suite store`
selects the query lifecycle test and three catalog/mapper tests. The narrower
`--suite catalog` selects only the catalog parser, parallel reader and coin
provider/mapper.

The lifecycle checks use a synthetic provider with the real native task queue:
copied inputs, result ownership, paging, cancellation, callback reentry and
asynchronous errors. Catalog checks inject local fixtures and mock fetch/Store
functions. They cover bounded concurrency, cache separation, cancellation,
regional offers, exact SKU joins, unknown/expired ownership and output lifetime.
The fixtures are handwritten synthetic data; these tests do not fetch catalog
documents, read an account or establish real product ownership.

No game, account or cloud calls are involved. Results, input source hashes and
test binary hashes remain below the ignored build directory. Prefix cleanup
stops only the wineserver associated with each test prefix.
The historical development validation also covered cross-prefix file locks,
private interface routing, real account authentication, signed license tokens,
Store cancellation, network policy, and runtime reference counting. Those
private run artifacts are intentionally excluded; this source export does not
pretend that its smaller clean test suite reproduces every integration run.

See [the runtime contract](docs/runtime.md) to prepare a local installation from
the built artifacts and user-owned runner, package and Wine prefix.
