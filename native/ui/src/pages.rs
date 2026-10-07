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
    pub(crate) fn help_link<'a>(&self, title: &str, link: HelpLink) -> Element<'a, Message> {
        button(label(
            format!("{} ↗", self.t(title)),
            14.0,
            Weight::Semibold,
            self.edition.accent(),
        ))
        .padding([6, 0])
        .on_press_maybe((!self.exporting).then_some(Message::OpenHelp(link)))
        .style(button::text)
        .into()
    }
    pub(crate) fn t<'a>(&self, text: &'a str) -> &'a str {
        if self.language == Language::En {
            TRANSLATIONS.get(text).map(String::as_str).unwrap_or(text)
        } else {
            text
        }
    }
    pub(crate) fn paragraph<'a>(
        &self,
        text: impl Into<std::borrow::Cow<'a, str>>,
    ) -> Element<'a, Message> {
        label(text, 16.0, Weight::Normal, MUTED)
            .line_height(1.5)
            .into()
    }
    pub(crate) fn heading<'a>(
        &self,
        text: impl Into<std::borrow::Cow<'a, str>>,
    ) -> Element<'a, Message> {
        label(text, 26.0, Weight::Semibold, INK)
            .weight(650)
            .tracking(-0.7)
            .into()
    }
    pub(crate) fn card<'a>(
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
    pub(crate) fn disclosure<'a>(
        &self,
        section: Disclosure,
        title: &str,
        summary: &str,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let expanded = self.expanded.contains(&section);
        let mut title_row =
            row![label(self.t(title).to_string(), 20.0, Weight::Semibold, INK).tracking(-0.4)]
                .spacing(12)
                .align_y(alignment::Vertical::Center);
        let badge = match section {
            Disclosure::Fenix => Some("Community-Vorschau"),
            Disclosure::Gsx => Some("Experimentell"),
            _ => None,
        };
        if let Some(badge) = badge {
            title_row = title_row.push(
                container(label(
                    self.t(badge).to_string(),
                    11.0,
                    Weight::Semibold,
                    self.edition.accent(),
                ))
                .padding([4, 9])
                .style(|_| {
                    container::Style::default()
                        .background(iced::color!(0x19343e))
                        .border(Border {
                            color: LINE,
                            width: 1.0,
                            radius: 20.0.into(),
                        })
                }),
            );
        }
        let mut heading = column![title_row.wrap()].spacing(5);
        if !summary.is_empty() {
            heading = heading.push(label(
                self.t(summary).to_string(),
                13.0,
                Weight::Normal,
                MUTED,
            ));
        }
        let toggle = button(
            row![
                heading.width(Length::Fill),
                svg(svg::Handle::from_memory(format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="{}" fill="none" stroke="#a6b8c9" stroke-width="2"/></svg>"##,if expanded { "m5 15 7-7 7 7" } else { "m5 9 7 7 7-7" }).into_bytes())).width(20).height(20)
            ]
            .spacing(18)
            .align_y(alignment::Vertical::Center),
        )
        .width(Length::Fill)
        .padding([21, 24])
        .on_press(Message::Toggle(section))
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered)
                .then_some(iced::color!(0x17303e).into()),
            text_color: INK,
            border: Border {
                radius: 14.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let mut body = column![toggle];
        if expanded {
            body = body
                .push(separator())
                .push(container(content.into()).padding(24).width(Length::Fill));
        }
        container(body).width(Length::Fill).style(card_style).into()
    }
    pub(crate) fn control<'a>(
        &self,
        title: &str,
        message: Option<Message>,
        primary: bool,
    ) -> Element<'a, Message> {
        let accent = self.edition.accent();
        let enabled = message.is_some();
        let color = if !enabled {
            MUTED.scale_alpha(0.48)
        } else if primary {
            BG
        } else {
            INK
        };
        button(label(
            self.t(title).to_string(),
            14.0,
            Weight::Semibold,
            color,
        ))
        .padding([11, 16])
        .on_press_maybe(message)
        .style(move |_, status| {
            let background = if primary && enabled {
                accent
            } else if status == button::Status::Hovered {
                iced::color!(0x19303e)
            } else {
                iced::color!(0x10212c)
            };
            button::Style {
                background: Some(background.into()),
                text_color: color,
                border: Border {
                    color: if primary && enabled { accent } else { LINE },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
    }
    pub(crate) fn refresh_button<'a>(&self) -> Element<'a, Message> {
        self.control(
            "Status neu laden",
            (!self.pending).then_some(Message::Refresh),
            false,
        )
    }
    pub(crate) fn action<'a>(&self, title: &str, action: Action) -> Element<'a, Message> {
        let allowed = self.request(&action).is_some();
        let primary = matches!(
            action,
            Launch
                | Backup
                | SetupCheck
                | SetupStart
                | ModsOpen
                | ModsRemove
                | Report
                | Graphics
                | VrSave
                | Proton
                | MaintenanceStart
                | Launcher("install" | "restart")
                | Game("start" | "sign-in")
                | Fenix("install" | "configure")
                | Gsx("prepare" | "configure")
        );
        self.control(title, allowed.then_some(Message::Action(action)), primary)
    }
    pub(crate) fn actions<'a>(&self, items: &[(&str, Action)]) -> Element<'a, Message> {
        iced::widget::Row::with_children(
            items
                .iter()
                .map(|(title, action)| self.action(title, action.clone())),
        )
        .spacing(10)
        .wrap()
        .into()
    }
    pub(crate) fn input<'a>(
        &'a self,
        title: &str,
        field: &'static str,
        placeholder: &str,
        pick: Option<Action>,
    ) -> Element<'a, Message> {
        let mut input = text_input(placeholder, self.forms.get(field))
            .id(field)
            .padding(12)
            .size(16)
            .style(|theme, status| {
                let mut style = text_input::default(theme, status);
                style.background = BG.into();
                style.border = Border {
                    color: LINE,
                    width: 1.0,
                    radius: 8.0.into(),
                };
                style
            });
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
    pub(crate) fn choices<'a>(
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
        let control: Element<'a, Message> = if self.can_edit_field(field) {
            pick_list(choices, selected, move |c: Choice| {
                Message::Field(field, c.id)
            })
            .placeholder(self.tr("Bitte auswählen …", "Select …"))
            .width(Length::Fill)
            .padding(12)
            .text_size(16)
            .style(|theme, status| {
                let mut style = iced::widget::pick_list::default(theme, status);
                style.background = BG.into();
                style.border = Border {
                    color: LINE,
                    width: 1.0,
                    radius: 8.0.into(),
                };
                style
            })
            .into()
        } else {
            container(label(
                selected.map(|c| c.label).unwrap_or_default(),
                14.0,
                Weight::Normal,
                MUTED,
            ))
            .padding(12)
            .width(Length::Fill)
            .style(card_style)
            .into()
        };
        column![
            label(self.t(title).to_string(), 14.0, Weight::Semibold, INK),
            container(control).id(field).width(Length::Fill)
        ]
        .spacing(8)
        .into()
    }
    pub(crate) fn options<'a>(
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
    pub(crate) fn flag<'a>(&self, title: &str, field: &'static str) -> Element<'a, Message> {
        let mut control = checkbox(self.forms.flag(field))
            .label(self.t(title).to_string())
            .size(18)
            .text_size(16);
        if !self.pending {
            control = control.on_toggle(move |v| Message::Flag(field, v));
        }
        control.into()
    }
    fn checks<'a>(&self, checks: &Value) -> Element<'a, Message> {
        let mut rows = Column::new().spacing(16);
        for check in checks.as_array().into_iter().flatten().take(100) {
            let (mark, color) = match check["ok"].as_bool() {
                Some(true) => ("✓", self.edition.accent()),
                Some(false) => ("!", iced::color!(0xff9199)),
                None => ("·", MUTED),
            };
            rows = rows
                .push(
                    row![
                        if mark == "✓" {
                            icon("check-circle", 32.0, color)
                        } else {
                            label(mark, 20.0, Weight::Bold, color).width(32).into()
                        },
                        column![
                            label(s(check, "label").to_string(), 14.0, Weight::Semibold, INK),
                            label(s(check, "detail").to_string(), 13.0, Weight::Normal, MUTED)
                        ]
                        .spacing(4)
                    ]
                    .spacing(18)
                    .align_y(alignment::Vertical::Center),
                )
                .push(separator());
        }
        rows.into()
    }
    pub(crate) fn pairs<'a>(&self, data: &Value, fields: &[(&str, &str)]) -> Element<'a, Message> {
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
    pub(crate) fn job<'a>(&self, key: &str) -> Element<'a, Message> {
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
                self.pairs(
                    if request.path == "mods/remove" {
                        &self.data("mods")["job"]
                    } else {
                        &Value::Null
                    },
                    &[
                        ("addon_id", "Add-on"),
                        ("entry_path", "Betroffener Eintrag")
                    ]
                ),
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
        let updates = self.card(
            "Updates",
            column![
                self.overview_update_status("launcher-update", "Flightdeck"),
                self.overview_update_status("game-update", "Simulator"),
                label(
                    self.tr(
                        "Automatische Prüfung · Downloads startest du selbst.",
                        "Automatic checks · You choose when to download."
                    ),
                    13.0,
                    Weight::Normal,
                    MUTED
                ),
                self.link(self.tr("Updates öffnen", "Open updates"), Page::Updates)
            ]
            .spacing(12),
        );
        let installation = self.card(
            "Installation",
            column![
                self.paragraph(self.edition.name()),
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
                separator(),
                self.automatic_saves(),
                self.link(
                    self.tr("Spielstände verwalten", "Manage saves"),
                    Page::Saves
                )
            ]
            .spacing(14),
        );
        let details: Element<'_, Message> = if width < 760.0 {
            column![installation, saves].spacing(20).into()
        } else {
            row![
                container(installation).width(Length::FillPortion(11)),
                container(saves).width(Length::FillPortion(10))
            ]
            .spacing(22)
            .into()
        };
        column![updates, details].spacing(20).into()
    }
    fn overview_update_status(&self, key: &'static str, name: &str) -> Element<'_, Message> {
        let data = self.data(key);
        let status = if !self.fresh(key) {
            self.tr("Status nicht verfügbar", "Status unavailable")
                .to_string()
        } else if key == "launcher-update" {
            self.t(presentation::launcher_update(data)).to_string()
        } else {
            self.t(presentation::game_update(data)).to_string()
        };
        let available = self.fresh(key) && yes(data, "update_available");
        let version = if available {
            format!(" · {}", s(data, "latest_version"))
        } else {
            String::new()
        };
        label(
            format!("{name}: {status}{version}"),
            15.0,
            if available {
                Weight::Semibold
            } else {
                Weight::Normal
            },
            if available {
                self.edition.accent()
            } else {
                MUTED
            },
        )
        .into()
    }
    fn save_summary(&self) -> Element<'_, Message> {
        let saves = &self.status()["saves"];
        row![
            icon("drive", 30.0, MUTED),
            column![
                label(
                    if yes(saves, "available") {
                        self.tr("Lokaler Speicher aktiv", "Local storage active")
                    } else {
                        self.tr(
                            "Lokaler Speicher nicht verfügbar",
                            "Local storage unavailable",
                        )
                    },
                    14.0,
                    Weight::Semibold,
                    INK
                ),
                label(
                    format!(
                        "{} · {} {} · {} {}",
                        bytes(number(&saves["bytes"])),
                        number(&saves["files"]),
                        self.tr("Dateien", "files"),
                        number(&saves["backups"]),
                        self.tr(
                            if saves["backups"] == 1 {
                                "Backup"
                            } else {
                                "Backups"
                            },
                            if saves["backups"] == 1 {
                                "backup"
                            } else {
                                "backups"
                            }
                        )
                    ),
                    13.0,
                    Weight::Normal,
                    MUTED
                )
            ]
            .spacing(10)
        ]
        .spacing(20)
        .into()
    }
    pub(crate) fn automatic_saves(&self) -> Element<'_, Message> {
        let cloud = &self.status()["cloud"];
        if !yes(cloud, "enabled") {
            return Column::new().into();
        }
        let mut content=column![label(self.tr("Flightdeck gleicht deine Xbox-Spielstände vor dem Start und nach dem Beenden ab. Vor Änderungen bleibt eine lokale Sicherung erhalten.","Flightdeck syncs your Xbox saves before starting and after exiting. A local backup is kept before changes."),14.0,Weight::Normal,MUTED)].spacing(12);
        for key in ["message", "error"] {
            if !s(cloud, key).is_empty() {
                content = content.push(self.paragraph(s(cloud, key)));
            }
        }
        if yes(cloud, "conflict") {
            content=content.push(self.paragraph(self.tr("Lokal und in der Cloud gibt es unterschiedliche Änderungen. Wähle den Stand, den du behalten möchtest.","Local and cloud saves have changed. Choose the version you want to keep."))).push(self.pairs(&cloud["summary"],&[("add_count","Neu"),("replace_count","Ersetzen"),("delete_count","Löschen"),("conflict_count","Konflikte")]))
        }
        let mut buttons = Column::new().spacing(10);
        let mut has_buttons = false;
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
                has_buttons = true;
            }
        }
        if has_buttons {
            content = content.push(buttons);
        }
        column![
            label(
                self.t("Automatisch vor und nach dem Spielen").to_string(),
                20.0,
                Weight::Semibold,
                INK
            ),
            content
        ]
        .spacing(12)
        .into()
    }
    fn update_panel<'a>(
        &self,
        title: &str,
        icon_name: &str,
        status: &str,
        data: &Value,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let version = |key, fallback| {
            let value = s(data, key);
            if value.is_empty() {
                self.t(fallback).to_string()
            } else {
                value.to_string()
            }
        };
        let metric = |title: &str, value: String| {
            container(
                column![
                    label(self.t(title).to_string(), 13.0, Weight::Normal, MUTED),
                    label(value, 22.0, Weight::Semibold, INK)
                ]
                .spacing(6),
            )
            .padding(18)
            .width(Length::Fill)
            .style(|_| {
                container::Style::default().background(BG).border(Border {
                    color: LINE,
                    width: 1.0,
                    radius: 10.0.into(),
                })
            })
        };
        container(
            column![
                row![
                    icon(icon_name, 34.0, self.edition.accent()),
                    column![
                        label(title.to_string(), 13.0, Weight::Semibold, MUTED),
                        label(status.to_string(), 26.0, Weight::Semibold, INK)
                            .weight(650)
                            .tracking(-0.6)
                    ]
                    .spacing(5)
                ]
                .spacing(18)
                .align_y(alignment::Vertical::Center),
                row![
                    metric(
                        "Installierte Version",
                        version("installed_version", "Nicht bekannt")
                    ),
                    metric(
                        "Verfügbare Version",
                        version("latest_version", "Noch nicht geprüft")
                    )
                ]
                .spacing(16),
                content.into()
            ]
            .spacing(22),
        )
        .padding(26)
        .width(Length::Fill)
        .style(card_style)
        .into()
    }
    fn updates_page(&self) -> Element<'_, Message> {
        let launcher = self.data("launcher-update");
        let game = self.data("game-update");
        let mut launcher_actions = vec![];
        if !active(&launcher["job"]) && !yes(launcher, "pending_restart") {
            launcher_actions.push(("Nach Updates suchen", Launcher("check")));
        }
        for (title, op, visible) in [
            (
                "Herunterladen & installieren",
                "install",
                yes(launcher, "can_install"),
            ),
            (
                "Flightdeck jetzt neu starten",
                "restart",
                yes(launcher, "pending_restart"),
            ),
            ("Abbrechen", "cancel", active(&launcher["job"])),
        ] {
            if visible {
                launcher_actions.push((title, Launcher(op)));
            }
        }
        let mut launcher_view=column![self.paragraph(self.t("Lade neue Flightdeck-Versionen direkt von GitHub. Deine Einstellungen bleiben erhalten.").to_string()),self.job("launcher-update"),self.actions(&launcher_actions)].spacing(16);
        for key in ["unavailable_reason"] {
            if !s(launcher, key).is_empty() {
                launcher_view = launcher_view.push(self.paragraph(s(launcher, key)));
            }
        }
        if !s(launcher, "notes").is_empty() {
            launcher_view = launcher_view
                .push(self.release_notes(s(launcher, "notes"), s(launcher, "release_url")));
        }
        if yes(launcher, "can_rollback") {
            launcher_view = launcher_view.push(self.disclosure(
                Disclosure::LauncherRollback,
                "Vorherige Launcher-Version",
                "",
                self.action("Vorherige Version wiederherstellen", Launcher("rollback")),
            ));
        }
        let launcher_title = if yes(launcher, "update_available")
            && !active(&launcher["job"])
            && !yes(launcher, "pending_restart")
            && launcher["job"]["state"] != "failed"
        {
            self.tr(
                "Flightdeck {version} ist verfügbar",
                "Flightdeck {version} is available",
            )
            .replace("{version}", s(launcher, "latest_version"))
        } else {
            self.t(presentation::launcher_update(launcher)).into()
        };
        let mut game_actions = vec![];
        if !active(&game["job"]) {
            game_actions.push(if yes(game, "auth_required") {
                ("Mit Microsoft anmelden und prüfen", Game("sign-in"))
            } else {
                ("Nach Updates suchen", Game("check"))
            });
        }
        if game["job"]["state"] == "ready" && yes(game, "can_start") {
            game_actions.push(("Update herunterladen", Game("start")));
        }
        for (title, op) in [
            ("Download pausieren", "pause"),
            ("Download fortsetzen", "resume"),
            ("Abbrechen", "cancel"),
        ] {
            if self.request(&SetupControl(op)).is_some() {
                game_actions.push((title, SetupControl(op)));
            }
        }
        let message = if game["available"] == false {
            s(game, "unavailable_reason")
        } else {
            self.t("Die Prüfung lädt kein Update herunter. Du startest den Download anschließend selbst.")
        };
        let game_view = column![
            self.paragraph(message),
            self.job("game-update"),
            row![self.actions(&game_actions), self.refresh_button()]
                .spacing(10)
                .wrap()
        ]
        .spacing(16);
        let integrity = &game["integrity"];
        let integrity_status = if integrity["available"] == false {
            s(integrity, "unavailable_reason")
        } else if integrity["result"].is_object() {
            self.t(if yes(&integrity["result"], "healthy") {
                "Keine Abweichungen gefunden."
            } else {
                "Die Dateiprüfung hat Abweichungen gefunden."
            })
        } else {
            self.t("Noch keine Dateiprüfung durchgeführt.")
        };
        let integrity_view = column![
            self.paragraph(integrity_status),
            self.pairs(
                &integrity["result"],
                &[
                    ("checked", "Geprüft"),
                    ("missing", "Fehlend"),
                    ("changed", "Verändert"),
                    ("unreadable", "Nicht lesbar"),
                    ("total", "Gesamt")
                ]
            ),
            self.actions(&[
                ("Dateien prüfen", Game("verify")),
                ("Reparatur vorbereiten", Game("repair"))
            ])
        ]
        .spacing(16);
        let mut page = column![
            self.heading(self.tr(
                "Updates für Flightdeck & Simulator",
                "Flightdeck & simulator updates"
            )),
            self.paragraph(
                self.t("Aktualisiere Flightdeck und deinen Simulator direkt hier im Launcher.")
                    .to_string()
            ),
            self.update_panel(
                self.t("Flightdeck · GitHub-Releases"),
                "refresh",
                &launcher_title,
                launcher,
                launcher_view
            ),
            row![
                column![
                    label(
                        self.t("Fenix Linux-Patch").to_string(),
                        16.0,
                        Weight::Semibold,
                        INK
                    ),
                    self.paragraph(
                        self.t(
                            "Den passenden Patch lädt Flightdeck unter Mods automatisch von GitHub."
                        )
                        .to_string()
                    )
                ]
                .spacing(5)
                .width(Length::Fill),
                self.control(
                    "Fenix-Patch verwalten",
                    Some(Message::Navigate(Page::Mods)),
                    false
                )
            ]
            .spacing(20)
            .align_y(alignment::Vertical::Center),
            self.update_panel(
                &self.edition.name(),
                "download",
                self.t(presentation::game_update(game)),
                game,
                game_view
            ),
            self.card("Spieldateien", integrity_view)
        ]
        .spacing(20);
        if yes(game, "can_rollback") {
            page = page.push(self.action("Vorherige Version wiederherstellen", Game("rollback")));
        }
        page.into()
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
        let saves = &self.status()["saves"];
        let metric = |title, value| {
            column![
                label(self.t(title).to_string(), 14.0, Weight::Normal, MUTED),
                label(value, 25.0, Weight::Normal, INK)
            ]
            .spacing(8)
            .width(Length::Fill)
        };
        let metrics = container(
            row![
                metric("Speicherplatz", bytes(number(&saves["bytes"]))),
                rule::vertical(1),
                metric("Dateien", number(&saves["files"]).to_string()),
                rule::vertical(1),
                metric("Backups", number(&saves["backups"]).to_string())
            ]
            .spacing(28)
            .height(64),
        )
        .padding(28)
        .width(Length::Fill)
        .style(card_style);
        let backup = container(
            row![
                column![
                    self.heading(self.t("Lokales Backup").to_string()),
                    self.paragraph(
                        self.t("Lege eine zusätzliche lokale Kopie deiner Spielstände an.")
                            .to_string()
                    ),
                    label(
                        self.t(if yes(saves, "can_backup") {
                            "Du kannst jetzt ein Backup anlegen."
                        } else {
                            "Beende zuerst das Spiel und laufende Einrichtungsvorgänge."
                        })
                        .to_string(),
                        14.0,
                        Weight::Normal,
                        MUTED
                    )
                ]
                .spacing(8)
                .width(Length::Fill),
                self.action("Backup erstellen", Backup)
            ]
            .spacing(20)
            .align_y(alignment::Vertical::Center),
        )
        .padding(28)
        .width(Length::Fill)
        .style(card_style);
        column![
            self.heading(self.t("Deine Spielstände").to_string()),
            self.paragraph(self.t("Deine Spielstände werden automatisch abgeglichen. Lokale Sicherungen bleiben auf diesem Rechner.").to_string()),
            container(self.automatic_saves()).padding(26).width(Length::Fill).style(card_style),
            metrics,backup,
            self.pairs(&saves["last_backup"],&[("created_at","Letztes Backup"),("name","Datei")]),
            self.card("Backups bleiben lokal.",self.paragraph(self.t("MSFS speichert während des Spiels lokal. Flightdeck gleicht vor dem Start und nach dem Beenden mit der Xbox-Cloud ab. Zusätzliche Backups bleiben auf diesem Rechner.").to_string())),
            self.disclosure(
                Disclosure::Cloud,
                "Erweiterte Spielstandwerkzeuge",
                "",
                self.card("Xbox-Cloud-Spielstände", manual)
            )
        ]
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
        let diagnostics = column![
            row![
                self.control(
                    "Aktualisieren",
                    (!self.pending).then_some(Message::Refresh),
                    false
                ),
                self.control(
                    "Diagnose kopieren",
                    self.diagnostics_text()
                        .is_some()
                        .then_some(Message::CopyDiagnostics),
                    false
                ),
                self.control(
                    "Diagnose speichern",
                    (!self.exporting && self.diagnostics_text().is_some())
                        .then_some(Message::SaveDiagnostics),
                    false
                )
            ]
            .spacing(12)
            .wrap(),
            self.job("diagnostics"),
            self.checks(&data["checks"]),
            self.disclosure(
                Disclosure::Diagnostics,
                "Details anzeigen",
                "",
                self.diagnostic_summary(&data["summary"])
            )
        ]
        .spacing(16);
        let mut store_actions = vec![
            ("Store-Prüfung starten", Store("start")),
            ("Microsoft-Anmeldung erneuern", Store("sign-in")),
        ];
        if active(&store["job"]) {
            store_actions.push(("Abbrechen", Store("cancel")));
        }
        let mut store_view=column![
            self.paragraph(self.t("Prüft Anmeldung, Produktkatalog, Spiellizenz, Bibliothek und die Anzeige des Store-Fensters. Dabei wird keine Kaufseite geöffnet und kein Kauf ausgeführt.").to_string()),
            self.paragraph(self.t("Abgelaufene Tickets werden automatisch erneuert. Falls Microsoft eine neue Anmeldung verlangt, beende den Simulator und melde dich hier erneut an.").to_string()),
            self.actions(&store_actions),self.job("store-check")].spacing(16);
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
        let mut report_card=column![self.paragraph(self.t("Beschreibe den Fehler. Flightdeck ergänzt bereinigte Diagnosedaten für einen E-Mail-Bericht. Du brauchst kein zusätzliches Konto.").to_string()),self.control("Fehlerbericht erstellen",Some(Message::Toggle(Disclosure::Report)),false)].spacing(16);
        if self.expanded.contains(&Disclosure::Report) || self.report_text().is_some() {
            report_card = report_card.push(report);
        }
        column![
            self.heading(self.t("Einblick ohne private Rohlogs").to_string()),
            self.paragraph(self.t("Dieser Bericht enthält die vom lokalen Dienst freigegebenen Status- und Prüfdaten. Er enthält keine Anmeldetokens oder privaten Spielprotokolle.").to_string()),
            self.card("Problem melden",report_card),
            self.card("Store prüfen", store_view),
            diagnostics
        ]
        .spacing(20)
        .into()
    }
}
