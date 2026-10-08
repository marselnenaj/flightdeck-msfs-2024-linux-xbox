use crate::*;
use iced::widget::{column, progress_bar};
use model::Action::*;

impl App {
    fn addon_action<'a>(&self, title: &str, action: model::Action) -> Element<'a, Message> {
        let message = self.request(&action).map(|_| Message::Action(action));
        self.control(title, message, true)
    }

    fn addon_facts<'a>(&self, facts: &[(&str, bool)]) -> Element<'a, Message> {
        let mut items = row![].spacing(10);
        for (name, detected) in facts {
            items = items.push(
                container(label(
                    format!("{} {}", if *detected { "✓" } else { "–" }, name),
                    12.0,
                    Weight::Medium,
                    MUTED,
                ))
                .padding([6, 10])
                .style(|_| {
                    container::Style::default().background(BG).border(Border {
                        color: LINE,
                        width: 1.0,
                        radius: 6.0.into(),
                    })
                }),
            );
        }
        items.wrap().into()
    }

    // Only the current runtime's validated snapshot may supply feedback. Keep
    // the last error and live progress beside the next action; full checks are
    // available in the management section below.
    fn addon_feedback<'a>(&self, data: &Value) -> Element<'a, Message> {
        let mut content = column![].spacing(8);
        for key in ["message", "unavailable_reason", "error", "startup_error"] {
            if !s(data, key).is_empty() && !(key == "message" && data["state"] == "legacy") {
                content = content.push(self.paragraph(s(data, key).to_string()));
            }
        }
        let job = &data["job"];
        if job["state"] == "failed" || model::active(job) {
            if job["state"] == "failed" {
                content = content.push(label(
                    self.tr("Letzter Vorgang fehlgeschlagen", "Last operation failed"),
                    13.0,
                    Weight::Semibold,
                    iced::color!(0xff9199),
                ));
            }
            for key in ["message", "error"] {
                if !s(job, key).is_empty() && (key != "error" || job["error"] != job["message"]) {
                    content = content.push(self.paragraph(s(job, key).to_string()));
                }
            }
            if model::active(job) {
                let transfer = &job["transfer"];
                if !transfer.is_null() {
                    content = content.push(self.paragraph(format!(
                        "{} / {}",
                        model::bytes(model::number(&transfer["received_bytes"])),
                        if model::number(&transfer["total_bytes"]) > 0 {
                            model::bytes(model::number(&transfer["total_bytes"]))
                        } else {
                            "?".into()
                        }
                    )));
                }
                let progress = model::transfer_progress(transfer).or_else(|| {
                    job["progress"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .map(|v| v.clamp(0.0, 100.0) as f32)
                });
                if let Some(progress) = progress {
                    content = content.push(progress_bar(0.0..=100.0, progress));
                }
            }
        }
        content.into()
    }

    fn fenix_installer(&self, manager: bool) -> Element<'_, Message> {
        column![
            self.help_link(
                "Offiziellen Installer im Fenix-Konto herunterladen",
                HelpLink::FenixInstaller
            ),
            self.input(
                "Fenix-Installer-Datei",
                "installer_path",
                "/home/…/FenixInstaller.exe",
                Some(FenixPick("installer"))
            ),
            self.control(
                if manager {
                    "Installer erneut ausführen"
                } else {
                    "Installer starten"
                },
                self.request(&Fenix("installer"))
                    .map(|_| Message::Action(Fenix("installer"))),
                !manager,
            ),
        ]
        .spacing(12)
        .into()
    }

    fn fenix_card(&self) -> Element<'_, Message> {
        let raw = self.data("fenix");
        let current = self.fresh("fenix") && s(raw, "runtime_path") == self.runtime();
        let data = if current { raw } else { &Value::Null };
        let progress = presentation::fenix(data, self.status());
        let manager = yes(data, "manager_installed");
        let active = model::active(&data["job"]);
        let failed_hook = data["job"]["state"] == "failed"
            && ["installer", "repair"].contains(&s(&data["job"], "operation"))
            && yes(data, "can_repair_installer");
        let supported = !progress.steps.is_empty() || data["state"] == "legacy";
        let mut content = column![].spacing(14);
        if current {
            content = content.push(self.addon_feedback(data));
            if !progress.busy.is_empty() {
                content = content.push(self.paragraph(self.t(progress.busy).to_string()));
            }
            if active || yes(data, "fenix_running") {
                if progress.busy.is_empty() {
                    content = content.push(self.paragraph(self.t(progress.detail).to_string()));
                }
                if yes(data, "can_stop") {
                    content = content.push(self.addon_action("Fenix beenden", Fenix("stop")));
                }
            } else if supported {
                content = content.push(label(
                    self.tr(
                        if progress.ready && !failed_hook {
                            "Einrichtung abgeschlossen"
                        } else {
                            "Als Nächstes"
                        },
                        if progress.ready && !failed_hook {
                            "Setup complete"
                        } else {
                            "Next step"
                        },
                    ),
                    13.0,
                    Weight::Semibold,
                    self.edition.accent(),
                ));
                if failed_hook {
                    content = content
                        .push(self.paragraph(self.t("Führt den Einrichtungsschritt der vorhandenen Fenix-App erneut aus. Dein Flugzeug wird dabei nicht installiert.").to_string()))
                        .push(self.addon_action("Fenix-App reparieren", Fenix("repair")));
                } else {
                    content = content.push(self.paragraph(self.t(progress.detail).to_string()));
                    if progress.ready {
                        content = content.push(self.control(
                            "Zur Übersicht und MSFS starten",
                            Some(Message::Navigate(Page::Overview)),
                            true,
                        ));
                    } else if data["state"] == "legacy" {
                        if yes(data, "fenix_installed") {
                            content =
                                content.push(self.addon_action("Fenix öffnen", Fenix("open")));
                        } else if manager {
                            content = content
                                .push(self.addon_action("Fenix-App öffnen", Fenix("manager")));
                        }
                    } else {
                        content = match progress.next {
                            1 => content.push(self.addon_action(
                                if yes(data, "can_retry") {
                                    "Einrichtung reparieren"
                                } else {
                                    "Patch einrichten"
                                },
                                Fenix("install"),
                            )),
                            2 if manager => content
                                .push(self.addon_action("Fenix-App öffnen", Fenix("manager"))),
                            2 => content.push(self.fenix_installer(false)),
                            3 => content.push(self.addon_action("Fenix öffnen", Fenix("open"))),
                            _ => content.push(
                                self.addon_action("Einrichtung abschließen", Fenix("configure")),
                            ),
                        };
                    }
                }
            } else if yes(data, "can_restore") {
                content = content
                    .push(self.paragraph(self.t(progress.detail).to_string()))
                    .push(self.addon_action("Patch rückgängig machen", Fenix("restore")));
            }
            if supported {
                content = content.push(self.addon_facts(&[
                    (
                        self.tr("Linux-Patch", "Linux patch"),
                        yes(data, "installed") || data["state"] == "legacy",
                    ),
                    (self.tr("Fenix-App", "Fenix app"), manager),
                    (
                        self.tr("Fenix-Dateien", "Fenix files"),
                        yes(data, "fenix_installed"),
                    ),
                    (
                        self.tr("Anzeigen & Autostart", "Displays & auto-start"),
                        yes(data, "configured"),
                    ),
                ]));
            }
            content = content.push(self.paragraph(self.tr("MSFS 2024 · CPU-Anzeigen ohne Wetterradar. Eine eigene Fenix-Lizenz ist erforderlich.", "MSFS 2024 · CPU displays without weather radar. Requires your own Fenix license.")));
            let mut manage = column![].spacing(14);
            let mut actions = vec![];
            if manager && !(progress.next == 2 && !failed_hook && !active) {
                actions.push(("Installer & Liveries", Fenix("manager")));
            }
            if yes(data, "fenix_installed") && progress.ready {
                actions.push(("Fenix öffnen", Fenix("open")));
            }
            if yes(data, "can_repair_installer") && !failed_hook {
                actions.push(("Fenix-App reparieren", Fenix("repair")));
            }
            if yes(data, "configured") {
                actions.push(("Einstellungen erneut anwenden", Fenix("configure")));
            }
            if yes(data, "update_available") {
                actions.push(("Patch aktualisieren", Fenix("install")));
            }
            for (title, action) in actions {
                let message = self.request(&action).map(|_| Message::Action(action));
                manage = manage.push(self.control(title, message, false));
            }
            let installer_is_next =
                progress.next == 2 && !manager && !active && !yes(data, "fenix_running");
            if yes(data, "installed") && !installer_is_next {
                manage = manage
                    .push(self.paragraph(
                        self.tr("Fenix-App neu installieren", "Reinstall the Fenix app"),
                    ))
                    .push(self.fenix_installer(true));
            }
            manage = manage.push(self.job("fenix"))
                .push(self.disclosure(Disclosure::FenixAdvanced, "Lokales Patch-Paket und Wiederherstellung", "", column![
                    self.input("Entpacktes Release (leer = geprüfter Download)", "bundle_path", self.tr("Automatisch herunterladen", "Download automatically"), Some(FenixPick("bundle"))),
                    self.paragraph(self.t("Die Wiederherstellung verwendet das Profil von vor dem Patch. Das neuere Profil bleibt als Sicherung erhalten.").to_string()),
                    self.action("Patch rückgängig machen", Fenix("restore")),
                ].spacing(14)))
                .push(self.help_link("Projekt und Anleitung", HelpLink::FenixProject));
            content = content.push(self.disclosure(
                Disclosure::FenixManage,
                self.tr("Verwalten & reparieren", "Manage & repair"),
                "",
                manage,
            ));
        } else {
            content = content.push(self.paragraph(self.tr("Der Status dieser Installation ist noch nicht verfügbar. Lade ihn erneut, bevor du die Einrichtung fortsetzt.", "The status of this installation is not available yet. Refresh before continuing setup.")));
        }
        content = content.push(self.refresh_button());
        self.disclosure(Disclosure::Fenix, "Fenix A320", progress.title, content)
    }

    fn gsx_card(&self) -> Element<'_, Message> {
        let raw = self.data("gsx");
        let current = self.fresh("gsx") && s(raw, "runtime_path") == self.runtime();
        let data = if current { raw } else { &Value::Null };
        let progress = presentation::gsx(data);
        let active = model::active(&data["job"]);
        let mut content = column![
            label(self.tr("Funktion im Simulator unbestätigt", "Simulator functionality unconfirmed"), 15.0, Weight::Semibold, iced::color!(0xf5c873)),
            self.paragraph(self.tr("Flightdeck kann FSDT vorbereiten und den Autostart einrichten. Lizenz, GSX-Menü und Bodendienste sind unter Linux noch nicht verifiziert.", "Flightdeck can prepare FSDT and configure auto-start. Licensing, the GSX menu and ground services have not been verified on Linux.")),
        ].spacing(14);
        if current {
            content = content.push(self.addon_feedback(data));
            content = content.push(self.paragraph(self.t(progress.detail).to_string()));
            if active || yes(data, "manager_running") {
                if yes(data, "can_stop") {
                    content = content.push(self.addon_action("FSDT schließen", Gsx("stop")));
                }
            } else if yes(data, "can_recover") {
                content = content
                    .push(self.addon_action("GSX-Vorbereitung wiederherstellen", Gsx("recover")));
            } else if data["state"] == "available" {
                let (title, action) = match progress.next {
                    1 => ("FSDT vorbereiten", Gsx("prepare")),
                    3 if yes(data, "startup_found") => {
                        ("Automatischen Start einrichten", Gsx("configure"))
                    }
                    _ => ("FSDT-Installer öffnen", Gsx("open")),
                };
                if data["idle"] == false
                    || yes(data, "busy")
                    || !["", "stopped"].contains(&s(&self.status()["game"], "state"))
                {
                    content = content.push(self.paragraph(self.tr(
                        "Beende MSFS oder die laufende Einrichtung, bevor du GSX änderst.",
                        "Close MSFS or the running setup before making changes to GSX.",
                    )));
                }
                if progress.ready {
                    content = content.push(self.control(
                        "Zur Übersicht und MSFS starten",
                        Some(Message::Navigate(Page::Overview)),
                        true,
                    ));
                } else {
                    content = content.push(self.addon_action(title, action));
                }
            }
            if data["state"] == "available" {
                content = content.push(self.addon_facts(&[
                    (
                        self.tr("FSDT vorbereitet", "FSDT prepared"),
                        yes(data, "prepared"),
                    ),
                    (
                        self.tr("GSX-Dateien", "GSX files"),
                        yes(data, "package_installed"),
                    ),
                    (
                        self.tr("Autostart eingerichtet", "Auto-start configured"),
                        yes(data, "configured"),
                    ),
                ]));
            }
            let mut manage = column![].spacing(14);
            if yes(data, "prepared") {
                if progress.ready || active || (progress.next == 3 && yes(data, "startup_found")) {
                    manage = manage.push(self.action("FSDT-Installer öffnen", Gsx("open")));
                }
                manage = manage.push(
                    self.control(
                        "FSDT-Installer reparieren",
                        self.request(&Gsx("prepare"))
                            .map(|_| Message::Action(Gsx("prepare"))),
                        false,
                    ),
                );
            }
            if yes(data, "configured") {
                manage = manage.push(self.action("GSX-Autostart ausschalten", Gsx("disable")));
            }
            manage = manage
                .push(self.job("gsx"))
                .push(self.help_link("GSX bei FSDreamTeam", HelpLink::Gsx));
            content = content.push(self.disclosure(
                Disclosure::GsxManage,
                self.tr("Verwalten & reparieren", "Manage & repair"),
                "",
                manage,
            ));
        } else {
            content = content.push(self.paragraph(self.tr("Der Status dieser Installation ist noch nicht verfügbar. Lade ihn erneut, bevor du die Einrichtung fortsetzt.", "The status of this installation is not available yet. Refresh before continuing setup.")));
        }
        content = content.push(self.refresh_button());
        self.disclosure(Disclosure::Gsx, "GSX Pro", progress.title, content)
    }

    pub(crate) fn mods_page(&self) -> Element<'_, Message> {
        let mods = self.data("mods");
        let mut inventory = column![
            row![
                self.action("Community-Ordner öffnen", ModsOpen),
                self.refresh_button()
            ]
            .spacing(10)
            .wrap(),
            self.job("mods"),
            self.paragraph(s(mods, "folder_path")),
        ]
        .spacing(16);
        let job = &mods["job"];
        if job["state"] == "ready" && job["operation"] == "remove" {
            inventory=inventory.push(self.card("Deinstallation prüfen",column![
                self.pairs(job,&[("addon_id","Add-on"),("entry_path","Betroffener Eintrag")]),
                self.paragraph(if yes(job,"is_link") { self.tr("Nur die Verknüpfung im Community-Ordner wird entfernt. Die Originaldateien bleiben erhalten.","Only the link in the Community folder will be removed. The original files will be kept.").to_string() } else { format!("{} · {}",model::bytes(model::number(&job["bytes"])),self.tr("Der ausgewählte Add-on-Ordner wird dauerhaft gelöscht, einschließlich darin gespeicherter Einstellungen.","The selected add-on folder and any settings it contains will be permanently deleted.")) }),
                self.actions(&[("Deinstallieren",ModsRemove),("Abbrechen",ModsDiscard)])].spacing(16)));
        } else if self.request(&ModsDiscard).is_some() {
            inventory = inventory.push(self.action("Abbrechen", ModsDiscard));
        }
        if let Some(path) = job["quarantine_path"].as_str() {
            inventory = inventory.push(self.paragraph(format!(
                "{}: {path}",
                self.tr("Zurückgehaltener Zwischenordner", "Retained staging folder")
            )));
        }
        if mods["state"] == "ready" {
            inventory = inventory.push(label(
                format!(
                    "{}: {}",
                    self.tr("Gefundene Ordner", "Folders found"),
                    model::number(&mods["count"])
                ),
                18.0,
                Weight::Semibold,
                INK,
            ));
            if mods["mods"].as_array().is_some_and(Vec::is_empty) {
                inventory=inventory.push(self.paragraph(self.t("Noch keine Add-ons in diesem Community-Ordner.").to_string()))
                    .push(self.paragraph(self.t("Öffne den Ordner und folge der Installationsanleitung deines Add-on-Anbieters. Aktualisiere danach diese Liste.").to_string()));
            }
        }
        for item in mods["mods"].as_array().into_iter().flatten().take(500) {
            inventory = inventory
                .push(separator())
                .push(label(
                    if s(item, "name").is_empty() {
                        s(item, "id")
                    } else {
                        s(item, "name")
                    },
                    18.0,
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
                ))
                .push(self.action(
                    "Deinstallation prüfen",
                    ModsPreview(s(item, "id").to_string()),
                ));
        }
        if yes(mods, "limited") {
            inventory = inventory.push(
                self.paragraph(
                    self.t("Die Liste ist begrenzt und zeigt möglicherweise nicht alle Ordner.")
                        .to_string(),
                ),
            );
        }
        column![self.heading(self.t("Deine Add-ons").to_string()),
            self.paragraph(self.t("Öffne die Einrichtung eines Add-ons oder verwalte unten deinen Community-Ordner.").to_string()),
            self.fenix_card(),self.gsx_card(),self.card("Community-Ordner", inventory),
            self.paragraph(self.tr("Fenix und GSX vollständig deinstallieren: Öffne den jeweiligen offiziellen Installer oben. Das Entfernen eines Community-Eintrags entfernt keine Windows-Begleitprogramme.","To fully uninstall Fenix or GSX, open its official installer above. Removing a Community entry does not uninstall Windows companion applications.")),
            self.paragraph(self.t("Diese Liste zeigt Dateien im Community-Ordner. Sie bestätigt weder eine Lizenz noch die Kompatibilität eines Add-ons mit MSFS oder Linux.").to_string())].spacing(16).into()
    }
}
