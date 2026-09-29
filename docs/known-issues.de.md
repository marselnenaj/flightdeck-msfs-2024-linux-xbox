# Bekannte Probleme

[English](known-issues.md)

Stand: Flightdeck 0.1.10, 29. September 2026.

**Komponentenupdates:** 0.1.10 behebt, dass ältere Store-Binaries bei angepassten
Startskripten erhalten blieben. Aktualisiere Flightdeck und öffne es bei
geschlossenem Simulator erneut; die Store-Prüfung sollte anschließend den
Komponentenschritt bestehen. Eigene Skripte bleiben erhalten. Die weiteren
Online-Prüfungen sind damit noch nicht bestätigt.

Die Behebung der folgenden gemeldeten Fehler ist noch nicht bestätigt. Sie betreffen nicht
jede Installation. Priorität hat die schwarze NVIDIA-Hauptansicht, danach
folgen die Zuverlässigkeit von Cloud-Sync und Marketplace-Sitzungen.

## Aktueller Umfang

- **NVIDIA:** MSFS 2024 kann funktionierende Menüs und Overlays anzeigen,
  während Weltkarte und 3D-Hauptansicht schwarz bleiben. Keiner der beiden
  Grafikmodi ist eine bestätigte Lösung; ein zweites Renderfenster kann einen
  Absturz auslösen. Flightdeck 0.1.9 enthält
  Upstream-Korrekturen. Dass sie die schwarze Hauptansicht auf NVIDIA behebt,
  ist noch nicht bestätigt. Ein funktionierendes zweites Fenster löst das
  Problem einer schwarzen Hauptansicht nicht.
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
