use crate::*;
use iced::widget::{column, progress_bar};
use model::Action::*;

pub(crate) fn fenix_hook_failure(data: &Value) -> Option<&str> {
    let job = &data["job"];
    // A persisted running record is not evidence that an app is still alive.
    // Current work takes precedence over the previous invocation's result.
    if model::active(job) || yes(data, "fenix_running") {
        return None;
    }
    let attempt = &data["last_attempt"];
    if (attempt.is_null() || yes(data, "last_attempt_superseded"))
        && ["installer", "repair"].contains(&s(job, "operation"))
    {
        if job["state"] == "failed" {
            return Some(s(job, "operation"));
        }
        if job["state"] == "complete" {
            return None;
        }
    }
    (attempt["scope"] == "latest_recorded_attempt"
        && attempt["status"] == "failed"
        && ["installer", "repair"].contains(&s(attempt, "operation")))
    .then(|| s(attempt, "operation"))
}

impl App {
    pub(crate) fn fenix_update_controls<'a>(&'a self, data: &'a Value) -> Element<'a, Message> {
        let mut manage = column![].spacing(14);
        if !model::active(&data["job"]) && ["preparing", "committing"].contains(&s(data, "state")) {
            manage = manage.push(self.paragraph(if yes(data, "can_retry") {
                self.tr(
                    "Die Fenix-Einrichtung wurde unterbrochen. Du kannst sie erneut starten; dein bisheriges Profil und deine Einstellungen bleiben erhalten.",
                    "Fenix setup was interrupted. You can retry; your existing profile and settings are retained.",
                )
            } else {
                self.tr(
                    "Die Fenix-Einrichtung wurde unterbrochen und muss unter Mods wiederhergestellt werden.",
                    "Fenix setup was interrupted and needs recovery under Mods.",
                )
            }));
            if yes(data, "can_retry") {
                manage = manage.push(
                    self.control(
                        self.tr("Fenix-Einrichtung erneut versuchen", "Retry Fenix setup"),
                        self.request(&Fenix("install"))
                            .map(|_| Message::Action(Fenix("install"))),
                        true,
                    ),
                );
            }
        }
        let installed = s(data, "installed_version");
        if !installed.is_empty() {
            manage = manage.push(self.paragraph(format!(
                "{}: {installed}",
                self.tr("Patch-Version", "Patch version")
            )));
        } else if data["state"] == "legacy" {
            manage = manage.push(self.paragraph(self.tr(
                "Installierter Patch: lokale Einrichtung, Version nicht bestätigt.",
                "Installed patch: local setup, version unverified.",
            )));
        }
        if installed.is_empty() && !s(data, "version").is_empty() {
            manage = manage.push(self.paragraph(format!(
                "{}: {}",
                self.tr("Verfügbarer Patch", "Available patch"),
                s(data, "version")
            )));
        }
        if yes(data, "migration_required") {
            if yes(data, "automatic_adoption_paused") {
                manage=manage.push(self.paragraph(self.tr(
                    "Nach der Wiederherstellung ist die automatische Übernahme pausiert. Du kannst sie hier erneut starten.",
                    "Automatic adoption is paused after restoration. You can start it again here.",
                )));
            }
            manage = manage.push(self.paragraph(self.tr(
                "Vorhandene Einrichtung übernehmen und auf den aktuellen Patch aktualisieren. Flightdeck prüft die Dateien und sichert den bisherigen Stand.",
                "Adopt the existing setup and update to the current patch. Flightdeck verifies the files and backs up the previous state.",
            ))).push(self.action("Einrichtung übernehmen & aktualisieren", Fenix("migrate")));
        }
        if data["patch_update"]["state"] == "checking" {
            manage = manage.push(self.paragraph(self.tr(
                "Fenix-Updates werden geprüft …",
                "Checking for Fenix updates …",
            )));
        } else if yes(&data["patch_update"], "requires_launcher_update") {
            manage = manage.push(self.paragraph(self.tr(
                "Für den neuesten Fenix-Patch ist zuerst ein Flightdeck-Update erforderlich.",
                "Update Flightdeck first to use the latest Fenix patch.",
            )));
        } else if data["patch_update"]["state"] == "failed" {
            manage = manage.push(self.paragraph(self.tr("Die Fenix-Updateprüfung ist fehlgeschlagen. Der installierte Patch bleibt nutzbar. Versuche es später erneut.", "The Fenix update check failed. Your installed patch remains available. Try again later.")));
        } else if data["patch_update"]["state"] == "complete"
            && yes(data, "installed")
            && !yes(data, "update_available")
        {
            manage = manage.push(self.paragraph(self.tr(
                "Der Fenix-Patch ist aktuell.",
                "The Fenix patch is up to date.",
            )));
        }
        if yes(data, "update_available") {
            let note = if self.is_steam() {
                self.tr(
                    "Beende Steam, MSFS und Fenix und wähle „Patch aktualisieren“.",
                    "Close Steam, MSFS and Fenix, then choose Update patch.",
                )
            } else {
                self.tr("Wird vor dem nächsten MSFS- oder Fenix-Start automatisch installiert. MSFS und Fenix vorher schließen.", "Installs automatically before the next MSFS or Fenix launch. Close MSFS and Fenix first.")
            };
            manage = manage.push(self.paragraph(format!(
                "{}: {}. {note}",
                self.tr("Neuer Fenix-Patch", "New Fenix patch"),
                s(data, "version")
            )));
        }
        manage = manage.push(
            self.control(
                self.tr("Fenix-Updates prüfen", "Check Fenix updates"),
                self.request(&Fenix("check-update"))
                    .map(|_| Message::Action(Fenix("check-update"))),
                false,
            ),
        );
        if yes(data, "update_available") {
            manage = manage.push(self.action("Patch aktualisieren", Fenix("install")));
        }
        manage.into()
    }

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
    pub(crate) fn addon_feedback<'a>(&self, data: &Value) -> Element<'a, Message> {
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
        let failure = fenix_hook_failure(data);
        let failed_repair = failure == Some("repair");
        let offer_repair = failure == Some("installer") && yes(data, "can_repair_installer");
        let supported = !progress.steps.is_empty() || data["state"] == "legacy";
        let mut content = column![].spacing(14);
        if current {
            content = content.push(self.addon_feedback(data));
            if failure.is_some() && data["job"]["state"] != "failed" {
                content = content.push(label(
                    self.tr("Letzter Vorgang fehlgeschlagen", "Last operation failed"),
                    13.0,
                    Weight::Semibold,
                    iced::color!(0xff9199),
                ));
            }
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
                        if progress.ready && failure.is_none() {
                            "Einrichtung abgeschlossen"
                        } else {
                            "Als Nächstes"
                        },
                        if progress.ready && failure.is_none() {
                            "Setup complete"
                        } else {
                            "Next step"
                        },
                    ),
                    13.0,
                    Weight::Semibold,
                    self.edition.accent(),
                ));
                if failed_repair {
                    content = content
                        .push(self.paragraph(self.tr(
                            "Die letzte Reparatur ist fehlgeschlagen. Prüfe den Diagnosebericht, bevor du einen weiteren Versuch startest.",
                            "The last repair failed. Review the diagnostics before trying again.",
                        )))
                        .push(self.control(
                            "Diagnose öffnen",
                            Some(Message::Navigate(Page::Diagnostics)),
                            true,
                        ));
                } else if offer_repair {
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
                        if data["readouts"] == false && !yes(data, "readouts_preference_retained") {
                            // Otherwise only reachable in the collapsed management section.
                            let action = Fenix("configure");
                            let message = self.request(&action).map(|_| Message::Action(action));
                            content = content.push(self.control(
                                "Einstellungen erneut anwenden",
                                message,
                                false,
                            ));
                        }
                    } else if data["state"] == "legacy" {
                        content = content.push(
                            self.action("Einrichtung übernehmen & aktualisieren", Fenix("migrate")),
                        );
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
                    (
                        self.tr("Legacy-FCU-Anzeigen", "Legacy FCU readouts"),
                        yes(data, "configured") && data["readouts"] != false,
                    ),
                ]));
            }
            let display_note = match data["display_renderer"].as_str() {
                Some("CPU") => self.tr(
                    "MSFS 2024 · CPU-Renderer · Legacy-FCU, ohne Wetterradar. Eigene Fenix-Lizenz nötig.",
                    "MSFS 2024 · CPU renderer · Legacy FCU, no weather radar. Own Fenix license required.",
                ),
                Some("GPU") => self.tr(
                    "MSFS 2024 · GPU-Renderer · Legacy-FCU. Eigene Fenix-Lizenz nötig.",
                    "MSFS 2024 · GPU renderer · Legacy FCU readouts. Own Fenix license required.",
                ),
                _ => self.tr(
                    "MSFS 2024 · Anzeigen & Legacy-FCU. Eigene Fenix-Lizenz nötig.",
                    "MSFS 2024 · Displays & Legacy FCU readouts. Own Fenix license required.",
                ),
            };
            content = content.push(self.paragraph(display_note));
            let mut manage = column![].spacing(14);
            let mut actions = vec![];
            if manager && !(progress.next == 2 && !offer_repair && !failed_repair && !active) {
                actions.push(("Installer & Liveries", Fenix("manager")));
            }
            if yes(data, "fenix_installed") && progress.ready {
                actions.push(("Fenix öffnen", Fenix("open")));
            }
            if yes(data, "can_repair_installer") && !offer_repair {
                actions.push(("Fenix-App reparieren", Fenix("repair")));
            }
            if yes(data, "configured") {
                actions.push(("Einstellungen erneut anwenden", Fenix("configure")));
            }
            for (title, action) in actions {
                let message = self.request(&action).map(|_| Message::Action(action));
                manage = manage.push(self.control(title, message, false));
            }
            let installer_is_next = progress.next == 2
                && !manager
                && !active
                && !failed_repair
                && !yes(data, "fenix_running");
            if yes(data, "installed") && !installer_is_next {
                manage = manage
                    .push(self.paragraph(
                        self.tr("Fenix-App neu installieren", "Reinstall the Fenix app"),
                    ))
                    .push(self.fenix_installer(true));
            }
            manage = manage.push(self.fenix_update_controls(data)).push(self.job("fenix"))
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
        self.disclosure(
            Disclosure::Fenix,
            "Fenix A320",
            if failed_repair {
                "Fenix-Reparatur fehlgeschlagen"
            } else {
                progress.title
            },
            content,
        )
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

    fn navigraph_installer(&self, app: &'static str, rerun: bool) -> Element<'_, Message> {
        let (download, title, field, placeholder, operation, button) = if app == "hub" {
            (
                "Navigraph Hub bei Navigraph herunterladen",
                "Navigraph-Hub-Installer-Datei",
                "navigraph_hub_installer_path",
                "/home/…/Navigraph Hub.exe",
                "install-hub",
                if rerun {
                    "Navigraph Hub neu installieren"
                } else {
                    "Navigraph Hub installieren"
                },
            )
        } else {
            (
                "Simlink bei Navigraph herunterladen",
                "Simlink-Installer-Datei",
                "navigraph_simlink_installer_path",
                "/home/…/Navigraph+Simlink.exe",
                "install-simlink",
                if rerun {
                    "Simlink neu installieren"
                } else {
                    "Simlink installieren"
                },
            )
        };
        column![
            self.help_link(download, HelpLink::Navigraph),
            self.input(title, field, placeholder, Some(NavigraphPick(app))),
            self.control(
                button,
                self.request(&Navigraph(operation))
                    .map(|_| Message::Action(Navigraph(operation))),
                !rerun,
            ),
        ]
        .spacing(12)
        .into()
    }

    fn navigraph_card(&self) -> Element<'_, Message> {
        let raw = self.data("navigraph");
        let current = self.fresh("navigraph") && s(raw, "runtime_path") == self.runtime();
        let data = if current { raw } else { &Value::Null };
        let progress = presentation::navigraph(data);
        let active = model::active(&data["job"]);
        let packages: Vec<_> = data["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut content = column![
            label(self.tr("Von Navigraph nicht offiziell unterstützt", "Not officially supported by Navigraph"), 15.0, Weight::Semibold, iced::color!(0xf5c873)),
            self.paragraph(self.tr("Flightdeck installiert Navigraph Hub für Navdata und Simlink für die Flugzeugposition in Navigraph Charts, jeweils im Profil dieser MSFS-Version. Anmeldung und Abonnement bleiben bei Navigraph.", "Flightdeck installs Navigraph Hub for navdata and Simlink for the aircraft position in Navigraph Charts, each in this MSFS edition's profile. Sign-in and subscription remain with Navigraph.")),
        ].spacing(14);
        if current {
            content = content.push(self.addon_feedback(data));
            content = content.push(self.paragraph(self.t(progress.detail).to_string()));
            if active || yes(data, "hub_running") || yes(data, "simlink_running") {
                if yes(data, "can_stop") {
                    content =
                        content.push(self.addon_action("Navigraph schließen", Navigraph("stop")));
                }
            } else if data["state"] == "available" {
                if data["idle"] == false
                    || yes(data, "busy")
                    || !["", "stopped"].contains(&s(&self.status()["game"], "state"))
                {
                    content = content.push(self.paragraph(self.tr(
                        "Beende MSFS oder die laufende Einrichtung, bevor du Navigraph änderst.",
                        "Close MSFS or the running setup before making changes to Navigraph.",
                    )));
                }
                content = match progress.next {
                    0 => content.push(self.control(
                        "Zur Übersicht und MSFS starten",
                        Some(Message::Navigate(Page::Overview)),
                        true,
                    )),
                    1 => content.push(
                        self.addon_action("Navigraph-Anmeldung einrichten", Navigraph("handler")),
                    ),
                    2 => content.push(self.navigraph_installer("hub", false)),
                    3 => content
                        .push(self.addon_action("Navigraph Hub öffnen", Navigraph("open-hub"))),
                    4 => content.push(self.navigraph_installer("simlink", false)),
                    _ => content.push(
                        self.addon_action("Simlink-Autostart einrichten", Navigraph("configure")),
                    ),
                };
            }
            if data["state"] == "available" {
                content = content.push(self.addon_facts(&[
                    (self.tr("Anmeldung", "Sign-in"), yes(data, "handler")),
                    ("Navigraph Hub", yes(data, "hub_installed")),
                    ("Navdata", !packages.is_empty()),
                    ("Simlink", yes(data, "simlink_installed")),
                    (
                        self.tr("Simlink-Autostart", "Simlink auto-start"),
                        yes(data, "startup"),
                    ),
                ]));
                if !packages.is_empty() {
                    content = content.push(self.paragraph(format!(
                        "{}: {}",
                        self.tr(
                            "Navigraph-Pakete im Community-Ordner",
                            "Navigraph packages in the Community folder"
                        ),
                        packages.join(", ")
                    )));
                }
            }
            let mut manage = column![].spacing(14);
            if yes(data, "hub_installed") {
                if progress.next != 3 {
                    manage =
                        manage.push(self.action("Navigraph Hub öffnen", Navigraph("open-hub")));
                }
                manage = manage.push(self.navigraph_installer("hub", true));
            }
            if yes(data, "simlink_installed") {
                manage = manage
                    .push(self.action("Simlink öffnen", Navigraph("open-simlink")))
                    .push(self.navigraph_installer("simlink", true));
            } else if progress.next != 4 && data["state"] == "available" {
                manage = manage.push(self.navigraph_installer("simlink", false));
            }
            if yes(data, "startup") {
                manage =
                    manage.push(self.action("Simlink-Autostart ausschalten", Navigraph("disable")));
            }
            manage = manage
                .push(self.job("navigraph"))
                .push(self.help_link("Navigraph-Downloads", HelpLink::Navigraph));
            content = content.push(self.disclosure(
                Disclosure::NavigraphManage,
                self.tr("Verwalten & reparieren", "Manage & repair"),
                "",
                manage,
            ));
        } else {
            content = content.push(self.paragraph(self.tr("Der Status dieser Installation ist noch nicht verfügbar. Lade ihn erneut, bevor du die Einrichtung fortsetzt.", "The status of this installation is not available yet. Refresh before continuing setup.")));
        }
        content = content.push(self.refresh_button());
        self.disclosure(Disclosure::Navigraph, "Navigraph", progress.title, content)
    }

    pub(crate) fn community_card(&self) -> Element<'_, Message> {
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
                ));
            if yes(mods, "can_remove") {
                inventory = inventory.push(self.action(
                    "Deinstallation prüfen",
                    ModsPreview(s(item, "id").to_string()),
                ));
            }
        }
        if yes(mods, "limited") {
            inventory = inventory.push(
                self.paragraph(
                    self.t("Die Liste ist begrenzt und zeigt möglicherweise nicht alle Ordner.")
                        .to_string(),
                ),
            );
        }
        self.card("Community-Ordner", inventory)
    }
    pub(crate) fn mods_page(&self) -> Element<'_, Message> {
        column![self.heading(self.t("Deine Add-ons").to_string()),
            self.paragraph(self.t("Öffne die Einrichtung eines Add-ons oder verwalte unten deinen Community-Ordner.").to_string()),
            self.fenix_card(),self.gsx_card(),self.navigraph_card(),self.community_card(),
            self.paragraph(self.tr("Fenix und GSX vollständig deinstallieren: Öffne den jeweiligen offiziellen Installer oben. Das Entfernen eines Community-Eintrags entfernt keine Windows-Begleitprogramme.","To fully uninstall Fenix or GSX, open its official installer above. Removing a Community entry does not uninstall Windows companion applications.")),
            self.paragraph(self.t("Diese Liste zeigt Dateien im Community-Ordner. Sie bestätigt weder eine Lizenz noch die Kompatibilität eines Add-ons mit MSFS oder Linux.").to_string())].spacing(16).into()
    }
}
