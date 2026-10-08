# Build Flightdeck and the compatibility components

## Rust application and native desktop

The launcher, all six desktop screens, installer, updater and runtime helpers
use Rust. The desktop is an iced 0.14 window with a software renderer, Wayland
and X11 support. It embeds the existing artwork, Manrope font and icons. It does
not need Chromium, JavaScript, Node or Python. Wine/Store ABI components retain
their C/C++ implementation. Python 3.11+ is maintainer build and test tooling.

Build with Rust/Cargo 1.98 and `woff2_decompress` (Debian/Ubuntu package `woff2`).
Linux development packages include `libxkbcommon-dev`, `libwayland-dev`,
`libx11-dev`, `libxrandr-dev`, `libxi-dev` and `pkg-config`:

```sh
cargo build --locked
cargo test --locked --workspace
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/check-rust-http.py --binary target/debug/flightdeck-rust
ICED_TEST_BACKEND=tiny-skia cargo test --locked -p flightdeck-ui capture_ -- --ignored
target/debug/flightdeck-rust
```

Native UI event/rendering tests and the real local-service integration test use
isolated synthetic data. Screenshot capture works without a desktop or GPU and
writes `build/native-ui/`. See [native desktop](docs/native-ui.md) for the
interface contract and [native status](docs/rust-migration.md) for live validation
limits. `--no-browser` retains its historical spelling and starts only the local
API service; there is no web UI at its URL.

The retired web interface is available in Git history (`v0.2.2:ui/`). It is no
longer kept as a second implementation in the source tree. Native controller,
interaction and rendering tests cover the migrated workflows; the historical
synthetic response fixture in `native/ui/tests/fixtures/` supports comparisons.

```sh
python3 -m unittest discover -s tests/compat -p 'test_*.py'
```

There is no Python application or pip package in the current source tree.
The Store sign-in and purchase windows are separate Xodus compatibility
components; their own webview tests below remain relevant.

### Native packages

Build the release with local checkout, Cargo-cache and toolchain paths remapped:

```sh
python3 scripts/build-native.py
mkdir -p build
cargo metadata --locked --filter-platform x86_64-unknown-linux-gnu --format-version 1 > build/cargo-metadata.json
python3 scripts/native-release.py \
  --binary target/release/flightdeck-rust \
  --cargo-metadata build/cargo-metadata.json \
  --rust-notices "$(rustc --print sysroot)/share/doc/rust/COPYRIGHT-library.html" \
  --native build/flightdeck-compat-0.1.16-linux-x86_64.tar.gz \
  --graphics build/graphics \
  --output build/native-package
python3 scripts/source-release.py --output build/native-package/flightdeck-source-0.2.8.tar.gz
```

Install the toolchain's `rust-docs` component if its standard-library notices
are missing. `--offline` on `build-native.py` uses the existing Cargo cache.
The full package requires the exact component archive in
`compat/bootstrap.lock.json` and the seven files from the pinned graphics
bundle. Hashes, binary architecture/version, dependency licenses and payload
limits are checked before packaging. The result contains deterministic tar/ZIP
archives, a production dependency inventory and notices. Crates that omit their
monorepo license use the exact upstream revisions and hashes recorded in
`compat/rust-licenses.lock.json`; the checked license texts ship with the notices. Update `SHA256SUMS` when adding
companion files after packaging.

Distribute the complete corresponding-source archives alongside a full package:
`flightdeck-native-sources-0.1.16.tar.gz`,
`flightdeck-dxvk-0.1.11-sources.tar.gz` and
`flightdeck-vkd3d-0.1.17-sources.tar.gz`. Preserve their published hashes and
licenses. [Provenance](docs/binary-release.md) describes their build inputs.
For an isolated launcher/API development package, replace `--native` and
`--graphics` with `--launcher-only`; it cannot supply a complete offline setup.

Extract the full archive and run `./install.sh --no-launch`, or invoke
`./bin/flightdeck install --source . --no-launch` directly. Custom `--data-dir`,
`--bin-dir` and `--applications-dir` allow a separate test installation.
The package built here targets Linux x86-64; check its host-library requirements
with `ldd`/`readelf` on the intended build image. The validated build requires
glibc 2.39 or later. [Performance checks](docs/performance.md) describe the
isolated comparison with the Python release.
The Rust executable also links to the host's `liblzma.so.5` and `libgcc_s.so.1`.
The UI loads libxkbcommon and the selected Wayland/X11 libraries at runtime.

Release packaging rejects binaries importing glibc symbols newer than 2.39.
Building on a newer host can otherwise raise this requirement through GUI math
dependencies. CI therefore uses Ubuntu 24.04. On another distribution, use the
pinned baseline image with the installed Rust toolchain and Cargo registry:

```sh
docker build -t flightdeck-native-build:ubuntu24.04 -f scripts/native-build.Dockerfile scripts
mkdir -p build/native-baseline
flightdeck_rust_root="$(rustc --print sysroot)"
flightdeck_registry="${CARGO_HOME:-$HOME/.cargo}/registry"
docker run --rm --network none --read-only --user "$(id -u):$(id -g)" \
  --tmpfs /tmp:rw,mode=1777 --tmpfs /cargo:rw,mode=1777 \
  --mount "type=bind,src=$PWD,dst=/src,readonly" \
  --mount "type=bind,src=$PWD/build/native-baseline,dst=/build" \
  --mount "type=bind,src=$flightdeck_rust_root,dst=/opt/rust,readonly" \
  --mount "type=bind,src=$flightdeck_registry,dst=/cargo/registry,readonly" \
  flightdeck-native-build:ubuntu24.04 python3 scripts/build-native.py --offline
```

Populate the Cargo registry on the host first with `cargo fetch --locked`.
Package `build/native-baseline/target/release/flightdeck-rust` in place of the
host release binary. The build mounts source, toolchain and registry read-only,
runs without network and writes only to the explicit build output.

### Linux distribution checks

The main CI workflow tests its packaged launcher on Ubuntu 24.04, Debian 13,
Fedora 44, Arch Linux and openSUSE Tumbleweed. To reproduce one matrix entry
with Docker and an extracted package:

```sh
python3 scripts/check-linux-distro.py --distribution ubuntu \
  --package build/native-package/flightdeck-linux \
  --output build/linux-check-ubuntu
```

Choose a new output directory for each run. Base images are pinned in
`compat/linux-test-images.json`; package installation uses the distribution's
current repositories and needs network access. The actual launcher test runs
as the invoking non-root user with no network, host display, GPU or credentials.
It verifies installation into a path containing spaces and Unicode, the local
HTTP contract, an X11 window, a Wayland buffer commit and uninstallation.
Logs and the tested binary/image identities are saved in the output directory.
These checks cover launcher portability, not simulator, graphics-driver,
Microsoft sign-in or add-on compatibility.

### Python transition package (0.1.22)

The bridge is already published and remains available to older installations.
Its sources and historical builder are preserved in tag `v0.1.22` and the
published `flightdeck-source-0.1.22.tar.gz`. Reproduce the bridge only from that
revision, in a separate checkout; the current branch builds Rust packages only.
Do not replace the published bridge with bytes from the current branch.

The [transition guide](docs/rust-transition.md) explains update ordering and
checks against the unchanged published packages. It does not require keeping
or installing the old Python application in the current source tree.

The commands below build the Wine/Store compatibility components independently.

## Compatibility components

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

To reproduce the automatic .NET repair with real Microsoft installers and a
supported, unmodified runner:

```sh
FLIGHTDECK_TEST_RUNNER="$RUNNER" \
FLIGHTDECK_TEST_CACHE="$RUNTIME/private/fenix-downloads" \
FLIGHTDECK_TEST_OUTPUT="$PWD/build/framework-repair-check" \
cargo test --locked --test native_live_framework -- --ignored --nocapture
```

The output directory must be new. Both pinned installers must already be in the
cache. The test verifies their hashes, copies them, creates an account-free Wine
profile and checks fresh installation, repair after removing its x86 CLR, and an
idempotent follow-up. Existing profiles and the supplied cache are untouched.
Results and private setup logs stay under `build/`. No Fenix license or game is
needed. This check takes several minutes and is separate from the synthetic CI
suite.

The hardware and C++ interchange harnesses invoke `examples/runtime-lab.rs` to
use the actual Rust prefix, graphics, VR, Fenix and save implementations. Build
this development-only executable before running them:

```sh
cargo build --locked --bin flightdeck-rust --example runtime-lab
```

It is not shipped in installer packages. `FLIGHTDECK_PROBE_BINARY` can select
another built copy. Each mutating probe requires a new synthetic runtime; it
cannot use an ordinary game runtime as its test destination.

With a separately supplied compatible Wine runner:

```sh
python3 tests/compat/run-native.py --stage build/compat \
  --wine "$RUNNER/files/bin/wine"
# Run just one family when investigating a change:
python3 tests/compat/run-native.py --suite store --stage build/compat \
  --wine "$RUNNER/files/bin/wine"
# --suite gamesave selects local storage and Rust/C++ interchange tests.
# --suite catalog selects only the three catalog/mapper tests.
# --suite user selects authenticated-user lookup and handle lifetime tests.
```

The default `--suite all` builds and runs the core, synchronous bridge and
asynchronous GameSave tests, the Rust/C++ save-format interchange test,
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

To exercise the built Wine broker transport with delayed synthetic responses:

```sh
python3 tests/compat/ipc-timeout-test.py --stage build/compat \
  --wine "$RUNNER/files/bin/wine"
```

This uses an isolated prefix and a local Unix socket, with no account or HTTP
requests. It covers inventory success and server timeouts after six seconds,
subsequent license queries, delayed Microsoft ticket parsing, package-update
queries, and rejection of late replies after a local transport timeout.

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

The Microsoft sign-in browser handoff has a separate native regression check:

```sh
python3 tests/compat/login-flow-test.py --stage build/compat \
  --output build/login-flow-check --backend x11
# Use --backend wayland to check the native Wayland attachment as well.
```

This requires the same Cargo environment and an active graphical session.
It uses only loopback pages and synthetic tokens, without Microsoft requests or
keyring access. The production webview must retain session cookies and ignore
callbacks from previous views through two follow-up steps. A separate case
checks response-error classification. The output directory must be new.

## Graphics adapter regression check

Flightdeck 0.1.9 also has a separately pinned NVIDIA VKD3D backport and a
real multiwindow rendering/readback check. See [renderer build and packaging](docs/nvidia-renderer.md).

The 0.1.17 test renderer also has a [3D texture layout regression](docs/nvidia-renderer.md#volume-layout-regression).
It requires the Khronos Vulkan validation layer and compares the previous
bundle, the corrected bundle, and controls with maintenance9 disabled. Passing
pixel readback alone does not pass this check.

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

## Experimental GSX setup

The GSX integration reuses Rust's .NET prerequisites, profile
copying and runtime lock without applying a Fenix patch. It downloads the official
FSDT installer from the URL and SHA-256 in `native/gsx.rs`; no proprietary
programs or activated profiles belong in the source or installer archives.

```sh
cargo test --locked --test native_addons --test native_framework --test native_supervisor
cargo test --locked -p flightdeck-ui
cargo test --locked -p flightdeck-ui
```

Runtime cleanup and direct-launch interruption checks also live in
`tests/native_supervisor.rs`. Synthetic package/startup fixtures establish local setup
behavior, not GSX activation or a working simulator connection. Review a changed
official installer in a separate profile before updating the pin, including its
licensing registration and automatic updater behavior. See the
[setup scope](docs/addons.md#gsx-pro-experimental).

## Optional Fenix patch

The Fenix Wine overlay has a separate source/build pipeline in
[fenix-a320-linux-patch](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/main/BUILDING.md).
It is not one of the six native components above. Flightdeck implements its MIT installer contract in Rust and pins the
separately downloaded Wine payload. A normal
Flightdeck build does not compile or bundle Fenix/Microsoft software.

After building and verifying a new patch release, maintainers import its license
and manifests with:

```sh
python3 scripts/sync-fenix.py /absolute/path/to/fenix-a320-linux-patch
cargo test --locked --test native_addons --test native_framework --test native_proton
cargo test --locked -p flightdeck-ui fenix
```

Review the matching Rust implementation, `compat/fenix/bundle.json` and
`compat/fenix/release.json` together. Publish the exact ZIP and corresponding
sources at the pinned public GitHub release URL before publishing a Flightdeck
package that depends on it. Replacing an existing ZIP with different bytes
breaks the pinned checksum. See the [release checklist](docs/contributing.md#fenix-patch-releases).
