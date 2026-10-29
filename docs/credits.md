# Credits

Flightdeck is built and maintained by [marselnenaj](https://github.com/marselnenaj)
with help from its community and the open-source projects below. The launcher’s
**Credits** page is available from the bottom of the sidebar in German and
English, including the compact icon navigation and without a configured game.

## People and collaboration

- **[OnlyCrayZ-Labs](https://github.com/OnlyCrayZ-Labs/MSFS2024-Linux)**:
  Wine system-memory media negotiation fix and taskqueue lifecycle test scenarios,
  integrated in Flightdeck 0.2.25. Exact reviewed revisions and adaptation details
  are in [media provenance](../compat/media/README.md).
- **[gut-engel](https://github.com/gut-engel)**: keep the Microsoft sign-in
  window open on NVIDIA/Wayland systems
  ([contribution](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/commit/ac74d8855901768640442c31b8a2215585e5ca49)).
- **Flightdeck community and testers**: system and GPU reports, reproductions,
  bug reports, ideas and support. Join the [community](https://discord.gg/fAhHR2DkqN).

## Projects used by Flightdeck

- **Xodus Gaming**, Xodus, XAL, xgameruntime and the adapted Proton runner.
- **Sightem / WineGDK**, Wine, CodeWeavers and Valve / Proton.
- **Microsoft / libHttpClient**, the original queue implementation ancestry.
- **tuxuser / eappx-rs** and **nlohmann / JSON for Modern C++**.
- **DXVK**, **VKD3D-Proton**, GStreamer and FFmpeg, including their contributors.
- **Rust**, **iced**, **tiny-skia** and all production Rust dependencies.
- **Fenix A320 Linux Patch**, the optional patch and shared Rust Steam installer engine.
- **MobiFlight / HubHop**, cockpit hardware integration, WASM module and event data.
  The community add-on patch supplied the starting ideas for the reviewed HID work.
- **James F. Bellinger / HidSharp**, Apache-2.0 HID library; Flightdeck's separately
  built compatibility change retains the original copyright and full license.
- **Mono.Cecil**, MIT-licensed maintainer-only assembly tooling.

Project and contributor credits acknowledge their work; they do not imply
endorsement or that every external program is bundled. Compatibility components
retain their original licenses and notices. See [component provenance](../compat/THIRD_PARTY_NOTICES.md)
and [binary package provenance](binary-release.md).

## Artwork and typography

**Manrope Project Authors**, including Mikhail Sharanda and Mirko Velimirovic,
provide the interface font under the SIL Open Font
License 1.1. The original Flightdeck aircraft illustrations were created with
OpenAI image generation. The logo and icons are part of the Flightdeck project.
See [artwork provenance](artwork.md) for the original sources, authorship and
license details.

## Complete dependency and license records

Each [release](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases)
includes `launcher-dependencies.json` with every production Rust crate, its
version, repository and license information, plus `THIRD-PARTY-NOTICES.txt` and
matching source archives. The installation also includes Rust standard-library
notices and the original native/graphics component notices. Credits-page
license buttons open the matching local documents when available and otherwise
link to the relevant published records. The dependency inventory and source
links use the running Flightdeck version.

The Credits page complements these complete records. New contributions and
projects should be added to `native/ui/src/credits.rs` with a concrete German
and English description and an upstream link. Update the corresponding source
pins and original notices when incorporating code.
