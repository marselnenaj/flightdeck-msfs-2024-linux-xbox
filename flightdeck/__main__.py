"""Run Flightdeck without a framework or a remote service."""
import argparse
import os
from pathlib import Path
import webbrowser

from .backend import Launcher, LauncherError
from .server import Server


SUPPORTED_LANGUAGES = ("de", "en")
SUPPORTED_DESKTOP = True


COPY = {
    "de": {
        "description": "Flightdeck – lokaler MSFS-Linux-Launcher",
        "runtime": "Absoluter Pfad zur vorbereiteten Runtime",
        "state": "Privater Ordner für Launcher-Einstellungen",
        "port": "Lokaler Port (0 = automatisch)",
        "no_browser": "Browser nicht automatisch öffnen",
        "language": "Sprache der Oberfläche und Terminalmeldungen",
        "desktop": "Als eigenes Fenster öffnen; Hintergrunddienst automatisch verwalten",
        "invalid_port": "Port muss zwischen 0 und 65535 liegen.",
        "failed": "Flightdeck konnte nicht starten: {error}\n",
        "local": "Nur lokal. Strg+C beendet die Oberfläche; ein gestartetes Spiel läuft weiter.",
    },
    "en": {
        "description": "Flightdeck – local MSFS launcher for Linux",
        "runtime": "Absolute path to a prepared runtime",
        "state": "Private directory for launcher settings",
        "port": "Local port (0 = automatic)",
        "no_browser": "Do not open the browser automatically",
        "language": "Language for the interface and terminal messages",
        "desktop": "Open an application window and manage the background service automatically",
        "invalid_port": "Port must be between 0 and 65535.",
        "failed": "Flightdeck could not start: {error}\n",
        "local": "Local only. Ctrl+C closes the interface; a running simulator is left running.",
    },
}


def environment_language():
    value = os.environ.get("LC_ALL") or os.environ.get("LC_MESSAGES") or os.environ.get("LANG", "")
    return "de" if value.lower().split("_")[0].split("-")[0].split(".")[0] == "de" else "en"


def main(argv=None):
    preliminary = argparse.ArgumentParser(add_help=False)
    preliminary.add_argument("--language", choices=SUPPORTED_LANGUAGES)
    selected, _ = preliminary.parse_known_args(argv)
    language = selected.language or environment_language()
    text = COPY[language]
    parser = argparse.ArgumentParser(description=text["description"])
    parser.add_argument("--language", choices=SUPPORTED_LANGUAGES, help=text["language"])
    parser.add_argument("--runtime", help=text["runtime"])
    parser.add_argument("--state-dir", type=Path, default=Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "flightdeck", help=text["state"])
    parser.add_argument("--port", type=int, default=0, help=text["port"])
    parser.add_argument("--no-browser", action="store_true", help=text["no_browser"])
    parser.add_argument("--desktop", action="store_true", help=text["desktop"])
    parser.add_argument("--desktop-service", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if not 0 <= args.port <= 65535:
        parser.error(text["invalid_port"])
    if args.desktop_service or (args.desktop and not args.no_browser):
        from . import desktop
        try:
            if args.desktop_service:
                desktop.serve(args.state_dir, args.runtime, args.port)
            else:
                desktop.start(args.state_dir, args.runtime, args.port, args.language)
            return
        except (desktop.DesktopError, LauncherError, OSError) as error:
            from .i18n import error_message, translate_message
            message = error.localized(language) if isinstance(error, desktop.DesktopError) else translate_message(error_message(error), language)
            parser.exit(1, text["failed"].format(error=message))
    try:
        server = Server(Launcher(args.state_dir, args.runtime), args.port)
    except (LauncherError, OSError) as error:
        from .i18n import error_message, translate_message
        parser.exit(1, text["failed"].format(error=translate_message(error_message(error), language)))
    # An explicit flag overrides/persists the UI selection. Otherwise the
    # browser's stored preference and language detection remain in control.
    url = server.url + ("/?lang=" + args.language if args.language else "")
    print(f"Flightdeck: {url}", flush=True)
    print(text["local"], flush=True)
    if not args.no_browser:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
