use crate::*;
use serde_json::json;
use std::time::Duration;

impl App {
    pub fn poll_interval(&self) -> Duration {
        let busy = !self.online
            || self.pending
            || self.reserved()
            || self.status()["game"]["state"] != "stopped"
            || self.status()["cloud"]["state"] == "syncing"
            || model::active(&self.data("launcher-update")["job"]);
        Duration::from_secs(if busy { 2 } else { 10 })
    }
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(self.poll_interval()).map(|_| Message::Tick),
            iced::event::listen_with(|event, _, _| {
                if matches!(event, iced::Event::Window(iced::window::Event::Focused)) {
                    return Some(Message::Refresh);
                }
                let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) = event
                else {
                    return None;
                };
                match key {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
                        Some(Message::CancelConfirm)
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) => {
                        Some(Message::Focus(modifiers.shift()))
                    }
                    _ => None,
                }
            }),
        ])
    }
    pub(crate) fn refresh(&mut self) -> Task<Message> {
        if self.polling || self.pending || self.restarting {
            return Task::none();
        }
        if self.reconnect
            && let Some(connector) = self.connector.clone()
        {
            self.polling = true;
            let generation = self.generation;
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || connector())
                        .await
                        .map_err(|_| "Local reconnect interrupted".to_string())?
                },
                move |result| Message::Reconnected(generation, result),
            );
        }
        let Some(client) = self.client.clone() else {
            return Task::none();
        };
        self.polling = true;
        let generation = self.generation;
        let language = self.language.code();
        let heavy =
            self.poll_interval() == Duration::from_secs(10) || self.poll_count.is_multiple_of(5);
        self.poll_count = self.poll_count.wrapping_add(1);
        let mut keys = vec!["setup", "launcher-update"];
        match self.page {
            Page::Overview => {}
            Page::Setup => keys.extend(["proton", "maintenance"]),
            Page::Updates => keys.push("game-update"),
            Page::Saves => keys.push("cloud-saves"),
            Page::Mods => {
                keys.extend(["fenix", "gsx"]);
                if heavy || !self.snapshot.contains_key("mods") {
                    keys.push("mods");
                }
            }
            Page::Diagnostics => {
                keys.push("store-check");
                if heavy || !self.snapshot.contains_key("diagnostics") {
                    keys.extend(["diagnostics", "problem-reports"]);
                }
            }
        }
        Task::perform(
            async move { client.poll(language, keys).await },
            move |result| Message::Loaded(generation, result),
        )
    }
    fn submit(&mut self, action: Action, request: client::Request) -> Task<Message> {
        let Some(client) = self.client.clone() else {
            return Task::none();
        };
        self.pending = true;
        self.confirmation = None;
        self.generation += 1;
        self.polling = false;
        let generation = self.generation;
        let language = self.language.code();
        Task::perform(
            async move { client.post(&request, language).await },
            move |result| Message::Completed(generation, action.clone(), result),
        )
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.confirmation.is_some()
            && matches!(
                message,
                Message::Action(_)
                    | Message::Field(..)
                    | Message::EditDescription(_)
                    | Message::Flag(..)
                    | Message::Select(_)
                    | Message::Navigate(_)
                    | Message::Language(_)
            )
        {
            return Task::none();
        }
        match message {
            Message::Reconnected(generation, result) => {
                if generation != self.generation {
                    return Task::none();
                }
                self.polling = false;
                match result {
                    Ok(client) => {
                        self.client = Some(client);
                        self.reconnect = false;
                        self.snapshot.clear();
                        self.confirmation = None;
                        self.connect_after_check = None;
                        self.startup_checked = false;
                        return self.refresh();
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            Message::Focus(previous) => {
                return if previous {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::Tick => return self.refresh(),
            Message::Refresh => {
                self.poll_count = 0;
                return self.refresh();
            }
            Message::Loaded(generation, result) => {
                if generation != self.generation {
                    return Task::none();
                }
                self.polling = false;
                match result {
                    Ok(snapshot) => {
                        let old_root = self.runtime().to_string();
                        let first = !self.snapshot.contains_key("status");
                        let new_root = snapshot
                            .get("status")
                            .map(|s| s["runtime"]["path"].as_str().unwrap_or(""))
                            .unwrap_or("");
                        if old_root != new_root {
                            self.snapshot.clear();
                            self.poll_count = 0;
                        }
                        self.snapshot.extend(snapshot);
                        self.online = true;
                        if old_root != self.runtime() || first {
                            self.confirmation = None;
                            self.discoveries.clear();
                            if yes(&self.status()["runtime"], "configured") {
                                self.edition = if self.status()["runtime"]["game_id"] == "msfs2020"
                                {
                                    Edition::Msfs2020
                                } else {
                                    Edition::Msfs2024
                                };
                            }
                            self.forms.text.insert("game_id", self.edition.id().into());
                            for (field, value) in [
                                ("runtime_path", self.runtime().to_string()),
                                (
                                    "graphics",
                                    s(&self.status()["graphics"], "nvidia_mode").to_string(),
                                ),
                                ("vr", s(&self.status()["vr"], "mode").to_string()),
                            ] {
                                self.forms.text.insert(field, value);
                            }
                            for field in [
                                "destination_path",
                                "artifacts_path",
                                "game_path",
                                "runner_path",
                                "prefix_path",
                                "media_plugins_path",
                                "market",
                            ] {
                                if let Some(value) = self.data("setup")["defaults"][field]
                                    .as_str()
                                    .filter(|v| !v.is_empty())
                                    .map(str::to_string)
                                {
                                    self.forms.text.entry(field).or_insert(value);
                                }
                            }
                            if first && yes(&self.status()["runtime"], "configured") {
                                self.forms.text.insert("mode", "existing".into());
                            }
                        }
                        if let Some(id) = &self.connect_after_check {
                            let job = &self.data("setup")["job"];
                            if s(job, "id") != id
                                || ["failed", "cancelled", "complete"].contains(&s(job, "state"))
                            {
                                self.connect_after_check = None;
                            } else if job["state"] == "ready" && job["mode"] == "existing" {
                                self.connect_after_check = None;
                                return self.update(Message::Action(Action::SetupStart));
                            }
                        }
                        if !self.startup_checked
                            && let Some(request) = self.request(&Action::Startup)
                        {
                            self.startup_checked = true;
                            return self.submit(Action::Startup, request);
                        }
                    }
                    Err(error) => {
                        self.online = false;
                        self.reconnect = true;
                        self.confirmation = None;
                        self.notice = Some(error);
                    }
                }
            }
            Message::Action(action) => {
                if let Some(request) = self.request(&action) {
                    if request.confirmation.is_some() {
                        self.confirmation = Some((action, request));
                    } else {
                        return self.submit(action, request);
                    }
                }
            }
            Message::Confirm => {
                if let Some((action, reviewed)) = self.confirmation.take() {
                    if self.request(&action).as_ref() == Some(&reviewed) {
                        return self.submit(action, reviewed);
                    }
                    self.notice = Some(
                        self.tr(
                            "Der Status hat sich geändert. Bitte die Aktion erneut prüfen.",
                            "The state changed. Please review the action again.",
                        )
                        .into(),
                    );
                }
            }
            Message::CancelConfirm => self.confirmation = None,
            Message::Completed(generation, action, result) => {
                if generation != self.generation {
                    return Task::none();
                }
                self.pending = false;
                self.poll_count = 0;
                match result {
                    Ok(value) => {
                        self.notice = value["message"]
                            .as_str()
                            .filter(|v| !v.is_empty())
                            .map(str::to_string);
                        match action {
                            Action::Pick(field) if value["cancelled"] != true => {
                                if let Some(path) =
                                    value["path"].as_str().filter(|v| v.starts_with('/'))
                                {
                                    let path = if field == "destination_path" {
                                        format!(
                                            "{}/{}",
                                            path.trim_end_matches('/'),
                                            self.forms.get("game_id")
                                        )
                                    } else {
                                        path.to_string()
                                    };
                                    self.forms.text.insert(field, path);
                                }
                            }
                            Action::FenixPick(kind) if value["cancelled"] != true => {
                                if let Some(path) =
                                    value["path"].as_str().filter(|v| v.starts_with('/'))
                                {
                                    self.forms.text.insert(
                                        if kind == "installer" {
                                            "installer_path"
                                        } else {
                                            "bundle_path"
                                        },
                                        path.to_string(),
                                    );
                                }
                            }
                            Action::SetupCheck if self.forms.get("mode") == "existing" => {
                                self.connect_after_check =
                                    value["job"]["id"].as_str().map(str::to_string);
                            }
                            Action::Launcher("restart") => {
                                self.restarting = true;
                                return iced::exit();
                            }
                            Action::Report => {
                                self.report_dirty = false;
                                self.snapshot.insert("problem-reports", value);
                            }
                            Action::ReportDiscard => {
                                self.report_dirty = false;
                                self.forms.text.insert("description", String::new());
                                self.description = iced::widget::text_editor::Content::new();
                            }
                            _ => {}
                        }
                    }
                    Err(error) => self.notice = Some(error),
                }
                // A POST result is not a new status. Disable further actions
                // until the complete post-mutation snapshot has arrived.
                self.online = false;
                return self.refresh();
            }
            Message::Select(edition) => {
                if self.request(&Action::Select(edition)).is_some() {
                    if yes(&self.status()["versions"][edition.id()], "ready") {
                        return self.update(Message::Action(Action::Select(edition)));
                    }
                    self.forms.text.insert("game_id", edition.id().into());
                    self.page = Page::Setup;
                    self.forms.text.insert("mode", "install".into());
                    self.forms.text.insert("destination_path", String::new());
                }
            }
            Message::Language(language) => {
                if self.pending {
                    return Task::none();
                }
                self.language = language;
                self.confirmation = None;
                self.notice = None;
                if let Some(request) = self.request(&Action::Preferences(language.code())) {
                    return self.submit(Action::Preferences(language.code()), request);
                }
                self.generation += 1;
                self.polling = false;
                self.online = false;
                return self.refresh();
            }
            Message::Navigate(page) => {
                self.page = page;
                self.confirmation = None;
                self.poll_count = 0;
                if !self.pending {
                    self.generation += 1;
                    self.polling = false;
                    self.online = false;
                }
                return self.refresh();
            }
            Message::EditDescription(action) => {
                if self.pending || self.confirmation.is_some() {
                    return Task::none();
                }
                self.description.perform(action);
                let value = self.description.text();
                let limited: String = value.chars().take(4000).collect();
                if limited != value {
                    self.description = iced::widget::text_editor::Content::with_text(&limited);
                }
                if limited != self.forms.get("description") {
                    self.report_dirty = true;
                    self.forms.text.insert("description", limited);
                }
            }
            Message::Field(field, value) => {
                if !self.can_edit_field(field) {
                    return Task::none();
                }
                self.confirmation = None;
                if ["description", "category"].contains(&field) {
                    self.report_dirty = true;
                }
                if field == "game_id"
                    && self.forms.get(field) != value
                    && self.forms.get("mode") == "install"
                {
                    self.forms.text.insert("destination_path", String::new());
                }
                self.forms.text.insert(
                    field,
                    value
                        .chars()
                        .take(if field == "description" { 4000 } else { 4096 })
                        .collect(),
                );
            }
            Message::Flag(field, value) => {
                if self.pending {
                    return Task::none();
                }
                self.confirmation = None;
                if ["keep_data", "delete_packages"].contains(&field)
                    && self.data("maintenance")["job"]["state"] == "ready"
                {
                    // Invalidate a reviewed destructive plan before any options change.
                    let task = self.update(Message::Action(Action::MaintenanceDiscard));
                    self.forms.flags.insert(field, value);
                    if !self.forms.flag("delete_packages") {
                        self.forms.flags.insert("keep_data", true);
                    }
                    return task;
                }
                if !["keep_data", "delete_packages"].contains(&field) {
                    self.report_dirty = true;
                }
                self.forms.flags.insert(field, value);
                if !self.forms.flag("delete_packages") {
                    self.forms.flags.insert("keep_data", true);
                }
            }
            Message::Discover(path) => {
                if !["setup/discover", "proton/discover"].contains(&path)
                    || self.pending
                    || !self.online
                {
                    return Task::none();
                }
                if let Some(client) = self.client.clone() {
                    self.pending = true;
                    self.generation += 1;
                    self.polling = false;
                    let generation = self.generation;
                    let language = self.language.code();
                    return Task::perform(
                        async move { client.discover(path, language).await },
                        move |result| Message::Discovered(generation, path, result),
                    );
                }
            }
            Message::Discovered(generation, path, result) => {
                if generation != self.generation {
                    return Task::none();
                }
                self.pending = false;
                match result {
                    Ok(value) => {
                        self.discoveries.insert(path, value);
                    }
                    Err(error) => self.notice = Some(error),
                }
                return self.refresh();
            }
            Message::MailReport => {
                if self.exporting {
                    return Task::none();
                }
                if let Some(uri) = self.mail_uri() {
                    self.exporting = true;
                    self.notice=Some(self.tr("Der E-Mail-Entwurf wird geöffnet. Prüfe ihn im Mailprogramm vor dem Senden.","Opening an email draft. Review it in your mail application before sending.").into());
                    return Task::perform(open_draft(uri), Message::Exported);
                }
            }
            Message::CopyReport => {
                if let Some(report) = self.report_text() {
                    return iced::clipboard::write(report);
                }
            }
            Message::CopyDiagnostics => {
                if let Some(report) = self.diagnostics_text() {
                    return iced::clipboard::write(report);
                }
            }
            Message::SaveReport | Message::SaveDiagnostics => {
                if self.exporting {
                    return Task::none();
                }
                let report = if matches!(message, Message::SaveReport) {
                    self.report_text()
                } else {
                    self.diagnostics_text()
                };
                if let Some(report) = report {
                    self.exporting = true;
                    let filename = if matches!(message, Message::SaveReport) {
                        "flightdeck-report.json"
                    } else {
                        "flightdeck-diagnose.json"
                    };
                    return Task::perform(export(report, filename), Message::Exported);
                }
            }
            Message::Exported(result) => {
                self.exporting = false;
                match result {
                    Ok(Some(path)) => {
                        self.notice = Some(format!("{}: {path}", self.tr("Gespeichert", "Saved")))
                    }
                    Ok(None) => {}
                    Err(error) => self.notice = Some(error),
                }
            }
            Message::Dismiss => self.notice = None,
        }
        Task::none()
    }
    pub fn can_edit_field(&self, field: &str) -> bool {
        if self.pending {
            return false;
        }
        if [
            "mode",
            "game_id",
            "runtime_path",
            "destination_path",
            "artifacts_path",
            "game_path",
            "runner_path",
            "prefix_path",
            "media_plugins_path",
            "market",
        ]
        .contains(&field)
        {
            let job = &self.data("setup")["job"];
            return !self.reserved()
                && !(["checking", "installing", "ready"].contains(&s(job, "state"))
                    && job["mode"] != "update");
        }
        true
    }
    pub fn launch_label(&self) -> &str {
        if yes(&self.status()["game"], "can_stop") {
            self.tr("Simulator beenden", "Stop simulator")
        } else if !yes(&self.status()["runtime"], "ready") {
            self.tr("Simulator einrichten", "Set up simulator")
        } else {
            self.tr("Simulator starten", "Start simulator")
        }
    }
    pub fn launch_message(&self) -> Option<Message> {
        if self.request(&Action::Stop).is_some() {
            Some(Message::Action(Action::Stop))
        } else if self.request(&Action::Launch).is_some() {
            Some(Message::Action(Action::Launch))
        } else if self.online && !yes(&self.status()["runtime"], "ready") {
            Some(Message::Navigate(Page::Setup))
        } else {
            None
        }
    }
    pub fn launch_note(&self) -> &str {
        if !self.online {
            self.tr(
                "Warte auf den aktuellen Installationsstatus.",
                "Waiting for the current installation status.",
            )
        } else if !yes(&self.status()["runtime"], "configured") {
            self.tr(
                "Richte deine Installation vor dem ersten Start ein.",
                "Set up your installation before the first launch.",
            )
        } else if yes(&self.status()["cloud"], "enabled") {
            self.tr(
                "Deine Spielstände werden vor dem Start automatisch abgeglichen.",
                "Your saves are synced automatically before starting.",
            )
        } else {
            self.tr(
                "Der automatische Cloud-Abgleich ist für diese Installation nicht aktiv.",
                "Automatic cloud sync is not active for this installation.",
            )
        }
    }
    pub fn launch_state(&self) -> &str {
        if !self.online {
            return self.tr("Verbindung wird hergestellt …", "Connecting …");
        }
        if self.automatic_busy() {
            return self.tr(
                "Cloud-Abgleich · Status unten beachten",
                "Cloud sync · check status below",
            );
        }
        match s(&self.status()["game"], "state") {
            "starting" => self.tr("Simulator wird gestartet …", "Starting simulator …"),
            "running" => self.tr("Simulator läuft", "Simulator is running"),
            "stopping" => self.tr("Simulator wird beendet …", "Stopping simulator …"),
            "external" => self.tr(
                "Installation wird außerhalb von Flightdeck verwendet",
                "Installation is in use outside Flightdeck",
            ),
            "stopped" if yes(&self.status()["runtime"], "ready") => {
                self.tr("Bereit zum Start", "Ready to start")
            }
            "stopped" => self.tr("Einrichtung erforderlich", "Setup required"),
            _ => self.tr("Status unbekannt", "Unknown state"),
        }
    }
    pub fn report_text(&self) -> Option<String> {
        let report = &self.data("problem-reports")["draft"]["report"];
        (!self.report_dirty && self.fresh("problem-reports") && report.is_object())
            .then(|| serde_json::to_string_pretty(report).ok())
            .flatten()
    }
    pub fn mail_uri(&self) -> Option<String> {
        let report = self.report_text()?;
        let recipient = s(self.data("problem-reports"), "recipient");
        let (local, domain) = recipient.split_once('@')?;
        if local.is_empty()
            || domain.is_empty()
            || !domain.contains('.')
            || recipient.len() > 254
            || !recipient
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
        {
            return None;
        }
        let encode = |value: &str| {
            value
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        };
        let uri = format!(
            "mailto:{recipient}?subject=Flightdeck%20report&body={}",
            encode(&report.replace('\n', "\r\n"))
        );
        (uri.len() <= 24000).then_some(uri)
    }
    pub fn diagnostics_text(&self) -> Option<String> {
        self.fresh("diagnostics").then(||{let data=self.data("diagnostics");serde_json::to_string_pretty(&json!({"summary":data["summary"],"checks":data["checks"],"generated_at":data["generated_at"]})).ok()}).flatten()
    }
}

async fn export(contents: String, filename: &'static str) -> Result<Option<String>, String> {
    use std::process::Stdio;
    let available = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(name).is_file()))
    };
    let mut command = if available("zenity") {
        let mut c = tokio::process::Command::new("zenity");
        c.args([
            "--file-selection",
            "--save",
            "--title=Flightdeck",
            "--filename",
            filename,
        ]);
        c
    } else if available("kdialog") {
        let mut c = tokio::process::Command::new("kdialog");
        c.args(["--getsavefilename", filename]);
        c
    } else {
        return Err("Kein Dateidialog verfügbar. Bitte zenity oder kdialog installieren; der Bericht kann auch kopiert werden.".into());
    };
    let output = tokio::time::timeout(
        Duration::from_secs(130),
        command
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "Dateiauswahl abgebrochen: Zeitlimit erreicht".to_string())?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Ok(None);
    }
    if output.stdout.len() > 8192 {
        return Err("Ungültiger Speicherpfad".into());
    }
    let path = String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();
    if !std::path::Path::new(&path).is_absolute() || path.contains(['\n', '\r']) {
        return Err("Ungültiger Speicherpfad".into());
    }
    // create_new rejects existing files and symlinks; export never overwrites.
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .await
        .map_err(|e| format!("{e}. Bitte einen neuen Dateinamen auswählen."))?;
    file.write_all(contents.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    file.sync_all().await.map_err(|e| e.to_string())?;
    Ok(Some(path))
}
async fn open_draft(uri: String) -> Result<Option<String>, String> {
    use std::process::Stdio;
    let status = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("xdg-open")
            .arg(uri)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await
    .map_err(|_| "Das Mailprogramm antwortet nicht. Bitte den Bericht kopieren.".to_string())?
    .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(None)
    } else {
        Err("Das Mailprogramm konnte nicht geöffnet werden. Bitte den Bericht kopieren.".into())
    }
}
