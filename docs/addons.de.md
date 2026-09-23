# Add-ons installieren

[English instructions](addons.md)

## Community-Ordner und Mod-Liste

Wähle zuerst MSFS 2024 oder MSFS 2020 und öffne im Flightdeck-Launcher
**Mods → Community-Ordner öffnen**. Flightdeck
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

1. Die gewünschte MSFS-Version in Flightdeck auswählen, den Simulator schließen
   und das Add-on für **MSFS 2024 bzw. MSFS 2020** beim Entwickler herunterladen.
2. Den Community-Ordner über Flightdeck öffnen.
3. Das Paket dort entpacken. Richtig ist beispielsweise
   `Community/beispiel-flugzeug/manifest.json`. Zwischen Paketordner und Manifest
   darf kein zusätzlicher äußerer ZIP-Ordner liegen.
4. In Flightdeck **Aktualisieren** wählen und Name sowie Version prüfen.
5. MSFS starten und prüfen, ob das Add-on in der Bibliothek oder Flugzeugauswahl
   erscheint und sich laden lässt.

Wenn der Entwickler einen eigenen Installer vorsieht, diesen verwenden. Vor dem
Ersetzen vorhandener Pakete eigene Einstellungen und Bemalungen sichern.
Die allgemeine Mod-Liste zeigt den Bestand und öffnet den Ordner.
Für Fenix gibt es zusätzlich den unten beschriebenen Ablauf, derzeit nur für MSFS 2024.

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

## Fenix A320 einrichten

Unter **Mods → Fenix A320** bietet Flightdeck **0.1.1** einen eigenen
Einrichtungsablauf. Ältere Launcher mit dem [vollständigen Paket](install.md)
aktualisieren. Derselbe Patch ist auch als
[eigenständiger Patch-Installer](https://github.com/marselnenaj/fenix-a320-linux-patch/releases/tag/v0.1.0-preview.1)
verfügbar.

Der Patch unterstützt MSFS 2024 mit dem festgelegten Xodus-Wine-Runner. Starte
MSFS 2024 einmal, damit die Benutzereinstellungen angelegt sind. Schließe danach
MSFS und alle Fenix-Anwendungen.

1. **Patch einrichten** lädt das Linux-ZIP aus dem öffentlichen Fenix-GitHub-Release
   und prüft dessen SHA-256. Es erstellt eine eigene Runner-/Profilkopie, bewahrt
   die bisherige Umgebung auf und installiert bei Bedarf Microsoft .NET Framework 4.8.
2. Den offiziellen Installer aus dem [Fenix-Konto](https://fenixsim.com/dashboard/)
   herunterladen, seine EXE auswählen und **Installer starten** wählen. Die normale
   Installation samt angebotenen Voraussetzungen im ausgewählten Simulatorprofil
   abschließen und den Installer danach schließen.
3. **Fenix öffnen**, anmelden und aktivieren. Danach Fenix wieder schließen.
4. **Einrichtung abschließen** setzt CPU-Anzeigen, Legacy-Readouts und Autostart.
   Sobald die grüne Meldung **Fenix ist startbereit** erscheint, zur Übersicht
   wechseln und MSFS ganz normal starten.

Flightdeck markiert erledigte Schritte und hebt die nächste Aktion hervor.
Das Beenden des offiziellen Installers allein schließt die Einrichtung noch
nicht ab. Läuft noch eine Windows-Anwendung, den Fenix-Installer und Fenix
vollständig beenden; die Anzeige aktualisiert sich automatisch. Deine Anmeldung
und Lizenz prüft Fenix selbst. Falls im Loginfenster `AltGr+Q` kein `@` eingibt,
`Strg+Alt+Q` versuchen oder `@` kopieren und mit `Strg+V` einfügen.

Nach der Einrichtung startet Fenix automatisch mit MSFS. Du musst es nicht
separat öffnen. Beim normalen Spielende, einem Absturz oder **Stoppen** beendet
Flightdeck die Fenix-Begleitprozesse dieser Sitzung; hängende Prozesse werden
nach einer kurzen Wartezeit beendet. Der offizielle Fenix-Installer und andere
Wine-Profile bleiben davon unberührt.

Dieser Quellstand verwendet Patch **0.1.0-preview.1**. Flightdeck lädt die geprüfte
Version aus `compat/fenix/release.json`; ein neueres GitHub-Release wird nicht
ungeprüft übernommen. Eine Flightdeck-Installation richtet Fenix nicht automatisch
ein. Für den öffentlichen Patch-Download ist keine GitHub-Anmeldung nötig. Das
Flugzeug selbst wird mit dem offiziellen Fenix-Programm heruntergeladen und über
dein Fenix-Konto aktiviert.

Für eine lokale Kopie das **Linux-Installer-ZIP** entpacken und unter **Lokales
Patch-Paket und Wiederherstellung** den Ordner mit `bundle.json` auswählen.
Der GitHub-Quellcode oder das reine Quellarchiv enthält die Wine-Binärdateien nicht.
Ein leeres Feld verwendet den geprüften Download bzw. Cache. Der eigenständige
Installer `install.sh` bietet denselben Ablauf ohne Flightdecks Oberfläche; seine
grafische Oberfläche benötigt Python Tk. Flightdecks Fenix-Bereich benötigt kein Tk.

### Liveries

**Fenix-Installer / Liveries öffnen** startet den bereits installierten offiziellen
Manager, sobald Flightdeck ihn im ausgewählten Wine-Profil erkennt. Vorher den
Simulator und andere Fenix-Anwendungen schließen. Bemalungen müssen zum gekauften
Flugzeug, Triebwerk und Flügel passen: A320 statt A321, CFM oder IAE sowie mit/ohne
Sharklets. Eine **A320-CFM-SL**-Livery gehört beispielsweise zur CFM-Sharklets-
Variante. Eine Livery schaltet kein zusätzliches Flugzeug frei.

Bei Drittanbieter-Downloads die Kompatibilität mit der installierten Fenix- und
MSFS-Version prüfen. Den eigentlichen Paketordner über **Mods → Community-Ordner
öffnen** im aktiven Community-Ordner entpacken und die Liste aktualisieren.
`manifest.json` muss direkt im Paketordner liegen. MSFS neu starten, die passende
Flugzeugvariante und dann die Bemalung auswählen. Wird sie nur in Flightdeck
angezeigt, zuerst Modell, Triebwerk/Flügel, Simulatorversion und eine zusätzliche
äußere Archivebene prüfen.

### Kompatibilität und Wiederherstellung

Getestet wurden Fenix 2.4.0.4720 und MSFS 2024 1.8.16.0 auf Hyprland. Die
Cockpitanzeigen inklusive MCDU, FCU, Funk und Uhr sind geprüft; ein vollständiger
Testflug steht noch aus. Wetterradar ist im verwendeten CPU-Modus nicht verfügbar.
Das Fenix-Binärpaket benötigt x86_64 Linux und glibc 2.38+; für Flightdecks
vollständiges natives Paket gilt weiterhin glibc 2.39+. Andere Wine-/Proton-Builds,
Steam-Prefixe und MSFS 2020 werden von diesem ersten Patch nicht unterstützt.
Der Fensterhelfer blendet passende Fenix-Dienst-/Anzeigefenster aus; die
Fenix-Hauptanwendung bleibt für die Anmeldung zugänglich.

**Vorhandener lokaler Fenix-Patch** bezeichnet eine frühere Entwickler-Einrichtung.
Sie bleibt aktiv; der neue Installationsknopf ist dann gesperrt. Eine automatische
Migration ist nicht vorgesehen. Eine frische Installation lässt sich bei Bedarf
in einer separaten kompatiblen Runtime testen. Gesperrte Schritte können außerdem
auf laufende MSFS-/Fenix-Prozesse, einen anderen Einrichtungsvorgang oder einen
noch offenen vorherigen Schritt hinweisen. Die Anwendungen schließen und
**Status neu laden** wählen. Für den offiziellen Installer muss auch eine EXE
ausgewählt sein.

**Patch rückgängig machen** stellt Runner, Skripte und Windows-Profil von vor dem
Patch wieder her. Das neuere Profil bleibt als Sicherung erhalten. Seit der
Patch-Installation darin ergänzte Einstellungen und Pakete bleiben in dieser
Sicherung und werden nicht in das alte Profil übernommen. Externe
Community-Pakete und Flightdecks separater Xbox-Spielstandspeicher werden nicht
entfernt. Nach unterbrochener Einrichtung bleibt der Spielstart bis zur
Wiederherstellung gesperrt.

[Eigenständiger Installer, Quellcode und Bauanleitung](https://github.com/marselnenaj/fenix-a320-linux-patch).
Das Paket enthält keine Fenix-/Microsoft-Programme, Flugzeuge oder Kontodaten.
Anmeldung und Lizenzaktivierung erfolgen wie üblich im offiziellen Fenix-Programm.
