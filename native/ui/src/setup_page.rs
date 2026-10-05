use crate::*;
use iced::widget::column;
use model::Action::*;

impl App {
    fn region_picker(&self) -> Element<'_, Message> {
        static REGIONS: LazyLock<std::collections::BTreeMap<String, Vec<(String, String)>>> =
            LazyLock::new(|| {
                serde_json::from_str(include_str!("../regions.json"))
                    .expect("compiled ISO region names")
            });
        self.choices(
            "Store-Region",
            "market",
            REGIONS[self.language.code()].iter().cloned(),
        )
    }
    fn setup_mode(
        &self,
        mode: &'static str,
        title: &'static str,
        detail: &'static str,
    ) -> Element<'_, Message> {
        let selected = self.forms.get("mode") == mode;
        let accent = self.edition.accent();
        button(
            row![
                container(
                    container(Space::new())
                        .width(24)
                        .height(24)
                        .style(move |_| container::Style::default()
                            .background(if selected { accent } else { Color::TRANSPARENT })
                            .border(Border {
                                radius: 20.0.into(),
                                ..Default::default()
                            }))
                )
                .center(40)
                .style(move |_| container::Style::default().border(Border {
                    color: if selected { accent } else { MUTED },
                    width: 3.0,
                    radius: 22.0.into()
                })),
                column![
                    label(self.t(title), 20.0, Weight::Semibold, INK),
                    label(self.t(detail), 14.0, Weight::Normal, MUTED).line_height(1.5)
                ]
                .spacing(6)
                .width(Length::Fill)
            ]
            .spacing(22)
            .align_y(alignment::Vertical::Center),
        )
        .padding(28)
        .width(Length::Fill)
        .on_press_maybe(
            (self.online && self.can_edit_field("mode"))
                .then(|| Message::Field("mode", mode.into())),
        )
        .style(move |_, _| button::Style {
            background: Some(iced::color!(0x101f2b).into()),
            text_color: INK,
            border: Border {
                color: if selected { accent } else { LINE },
                width: if selected { 2.0 } else { 1.0 },
                radius: 12.0.into(),
            },
            ..Default::default()
        })
        .into()
    }
    fn setup_form(&self) -> Element<'_, Message> {
        let mode = self.forms.get("mode");
        let mut form = Column::new().spacing(18);
        if mode == "existing" {
            form = form
                .push(
                    self.paragraph(
                        self.t("Wähle den Ordner, in dem deine vorbereitete Runtime liegt.")
                            .to_string(),
                    ),
                )
                .push(
                    row![
                        label(
                            self.t("Gefundene Installationen").to_string(),
                            18.0,
                            Weight::Semibold,
                            INK
                        ),
                        Space::new().width(Length::Fill),
                        self.control(
                            "Installationen suchen",
                            (self.online && self.can_edit_field("runtime_path"))
                                .then_some(Message::Discover("setup/discover")),
                            false
                        )
                    ]
                    .spacing(12)
                    .align_y(alignment::Vertical::Center),
                );
            if let Some(discovered) = self.discoveries.get("setup/discover") {
                if !s(discovered, "_error").is_empty() {
                    form = form.push(self.paragraph(s(discovered, "_error").to_string()));
                }
                let count = discovered["runtimes"].as_array().map(Vec::len).unwrap_or(0);
                form = form.push(label(
                    format!(
                        "{count} {}",
                        self.tr(
                            if count == 1 {
                                "Installation gefunden."
                            } else {
                                "Installationen gefunden."
                            },
                            if count == 1 {
                                "installation found."
                            } else {
                                "installations found."
                            }
                        )
                    ),
                    14.0,
                    Weight::Normal,
                    MUTED,
                ));
                for item in discovered["runtimes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(40)
                {
                    if let Some(path) = item["path"].as_str() {
                        form = form.push(
                            button(
                                column![
                                    label(s(item, "name").to_string(), 14.0, Weight::Semibold, INK),
                                    label(path.to_string(), 13.0, Weight::Normal, MUTED)
                                ]
                                .spacing(5),
                            )
                            .padding(20)
                            .width(Length::Fill)
                            .style(|_, status| button::Style {
                                background: Some(
                                    if status == button::Status::Hovered {
                                        iced::color!(0x152a37)
                                    } else {
                                        BG
                                    }
                                    .into(),
                                ),
                                text_color: INK,
                                border: Border {
                                    color: LINE,
                                    width: 1.0,
                                    radius: 10.0.into(),
                                },
                                ..Default::default()
                            })
                            .on_press_maybe(
                                self.can_edit_field("runtime_path")
                                    .then(|| Message::Field("runtime_path", path.into())),
                            ),
                        );
                    }
                }
                if yes(discovered, "limited") {
                    form = form.push(self.paragraph(self.tr(
                        "Die Suche wurde begrenzt. Du kannst den Pfad direkt eingeben.",
                        "Search was limited. You can enter the path directly.",
                    )));
                }
            }
            form = form.push(self.input(
                "Installationsordner",
                "runtime_path",
                "/…",
                Some(Pick("runtime_path")),
            ));
        } else {
            form = form.push(self.options(
                "MSFS-Version",
                "game_id",
                &[
                    ("msfs2024", "Microsoft Flight Simulator 2024"),
                    ("msfs2020", "Microsoft Flight Simulator 2020"),
                ],
            ));
            if mode == "prepare" {
                for (field, title) in [
                    ("artifacts_path", "Gebautes Komponentenverzeichnis"),
                    ("game_path", "MSFS-Spielordner"),
                    ("runner_path", "Kompatibler Proton-Runner"),
                    ("prefix_path", "Vorbereiteter Wine-Prefix"),
                    ("media_plugins_path", "Medien-Plugins (optional)"),
                ] {
                    form = form.push(self.input(title, field, "/…", Some(Pick(field))));
                }
            } else {
                form = form.push(self.paragraph(self.t("Flightdeck bereitet die nötigen Komponenten automatisch vor. Dein Spiel wird erst nach der Anmeldung heruntergeladen.").to_string()));
            }
            form = form.push(self.region_picker())
                .push(self.paragraph(self.t("Wähle die Region deines Microsoft-Kontos.").to_string()))
                .push(self.input("Neuer Zielordner","destination_path",s(&self.data("setup")["defaults"],"destination_path"),Some(Pick("destination_path"))))
                .push(self.paragraph(self.t("Mindestens 100 GiB freier Speicherplatz. Vor dem Download prüfen wir deinen Rechner und diesen Speicherort.").to_string()));
        }
        form = form.push(
            self.disclosure(
                Disclosure::AdvancedSetup,
                "Erweiterte Einrichtung",
                "",
                column![
                    self.paragraph(
                        self.t(
                            "Für selbst gebaute Komponenten. Hier wird kein Spiel heruntergeladen."
                        )
                        .to_string()
                    ),
                    self.setup_mode(
                        "prepare",
                        "Neue Runtime vorbereiten",
                        "Eigene Spieldateien und Komponenten zusammenführen."
                    )
                ]
                .spacing(16),
            ),
        );
        let mut actions = vec![(
            if mode == "existing" {
                "Verbinden"
            } else {
                "Installation prüfen"
            },
            SetupCheck,
        )];
        if self.request(&SetupStart).is_some() {
            actions.push((
                if mode == "install" {
                    "Installation starten"
                } else {
                    "Runtime verbinden"
                },
                SetupStart,
            ));
        }
        for (title, op) in [
            ("Download pausieren", "pause"),
            ("Download fortsetzen", "resume"),
            ("Abbrechen", "cancel"),
        ] {
            if self.request(&SetupControl(op)).is_some() {
                actions.push((title, SetupControl(op)));
            }
        }
        form = form.push(self.job("setup")).push(self.actions(&actions));
        if self.data("setup")["job"]["state"] == "failed" {
            form = form.push(self.help_link(
                "Hilfe zu den Linux-Voraussetzungen öffnen",
                HelpLink::Install,
            ));
        }
        if self.data("setup")["job"]["state"] == "complete" {
            form = form.push(self.link(self.t("Zur Übersicht"), Page::Overview));
        }
        form = form.push(self.paragraph(format!(
            "{} {}",
            self.t("Aktuell verbunden:"),
            if self.runtime().is_empty() {
                self.t("Noch kein Ordner verbunden.")
            } else {
                self.runtime()
            }
        )));
        self.card(
            if mode == "install" {
                "MSFS installieren"
            } else {
                "Deine Installation"
            },
            form,
        )
    }
    pub(crate) fn setup_page(&self) -> Element<'_, Message> {
        let status = self.status();
        let mut page = column![label(
            self.t(
                "Installieren oder verbinden. Danach startet dein Simulator direkt aus Flightdeck."
            )
            .to_string(),
            22.0,
            Weight::Normal,
            MUTED
        )]
        .spacing(24);
        let busy = model::active(&self.data("setup")["job"])
            && self.data("setup")["job"]["mode"] != "update";
        if yes(&status["runtime"], "configured") && !busy {
            if yes(&status["graphics"], "available") {
                page = page.push(self.disclosure(Disclosure::Graphics,"NVIDIA-Grafik",s(&status["graphics"],"label"),column![
                    self.options("NVIDIA-Modus","graphics",&[("auto","Automatisch (Kompatibilität bevorzugen)"),("compatibility","Kompatibilität (ohne NVIDIA-Zusatzfunktionen)"),("features","NVIDIA-Funktionen (experimentell)")]),
                    self.action("Modus speichern",Graphics),self.paragraph(s(&status["graphics"],"description")),self.paragraph(s(&status["graphics"],"error")),
                    self.paragraph(self.t("Gilt für den ausgewählten Simulator ab dem nächsten Spielstart. Jederzeit umstellbar.").to_string())].spacing(14)));
            }
            if yes(&status["vr"], "available") {
                page = page.push(self.disclosure(Disclosure::Vr,"Virtual Reality",s(&status["vr"],"message"),column![
                    self.paragraph(self.t("Verbinde dein Headset mit WiVRn, SteamVR oder Monado. Flightdeck verbindet den Simulator mit deinem VR-System.").to_string()),
                    self.options("VR-System","vr",&[("off","Aus (Monitor)"),("auto","Aktives VR-System automatisch verwenden"),("wivrn","WiVRn (Quest / Pico)"),("steamvr","SteamVR"),("monado","Monado")]),
                    self.actions(&[("VR-Modus speichern",VrSave),("Headset prüfen",VrCheck)]),
                    self.paragraph(s(&status["vr"],"message")),self.paragraph(s(&status["vr"],"error")),self.paragraph(s(&status["vr"]["check"],"message")),
                    self.paragraph(self.t("Starte danach MSFS und wechsle im Simulator in den VR-Modus (standardmäßig Strg+Tab). Verbinde das Headset vor jedem Spielstart.").to_string())].spacing(14)));
            }
            page = page.push(self.proton_card()).push(self.maintenance_card());
        }
        let mut steps = iced::widget::Row::new();
        let job = &self.data("setup")["job"];
        let active = match s(job, "state") {
            "checking" => 2,
            "ready" | "installing" | "complete" => 3,
            _ => 1,
        };
        for (index, title) in [
            "Installation wählen",
            "Prüfen",
            if self.forms.get("mode") == "install" {
                "Installieren"
            } else {
                "Verbinden"
            },
        ]
        .into_iter()
        .enumerate()
        {
            let color = if active == index + 1 {
                self.edition.accent()
            } else {
                MUTED
            };
            steps = steps.push(
                column![
                    container(label(
                        (index + 1).to_string(),
                        20.0,
                        Weight::Semibold,
                        if active == index + 1 { BG } else { MUTED }
                    ))
                    .width(46)
                    .height(46)
                    .center_x(46)
                    .center_y(46)
                    .style(move |_| container::Style::default()
                        .background(if active == index + 1 { color } else { BG })
                        .border(Border {
                            color,
                            width: 3.0,
                            radius: 24.0.into()
                        })),
                    label(
                        self.t(title).to_string(),
                        16.0,
                        if active == index + 1 {
                            Weight::Semibold
                        } else {
                            Weight::Normal
                        },
                        if active == index + 1 { INK } else { MUTED }
                    )
                ]
                .spacing(10)
                .align_x(alignment::Horizontal::Center)
                .width(Length::Fill),
            );
        }
        let steps = stack![
            Space::new().width(Length::Fill).height(84),
            container(rule::horizontal(2).style(|_| rule::Style {
                color: LINE,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true
            }))
            .padding(Padding {
                top: 22.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0
            }),
            steps
        ];
        page = page.push(container(steps).padding([0,40])).push(responsive(|size| {
            let install = self.setup_mode("install","MSFS installieren","Mit deinem Microsoft-Konto anmelden und deine gekaufte Xbox-PC-Version herunterladen.");
            let existing = self.setup_mode("existing","Vorhandene Installation verbinden","Bereits installiert? Deinen Ordner auswählen und verbinden.");
            if size.width<760.0 { column![install,existing].spacing(16).into() } else { row![install,existing].spacing(18).into() }
        }));
        page.push(responsive(|size| {
            if size.width<1000.0 { return self.setup_form(); }
            let guide_row=|name,title,detail|row![icon(name,28.0,MUTED),column![label(self.t(title).to_string(),16.0,Weight::Semibold,INK),label(self.t(detail).to_string(),14.0,Weight::Normal,MUTED).line_height(1.5)].spacing(6).width(Length::Fill)].spacing(22);
            let guide=self.card("In drei Schritten",column![
                guide_row("gamepad","Deine Installation vorbereiten","Wähle die Neuinstallation oder verbinde einen vorhandenen Ordner."),separator(),
                guide_row("database","Verbinden","Bei der Neuinstallation meldest du dich mit deinem Microsoft-Konto an. Flightdeck lädt deine lizenzierte Spielversion."),separator(),
                guide_row("play","Simulator starten","Danach startest du deinen Simulator direkt aus Flightdeck.")].spacing(20));
            row![container(self.setup_form()).width(Length::FillPortion(23)),container(guide).width(Length::FillPortion(10))].spacing(22).into()
        })).into()
    }
    fn proton_card(&self) -> Element<'_, Message> {
        let data = self.data("proton");
        let mut choices = vec![("default".into(), "Flightdeck (Xodus, Standard)".into())];
        let discovered = self
            .discoveries
            .get("proton/discover")
            .unwrap_or(&Value::Null);
        for item in discovered["choices"].as_array().into_iter().flatten() {
            if let Some(path) = item["path"].as_str() {
                let suffix = if yes(data, "fenix") {
                    format!(
                        " · {}",
                        self.t(if yes(item, "fenix") {
                            "Fenix verfügbar"
                        } else {
                            "Fenix-Patch fehlt"
                        })
                    )
                } else {
                    String::new()
                };
                choices.push((
                    path.into(),
                    format!("{} · {}{suffix}", s(item, "label"), s(item, "version")),
                ));
            }
        }
        choices.push((
            "custom".into(),
            self.t("Anderen Proton-Ordner wählen").into(),
        ));
        let mut content = column![self.paragraph(self.t("Wähle die Proton-Version für deinen Simulator und deine Add-ons. Flightdeck übernimmt die aktuelle Installation beim Wechsel.").to_string()),
            self.pairs(data,&[("selected","Aktive Proton-Version:")]),self.choices("Installierte Proton-Version","proton",choices),
            self.control("Versionen neu suchen",self.can_edit_field("proton").then_some(Message::Discover("proton/discover")),false)].spacing(16);
        if self.forms.get("proton") == "custom" {
            content = content.push(self.input(
                "Proton-Ordner",
                "proton_path",
                "/…",
                Some(Pick("proton_path")),
            ));
        }
        content=content.push(self.paragraph(self.t("Mods, Anmeldungen und Einstellungen bleiben erhalten, auch bei der Rückkehr zu Flightdeck. Das vorherige Windows-Profil wird als Sicherung aufbewahrt.").to_string()));
        if yes(data, "fenix") {
            content=content.push(self.paragraph(self.t("Für Fenix werden Versionen mit passendem Kompatibilitätspatch angezeigt. Flightdeck richtet den Patch beim Wechsel automatisch ein.").to_string()));
        }
        let mut controls = vec![("Proton vorbereiten & verwenden", Proton)];
        if yes(data, "can_restore") {
            controls.push(("Zur Flightdeck-Umgebung zurückkehren", ProtonDefault));
        }
        if model::active(&data["job"]) {
            controls.push(("Abbrechen", ProtonCancel));
        }
        content = content
            .push(self.actions(&controls))
            .push(self.job("proton"))
            .push(self.paragraph(s(discovered, "_error").to_string()));
        self.disclosure(
            Disclosure::Proton,
            "Proton-Version (experimentell)",
            if s(data, "error").is_empty() {
                s(data, "selected")
            } else {
                s(data, "error")
            },
            content,
        )
    }
    fn maintenance_card(&self) -> Element<'_, Message> {
        let job = &self.data("maintenance")["job"];
        let mut controls = vec![("Spielumgebung zurücksetzen", Maintenance("reset"))];
        if yes(self.data("maintenance"), "can_restore") {
            controls.push((
                "Letztes Zurücksetzen rückgängig machen",
                Maintenance("restore"),
            ));
        }
        let mut content=column![self.paragraph(self.t("Bei Startproblemen kannst du die Spielumgebung zurücksetzen. Vor jeder Änderung zeigt Flightdeck eine Vorschau.").to_string()),self.actions(&controls),
            self.disclosure(Disclosure::Removal,"Spiel deinstallieren","",column![self.flag("Einstellungen und lokale Spielstände sichern","keep_data"),self.flag("Basisspiel und frühere Spielversionen löschen","delete_packages"),self.action("Deinstallation prüfen",Maintenance("uninstall"))].spacing(14)),
            self.job("maintenance")].spacing(14);
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
        self.disclosure(
            Disclosure::Maintenance,
            "Installation verwalten",
            "Wiederherstellen, zurücksetzen oder entfernen",
            content,
        )
    }
}
