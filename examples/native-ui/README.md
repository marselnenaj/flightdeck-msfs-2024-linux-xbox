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
ICED_TEST_BACKEND=tiny-skia cargo test --locked -p flightdeck-ui capture_ -- --ignored
```

Captures are written to `build/native-ui`. The old browser implementation is
available in Git history at `v0.2.2:ui/`; it is no longer maintained in parallel.
`capture_parity_screens` renders the native UI with the historical synthetic
status fixture at 1536 × 1024 for side-by-side comparison.
