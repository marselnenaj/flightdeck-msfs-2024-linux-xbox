# Native UI fixture

This example uses the **production** Rust widgets in `native/ui`, with synthetic
state and no client connection. Buttons cannot modify any game, account, or
installation. All six screens are available.

```sh
cargo run --locked --manifest-path examples/native-ui/Cargo.toml
```

Rust 1.98 and `woff2_decompress` are required. See `BUILDING.md` for Linux packages.
The production interface uses iced's CPU renderer on X11 and Wayland; it does not
need Chromium or a GPU rendering context. This alone is not a performance claim.

Tests and screenshots live with the production crate:

```sh
cargo test --locked -p flightdeck-ui
ICED_TEST_BACKEND=tiny-skia cargo test --locked -p flightdeck-ui capture_all_native_screens -- --ignored
```

Captures are written to `build/native-ui`. The historical browser UI is retained
only in `tests/reference-web` for migration comparisons. Its synthetic screenshot
harness can still be run with `node examples/native-ui/capture-reference.mjs`.
It is not embedded, served, or executed by the launcher.
