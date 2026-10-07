# Bekannte Probleme

[English](known-issues.md) · [Dokumentation](index.md)

Stand: 8. Oktober 2026, für die stabile Version **0.2.6**.
Siehe [Änderungsübersicht](changelog.md#026--8-october-2026).

- **NVIDIA:** Die normale MSFS-2024-Darstellung ist durch Nutzertests bestätigt.
  Kleinere Probleme bleiben möglich, etwa ein fehlendes oder schwarzes Startvideo.
  Das bestätigt nicht jede GPU-/Treiberkombination, VR oder NVIDIA-Zusatzfunktionen.
  Bleiben auch Hauptkarte oder Cockpit schwarz, bitte getrennt melden.
  [Grafikeinrichtung und Fehlermeldungen](graphics.de.md).
- **Fenix-Installation:** Bei „application install hook failed“ können bereits
  FenixApp-Dateien vorhanden sein, obwohl die Einrichtung nicht abgeschlossen ist.
  Ein reproduzierter ICU-/.NET-Fall ist reparierbar; die allgemeine Meldung allein
  verrät die Ursache nicht. In 0.2.6 den Installer schließen und **Mods →
  Fenix → Fenix-App reparieren** verwenden. Dies wiederholt den offiziellen Einrichtungsschritt der vorhandenen
  App; die Diagnose erfasst den letzten Versuch. Eine erkannte FenixApp bestätigt
  weder die Flugzeuginstallation noch die Aktivierung.
  [Fenix einrichten](addons.de.md#fenix-a320-einrichten) · [Prüfumfang](native-ui.md#fenix-install-hook-warning).
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
