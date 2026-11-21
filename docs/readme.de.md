<p align="center">
  <img src="../ui/mark.svg" width="76" alt="Flightdeck-Logo">
</p>
<h1 align="center">Flightdeck</h1>
<p align="center">
  Linux-Launcher für Microsoft Flight Simulator 2024 und 2020 auf Xbox PC und Steam.
</p>
<p align="center">
  <a href="#loslegen"><strong>Loslegen</strong></a> &nbsp;·&nbsp;
  <a href="index.md">Dokumentation</a> &nbsp;·&nbsp;
  <a href="known-issues.de.md">Bekannte Probleme</a> &nbsp;·&nbsp;
  <a href="changelog.md">Änderungen</a> &nbsp;·&nbsp;
  <a href="https://discord.gg/fAhHR2DkqN">Discord</a> &nbsp;·&nbsp;
  <a href="../README.md">English</a>
</p>

![Originales Flightdeck-Flugzeugmotiv](../ui/flight-panorama.png)

Flightdeck installiert und startet deine **gekaufte Xbox-PC-/Microsoft-Store-Version
von MSFS 2024 oder 2020** unter Linux über Wine/Proton. Melde dich mit deinem
Microsoft-Konto an, lade das lizenzierte Spiel herunter und starte es über die
native Rust-Oberfläche. Windows, die Xbox-App und eine bestehende MSFS-Installation
sind für die Xbox-PC-Einrichtung nicht erforderlich. Flightdeck erkennt außerdem
bestehende **Steam-Installationen** und verwaltet deren optionale Einrichtung von
Fenix, GSX, Navigraph Hub/Simlink, Orbx Central und MobiFlight;
Spieldownloads, Updates und Kontoanmeldung übernimmt Steam.

**Lade die [neueste stabile Version](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/latest).**
Launcher, Installer und Updater benötigen kein Python.

## Neu

- **0.3.14:** MSFS-2024-Deluxe-Inhalte, die als abgelaufene Testversion
  erschienen, funktionieren wieder; ein Spieler hat die Korrektur bestätigt.
  Store-Erweiterungen werden lizenziert, auch wenn Microsoft dafür eine
  Vorbesteller-SKU führt, und ein aus dem Microsoft-Katalog entferntes Produkt
  verdeckt nicht mehr alle besessenen Erweiterungen.
  [Details](changelog.md#0314--21-november-2026)
- **0.3.13:** Friert MSFS beim Laden der Fenix ein oder bleiben ihre Anzeigen
  schwarz, stellt **Verwalten & reparieren → Fenix-Anzeigen auf der CPU
  zeichnen** auf CPU-Anzeigen um. **Fenix-Logs sammeln** enthält die letzten
  drei Sitzungen. [Details](changelog.md#0313--16-november-2026)
- **0.3.12:** Flightdeck repariert den Fenix-Autostart vor jedem MSFS-Start,
  damit sich der Fenix-Companion wieder mit dem Flugzeug öffnet.
  [Details](changelog.md#0312--15-november-2026)

Frühere Versionen stehen in der [Änderungsübersicht](changelog.md).

## Loslegen

1. Lade **Flightdeck-Linux-x86_64.tar.gz** aus der
   [neuesten Veröffentlichung](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/latest),
   entpacke es und öffne **Install Flightdeck.desktop**. Eventuell musst du den
   lokalen Starter im Dateimanager freigeben. Alternativ dort `./install.sh` ausführen.
2. Für Xbox PC: Wähle **MSFS installieren**, die Edition 2024 oder 2020 und einen
   Zielordner. Melde dich über **Anmelden & installieren** mit dem Microsoft-Konto
   an, dem die PC-Version gehört. Für Steam: Installiere die Windows-Version über
   Steam und wähle die erkannte Installation in Flightdeck.
   [Steam einrichten](steam.de.md)
3. Öffne **Flightdeck** über dein Anwendungsmenü und starte den Simulator.
   Auf der Übersicht wechselst du zwischen installierten Editionen. Jede behält
   ihre eigene Runtime, ihr Wine-Profil, ihre Updates und lokalen Spielstände.

**Bazzite:** Im Desktop-Modus installieren. Meldet der Installer einen
symbolischen Link im Pfad, hilft die [Installationsanleitung mit Ausweichlösung](install.md#bazzite-and-symbolic-link-errors).

Du benötigst eine Internetverbindung und eine gekaufte PC-Lizenz. Plane mindestens
**100 GiB freien Speicher** ein, auch für gestreamte Inhalte, Caches und aufbewahrte
Spielversionen. Die angezeigte Größe des Basisspiel-Downloads ist nur ein Teil davon.

Das Binärpaket benötigt Linux x86-64 mit **glibc 2.39+**, Vulkan-Treiber,
einen Wayland- oder X11-Desktop und einen Linux-Secret-Service-Schlüsselbund.
Zur vollständigen Runtime gehören außerdem GTK 3, WebKitGTK 4.1, OpenSSL 3 und
GStreamer Good/Bad/Libav. Der Launcher benötigt außerdem libgcc_s,
libxkbcommon und die Bibliotheken des verwendeten Wayland-/X11-Desktops.
Setup prüft fehlende Voraussetzungen. Die Launcher-Prüfungen auf Ubuntu 24.04,
Debian 13, Fedora 44, Arch Linux und openSUSE Tumbleweed bestanden am
8. Oktober 2026. Der geprüfte Umfang steht in der
[Linux-Testmatrix](install.md#linux-test-matrix).
Diese Containerprüfungen bestätigen keine Simulator- oder Grafiktreiber-Kompatibilität.
[Voraussetzungen und Installationsoptionen](install.md)

Aktualisiere den Launcher über **Updates → Flightdeck** oder den vollständigen
Installer. Bei 0.1.21 und älter, der 0.2.0-Vorschau oder der zurückgezogenen 0.2.0
ist einmal der vollständige Installer nötig. Bestehende Runtimes und Spielstände
bleiben erhalten. [Update und Rollback](install.md#update-and-rollback) ·
[Migration älterer Versionen](rust-transition.md)

## Im Launcher

| Bereich | Funktionen |
| --- | --- |
| **Installation und Updates** | Lizenzierte Downloads, erneute Versuche, Pause/Fortsetzen, Dateiprüfung, vollständige Reparatur und Rollback. [Details](game-updates.md) |
| **Mods** | Community-Ordner verwalten sowie optionale Einrichtung von Fenix, GSX, Navigraph und Orbx im gewählten Profil. [Anleitung](addons.de.md) |
| **Spielstände** | Lokale Backups und experimenteller automatischer Xbox-Cloud-Abgleich mit Konfliktbehandlung. [Anleitung](cloud-saves.de.md) |
| **Grafik und VR** | Automatische GPU-Einrichtung, NVIDIA-Kompatibilitätsoptionen und optionale OpenXR-Einrichtung. [Grafik](graphics.de.md) · [VR](vr.de.md) |
| **Diagnose** | Ausgewählte Prüfungen und kontrollierbare Problemberichte ohne rohe Spiellogs, Tokens oder Spielstandinhalte. [Anleitung](problem-reports.de.md) |

Die Oberfläche unterstützt Deutsch und Englisch. Der lokale Hintergrunddienst
startet automatisch; kein Terminal muss geöffnet bleiben. Das Schließen von
Flightdeck beendet einen laufenden Simulator nicht. Ein Download lässt sich nach
einem Dienst- oder Linux-Neustart noch nicht wiederaufnehmen.
[Grenzen von Pause/Fortsetzen](download-pause.de.md)

## Kompatibilität

Flightdeck ist **experimentell**. Bisher bestätigt:

- **MSFS 2024, Xbox PC:** Installation, Lizenzierung und Start bis ins Cockpit
  und zum Abheben auf AMD; Spieler bestätigen die NVIDIA-Darstellung. Ein
  vollständiger isolierter Download aller 28.621 Spieldateien bestand, und
  Deluxe-Inhalte sind seit 0.3.14 lizenziert.
- **MSFS 2020, Xbox PC:** Installation und Lizenzprüfung sind implementiert;
  Simulatorstart und vollständige Flüge sind noch nicht bestätigt.
- **Steam MSFS 2020/2024:** Beta. Downloads, Updates und Anmeldung übernimmt
  Steam; vollständige Flüge müssen Tester noch bestätigen.
  [Steam-Anleitung](steam.de.md)
- **Add-ons:** Die Einrichtung von Fenix, GSX, Navigraph Hub/Simlink, Orbx
  Central und MobiFlight ist experimentell. Die Fenix-Cockpitanzeigen wurden
  geprüft; vollständige Flüge, GSX im Betrieb und Hardware-Verbindungen müssen
  noch bestätigt werden.
  [Add-on-Umfang](addons.de.md#kompatibilität-und-wiederherstellung) ·
  [MobiFlight / WinWing](mobiflight.de.md)

Noch nicht geprüft: alle GPUs und Treiber, echte VR-Headsets und Cloud-Saves
über mehrere Geräte. Offen sind unter anderem ein fehlendes oder schwarzes
Startvideo und eine VSync-Bildratenbegrenzung. Vorhandene Dateien bestätigen
weder Aktivierung noch funktionierende Flugzeugsysteme.
[Bekannte Probleme](known-issues.de.md) ·
[Marketplace-Umfang](marketplace-collections.md)

## Community

In der [Flightdeck-Community auf Discord](https://discord.gg/fAhHR2DkqN) kannst du
Fragen stellen, dein Linux-Setup teilen und an Flightdeck mitarbeiten.
Reproduzierbare Fehler bitte über [GitHub Issues](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues) melden.

[Credits und Danksagungen](credits.md) nennen die Mitwirkenden und Projekte
hinter Flightdeck. Im Launcher gibt es außerdem eine eigene **Credits**-Seite.

## Lokale Daten

Flightdeck verwendet Rust mit einer nativen Oberfläche und einer API, die nur
auf Loopback lauscht. Herkunftsprüfungen und ein Sitzungstoken schützen jeden
API-Lesezugriff und jede Aktion. Desktop- und Headless-Clients lesen das Token
aus der nur für den Benutzer zugänglichen `desktop-service.json` im gewählten
Zustandsordner und senden es im Header `X-Flightdeck-Token`. Nicht authentifizierte
Statusabfragen können keine Sitzung initialisieren. Der Launcher enthält keine
Telemetrie und benötigt kein CDN. Microsoft-Anmeldung, Downloads und Online-Inhalte
des Simulators verwenden weiterhin ihre jeweiligen Netzwerkdienste.

Runtime-Dateien, Zugangsdaten, Spielstände und Logs gehören nicht zu Quell- oder
Release-Archiven. Die Oberfläche kann lokale Pfade zeigen; Screenshots vor dem
Teilen prüfen.

## Dokumentation und Entwicklung

Der [Dokumentationsindex](index.md) führt zu Nutzeranleitungen, Entwicklung und
technischen Hintergründen. [BUILDING.md](../BUILDING.md) enthält Build- und
Prüfbefehle, der [Beitragsleitfaden](contributing.md) beschreibt Review und Releases.
Tests verwenden isolierte Beispieldaten. Frühere Versionsmeldungen stehen in der
[Änderungsübersicht](changelog.md); die frühere Python-/Web-Anwendung bleibt in der
Git-Historie verfügbar.

## Lizenz

Launcher, Oberfläche und neue Integrationstools stehen unter [MIT](../LICENSE).
Xodus behält GPL-3.0-only, Wine/GDK-Komponenten LGPL-2.1-or-later. Native Pakete
enthalten Lizenzhinweise und vollständige zugehörige Quellen. Der separat geladene
Runner behält seine eigenen Lizenzen; Manrope steht unter SIL Open Font License 1.1.
[Lizenzhinweise](../compat/THIRD_PARTY_NOTICES.md) · [Paket-Herkunft](binary-release.md)

Flightdeck ist unabhängig und wird nicht von Microsoft, Xbox, Asobo oder den
Upstream-Projekten unterstützt. MSFS-Dateien, kostenpflichtige Add-ons, proprietäre
SDK-Header, Zugangsdaten und Spiellizenzen werden nicht mitgeliefert. Das Titelbild
ist originales Flightdeck-Artwork und kein Simulator-Screenshot.
[Herkunft der Grafiken](artwork.md)
