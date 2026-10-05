# Localization

Flightdeck supports German (`de`) and English (`en`) across the interface,
setup status/errors, launcher messages and native installer. The generated Rust
CLI option reference is currently in English. The language selector
does not change the simulator's language, account or Store country.

## Selection

The native interface chooses the first available language from:

1. An explicit `flightdeck --language de` or `--language en`.
2. The selector preference in private `ui-preferences.json` in the launcher state directory.
3. `LC_ALL`, then `LC_MESSAGES`, then `LANG`; German locales select German and
   other locales fall back to English.

The preference is local; no account or remote service is involved. Changing the
language preserves the current view and form values. During a submitted command,
changes are deferred by disabling the selector until its response arrives.
The native window does not read preferences from a browser profile. An existing
0.2.2 browser preference stays in that browser for a possible rollback.

The CLI and installer also accept `--language de|en`. Their saved management
language is separate from the desktop preference. Only an explicit launch flag
overrides the desktop's saved choice.

## Implementation

- `native/ui/catalog.json` and native `tr` selections own interface text.
  Rendered values remain plain text. The previous web implementation's catalog
  stays with the frozen test reference in `tests/reference-web/`.
- Requests carry `Accept-Language`. `native/i18n.rs` and `native/catalog.json`
  translate known display fields at the response boundary. API keys, enums,
  IDs, paths, hashes, saved settings and authorization tokens retain their values.
- Asynchronous setup messages retain their source template and parameters.
  Each client reads the same job in its chosen language; HTTP threads have no
  shared mutable display language.
- The installer and CLI keep their terminal copy next to their code, without
  requiring gettext tools or downloaded language packs.

## Adding translations

Keep new messages in the relevant catalogs and use named placeholders for
runtime values. Preserve placeholder names between translations. Do not infer
languages from paths or rewrite arbitrary strings to translate error messages.
When adding a supported language, update selection validation, all catalogs,
CLI/installer choices and documentation together.

Run `cargo test --locked --workspace` and the HTTP checks. Use the native
`capture_all_native_screens` rendering test for long labels in both supported
window sizes. Check persisted language, switching during jobs, failed requests
and sanitized exports. Technical filenames and product names are intentionally
not translated.
