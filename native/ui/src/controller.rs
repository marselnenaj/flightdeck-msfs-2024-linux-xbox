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
            Page::Overview => keys.push("game-update"),
            Page::Setup => {
                keys.extend(["proton", "maintenance"]);
                for key in ["proton/discover", "setup/discover"] {
                    if !self.discoveries.contains_key(key) {
                        keys.push(key);
                    }
                }
            }
            Page::Updates => keys.push("game-update"),
            Page::Saves => keys.push("cloud-saves"),
            Page::Mods => {
                keys.extend(["fenix", "gsx"]);
                if heavy
                    || !self.snapshot.contains_key("mods")
                    || model::active(&self.data("mods")["job"])
                    || self.data("mods")["job"]["state"] == "ready"
                {
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
        let (task, handle) = Task::perform(
            async move { client.poll(language, keys).await },
            move |result| Message::Loaded(generation, result),
        )
        .abortable();
        self.poll_task = Some(handle.abort_on_drop());
        task
    }
    fn submit(&mut self, action: Action, request: client::Request) -> Task<Message> {
        let Some(client) = self.client.clone() else {
            return Task::none();
        };
        self.pending = true;
        self.pending_action = Some(action.clone());
        self.confirmation = None;
        self.generation += 1;
        self.polling = false;
        self.poll_task = None;
        let generation = self.generation;
        let language = self.language.code();
        Task::perform(
            async move { client.post(&request, language).await },
            move |result| Message::Completed(generation, action.clone(), result),
        )
    }
    fn reset_startup_check(&mut self) {
        self.startup_checked = false;
        self.startup_attempt = None;
        self.startup_inflight = None;
        self.startup_retry = false;
    }
    pub(crate) fn startup_due(&self, now: std::time::Instant) -> bool {
        !self.restarting
            && self.startup_inflight.is_none()
            && (!self.startup_checked
                || self.startup_attempt.is_some_and(|attempt| {
                    now.saturating_duration_since(attempt)
                        >= Duration::from_secs(if self.startup_retry { 30 } else { 300 })
                }))
    }
    fn check_startup(&mut self) -> Task<Message> {
        let Some(request) = self.request(&Action::Startup) else {
            return Task::none();
        };
        let Some(client) = self.client.clone() else {
            return Task::none();
        };
        self.startup_sequence = self.startup_sequence.wrapping_add(1);
        let sequence = self.startup_sequence;
        self.startup_inflight = Some(sequence);
        self.startup_checked = true;
        self.startup_attempt = Some(std::time::Instant::now());
        let runtime = request.runtime.clone();
        let language = self.language.code();
        // Version discovery has its own completion token. It neither reserves
        // the UI nor invalidates a poll/action when the user changes pages.
        // The service throttles network checks (30 min success / 5 min failure).
        Task::perform(
            async move { client.post(&request, language).await },
            move |result| Message::StartupCompleted(sequence, runtime.clone(), result),
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
                    | Message::Toggle(_)
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
                        self.reset_startup_check();
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
                if self.page == Page::Setup && !self.pending {
                    self.discoveries.clear();
                }
                return self.refresh();
            }
            Message::Loaded(generation, result) => {
                if generation != self.generation {
                    return Task::none();
                }
                self.polling = false;
                self.poll_task = None;
                self.pending_action = None;
                match result {
                    Ok(mut snapshot) => {
                        let old_root = self.runtime().to_string();
                        let first = !self.snapshot.contains_key("status");
                        let new_root = snapshot
                            .get("status")
                            .map(|s| s["runtime"]["path"].as_str().unwrap_or(""))
                            .unwrap_or("");
                        if old_root != new_root {
                            self.snapshot.clear();
                            self.poll_count = 0;
                            self.reset_startup_check();
                        }
                        let discovered = ["proton/discover", "setup/discover"]
                            .map(|key| (key, snapshot.remove(key)));
                        self.snapshot.extend(snapshot);
                        self.online = true;
                        if old_root != self.runtime() || first {
                            self.confirmation = None;
                            self.discoveries.clear();
                            self.proton_active = None;
                            self.forms.text.insert("proton", String::new());
                            self.forms.text.remove("proton_path");
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
                        for (key, value) in discovered {
                            if let Some(value) = value {
                                self.discoveries.insert(key, value);
                            }
                        }
                        self.sync_proton_selection();
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
                        let startup = self.check_startup();
                        if self.page == Page::Setup
                            && !self.discoveries.contains_key("proton/discover")
                        {
                            return Task::batch([startup, self.refresh()]);
                        }
                        return startup;
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
                if action == Action::Startup {
                    return self.check_startup();
                }
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
            Message::StartupCompleted(sequence, runtime, result) => {
                if self.startup_inflight != Some(sequence) || self.runtime() != runtime {
                    return Task::none();
                }
                self.startup_inflight = None;
                self.startup_retry = result
                    .as_ref()
                    .map_or(true, |value| value["deferred"] == true);
                return self.refresh();
            }
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
                    Err(error) => {
                        self.pending_action = None;
                        self.notice = Some(error);
                    }
                }
                // A POST result is not a new status. Disable further actions
                // until the complete post-mutation snapshot has arrived.
                self.online = false;
                return self.refresh();
            }
            Message::Select(edition) => {
                if self.request(&Action::Select(edition)).is_some() {
                    if yes(&self.status()["versions"][edition.id()], "installed")
                        || yes(&self.status()["versions"][edition.id()], "ready")
                    {
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
                    self.poll_task = None;
                }
                return self.refresh();
            }
            Message::Toggle(section) => {
                if !self.expanded.remove(&section) {
                    if let Some(group) = section.group() {
                        self.expanded.retain(|item| item.group() != Some(group));
                    }
                    self.expanded.insert(section);
                }
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
                if field == "proton"
                    && yes(self.data("proton"), "fenix")
                    && self
                        .discoveries
                        .get("proton/discover")
                        .and_then(|v| v["choices"].as_array())
                        .is_some_and(|items| {
                            items
                                .iter()
                                .any(|item| s(item, "path") == value && !yes(item, "fenix"))
                        })
                {
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
                    || (path == "proton/discover" && !self.can_edit_field("proton"))
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
                        self.sync_proton_selection();
                    }
                    Err(error) => {
                        self.discoveries.insert(path, json!({"_error":error}));
                        self.notice = Some(error);
                    }
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
            Message::OpenHelp(link) => {
                if !self.exporting {
                    self.exporting = true;
                    return Task::perform(open_help(link), Message::Exported);
                }
            }
            Message::OpenReleaseLink(url) => {
                if !self.exporting && release_notes::safe_url(&url).is_some() {
                    self.exporting = true;
                    return Task::perform(open_url(url), Message::Exported);
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
                    Err(error) => self.notice = Some(self.t(&error).to_string()),
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
        if ["proton", "proton_path"].contains(&field) {
            return self.idle()
                && self.fresh("proton")
                && s(self.data("proton"), "runtime_path") == self.runtime()
                && !model::active(&self.data("proton")["job"])
                && !["syncing", "playing"].contains(&s(&self.status()["cloud"], "state"));
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
    fn sync_proton_selection(&mut self) {
        let data = self.data("proton");
        if !self.fresh("proton") || s(data, "runtime_path") != self.runtime() {
            return;
        }
        let active = (
            yes(data, "experimental"),
            s(data, "selected").to_string(),
            s(data, "selected_path").to_string(),
        );
        let choices = &self
            .discoveries
            .get("proton/discover")
            .unwrap_or(&Value::Null)["choices"];
        let valid = ["default", "custom"].contains(&self.forms.get("proton"))
            || choices
                .as_array()
                .into_iter()
                .flatten()
                .any(|item| s(item, "path") == self.forms.get("proton"));
        if self.proton_active.as_ref() != Some(&active) || !valid {
            let selection = if active.0 {
                choices
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|item| {
                        if active.2.is_empty() {
                            s(item, "version") == active.1
                        } else {
                            s(item, "path") == active.2
                        }
                    })
                    .map(|item| s(item, "path"))
                    .unwrap_or("")
            } else {
                "default"
            };
            self.forms.text.insert("proton", selection.to_string());
        }
        self.proton_active = Some(active);
    }
    pub fn launch_label(&self) -> &str {
        if self.pending_action == Some(Action::Launch) {
            self.tr("Start wird vorbereitet …", "Preparing launch …")
        } else if self.pending_action == Some(Action::Stop)
            || self.status()["game"]["state"] == "stopping"
        {
            self.tr("Simulator wird beendet …", "Stopping simulator …")
        } else if self.status()["cloud"]["state"] == "syncing" {
            if self.status()["cloud"]["phase"] == "recovery" {
                self.tr("Sitzung wird geprüft …", "Checking session …")
            } else if self.status()["cloud"]["phase"] == "after_exit" {
                self.tr("Spielstände sichern …", "Saving progress …")
            } else {
                self.tr("Spielstände abgleichen …", "Syncing saves …")
            }
        } else if yes(&self.status()["game"], "can_stop") {
            self.tr("Simulator beenden", "Stop simulator")
        } else if self.status()["cloud"]["state"] == "attention" {
            self.tr("Start blockiert", "Launch blocked")
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
    pub(crate) fn session_failed(&self) -> bool {
        self.online
            && !self.pending
            && self.status()["game"]["state"] == "stopped"
            && self.status()["cloud"]["state"] != "syncing"
            && self.status()["game"]["exit_code"]
                .as_i64()
                .is_some_and(|code| ![0, 130, 143].contains(&code))
    }
    pub fn launch_note(&self) -> &str {
        if self.pending_action == Some(Action::Launch) {
            self.tr(
                "Dein Start wurde angefordert. Flightdeck bereitet den Simulator vor.",
                "Launch requested. Flightdeck is preparing the simulator.",
            )
        } else if self.pending_action == Some(Action::Stop)
            || self.status()["game"]["state"] == "stopping"
        {
            self.tr(
                "Flightdeck wartet, bis die Prozesse des Simulators geschlossen sind.",
                "Flightdeck is waiting for the simulator's processes to close.",
            )
        } else if ["syncing", "attention"].contains(&s(&self.status()["cloud"], "state"))
            && !s(&self.status()["cloud"], "message").is_empty()
        {
            s(&self.status()["cloud"], "message")
        } else if !self.online {
            self.tr(
                "Warte auf den aktuellen Installationsstatus.",
                "Waiting for the current installation status.",
            )
        } else if !yes(&self.status()["runtime"], "configured") {
            self.tr(
                "Richte deine Installation vor dem ersten Start ein.",
                "Set up your installation before the first launch.",
            )
        } else if self.session_failed() {
            self.tr(
                "Die letzte Simulator-Sitzung ist fehlgeschlagen. Details findest du unter Diagnose; du kannst erneut starten.",
                "The last simulator session failed. Check Diagnostics for details; you can try starting again.",
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
        if self.pending_action == Some(Action::Launch) {
            return self.tr("Start wird vorbereitet …", "Preparing launch …");
        }
        if self.pending_action == Some(Action::Stop) {
            return self.tr("Simulator wird beendet …", "Stopping simulator …");
        }
        if !self.online {
            return self.tr("Verbindung wird hergestellt …", "Connecting …");
        }
        if self.status()["cloud"]["state"] == "attention" {
            return self.tr(
                "Start blockiert · Hinweis beachten",
                "Launch blocked · check the message",
            );
        }
        if self.status()["cloud"]["state"] == "syncing" {
            if self.status()["cloud"]["phase"] == "recovery" {
                return self.tr("Sitzung wird geprüft …", "Checking session …");
            }
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
            "stopped" if self.session_failed() => self.tr(
                "Simulator-Sitzung fehlgeschlagen",
                "Simulator session failed",
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
    use crate::platform::{DIALOG_FAILED, DIALOG_TIMEOUT, EXPORT_FAILED, selected_path};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;
    let available = |name: &str| {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|p| {
                p.join(name)
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
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
    .map_err(|_| DIALOG_TIMEOUT.to_string())?
    .map_err(|_| DIALOG_FAILED.to_string())?;
    let Some(path) = selected_path(output.status.code(), &output.stdout).map_err(str::to_owned)?
    else {
        return Ok(None);
    };
    // create_new rejects existing files and symlinks; export never overwrites.
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .await
        .map_err(|_| EXPORT_FAILED.to_string())?;
    file.write_all(contents.as_bytes())
        .await
        .map_err(|_| EXPORT_FAILED.to_string())?;
    file.sync_all()
        .await
        .map_err(|_| EXPORT_FAILED.to_string())?;
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

async fn open_help(link: HelpLink) -> Result<Option<String>, String> {
    open_url(link.url().to_string()).await
}

async fn open_url(url: String) -> Result<Option<String>, String> {
    use std::process::Stdio;
    let status = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("xdg-open")
            .arg(url)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await
    .map_err(|_| "Browser did not respond".to_string())?
    .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(None)
    } else {
        Err("Could not open the browser".into())
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    fn snapshot(root: &str) -> Snapshot {
        Snapshot::from([
            (
                "status",
                json!({
                    "runtime":{"path":root,"configured":true,"ready":true,"game_id":"msfs2024"},
                    "game":{"state":"stopped","can_start":true},
                    "setup":{"busy":false},"cloud":{"state":"idle"}
                }),
            ),
            ("setup", json!({})),
        ])
    }

    fn app() -> App {
        let mut app = App::new(Edition::Msfs2024);
        // Tasks remain unpolled: these tests exercise message ordering without
        // opening a socket, checking GitHub or launching a simulator.
        app.client = Some(Client::new(9, "a".repeat(32)).expect("synthetic client"));
        let _ = app.update(Message::Loaded(0, Ok(snapshot("/synthetic/2024"))));
        app
    }

    #[test]
    fn background_check_does_not_block_actions_or_lose_completion_after_generation_change() {
        let mut app = app();
        assert!(!app.pending);
        assert_eq!(app.startup_inflight, Some(1));
        assert!(app.request(&Action::Launch).is_some());
        let _ = app.update(Message::Loaded(0, Ok(snapshot("/synthetic/2024"))));
        assert_eq!(
            app.startup_sequence, 1,
            "polls must not duplicate an active check"
        );
        let _ = app.update(Message::Action(Action::Select(Edition::Msfs2020)));
        assert!(app.pending);
        assert_eq!(app.generation, 1);
        let _ = app.update(Message::StartupCompleted(
            1,
            "/synthetic/2024".into(),
            Ok(json!({"ok":true})),
        ));
        assert_eq!(app.startup_inflight, None);
        assert!(
            app.pending,
            "background completion must not complete the user's action"
        );
        assert_eq!(app.generation, 1);
    }

    #[test]
    fn switched_runtime_ignores_old_completion_without_clearing_its_new_check() {
        let mut app = app();
        let _ = app.update(Message::Loaded(0, Ok(snapshot("/synthetic/2020"))));
        assert_eq!(app.startup_inflight, Some(2));
        let _ = app.update(Message::StartupCompleted(
            1,
            "/synthetic/2024".into(),
            Err("old runtime".into()),
        ));
        assert_eq!(app.startup_inflight, Some(2));
        assert!(!app.startup_retry);
        let _ = app.update(Message::StartupCompleted(
            2,
            "/synthetic/2020".into(),
            Ok(json!({"ok":true})),
        ));
        assert_eq!(app.startup_inflight, None);
        assert_eq!(app.runtime(), "/synthetic/2020");
    }

    #[test]
    fn deferred_and_unreachable_checks_retry_with_a_bound_and_successes_periodically() {
        for result in [
            Ok(json!({"ok":true,"deferred":true})),
            Err("offline".into()),
            Ok(json!({"ok":true})),
        ] {
            let mut app = app();
            let retry = result
                .as_ref()
                .map_or(true, |value| value["deferred"] == true);
            let attempt = app.startup_attempt.expect("attempt recorded");
            let _ = app.update(Message::StartupCompleted(
                1,
                "/synthetic/2024".into(),
                result,
            ));
            let seconds = if retry { 30 } else { 300 };
            assert!(!app.startup_due(attempt + Duration::from_secs(seconds - 1)));
            assert!(app.startup_due(attempt + Duration::from_secs(seconds)));
            app.startup_attempt = Some(std::time::Instant::now() - Duration::from_secs(seconds));
            app.online = false;
            assert!(
                app.request(&Action::Startup).is_none(),
                "offline polling must not queue checks"
            );
            app.online = true;
            let _ = app.update(Message::Action(Action::Startup));
            assert_eq!(app.startup_inflight, Some(2));
            assert!(!app.pending);
        }
    }
}
