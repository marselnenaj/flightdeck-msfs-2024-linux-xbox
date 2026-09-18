<p align="center">
  <img src="../ui/mark.svg" width="76" alt="Flightdeck-Logo">
</p>
<h1 align="center">Flightdeck</h1>
<p align="center">
  Ein Linux-Launcher für die Xbox-PC-Version von Microsoft Flight Simulator 2024.
</p>
<p align="center">
  <a href="#loslegen"><strong>Loslegen</strong></a> &nbsp;·&nbsp;
  <a href="addons.de.md">Mods installieren</a> &nbsp;·&nbsp;
  <a href="game-updates.md">Spielupdates</a> &nbsp;·&nbsp;
  <a href="../BUILDING.md">Selbst bauen</a> &nbsp;·&nbsp;
  <a href="../README.md">English</a>
</p>

![Originales Flightdeck-Flugzeugmotiv](../ui/flight-panorama.png)

Flightdeck installiert und startet deine **gekaufte Xbox-PC-/Microsoft-Store-
Version von MSFS 2024** unter Linux mit Wine/Proton. Mit dem Microsoft-Konto
anmelden, das Spiel herunterladen und im Launcher starten.

**Experimentell.** Der lokale Spielstart und ein kontrollierter Start mit einem
Flugzeug wurden bereits beobachtet. Online-Multiplayer wurde unter Linux als
funktionierend bestätigt. Der automatische Abgleich von Xbox-Cloud-Spielständen ist experimentell.

## Loslegen

**1. Flightdeck installieren**

**Flightdeck-Linux-x86_64.tar.gz** auf der
[Release-Seite](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases)
herunterladen, entpacken und **Install Flightdeck.desktop** doppelklicken. Der
Dateimanager kann verlangen, diesen lokalen Starter als vertrauenswürdig zu
markieren.

**2. MSFS herunterladen**

**MSFS installieren** wählen, den Zielordner prüfen und mit dem Microsoft-Konto
anmelden, das die PC-Version besitzt. Flightdeck bereitet die Wine-Umgebung vor,
lädt das lizenzierte Spiel über Xodus und verbindet die fertige Installation.

**3. Über das Anwendungsmenü starten**

**Flightdeck** öffnen und MSFS starten. Der lokale Hintergrunddienst startet
automatisch. Du musst keinen Server starten und kein Terminal offen halten.

Windows, Xbox-App, Microsoft Store und eine vorhandene MSFS-Installation werden
nicht vorausgesetzt. Deine gekaufte PC-Lizenz und eine Internetverbindung sind
weiterhin erforderlich.

<details>
<summary><strong>Systemanforderungen und weitere Installationswege</strong></summary>

Das aktuelle Paket benötigt Linux x86-64 mit **glibc 2.39+**, Python 3.10.12+,
Vulkan-Grafiktreiber und eine grafische Sitzung mit Linux-Secret-Service-
Schlüsselbund. GTK 3, WebKitGTK 4.1, OpenSSL 3 sowie GStreamer Good/Bad/Libav
müssen vorhanden sein. Für die Erstinstallation mindestens **100 GiB freien
Speicher** vorsehen; Updates und Reparaturen behalten zusätzlich das bisherige
Spielpaket.

Getestet wurde Arch Linux. Andere Distributionen benötigen kompatible
Bibliotheken und sind noch nicht bestätigt. Die Einrichtung zeigt fehlende
Voraussetzungen an. Chromium stellt, sofern verfügbar, das Anwendungsfenster;
sonst öffnet sich der Standardbrowser.
Fehlende Systempakete über die Softwareverwaltung deiner Distribution installieren.

- [Installationsdetails](install.md)
- [Vorbereitete Runtime verbinden](runtime.md)
- [Komponenten selbst bauen](../BUILDING.md)

`./install.sh` führt dieselbe Installation im Benutzerkonto aus. Aus einem
neueren entpackten Paket gestartet aktualisiert es Flightdeck.
`flightdeck --rollback` stellt die vorherige **Launcher-Version** wieder her.
`flightdeck --uninstall` entfernt die verwalteten Launcher-Dateien und erhält
Runtime, Einstellungen und Spielstände. `sudo` ist nicht erforderlich.
Die Installation fehlender Linux-Systempakete kann Administratorrechte benötigen.

</details>

## Im Launcher

![Flightdeck-Übersicht](images/launcher-overview.png)

<sub>Der Launcher mit Beispieldaten einer Installation.</sub>

| Bereich | Funktion |
| :--- | :--- |
| **Installation** | Microsoft-Anmeldung, Zielordner und Download deiner gekauften PC-Version, mit MB/GB und Prozentanzeige bei bekannter Gesamtgröße. |
| **Pause und Fortsetzen** | Fertige, geprüfte Dateien bleiben während der laufenden Installationssitzung erhalten. |
| **Spielupdates** | Installierte und verfügbare Store-Version vergleichen, Update herunterladen und geprüft aktivieren. Die vorherige Version bleibt für eine Rückkehr erhalten. |
| **Prüfen und reparieren** | Spieldateien mit den ursprünglichen Download-Prüfsummen vergleichen. Bei fehlenden oder beschädigten Dateien eine vollständige Reparatur vorbereiten, auch mit derselben Spielversion. |
| **Mods** | Den tatsächlichen Community-Ordner öffnen und Paketnamen, Versionen und Hersteller sehen. |
| **Lokale Spielstände** | Lokal speichern und bei beendetem Simulator Backups erstellen. |
| **Xbox-Cloud-Spielstände** | Vor dem Spielen den Cloud-Stand laden, nach dem Beenden Änderungen hochladen und lokale Sicherungen behalten. Experimentell. |
| **Diagnose** | Ausgewählte Prüfungen exportieren, ohne rohe Spiellogs, Kontotokens oder Spielstandinhalte. |
| **Deutsch und Englisch** | Sprache der Oberfläche wechseln; auch Installer und Kommandozeile sind übersetzt. |

Beim Pausieren können bis zu vier noch unvollständige Dateien neu beginnen.
Fortsetzen nach einem Neustart des Hintergrunddienstes oder Rechners ist noch
nicht implementiert. Spielupdates laden das vollständige Store-Basispaket;
Zusatzinhalte werden von MSFS und den jeweiligen Add-on-Installern verwaltet.
[Download-Verhalten](download-pause.md) · [Updates, Prüfung und Reparatur](game-updates.md)

Die angezeigte Downloadgröße umfasst die Dateien des Store-Basispakets. Bei
MSFS 1.8.16.0 waren das rund **9,9 GB**; die Größe kann sich mit der Spielversion
ändern. Wine/Proton, im Spiel nachgeladene Inhalte, Caches und Add-ons brauchen
zusätzlichen Speicher. Der Simulator läuft lokal und lädt weitere Inhalte bei
Bedarf nach. Deshalb weiterhin mindestens **100 GiB freien Speicher** vorsehen.
[Downloadgröße und Speicherbedarf](install.md#download-size-and-storage)

Die Dateiprüfung benötigt einen vollständigen Prüfnachweis aus einem erfolgreichen
Flightdeck-Download. Bei älteren Installationen ohne diesen Nachweis ist zuerst
eine vollständige Reparatur nötig. Sie lädt das gesamte verfügbare Basispaket
neu und erhält die bisherige Installation. Der separate Community-Ordner und
Spielstände bleiben bestehen.

## Aktueller Stand

| Bereich | Nachweis |
| :--- | :--- |
| **Simulator** | Cockpit erreicht und ein kontrollierter Flugzeugstart auf einem Entwicklungssystem durchgeführt. |
| **Lokale Spielstände** | Laden nach einem Neustart und lokale Backups getestet. |
| **Kostenlose Store-Inhalte** | Download in einem Nutzertest erfolgreich. |
| **Kostenpflichtiger Marketplace** | Kaufabschluss und vollständiger DLC-Bestand noch nicht bestätigt. |
| **Multiplayer** | Online-Multiplayer unter Linux als funktionierend gemeldet. Gruppeneinladungen müssen separat getestet werden. |
| **Xbox-Cloud-Saves** | Automatischer Abgleich bei Spielstart und Spielende, lokale Backups und Konfliktbehandlung implementiert. Nativer Cloud-Zugriff getestet; ein Spieltest über mehrere Geräte steht aus. [Details](cloud-saves.de.md) |
| **FlyByWire A32NX** | MSFS-2024-Version Stable 2024.1.0 installiert und erkannt; Flugtest offen. |
| **SimBridge** | Dienstprüfung, Web-MCDU, WebSocket und Geländedateninitialisierung unter Wine getestet; Spielverbindung offen. |
| **Fenix A320** | Offizielle Installation von 2.4.0.4720 mit Wine/.NET-Anpassungen abgeschlossen, Community-Paket erkannt. Begleitanwendung und Flugzeugsysteme noch nicht bestätigt. |

Installation und Updates haben Komponenten-, simulierte Ablauf- und Browsertests.
Ein vollständiger frischer MSFS-Download, Runtime-Einrichtung, Pause/Fortsetzen
und die vollständige Dateiprüfung wurden zusätzlich auf einem Arch-Linux-Rechner
mit getrennten Launcher- und Kontodaten erfolgreich geprüft. Die Installation
auf einem frischen Betriebssystem, das Erreichen des Hauptmenüs nach dieser
frischen Spielinstallation und ein Flug nach einem echten Store-Update sind noch
nicht nachgewiesen. Ein Eintrag in der Mod-Liste bestätigt lokale
Paketdateien, nicht Lizenzaktivierung oder funktionierende Cockpit-Systeme.

[Mod-Anleitung](addons.de.md) · [Marketplace-Umfang](marketplace-collections.md) ·
[Multiplayer testen](multiplayer.md)

## Lokale Daten

Der Launcher verwendet die Python-Standardbibliothek und lokale HTML-, CSS- und
JavaScript-Dateien. Sein Dienst lauscht nur auf Loopback. Herkunftsprüfungen und
ein Sitzungstoken schützen Aktionen. Der Launcher enthält keine Telemetrie und
braucht kein CDN. Microsoft-Anmeldung, Downloads und Online-Inhalte des Spiels
verwenden weiterhin ihre jeweiligen Netzwerkdienste.

Runtime, Zugangsdaten, Spielstände und Logs gehören nicht zu den Quell- oder
Release-Archiven. Die Oberfläche kann lokale Installationspfade anzeigen;
Screenshots vor dem Teilen prüfen. Das Schließen von Flightdeck beendet einen
bereits laufenden Simulator nicht.

## Mitentwickeln

Die [Build-Anleitung](../BUILDING.md), der [Beitragsleitfaden](contributing.md)
und die [Prüfbefehle in der englischen README](../README.md#documentation-and-development)
beschreiben den Einstieg. Tests verwenden isolierte Beispieldaten, keine echten
Konten oder Spielinstallationen.

## Lizenz

Launcher, Oberfläche und neue Integrationstools stehen unter [MIT](../LICENSE).
Xodus behält GPL-3.0-only, Wine/GDK-Komponenten LGPL-2.1-or-later. Native Pakete
enthalten die Lizenzhinweise und vollständigen zugehörigen Quellen. Der separat
geladene Runner behält seine eigenen Lizenzen. Die mitgelieferte Schrift Manrope
steht unter der SIL Open Font License 1.1.

[Lizenzhinweise](../compat/THIRD_PARTY_NOTICES.md) ·
[Paket-Herkunft](binary-release.md) · [Grafiken und Schrift](artwork.md)

Flightdeck ist ein unabhängiges Projekt und wird nicht von Microsoft, Xbox,
Asobo oder den Upstream-Projekten unterstützt. MSFS-Dateien, kostenpflichtige
Add-ons, proprietäre SDK-Header, Kontozugangsdaten und Spiellizenzen werden nicht
mitgeliefert. Das Titelbild ist ein originales Flightdeck-Motiv und kein
Simulator-Screenshot.
