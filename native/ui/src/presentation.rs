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
    if !data["_error"].is_null() {
        return AddonProgress {
            title: "Fenix-Status derzeit nicht verfügbar",
            detail: "Lade den Status neu, bevor du die Einrichtung änderst.",
            steps: Vec::new(),
            next: 0,
            ready: false,
            busy: "",
        };
    }
    let patched = data["state"] == "installed" && yes(data, "installed");
    // Fenix.exe and settings files are local installation evidence. Neither
    // establishes an installed Community aircraft, an account or a valid license.
    let companion = patched && yes(data, "fenix_installed");
    let settings = companion && yes(data, "settings_ready");
    let ready = settings && yes(data, "configured");
    let supported =
        ["available", "installed"].contains(&s(data, "state")) || yes(data, "can_retry");
    let next = if ready {
        0
    } else if settings {
        4
    } else if companion {
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
        progress.steps = vec![patched, companion, settings, ready];
        progress.title = if ready {
            "Fenix lokal eingerichtet"
        } else if next == 2 && yes(data, "manager_installed") {
            "Fenix-App erkannt · Einrichtung unvollständig"
        } else {
            "Einrichtung noch nicht abgeschlossen"
        };
        progress.detail = match next {
            0 => {
                "Die lokale Einrichtung ist abgeschlossen. Anmeldung und Lizenz prüft Fenix selbst."
            }
            1 => {
                "Richte zuerst den Linux-Patch ein. Flightdeck lädt das geprüfte Paket automatisch herunter."
            }
            2 if yes(data, "manager_installed") => {
                "Öffne die vorhandene Fenix-App und vervollständige dort die Installation. Schließe die App danach."
            }
            2 => "Lade den offiziellen Fenix-Installer herunter und wähle die EXE aus.",
            3 => "Öffne Fenix, melde dich dort an und schließe das Programm danach.",
            _ => {
                "Schließe Fenix und übernimm die Einstellungen für Anzeigen und automatischen Start."
            }
        };
        if yes(data, "can_retry") {
            progress.ready = false;
            progress.next = 1;
            progress.title = "Einrichtung kann automatisch repariert werden";
            progress.detail = "Repariere zuerst die Linux-Einrichtung. Flightdeck prüft .NET im kopierten Profil.";
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
    let current = data["_error"].is_null();
    let supported = current && data["state"] == "available";
    let prepared = supported && yes(data, "prepared");
    let package = prepared && yes(data, "package_installed");
    let configured = package && yes(data, "startup_found") && yes(data, "configured");
    let steps = vec![prepared, package, configured];
    let next = steps.iter().position(|v| !v).map(|i| i + 1).unwrap_or(0);
    let mut progress = AddonProgress {
        title: "GSX-Status wird geladen …",
        detail: "",
        steps: Vec::new(),
        next,
        // This means local setup only; the backend has no simulator or license proof.
        ready: configured,
        busy: "",
    };
    if !current {
        progress.title = "GSX-Status derzeit nicht verfügbar";
    } else if supported {
        progress.title = if next == 0 {
            "GSX lokal eingerichtet · Funktion unbestätigt"
        } else {
            "GSX-Einrichtung"
        };
        progress.detail = match next {
            0 => "Öffne MSFS und prüfe das GSX-Menü und die Bodendienste.",
            1 => "Bereite den FSDT-Installer und .NET in einer Profilkopie vor.",
            2 => "Installiere und aktiviere GSX im FSDT-Installer. Schließe ihn danach.",
            _ if yes(data, "startup_found") => {
                "Übernimm den von FSDT angelegten automatischen Start."
            }
            _ => "Die FSDT-Starteinstellung fehlt. Öffne den Installer für ein Update.",
        };
        progress.steps = steps;
    } else if yes(data, "can_recover") {
        progress.title = "GSX-Einrichtung unterbrochen";
        progress.detail = "Stelle zuerst das bisherige Windows-Profil wieder her.";
    } else if data["state"].is_string() {
        progress.title = "GSX ist für diese Installation nicht verfügbar";
    }
    if current && data["job"]["state"] == "running" {
        progress.ready = false;
        progress.title = if yes(&data["job"], "stopping") {
            "FSDT wird beendet …"
        } else {
            "GSX-Einrichtung läuft …"
        };
        progress.detail = if data["job"]["operation"] == "open" && !yes(&data["job"], "stopping") {
            "Der FSDT-Installer ist geöffnet. Schließe ihn nach dem Download."
        } else {
            "Warte, bis der aktuelle Schritt abgeschlossen ist."
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

#[cfg(test)]
mod addon_tests {
    use super::{fenix, gsx};
    use serde_json::{Value, json};

    #[test]
    fn fenix_local_completion_and_manager_detection_do_not_claim_license_or_aircraft() {
        let mut data = json!({"state":"installed", "installed":true,
            "fenix_installed":true, "manager_installed":true,
            "settings_ready":true, "configured":true});
        let status = json!({"game":{"state":"stopped"}});
        let complete = fenix(&data, &status);
        assert!(complete.ready);
        assert_eq!(complete.title, "Fenix lokal eingerichtet");
        assert!(complete.detail.contains("Lizenz prüft Fenix selbst"));
        assert_eq!(complete.steps, [true, true, true, true]);
        data["fenix_installed"] = json!(false);
        let manager = fenix(&data, &status);
        assert!(!manager.ready);
        assert_eq!(manager.next, 2);
        assert_eq!(manager.steps, [true, false, false, false]);
        assert_eq!(
            manager.title,
            "Fenix-App erkannt · Einrichtung unvollständig"
        );
        assert!(!manager.title.contains("Flugzeug"));
    }

    #[test]
    fn fenix_stale_active_and_legacy_states_are_not_managed_completion() {
        let mut data = json!({"state":"installed", "installed":true,
            "fenix_installed":true, "settings_ready":true, "configured":true});
        data["_error"] = json!("offline");
        let stale = fenix(&data, &Value::Null);
        assert!(!stale.ready);
        assert!(stale.steps.is_empty());
        data["_error"] = Value::Null;
        data["job"] = json!({"state":"running", "operation":"open"});
        assert!(!fenix(&data, &Value::Null).ready);
        data["job"] = Value::Null;
        data["state"] = json!("legacy");
        let legacy = fenix(&data, &Value::Null);
        assert!(!legacy.ready);
        assert!(legacy.steps.is_empty());
        assert_eq!(legacy.title, "Vorhandene Fenix-Einrichtung");
    }

    #[test]
    fn gsx_requires_each_local_prerequisite_and_never_claims_simulator_validation() {
        for flags in 0..16 {
            let prepared = flags & 1 != 0;
            let package = flags & 2 != 0;
            let startup = flags & 4 != 0;
            let configured = flags & 8 != 0;
            let progress = gsx(&json!({"state":"available", "prepared":prepared,
                "package_installed":package, "startup_found":startup, "configured":configured}));
            assert_eq!(
                progress.steps,
                [
                    prepared,
                    prepared && package,
                    prepared && package && startup && configured
                ]
            );
            assert_eq!(progress.ready, flags == 15);
            if progress.ready {
                assert!(progress.title.contains("Funktion unbestätigt"));
            }
        }
    }

    #[test]
    fn gsx_unsupported_stale_or_active_cannot_be_ready() {
        let mut data = json!({"state":"available", "prepared":true,
            "package_installed":true, "startup_found":true, "configured":true});
        data["state"] = json!("unavailable");
        assert!(!gsx(&data).ready);
        assert!(gsx(&data).steps.is_empty());
        data["state"] = json!("available");
        data["_error"] = json!("offline");
        assert!(!gsx(&data).ready);
        assert!(gsx(&data).steps.is_empty());
        data["_error"] = Value::Null;
        data["job"] = json!({"state":"running", "operation":"open"});
        assert!(!gsx(&data).ready);
        data["job"]["stopping"] = json!(true);
        let stopping = gsx(&data);
        assert!(!stopping.ready);
        assert_eq!(stopping.title, "FSDT wird beendet …");

        assert!(!gsx(&Value::Null).ready);
    }
}
