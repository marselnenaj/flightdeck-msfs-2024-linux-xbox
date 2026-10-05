# Historical native UI comparison fixture

`web-0.2.2.json` contains synthetic API responses used to render the former web
interface and the Rust interface at the same size. It contains no real user
paths, accounts or service credentials. The web implementation is available in
`v0.2.2:ui/`; its unchanged reference was carried in v0.2.3 before removal.

Run `ICED_TEST_BACKEND=tiny-skia cargo test --locked -p flightdeck-ui
capture_parity_screens -- --ignored` from the repository root. The native
comparison images are written to `build/ui-parity/`. They are evidence for
visual review, not proof of an end-to-end simulator or account workflow.
