# Bekannte Probleme

[English](known-issues.md) · [Dokumentation](index.md)

Stand: 30. Oktober 2026, veröffentlichter Launcher **0.3.7**.
Siehe [Änderungsübersicht](changelog.md#037--30-october-2026).

- **MobiFlight-Einrichtung in 0.3.7:** Die App kann installiert sein, während die
  WASM-/Ereignis-Einrichtung an HubHops alter, weitergeleiteter Adresse oder langen
  Ereignisskripten scheitert. Der Entwicklungsstand behebt beides und setzt die
  Einrichtung auch nach einem Fenix-/Proton-Profilwechsel fort. Der Fix ist noch
  nicht veröffentlicht. Nach einem Update mit dem Fix verwendet **MobiFlight
  vorbereiten / reparieren** die vorhandene App und erhält eigene Ereignisse.

- **Bazzite-Installation:** Der Installer lehnt symbolische Links in Quell- und
  Zielpfaden ab, auch Fedora Atomics `/home`-Verknüpfung. Im Desktop-Modus mit
  [aufgelösten Verzeichnispfaden installieren](install.md#bazzite-and-symbolic-link-errors).
  Die Ausweichlösung bestand einen synthetischen Installer-Test; die
  Simulator-Kompatibilität auf Bazzite bleibt ungeprüft.

- **Spieldownload:** Version 0.2.21 bietet bei fehlgeschlagenem fortsetzbarem
  Download oder fehlgeschlagener Anmeldung **Download erneut versuchen**, solange
  der Hintergrunddienst läuft. Der vorhandene Ordner wird nach Prüfung fertiger
  Dateien weiterverwendet. Numerische Einrichtungsfehler bleiben auch vor der
  ersten Installation in der Diagnose erhalten. Alte Berichte liefern fehlende
  Codes nicht nach; von älteren Versionen oder einem Dienstneustart beendete
  Jobs stellt der Knopf nicht wieder her. [Ablauf und Grenzen](download-pause.de.md).

- **Xbox-Authentifizierung beim Download (gemeldet mit 0.3.0):** Die Anmeldung
  im Browser kann gelingen, während die nachfolgende Xbox-Tokenanfrage HTTP 400
  mit `X-Err: 00000005` liefert. Das gebündelte Xodus bricht dann mit einer
  Panic und Downloadcode 101 ab. Der Fehler wurde lokal ohne Proxy reproduziert.
  Version **0.3.1** verwendet stattdessen Windows SISU und funktioniert mit
  demselben Konto: Die
  authentifizierte MSFS-Paketabfrage liefert HTTP 200 und die Übertragung der
  Metadaten vom Microsoft-CDN beginnt. Benutzer-, Geräte- und Anwendungsanmeldung
  bleiben erhalten, einschließlich desselben Geräte-Proof-Keys. Flightdeck
  aktualisieren, um die korrigierten nativen Komponenten zu erhalten.
  Die Folgemeldung mit **0.3.1** erreicht die Lizenzabfrage, scheitert dort aber
  an `InvalidMsaTicketFormat` / Fehler 4003. Version **0.3.2** entfernt das Xbox-
  Delegationspräfix `d=` ausschließlich aus der MSA-Benutzeridentität für die
  Lizenzabfrage. Im direkten Vergleich weist Microsoft die Identität mit
  Präfix zurück und stellt für dasselbe Ticket ohne Präfix eine Lizenz aus.
  Der erneute Test des ursprünglichen Melders steht aus. Der Fehler belegt
  weder eine Kontosperre noch einen fehlenden Kauf.
  [Upstream-PR #201](https://github.com/xodus-gaming/xodus/pull/201) ist ein
  separates Experiment mit Geräteauthentifizierung; Flightdeck verlangt
  weiterhin Benutzerzugriff auf das Paket und eine gültige Inhaltslizenz.

- **NVIDIA:** Die normale MSFS-2024-Darstellung ist durch Nutzertests bestätigt.
  Kleinere Probleme bleiben möglich, etwa ein fehlendes oder schwarzes Startvideo.
  Das bestätigt nicht jede GPU-/Treiberkombination, VR oder NVIDIA-Zusatzfunktionen.
  Version 0.2.13 bevorzugt Software-H.264-Dekodierung und korrigiert die Codec-Auswahl
  für unterstützte Proton-Versionen. Beim Startabsturz unter Nobara mit RTX 5080
  [bestätigt der Melder den Betrieb mit einer anderen Proton-Version, etwa Proton-GE](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/3#issuecomment-6348916712).
  Der Absturz mit dem Standard-Runner Xodus bleibt ungeklärt; die funktionierenden
  Proton- und Flightdeck-Versionen wurden noch nicht genannt.
  Bleiben auch Hauptkarte oder Cockpit schwarz, bitte getrennt melden.
  [Grafikeinrichtung und Fehlermeldungen](graphics.de.md).
- **Andere Proton-Versionen:** Version 0.2.14 behebt den Xbox-Anmeldefehler
  `000-2000-0x80004001`, bestätigt mit Proton Experimental 11 im echten MSFS 2024.
  CachyOS 10 sunset endet weiterhin vor Abschluss der Anmeldung mit Code 84.
  Verwende die Flightdeck-Standardumgebung oder einen anderen geprüften Runner.
  Mit **0.2.20 und CachyOS 11.0-20261015** erreicht der Melder nach Warten
  während des schwarzen Ladebildschirms mit Musik die Start-Schaltfläche.
  Nach Start erscheinen das Menü und eine gerenderte Szene im Simulator.
  Das ist eine gemeldete Starthilfe; das Video ist weiterhin defekt und ein
  vollständiger Flug damit noch nicht bestätigt.
  [Rückmeldung in Issue #4](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/4#issuecomment-6377721086).
- **VSync:** Die in Issue #4 gemeldete Überschreitung der 48-FPS-Grenze bei
  einem Drittel der Bildwiederholrate eines 144-Hz-Monitors besteht laut
  Rückmeldung auch unter **0.2.20**. Der neueste Screenshot zeigt etwa 56 FPS
  mit „VSync One-third rate“. Ursache und Behebung bleiben unbestätigt;
  ein erfolgreicher Start mit einem anderen Runner behebt das nicht.
- **Fenix v3:** Patch 0.1.0 ist experimentell. Offizielle Installation,
  Anmeldung und Cockpitanzeigen wurden mit Fenix 3.0.0.7454 unter Xodus geprüft.
  Ein Start scheiterte an Wines `create_view`-Assertion; sechs spätere Starts,
  davon fünf warme Neustarts der Fenix-Dienste, reproduzierten sie nicht. Ursache,
  Stabilität über vollständige Flüge und das frühere Einfrieren der Anzeigen
  bleiben ungeklärt. Preview.11 korrigiert die Gateway-Prüfung des Loader-Namens.
  Der Melder mit Steam MSFS 2020 / CachyOS 11 / NVIDIA bestätigt funktionierende
  Gateway-/EFB-Verbindungen und GPU-Anzeigen mit **USE DISPLAY SAFE MODE aus**.
  Andere Runner bestehen passende native Modultests; die Rückmeldung bestätigt
  keine Stabilität über vollständige Flüge.
  [Prüfablauf und Grenzen](https://github.com/marselnenaj/fenix-a320-linux-patch/blob/v0.1.0/docs/fenix-v3.md).
- **Unterbrochene Fenix-Patch-Updates:** Flightdeck 0.2.29 zeigt Einrichtungsfehler
  unter **Updates** und bietet für eine unveränderte Vorbereitung
  **Fenix-Einrichtung erneut versuchen** an, auch nach einem Launcher-Neustart.
  MSFS und Fenix vorher schließen. Flightdeck prüft den bisherigen Patch und
  erhält Flugzeuge, Einstellungen und Wiederherstellungspunkte; der geprüfte
  Download wird wiederverwendet. Geänderte Dateien oder eine bereits begonnene
  Aktivierung erfordern eine Wiederherstellung unter **Mods**.
- **Fenix-Installation:** Bei „application install hook failed“ können bereits
  FenixApp-Dateien vorhanden sein, obwohl die Einrichtung nicht abgeschlossen ist.
  Ein reproduzierter ICU-/.NET-Fall ist reparierbar; die allgemeine Meldung allein
  verrät die Ursache nicht. Den Installer schließen und **Mods → Fenix →
  Fenix-App reparieren** verwenden, sofern angeboten. Scheitert die Reparatur,
  folgt **Diagnose öffnen**; weitere Reparatur oder Neuinstallation stehen unter
  **Verwalten & reparieren**. Der zuletzt protokollierte Fehler bleibt nach einem
  Launcher-Neustart sichtbar. Der grüne Check **Fenix-Linux-Patch** bestätigt nur
  den Patch, nicht den App-Hook, die Flugzeuginstallation oder Aktivierung.
  Version 0.2.8 filtert geerbte .NET-Vorgaben des Linux-Hosts: Ein kontrollierter
  Startup-Hook-Test mit FenixApp 1.0.286 wechselte von Exitcode 82 zu 0. Dass
  gemeldete Exit-82-Fehler bei Nutzern dadurch behoben sind, ist unbestätigt;
  der Code allein benennt keine Ursache.
  [Fenix einrichten](addons.de.md#fenix-a320-einrichten) · [Prüfumfang](native-ui.md#fenix-install-hook-warning).
- **GSX Pro:** Experimentell. Der offizielle FSDT-Installer wurde in einem
  isolierten Wine-Profil geprüft. Lizenzierte Installation, Aktivierung,
  Couatl/SimConnect, Menü im Simulator und Bodendienste bleiben unbestätigt.
  **GSX lokal eingerichtet · Funktion unbestätigt** beschreibt nur die lokale
  Einrichtung. [Prüfgrenzen und Einrichtung](addons.de.md#gsx-pro-einrichten-experimentell).
- **Unterbrochene Sitzungen und Schaltflächen:** Seit 0.2.6 blieb der Start bis
  zum Neustart blockiert, wenn ein Prozess der vorherigen Sitzung überlebte.
  Version 0.2.9 beendet solche Prozesse und nennt einen Überlebenden im
  Starthinweis; ein nach GPU-Geräteverlust im Kernel hängender Prozess braucht
  weiterhin einen Neustart. [Details](cloud-saves.de.md#sicherungen-und-unterbrechungen).
- **Cloud-Spielstände:** Wiederholte Sync-Fehler bleiben gemeldet.
  Wiederherstellung und lokales Spielen beschreibt die
  [Cloud-Anleitung](cloud-saves.de.md). Lokales Spielen bestätigt keinen Cloud-Abgleich.
- **Microsoft-Anmeldung:** Vor einem erneuten Versuch Flightdeck aktualisieren
  und neu starten. Bleibt der Fehler bestehen, den angezeigten Code und die
  Launcher-Version melden. [Anmeldung und Store-Reparatur](store-session-refresh.md).
- **Marketplace:** Die blockierende Meldung „Marketplace-Sitzung abgelaufen“
  bleibt im Simulator gemeldet. Ein Neustart ist eine Wiederherstellung, keine
  bestätigte dauerhafte Behebung. Abgeschlossene Käufe und die Auslieferung
  gekaufter Inhalte bleiben unbestätigt. [Marketplace-Umfang](marketplace-collections.md).
- **VR:** OpenXR-Einrichtung für WiVRn, SteamVR und Monado ist verfügbar. Stereo-
  Tests bestehen auf AMD mit simuliertem Headset. Echte Headsets, NVIDIA-VR und
  MSFS-VR-Flüge müssen gesondert geprüft werden. [VR einrichten](vr.de.md).

Weitere Grenzen stehen unter [Kompatibilität](readme.de.md#kompatibilität).
[Problemberichte](problem-reports.de.md) enthalten ausgewählte Diagnosedaten.
