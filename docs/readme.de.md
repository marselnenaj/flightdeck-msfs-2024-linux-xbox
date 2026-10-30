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

**Neu in 0.3.6:** Die Medienprüfung beim Xbox-Spielstart verwendet dieselbe
GStreamer-Cachedatei wie Wine. Scheitert ein benötigter Video-Baustein an alten negativen Plugin-Einträgen,
wird nur dieser Cache einmal neu aufgebaut. Das Spielprofil bleibt erhalten.
Der reproduzierte Widerspruch zwischen grüner Prüfung und fehlgeschlagener
4K-Wiedergabe wird im selben isolierten Profil behoben. Die sichere Diagnose
enthält Cache-Reparatur, Plugin-Architektur und die fehlgeschlagene Wartungsphase
mit Fehlercode. Den vollständigen Simulatorstart in #16 muss der Tester noch bestätigen.
[Reproduktion und Testumfang](media-validation.md)

**Neu in 0.3.5:** [MobiFlight / WinWing](mobiflight.de.md), GSX-Engine-Reparatur,
begrenzte X11-Startprüfung und eine Reparatur des CLR-Overrides hinter dem
Fenix-Ladefehler aus #10. Hardware-Verbindungen und vollständige Add-on-Flüge
bleiben experimentell. Die Loader-Korrektur wurde in einem konto-freien
Wine-Profil geprüft; der vollständige Fenix-Installer muss noch bestätigt werden.

**Neu in 0.3.4:** Die Medienprüfung lädt Decoder und Wines Video-Nachbearbeitung
tatsächlich, bevor Flightdeck einen Proton-Medienpfad auswählt. Das behebt einen
Prüffehler bei zwischengespeicherten Plugin-Einträgen und verbessert die sichere
Diagnose fehlender Codecs. Der korrigierte CachyOS-20261015-Fallback bestand den
echten 4K-Ladevideotest einschließlich Zurückspulen und sauberem Beenden. Den
vollständigen Simulatorstart in #16 muss der Tester noch bestätigen.
[Reproduktion und Testumfang](media-validation.md)

**Neu in 0.3.3:** optionale Einrichtung von Orbx Central, Navigraph Hub und
Simlink sowie GSX-Vorbereitung im gewählten **Xbox- oder Steam-Profil** von MSFS
2020/2024. Die experimentellen Integrationen verwenden das vorhandene Wine-Profil
und dessen gewähltes Proton. Die Navigraph-Browser-Anmeldung erreicht die wartende
App; Orbx umgeht den fehlschlagenden Web-Updater und das Wine-Tray-Fenster.
Vollständige Flüge mit diesen Add-ons müssen Tester noch bestätigen.
[Einrichtung und Testumfang](addons.de.md)

Die SISU- und Lizenzkorrekturen aus 0.3.2 bleiben erhalten. Ein vollständiger
isolierter Xbox-MSFS-2024-Download bestand mit allen 28.621 geprüften Dateien.

**Steam-Unterstützung:** Steam-MSFS 2020/2024 lässt sich neben Xbox-Installationen
auswählen. Fenix verwendet den gemeinsamen Rust-Code des eigenständigen
Installers. Steam-Unterstützung ist Beta; echte Simulatorflüge müssen Tester
noch bestätigen. Siehe [Steam-Anleitung](steam.de.md) und [Änderungen](changelog.md).

In der [Flightdeck-Community auf Discord](https://discord.gg/fAhHR2DkqN) kannst du
Fragen stellen, dein Linux-Setup teilen und an Flightdeck mitarbeiten.
Reproduzierbare Fehler bitte über [GitHub Issues](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues) melden.

[Credits und Danksagungen](credits.md) nennen die Mitwirkenden und Projekte
hinter Flightdeck. Im Launcher gibt es außerdem eine eigene **Credits**-Seite.

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
GStreamer Good/Bad/Libav. Der Launcher benötigt außerdem liblzma, libgcc_s,
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
| **Mods** | Community-Ordner verwalten sowie Fenix und GSX über ihre offiziellen Installer einrichten. [Anleitung](addons.de.md) |
| **Spielstände** | Lokale Backups und experimenteller automatischer Xbox-Cloud-Abgleich mit Konfliktbehandlung. [Anleitung](cloud-saves.de.md) |
| **Grafik und VR** | Automatische GPU-Einrichtung, NVIDIA-Kompatibilitätsoptionen und optionale OpenXR-Einrichtung. [Grafik](graphics.de.md) · [VR](vr.de.md) |
| **Diagnose** | Ausgewählte Prüfungen und kontrollierbare Problemberichte ohne rohe Spiellogs, Tokens oder Spielstandinhalte. [Anleitung](problem-reports.de.md) |

Die Oberfläche unterstützt Deutsch und Englisch. Der lokale Hintergrunddienst
startet automatisch; kein Terminal muss geöffnet bleiben. Das Schließen von
Flightdeck beendet einen laufenden Simulator nicht. Ein Download lässt sich nach
einem Dienst- oder Linux-Neustart noch nicht wiederaufnehmen.
[Grenzen von Pause/Fortsetzen](download-pause.de.md)

## Kompatibilität

**Experimentell.** Cockpit-Zugriff und Flugzeugstart in MSFS 2024 wurden auf AMD
getestet. **Die NVIDIA-Darstellung ist durch Nutzertests bestätigt**;
kleinere Probleme und das Startvideo bleiben offen. Das bestätigt keine allgemeine
Unterstützung aller GPUs und Treiber. Mit CachyOS 11.0-20261015 und 0.2.20
erreicht ein Melder nach schwarzem Ladebildschirm das gerenderte Menü und eine
Szene in MSFS 2024. Startvideo und die VSync-Begrenzung auf 48 FPS bleiben offen.
[Gemeldeter Prüfumfang](known-issues.de.md). Für MSFS 2020 sind Installation und
Lizenzprüfung implementiert; erfolgreicher Simulatorstart und vollständige Flüge
bleiben unbestätigt.

Das Fenix-Cockpit wurde geprüft. Umfassende Flugtests, GSX im Simulator, echte
VR-Headsets und geräteübergreifendes Spielen mit Cloud-Saves stehen noch aus.
Marketplace-Sitzungsfehler und unvollständig geprüfte DLC-/Kauffunktionen sind
separat dokumentiert. Vorhandene Dateien bestätigen weder Aktivierung noch
funktionierende Flugzeugsysteme.
Fenix 0.1.0 behält GPU-Anzeigen für die festgelegten Xodus- und
Proton-11-Versionen mit CPU-Rückfall sowie die Direct2D-Speicherkorrektur bei.
Fenix v3 bleibt experimentell; siehe den [Prüfumfang](addons.de.md#kompatibilität-und-wiederherstellung).
[Bekannte Probleme](known-issues.de.md) · [Add-on-Umfang](addons.de.md) ·
[Marketplace-Umfang](marketplace-collections.md)

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
