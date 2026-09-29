# NVIDIA-Grafik

[English](graphics.md)

**Flightdeck 0.1.9** enthält Upstream-Korrekturen für
NVIDIAs Verwaltung mehrerer Renderfenster. Ihr Kompatibilitätsmodus deaktiviert
zusätzlich den eigenständigen Reflex-Pfad in VKD3D. Die Behebung der schwarzen
Hauptszene auf RTX 4060/5060 Ti ist damit noch nicht auf NVIDIA-Hardware bestätigt.
[Umfang, Build und Prüfungen](nvidia-renderer.md).

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

**Flightdeck 0.1.7** korrigiert einen Fehler bei der Grafikkartenauswahl:
Wine kann einen anderen GPU-Namen als Linux melden. Der bisherige automatische
Namensfilter konnte dadurch die gewünschte Karte ausschließen. Die Auswahl
verwendet jetzt die stabile Geräte-ID; sie bleibt auch beim Verbergen der
NVIDIA-Kennung im Kompatibilitätsmodus gültig. Die öffentliche Version 0.1.6
enthält diese Korrektur noch nicht.

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

Eine schwarze 3D-Hauptansicht bei funktionierenden Menüs wird weiterhin auf
NVIDIA gemeldet, darunter Systeme mit RTX 4060, RTX 4080 und RTX 5060 Ti. Auf
betroffenen Installationen haben beide Grafikmodi die Hauptansicht nicht
wiederhergestellt. In einem Bericht beendete ein Treiberupdate die Abstürze
beim zweiten Renderfenster; das Hauptfenster blieb jedoch schwarz. Ein zweites
Renderfenster ist deshalb keine bestätigte Umgehung des Fehlers.
Siehe [bekannte Probleme](known-issues.de.md).

Flightdeck 0.1.9 ist noch kein bestätigter Fix für diesen Fehler. Zur Bestätigung müssen
Menü-Globus, Free-Flight-Karte, Hauptansicht im Cockpit und ein zweites Fenster
sowie ein erneuter Spielstart geprüft werden. Die
[Entwicklerreferenz](nvidia-steam-parity.md) hält diesen Umfang und die
Upstream-Nachweise fest. Diese Anleitung verlangt keine wiederholten
Diagnoseexporte desselben Fehlers.

Die Dateiprüfung repariert das Basisspiel. Das Zurücksetzen der Wine-Umgebung
erstellt ein neues Profil. Beides ersetzt keine Grafiktreiber oder bestätigt die
Behebung eines Darstellungsfehlers. Probiere zunächst die Grafikeinstellungen,
bevor du eine ansonsten funktionierende Installation zurücksetzt.

Details zu eigenen Runtimes und Umgebungsvariablen stehen in der
[Runtime-Referenz](runtime.md#nvidia-graphics-in-launcher-managed-starts).
