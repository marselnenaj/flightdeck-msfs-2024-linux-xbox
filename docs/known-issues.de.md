# Bekannte Probleme

[English](known-issues.md)

Stand: Flightdeck 0.2.3, 5. Oktober 2026.

**VR:** Die optionale OpenXR-Einrichtung für WiVRn, SteamVR und Monado ist
verfügbar. DirectX-11-/DirectX-12-Stereo-Frames bestehen den Test auf AMD mit
simuliertem Monado-Headset. Echte Headsets, NVIDIA-Hardware und tatsächliche
MSFS-VR-Flüge sind noch nicht bestätigt. [Einrichtung und Teststand](vr.de.md).

**Komponentenupdates:** 0.1.10 behebt, dass ältere Store-Binaries bei angepassten
Startskripten erhalten blieben. Aktualisiere Flightdeck und öffne es bei
geschlossenem Simulator erneut; die Store-Prüfung sollte anschließend den
Komponentenschritt bestehen. Eigene Skripte bleiben erhalten. Die weiteren
Online-Prüfungen sind damit noch nicht bestätigt.

Die Behebung der folgenden gemeldeten Fehler ist noch nicht bestätigt. Sie betreffen nicht
jede Installation. Ein Anmeldefehler kann verhindern, dass die Darstellung im
Simulator geprüft wird.

## Aktueller Umfang

- **Microsoft-Anmeldung:** 0.1.16 verarbeitet Verifizierungsschritte innerhalb
  einer Antwort mit mehreren Tokens, verkürzte SOAP-Fehlerantworten und
  verschlüsselte Antworten, die bisher mit Code 74 scheiterten. Die Korrekturen
  für Sitzungscookies und Rückmeldungen aus 0.1.15 bleiben enthalten. Vor einem
  erneuten Versuch aktualisieren und neu starten. Falls die Anmeldung weiter
  scheitert, den angezeigten Code und die Version melden; der Erfolg ist noch
  nicht auf allen betroffenen Systemen bestätigt. Ein Zusammenhang mit dem
  NVIDIA-Problem ist nicht bestätigt.
  [Korrektur und Prüfumfang (Englisch)](store-session-refresh.md#soap-response-correction-0116).
- **NVIDIA:** MSFS 2024 kann funktionierende Menüs und Overlays anzeigen,
  während Weltkarte und 3D-Hauptansicht schwarz bleiben. 0.1.11 korrigiert DXVKs
  wirkungslose Low-Latency-Abschaltung und verwendet automatisch das vollständige
  Kompatibilitätsprofil für DirectX 11 und 12. DLSS, Reflex und NVIDIA Frame
  Generation sind damit standardmäßig deaktiviert. Ein Nutzer meldete, dass die
  späteren Startkorrekturen die Hauptansicht wiederherstellen; das Ladevideo
  bleibt bei ihm schwarz. Das ist eine Nutzerrückmeldung, keine Bestätigung für
  alle NVIDIA-Karten und Treiber. Ein funktionierendes zweites Fenster allein
  bestätigt die Hauptansicht nicht.
  [Grafikmodi und aktueller Stand](graphics.de.md).
- **Fenix-Installation:** 0.2.3 korrigiert den reproduzierten ICU-Fehler beim
  Installationshook und wartet auf Installer-Unterprozesse. Der offizielle Hook
  besteht den Test in einem separaten Wine-Profil. Die allgemeine Warnung im
  gemeldeten Bild beweist die genaue Ursache beim Nutzer nicht. Erneut über
  **Mods → Fenix → Installer starten** ausführen und danach alle Fenster schließen.
  [Korrektur und Prüfumfang](native-ui.md#fenix-install-hook-warning).
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
