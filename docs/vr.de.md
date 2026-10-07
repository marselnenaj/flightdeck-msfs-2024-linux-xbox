# Virtual Reality

[English](vr.md)

Ab Flightdeck 0.1.18 kannst du MSFS 2024 und 2020 über OpenXR mit einem
Linux-VR-System verbinden. Öffne **Einrichtung → Virtual Reality**, wähle dein
VR-System und speichere den Modus. Verbinde das Headset und klicke auf
**Headset prüfen**. Starte danach MSFS über Flightdeck und wechsle im Simulator
mit **Strg+Tab** in den VR-Modus, sofern du die Standardbelegung verwendest.

VR ist standardmäßig ausgeschaltet. Jede Simulatorinstallation speichert ihre
eigene Auswahl; Änderungen gelten ab dem nächsten Start. Das VR-Programm und
das verbundene Headset müssen vor jedem Spielstart bereit sein.

## Voraussetzungen und Auswahl

Installiere eine passende VR-Runtime und den **64-Bit-OpenXR-Loader für Linux**
über deine Distribution oder nach Anleitung des jeweiligen Projekts.
Flightdeck installiert keine Grafiktreiber, Headset-Firmware oder
Streaming-Software.

- **Automatisch:** verwendet deine aktive Linux-OpenXR-Runtime. Bei mehreren
  installierten Systemen ohne aktive Registrierung wählst du eines ausdrücklich.
- **WiVRn:** für unterstützte Standalone-Headsets wie Quest oder Pico mit
  eingerichteter WiVRn-Verbindung. [Projekt und Einrichtung](https://github.com/WiVRn/WiVRn).
- **SteamVR:** verwendet die Runtime der installierten SteamVR-Anwendung.
  Starte SteamVR und verbinde das Headset vorher.
  [SteamVR unter Linux](https://github.com/ValveSoftware/SteamVR-for-Linux).
- **Monado:** für eine installierte Monado-Runtime mit einem unterstützten
  Headset. [Projekt und Einrichtung](https://monado.freedesktop.org/).
- **Aus:** Flightdeck bereitet beim Spielstart keine VR-Verbindung vor.

Eine fehlerhafte aktive Registrierung wird gemeldet. Flightdeck wechselt dann
nicht unbemerkt zu einer anderen Runtime. Das gilt auch für eine ausdrücklich
gesetzte Umgebungsvariable `XR_RUNTIME_JSON`. Bibliotheken müssen vom
Linux-Spielprozess aus erreichbar sein: Verweist ein Flatpak-Manifest auf ein
nur innerhalb der Sandbox vorhandenes `/app`-Verzeichnis, nutze die vom Projekt
vorgesehene Host-Anbindung oder ein natives Paket.

## AMD und NVIDIA

Beide Hersteller verwenden dieselbe OpenXR-/Vulkan-Anbindung. Flightdeck liest
die vom VR-System verwendete GPU aus und ordnet auch dem Spiel diese GPU zu.
Entferne manuelle GPU-Namens- und Indexfilter. Eine widersprüchliche explizite
GPU-UUID wird als Fehler gemeldet.

Beginne bei NVIDIA mit dem Grafikmodus **Automatisch**. Er deaktiviert DLSS,
Reflex, NVIDIA Frame Generation und NVAPI zugunsten der Kompatibilität.
Die experimentellen NVIDIA-Funktionen sind für VR nicht erforderlich.
Die [normale NVIDIA-Darstellung](graphics.de.md) ist durch Nutzertests bestätigt;
VR muss gesondert geprüft werden. Treiber, Runtime, Headset und bei drahtloser Verbindung
auch Encoder und Netzwerk beeinflussen, ob eine Kombination funktioniert.

## Fehler eingrenzen

Wird keine Runtime gefunden, starte dein VR-Programm und wähle dort dessen
OpenXR-Runtime erneut aus. Fehlt das Headset, verbinde oder aktiviere es vor
der nächsten Prüfung. Bei fehlendem Loader installierst du die
OpenXR-Systempakete deiner Distribution. Bei einem Zeitlimit starte dein
VR-Programm neu. Verwende den von Flightdeck eingerichteten Runner, wenn
OpenXR-Komponenten fehlen.

Bei unterschiedlichen GPUs müssen Spiel und VR-System derselben Grafikkarte
zugeordnet werden. Entferne gegebenenfalls `DXVK_FILTER_DEVICE_NAME`,
`VKD3D_FILTER_DEVICE_NAME` und `VKD3D_VULKAN_DEVICE`; eine gesetzte
`DXVK_FILTER_DEVICE_UUID` muss zur GPU des VR-Systems passen.

Eine erfolgreiche Prüfung bestätigt, dass OpenXR Headset und GPU erreicht.
Das Bild und Tracking im Simulator musst du anschließend prüfen. Ohne Bild
kontrolliere zuerst die VR-Tastenbelegung und teste eine native
OpenXR-Anwendung mit derselben Runtime.

Während Spiel, Cloud-Abgleich oder Einrichtung laufen, sind VR-Änderungen und
Prüfungen gesperrt. Schlägt die VR-Vorbereitung beim Start fehl, erscheint eine
Fehlermeldung. Mit **Aus** kehrst du zum normalen Startablauf zurück.
Diagnosen enthalten Modus und Prüfergebnis, keine Runtime-Pfade, Geräte-UUIDs
oder ungefilterten Treiberausgaben.

## Teststand

Mit einer AMD Radeon RX 6900 XT und simuliertem Monado-Headset wurden native
OpenXR-Erkennung, Windows-Manifest, DirectX-11- und DirectX-12-Sitzungen,
Stereo-Bildpuffer, Tracking-Daten und Frame-Übergabe durch Wine erfolgreich
geprüft. Dazu kommen automatisierte Tests für Auswahl, NVIDIA-kompatible
Vorbereitung, Fehlerfälle, Sperren und die deutsche/englische Oberfläche.
[Technische Reproduktion](vr-validation.md).

Echte Headsets, NVIDIA-Hardware, WiVRn-Streaming, SteamVR-Headsets,
Motion-Controller, Latenz und ein tatsächlicher MSFS-VR-Flug sind für dieses
Release **noch nicht validiert**. Die Anbindung ist dafür verfügbar; eine
vollständig bestätigte Hardware-Kompatibilität wird damit nicht behauptet.
