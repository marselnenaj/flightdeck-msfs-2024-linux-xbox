# Historical web UI reference

Frozen UI from the 0.2.2 backend release, retained only as a behavior and visual
reference for the native Rust migration. It is not an application entry point,
not embedded in the Rust executable, and not served by the production API.
The active interface is `native/ui`.

Existing JavaScript unit tests document the old normalization and coordination
contracts. Native policy and interaction tests exercise the replacement directly.
Use `node --test tests/reference-web/tests/*.test.mjs` for this reference only.
The synthetic Chromium harness can capture the original design using the shared
artwork in `ui/`. No browser is required to run or test the native GUI.
