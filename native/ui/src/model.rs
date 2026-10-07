use crate::{App, Edition, client::Request};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_completion_requires_verified_bytes_and_known_files_to_finish() {
        let mut transfer = json!({"received_bytes":9999,"verified_bytes":9999,"total_bytes":10000,"completed_files":1,"total_files":2});
        assert_eq!(transfer_progress(&transfer), Some(99.9));
        transfer["received_bytes"] = json!(10000);
        assert_eq!(transfer_progress(&transfer), Some(99.9));
        transfer["verified_bytes"] = json!(10000);
        assert_eq!(transfer_progress(&transfer), Some(99.9));
        transfer["completed_files"] = json!(2);
        assert_eq!(transfer_progress(&transfer), Some(100.0));
        transfer["received_bytes"] = json!(10001);
        assert_eq!(transfer_progress(&transfer), None);
        transfer["total_bytes"] = Value::Null;
        assert_eq!(transfer_progress(&transfer), None);
    }
}
pub fn yes(v: &Value, key: &str) -> bool {
    v[key] == true
}
pub fn active(v: &Value) -> bool {
    ["running", "checking", "installing", "preparing"].contains(&s(v, "state"))
}
pub fn number(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}
pub fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// Downloaded bytes are not complete until their integrity checks finish.
/// Unknown or inconsistent counters must never produce a completion indicator.
pub fn transfer_progress(transfer: &Value) -> Option<f32> {
    let received = transfer["received_bytes"].as_u64()?;
    let verified = transfer["verified_bytes"].as_u64()?;
    let total = transfer["total_bytes"].as_u64().filter(|n| *n > 0)?;
    if verified > received || received > total {
        return None;
    }
    let completed = transfer["completed_files"].as_u64();
    let files = transfer["total_files"].as_u64();
    if files == Some(0) || completed.zip(files).is_some_and(|(a, b)| a > b) {
        return None;
    }
    let files_complete = completed.zip(files).is_none_or(|(a, b)| a == b);
    Some(
        if received == total && verified == total && files_complete {
            100.0
        } else {
            ((received as f64 / total as f64 * 1000.0).floor() / 10.0).min(99.9) as f32
        },
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Preferences(&'static str),
    Launch,
    Stop,
    Select(Edition),
    Backup,
    Graphics,
    VrSave,
    VrCheck,
    SetupCheck,
    SetupStart,
    SetupControl(&'static str),
    Pick(&'static str),
    Game(&'static str),
    Launcher(&'static str),
    Cloud(&'static str),
    Automatic(&'static str),
    Fenix(&'static str),
    FenixPick(&'static str),
    Gsx(&'static str),
    Proton,
    ProtonDefault,
    ProtonCancel,
    Maintenance(&'static str),
    MaintenanceStart,
    MaintenanceDiscard,
    ModsOpen,
    ModsPreview(String),
    ModsRemove,
    ModsDiscard,
    Store(&'static str),
    Report,
    ReportDiscard,
    Startup,
}
pub struct Forms {
    pub text: BTreeMap<&'static str, String>,
    pub flags: BTreeMap<&'static str, bool>,
}
impl Default for Forms {
    fn default() -> Self {
        Self {
            text: BTreeMap::from([
                ("mode", "install".into()),
                ("game_id", "msfs2024".into()),
                ("graphics", "auto".into()),
                ("vr", "off".into()),
                ("proton", "default".into()),
                ("category", "installation".into()),
            ]),
            flags: BTreeMap::from([("keep_data", true)]),
        }
    }
}
impl Forms {
    pub fn get(&self, key: &str) -> &str {
        self.text.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn flag(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false)
    }
}

impl App {
    pub fn data(&self, key: &str) -> &Value {
        self.snapshot.get(key).unwrap_or(&Value::Null)
    }
    pub fn status(&self) -> &Value {
        self.data("status")
    }
    pub fn runtime(&self) -> &str {
        s(&self.status()["runtime"], "path")
    }
    pub fn fresh(&self, key: &str) -> bool {
        self.online && self.snapshot.contains_key(key) && self.data(key)["_error"].is_null()
    }
    pub fn stopped(&self) -> bool {
        self.status()["game"]["state"] == "stopped"
    }
    pub fn reserved(&self) -> bool {
        yes(&self.status()["setup"], "busy")
            || self.data("setup")["job"]["state"] == "ready"
                && self.data("setup")["job"]["mode"] != "update"
            || active(&self.data("launcher-update")["job"])
                && self.data("launcher-update")["job"]["operation"] != "check"
    }
    pub fn idle(&self) -> bool {
        self.online && !self.pending && self.stopped() && !self.reserved()
    }
    pub fn automatic_busy(&self) -> bool {
        ["syncing", "playing", "attention"].contains(&s(&self.status()["cloud"], "state"))
    }
    pub fn request(&self, action: &Action) -> Option<Request> {
        use Action::*;
        if !self.online || self.pending {
            return None;
        }
        let status = self.status();
        let root = self.runtime();
        let configured = yes(&status["runtime"], "configured");
        let idle = self.idle();
        let safe_idle = idle && !self.automatic_busy();
        let mut body = json!({});
        let mut confirmation = None;
        let (path, allowed) = match action {
            Preferences(language) => {
                body = json!({"language":language});
                ("preferences", ["de", "en"].contains(language))
            }
            Launch => (
                "launch",
                safe_idle
                    && configured
                    && yes(&status["runtime"], "ready")
                    && yes(&status["game"], "can_start"),
            ),
            Stop => (
                "stop",
                yes(&status["game"], "managed") && yes(&status["game"], "can_stop"),
            ),
            Select(edition) => {
                body = json!({"game_id":edition.id()});
                // An inactive cloud review belongs to the current runtime.
                // It must not prevent selecting another installed simulator.
                (
                    "game/select",
                    idle && !["syncing", "playing"].contains(&s(&status["cloud"], "state")),
                )
            }
            Backup => (
                "saves/backup",
                safe_idle && yes(&status["saves"], "can_backup"),
            ),
            Graphics => {
                body = json!({"runtime_path":root,"nvidia_mode":self.forms.get("graphics")});
                (
                    "graphics",
                    idle && configured
                        && yes(&status["graphics"], "available")
                        && !["syncing", "playing"].contains(&s(&status["cloud"], "state")),
                )
            }
            VrSave | VrCheck => {
                body = json!({"runtime_path":root,"mode":self.forms.get("vr")});
                (
                    if *action == VrCheck {
                        "vr/check"
                    } else {
                        "vr/configure"
                    },
                    idle && configured
                        && yes(&status["vr"], "available")
                        && !["syncing", "playing"].contains(&s(&status["cloud"], "state"))
                        && (*action != VrCheck || status["vr"]["mode"] != "off"),
                )
            }
            SetupCheck => {
                let mode = self.forms.get("mode");
                body = json!({"mode":mode,"game_id":self.forms.get("game_id"),"market":self.forms.get("market").to_uppercase(),"local_saves":true});
                for key in [
                    "runtime_path",
                    "destination_path",
                    "artifacts_path",
                    "game_path",
                    "runner_path",
                    "prefix_path",
                    "media_plugins_path",
                ] {
                    if !self.forms.get(key).trim().is_empty() {
                        body[key] = json!(self.forms.get(key).trim());
                    }
                }
                let available = match mode {
                    "existing" => yes(self.data("setup"), "available"),
                    "install" => yes(self.data("setup"), "install_available"),
                    "prepare" => yes(self.data("setup"), "prepare_available"),
                    _ => false,
                };
                ("setup/check", self.fresh("setup") && safe_idle && available)
            }
            SetupStart => {
                let job = &self.data("setup")["job"];
                body = json!({"check_id":job["id"]});
                (
                    "setup/start",
                    self.fresh("setup")
                        && self.stopped()
                        && job["state"] == "ready"
                        && job["mode"] != "update"
                        && !s(job, "id").is_empty()
                        && !self.automatic_busy(),
                )
            }
            SetupControl(op) => {
                let job = &self.data("setup")["job"];
                body = json!({"job_id":job["id"]});
                match *op {
                    "pause" => (
                        "setup/pause",
                        self.fresh("setup")
                            && job["state"] == "installing"
                            && job["phase"] == "download"
                            && yes(job, "can_pause"),
                    ),
                    "resume" => (
                        "setup/resume",
                        self.fresh("setup") && job["phase"] == "paused" && yes(job, "can_resume"),
                    ),
                    "cancel" => (
                        "setup/cancel",
                        self.fresh("setup")
                            && (active(job) || job["state"] == "ready")
                            && !s(job, "id").is_empty(),
                    ),
                    _ => return None,
                }
            }
            Pick(field) => {
                if ![
                    "runtime_path",
                    "destination_path",
                    "artifacts_path",
                    "game_path",
                    "runner_path",
                    "prefix_path",
                    "media_plugins_path",
                    "proton_path",
                ]
                .contains(field)
                {
                    return None;
                }
                body = json!({"field":if *field == "proton_path" { "runner_path" } else { field },"initial":self.forms.get(field)});
                (
                    "setup/pick",
                    safe_idle && self.fresh("setup") && yes(self.data("setup"), "directory_picker"),
                )
            }
            Game(op) => {
                let data = self.data("game-update");
                let job = &data["job"];
                let ready = self.fresh("game-update")
                    && configured
                    && self.stopped()
                    && !self.automatic_busy();
                match *op {
                    "check" => (
                        "game-update/check",
                        ready && idle && yes(data, "can_check") && !yes(data, "auth_required"),
                    ),
                    "sign-in" => {
                        body = json!({"sign_in":true});
                        (
                            if job["operation"] == "repair" {
                                "game-update/repair/check"
                            } else {
                                "game-update/check"
                            },
                            ready && idle && yes(data, "auth_required"),
                        )
                    }
                    "repair" => (
                        "game-update/repair/check",
                        ready && idle && yes(data, "can_repair"),
                    ),
                    "verify" => (
                        "game-update/verify",
                        ready && idle && yes(&data["integrity"], "can_check"),
                    ),
                    "start" => {
                        body = json!({"check_id":job["id"]});
                        (
                            "game-update/start",
                            ready
                                && job["state"] == "ready"
                                && yes(data, "can_start")
                                && !s(job, "id").is_empty(),
                        )
                    }
                    "rollback" => {
                        confirmation = Some(
                            "Zur vorherigen Spielversion zurückkehren? Die aktuelle Version bleibt als Sicherung erhalten.",
                        );
                        (
                            "game-update/rollback",
                            ready && idle && yes(data, "can_rollback"),
                        )
                    }
                    _ => return None,
                }
            }
            Launcher(op) => {
                let data = self.data("launcher-update");
                if !self.fresh("launcher-update") {
                    return None;
                }
                match *op {
                    "check" => ("launcher-update/check", yes(data, "can_check")),
                    "install" => {
                        body = json!({"check_id":data["check_id"]});
                        (
                            "launcher-update/install",
                            safe_idle
                                && yes(data, "can_install")
                                && !s(data, "check_id").is_empty(),
                        )
                    }
                    "cancel" => {
                        body = json!({"job_id":data["job"]["id"]});
                        ("launcher-update/cancel", yes(&data["job"], "can_cancel"))
                    }
                    "restart" => (
                        "launcher-update/restart",
                        safe_idle && yes(data, "can_restart"),
                    ),
                    "rollback" => {
                        confirmation = Some("Zur vorherigen Flightdeck-Version zurückkehren?");
                        (
                            "launcher-update/rollback",
                            safe_idle && yes(data, "can_rollback"),
                        )
                    }
                    _ => return None,
                }
            }
            Fenix(op) | Gsx(op) => {
                let fenix = matches!(action, Fenix(_));
                let data = self.data(if fenix { "fenix" } else { "gsx" });
                let enabled = self.fresh(if fenix { "fenix" } else { "gsx" })
                    && s(data, "runtime_path") == root;
                let change = enabled && safe_idle && yes(data, "can_change");
                body = json!({"runtime_path":root});
                if fenix {
                    match *op {
                        "install" => {
                            body["bundle_path"] = json!(self.forms.get("bundle_path"));
                            (
                                "fenix/install",
                                change
                                    && (data["state"] == "available"
                                        || yes(data, "can_retry")
                                        || yes(data, "update_available")),
                            )
                        }
                        "installer" => {
                            body["installer_path"] = json!(self.forms.get("installer_path"));
                            (
                                "fenix/installer",
                                change
                                    && yes(data, "installed")
                                    && !self.forms.get("installer_path").trim().is_empty(),
                            )
                        }
                        "open" => (
                            "fenix/open",
                            change
                                && yes(data, "fenix_installed")
                                && (yes(data, "installed") || data["state"] == "legacy"),
                        ),
                        "manager" => (
                            "fenix/manager",
                            change
                                && yes(data, "manager_installed")
                                && (yes(data, "installed") || data["state"] == "legacy"),
                        ),
                        "repair" => (
                            "fenix/repair",
                            change
                                && yes(data, "can_repair_installer")
                                && yes(data, "manager_installed")
                                && (yes(data, "installed") || data["state"] == "legacy"),
                        ),
                        "configure" => (
                            "fenix/configure",
                            change
                                && yes(data, "installed")
                                && yes(data, "fenix_installed")
                                && yes(data, "settings_ready"),
                        ),
                        "restore" => {
                            confirmation = Some(
                                "Runner und Windows-Profil auf den Stand vor dem Fenix-Patch zurücksetzen? Das aktuelle Profil bleibt als Sicherung erhalten.",
                            );
                            ("fenix/restore", change && yes(data, "can_restore"))
                        }
                        "stop" => ("fenix/stop", enabled && yes(data, "can_stop")),
                        _ => return None,
                    }
                } else {
                    match *op {
                        "prepare" => ("gsx/prepare", change && data["state"] == "available"),
                        "open" => (
                            "gsx/open",
                            change && data["state"] == "available" && yes(data, "prepared"),
                        ),
                        "configure" => (
                            "gsx/configure",
                            change
                                && yes(data, "prepared")
                                && yes(data, "package_installed")
                                && yes(data, "startup_found"),
                        ),
                        "disable" => ("gsx/disable", change && yes(data, "configured")),
                        "recover" => {
                            confirmation = Some(
                                "Das Windows-Profil vor der GSX-Einrichtung wiederherstellen?",
                            );
                            ("gsx/recover", change && yes(data, "can_recover"))
                        }
                        "stop" => ("gsx/stop", enabled && yes(data, "can_stop")),
                        _ => return None,
                    }
                }
            }
            FenixPick(kind) => {
                body = json!({"kind":kind});
                (
                    "fenix/pick",
                    safe_idle && ["installer", "bundle"].contains(kind),
                )
            }
            Proton | ProtonDefault => {
                let data = self.data("proton");
                let default = *action == ProtonDefault || self.forms.get("proton") == "default";
                let path = if self.forms.get("proton") == "custom" {
                    self.forms.get("proton_path")
                } else {
                    self.forms.get("proton")
                };
                body = json!({"runtime_path":root,"mode":if default {"default"} else {"proton"},"path":if default {""} else {path}});
                (
                    "proton/select",
                    self.fresh("proton")
                        && s(data, "runtime_path") == root
                        && configured
                        && idle
                        && !active(&data["job"])
                        && (if default {
                            yes(data, "can_restore")
                                && !["syncing", "playing"].contains(&s(&status["cloud"], "state"))
                        } else {
                            !self.automatic_busy()
                                && path.starts_with('/')
                                && data["error"].as_str().is_none_or(str::is_empty)
                                && (!yes(data, "fenix")
                                    || self.forms.get("proton") == "custom"
                                    || self
                                        .discoveries
                                        .get("proton/discover")
                                        .and_then(|v| v["choices"].as_array())
                                        .is_some_and(|items| {
                                            items.iter().any(|item| {
                                                s(item, "path") == path && yes(item, "fenix")
                                            })
                                        }))
                        }),
                )
            }
            ProtonCancel => {
                let data = self.data("proton");
                body = json!({"job_id":data["job"]["id"]});
                (
                    "proton/cancel",
                    self.fresh("proton") && s(data, "runtime_path") == root && active(&data["job"]),
                )
            }
            Cloud(op) => {
                let data = self.data("cloud-saves");
                let ready =
                    self.fresh("cloud-saves") && configured && safe_idle && !active(&data["job"]);
                match *op {
                    "check" => ("cloud-saves/check", ready && yes(data, "can_check")),
                    "download" => ("cloud-saves/download", ready && yes(data, "can_download")),
                    "prepare-import" => (
                        "cloud-saves/prepare-import",
                        ready && yes(data, "can_prepare_import"),
                    ),
                    "import" | "upload" => {
                        body = json!({"plan_id":data["plan"]["id"],"choice":if *op=="import" {"cloud"}else{"local"}});
                        confirmation = Some(if *op == "import" {
                            "Den geprüften Cloud-Stand lokal übernehmen? Die betroffenen Spielstände werden vorher gesichert."
                        } else {
                            "Den geprüften lokalen Stand in die Xbox-Cloud übertragen?"
                        });
                        (
                            if *op == "import" {
                                "cloud-saves/import"
                            } else {
                                "cloud-saves/upload"
                            },
                            ready
                                && !s(&data["plan"], "id").is_empty()
                                && yes(
                                    data,
                                    if *op == "import" {
                                        "can_import"
                                    } else {
                                        "can_upload"
                                    },
                                ),
                        )
                    }
                    "restore" => {
                        body = json!({"backup_id":data["restore_id"]});
                        confirmation = Some("Die lokale Sicherung wiederherstellen?");
                        (
                            "cloud-saves/restore",
                            ready && yes(data, "can_restore") && !s(data, "restore_id").is_empty(),
                        )
                    }
                    "discard-plan" => {
                        body = json!({"plan_id":data["plan"]["id"]});
                        (
                            "cloud-saves/discard-plan",
                            ready && !s(&data["plan"], "id").is_empty(),
                        )
                    }
                    "cancel" => {
                        body = json!({"job_id":data["job"]["id"]});
                        (
                            "cloud-saves/cancel",
                            self.fresh("cloud-saves")
                                && active(&data["job"])
                                && yes(data, "can_cancel"),
                        )
                    }
                    _ => return None,
                }
            }
            Automatic(op) => {
                let cloud = &status["cloud"];
                let ready = (idle || (*op == "cancel-auto" && self.stopped()))
                    && configured
                    && yes(cloud, "enabled")
                    && !s(cloud, "request_id").is_empty();
                body = json!({"request_id":cloud["request_id"]});
                match *op {
                    "retry" => (
                        "cloud-saves/retry",
                        ready && cloud["state"] == "attention" && yes(cloud, "can_retry"),
                    ),
                    "sign-in" => (
                        "cloud-saves/sign-in",
                        ready && cloud["state"] == "attention" && yes(cloud, "can_sign_in"),
                    ),
                    "play-local" => (
                        "cloud-saves/play-local",
                        ready
                            && yes(cloud, "can_play_local")
                            && !yes(cloud, "conflict")
                            && cloud["state"] == "attention",
                    ),
                    "cancel-auto" => (
                        "cloud-saves/cancel-auto",
                        ready
                            && yes(cloud, "can_cancel")
                            && ["syncing", "attention"].contains(&s(cloud, "state")),
                    ),
                    "cloud" | "local" => {
                        body["choice"] = json!(op);
                        confirmation = Some(if *op == "cloud" {
                            "Den Cloud-Spielstand verwenden? Eine lokale Sicherung bleibt erhalten."
                        } else {
                            "Den lokalen Spielstand verwenden und in die Cloud übertragen?"
                        });
                        (
                            "cloud-saves/resolve",
                            ready
                                && cloud["state"] == "attention"
                                && yes(cloud, "conflict")
                                && yes(cloud, "can_retry"),
                        )
                    }
                    _ => return None,
                }
            }
            Maintenance(op) => {
                body = json!({"operation":op,"keep_data":self.forms.flag("keep_data"),"delete_packages":self.forms.flag("delete_packages")});
                (
                    "maintenance/preview",
                    self.fresh("maintenance")
                        && idle
                        && configured
                        && !["syncing", "playing"].contains(&s(&status["cloud"], "state"))
                        && !active(&self.data("maintenance")["job"])
                        && ["reset", "restore", "uninstall"].contains(op)
                        && (*op != "restore" || yes(self.data("maintenance"), "can_restore")),
                )
            }
            MaintenanceStart | MaintenanceDiscard => {
                let job = &self.data("maintenance")["job"];
                body = json!({"job_id":job["id"],"confirmed":true});
                (
                    if *action == MaintenanceStart {
                        "maintenance/start"
                    } else {
                        "maintenance/discard"
                    },
                    self.fresh("maintenance")
                        && self.stopped()
                        && job["state"] == "ready"
                        && s(job, "runtime_path") == root
                        && !s(job, "id").is_empty()
                        && (*action == MaintenanceDiscard
                            || (job["keep_data"].as_bool() == Some(self.forms.flag("keep_data"))
                                && job["delete_packages"].as_bool()
                                    == Some(self.forms.flag("delete_packages"))))
                        && (*action == MaintenanceDiscard || (idle && !self.automatic_busy())),
                )
            }
            ModsPreview(id) => {
                let data = self.data("mods");
                body = json!({"runtime_path":root,"addon_id":id});
                (
                    "mods/preview-remove",
                    safe_idle
                        && self.fresh("mods")
                        && s(data, "runtime_path") == root
                        && yes(data, "can_remove")
                        && data["mods"]
                            .as_array()
                            .is_some_and(|items| items.iter().any(|item| s(item, "id") == id)),
                )
            }
            ModsRemove | ModsDiscard => {
                let data = self.data("mods");
                let job = &data["job"];
                body = json!({"runtime_path":root,"job_id":job["id"]});
                let current = self.fresh("mods")
                    && s(data, "runtime_path") == root
                    && s(job, "runtime_path") == root
                    && !s(job, "id").is_empty();
                if *action == ModsRemove {
                    confirmation = Some(if yes(job, "is_link") {
                        "Nur diese Community-Verknüpfung entfernen? Die Originaldateien bleiben erhalten."
                    } else {
                        "Dieses Add-on dauerhaft aus dem Community-Ordner entfernen? Enthaltene Einstellungen werden ebenfalls gelöscht."
                    });
                    (
                        "mods/remove",
                        current
                            && self.stopped()
                            && !self.automatic_busy()
                            && job["state"] == "ready",
                    )
                } else {
                    (
                        "mods/discard-remove",
                        current
                            && (job["state"] == "ready"
                                || (job["state"] == "running" && job["phase"] == "checking")),
                    )
                }
            }
            ModsOpen => (
                "mods/open-folder",
                self.fresh("mods") && !self.reserved() && yes(self.data("mods"), "can_open"),
            ),
            Store(op) => {
                let job = &self.data("store-check")["job"];
                match *op {
                    "start" => (
                        "store-check/start",
                        self.fresh("store-check")
                            && idle
                            && configured
                            && !["syncing", "playing"].contains(&s(&status["cloud"], "state")),
                    ),
                    "sign-in" => {
                        body = json!({"job_id":job["id"]});
                        (
                            "store-check/sign-in",
                            self.fresh("store-check")
                                && idle
                                && configured
                                && !["syncing", "playing"].contains(&s(&status["cloud"], "state")),
                        )
                    }
                    "cancel" => {
                        body = json!({"job_id":job["id"]});
                        (
                            "store-check/cancel",
                            self.fresh("store-check") && active(job),
                        )
                    }
                    _ => return None,
                }
            }
            Report => {
                let observations: Vec<_> = [
                    "menus_visible",
                    "main_view_black",
                    "second_window_works",
                    "second_window_crashes",
                ]
                .into_iter()
                .filter(|key| self.forms.get("category") == "graphics" && self.forms.flag(key))
                .collect();
                body = json!({"runtime_path":if root.is_empty(){Value::Null}else{json!(root)},"category":self.forms.get("category"),"description":self.forms.get("description"),"observations":observations});
                (
                    "problem-reports/prepare",
                    (10..=4000).contains(&self.forms.get("description").trim().chars().count()),
                )
            }
            ReportDiscard => {
                body = json!({"report_id":self.data("problem-reports")["draft"]["report"]["id"]});
                (
                    "problem-reports/discard",
                    self.fresh("problem-reports")
                        && !s(&self.data("problem-reports")["draft"]["report"], "id").is_empty(),
                )
            }
            Startup => (
                "updates/check-startup",
                self.startup_due(std::time::Instant::now()),
            ),
        };
        allowed.then(|| Request {
            path,
            body,
            runtime: root.to_string(),
            confirmation,
        })
    }
}
