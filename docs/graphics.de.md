# NVIDIA-Grafik

[English](graphics.md)

**Der NVIDIA-Betrieb ist durch Nutzertests bestätigt.** Die Hauptansicht von
MSFS 2024 funktioniert in den gemeldeten Konfigurationen. Kleinere Probleme können
weiter auftreten, etwa ein fehlendes oder schwarzes Video beim Start. Die
Bestätigung gilt für die normale Monitordarstellung; sie ist keine Freigabe für
jede GPU-/Treiberkombination, VR, DLSS oder Frame Generation.

Verwende zunächst **Automatisch** und halte Flightdeck über **Updates → Flightdeck**
aktuell. Nach einem Moduswechsel den Simulator schließen und erneut starten.
[Aktuelle Einschränkungen](known-issues.de.md) · [Technischer Hintergrund](nvidia-renderer.md)

Flightdeck verwendet die Grafikkomponenten des Runners und den unter Linux
installierten NVIDIA-Treiber. Ein funktionierender Vulkan-Treiber ist erforderlich.
Verwende den empfohlenen Treiber deiner Distribution und starte Linux nach
einem Treiberwechsel neu. Flightdeck installiert keine Systemtreiber. Bei einer
einzelnen dedizierten NVIDIA-Karte wird dieselbe physische Karte für DXGI und
DirectX 12 über ihre stabile Geräte-ID ausgewählt, auch bei verborgener Herstellerkennung.

## Modus auswählen

1. Wähle den Simulator und beende ein laufendes Spiel oder eine Einrichtung.
2. Öffne **Einrichtung → NVIDIA-Grafik**.
3. Wähle einen Modus, klicke **Modus speichern** und starte den Simulator.

| Modus | Verhalten |
| :--- | :--- |
| **Automatisch (Kompatibilität bevorzugen)** | Verwendet das unten beschriebene Kompatibilitätsprofil. Das gilt auch nach dem Update einer bereits gespeicherten automatischen Einstellung. |
| **Kompatibilität (ohne NVIDIA-Zusatzfunktionen)** | Deaktiviert NVAPI, Optical Flow, NGX und NVIDIA Low Latency in DXVK/VKD3D. Verbirgt die NVIDIA-Herstellerkennung gegenüber Wine/DXGI. DLSS, Reflex und NVIDIA Frame Generation sind nicht verfügbar. |
| **NVIDIA-Funktionen (experimentell)** | Aktiviert die NVIDIA-Komponenten des Runners und verfügbare NGX-Komponenten des installierten Treibers. Entspricht dem bisherigen automatischen Verhalten. DLSS benötigt passende Treiberkomponenten; explizite Abschaltungen über Umgebungsvariablen bleiben wirksam. |

Die Auswahl wird pro Installation gespeichert und gilt beim nächsten Spielstart;
ein Neustart des Launcher-Dienstes ist nicht nötig. **NVIDIA-Funktionen
(experimentell)** stellt die Zusatzfunktionen beim nächsten Start wieder bereit.
Der Kompatibilitätsmodus entfernt keine DLLs, ändert keine Treiber und wählt keine
integrierte Grafik. AMD-/Intel-Systeme behalten ihre bisherige Grafikeinrichtung
und zeigen diese Auswahl nicht an.

Flightdeck gleicht vor dem Start auch die
gespeicherten Grafikoptionen von MSFS 2024 ab. Bei abgeschalteten NVIDIA-Funktionen
ersetzt Flightdeck gespeichertes DLSS durch TAA und deaktiviert Reflex sowie NVIDIA
Frame Generation, einschließlich ihrer VR-Einstellungen. Die ursprünglichen
Werte bleiben in der Wine-Umgebung gesichert. Beim Wechsel zu NVIDIA-Funktionen
werden nur Werte wiederhergestellt, die noch Flightdecks Änderung entsprechen.
FSR, andere Frame-Generatoren, Auflösung, Grafikqualität und spätere manuelle
Änderungen bleiben erhalten. Uneindeutige oder extern verknüpfte Dateien bleiben
unverändert.

Beide Simulator-Versionen starten mit `-FastLaunch`, um den Intro-Pfad zu
überspringen. Videowiedergabe und 3D-Hauptansicht sind getrennt zu prüfen: Ein
fehlendes Startvideo bedeutet für sich allein keinen Ausfall der NVIDIA-Darstellung.

Der vollständige Installer ersetzt nur erkannte Grafik-DLLs durch das abgestimmte
Paket. Eigene Grafikbibliotheken bleiben erhalten. Das reine Quellpaket enthält
die neu gebauten Bibliotheken nicht; verwende für die Korrektur den vollständigen Installer.

## Verbleibende Probleme melden

Das Startvideo kann fehlen oder schwarz bleiben, obwohl Menü, Karte und Cockpit
normal dargestellt werden. Treiberspezifische Probleme sind weiterhin möglich.
Bleibt auch die 3D-Hauptansicht schwarz, melde das getrennt mit Flightdeck-Version,
GPU, Treiber, Grafikmodus und einem [Diagnosebericht](problem-reports.de.md).

Frühere Versionen hatten Meldungen zur schwarzen Hauptansicht. Die damaligen
Untersuchungen und isolierten Tests bleiben im [technischen Hintergrund](nvidia-renderer.md)
erhalten; sie beschreiben nicht den allgemeinen aktuellen Kompatibilitätsstand.
Lokale AMD-Renderertests ersetzen keine NVIDIA-Nutzertests.

Eigene Einstellungen beschreibt die
[Runtime-Referenz](runtime.md#nvidia-graphics-in-launcher-managed-starts).
