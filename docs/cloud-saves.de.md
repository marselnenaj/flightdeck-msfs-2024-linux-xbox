# Xbox-Cloud-Spielstände

[English](cloud-saves.md)

Flightdeck verwendet standardmäßig deine Xbox-Cloud-Spielstände, wenn du MSFS
im Launcher startest. Vor dem Start wird der Cloud-Stand geladen, lokale
Sicherungen bleiben erhalten und nach dem Beenden werden Änderungen
hochgeladen. Die Funktion ist experimentell. Entscheidend ist das Xbox-Profil,
mit dem du im Spiel angemeldet bist.

Mit der Simulatorauswahl in Flightdeck 0.1.1 bestimmt die ausgewählte Runtime
auch die Cloud-Spielkennung, das Hilfsprofil und die lokalen Backups.
MSFS-2020- und MSFS-2024-Spielstände sind nicht austauschbar. Der neue 2020-Pfad
benötigt noch einen vollständigen Spieltest; siehe [Veröffentlichungsstand](changelog.md).

## Einfach den Simulator starten

1. Das aktuelle vollständige Flightdeck-Paket installieren und **Simulator
   starten** wählen.
2. Flightdeck sichert den lokalen Stand und prüft die Cloud. Beim ersten
   Abgleich wird ein vorhandener Cloud-Stand verwendet. Eine leere Cloud
   entfernt keine bereits vorhandenen lokalen Spielstände.
3. Nach dem Abgleich startet der Simulator automatisch.
4. Nach dem Beenden sichert Flightdeck den lokalen Stand, lädt Änderungen hoch
   und liest sie zur Prüfung erneut zurück. Erst dann erscheint die Meldung,
   dass deine Spielstände synchronisiert sind.

Du musst nicht bei jedem Start einen Stand auswählen oder eine Kopie laden.
Der lokale Spielstandspeicher wird automatisch aktiviert. Das Öffnen des
Launchers oder der Spielstandseite allein überträgt keine Daten; der normale
Spielstart löst den Ablauf aus. Beim Schließen des Launcherfensters läuft der
lokale Dienst weiter und kann eine gestartete Sitzung fertig synchronisieren.

Bei weiteren Starts prüft Flightdeck den aktuellen Cloud-Index und lädt nur
geänderte Spielstandcontainer herunter. Unveränderte Daten kommen aus einer
lokal per Prüfsumme geprüften Kopie desselben Xbox-Profils und derselben
Cloud-Version. Fehlende oder beschädigte Kopien werden neu geladen. Jeder
geschriebene Container wird zur Upload-Prüfung vollständig zurückgelesen, auch
bei unveränderter gemeldeter Version. Für andere Container muss die aktuelle
Cloud-Version exakt übereinstimmen. Der vollständige Index wird vor und nach
jedem Lesevorgang geprüft.

Nach sauberem Beenden behält Flightdeck die separate Wine-Umgebung seines
Cloud-Hilfsprozesses. Dadurch entfällt deren erneute Einrichtung bei jedem
Abgleich. Xbox-Konto und Spielidentität werden bei jeder Verbindung neu geprüft.

## Wann Flightdeck nachfragt

Der letzte geprüfte Abgleich dient als gemeinsamer Vergleichsstand.
Einseitige Änderungen werden automatisch übernommen. Unabhängige Änderungen
in verschiedenen Containern lassen sich zusammenführen. Wurde derselbe
Container auf beiden Seiten unterschiedlich geändert, fragt Flightdeck nach
Cloud- oder lokalem Stand. Zeitstempel bestimmen nicht den Gewinner.
Die Auswahl gilt nur für die konkret verglichenen Versionen; bei Änderungen
oder abgelaufenen Vergleichen muss erneut geprüft werden.

Fehlen Anmeldung oder Verbindung, kannst du erneut versuchen oder ausdrücklich
mit lokalen Spielständen starten. So kannst du MSFS auch zur ersten
Xbox-Anmeldung öffnen. Lokale Sitzungen werden gesichert und beim nächsten
normalen Start erneut abgeglichen. Ein Anmeldefehler gilt niemals als leere
Cloud. Verwende dasselbe Spielprofil währenddessen nicht auf einem anderen
Gerät: Dortige Änderungen können vor dem abschließenden Upload eine Auswahl
erforderlich machen.

Scheitert ein Upload nach dem Beenden, kannst du auch dann **Mit lokalen
Spielständen spielen** wählen. Flightdeck startet eine neue lokal gesicherte
Sitzung, statt denselben Upload immer wieder vorauszusetzen. Der ausstehende
Abgleich bleibt erhalten. Beim nächsten Cloud-Versuch kann neuer lokaler
Fortschritt eine ausdrückliche Auswahl zwischen Cloud und lokalem Stand
erfordern, auch nach einem teilweise abgeschlossenen Upload. Ist der lokale
Stand, die Kontozuordnung oder das Ende des Spielprozesses unklar, wird diese
Option nicht angeboten. Die Diagnose enthält Abgleichphase, Fehlerkategorie
und verfügbare numerische HTTP-/HRESULT-Angaben ohne Antworttexte oder Kontokennung.

## Sicherungen und Unterbrechungen

Flightdeck behält Sicherungen vor dem Spielen, vor lokalen Übernahmen und vor
Uploads. Die Runtime bleibt während Abgleich, Spielsitzung und Upload gegen
einen weiteren Start oder Wartungsvorgang gesperrt. Während der Übertragung
ist außerdem der native Spielstandschreiber gesperrt.

Uploads verwenden die normale Xbox-Speichersperre und verdrängen kein anderes
Gerät. Einzelne Container werden atomar aktualisiert, die gesamte Sammlung
jedoch nicht. Bei einem Verbindungsabbruch können Teile bereits hochgeladen
sein. Der dauerhafte Sitzungsnachweis und die gesicherten Daten bleiben
erhalten; vor dem nächsten Versuch wird die Cloud frisch gelesen. Ein unklarer
Schreibvorgang wird nicht blind wiederholt und ausstehender Fortschritt nicht
unbemerkt durch den Cloud-Stand ersetzt.

Wird der Launcher oder sein Spielstartprozess hart beendet, kann vor einer
weiteren automatischen Sitzung ein Linux-Neustart erforderlich sein. Dadurch
werden Spielstände nicht verändert, während ein verwaister Spielprozess noch
laufen könnte. Nach normalem Beenden oder dem Stoppknopf ist kein Neustart nötig.

## Erweiterte Werkzeuge

Unter **Spielstände → Erweiterte Optionen** findest du manuellen Vergleich,
Übernahme, Upload und getrennte Cloud-Kopien. Für den normalen Spielstart sind
sie nicht nötig. Eine manuelle Übertragung ersetzt den vollständigen Stand des
ausgewählten Spielprofils einschließlich bestätigter Löschungen. Vor einer
leeren Quelle warnt der Launcher. **Letzte Übernahme rückgängig machen** stellt
in der laufenden Launcher-Sitzung den vorherigen lokalen Stand wieder her,
solange das Spiel ihn seitdem nicht verändert hat. Die Cloud bleibt dabei gleich.

## Private Daten

Die Daten bleiben im ausgewählten Runtimeordner:

Beim Umziehen einer vorhandenen Runtime gehört der vollständige Ordner
`private/` zusammen. Beende vorher das Spiel und den Cloud-Abgleich. Wird nur
`local-saves/` kopiert, können die Nachweise und Sicherungen des letzten
Abgleichs fehlen. Flightdeck kann den Vergleich dann nicht prüfen. Die passenden
Originaldateien müssen mit ihren privaten Zugriffsrechten erhalten bleiben;
heruntergeladene Cloud-Kopien ersetzen diese Nachweise nicht.

| Pfad | Inhalt |
| --- | --- |
| `private/cloud-saves/` | Getrennte Cloud-Kopien, auch vom Stand vor einem Upload |
| `private/cloud-cache/` | Profilgebundene Verweise auf geprüfte Kopien für schnellere Starts |
| `private/cloud-helper-prefixes/` | Separate, versionsgebundene Wine-Umgebung des Cloud-Hilfsprozesses |
| `private/cloud-import-backups/` | Lokale Spielstände vor Übernahme und Rücknahme |
| `private/cloud-import-receipts/` | Nachweise für geprüfte abgeschlossene Übertragungen |
| `private/local-saves/` | Aktive lokale Spielstände und Vergleichsstand |
| `private/save-backups/` | Lokale Sicherungen vor und nach Spielsitzungen |
| `private/cloud-sessions/` | Profilgebundene ausstehende Übertragungen und Wiederherstellungsnachweise |
| `private/cloud-offline.pending` | Schutz für unbestätigten Fortschritt, auch bei Kontowechseln im Spiel |
| `private/connected-storage-device.seed` | Zufällige lokale Kennung für die Cloud-Sperre |

Diese Dateien enthalten private Daten und gehören nicht in Fehlerberichte,
Quellcode-Exporte oder Releases. Kontotokens, Signaturen und temporäre
Upload-Adressen bleiben in der nativen Komponente. Die heruntergeladenen
Datenblöcke nicht manuell über eine `state.bin` kopieren: Die Formate unterscheiden
sich.

## Prüfstand

Tests mit Beispieldaten prüfen Übernahme und Rücknahme, Profilbindung,
Dateizugriffe, Sperrverlust, teilweise Uploads, Abbruch, Rücklesen, gespeicherte
Vergleichsstände nach einem Neustart, automatischen Spielstart und Abgleich nach
dem Beenden sowie die deutsche und englische Oberfläche.
Mit einem echten MSFS-Profil wurden außerdem der vollständige Cloud-Download,
die normale Speichersperre sowie Upload, Rücklesen und Löschen eines eigenen
32-Byte-Testcontainers geprüft. Der Testcontainer wurde entfernt; sämtliche
vorhandenen Cloud-Spielstände und lokalen Spielstände blieben unverändert.
Ein Spieltest von Windows/Xbox nach Linux und zurück steht vor einer Einstufung
als stabile Funktion noch aus.

Technische Belege und Protokollquellen stehen in der
[Protokolldokumentation](connected-storage-protocol.md). Die Implementierung ist
eigenständig; Universal Title Storage wird nicht als Ersatzdienst verwendet.
