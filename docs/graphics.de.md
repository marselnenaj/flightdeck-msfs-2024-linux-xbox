# NVIDIA-Grafik

[English](graphics.md)

**Flightdeck 0.1.11** verwendet auf NVIDIA automatisch Kompatibilitätseinstellungen.
NVIDIA Low Latency wird für DirectX 11 und 12 deaktiviert; DXVKs bislang
wirkungslose Abschaltoption ist korrigiert. Die bisherigen VKD3D-Korrekturen für
mehrere Renderfenster sind enthalten. Damit ist die unvollständige Einrichtung
korrigiert; die Behebung der schwarzen MSFS-Hauptansicht muss noch auf
NVIDIA-Hardware bestätigt werden. [Build und Prüfungen](nvidia-renderer.md).

Aktualisiere über **Updates → Flightdeck**, schließe das Update ab und starte
den Simulator erneut. Eine bestehende Einstellung **Automatisch** übernimmt das
neue Profil. Eine Neuinstallation von MSFS, das Zurücksetzen der Wine-Umgebung
oder zusätzliche Startparameter sind dafür nicht erforderlich.

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

Die native Testversion 0.2.0-dev.1 gleicht vor dem Start auch die
gespeicherten Grafikoptionen von MSFS 2024 ab. Bei abgeschalteten NVIDIA-Funktionen
ersetzt er gespeichertes DLSS durch TAA und deaktiviert Reflex sowie NVIDIA
Frame Generation, einschließlich ihrer VR-Einstellungen. Die ursprünglichen
Werte bleiben in der Wine-Umgebung gesichert. Beim Wechsel zu NVIDIA-Funktionen
werden nur Werte wiederhergestellt, die noch Flightdecks Änderung entsprechen.
FSR, andere Frame-Generatoren, Auflösung, Grafikqualität und spätere manuelle
Änderungen bleiben erhalten. Uneindeutige oder extern verknüpfte Dateien bleiben
unverändert. Diese Anpassung ist noch nicht im veröffentlichten Paket 0.1.22.

Der Launcher 0.2.0-dev.1 startet beide Simulator-Versionen außerdem mit
`-FastLaunch`. Das übernimmt einen [berichteten Workaround für die schwarze
Hauptansicht](nvidia-renderer.md#intro-startup-workaround) auch beim
Standard-Runner und den ausgewählten Proton-Versionen. Die Wirkung beim
gemeldeten Linux-/NVIDIA-Fall ist damit noch nicht bestätigt.

Der vollständige Installer ersetzt nur erkannte Grafik-DLLs durch das abgestimmte
Paket. Eigene Grafikbibliotheken bleiben erhalten. Das reine Quellpaket enthält
die neu gebauten Bibliotheken nicht; verwende für die Korrektur den vollständigen Installer.

## Verbleibende Einschränkung

Funktionierende Menüs bei schwarzem Globus, schwarzer Free-Flight-Karte oder
Cockpit-Hauptansicht bleiben ein gemeldetes NVIDIA-Problem. Die Modi früherer
Versionen haben nicht jeden Fall behoben. Ein funktionierendes zweites Fenster
bestätigt nicht die Behebung der Hauptansicht. Siehe [bekannte Probleme](known-issues.de.md)
und die [Steam-Kompatibilitätsreferenz](nvidia-steam-parity.md).

Ein betroffener Tester hat **0.1.18**, **Automatisch** und den korrigierten
Renderer-Build `628afa6f9cfece4` bestätigt. Globus/Karte und Cockpit bleiben
auf seinem System schwarz. Die 3D-Texturkorrektur behebt diesen Fall somit nicht.

Flightdeck 0.1.19 bietet unter **Einrichtung → Proton-Version
(experimentell)** eine Auswahl installierter Proton-Versionen mit eigener
Windows-Umgebung und Rückkehr zum bisherigen Flightdeck-Runner. Sie ermöglicht den
Vergleich ganzer Wine-/DXVK-/VKD3D-Versionen; eine Behebung des NVIDIA-Problems
ist damit noch nicht bestätigt. [Ablauf und Grenzen](runtime.md#experimental-proton-selection).

Der lokale Render-Test umfasst gleichzeitig verwendete DirectX-11-/12-Geräte,
Haupt- und Zweitfenster, Größenänderungen und das Schließen von Fenstern. Er
besteht auf AMD-Hardware und bestätigt damit weder NVIDIA-Treiberverhalten noch
MSFS-Flugstabilität.

Eigene Einstellungen beschreibt die
[Runtime-Referenz](runtime.md#nvidia-graphics-in-launcher-managed-starts).
