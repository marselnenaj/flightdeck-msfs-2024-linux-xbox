# Bekannte Probleme

[English](known-issues.md) · [Dokumentation](index.md)

Stand: 22. Oktober 2026, für die stabile Version **0.2.17**.
Siehe [Änderungsübersicht](changelog.md#0217--22-october-2026).

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
  [Issue #4](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/4).
- **VSync:** Die in Issue #4 gemeldete Überschreitung der 48-FPS-Grenze bei
  einem Drittel der Bildwiederholrate eines 144-Hz-Monitors bleibt ungeprüft.
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
