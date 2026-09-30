# Bekannte Probleme

[English](known-issues.md)

Stand: Flightdeck 0.1.14, 30. September 2026.

**Komponentenupdates:** 0.1.10 behebt, dass ältere Store-Binaries bei angepassten
Startskripten erhalten blieben. Aktualisiere Flightdeck und öffne es bei
geschlossenem Simulator erneut; die Store-Prüfung sollte anschließend den
Komponentenschritt bestehen. Eigene Skripte bleiben erhalten. Die weiteren
Online-Prüfungen sind damit noch nicht bestätigt.

Die Behebung der folgenden gemeldeten Fehler ist noch nicht bestätigt. Sie betreffen nicht
jede Installation. Ein Anmeldefehler kann verhindern, dass die Darstellung im
Simulator geprüft wird.

## Aktueller Umfang

- **Microsoft-Anmeldung:** Das Anmeldefenster schließt sich laut Bericht sofort
  mit einer allgemeinen Fehlermeldung, noch bevor der Simulator startet. Die
  Anmeldekomponente war zwischen 0.1.12 und 0.1.13 unverändert. Version 0.1.14
  erhält zusätzliche Microsoft-Anmeldeschritte,
  die bisher als endgültiger Fehler verworfen werden konnten. Damit ist weder
  die Ursache jedes geschlossenen Fensters noch ein Zusammenhang mit dem
  NVIDIA-Darstellungsproblem bestätigt.
  [Korrektur und Prüfumfang (Englisch)](store-session-refresh.md#interactive-challenge-correction-0114).
- **NVIDIA:** MSFS 2024 kann funktionierende Menüs und Overlays anzeigen,
  während Weltkarte und 3D-Hauptansicht schwarz bleiben. 0.1.11 korrigiert DXVKs
  wirkungslose Low-Latency-Abschaltung und verwendet automatisch das vollständige
  Kompatibilitätsprofil für DirectX 11 und 12. DLSS, Reflex und NVIDIA Frame
  Generation sind damit standardmäßig deaktiviert. Die Behebung der schwarzen
  Hauptansicht ist noch nicht auf NVIDIA-Hardware bestätigt. Ein funktionierendes
  zweites Fenster bestätigt sie nicht.
  [Grafikmodi und aktueller Stand](graphics.de.md).
- **Cloud-Spielstände:** Wiederholte Sync-Fehler werden auch nach den
  Timeout-Korrekturen aus 0.1.8 gemeldet. Wiederherstellung und lokales Spielen
  beschreibt die [Cloud-Anleitung](cloud-saves.de.md). Lokales Spielen bestätigt
  keinen erfolgreichen Cloud-Abgleich.
- **Marketplace:** Die nicht schließbare Meldung „Marketplace-Sitzung abgelaufen“
  tritt laut Bericht beim Öffnen des Marketplace im Simulator auf. Sie ist von
  Fehlern im Flightdeck-Kauffenster
  zu unterscheiden. Ein Simulator-Neustart dient der Wiederherstellung und ist
  keine dauerhafte Fehlerbehebung. [Marketplace-Umfang](marketplace-collections.md).

Weitere ungeprüfte Funktionen, darunter der MSFS-2020-Spielstart und vollständig
abgeschlossene Käufe, stehen in der [Kompatibilitätsübersicht](readme.de.md#aktueller-stand).

Flightdeck 0.1.9 erneuert ablaufende Tickets, erhält die
Store-Sitzung desselben Kontos und bietet eine erneute Microsoft-Anmeldung mit
anschließender Prüfung bei geschlossenem Simulator. Damit ist die blockierende
Marketplace-Meldung im Spiel noch nicht als behoben bestätigt.
[Umfang und Prüfung der Store-Korrektur (Englisch)](store-session-refresh.md).
