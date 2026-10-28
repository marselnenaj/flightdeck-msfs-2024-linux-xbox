# Steam-Installationen (Beta)

Flightdeck erkennt die Steam-Versionen von MSFS 2020 und 2024 auch in weiteren
Steam-Bibliotheken. Xbox- und Steam-Installationen derselben Ausgabe lassen sich
getrennt auswählen. Die vorhandene Xbox-Konfiguration bleibt beim Wechsel und
für eine Rückkehr zur bisherigen Flightdeck-Version erhalten.

Steam-Unterstützung ist Beta. Lokale Tests prüfen Erkennung, native und
Flatpak-Prozesszuordnung, Profilwechsel, den gemeinsamen Fenix-Installer,
unterbrochene Übernahmen und das gezielte Beenden von Begleitprogrammen. Ein
Offline-Test mit echtem Wine prüft außerdem ein frisches Steam-Profil, das
Patch-Update von 0.1.0 auf 0.1.1, den offiziellen FenixApp-1.0.359-
Installationsschritt und die Wiederherstellung mit erhaltenen Packages-Dateien. Echte
Steam-MSFS-Flüge auf verschiedenen Clients, GPUs und Distributionen müssen
Tester noch bestätigen. Auf dem Entwicklungsrechner ist kein Steam-MSFS
installiert.

## Auswählen und starten

1. Installiere MSFS über Steam. Flightdeck durchsucht natives Steam, Flatpak und
   Snap sowie deren registrierte Bibliotheken. Wähle im Installationsmenü die
   genaue Ausgabe und Plattform.
2. Starte MSFS einmal über Steam und beende es wieder. Die Erkennung funktioniert
   vorher bereits; für Fenix muss das Proton-Profil vorhanden sein.
3. **MSFS starten** beauftragt den passenden Steam-Client. Flightdeck beobachtet
   den tatsächlichen Simulatorprozess, auch nach einem Launcher-Neustart.
4. **MSFS beenden** beendet nur diesen Simulator in seinem Profil. Steam und
   andere Spiele laufen weiter. Falls der Prozess nicht endet, schließe das Spiel
   direkt über Steam.

Getrennte oder verschobene Bibliotheken bleiben sichtbar. Suche erneut und wähle
den neuen Fund. Veraltete Ansichten können kein ersetztes Profil verändern.
Einen übersehenen Steam-Client kannst du unter **Einrichtung → Xbox- und
Steam-Installationen → Steam-Ordner manuell angeben** hinzufügen. Wähle den
Clientordner, in dem Steam seine Bibliotheksliste verwaltet. Flatpak- und Snap-
Ordner müssen zum gewählten Client gehören. Ist dieselbe App-ID in mehreren
Bibliotheken registriert, bereinige zuerst die veraltete Installation in Steam:
Ein Start über die App-ID kann keine bestimmte Bibliothek auswählen.

Steam übernimmt Spielinstallation, Lizenz, Spielupdates, Startoptionen und seine
Synchronisierung. Flightdecks Xbox-Einrichtung, Store-Prüfung und Xbox-Cloud-
Import/-Upload stehen für eine Steam-Installation nicht zur Verfügung.
Vorhandene Steam-Startoptionen und die Plattform-Synchronisierung bleiben bestehen.

## Fenix einrichten

Öffne **Mods → Fenix Linux-Patch**. Ein vorhandener Patch des eigenständigen
[Fenix-Installers](https://github.com/marselnenaj/fenix-a320-linux-patch) wird
direkt erkannt. Beide Programme verwenden denselben fest gepinnten Rust-Code
für Einrichtung, Konfiguration, Proton-Zuordnung, Update und Wiederherstellung.

1. Beende MSFS, Fenix und Steam vollständig, bevor du den Patch oder seine
   Proton-Zuordnung änderst. Laufendes Steam könnte seine Konfiguration sonst
   wieder überschreiben.
2. Richte den Patch ein und wähle **Fenix-Proton für Steam auswählen**. Nur die
   Zuordnung dieses Simulators wird geändert. Ein neues Kompatibilitätswerkzeug
   erscheint gegebenenfalls erst nach einem Steam-Neustart in dessen Oberfläche.
3. Starte die offizielle Fenix-Installer-EXE aus deinem Fenix-Konto. Warte, bis
   die Installation einschließlich gestarteter Unterprogramme fertig ist, und
   schließe den offiziellen Installer.
4. Öffne Fenix für seine Anmeldung und Aktivierung. Beende es anschließend und
   wähle **Anzeigen und Autostart einrichten**.

Ein verwalteter Proton pro unterstützter Variante genügt; MSFS 2020 und 2024
können ihn gemeinsam verwenden. Wiederholte Einrichtung verwendet ihn erneut.
Ein Update verändert ein geteiltes Werkzeug nur, wenn auch seine anderen Profile
beendet sind. Die Wiederherstellung einer Ausgabe entfernt keinen Proton, den
die andere Ausgabe noch benötigt.

Updates behalten das aktuelle Profil. Bei der ersten Einrichtung wird das
Originalprofil gesichert; große Packages-Inhalte werden verschoben statt
dupliziert. Die Wiederherstellung behält das neuere Profil als Sicherung.
Nach unterbrochener Vorbereitung ist eine Wiederherstellung möglich. Lösche
keine unbekannten Profile oder Sicherungen, um eine fehlgeschlagene Prüfung
zu umgehen.

Bei einer teilweise erfolgreichen offiziellen Installation erfasst Flightdeck
das Ergebnis des aktuellen Einrichtungsschritts. **Fenix-App reparieren** führt
den offiziellen Installations-Hook erneut aus, installiert aber kein Flugzeug.
Prüfe nach einer fehlgeschlagenen Reparatur zuerst die Diagnose. Eine
`FileNotFoundException` ohne ihre Meldung benennt die fehlende Datei nicht.
Exportierte Berichte enthalten begrenzte Fehlertypen und Exit-Codes; die genaue
Drittanbieter-Meldung steht im privaten lokalen Fenix-Protokoll.

Fenster- und MCDU-Helfer begleiten einen erkannten Simulator mit geprüftem
Fenix-Proton. Das Schließen von Flightdeck beendet keinen Steam-Flug. Nach einem
bestätigten Spielende werden Fenix-Begleitprogramme nur dieses Profils beendet.

## Community und Spielstände

Der Community-Ordner wird aus der `UserCfg.opt` des gewählten Steam-Profils und
seinen Wine-Laufwerken ermittelt. Widersprüchliche Konfigurationen werden
angezeigt. Community-Deinstallation ist in dieser
Steam-Beta noch nicht aktiviert; verwende dafür den offiziellen Add-on-Installer.

Für Spielstände bleibt das bisherige Verhalten der Plattform erhalten.
Flightdecks Xbox-Cloud-Abgleich und Xbox-Spielstandsicherungen laufen nicht
gegen Steam-Profile. Fenix-Wiederherstellungspunkte sind davon getrennt.

Melde nachvollziehbare Probleme in [GitHub Issues](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues)
oder im [Discord](https://discord.gg/fAhHR2DkqN) mit MSFS-Ausgabe, Steam-Client,
Distribution, GPU und fehlgeschlagener Aktion.

## GSX, Navigraph und Orbx ab 0.3.3

Unter **Mods** stehen die experimentellen GSX-, Navigraph-Hub/Simlink- und
Orbx-Central-Abläufe auch für Steam-MSFS 2020/2024 bereit. Sie verwenden das echte
Profil und dessen ausgewähltes Proton, auch in zusätzlichen Bibliotheken. Zuerst
Proton in Steam wählen und MSFS einmal starten, dann Spiel und andere
Windows-Anwendungen dieses Profils vor der Einrichtung schließen. Ein weiteres
dauerhaftes Wine-Profil wird nicht angelegt. [Anleitung und Testumfang](addons.de.md)

Metadaten und Protokolle liegen unter
`~/.local/share/flightdeck/steam/<installation-id>/addon-apps/`. GSX behält eine
Sicherung `pfx.before-gsx-*` neben dem echten Profil. Unterbrochene Vorbereitung
muss vor einem Spielstart wiederhergestellt werden. Nach einem Proton-Wechsel
oder Profiltausch den Status neu laden. Simlink und Couatl verwenden die echte
`exe.xml`; nach bestätigtem Spielende werden nur aktivierte Begleitprogramme
dieses Profils beendet. Vollständige Steam-Flüge und lizenzierte Add-on-Funktionen
müssen Tester noch bestätigen.
