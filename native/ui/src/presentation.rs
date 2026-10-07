//! Interpret persisted add-on state before presenting the setup workflow.
//! In particular, a legacy patch is active but is not a managed-patch install.
use crate::model::{s, yes};
use serde_json::Value;

pub(crate) struct AddonProgress {
    pub title: &'static str,
    pub detail: &'static str,
    pub steps: Vec<bool>,
    pub next: usize,
    pub ready: bool,
    pub busy: &'static str,
}

pub(crate) fn fenix(data: &Value, status: &Value) -> AddonProgress {
    let patched = data["state"] == "installed" && yes(data, "installed");
    let aircraft = patched && yes(data, "fenix_installed");
    let settings = aircraft && yes(data, "settings_ready");
    let ready = settings && yes(data, "configured");
    let supported =
        ["available", "installed"].contains(&s(data, "state")) || yes(data, "can_retry");
    let next = if ready {
        0
    } else if settings {
        4
    } else if aircraft {
        3
    } else if patched {
        2
    } else {
        1
    };
    let mut progress = AddonProgress {
        title: "Status wird geladen …",
        detail: "",
        steps: Vec::new(),
        next,
        ready,
        busy: "",
    };
    if supported {
        progress.steps = vec![patched, aircraft, settings, ready];
        progress.title = if ready {
            "Fenix ist startbereit"
        } else if next == 2 && yes(data, "manager_installed") {
            "Fenix-App erkannt · Flugzeug noch nicht installiert"
        } else {
            "Einrichtung noch nicht abgeschlossen"
        };
        progress.detail = match next {
            0 => {
                "Starte MSFS 2024 ganz normal in Flightdeck. Fenix startet automatisch mit dem Spiel und wird beim Beenden mit geschlossen."
            }
            1 => {
                "Schritt 1 von 4: Richte zuerst den Linux-Patch ein. Flightdeck lädt das geprüfte Paket automatisch herunter."
            }
            2 if yes(data, "manager_installed") => {
                "Schritt 2 von 4: Öffne die vorhandene Fenix-App und installiere dort dein Flugzeug. Schließe die App danach vollständig."
            }
            2 => {
                "Schritt 2 von 4: Lade den offiziellen Fenix-Installer herunter, wähle die EXE aus und installiere dein Flugzeug."
            }
            3 => {
                "Schritt 3 von 4: Öffne Fenix, melde dich an und schließe das Programm danach vollständig."
            }
            _ => {
                "Schritt 4 von 4: Schließe Fenix und klicke auf „Einrichtung abschließen“. Damit werden Anzeigen und automatischer Start eingerichtet."
            }
        };
        if yes(data, "can_retry") {
            progress.title = "Einrichtung kann automatisch repariert werden";
            progress.detail = "Klicke auf „Einrichtung reparieren“. Flightdeck prüft und repariert .NET im kopierten Profil. Ein PC-Neustart oder manuelles Wiederherstellen ist dafür nicht nötig.";
        }
    } else if data["state"] == "legacy" {
        progress.title = "Vorhandene Fenix-Einrichtung";
        progress.detail = "Dein lokaler Fenix-Patch bleibt aktiv. Eine erneute Installation über diesen Assistenten ist nicht erforderlich.";
    } else if yes(data, "can_restore") {
        progress.title = "Einrichtung unterbrochen · Wiederherstellung verfügbar";
        progress.detail =
            "Öffne die Wiederherstellung unten, bevor du die Einrichtung erneut startest.";
    } else if data["state"].is_string() {
        progress.title = "Für diese Runtime nicht verfügbar";
    }
    let active = data["job"]["state"] == "running";
    let interactive = ["installer", "open", "manager"].contains(&s(&data["job"], "operation"));
    if active && !yes(&data["job"], "stopping") && interactive {
        progress.busy = "Eine Windows-Anwendung läuft noch in dieser Installation. Schließe den Fenix-Installer und Fenix nach der Anmeldung vollständig. Flightdeck aktualisiert den Status automatisch.";
    } else if !active && status["game"]["state"] == "external" {
        progress.busy = "Diese Installation wird gerade verwendet. Beende MSFS oder die andere laufende Einrichtung, bevor du Fenix änderst.";
    } else if !active && !["", "stopped"].contains(&s(&status["game"], "state")) {
        progress.busy = "MSFS läuft. Beende das Spiel, bevor du die Fenix-Einrichtung änderst.";
    } else if !active && data["idle"] == false {
        progress.busy = "Eine Windows-Anwendung läuft noch in dieser Installation. Schließe den Fenix-Installer und Fenix nach der Anmeldung vollständig. Flightdeck aktualisiert den Status automatisch.";
    } else if !active && yes(data, "busy") {
        progress.busy = "Eine andere Einrichtung läuft. Warte, bis sie abgeschlossen ist.";
    }
    if active {
        progress.ready = false;
        progress.title = if yes(&data["job"], "stopping") {
            "Fenix wird beendet …"
        } else {
            "Fenix-Einrichtung läuft …"
        };
        if !interactive || yes(&data["job"], "stopping") {
            progress.detail = "Bitte warte, bis der aktuelle Schritt abgeschlossen ist.";
        }
    }
    progress
}

pub(crate) fn gsx(data: &Value) -> AddonProgress {
    let steps = vec![
        yes(data, "prepared"),
        yes(data, "package_installed"),
        yes(data, "configured"),
    ];
    let next = steps.iter().position(|v| !v).map(|i| i + 1).unwrap_or(0);
    let mut progress = AddonProgress {
        title: "GSX-Status wird geladen …",
        detail: "",
        steps: Vec::new(),
        next,
        ready: next == 0,
        busy: "",
    };
    if data["state"] == "available" {
        progress.title = if next == 0 {
            "GSX eingerichtet · Flugtest ausstehend"
        } else {
            "GSX-Einrichtung"
        };
        progress.detail = match next {
            0 => {
                "Starte MSFS und prüfe das GSX-Menü sowie die Bodendienste. Der Flugbetrieb unter Linux ist noch nicht bestätigt."
            }
            1 => {
                "Schritt 1 von 3: Flightdeck lädt den geprüften FSDT-Installer und richtet .NET in einer Profilkopie ein."
            }
            2 => {
                "Schritt 2 von 3: Installiere und aktiviere GSX im offiziellen FSDT-Installer. Schließe ihn danach vollständig."
            }
            _ if yes(data, "startup_found") => {
                "Schritt 3 von 3: Übernimm den von FSDT angelegten automatischen Start."
            }
            _ => {
                "Schritt 3 von 3: Die FSDT-Starteinstellung fehlt. Führe im FSDT-Installer ein Update aus und prüfe erneut."
            }
        };
        progress.steps = steps;
    } else if yes(data, "can_recover") {
        progress.title = "GSX-Einrichtung unterbrochen";
        progress.detail = "Stelle zuerst das bisherige Windows-Profil wieder her. Danach kannst du die Vorbereitung erneut starten.";
    } else if data["state"].is_string() {
        progress.title = "GSX ist für diese Installation nicht verfügbar";
    }
    if data["job"]["state"] == "running" {
        progress.title = "GSX-Einrichtung läuft …";
        progress.detail = if data["job"]["operation"] == "open" {
            "Der FSDT-Installer ist geöffnet. Beende laufende Downloads und schließe ihn danach."
        } else {
            "Bitte warte, bis der aktuelle Schritt abgeschlossen ist."
        };
    }
    progress
}

pub(crate) fn launcher_update(data: &Value) -> &'static str {
    if data.is_null() || !data["_error"].is_null() {
        return "Updatestatus wird geladen …";
    }
    let job = &data["job"];
    if job["state"] == "running" {
        return match s(job, "phase") {
            "checking" => "Suche nach Flightdeck-Updates …",
            "downloading" => "Flightdeck wird heruntergeladen …",
            "verifying" => "Download wird geprüft …",
            "installing" => "Flightdeck wird installiert …",
            "restart" => "Flightdeck wird neu geöffnet …",
            _ => "Update wird vorbereitet …",
        };
    }
    if yes(data, "pending_restart") {
        "Bereit zum Neustart"
    } else if job["state"] == "failed" {
        "Update nicht abgeschlossen"
    } else if yes(data, "update_available") {
        "Flightdeck-Update verfügbar"
    } else if data["update_available"] == false {
        "Flightdeck ist aktuell"
    } else {
        "Noch nicht nach Updates gesucht"
    }
}

pub(crate) fn game_update(data: &Value) -> &'static str {
    if data.is_null() || !data["_error"].is_null() {
        return "Updatestatus wird geladen …";
    }
    let job = &data["job"];
    // A newer background discovery supersedes a historical manual job's
    // summary. The job and its diagnostics remain visible below the heading.
    if yes(data, "background_current") && !super::model::active(job) && job["state"] != "ready" {
        if yes(data, "background_checking") {
            return "MSFS-Version wird geprüft";
        }
        if yes(data, "auth_required") {
            return "Anmeldung zum Prüfen erforderlich";
        }
        if !s(data, "startup_error").is_empty() {
            return "Update nicht abgeschlossen";
        }
        if yes(data, "update_available") {
            return "Eine neue Spielversion ist verfügbar";
        }
        if data["update_available"] == false && !s(data, "latest_version").is_empty() {
            return "Deine Spielversion ist aktuell";
        }
    }
    if job["operation"] == "verify" && super::model::active(job) {
        return "Spieldateien werden geprüft";
    }
    if job["operation"] == "repair" && job["state"] == "ready" {
        return "Reparatur vorbereitet";
    }
    if job["operation"] == "repair" && job["state"] == "complete" {
        return "Reparatur abgeschlossen";
    }
    if job["operation"] == "verify" && job["state"] == "complete" {
        return if data["integrity"]["result"]["healthy"] == true {
            "Spieldateien geprüft"
        } else {
            "Dateiprüfung abgeschlossen"
        };
    }
    if data["available"] == false {
        return "Updates derzeit nicht verfügbar";
    }
    if (yes(data, "background_checking") && job.is_null()) || job["state"] == "checking" {
        return "MSFS-Version wird geprüft";
    }
    if job["state"] == "installing" {
        return match s(job, "phase") {
            "authentication" => "Microsoft-Anmeldung",
            "download" => "Update wird heruntergeladen",
            "pausing" => "Download wird pausiert",
            "paused" => "Download pausiert",
            "verify_update" => "Update wird geprüft",
            "switch_update" => "Spielversion wird gewechselt",
            _ => "Update wird vorbereitet",
        };
    }
    if yes(data, "auth_required") {
        "Anmeldung zum Prüfen erforderlich"
    } else if job["state"] == "failed" || !s(data, "startup_error").is_empty() {
        "Update nicht abgeschlossen"
    } else if job["state"] == "cancelled" {
        "Update abgebrochen"
    } else if yes(data, "update_available") && !s(data, "latest_version").is_empty() {
        "Eine neue Spielversion ist verfügbar"
    } else if data["update_available"] == false && !s(data, "latest_version").is_empty() {
        "Deine Spielversion ist aktuell"
    } else {
        "Noch nicht nach Updates gesucht"
    }
}
