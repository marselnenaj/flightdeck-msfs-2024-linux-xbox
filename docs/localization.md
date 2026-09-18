# Localization

Flightdeck supports German (`de`) and English (`en`) across the interface,
setup status/errors, launcher CLI and source installer. The language selector
does not change the simulator's language, account or Store country.

## Selection

The interface chooses the first available language from:

1. An explicit `?lang=de` or `?lang=en` URL parameter, including URLs opened by
   `flightdeck --language …`.
2. The preference saved by the language selector in this browser.
3. The browser language (`de` selects German; other languages fall back to English).

The selected language is saved locally; no account or remote service is involved.
If browser storage is unavailable, the app can still select a language for that
session. Dates, numbers and count labels follow the selected UI language.

The CLI and installer accept `--language de|en`. Terminal language detection
uses `LC_ALL`, then `LC_MESSAGES`, then `LANG`; German locales select German,
with English as fallback. An explicit flag wins. The installer keeps its chosen language for later management actions. Normal
app launches preserve the browser preference; only an explicit launch flag
overrides it.

## Implementation

- `ui/i18n.js` owns the UI catalog and selection. Interpolated values are inserted
  as text, never as translated HTML. Language changes preserve the current view,
  form values and any ongoing setup job.
- Requests carry `Accept-Language`. `flightdeck/i18n.py` translates only known
  display fields at the response boundary. API keys, enums, IDs, paths, hashes,
  saved settings and authorization tokens retain their values.
- Asynchronous setup messages retain their source template and explicit
  parameters. Each client can read the same job in its chosen language; there
  is no global mutable language shared between HTTP threads.
- The installer and launcher CLI keep their terminal copy next to their code,
  without requiring gettext tools or downloaded language packs at runtime.

## Adding translations

Keep new messages in the relevant catalogs and use named placeholders for
runtime values. Preserve placeholder names between translations. Do not infer
languages from paths or rewrite arbitrary strings to translate error messages.
When adding a supported language, update selection validation, all catalogs,
CLI/installer choices and documentation together.

Run the Python tests and `node --test ui/tests/*.test.mjs`, then exercise the
browser suite. Check long labels at desktop and mobile sizes, persisted language
choice, switching during setup, failed requests and safe diagnostic exports.
Technical filenames and product names are intentionally not translated.
