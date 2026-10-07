# Bekannte Probleme

[English](known-issues.md) · [Dokumentation](index.md)

Stand: 7. Oktober 2026. Der veröffentlichte Installer ist 0.2.5. Änderungen für
die nächste Version sind im [Changelog](changelog.md#unreleased) als **nur im
Quellcode** gekennzeichnet.

- **NVIDIA:** Die normale MSFS-2024-Darstellung ist durch Nutzertests bestätigt.
  Kleinere Probleme bleiben möglich, etwa ein fehlendes oder schwarzes Startvideo.
  Das bestätigt nicht jede GPU-/Treiberkombination, VR oder NVIDIA-Zusatzfunktionen.
  Bleiben auch Hauptkarte oder Cockpit schwarz, bitte getrennt melden.
  [Grafikeinrichtung und Fehlermeldungen](graphics.de.md).
- **Fenix-Installation:** Bei „application install hook failed“ können bereits
  FenixApp-Dateien vorhanden sein, obwohl die Einrichtung nicht abgeschlossen ist.
  Ein reproduzierter ICU-/.NET-Fall ist reparierbar; die allgemeine Meldung allein
  verrät die Ursache nicht. In 0.2.5 den Installer schließen und über **Mods →
  Fenix → Installer starten** erneut ausführen. **Nur im Quellcode:** **Fenix-App
  reparieren** wiederholt den offiziellen Einrichtungsschritt der vorhandenen
  App; die Diagnose erfasst den letzten Versuch. Eine erkannte FenixApp bestätigt
  weder die Flugzeuginstallation noch die Aktivierung.
  [Fenix einrichten](addons.de.md#fenix-a320-einrichten) · [Prüfumfang](native-ui.md#fenix-install-hook-warning).
- **Unterbrochene Sitzungen und Schaltflächen:** **Nur im Quellcode:** sichere
  Sitzungsprüfung, Reparatur verwaister Dienst-Sockets und korrigierte Anzeigen
  für Start, Stop und Simulatorauswahl beheben Startblockaden nach unterbrochenen
  Sitzungen. [Details](native-ui.md#upcoming-session-and-control-fixes-source-only).
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
