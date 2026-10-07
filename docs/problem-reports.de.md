# Ein Problem per E-Mail melden

[English](problem-reports.md)

Verfügbar ab Flightdeck 0.1.9.

Öffne **Diagnose → Problem melden**, wähle eine Kategorie, beschreibe den Fehler
und klicke auf **Bericht vorbereiten**. Bei Grafikfehlern kannst du zusätzlich
ankreuzen, was du gesehen hast. Flightdeck speichert den Bericht lokal und zeigt
den E-Mail-Text zur Prüfung an.

**E-Mail-Entwurf öffnen** trägt `contact@flightdeck-app.com`, den Betreff, deine
Beschreibung und die erfassten Diagnosedaten ein. Ein Anhang oder GitHub-Konto
ist nicht nötig. Prüfe den Entwurf im Mailprogramm und klicke dort auf
**Senden**. Flightdeck versendet selbst keine E-Mails und kann den Versand nicht
bestätigen.

Für Webmail, ohne eingerichtetes Mailprogramm oder bei einem zu langen
Mail-Link kannst du **E-Mail-Text kopieren** verwenden und den Text in eine
Nachricht an die angezeigte Adresse einfügen. Wenn die Zwischenablage nicht
verfügbar ist, lässt sich der vollständige Text direkt markieren und kopieren.
**Als Text speichern** lädt optional eine `.txt`-Datei herunter. Der Bericht
wird nicht still gekürzt.

## Inhalt und Speicherung

Erfasst werden deine Beschreibung, optionale Beobachtungen, Berichtsnummer und
Zeitpunkt sowie Distribution, Kernel, Architektur und verfügbare GPU- und
Treiberdaten. Dazu kommen die freigegebenen Diagnosedaten der letzten
verfügbaren Spielsitzung der ausgewählten Installation: Launcher-Version,
Grafikkomponenten und Einstellungen, numerische Anmelde-, Store- und
Cloud-Fehler sowie die letzte Store-Prüfung. Der aktuelle Cloud-Dienststatus
ist ausdrücklich getrennt vom letzten Spiellog gekennzeichnet.

**In 0.2.6:** Berichte enthalten außerdem den
letzten erfassten Fenix-Installer-, Manager- oder Reparaturversuch der ausgewählten
Installation: Zeitpunkte, geprüfte App-Version, Runner-Kategorie, Exitcodes und
eine feste Fehlerkategorie. Fenix-Logtext wird nicht übernommen. Nach dem Fehler
einen neuen Bericht vorbereiten; alte Berichte enthalten diesen Befund nicht
rückwirkend.

Flightdeck übernimmt keine rohen Spiellogs, Kontozugangsdaten, Anmeldetokens,
Benutzernamen, lokalen Pfade, Hardware-UUIDs oder Spielstandinhalte. Das
Beschreibungsfeld ist Freitext: Füge dort keine Passwörter oder privaten Logs
ein. Beim Öffnen des Entwurfs wird der Bericht an deinen gewählten
E-Mail-Handler übergeben; bei Webmail kann das ein Webdienst sein.

Der Bericht beschreibt die verfügbaren Daten **beim Vorbereiten**, nicht einen
unabhängig aufgezeichneten Zustand im Moment des Fehlers. Statusabfragen,
Spielwechsel und Launcher-Neustarts verändern ihn nicht. Nach Änderungen an
der Beschreibung musst du ihn erneut vorbereiten. Ein neuer Bericht ersetzt
den bisherigen Entwurf.

Die interne Datei `problem-report.json` liegt im Flightdeck-Zustandsordner
(normalerweise `$XDG_STATE_HOME/flightdeck` oder `~/.local/state/flightdeck`)
und ist nur für den Besitzer zugänglich. E-Mail und optionaler Download sind
Klartext. **Lokalen Entwurf löschen** entfernt diese gespeicherte Kopie;
Downloads und E-Mail-Entwürfe bleiben bestehen.

## Empfang einrichten

Offizielle Builds verwenden `contact@flightdeck-app.com`. Die Berichte kommen
als normale E-Mails an; Kategorie und Berichtsnummer stehen im Betreff.
Antworten gehen an den Absender. Der Launcher benötigt keinen Upload-Dienst,
kein SMTP-Passwort und keinen API-Schlüssel. Es entsteht kein zusätzlicher
kostenpflichtiger Meldedienst. Das vorhandene Postfach muss erreichbar sein.

Andere Builds können beim Start `FLIGHTDECK_SUPPORT_EMAIL` auf eine einzelne
gültige E-Mail-Adresse setzen. Ein leerer oder ungültiger Wert deaktiviert den
Mail-Link; Erstellen, Vorschau, Kopieren und Download bleiben verfügbar.
