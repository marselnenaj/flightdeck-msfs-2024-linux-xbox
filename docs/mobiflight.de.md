# MobiFlight und WinWing

[English instructions](mobiflight.md)

**Mods → MobiFlight / WinWing** verwendet experimentell das gewählte Xbox- oder
Steam-Profil von MSFS 2020/2024 und dessen Proton. Flightdeck lädt die offizielle
portable Version **11.3.1**, prüft Größe und SHA-256 und verwendet deren enthaltene
Windows-.NET-Laufzeit. Ein weiteres dauerhaftes Wine-Profil oder Linux-.NET ist
nicht nötig.

Der automatische Download verwendet MobiFlights öffentliches Release-ZIP;
eine separate Installer-Datei musst du nicht auswählen. Wenn der App-Download
fertig ist, aber die WASM-/Ereignis-Einrichtung fehlschlägt, wähle
**MobiFlight vorbereiten / reparieren**. Flightdeck setzt die vorhandene
Installation fort und erhält deine Einstellungen, ohne die App erneut zu laden.
Falls Fenix oder ein Proton-Wechsel seit dem Abbruch das Windows-Profil ersetzt
hat, archiviert Flightdeck das alte Einrichtungsprotokoll und erhält dessen
Zwischendateien. Die Reparatur startet einen neuen Versuch im ausgewählten Profil.

1. Die richtige Simulator-Installation wählen, MSFS und Begleitprogramme schließen.
2. **MobiFlight installieren** bzw. **MobiFlight vorbereiten / reparieren** wählen.
3. MobiFlight öffnen, Hardware und Konfiguration auswählen und die Verbindung im
   Flug prüfen. **MobiFlight mit MSFS starten** verwaltet optional den Autostart
   ausschließlich dieses Profils.

Die Vorbereitung installiert das offizielle WASM-Modul im tatsächlichen
Community-Ordner und lädt fehlende Ereignisdefinitionen vom offiziellen
HubHop-Dienst. Vorhandene Module, Ereignisse und eigene `events.user.txt` bleiben
erhalten. Unbekannte Module bleiben unter deiner Kontrolle. Flightdeck aktiviert
keine input-remapper-Presets; exklusiver Gerätezugriff durch ein aktives Preset
kann ein Gerät vor Wine verbergen.

Flightdeck verwendet die aktuelle HubHop-Export-Adresse direkt, ohne Weiterleitungen.

Der HID-Fix wird aus der exakt geprüften offiziellen HidSharp-2.6.4-DLL gebaut.
Flightdeck sichert das Original und ersetzt nur diese unterstützte Version
atomar. Unbekannte DLL-Versionen bleiben unverändert. Quellcode, Prüfsummen,
Lizenz und Änderungen stehen in [compat/mobiflight](../compat/mobiflight/README.md).

Nur beim exakt geprüften Xodus-/Fenix-`winebus.sys` werden vier WinWing-Geräte-IDs
nach Registry-Sicherung in die bestehende `EnableHidraw`-Liste aufgenommen.
Andere Freigaben und ausdrücklich deaktivierte Geräte bleiben erhalten. Andere
Winebus-Versionen werden als ungeprüft gemeldet und nicht automatisch geändert.
SimAppPro, Firmware-Updates, alle WinWing-Modelle und vollständige Flüge sind
damit noch nicht bestätigt. Tests mit echter Hardware bleiben erforderlich.

## Linux-Geräteberechtigungen

Der Status prüft Lese-/Schreibrechte angeschlossener WinWing-hidraw-Geräte. Fehlen
sie, diese gezielte Regel mit dem üblichen Administrator-Editor deiner Distribution
unter `/etc/udev/rules.d/70-flightdeck-winwing.rules` anlegen:

```udev
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="4098", TAG+="uaccess"
```

Anschließend `sudo udevadm control --reload-rules` ausführen, das Gerät abziehen
und neu verbinden und den Flightdeck-Status aktualisieren. logind gibt es damit
für die aktive lokale Sitzung frei. Flightdeck prüft die Rechte derzeit ohne
privilegierte Systemänderung. Pauschale `MODE="0666"`-Regeln oder die Gruppe
`input` sind für diese Integration nicht erforderlich.

Beim Beenden des Simulators wird nur dessen aktiviertes MobiFlight-Begleitprogramm
geschlossen. Das Schließen von Flightdeck lässt einen aktiven Flug weiterlaufen.
Nach einem Proton-Wechsel den Add-on-Status neu laden.
