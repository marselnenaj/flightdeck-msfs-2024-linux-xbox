# Build the compatibility components

The source export contains a native runtime proxy, local GameSave provider,
patches for a WineGDK builtin module and patches for the Xodus broker/CLI. It
does not contain a game, runner, Windows SDK, compiled library, credentials or
authenticated service response. Linux x86-64 is the tested host architecture.

## Toolchain

Use Python 3.12+, Git, GNU Make, GCC/G++, a POSIX-threaded x86-64 MinGW toolchain,
Rust/Cargo 1.98+, protobuf `protoc`, pkg-config and the development packages
required by Xodus's GTK/WebKit login and Store windows and OpenSSL. The original native
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
# --suite user selects authenticated-user lookup and handle lifetime tests.
```

The default `--suite all` builds and runs the core, synchronous bridge and
asynchronous GameSave tests, the Python/native save-format interchange test,
plus seven Store tests and the user-lookup/cache test. Each test uses its own new
Wine prefix. `--suite gamesave` selects storage and interchange tests; `--suite store`
selects explicit product queries, Durable license handles, base-package update
queries, purchase-dialog lifecycle and three catalog/mapper tests. The narrower
`--suite catalog` selects only the catalog parser, parallel reader and coin
provider/mapper.

The lifecycle checks use a synthetic provider with the real native task queue:
copied inputs, result ownership, paging, cancellation, callback reentry and
asynchronous errors. Durable-license tests also check expiry, bounded active
handles, release and delayed result access. Package-update tests cover the
registered base-game scope and unsupported/error cases.
Catalog checks inject local fixtures and mock fetch/Store
functions. They cover bounded concurrency, cache separation, cancellation,
regional offers, exact SKU joins, unknown/expired ownership and output lifetime.
The fixtures are handwritten synthetic data; these tests do not fetch catalog
documents, read an account or establish real product ownership.

No game, account or cloud calls are involved. Results, input source hashes and
test binary hashes remain below the ignored build directory. Prefix cleanup
stops only the wineserver associated with each test prefix.
These synthetic checks do not replace live validation of account authentication,
signed licenses, Microsoft-hosted checkout or in-game content availability.

The purchase host also has browser and native Linux window checks:

```sh
node tests/compat/purchase-ui-test.mjs build/compat
python3 tests/compat/store-window-test.py --stage build/compat --backend x11
python3 tests/compat/store-navigation-test.py --stage build/compat --backend x11
# On a Hyprland desktop, also check the native Wayland attachment:
python3 tests/compat/store-window-test.py --stage build/compat --backend wayland
```

The browser check requires Chromium. The window check uses the native build's
Cargo environment, a running desktop, Pillow and ImageMagick; its Wayland
screenshot check also uses Hyprland and grim. Both use synthetic content and
block external requests. The native check renders the production host, captures
its window and verifies cancellation without opening a real Microsoft checkout.
`--legacy --backend x11` reproduces the previous window attachment for comparison.
The navigation test uses a loopback-only server to reproduce the empty response
to an opaque form origin and verify the public-page bootstrap, cancellation and
document-origin checks. It accepts the same `--backend` options and never loads
a real Store page.

See [the runtime contract](docs/runtime.md) to prepare a local installation from
the built artifacts and user-owned runner, package and Wine prefix.

## Graphics adapter regression check

With a graphical session, a working Vulkan driver, one discrete GPU and the
MinGW toolchain, check the runner's actual DXGI/DirectX 12 adapter handoff:

```sh
python3 scripts/check-graphics-adapter.py --runner "$RUNNER" \
  --output build/graphics-adapter-check
```

The output directory must be new. The check creates an isolated Wine prefix,
installs the supplied runner's DXVK/VKD3D libraries and compiles a small probe.
It compares the old Linux-name filter with UUID selection, verifies explicit
and default D3D12 device creation on the same adapter, checks vendor hiding and
rejects an intentionally invalid UUID. On NVIDIA it also runs both Flightdeck
graphics modes with the actual preparation code. It uses no game or account
and leaves existing installations untouched. These checks establish adapter
selection and device creation; they do not validate simulator rendering or
flight stability. Raw device UUIDs are omitted from the result summary.

## Optional Fenix patch

The Fenix Wine overlay has a separate source/build pipeline in
[fenix-a320-linux-patch](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/main/BUILDING.md).
It is not one of the six native components above. Flightdeck vendors its MIT
installer engine and pins the separately downloaded Wine payload. A normal
Flightdeck build does not compile or bundle Fenix/Microsoft software.

After building and verifying a new patch release, maintainers import its engine
and manifests with:

```sh
python3 scripts/sync-fenix.py /absolute/path/to/fenix-a320-linux-patch
python3 -m unittest discover -s tests -p 'test_fenix.py' -v
node --test ui/tests/fenix.test.mjs
```

Review the imported engine, `compat/fenix/bundle.json` and
`compat/fenix/release.json` together. Publish the exact ZIP and corresponding
sources at the pinned public GitHub release URL before publishing a Flightdeck
package that depends on it. Replacing an existing ZIP with different bytes
breaks the pinned checksum. See the [release checklist](docs/contributing.md#fenix-patch-releases).
