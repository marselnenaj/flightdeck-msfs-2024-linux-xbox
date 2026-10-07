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
Für Fenix und die experimentelle GSX-Pro-Integration gibt es eigene Abläufe,
derzeit nur für MSFS 2024.

## Community-Mod deinstallieren

Beende den Simulator und laufende Einrichtungen. Wähle in **Mods** beim gewünschten
Paket **Deinstallation prüfen**. Kontrolliere Ordner und Größe, wähle
**Deinstallieren** und bestätige. Ein echter Paketordner wird dauerhaft mit allen
darin gespeicherten Einstellungen gelöscht. Bei einer Verknüpfung entfernt
Flightdeck nur den Community-Eintrag; die Originaldateien bleiben erhalten.
**Abbrechen** lässt das Paket unverändert. Hat sich der Ordner seit der Vorschau
geändert, verlangt Flightdeck eine neue Prüfung.

Fenix und GSX vollständig über ihren offiziellen Installer/Manager im jeweiligen
Add-on-Bereich entfernen. Die Community-Deinstallation entfernt keine zugehörigen
Windows-Begleitprogramme.

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

## GSX Pro einrichten (experimentell)

Ab **0.1.19** öffnet **Mods → GSX Pro** einen Ablauf mit drei Schritten.
Die anderen Add-on-Karten bleiben dabei eingeklappt. Der offizielle FSDT-Installer wurde mit
nativem .NET 4.8 in einem separaten MSFS-2024-Windows-Profil geprüft, einschließlich
Öffnen, Schließen und Freigeben der Installation. GSX-Installation, Aktivierung,
Couatl/SimConnect, das Menü im Simulator und die Bodendienste müssen noch mit
einer lizenzierten Kopie getestet werden.

Ab 0.1.20 verwendet die FSDT-Einrichtung die aktive Proton-Version. Installierte
Dateien und Einstellungen werden beim Wechsel übernommen. Eine unterbrochene
GSX-Vorbereitung muss vor einem Proton-Wechsel wiederhergestellt werden.

MSFS 2024 einmal starten, damit die Paketkonfiguration vorhanden ist. Danach den
Simulator, Fenix und andere Windows-Anwendungen dieses Profils schließen.

1. **FSDT vorbereiten** lädt den offiziellen Installer mit festgelegter SHA-256,
   kopiert das Windows-Profil und richtet bei Bedarf .NET 4.8 ein. Flightdeck
   registriert FSDTs Lizenzkomponente und bewahrt das bisherige Profil als
   Sicherung auf. Der Spiel-Runner bleibt unverändert. Das kann mehrere Minuten
   dauern.
2. **FSDT-Installer öffnen**, GSX Pro auswählen und FSDTs Installation und
   Aktivierung abschließen. Prüfen, ob Simulator und Community-Pfad zur gewählten
   Flightdeck-Installation passen. Laufende Downloads abschließen und den
   Installer schließen oder in Flightdeck **FSDT schließen** wählen. FSDT verwaltet
   seine Paketverknüpfungen selbst; nicht die gesamte Installation manuell nach
   Community kopieren.
3. **Automatischen Start einrichten** wird verfügbar, sobald das GSX-Paket im
   Community-Ordner und FSDTs Couatl-Eintrag in `exe.xml` erkannt werden. Flightdeck
   aktiviert diesen Eintrag und erhält dessen Argumente sowie andere Add-ons.
   Fehlt der Eintrag, im FSDT-Installer aktualisieren und den Status neu laden.
   Anschließend MSFS starten und GSX-Menü sowie Bodendienste prüfen.
   **GSX eingerichtet · Flugtest ausstehend** bestätigt nur die lokale Einrichtung,
   weder die Lizenz noch die Linux-Kompatibilität.

GSX ist kostenpflichtig. Laut [offiziellem GSX-Handbuch](https://www.fsdreamteam.com/gsx_manual_msfs.pdf)
setzt die Installation eine GSX-Lizenz oder einen berechtigten, aktivierten
FSDT-Flughafen voraus. Mit Letzterem sind die Nutzung an FSDT-Flughäfen und ein
begrenzter Test an KSFO, LIMC und EDDM möglich. Die Vorbereitung in Flightdeck
schaltet GSX nicht frei. Kauf und Aktivierung erfolgen bei
[FSDreamTeam](https://www.fsdreamteam.com/products_gsxpro.html).

**GSX-Autostart ausschalten** ändert nur den Couatl-Eintrag und deinstalliert keine
Pakete. Bei eingerichtetem GSX-Autostart schließt Flightdeck die benannten
Couatl-Begleitprozesse dieses Profils auch bei Spielende, Absturz oder Stoppen.
Andere Profile und der FSDT-Installer gehören nicht zu dieser Sitzungsbereinigung.

Nach unterbrochener Vorbereitung bleibt der Spielstart gesperrt, bis
**GSX-Vorbereitung wiederherstellen** das bisherige Profil wieder aktiviert.
Eine abgeschlossene Vorbereitung bewahrt `local/msfs-prefix.before-gsx-*` als
Sicherung auf. Die Wiederherstellung ist für unterbrochene Vorbereitung gedacht,
nicht zum Rückgängigmachen einer späteren GSX-Installation. Protokolle liegen in
`private/gsx-setup-*.log` und `private/gsx-manager.log`. Ersetzt FSDT den
Installer-Download, stoppt ein Prüfsummenfehler die Vorbereitung, bis Flightdecks
festgelegte Version geprüft und aktualisiert wurde. FSDT- und Microsoft-Binärdateien
werden nicht mitgeliefert.

## Fenix A320 einrichten

Nach dem Entpacken der FenixApp kann der offizielle Installer **„application
install hook failed“** melden. Flightdeck setzt den vorhandenen ICU-/.NET-Fix,
wenn die EXE über **Mods → Fenix → Installer starten** ausgeführt wird; dieser
Workaround ist bereits in 0.2.5 enthalten. Vor einem
erneuten Versuch alle Installer-Fenster schließen.

**In 0.2.6:** Bei erkannter FenixApp führt
**Fenix-App öffnen** direkt in den vorhandenen Manager. **Fenix-App reparieren**
wiederholt bei gültigen Paketdaten den offiziellen Einrichtungsschritt, ohne das
Profil zurückzusetzen. Anschließend den Manager öffnen und das Flugzeug
installieren. Scheitert die Reparatur, enthält ein neuer Diagnosebericht den
aktuellen Fenix-Befund. Ein erkannter Manager oder erfolgreicher Hook bestätigt
noch keine Flugzeuginstallation oder Aktivierung.
[Prüfumfang](native-ui.md#fenix-install-hook-warning).

Unter **Mods → Fenix A320** bietet Flightdeck **0.1.4** einen eigenen
Einrichtungsablauf. Ältere Launcher mit dem [vollständigen Paket](install.md)
aktualisieren. Derselbe Patch ist auch als
[eigenständiger Patch-Installer](https://github.com/marselnenaj/fenix-a320-linux-patch/releases)
verfügbar.

Der Patch unterstützt MSFS 2024 mit dem festgelegten Xodus-Wine-Runner. Starte
MSFS 2024 einmal, damit die Benutzereinstellungen angelegt sind. Schließe danach
MSFS und alle Fenix-Anwendungen.

Vorhandene Patch-Installationen mit preview.1 oder preview.2 werden beim Öffnen von Fenix über
diesen Launcher automatisch aktualisiert. **Patch aktualisieren** startet das
Update auch direkt. Flugzeug, Einstellungen und der ursprüngliche
Wiederherstellungspunkt bleiben erhalten; Fenix muss nicht neu installiert werden.

1. **Patch einrichten** lädt das Linux-ZIP aus dem öffentlichen Fenix-GitHub-Release
   und prüft dessen SHA-256. Es erstellt eine eigene Runner-/Profilkopie, bewahrt
   die bisherige Umgebung auf und installiert bei Bedarf Microsoft .NET Framework 4.8
   sowie die separat heruntergeladene, geprüfte Geometrie-Abhängigkeit.
2. Den offiziellen Installer aus dem [Fenix-Konto](https://fenixsim.com/dashboard/)
   herunterladen, seine EXE auswählen und **Installer starten** wählen. Die normale
   Installation samt angebotenen Voraussetzungen im ausgewählten Simulatorprofil
   abschließen und den Installer danach schließen.
3. **Fenix öffnen**, anmelden und aktivieren. Danach Fenix wieder schließen
   oder in Flightdeck **Fenix beenden** wählen.
4. **Einrichtung abschließen** setzt CPU-Anzeigen, Legacy-Readouts und Autostart.
   Sobald die grüne Meldung **Fenix ist startbereit** erscheint, zur Übersicht
   wechseln und MSFS ganz normal starten.

Flightdeck markiert erledigte Schritte und hebt die nächste Aktion hervor.
Das Beenden des offiziellen Installers allein schließt die Einrichtung noch
nicht ab. Läuft noch eine Windows-Anwendung, den Fenix-Installer und Fenix
vollständig beenden; die Anzeige aktualisiert sich automatisch. Deine Anmeldung
und Lizenz prüft Fenix selbst. Falls im Loginfenster `AltGr+Q` kein `@` eingibt,
`Strg+Alt+Q` versuchen oder `@` kopieren und mit `Strg+V` einfügen.

**Fenix beenden** erscheint, wenn Fenix im ausgewählten Profil läuft. Der Knopf
schließt Fenix, seine Helfer und den offiziellen Manager. Erst danach gibt
Flightdeck die nächsten Schritte frei. Während MSFS oder ein Installer läuft,
bleibt der Knopf gesperrt. Laufende Installationen und Livery-Downloads im
offiziellen Manager vor dem Beenden abschließen.

Nach der Einrichtung startet Fenix automatisch mit MSFS. Du musst es nicht
separat öffnen. Beim normalen Spielende, einem Absturz oder **Stoppen** beendet
Flightdeck die Fenix-Begleitprozesse dieser Sitzung; hängende Prozesse werden
nach einer kurzen Wartezeit beendet. Der offizielle Fenix-Installer und andere
Wine-Profile bleiben davon unberührt.

Flightdeck verwendet Patch **0.1.0-preview.3**. Neuere Patch-Versionen werden
mit einem Flightdeck-Update übernommen; der Launcher sucht nicht eigenständig
nach dem neuesten Fenix-Patch auf GitHub. Unterstützte ältere Patches werden
beim Öffnen von Fenix, seinem Installer oder Manager sowie beim Abschließen der
Einrichtung auf die mit Flightdeck bereitgestellte Version aktualisiert.
Eine Flightdeck-Installation richtet Fenix nicht automatisch ein.
Für den öffentlichen Patch-Download ist keine GitHub-Anmeldung nötig. Das
Flugzeug selbst wird mit dem offiziellen Fenix-Programm heruntergeladen und über
dein Fenix-Konto aktiviert.

Für eine lokale Kopie das **Linux-Installer-ZIP** entpacken und unter **Lokales
Patch-Paket und Wiederherstellung** den Ordner mit `bundle.json` auswählen.
Der GitHub-Quellcode oder das reine Quellarchiv enthält die Wine-Binärdateien nicht.
Ein leeres Feld verwendet den geprüften Download bzw. Cache. Der eigenständige
Installer `install.sh` bietet denselben Ablauf ohne Flightdecks Oberfläche; seine
grafische Oberfläche benötigt Python Tk. Flightdecks Fenix-Bereich benötigt kein Tk.

### Liveries

**Installer & Liveries** startet den bereits installierten offiziellen Manager
für Flugzeuginstallation, Updates und Liveries. **Fenix öffnen** startet die
Fenix-Hauptanwendung. Der Manager-Button wird verfügbar, sobald Flightdeck ihn im
ausgewählten Wine-Profil erkennt. Vorher den
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
vollständiges natives Paket gilt weiterhin glibc 2.39+. Ab 0.1.20 gibt es passende
Fenix-Patches für Proton Experimental 11.0 (20260924) und CachyOS Proton 10.0
sunset. Die nativen Prüfungen bestehen; ein vollständiger Flug mit diesen
Versionen ist noch nicht bestätigt. Weitere Builds brauchen einen passenden
Patch. Steam-Prefixe und MSFS 2020 bleiben außerhalb des unterstützten Umfangs.
Der Fensterhelfer läuft mit dem Simulator und blendet passende Fenix-Dienst-/
Anzeigefenster aus. Die Fenix-Hauptanwendung bleibt für die Anmeldung zugänglich.
Manuelles Öffnen von Fenix startet den Helfer nicht.

**Vorhandener lokaler Fenix-Patch** bezeichnet eine frühere Entwickler-Einrichtung.
Sie bleibt aktiv, bis in Flightdeck eine unterstützte Proton-Version ausgewählt
wird. Beim Wechsel werden bekannte Startskripte übernommen und der passende
Fenix-Patch eingerichtet. Flugzeuge und Einstellungen bleiben erhalten. Eigene
Skriptänderungen werden nicht überschrieben. Gesperrte Schritte können außerdem
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

Patch preview.3 korrigiert fehlende Routen, überlange Linien und die
X11-/Xwayland-Hilfsfenster. Nach einem Display-Neustart werden festgehaltene dunkle
MCDU-Bilder automatisch aufgefrischt. Der Helfer schaltet kurz die Einstellung
für Pop-out-Anzeigen um und stellt sie wieder her. Bei maximaler Helligkeit
nutzt er einen DIM/BRT-Wechsel. Seiten und Flugplandaten bleiben unverändert.
Der offizielle Display-Neustart wurde damit erfolgreich geprüft.
[Details zum Anzeige-Helfer](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/main/docs/mcdu-restart.md).
