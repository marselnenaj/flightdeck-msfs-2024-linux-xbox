# NVIDIA-Grafik

[English](graphics.md)

Flightdeck richtet die NVIDIA-Grafik für MSFS 2024 und MSFS 2020 ein. Dafür
verwendet es die Grafikkomponenten des Runners und den unter Linux installierten
NVIDIA-Treiber. Auf Rechnern mit einer dedizierten NVIDIA-Karte und integrierter
Grafik wählt es die NVIDIA-Karte einheitlich für DXGI und DirectX 12.

Ein funktionierender Vulkan-Treiber ist Voraussetzung. Installiere den empfohlenen
NVIDIA-Treiber über die Softwareverwaltung deiner Distribution und starte Linux
nach einem Treiberwechsel neu. Für DLSS werden zusätzlich die NGX-Komponenten
des Treibers benötigt. Flightdeck installiert keine Systemtreiber. Eine erkannte
Grafikkarte allein bestätigt noch keine korrekte Darstellung im Simulator.

Die NVIDIA-Starteinrichtung ist implementiert. Darstellung und Flugstabilität
sind noch nicht für die verschiedenen NVIDIA-Karten und Treiberversionen
bestätigt. Erfolgreiche Steam-/Proton-Berichte liefern Hinweise zur Kompatibilität,
bestätigen aber nicht den separaten Xbox-PC-Startweg von Flightdeck.

## Modus auswählen

Die Modusauswahl ist ab **Flightdeck 0.1.6** verfügbar. Aktualisiere über
**Updates → Flightdeck**, falls sie fehlt. Siehe [Änderungsübersicht](changelog.md).

1. Wähle den Simulator in Flightdeck und beende ein laufendes Spiel oder eine Einrichtung.
2. Öffne **Einrichtung → NVIDIA-Grafik**.
3. Wähle einen Modus, klicke **Modus speichern** und starte den Simulator.

| Modus | Verhalten |
| :--- | :--- |
| **Automatisch (NVIDIA-Funktionen nutzen)** | Richtet NVAPI und Optical Flow aus dem Runner sowie verfügbare NGX-Komponenten aus dem installierten Treiber ein. Explizite Einstellungen über Umgebungsvariablen bleiben wirksam. |
| **Kompatibilität (ohne NVIDIA-Zusatzfunktionen)** | Deaktiviert NVAPI, Optical Flow und NGX für den Spielprozess und unterdrückt die NVIDIA-Herstellerkennung gegenüber Wine und DXGI. Bei schwarzer Welt oder NVIDIA-Abstürzen ausprobieren. DLSS und NVIDIA Frame Generation stehen in diesem Modus nicht zur Verfügung. |

Die Auswahl wird für jede Installation separat gespeichert und gilt ab dem
nächsten Spielstart. Flightdeck muss dafür nicht neu gestartet und die
Wine-Umgebung nicht zurückgesetzt werden. Mit Automatisch wird beim nächsten
Start wieder die normale Einrichtung verwendet. Der Kompatibilitätsmodus ändert
keine Systemtreiber, entfernt keine Grafikbibliotheken und wechselt nicht auf
integrierte Grafik.

Dieser Modus beruht auf einem
[MSFS-2024-Kompatibilitätsbericht im Proton-Projekt](https://github.com/ValveSoftware/Proton/issues/9641).
Er dient zur Fehlerbehebung, garantiert aber keine Lösung für jeden schwarzen
Bildschirm. Auf Rechnern ausschließlich mit AMD-/Intel-Grafik wird die
NVIDIA-Auswahl nicht angezeigt.

## Wenn die Darstellung weiterhin fehlschlägt

Teste dieselbe Szene einmal pro Modus. Lade nach jedem Versuch unter
**Diagnose** einen neuen Bericht und speichere ihn. Gib bei einer Fehlermeldung
Flightdeck-Version, Simulator-Ausgabe, Grafikkarte, Treiberversion und den
Zeitpunkt des Darstellungsfehlers an. Der Bericht enthält verfügbare
Vulkan-Grafikkarten und die angeforderten Einstellungen des letzten Starts über
Flightdeck; er bestätigt keine erfolgreiche Darstellung.

Die Dateiprüfung repariert das Basisspiel. Das Zurücksetzen der Wine-Umgebung
erstellt ein neues Profil. Beides ersetzt keine Grafiktreiber oder bestätigt die
Behebung eines Darstellungsfehlers. Probiere zunächst die Grafikeinstellungen,
bevor du eine ansonsten funktionierende Installation zurücksetzt.

Details zu eigenen Runtimes und Umgebungsvariablen stehen in der
[Runtime-Referenz](runtime.md#nvidia-graphics-in-launcher-managed-starts).
