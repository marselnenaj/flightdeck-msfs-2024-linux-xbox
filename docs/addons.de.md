# Add-ons installieren

[English instructions](addons.md)

**Seit 0.3.3:** GSX, Navigraph Hub/Simlink und Orbx Central verwenden das
vorhandene Wine-Profil und Proton der gewählten Xbox- oder Steam-Installation von
MSFS 2020/2024. Diese Integrationen bleiben experimentell. Für Steam zuerst Proton
in den Kompatibilitätseinstellungen wählen und MSFS einmal starten. Ein weiteres
dauerhaftes Wine-Profil ist nicht nötig. Spieldownloads und Updates verwaltet Steam.
Steam-Metadaten, Protokolle und XML-Sicherungen liegen unter
`~/.local/share/flightdeck/steam/<installation-id>/addon-apps/`; Programme und
Simulator-Einstellungen bleiben im echten `compatdata/<AppID>/pfx`. Nach einem
Proton-Wechsel den Add-on-Status neu laden. Community-Pakete lassen sich über
Flightdeck weiterhin nur bei Xbox entfernen.

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
Für Fenix und die experimentellen GSX-, Navigraph- und Orbx-Integrationen gibt es
eigene Abläufe. Die drei letzteren unterstützen beide Editionen und Plattformen;
Fenix für Steam ist in der [Steam-Anleitung](steam.de.md) beschrieben.

Seit 0.2.7 zeigt Flightdeck für jedes Add-on einen hervorgehobenen nächsten Schritt.
Erkannte Komponenten bleiben sichtbar; zusätzliche Aktionen stehen unter
**Verwalten & reparieren**.

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

Öffne **Mods → GSX Pro** und folge dem hervorgehobenen nächsten Schritt.
Erledigte Schritte bleiben zusammengefasst; zusätzliche Aktionen findest du im
eingeklappten Bereich **Verwalten & reparieren**. Vor der Einrichtung MSFS, Fenix
und andere Windows-Anwendungen im ausgewählten Profil schließen. Die gewählte MSFS-Edition muss
einmal gestartet worden sein, damit seine Paketkonfiguration vorhanden ist.

GSX bleibt experimentell. Der offizielle FSDT-Installer wurde mit nativem .NET 4.8
in einem separaten MSFS-2024-Windows-Profil geprüft, einschließlich Öffnen,
Schließen und Freigeben der Installation. GSX-Installation, Aktivierung,
Couatl/SimConnect, das Menü im Simulator und die Bodendienste sind mit einer
lizenzierten Kopie noch nicht bestätigt.

Ab 0.1.20 verwendet die FSDT-Einrichtung die aktive Proton-Version. Installierte
Dateien und Einstellungen werden beim Wechsel übernommen. Eine unterbrochene
GSX-Vorbereitung muss vor einem Proton-Wechsel wiederhergestellt werden.

1. **FSDT vorbereiten** lädt den offiziellen Installer mit festgelegter SHA-256,
   kopiert das Windows-Profil und richtet bei Bedarf .NET 4.8 ein. Flightdeck
   registriert FSDTs Lizenzkomponente und bewahrt das bisherige Profil als
   Sicherung auf. Der Spiel-Runner bleibt unverändert. Das kann mehrere Minuten
   dauern.
2. **FSDT-Installer öffnen**, GSX Pro auswählen und FSDTs Installation und
   Aktivierung abschließen. Prüfen, ob Simulator und Community-Pfad zur gewählten
   Flightdeck-Installation passen. Laufende Downloads abschließen und den
   Installer schließen oder Flightdecks Aktion zum Schließen verwenden. FSDT verwaltet
   seine Paketverknüpfungen selbst; nicht die gesamte Installation manuell nach
   Community kopieren.
3. **Automatischen Start einrichten** wird verfügbar, sobald das GSX-Paket im
   Community-Ordner und FSDTs Couatl-Eintrag in `exe.xml` erkannt werden. Flightdeck
   aktiviert diesen Eintrag und erhält dessen Argumente sowie andere Add-ons.
   Fehlt der Eintrag, im FSDT-Installer aktualisieren und den Status neu laden.
   Anschließend MSFS starten und GSX-Menü sowie Bodendienste prüfen.
   **GSX lokal eingerichtet · Funktion unbestätigt** bestätigt nur die lokale
   Einrichtung, weder die Lizenz noch die Linux-Kompatibilität.

GSX ist kostenpflichtig. Laut [offiziellem GSX-Handbuch](https://www.fsdreamteam.com/gsx_manual_msfs.pdf)
setzt die Installation eine GSX-Lizenz oder einen berechtigten, aktivierten
FSDT-Flughafen voraus. Mit Letzterem sind die Nutzung an FSDT-Flughäfen und ein
begrenzter Test an KSFO, LIMC und EDDM möglich. Die Vorbereitung in Flightdeck
schaltet GSX nicht frei. Kauf und Aktivierung erfolgen bei
[FSDreamTeam](https://www.fsdreamteam.com/products_gsxpro.html).

**FSDT-Installer öffnen** steht je nach Fortschritt als aktueller nächster Schritt
oder unter **Verwalten & reparieren**. Dort findest du auch die Reparatur der
FSDT-Einrichtung und die Autostart-Aktionen.
**GSX-Autostart ausschalten** ändert nur den Couatl-Eintrag und deinstalliert keine
Pakete. Bei eingerichtetem GSX-Autostart schließt Flightdeck die benannten
Couatl-Begleitprozesse dieses Profils auch bei Spielende, Absturz oder Stoppen.
Andere Profile und der FSDT-Installer gehören nicht zu dieser Sitzungsbereinigung.

Nach unterbrochener Vorbereitung erscheint **GSX-Vorbereitung wiederherstellen**
direkt als nächste Aktion. Sie aktiviert das bisherige Profil wieder; bis dahin
bleibt der Spielstart gesperrt.
Eine abgeschlossene Vorbereitung bewahrt `local/msfs-prefix.before-gsx-*` als
Sicherung auf. Die Wiederherstellung ist für unterbrochene Vorbereitung gedacht,
nicht zum Rückgängigmachen einer späteren GSX-Installation. Protokolle liegen in
`private/gsx-setup-*.log` und `private/gsx-manager.log`. Ersetzt FSDT den
Installer-Download, stoppt ein Prüfsummenfehler die Vorbereitung, bis Flightdecks
festgelegte Version geprüft und aktualisiert wurde. FSDT- und Microsoft-Binärdateien
werden nicht mitgeliefert.

## Orbx Central einrichten (experimentell)

Wähle die gewünschte Xbox- oder Steam-MSFS-Installation und öffne **Mods → Orbx Central**. Starte MSFS
vorher einmal, damit seine echte `UserCfg.opt` existiert. Beende danach den
Simulator und andere Windows-Programme im gewählten Profil.

1. **Orbx Central einrichten** lädt das vollständige offizielle Windows-Paket von
   Central 4.5.50 und prüft seine festgelegte SHA-256. Flightdeck entpackt es ins
   vorhandene Spielprofil und verwendet dessen Runner. Es entsteht kein weiteres
   Wine-Profil und kein zusätzlicher Proton-Download. Eine Reparatur beschädigter
   verwalteter Programmdateien bewahrt den vorherigen Programmordner auf.
2. Flightdeck verweist Orbx auf die echte `UserCfg.opt` der gewählten Edition und
   erhält die Simulator-Einstellungen sowie den Orbx-Pfad der anderen Edition.
   Eine 2024-Installation wird nicht als MSFS 2020 ausgegeben.
3. **Orbx Central öffnen**, normal anmelden und eigene lizenzierte Produkte
   installieren. Prüfe Edition und Community-Pfad in Orbx. Schließe anschließend
   das Fenster oder nutze **Orbx Central schließen** in Flightdeck. Tray-Modus und
   Windows-Anmeldestart sind deaktiviert, damit Orbx über sein Hauptfenster und
   Flightdeck erreichbar bleibt. Flightdeck beendet nur die benannten
   Orbx-Prozesse dieses Profils.

Der offizielle Web-Installer kann unter Wine **„Orbx Central cannot be closed“**
melden, obwohl Central bereits beendet ist. Flightdeck verwendet das geprüfte
vollständige Programmpaket und deaktiviert dessen automatisches Web-Update.
Andere Orbx-Instanzen musst du vor dem Öffnen selbst schließen: Der Worker nutzt
einen festen lokalen Port; Flightdeck beendet dafür kein anderes Profil.

Central 4.5.50 und sein Worker starteten im vorhandenen Flightdeck-MSFS-2024-Profil;
gezieltes Schließen gab Prozesse und Port wieder frei. Das bisherige separate
Orbx-Profil öffnete sich ebenfalls mit bestehender Anmeldung und ohne sichtbares
Wine-Tray-Fenster. In diesem separaten Profil wurden zuvor alle 2.937 Layout-Einträge
einer Installation von Gaya LOWW 2.0.1 geprüft. Anmeldung, neue Szenerieinstallation
und ein Simulatorflug im gemeinsamen Profil bleiben unbestätigt. Die lokale
Einrichtung bestätigt keine Szeneriekompatibilität. Protokolle bleiben unter
`private/orbx-central.log`. Orbx-Programme, Zugangsdaten und Szenerien werden nicht
mit Flightdeck ausgeliefert. [Offizielles Orbx Central](https://orbxdirect.com/central)

## Navigraph Hub und Simlink einrichten (experimentell)

Öffne **Mods → Navigraph** und folge dem hervorgehobenen nächsten Schritt. Jede
MSFS-Version hat ein eigenes Windows-Profil; richte Navigraph daher für 2024 und
2020 getrennt ein. Beende vorher MSFS und andere Windows-Programme im gewählten
Profil. Navigraph unterstützt Linux offiziell nicht; Anmeldung, Abonnement und
Navdata bleiben bei Navigraph.

1. **Navigraph-Anmeldung einrichten** trägt über `xdg-mime` einen versteckten
   Eintrag `flightdeck-navigraph.desktop` als Handler für `navigraph-desktop://`
   (Hub) und `navigraph-traffic-desktop://` (Simlink) ein. Nach der Anmeldung im
   Linux-Browser gibt der Browser das Ergebnis an Flightdeck zurück, das es an die
   wartende Anwendung im passenden Profil weiterreicht. Ohne diesen Handler endet
   die Anmeldung im Browser, kommt aber nie bei Hub oder Simlink an. `xdg-utils`
   wird benötigt.
2. Lade den offiziellen Navigraph-Hub-Installer bei
   [Navigraph](https://navigraph.com/downloads) herunter, wähle ihn aus und
   klicke auf **Navigraph Hub installieren**. Flightdeck installiert Hub ohne
   Rückfragen für den Windows-Benutzer dieses Profils; akzeptiert wird nur eine
   Datei, die wie Navigraphs Hub-Installer benannt ist. Flightdeck legt den
   mitgelieferten Process Governor (`procgov.exe`) beiseite, weil dessen Aufruf
   unter Wine mit `Win32Exception (87)` scheitert und jedes Entpacken abbricht.
   Hub ruft 7-Zip dann direkt auf. Hub-Updates stellen die Datei wieder her;
   Flightdeck legt sie vor jedem Start erneut beiseite. Außerdem schaltet
   Flightdeck Hubs Einstellungen **Im Hintergrund ausführen** und den
   Windows-Autostart aus, damit Hub mit seinem Fenster beendet wird.
3. **Navigraph Hub öffnen**, anmelden und die Navdata installieren. Hub findet den
   Simulator über dieselbe `UserCfg.opt` wie Flightdeck; starte MSFS vorher
   einmal. Schließe das Hub-Fenster nach der Installation oder nutze
   **Navigraph schließen**. Flightdeck zeigt die gefundenen `navigraph-*`-Pakete
   im Community-Ordner an.
4. Optional für die Flugzeugposition in Navigraph Charts: Lade den
   Simlink-Installer bei [Navigraph](https://navigraph.com/downloads) herunter,
   wähle ihn aus und klicke auf **Simlink installieren**. Flightdeck installiert
   Simlink ohne Assistenten in den Standardordner
   (`C:\Program Files\Navigraph\Simlink`), weil der Assistent unter Wine ein
   zusätzliches leeres Fenster zeigt. Öffne Simlink danach über **Simlink öffnen
   und anmelden** und melde dich im Browser an. Simlinks Fenster blendet es nur
   aus; beende es mit **Navigraph schließen**. Flightdeck erkennt die Anmeldung
   am gespeicherten Token, ohne seinen Inhalt auszugeben, und schaltet Simlinks eigenen
   Windows-Autostart aus, weil Wine ihn bei jedem Programmstart im Profil
   ausführen würde.
5. **Simlink-Autostart einrichten** ergänzt einen Simlink-Eintrag in der
   `exe.xml` neben der `UserCfg.opt` dieser Version. Andere Add-ons bleiben
   erhalten, eine Sicherung liegt in `private/navigraph-exe-*.xml`. MSFS startet
   Simlink dann in derselben Wine-Sitzung, SimConnect ist so ohne
   Netzwerkeinstellung erreichbar. Flightdeck beendet Simlink mit der
   Simulator-Sitzung. Melde dich einmal über **Verwalten & reparieren → Simlink
   öffnen** in Simlink an.

**Navigraph lokal eingerichtet · Funktion unbestätigt** bestätigt nur lokale
Dateien. Die Navdata-Installation durch Hub, die SimConnect-Verbindung von
Simlink und die Positionsanzeige in Charts sind mit Flightdeck noch nicht in einem
vollständigen Flug verifiziert. Hub erkennt Flightdecks Installation wie eine
Steam-Installation und sieht Marketplace-Flugzeuge im Official-Ordner nicht;
flugzeugspezifische Navdata dafür können fehlen. Protokolle liegen in
`private/navigraph-*.log` der Runtime. Flightdeck liefert keine Navigraph-Dateien
mit.

## Fenix A320 einrichten

Öffne **Mods → Fenix A320** und folge dem hervorgehobenen nächsten Schritt.
Flightdeck fasst erledigte Schritte zusammen. Weitere Aktionen findest du unter
**Verwalten & reparieren**, darunter den Bereich **Lokales Patch-Paket und
Wiederherstellung**. Derselbe Patch ist auch als
[eigenständiger Patch-Installer](https://github.com/marselnenaj/fenix-a320-linux-patch/releases)
verfügbar.

Der Patch unterstützt MSFS 2024 mit dem festgelegten Xodus-Wine-Runner. Starte
MSFS 2024 einmal, damit die Benutzereinstellungen angelegt sind. Schließe danach
MSFS und alle Fenix-Anwendungen.

Für den Fenix-v3-Installer wird zusätzlich die anhand ihrer Prüfsumme verifizierte
Microsoft .NET 10 Desktop Runtime eingerichtet. Fehlt sie später, ergänzt
Flightdeck sie vor dem Öffnen einer Fenix-App. Im offiziellen Installer den A320
für den ausgewählten Simulator installieren, den Abschluss der
Flugzeuginstallation abwarten und erst danach den Installer schließen.

1. **Patch einrichten** lädt das Linux-ZIP aus dem öffentlichen Fenix-GitHub-Release
   und prüft dessen SHA-256. Es erstellt eine eigene Runner-/Profilkopie, bewahrt
   die bisherige Umgebung auf und installiert bei Bedarf Microsoft .NET Framework 4.8
   sowie die separat heruntergeladene, geprüfte Geometrie-Abhängigkeit.
2. Wird **Fenix-App öffnen** angeboten, im erkannten Manager weiter installieren.
   Andernfalls den offiziellen Installer aus dem
   [Fenix-Konto](https://fenixsim.com/dashboard/) herunterladen, seine EXE auswählen
   und **Installer starten** wählen. Voraussetzungen und Flugzeug im ausgewählten
   Simulatorprofil installieren, danach Manager oder Installer schließen.
3. **Fenix öffnen**, anmelden und aktivieren. Danach Fenix wieder schließen
   oder in Flightdeck **Fenix beenden** wählen.
4. **Einrichtung abschließen** setzt GPU-Anzeigen auf unterstützten Versionen
   (CPU bei CachyOS Proton 10), Legacy-Readouts und Autostart.
   Sobald die lokale Einrichtung abgeschlossen ist, zur Übersicht wechseln und
   MSFS ganz normal starten.
   Hat Fenix seine Readout-Einstellungen noch nicht angelegt, weist das Panel
   darauf hin: Flugzeug einmal im Simulator laden, MSFS beenden und
   **Einstellungen erneut anwenden** wählen.

**Fenix lokal eingerichtet** bestätigt nur Flightdecks Einstellungen.
Sie bestätigt weder deine Fenix-Lizenz und Aktivierung noch ein funktionierendes
Cockpit. Anmeldung und Lizenz prüft Fenix; das Flugzeug anschließend in MSFS
prüfen. Ein erkannter Manager oder erfolgreicher Installations-Hook bestätigt
allein noch keine Flugzeuginstallation.

Das Beenden des offiziellen Installers allein schließt die Einrichtung noch
nicht ab. Läuft noch eine Windows-Anwendung, den Fenix-Installer und Fenix
vollständig beenden; die Anzeige aktualisiert sich automatisch. Falls im
Loginfenster `AltGr+Q` kein `@` eingibt,
`Strg+Alt+Q` versuchen oder `@` kopieren und mit `Strg+V` einfügen.

**Fenix beenden** erscheint, wenn Fenix im ausgewählten Profil läuft. Der Knopf
schließt Fenix, seine Helfer und den offiziellen Manager. Erst danach gibt
Flightdeck die nächsten Schritte frei. Während MSFS oder ein Installer läuft,
bleibt der Knopf gesperrt. Laufende Installationen und Livery-Downloads im
offiziellen Manager vor dem Beenden abschließen.

Unter **Verwalten & reparieren** findest du Patch-Updates, das erneute Anwenden
der Einstellungen und die Neuinstallation der offiziellen App. Manager- und
Reparaturaktionen stehen dort, wenn sie nicht schon der aktuelle nächste Schritt
sind. Vorhandene Patches mit preview.1 oder
preview.2 werden beim Öffnen von Fenix automatisch aktualisiert; **Patch
aktualisieren** startet das Update auch direkt. Flugzeug, Einstellungen und der
ursprüngliche Wiederherstellungspunkt bleiben erhalten.

Meldet der offizielle Installer **„application install hook failed“**, zuerst
alle Installer-Fenster schließen. Beim Start seiner EXE über Flightdeck greift
der vorhandene ICU-/.NET-Fix, der bereits in 0.2.5 enthalten ist. Nach einem
fehlgeschlagenen Installer erscheint **Fenix-App reparieren** als nächste Aktion,
sofern die Reparatur verfügbar ist. Sie wiederholt den offiziellen
Einrichtungsschritt mit gültigen Paketdaten, ohne das Profil zurückzusetzen.
Nach erfolgreicher Reparatur den Manager erneut öffnen.

In **0.2.8** bleibt der zuletzt protokollierte fehlgeschlagene Versuch nach einem
Launcher-Neustart sichtbar. Nach einer fehlgeschlagenen Reparatur führt der
nächste Schritt zu **Diagnose öffnen**. Vor einem weiteren Versuch einen aktuellen
Bericht erstellen; eine verfügbare Reparatur bleibt unter **Verwalten & reparieren**.
Fehlt der Manager oder lässt er sich nicht eindeutig erkennen, kann dort auch
der offizielle Installer ausgewählt werden. Der grüne Check **Fenix-Linux-Patch**
bestätigt nur den Linux-Patch, nicht den erfolgreichen App-Hook. Ein neuer
protokollierter Versuch ersetzt das vorherige Ergebnis.

Version 0.2.8 filtert geerbte Linux-.NET-Ladepfade, Ladeoptionen und Startup-Hooks
vor Windows-Installer und Reparatur. Im kontrollierten Test mit offizieller
FenixApp 1.0.286 verursachte ein ungültiger geerbter `DOTNET_STARTUP_HOOKS` Exitcode
82; dieselbe Eingabe bestand nach dem Filtern mit Exitcode 0. Diese reproduzierte
Ursache ist damit behoben. Dass gemeldete Exit-82-Fehler bei Nutzern dadurch
behoben sind, ist noch nicht bestätigt. Exitcode 82 allein benennt keine Ursache.
Diagnoseberichte enthalten erkannte Ausnahmetypen und einen ausdrücklich
beobachteten CLR-Code, keine rohen Fehlermeldungen;
`<Runtime-Ordner>/private/fenix-app.log` bleibt lokal.
[Prüfumfang](native-ui.md#fenix-install-hook-warning).

Flightdeck prüft außerdem gespeicherte `DOTNET_STARTUP_HOOKS` in der Benutzer-
und Systemumgebung des Wine-Profils. Ein fehlender absoluter `C:\…`-Hook kann
Exitcode 82/`FileNotFoundException` trotz gefilterter Linux-Umgebung erneut
verursachen. Entfernt werden nur nachweislich fehlende Pfade. Vorhandene Hooks,
relative Einträge, Variablen und andere Laufwerke bleiben erhalten. Vor der
Registry-Änderung wird der ursprüngliche Wert lokal in
`<Wine-Profil>/.flightdeck-fenix-environment.json` gesichert. Das behebt den
reproduzierten Fall; andere fehlende Assemblies können weitere Ursachen haben.

Nach der Einrichtung startet Fenix automatisch mit MSFS. Du musst es nicht
separat öffnen. Beim normalen Spielende, einem Absturz oder **Stoppen** beendet
Flightdeck die Fenix-Begleitprozesse dieser Sitzung; hängende Prozesse werden
nach einer kurzen Wartezeit beendet. Seit 0.2.9 gilt das auch für die offizielle
Fenix-App, falls sie noch geöffnet ist; schließe Installationen oder
Livery-Downloads dort ab, bevor du MSFS beendest. Andere Wine-Profile bleiben
davon unberührt.

Flightdeck 0.2.23 enthält Patch **0.1.0** als geprüfte Ausgangsversion
und sucht beim Start sowie alle 30 Minuten eigenständig auf GitHub nach
kompatiblen Fenix-Patches. Fehlgeschlagene Prüfungen werden nach fünf Minuten
wiederholt; der installierte Patch bleibt nutzbar. Unter **Updates** und
**Mods → Fenix A320 → Verwalten & reparieren** stehen die installierte Version,
ein verfügbares Update und **Fenix-Updates prüfen**.

Ein gefundenes kompatibles Update wird vor dem MSFS-Start, vor dem Öffnen von
Fenix, seinem Installer oder Manager sowie beim Abschließen der Einrichtung
automatisch installiert. Alternativ **Patch aktualisieren** wählen. MSFS und
Fenix müssen geschlossen sein; Runtime-Sperre und die vorhandenen Profil- und
Runner-Sicherungen schützen die Installation. Flugzeugdateien, Renderer-Wahl und
Wiederherstellungspunkte bleiben erhalten. Nach einem Launcher-Neustart oder
einer Offline-Prüfung wird ein neuerer installierter Patch nicht zurückgestuft.
Preview-Installationen bevorzugen eine verfügbare stabile Version; stabile
Installationen folgen stabilen Releases. Benötigt ein Patch eine neue
Installer-Schnittstelle oder andere Runner, ist zuerst ein Flightdeck-Update nötig.

Ab 0.2.23 lässt sich eine bekannte ältere lokale Einrichtung vor dem Start
automatisch oder über **Einrichtung übernehmen & aktualisieren** übernehmen.
Flightdeck prüft die Dateien, kopiert Profil und Runner und behält den vorherigen
Stand zur Wiederherstellung. Flugzeug sowie vorhandene Renderer- und FCU-Wahl
bleiben erhalten. Unbekannte angepasste Dateien stoppen die Übernahme. Nach einer Wiederherstellung ist die automatische Übernahme pausiert; mit der Übernahme-Schaltfläche kannst du sie erneut aktivieren. Übersicht
und Update-Symbol zeigen die ausstehende Übernahme; eine **verfügbare** Version
bestätigt keine installierte Version.

Patch 0.1.0 ersetzt die restlichen eigenen Linux-Python-Helfer durch Rust.
Er benötigt Flightdeck **ab 0.2.22**; zuerst den Launcher aktualisieren. Wine,
DXVK und die Windows-ABI-Helfer bleiben natives C/C++. Stabilität über ganze
Flüge und das separate Speicherwachstum von FenixSystem bleiben ungeklärt.

Die Flightdeck-Installation richtet das Fenix-Flugzeug nicht automatisch ein.
Flugzeug-Updates und Lizenzierung übernimmt weiterhin die offizielle Fenix-App.
Für den öffentlichen Patch-Download ist keine GitHub-Anmeldung nötig. Das
Flugzeug selbst wird mit dem offiziellen Fenix-Programm heruntergeladen und über
dein Fenix-Konto aktiviert.

Für eine lokale Kopie das **Linux-Installer-ZIP** entpacken und unter **Lokales
Patch-Paket und Wiederherstellung**, unterhalb von **Verwalten & reparieren**,
den Ordner mit `bundle.json` auswählen.
Der GitHub-Quellcode oder das reine Quellarchiv enthält die Wine-Binärdateien nicht.
Ein leeres Feld verwendet den geprüften Download bzw. Cache. Der eigenständige
Installer `install.sh` bietet denselben Ablauf ohne Flightdecks Oberfläche, als
geführte Einrichtung im Webbrowser.

### Liveries

Den installierten offiziellen Manager öffnest du über **Fenix-App öffnen** im
aktuellen Einrichtungsschritt oder über **Installer & Liveries** unter
**Verwalten & reparieren**. Er übernimmt Flugzeuginstallation, Updates und Liveries.
**Fenix öffnen** startet die Fenix-Hauptanwendung. Der Manager-Button wird verfügbar,
sobald Flightdeck ihn im ausgewählten Wine-Profil erkennt. Vorher den Simulator
und andere Fenix-Anwendungen schließen. Bemalungen müssen zum gekauften
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

**Fenix v3 ist experimentell.** Preview.11 korrigiert die Gateway-Loadernamen
für alle vier gepinnten Runner. Mit **Fenix 3.0.0.7454 / Steam-MSFS 2020 /
CachyOS 11 / NVIDIA-PRIME** bestätigt der Reporter danach Gateway-Protokoll,
Port 8083 und ein funktionierendes EFB. Die GPU-Displays funktionieren mit
**USE DISPLAY SAFE MODE aus** in der Fenix-App; CPU bleibt als Rückfall verfügbar.
Diese Community-Ergebnisse werden durch 64 unabhängige native Vergleiche der
Prozessnamen auf allen vier gepatchten Runnern ergänzt.

Der frühere lokale Xodus-/MSFS-2024-Test erreichte funktionierende Cockpitanzeigen
und bestand einen neuen Start sowie fünf Backend-Neustarts. Eine frühere Wine-
`create_view`-Assertion bleibt ungeklärt; vollständige v3-Flüge sind nicht validiert.
Das separate System-Speicherwachstum von Fenix 2 bleibt offen.
[Gateway-Fix und Geltungsbereich](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0/docs/gateway-loader-case.md).

Zuvor getestet wurden Fenix 2.4.0.4720 und MSFS 2024 1.8.16.0 auf Hyprland. Die
Cockpitanzeigen inklusive MCDU, FCU, Funk und Uhr sind geprüft; ein vollständiger
Testflug steht noch aus. Der GPU-Modus wurde auf AMD im Xodus-Cockpit mit PFD,
ND, ECAM und bewegter Geschwindigkeitsanzeige geprüft. NVIDIA außerhalb des gemeldeten Systems und GPU-Wetterradar
sind noch nicht geprüft; im CPU-Modus ist Wetterradar nicht verfügbar.
Eine bisher automatisch gesetzte CPU-Konfiguration wechselt auf unterstützten
Versionen zu GPU. Eine gespeicherte CPU-Präferenz bleibt bei Updates erhalten.
[GPU-Prüfstand und CPU-Rückfall](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0/docs/gpu-displays.md).
Das Fenix-Binärpaket benötigt x86_64 Linux und glibc 2.38+; für Flightdecks
vollständiges natives Paket gilt weiterhin glibc 2.39+. Ab 0.1.20 gibt es passende
Fenix-Patches für Proton Experimental 11.0 (20260924), CachyOS Proton 11.0
(20261005) und CachyOS Proton 10.0
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

**Patch rückgängig machen** unter **Lokales Patch-Paket und Wiederherstellung**
stellt Runner, Skripte und Windows-Profil von vor dem
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
