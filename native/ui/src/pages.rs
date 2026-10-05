use crate::*;
use iced::widget::{checkbox, column, progress_bar, text_editor, text_input};
use model::{Action::*, active, bytes, number};
use std::collections::BTreeMap;

static TRANSLATIONS: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../catalog.json")).expect("compiled UI catalog")
});

#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice {
    id: String,
    label: String,
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    fn t<'a>(&self, text: &'a str) -> &'a str {
        if self.language == Language::En {
            TRANSLATIONS.get(text).map(String::as_str).unwrap_or(text)
        } else {
            text
        }
    }
    fn paragraph<'a>(&self, text: impl Into<std::borrow::Cow<'a, str>>) -> Element<'a, Message> {
        label(text, 14.0, Weight::Normal, MUTED)
            .line_height(1.6)
            .into()
    }
    fn heading<'a>(&self, text: impl Into<std::borrow::Cow<'a, str>>) -> Element<'a, Message> {
        label(text, 25.0, Weight::Semibold, INK)
            .tracking(-0.7)
            .into()
    }
    fn card<'a>(
        &self,
        title: &str,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        container(column![self.heading(self.t(title).to_string()), content.into()].spacing(18))
            .padding(26)
            .width(Length::Fill)
            .style(card_style)
            .into()
    }
    fn action<'a>(&self, title: &str, action: Action) -> Element<'a, Message> {
        let allowed = self.request(&action).is_some();
        button(label(
            self.t(title).to_string(),
            14.0,
            Weight::Semibold,
            INK,
        ))
        .padding([11, 16])
        .on_press_maybe(allowed.then_some(Message::Action(action)))
        .into()
    }
    fn actions<'a>(&self, items: &[(&str, Action)]) -> Element<'a, Message> {
        // Two controls per row keeps translations usable at the minimum width.
        let mut rows = Column::new().spacing(10);
        for pair in items.chunks(2) {
            let mut line = iced::widget::Row::new().spacing(10);
            for (title, action) in pair {
                line = line.push(self.action(title, action.clone()));
            }
            rows = rows.push(line);
        }
        rows.into()
    }
    fn input<'a>(
        &'a self,
        title: &str,
        field: &'static str,
        placeholder: &str,
        pick: Option<Action>,
    ) -> Element<'a, Message> {
        let mut input = text_input(placeholder, self.forms.get(field))
            .id(field)
            .padding(12)
            .size(14);
        if self.can_edit_field(field) {
            input = input.on_input(move |v| Message::Field(field, v));
        }
        let mut line = iced::widget::Row::new()
            .push(input.width(Length::Fill))
            .spacing(10);
        if let Some(action) = pick {
            line = line.push(self.action(
                if matches!(action, FenixPick("installer")) {
                    "Datei wählen"
                } else {
                    "Ordner wählen"
                },
                action,
            ));
        }
        column![
            label(self.t(title).to_string(), 14.0, Weight::Semibold, INK),
            line
        ]
        .spacing(8)
        .into()
    }
    fn choices<'a>(
        &self,
        title: &str,
        field: &'static str,
        choices: impl IntoIterator<Item = (String, String)>,
    ) -> Element<'a, Message> {
        let choices: Vec<_> = choices
            .into_iter()
            .map(|(id, label)| Choice { id, label })
            .collect();
        let selected = choices
            .iter()
            .find(|c| c.id == self.forms.get(field))
            .cloned();
        column![
            label(self.t(title).to_string(), 14.0, Weight::Semibold, INK),
            pick_list(choices, selected, move |c: Choice| Message::Field(
                field, c.id
            ))
            .width(Length::Fill)
            .padding(12)
            .text_size(14)
        ]
        .spacing(8)
        .into()
    }
    fn options<'a>(
        &self,
        title: &str,
        field: &'static str,
        items: &[(&str, &str)],
    ) -> Element<'a, Message> {
        self.choices(
            title,
            field,
            items
                .iter()
                .map(|(id, label)| (id.to_string(), self.t(label).to_string())),
        )
    }
    fn flag<'a>(&self, title: &str, field: &'static str) -> Element<'a, Message> {
        let mut control = checkbox(self.forms.flag(field))
            .label(self.t(title).to_string())
            .size(18)
            .text_size(14);
        if !self.pending {
            control = control.on_toggle(move |v| Message::Flag(field, v));
        }
        control.into()
    }
    fn checks<'a>(&self, checks: &Value) -> Element<'a, Message> {
        let mut rows = Column::new().spacing(12);
        for check in checks.as_array().into_iter().flatten().take(100) {
            let (mark, color) = match check["ok"].as_bool() {
                Some(true) => ("✓", self.edition.accent()),
                Some(false) => ("!", iced::color!(0xff9199)),
                None => ("·", MUTED),
            };
            rows = rows.push(
                row![
                    label(mark, 20.0, Weight::Bold, color).width(24),
                    column![
                        label(s(check, "label").to_string(), 14.0, Weight::Semibold, INK),
                        self.paragraph(s(check, "detail").to_string())
                    ]
                    .spacing(4)
                ]
                .spacing(12),
            );
        }
        rows.into()
    }
    fn pairs<'a>(&self, data: &Value, fields: &[(&str, &str)]) -> Element<'a, Message> {
        let mut rows = Column::new().spacing(8);
        for (key, title) in fields {
            let v = &data[*key];
            if v.is_null() {
                continue;
            }
            let value = if let Some(text) = v.as_str() {
                text.to_string()
            } else if let Some(value) = v.as_bool() {
                self.tr(
                    if value { "Ja" } else { "Nein" },
                    if value { "Yes" } else { "No" },
                )
                .into()
            } else if v.is_number() {
                v.to_string()
            } else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            rows = rows.push(
                row![
                    label(self.t(title).to_string(), 13.0, Weight::Medium, MUTED).width(190),
                    label(value, 14.0, Weight::Normal, INK).width(Length::Fill)
                ]
                .spacing(14),
            );
        }
        rows.into()
    }
    fn job<'a>(&self, key: &str) -> Element<'a, Message> {
        let data = self.data(key);
        let job = &data["job"];
        let mut out = Column::new().spacing(12);
        for key in [
            "_error",
            "unavailable_reason",
            "message",
            "error",
            "startup_error",
        ] {
            if let Some(text) = data[key].as_str().filter(|v| !v.is_empty()) {
                out = out.push(self.paragraph(text.to_string()));
            }
        }
        if job.is_object() {
            if !s(job, "message").is_empty() {
                out = out.push(self.paragraph(s(job, "message").to_string()));
            }
            if !s(job, "error").is_empty() && job["error"] != job["message"] {
                out = out.push(label(
                    s(job, "error").to_string(),
                    14.0,
                    Weight::Medium,
                    iced::color!(0xff9199),
                ));
            }
            if active(job) {
                let transfer = &job["transfer"];
                if !transfer.is_null() {
                    let received = number(&transfer["received_bytes"]);
                    let total = number(&transfer["total_bytes"]);
                    let verified = number(&transfer["verified_bytes"]);
                    out = out.push(self.paragraph(format!(
                        "{} / {} · {} {}",
                        bytes(received),
                        if total > 0 { bytes(total) } else { "?".into() },
                        bytes(verified),
                        self.tr("geprüft", "verified")
                    )));
                    if let Some(progress) = model::transfer_progress(transfer) {
                        out = out.push(progress_bar(0.0..=100.0, progress));
                    }
                } else if let Some(progress) = job["progress"].as_f64().filter(|v| v.is_finite()) {
                    out = out.push(progress_bar(0.0..=100.0, progress.clamp(0.0, 100.0) as f32));
                } else {
                    out = out.push(
                        self.paragraph(self.tr("Vorgang läuft …", "Operation in progress …")),
                    );
                }
            }
            out = out.push(self.checks(&job["checks"]));
        }
        out.into()
    }
    pub(crate) fn confirmation_view<'a>(&self, request: &client::Request) -> Element<'a, Message> {
        self.card(
            "Bitte prüfen",
            column![
                self.paragraph(
                    self.t(request.confirmation.unwrap_or("Bitte prüfen"))
                        .to_string()
                ),
                self.paragraph(request.runtime.clone()),
                row![
                    button(self.tr("Bestätigen", "Confirm"))
                        .on_press(Message::Confirm)
                        .padding(12),
                    button(self.tr("Abbrechen", "Cancel"))
                        .on_press(Message::CancelConfirm)
                        .padding(12)
                ]
                .spacing(12)
            ]
            .spacing(15),
        )
    }
    pub(crate) fn page_view(&self) -> Element<'_, Message> {
        match self.page {
            Page::Setup => self.setup_page(),
            Page::Updates => self.updates_page(),
            Page::Saves => self.saves_page(),
            Page::Mods => self.mods_page(),
            Page::Diagnostics => self.diagnostics_page(),
            Page::Overview => Column::new().into(),
        }
    }
    pub(crate) fn overview_details(&self, width: f32, _compact: bool) -> Element<'_, Message> {
        let installation = self.card(
            "Installation",
            column![
                self.paragraph(self.runtime()),
                self.checks(&self.status()["runtime"]["checks"]),
                self.link(
                    self.tr("Installationen verwalten", "Manage installations"),
                    Page::Setup
                ),
                self.link(
                    self.tr("Updates & Dateiprüfung", "Updates & file verification"),
                    Page::Updates
                )
            ]
            .spacing(14),
        );
        let saves = self.card(
            "Lokale Spielstände",
            column![
                self.save_summary(),
                self.link(
                    self.tr("Spielstände verwalten", "Manage saves"),
                    Page::Saves
                )
            ]
            .spacing(14),
        );
        if width < 760.0 {
            column![installation, saves].spacing(20).into()
        } else {
            row![installation, saves].spacing(20).into()
        }
    }
    fn save_summary(&self) -> Element<'_, Message> {
        let saves = &self.status()["saves"];
        column![
            self.paragraph(if yes(saves, "available") {
                self.tr("Lokaler Speicher aktiv", "Local storage active")
            } else {
                self.tr(
                    "Lokaler Speicher nicht verfügbar",
                    "Local storage unavailable",
                )
            }),
            self.paragraph(format!(
                "{} · {} {} · {} {}",
                bytes(number(&saves["bytes"])),
                number(&saves["files"]),
                self.tr("Dateien", "files"),
                number(&saves["backups"]),
                self.tr("Sicherungen", "backups")
            )),
            self.pairs(
                &saves["last_backup"],
                &[("created_at", "Letztes Backup"), ("name", "Datei")]
            )
        ]
        .spacing(10)
        .into()
    }
    pub(crate) fn automatic_saves(&self) -> Element<'_, Message> {
        let cloud = &self.status()["cloud"];
        if !yes(cloud, "enabled") {
            return Column::new().into();
        }
        let mut content=column![self.paragraph(self.tr("Flightdeck gleicht deine Xbox-Spielstände vor dem Start und nach dem Beenden ab. Vor Änderungen bleibt eine lokale Sicherung erhalten.","Flightdeck syncs your Xbox saves before starting and after exiting. A local backup is kept before changes.")),self.paragraph(s(cloud,"message")),self.paragraph(s(cloud,"error"))].spacing(12);
        if yes(cloud, "conflict") {
            content=content.push(self.paragraph(self.tr("Lokal und in der Cloud gibt es unterschiedliche Änderungen. Wähle den Stand, den du behalten möchtest.","Local and cloud saves have changed. Choose the version you want to keep."))).push(self.pairs(&cloud["summary"],&[("add_count","Neu"),("replace_count","Ersetzen"),("delete_count","Löschen"),("conflict_count","Konflikte")]))
        }
        let mut buttons = Column::new().spacing(10);
        for (title, op) in [
            ("Erneut versuchen", "retry"),
            ("Anmelden", "sign-in"),
            ("Lokal spielen", "play-local"),
            ("Abbrechen", "cancel-auto"),
            ("Cloud-Stand verwenden", "cloud"),
            ("Lokalen Stand verwenden", "local"),
        ] {
            let action = Automatic(op);
            if self.request(&action).is_some() {
                buttons = buttons.push(self.action(title, action));
            }
        }
        content = content.push(buttons);
        self.card("Automatischer Cloud-Abgleich", content)
    }
    fn setup_page(&self) -> Element<'_, Message> {
        let mode = self.forms.get("mode");
        let mut install = column![
            self.edition_picker(),
            self.options(
                "Installation",
                "mode",
                &[
                    ("install", "Neu installieren"),
                    ("existing", "Vorhandene Installation verbinden"),
                    ("prepare", "Erweiterte Einrichtung")
                ]
            )
        ]
        .spacing(16);
        match mode {
            "existing" => {
                install = install
                    .push(self.input(
                        "Installationsordner",
                        "runtime_path",
                        "/…",
                        Some(Pick("runtime_path")),
                    ))
                    .push(
                        button(self.tr("Installationen suchen", "Find installations"))
                            .on_press_maybe(
                                (self.online && !self.pending)
                                    .then_some(Message::Discover("setup/discover")),
                            )
                            .padding(12),
                    );
                if let Some(discovered) = self.discoveries.get("setup/discover") {
                    for item in discovered["runtimes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(40)
                    {
                        if let Some(path) = item["path"].as_str() {
                            install = install.push(
                                button(label(
                                    format!("{} · {path}", s(item, "name")),
                                    13.0,
                                    Weight::Normal,
                                    INK,
                                ))
                                .on_press(Message::Field("runtime_path", path.to_string()))
                                .padding(10),
                            );
                        }
                    }
                    if yes(discovered, "limited") {
                        install = install.push(self.paragraph(self.tr(
                            "Die Suche wurde begrenzt. Du kannst den Pfad direkt eingeben.",
                            "Search was limited. You can enter the path directly.",
                        )));
                    }
                }
            }
            "prepare" => {
                for (field, title) in [
                    ("runtime_path", "Installationsordner"),
                    ("artifacts_path", "Runtime-Komponenten"),
                    ("game_path", "Spielordner"),
                    ("runner_path", "Wine-Runner"),
                    ("prefix_path", "Windows-Profil"),
                    ("media_plugins_path", "Media-Plugins (optional)"),
                ] {
                    install = install.push(self.input(title, field, "/…", Some(Pick(field))));
                }
                install = install.push(self.input(
                    "Store-Region (ISO-Code)",
                    "market",
                    "AT / DE / US",
                    None,
                ));
            }
            _ => {
                install=install.push(self.input("Zielordner (optional)","destination_path",self.tr("Standardordner verwenden","Use default folder"),Some(Pick("destination_path")))).push(self.input("Store-Region (ISO-Code)","market","AT / DE / US",None)).push(self.paragraph(self.tr("Du benötigst die Xbox-PC-Version. Melde dich im Microsoft-Fenster mit dem Konto an, dem das Spiel gehört.","You need the Xbox PC version. Sign in in the Microsoft window using the account that owns the game.")));
            }
        }
        install = install.push(self.job("setup")).push(self.actions(&[
            (
                if mode == "existing" {
                    "Verbinden"
                } else {
                    "Angaben prüfen"
                },
                SetupCheck,
            ),
            ("Installation starten", SetupStart),
            ("Download pausieren", SetupControl("pause")),
            ("Fortsetzen", SetupControl("resume")),
            ("Abbrechen", SetupControl("cancel")),
        ]));
        let graphics = self.card(
            "Grafik",
            column![
                self.options(
                    "NVIDIA-Modus",
                    "graphics",
                    &[
                        ("auto", "Automatisch"),
                        ("compatibility", "Kompatibilität"),
                        ("features", "NVIDIA-Funktionen")
                    ]
                ),
                self.paragraph(s(&self.status()["graphics"], "error")),
                self.action("Speichern", Graphics)
            ]
            .spacing(14),
        );
        let vr = self.card(
            "Virtual Reality",
            column![
                self.options(
                    "VR-Modus",
                    "vr",
                    &[
                        ("off", "Aus"),
                        ("auto", "Automatisch"),
                        ("wivrn", "WiVRn"),
                        ("steamvr", "SteamVR"),
                        ("monado", "Monado")
                    ]
                ),
                self.paragraph(s(&self.status()["vr"], "message")),
                self.paragraph(s(&self.status()["vr"], "error")),
                self.paragraph(s(&self.status()["vr"]["check"], "message")),
                self.actions(&[("Speichern", VrSave), ("Headset prüfen", VrCheck)])
            ]
            .spacing(14),
        );
        column![
            self.card("Simulator einrichten", install),
            graphics,
            vr,
            self.proton_card(),
            self.maintenance_card()
        ]
        .spacing(20)
        .into()
    }
    fn proton_card(&self) -> Element<'_, Message> {
        let mut choices = vec![("default".into(), "Flightdeck (Xodus, Standard)".into())];
        if let Some(discovered) = self.discoveries.get("proton/discover") {
            for item in discovered["choices"].as_array().into_iter().flatten() {
                if let Some(path) = item["path"].as_str() {
                    choices.push((
                        path.into(),
                        format!("{} {}", s(item, "label"), s(item, "version")),
                    ));
                }
            }
        }
        choices.push((
            "custom".into(),
            self.t("Anderen Proton-Ordner wählen").into(),
        ));
        self.card("Proton-Version",column![self.paragraph(self.tr("Flightdeck bleibt die Standardumgebung. Andere installierte Proton-Versionen werden in einer eigenen Profilkopie vorbereitet.","Flightdeck remains the default runner. Other installed Proton versions are prepared in a separate profile copy.")),self.pairs(self.data("proton"),&[("label","Aktive Proton-Version:"),("version","Version")]),self.choices("Proton-Version","proton",choices),self.input("Proton-Ordner","proton_path","/…",Some(Pick("proton_path"))),button(self.tr("Proton-Versionen suchen","Find Proton versions")).on_press_maybe((self.online&&!self.pending).then_some(Message::Discover("proton/discover"))).padding(12),self.job("proton"),self.actions(&[("Auswahl übernehmen",Proton),("Flightdeck wiederherstellen",ProtonDefault),("Abbrechen",ProtonCancel)])].spacing(14))
    }
    fn maintenance_card(&self) -> Element<'_, Message> {
        let job = &self.data("maintenance")["job"];
        let mut content=column![self.paragraph(self.tr("Prüfe zuerst, welche Dateien betroffen sind. Änderungen beginnen erst nach deiner Bestätigung.","Review the affected files first. Changes begin only after your confirmation.")),self.flag("Spieldateien löschen","delete_packages"),self.flag("Spielstände und Add-ons behalten","keep_data"),self.actions(&[("Windows-Profil zurücksetzen",Maintenance("reset")),("Windows-Profil wiederherstellen",Maintenance("restore")),("Installation entfernen",Maintenance("uninstall"))]),self.job("maintenance")].spacing(14);
        if job["state"] == "ready" {
            content = content
                .push(self.heading(self.t("Vorschau")))
                .push(self.pairs(
                    job,
                    &[
                        ("operation", "Vorgang"),
                        ("runtime_path", "Installationsordner"),
                        ("game_path", "Spielordner"),
                        ("keep_data", "Spielstände und Add-ons behalten"),
                        ("delete_packages", "Spieldateien löschen"),
                        ("package_bytes", "Bytes"),
                    ],
                ));
            let effect = match s(job, "operation") {
                "uninstall" if yes(job, "keep_data") => {
                    "Einstellungen, lokale Spielstände und die übrige Installation werden in einem Sicherungsordner behalten."
                }
                "uninstall" => {
                    "Die gesamte ausgewählte Installation mit lokalen Spielständen und Einstellungen wird dauerhaft gelöscht."
                }
                "restore" => {
                    "Die letzte gesicherte Umgebung wird wieder aktiviert. Die aktuelle Umgebung bleibt ebenfalls erhalten."
                }
                _ => {
                    "Eine frische Windows-Umgebung ersetzt die bisherige. Die alte Umgebung wird gesichert. Basisspiel und lokale Spielstände bleiben erhalten; Zusatzprogramme müssen neu eingerichtet werden."
                }
            };
            content = content.push(self.paragraph(self.t(effect).to_string()));
            content = content.push(self.paragraph(self.t(if job["operation"] == "uninstall" && !yes(job, "delete_packages") {
                "Die Spieldateien bleiben erhalten. Die Installation wird nur aus Flightdeck entfernt."
            } else {
                "Externe Add-ons, Runner, Anmeldedaten außerhalb des Installationsordners und Xbox-Cloud-Spielstände bleiben erhalten."
            }).to_string()));
            for key in ["effects", "retained", "packages"] {
                for item in job[key].as_array().into_iter().flatten().take(100) {
                    if let Some(text) = item.as_str() {
                        content = content.push(self.paragraph(text.to_string()));
                    } else {
                        content = content.push(self.pairs(
                            item,
                            &[
                                ("path", "Ordner"),
                                ("total_bytes", "Bytes"),
                                ("label", "Änderung"),
                            ],
                        ))
                    }
                }
            }
            content = content.push(self.actions(&[
                ("Geprüfte Änderungen durchführen", MaintenanceStart),
                ("Vorschau verwerfen", MaintenanceDiscard),
            ]));
        }
        self.card("Wiederherstellen, zurücksetzen oder entfernen", content)
    }
    fn updates_page(&self) -> Element<'_, Message> {
        let launcher = self.data("launcher-update");
        let game = self.data("game-update");
        column![
            self.card(
                "Flightdeck",
                column![
                    self.pairs(
                        launcher,
                        &[
                            ("installed_version", "Installiert"),
                            ("latest_version", "Verfügbar"),
                            ("checked_at", "Zuletzt geprüft"),
                            ("pending_restart", "Neustart erforderlich")
                        ]
                    ),
                    self.job("launcher-update"),
                    self.actions(&[
                        ("Nach Updates suchen", Launcher("check")),
                        ("Update installieren", Launcher("install")),
                        ("Flightdeck neu starten", Launcher("restart")),
                        ("Abbrechen", Launcher("cancel")),
                        ("Vorherige Version wiederherstellen", Launcher("rollback"))
                    ]),
                    self.paragraph(s(launcher, "notes"))
                ]
                .spacing(16)
            ),
            self.card(
                "Simulator-Updates",
                column![
                    self.paragraph(self.edition.name()),
                    self.pairs(
                        game,
                        &[
                            ("installed_version", "Installiert"),
                            ("latest_version", "Verfügbar")
                        ]
                    ),
                    self.job("game-update"),
                    self.actions(&[
                        ("Nach Updates suchen", Game("check")),
                        ("Anmelden und prüfen", Game("sign-in")),
                        ("Update installieren", Game("start")),
                        ("Download pausieren", SetupControl("pause")),
                        ("Fortsetzen", SetupControl("resume")),
                        ("Abbrechen", SetupControl("cancel")),
                        ("Dateien prüfen", Game("verify")),
                        ("Reparatur prüfen", Game("repair")),
                        ("Vorherige Version wiederherstellen", Game("rollback"))
                    ]),
                    self.pairs(
                        &game["integrity"]["result"],
                        &[
                            ("healthy", "Dateien intakt"),
                            ("checked", "Geprüft"),
                            ("missing", "Fehlend"),
                            ("changed", "Verändert"),
                            ("unreadable", "Nicht lesbar"),
                            ("total", "Gesamt")
                        ]
                    )
                ]
                .spacing(16)
            )
        ]
        .spacing(20)
        .into()
    }
    fn saves_page(&self) -> Element<'_, Message> {
        let cloud = self.data("cloud-saves");
        let plan = &cloud["plan"];
        let mut manual = column![
            self.paragraph(self.tr(
                "Manueller Abgleich: Prüfe die Änderungen vor dem Übernehmen oder Hochladen. Bestehende lokale Daten werden vor Änderungen gesichert.",
                "Manual sync: review changes before importing or uploading. Existing local data is backed up before changes."
            )),
            self.job("cloud-saves"),
            self.pairs(&cloud["job"]["result"], &[
                ("container_count", "Container"), ("blob_count", "Dateien"), ("total_bytes", "Bytes")
            ])
        ].spacing(16);
        if plan.is_object() {
            manual = manual
                .push(self.heading(self.t("Vorschau")))
                .push(self.pairs(
                    plan,
                    &[
                        ("container_count", "Cloud-Container"),
                        ("local_container_count", "Lokale Container"),
                        ("blob_count", "Dateien"),
                        ("total_bytes", "Bytes"),
                        ("add_count", "Neu"),
                        ("replace_count", "Ersetzen"),
                        ("delete_count", "Löschen"),
                        ("unchanged_count", "Unverändert"),
                        ("conflict_count", "Konflikte"),
                    ],
                ));
            if number(&plan["conflict_count"]) > 0 {
                manual = manual.push(self.paragraph(self.tr(
                    "Die Spielstände unterscheiden sich ohne gemeinsamen Vergleichsstand. Wähle bewusst, welchen Stand du behalten möchtest.",
                    "These saves differ without a common baseline. Choose which version you want to keep."
                )));
            }
            if plan["container_count"] == 0 && number(&plan["local_container_count"]) > 0 {
                manual = manual.push(self.paragraph(self.tr(
                    "Die Cloud ist leer. Beim Übernehmen werden die vorhandenen lokalen Spielstände entfernt und vorher gesichert.",
                    "The cloud is empty. Importing it will back up and remove the existing local saves."
                )));
            }
            manual = manual.push(self.paragraph(format!(
                "{}: {} {} · {} {} · {} {}",
                self.tr("Beim Hochladen", "When uploading"),
                number(&plan["delete_count"]),
                self.tr("hinzufügen", "add"),
                number(&plan["replace_count"]),
                self.tr("ersetzen", "replace"),
                number(&plan["add_count"]),
                self.tr("entfernen", "remove")
            )));
        }
        manual = manual.push(self.actions(&[
            ("Cloud prüfen", Cloud("check")),
            ("Cloud herunterladen", Cloud("download")),
            ("Änderungen prüfen", Cloud("prepare-import")),
            ("Cloud-Stand übernehmen", Cloud("import")),
            ("Lokalen Stand hochladen", Cloud("upload")),
            ("Plan verwerfen", Cloud("discard-plan")),
            ("Sicherung wiederherstellen", Cloud("restore")),
            ("Abbrechen", Cloud("cancel")),
        ]));
        column![
            self.automatic_saves(),
            self.card(
                "Lokale Spielstände",
                column![self.save_summary(), self.action("Backup erstellen", Backup)].spacing(16)
            ),
            self.card("Cloud-Spielstände", manual)
        ]
        .spacing(20)
        .into()
    }
    fn mods_page(&self) -> Element<'_, Message> {
        let data = self.data("fenix");
        let steps = [
            yes(data, "installed"),
            yes(data, "fenix_installed"),
            yes(data, "settings_ready"),
            yes(data, "configured"),
        ];
        let titles = [
            self.tr("Linux-Patch", "Linux patch"),
            self.tr("Flugzeug installieren", "Install aircraft"),
            self.tr("Fenix anmelden", "Sign in to Fenix"),
            self.tr("Einrichtung abschließen", "Finish setup"),
        ];
        let mut steps_view = Column::new().spacing(12);
        for (i, done) in steps.into_iter().enumerate() {
            steps_view = steps_view.push(label(
                format!("{}  {}. {}", if done { "✓" } else { "○" }, i + 1, titles[i]),
                16.0,
                Weight::Semibold,
                if done { self.edition.accent() } else { INK },
            ));
        }
        let fenix=self.card("Fenix A320",column![steps_view,self.paragraph(self.tr("Installiere das Flugzeug mit dem offiziellen Fenix-Installer. Melde dich anschließend in Fenix an und schließe alle Fenix-Fenster, bevor du die Einrichtung abschließt.","Install the aircraft using the official Fenix installer. Sign in to Fenix, then close every Fenix window before finishing setup.")),self.input("Offizieller Fenix-Installer","installer_path","FenixInstaller.exe",Some(FenixPick("installer"))),self.input("Lokales Patch-Paket (optional)","bundle_path",self.tr("Automatisch herunterladen","Download automatically"),Some(FenixPick("bundle"))),self.job("fenix"),self.actions(&[(if yes(data,"can_retry"){"Einrichtung reparieren"}else if yes(data,"update_available"){"Patch aktualisieren"}else{"Patch einrichten"},Fenix("install")),("Installer starten",Fenix("installer")),("Fenix öffnen",Fenix("open")),("Einrichtung abschließen",Fenix("configure")),("Fenix Installer öffnen",Fenix("manager")),("Fenix beenden",Fenix("stop")),("Profil vor dem Patch wiederherstellen",Fenix("restore"))])].spacing(16));
        let gsx=self.card("GSX · experimentell",column![self.paragraph(self.tr("1. FSDT vorbereiten. 2. GSX im offiziellen FSDT-Installer installieren und aktivieren. 3. Automatischen Start übernehmen. Der Flugbetrieb unter Linux ist noch nicht bestätigt.","1. Prepare FSDT. 2. Install and activate GSX in the official FSDT installer. 3. Configure automatic startup. In-flight operation on Linux is not yet confirmed.")),self.pairs(self.data("gsx"),&[("prepared","FSDT vorbereitet"),("package_installed","GSX installiert"),("configured","Automatischer Start eingerichtet")]),self.job("gsx"),self.actions(&[("FSDT vorbereiten",Gsx("prepare")),("FSDT öffnen",Gsx("open")),("Automatischen Start einrichten",Gsx("configure")),("Automatischen Start deaktivieren",Gsx("disable")),("FSDT beenden",Gsx("stop")),("Profil wiederherstellen",Gsx("recover"))])].spacing(16));
        let mods = self.data("mods");
        let mut inventory = column![
            self.paragraph(s(mods, "folder_path")),
            self.job("mods"),
            self.action("Community-Ordner öffnen", ModsOpen)
        ]
        .spacing(14);
        for item in mods["mods"].as_array().into_iter().flatten().take(500) {
            inventory = inventory
                .push(separator())
                .push(label(
                    if s(item, "name").is_empty() {
                        s(item, "id")
                    } else {
                        s(item, "name")
                    },
                    16.0,
                    Weight::Semibold,
                    INK,
                ))
                .push(self.pairs(
                    item,
                    &[
                        ("creator", "Ersteller"),
                        ("version", "Version"),
                        ("id", "Ordner"),
                        ("status", "Status"),
                    ],
                ));
        }
        if yes(mods, "limited") {
            inventory = inventory.push(self.paragraph(self.tr(
                "Die Liste zeigt einen begrenzten Ausschnitt.",
                "The list shows a limited selection.",
            )));
        }
        column![fenix, gsx, self.card("Community-Ordner", inventory)]
            .spacing(20)
            .into()
    }
    fn diagnostic_summary<'a>(&self, summary: &Value) -> Element<'a, Message> {
        let mut out = Column::new().spacing(14);
        for (key, title) in [
            ("graphics", "Grafik und Vulkan"),
            ("proton", "Proton-Version"),
            ("audio", "Audio und Medien"),
            ("vr", "Virtual Reality"),
            ("cloud_sync", "Xbox-Cloud-Abgleich"),
            ("store_session", "Store-Sitzungsverlauf"),
            ("exit", "Letztes Sitzungsende"),
        ] {
            if summary[key].is_null() {
                continue;
            }
            let text = serde_json::to_string_pretty(&summary[key]).unwrap_or_default();
            out = out
                .push(label(
                    self.t(title).to_string(),
                    15.0,
                    Weight::Semibold,
                    INK,
                ))
                .push(label(
                    text.chars().take(12000).collect::<String>(),
                    12.0,
                    Weight::Normal,
                    MUTED,
                ));
        }
        out.into()
    }
    fn diagnostics_page(&self) -> Element<'_, Message> {
        let data = self.data("diagnostics");
        let store = self.data("store-check");
        let diagnostics=self.card("Diagnose",column![self.paragraph(self.tr("Prüfungen beziehen sich auf die ausgewählte Installation. Der Export enthält die vom Dienst bereinigten Diagnosedaten.","Checks apply to the selected installation. Export contains diagnostics sanitized by the service.")),self.job("diagnostics"),self.checks(&data["checks"]),self.diagnostic_summary(&data["summary"]),row![button(self.tr("Aktualisieren","Refresh")).on_press_maybe((!self.pending).then_some(Message::Refresh)).padding(12),button(self.tr("Diagnose kopieren","Copy diagnostics")).on_press_maybe(self.diagnostics_text().is_some().then_some(Message::CopyDiagnostics)).padding(12),button(self.tr("Diagnose speichern","Save diagnostics")).on_press_maybe((!self.exporting && self.diagnostics_text().is_some()).then_some(Message::SaveDiagnostics)).padding(12)].spacing(12)].spacing(16));
        let mut store_view = column![
            self.job("store-check"),
            self.actions(&[
                ("Store prüfen", Store("start")),
                ("Microsoft-Anmeldung erneuern", Store("sign-in")),
                ("Abbrechen", Store("cancel"))
            ])
        ]
        .spacing(16);
        for step in store["job"]["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .take(30)
        {
            store_view = store_view.push(self.pairs(
                step,
                &[
                    ("stage", "Prüfschritt"),
                    ("state", "Status"),
                    ("code", "Ergebnis"),
                    ("message", "Details"),
                ],
            ));
        }
        let mut report=column![self.paragraph(self.tr("Beschreibe den Fehler. Flightdeck ergänzt bereinigte Diagnosedaten. Bitte keine Passwörter oder privaten Rohlogs einfügen.","Describe the issue. Flightdeck adds sanitized diagnostics. Please do not include passwords or private raw logs.")),self.options("Kategorie","category",&[("graphics","Grafik / NVIDIA"),("cloud","Cloud-Sync"),("marketplace","Marketplace"),("installation","Installation / Start"),("other","Anderer Fehler")]),column![label(self.t("Was ist passiert?").to_string(),14.0,Weight::Semibold,INK),text_editor(&self.description).placeholder(self.tr("Mindestens 10 Zeichen","At least 10 characters")).height(150).padding(12).on_action(Message::EditDescription)].spacing(8)].spacing(16);
        if self.forms.get("category") == "graphics" {
            for (field, title) in [
                ("menus_visible", "Menüs sind sichtbar"),
                ("main_view_black", "Die Hauptansicht ist schwarz"),
                (
                    "second_window_works",
                    "Ein zweites Renderfenster funktioniert",
                ),
                (
                    "second_window_crashes",
                    "Ein zweites Renderfenster verursacht einen Absturz",
                ),
            ] {
                report = report.push(self.flag(title, field));
            }
        }
        report = report.push(self.action("Bericht vorbereiten", Report));
        if let Some(text) = self.report_text() {
            report = report
                .push(self.paragraph(self.tr(
                    "Prüfe den Bericht vor dem Weitergeben. Es wird nichts automatisch versendet.",
                    "Review the report before sharing. Nothing is sent automatically.",
                )))
                .push(self.pairs(self.data("problem-reports"), &[("recipient", "Empfänger")]))
                .push(
                    row![
                        button(self.tr("Bericht kopieren", "Copy report"))
                            .on_press(Message::CopyReport)
                            .padding(12),
                        button(self.tr("Bericht speichern", "Save report"))
                            .on_press_maybe((!self.exporting).then_some(Message::SaveReport))
                            .padding(12)
                    ]
                    .spacing(12),
                )
                .push(
                    button(self.tr("E-Mail-Entwurf öffnen", "Open email draft"))
                        .padding(12)
                        .on_press_maybe(
                            (!self.exporting && self.mail_uri().is_some())
                                .then_some(Message::MailReport),
                        ),
                )
                .push(self.action("Entwurf löschen", ReportDiscard))
                .push(label(text, 12.0, Weight::Normal, MUTED));
        }
        column![
            diagnostics,
            self.card("Microsoft Store / Marketplace", store_view),
            self.card("Problem melden", report)
        ]
        .spacing(20)
        .into()
    }
}
