use crate::*;
use iced::widget::column;
use model::Action;

#[derive(Clone, Debug, PartialEq, Eq)]
struct InstallationChoice {
    id: String,
    label: String,
}
impl std::fmt::Display for InstallationChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
impl App {
    pub(crate) fn is_steam(&self) -> bool {
        self.status()["runtime"]["source"] == "steam"
    }
    pub(crate) fn installation_picker(&self) -> Element<'_, Message> {
        let data = self.data("installations");
        let rows = data["installations"].as_array();
        let mut choices = Vec::new();
        for item in rows.into_iter().flatten() {
            let name = s(item, "name");
            let duplicate =
                rows.is_some_and(|rows| rows.iter().filter(|i| s(i, "name") == name).count() > 1);
            choices.push(InstallationChoice {
                id: s(item, "id").into(),
                label: if duplicate {
                    format!("{name} · {}", s(item, "path"))
                } else {
                    name.into()
                },
            });
        }
        if choices.is_empty() {
            return Space::new().height(0).into();
        }
        let selected = choices
            .iter()
            .find(|i| Some(i.id.as_str()) == data["selected_id"].as_str())
            .cloned();
        let title = label(
            self.tr("Installation", "Installation"),
            14.0,
            Weight::Semibold,
            MUTED,
        );
        let selector: Element<'_, Message> =
            if self.fresh("installations") && self.idle() && yes(data, "can_select") {
                pick_list(choices, selected, |item| {
                    Message::Action(Action::SelectInstallation(item.id))
                })
                .placeholder(self.tr("Installation auswählen", "Select installation"))
                .width(Length::Fill)
                .padding(12)
                .into()
            } else {
                label(
                    selected.map(|i| i.label).unwrap_or_default(),
                    15.0,
                    Weight::Normal,
                    MUTED,
                )
                .into()
            };
        container(column![title, selector].spacing(8))
            .padding(16)
            .width(Length::Fill)
            .style(card_style)
            .into()
    }
    pub(crate) fn installations_card(&self) -> Element<'_, Message> {
        self.card(self.tr("Xbox- und Steam-Installationen","Xbox and Steam installations"),column![
            self.paragraph(self.tr("Vorhandene Installationen werden automatisch gefunden. Steam-Spiele und bereits eingerichtete Fenix-Patches können direkt ausgewählt werden.","Existing installations are detected automatically. Select a Steam game to use it and manage an existing Fenix patch.")),
            self.installation_picker(),
            self.control(self.tr("Installationen erneut suchen","Scan installations again"),(!self.pending).then_some(Message::Refresh),false),
            self.disclosure(Disclosure::Installations,self.tr("Steam-Ordner manuell angeben","Choose Steam directory manually"),"",column![
                self.input(self.tr("Weiterer Steam-Ordner","Additional Steam directory"),"steam_root","/…/Steam",None),
                self.options(self.tr("Steam-Client","Steam client"),"steam_client",&[("native","Steam"),("flatpak","Steam (Flatpak)"),("snap","Steam (Snap)")]),
                self.control(self.tr("Steam-Ordner hinzufügen","Add Steam directory"),self.request(&Action::AddSteamRoot).map(|_|Message::Action(Action::AddSteamRoot)),false)
            ].spacing(14)),
            self.control(self.tr("Xbox-Version einrichten","Set up Xbox edition"),self.request(&Action::XboxSetup).map(|_|Message::Action(Action::XboxSetup)),false)
        ].spacing(14))
    }
    pub(crate) fn steam_fenix_card(&self) -> Element<'_, Message> {
        let raw = self.data("fenix");
        let current = self.fresh("fenix") && s(raw, "runtime_path") == self.runtime();
        let data = if current { raw } else { &Value::Null };
        let mut content=column![self.paragraph(self.tr("Verwaltet den Fenix-Linux-Patch für diese Steam-Installation. Vorhandene Patches werden erkannt; Flugzeuge, Anmeldung und Einstellungen bleiben erhalten.","Manage the Fenix Linux patch for this Steam installation. Existing patches are recognized; aircraft, sign-in and settings are retained."))].spacing(14);
        content = content.push(self.addon_feedback(data));
        if crate::addons::fenix_hook_failure(data) == Some("repair") {
            content=content.push(self.paragraph(self.tr("Die letzte Reparatur ist fehlgeschlagen. Prüfe den Diagnosebericht, bevor du einen weiteren Versuch startest.","The last repair failed. Review the diagnostics before trying again.")))
                .push(self.control("Diagnose öffnen",Some(Message::Navigate(Page::Diagnostics)),true));
        }
        if !current {
            content = content.push(self.paragraph(self.tr(
                "Lade den Status neu, bevor du die Einrichtung änderst.",
                "Refresh the status before changing setup.",
            )));
        }
        if yes(data, "needs_first_launch") || yes(&self.status()["runtime"], "needs_first_launch") {
            content=content.push(self.paragraph(self.tr("Starte MSFS einmal über Steam und beende es wieder. Danach kann der Fenix-Patch eingerichtet werden.","Start MSFS once through Steam, then close it. Fenix patch setup becomes available afterwards.")));
        }
        if yes(data, "steam_running") {
            content=content.push(self.paragraph(self.tr("Beende Steam vollständig, bevor du den Patch, seine Einstellungen oder die Proton-Zuordnung änderst. Die Fenix-App kann weiterhin geöffnet werden.","Quit Steam completely before changing the patch, its settings or the Proton selection. You can still open the Fenix app.")));
        }
        if !s(data, "message").is_empty() {
            content = content.push(self.paragraph(s(data, "message")));
        }
        if yes(data, "installed") && data["tool_selected"] != true {
            content = content.push(
                self.control(
                    self.tr(
                        "Fenix-Proton für Steam auswählen",
                        "Select Fenix Proton for Steam",
                    ),
                    self.request(&Action::Fenix("select"))
                        .map(|_| Message::Action(Action::Fenix("select"))),
                    true,
                ),
            );
        }
        for (de, en, op, show) in [
            (
                "Fenix-Patch einrichten",
                "Set up Fenix patch",
                "install",
                !yes(data, "installed"),
            ),
            (
                "Fenix-Patch aktualisieren",
                "Update Fenix patch",
                "install",
                yes(data, "installed") && yes(data, "update_available"),
            ),
            (
                "Fenix-App öffnen",
                "Open Fenix app",
                "manager",
                yes(data, "manager_installed"),
            ),
            (
                "Fenix-App reparieren",
                "Repair Fenix app",
                "repair",
                yes(data, "can_repair_installer"),
            ),
            (
                "Fenix öffnen",
                "Open Fenix",
                "open",
                yes(data, "fenix_installed"),
            ),
            (
                "Anzeigen und Autostart einrichten",
                "Configure displays and autostart",
                "configure",
                yes(data, "settings_ready"),
            ),
            ("Fenix beenden", "Stop Fenix", "stop", yes(data, "can_stop")),
        ] {
            if show {
                let action = Action::Fenix(op);
                content = content.push(self.control(
                    self.tr(de, en),
                    self.request(&action).map(|_| Message::Action(action)),
                    false,
                ));
            }
        }
        if yes(data, "installed") {
            content = content
                .push(self.input(
                    self.tr("Offizieller Fenix-Installer", "Official Fenix installer"),
                    "installer_path",
                    "/…/FenixInstaller.exe",
                    Some(Action::FenixPick("installer")),
                ))
                .push(
                    self.control(
                        self.tr("Fenix-Installer starten", "Run Fenix installer"),
                        self.request(&Action::Fenix("installer"))
                            .map(|_| Message::Action(Action::Fenix("installer"))),
                        false,
                    ),
                );
        }
        content = content
            .push(self.fenix_update_controls(data))
            .push(self.job("fenix"));
        if yes(data, "can_cancel") && !yes(data, "can_stop") {
            content = content.push(self.action("Abbrechen", Action::Fenix("cancel")));
        }
        if !s(&data["job"], "detail").is_empty() {
            content = content.push(self.paragraph(s(&data["job"], "detail")));
        }
        if yes(data, "can_restore") {
            content = content
                .push(self.action("Originalprofil wiederherstellen", Action::Fenix("restore")));
        }
        self.card("Fenix Linux-Patch", content)
    }
    pub(crate) fn steam_page(&self) -> Element<'_, Message> {
        let about=self.card(self.tr("Steam-Installation","Steam installation"),column![
            self.paragraph(s(&self.status()["runtime"],"installation_name")),
            self.paragraph(self.tr("Steam übernimmt Installation, Spielupdates und seine Synchronisierung. Flightdeck startet den passenden Steam-Client und verwaltet den Fenix-Patch.","Steam handles installation, game updates and its synchronization. Flightdeck launches the matching Steam client and manages the Fenix patch.")),
            self.paragraph(s(&self.status()["game"],"message")),
            self.control(self.tr("In Steam öffnen","Open in Steam"),self.request(&Action::SteamOpen).map(|_|Message::Action(Action::SteamOpen)),false),
            self.paragraph(self.tr("Steam-Unterstützung: Beta. Gib bei Problemen die MSFS-Ausgabe, den Steam-Client, deine Distribution und die Grafikkarte an.","Steam support: beta. When reporting a problem, include the MSFS edition, Steam client, distribution and GPU."))
        ].spacing(14));
        let mut page = column![about].spacing(20);
        match self.page {
            Page::Setup => {
                page = page
                    .push(self.installations_card())
                    .push(self.steam_fenix_card());
            }
            Page::Mods => {
                page = page
                    .push(self.steam_fenix_card())
                    .push(self.community_card());
            }
            Page::Overview => {
                let data = self.data("fenix");
                let mut fenix = column![
                    self.addon_feedback(data),
                    self.paragraph(if yes(data, "configured") {
                        self.tr("Fenix lokal eingerichtet", "Fenix locally configured")
                    } else {
                        self.tr(
                            "Einrichtung noch nicht abgeschlossen",
                            "Setup is not complete yet",
                        )
                    })
                ]
                .spacing(14);
                if crate::addons::fenix_hook_failure(data).is_some() {
                    fenix=fenix.push(self.paragraph(self.tr("Der letzte Fenix-Einrichtungsschritt ist fehlgeschlagen. Öffne Mods für den Status und die Diagnose.","The last Fenix setup step failed. Open Mods for its status and diagnostics.")));
                }
                fenix = fenix.push(self.control(
                    self.tr("Fenix verwalten", "Manage Fenix"),
                    Some(Message::Navigate(Page::Mods)),
                    true,
                ));
                if yes(data, "can_stop") {
                    fenix = fenix.push(self.action("Fenix beenden", Action::Fenix("stop")));
                }
                page = page.push(self.card("Fenix A320", fenix));
            }
            Page::Saves => {
                page=page.push(self.paragraph(self.tr("Flightdecks Xbox-Cloud-Abgleich wird für diese Steam-Installation nicht verwendet. Wiederherstellungspunkte des Fenix-Patches findest du unter Mods.","Flightdeck's Xbox cloud synchronization is not used for this Steam installation. Fenix patch restore points are available under Mods.")));
            }
            Page::Diagnostics => {
                page = page
                    .push(self.diagnostic_summary(&self.data("diagnostics")["summary"]))
                    .push(self.pairs(
                        &self.status()["runtime"],
                        &[
                            ("game_path", "Spielordner"),
                            ("prefix", "Windows-Profil"),
                            ("installation_id", "Installation"),
                        ],
                    ))
                    .push(self.steam_fenix_card());
            }
            _ => {}
        }
        page.into()
    }
}
