# Bekannte Probleme

[English](known-issues.md) · [Dokumentation](index.md)

Stand: 8. Oktober 2026, für die stabile Version **0.2.8**.
Siehe [Änderungsübersicht](changelog.md#028--8-october-2026).

- **NVIDIA:** Die normale MSFS-2024-Darstellung ist durch Nutzertests bestätigt.
  Kleinere Probleme bleiben möglich, etwa ein fehlendes oder schwarzes Startvideo.
  Das bestätigt nicht jede GPU-/Treiberkombination, VR oder NVIDIA-Zusatzfunktionen.
  Bleiben auch Hauptkarte oder Cockpit schwarz, bitte getrennt melden.
  [Grafikeinrichtung und Fehlermeldungen](graphics.de.md).
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
- **Unterbrochene Sitzungen und Schaltflächen:** In 0.2.6 beheben die sichere
  Sitzungsprüfung, Reparatur verwaister Dienst-Sockets und korrigierte Anzeigen
  für Start, Stop und Simulatorauswahl Startblockaden nach unterbrochenen
  Sitzungen. [Details](native-ui.md#session-and-control-fixes-in-026).
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
