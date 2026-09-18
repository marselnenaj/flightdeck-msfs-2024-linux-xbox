# Add-ons installieren

[English instructions](addons.md)

## Community-Ordner und Mod-Liste

Öffne im Flightdeck-Launcher **Add-ons → Community-Ordner öffnen**. Flightdeck
liest den tatsächlichen Paketpfad aus deiner Runtime oder der `UserCfg.opt` des
Simulators und öffnet ihn im Linux-Dateimanager. Du musst den Ordner nicht selbst
im Wine-Präfix suchen.

**Aktualisieren** liest die Paketnamen, Versionen und Hersteller aus den lokalen
`manifest.json`-Dateien. Verknüpfte Paketordner werden unterstützt. Fehlende oder
ungültige Manifeste sind gekennzeichnet. Die Liste bestätigt weder die Aktivierung
im Spiel noch Marketplace-Lizenzen oder Linux-Kompatibilität.

Wenn kein Ordner erkannt wird, MSFS einmal starten und den Paketpfad im Spiel
prüfen. Bei mehreren widersprüchlichen Konfigurationen zeigt Flightdeck keinen
willkürlich ausgewählten Ordner an. Bei großen Beständen weist die Oberfläche auf
eine begrenzte Prüfung hin.

## Normale Mods als ZIP

1. MSFS schließen und die **MSFS-2024-Version** beim Entwickler herunterladen.
2. Den Community-Ordner über Flightdeck öffnen.
3. Das Paket dort entpacken. Richtig ist beispielsweise
   `Community/beispiel-flugzeug/manifest.json`. Zwischen Paketordner und Manifest
   darf kein zusätzlicher äußerer ZIP-Ordner liegen.
4. In Flightdeck **Aktualisieren** wählen und Name sowie Version prüfen.
5. MSFS starten und prüfen, ob das Add-on in der Bibliothek oder Flugzeugauswahl
   erscheint und sich laden lässt.

Wenn der Entwickler einen eigenen Installer vorsieht, diesen verwenden. Vor dem
Ersetzen vorhandener Pakete eigene Einstellungen und Bemalungen sichern.
Flightdeck installiert, aktualisiert oder löscht einzelne Mods derzeit nicht;
die neue Ansicht zeigt den Bestand und öffnet den Ordner.

## FlyByWire A32NX

1. Den **Linux-Installer** von [FlyByWire](https://flybywiresim.com/downloads/)
   herunterladen. Die AppImage lässt sich ohne systemweite Installation verwenden.
2. Als Community-Pfad den genauen Pfad aus Flightdeck einstellen. Eine
   Wine-Installation wird unter Linux nicht in jeder Konfiguration automatisch
   gefunden.
3. **A32NX → Microsoft Flight Simulator 2024 → Stable** auswählen und installieren.
   Die Pakete für MSFS 2020 und 2024 sind getrennt.
4. Nach Abschluss in Flightdeck aktualisieren. Der Flugzeugordner heißt
   `flybywire-aircraft-a320-neo`.

Für Zusatzfunktionen wie Geländedaten und die entfernte MCDU-Oberfläche lässt sich
[SimBridge](https://docs.flybywiresim.com/tools/simbridge/install-configure/installation/)
ebenfalls über den FlyByWire-Installer installieren. Das geprüfte Paket 0.7.0
enthält die Windows-Anwendung `fbw-simbridge.exe`; der Linux-Installer macht daraus
keine native Linux-Anwendung. Start unter Wine und Verbindung zum Simulator müssen
separat funktionieren. Ein vorhandener Flugzeugordner reicht dafür nicht aus.

Im getrennten SimBridge-Test starteten der HTTP-Dienst und die Geländekarten-
Initialisierung. Dafür wurden die drei vorhandenen Runner-Dateien
`libvkd3d-1.dll`, `libvkd3d-shader-1.dll` und `libvkd3d-utils-1.dll` im Testpräfix
benötigt. Eine Verbindung zum laufenden Simulator ist damit noch nicht bestätigt.

## Fenix: bisheriger Wine-Test

Den aktuellen Installer im [eigenen Fenix-Konto](https://fenixsim.com/dashboard/downloads/)
herunterladen. Laut [Fenix-Anleitung](https://support.fenixsim.com/hc/en-us/articles/12459059815823-New-Fenix-Installer)
müssen MSFS und die Fenix-Anwendung vor der Installation geschlossen sein. Fenix
benötigt deine Anmeldung und installiert eine separate Begleitanwendung.

Der getestete Installer 1.0.286 benötigte WebView2, .NET 8 Windows Desktop x64 und
die Visual-C++-Laufzeit x64. Download und Installation des Fenix Airbus A320
**2.4.0.4720** wurden in einem getrennten Wine-Testpräfix mit folgenden
Einstellungen abgeschlossen:

- Umgebungsvariable `DOTNET_SYSTEM_GLOBALIZATION_USENLS=1` für .NET.
- Registry-Wert `HKCU\Software\Microsoft\Avalon.Graphics\DisableHWAcceleration`
  vom Typ DWORD auf `1`, damit WPF per Software rendert.
- Den Wine-Lader `mscoree` aktiviert lassen.
- Umgebungsvariable `DOTNET_ReadyToRun=0` für den Fenix-Prozess. Ohne sie scheiterte
  die Downloadplanung mit `SQLite Error 1`, entweder `no more rows available`
  oder `SQL logic error`. Mit ihr schloss der offizielle Installer den Download
  und die Installation ab. Der genaue Laufzeitfehler ist noch nicht geklärt;
  die Datenbank musste weder ersetzt noch gelöscht werden.

Ohne diese Anpassungen traten ein ICU-Ladefehler beziehungsweise ein leeres
Fenster auf. Die englische Anleitung verlinkt die Microsoft-Dokumentation zu den
Einstellungen. Die Umgebungsvariablen nur für den Fenix-Start setzen, nicht
global für MSFS. Flightdeck richtet diese Fenix-Anpassungen derzeit nicht
automatisch ein und startet die Begleitanwendung nicht selbst.

Bei einem unsichtbaren Mauszeiger lässt sich für Fenix
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-features=HideCursorWhileTyping`
testen. Bereits vorhandene Browser-Argumente erhalten. Die Einstellung richtet
sich gegen einen dokumentierten
[WebView2-Cursorfehler](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5687);
ihre einzelne Wirkung ist im Wine-Test noch nicht bestätigt.

**MSFS wird nicht gefunden:** Prüfen, in welchem Wine-Präfix Fenix läuft. Ein
frisches Präfix kennt dein vorhandenes Simulator-Profil nicht. Fenix liest die
`UserCfg.opt` und prüft auch die umgebenden Profildateien. Nur diese eine Datei zu
kopieren kann deshalb weiterhin fehlschlagen. `InstalledPackagesPath` muss über die
Wine-Laufwerke dieses Präfixes zum tatsächlichen Paketordner führen. Für erste
Tests getrennte Kopien verwenden, ohne das gesamte Live-Profil zu verknüpfen.
Keine leeren Dateien zum Überlisten der Erkennung anlegen.

**Fenix fehlt in Flightdecks Mod-Liste:** Bei einem getrennten Testprofil landet
das Flugzeug zunächst in dessen eigenem Community-Ordner. MSFS schließen und das
fertige Paket `fnx-aircraft-320` in den Community-Ordner aus Flightdeck kopieren,
ohne eine andere Installation zu überschreiben. Danach die Mod-Liste
aktualisieren. Im Test wurden alle 2.076 kopierten Dateien geprüft und das Paket
von Flightdeck erkannt. Spätere Installer-Updates im Testprofil aktualisieren
diese Kopie nicht automatisch.

Lizenzaktivierung des Flugzeugs, Begleitanwendung und Verbindung zu MSFS sind
damit noch nicht bestätigt. Eine abgeschlossene Installation ist kein Nachweis
für funktionierende Cockpit-Anzeigen oder Flugzeugsysteme. Die separate
Begleitanwendung gehört nicht zum Community-Paket.

Flightdeck enthält keine Flugzeugpakete, kostenpflichtigen Installer oder Kontodaten.
