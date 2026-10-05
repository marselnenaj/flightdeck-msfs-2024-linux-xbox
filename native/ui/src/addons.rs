use crate::*;
use iced::widget::column;
use model::Action::*;

impl App {
    fn addon_step<'a>(
        &self,
        index: usize,
        done: bool,
        next: bool,
        title: &str,
        content: Column<'a, Message>,
    ) -> Element<'a, Message> {
        let state = if done {
            "Erledigt"
        } else if next {
            "Als Nächstes"
        } else {
            "Noch offen"
        };
        let color = if done || next {
            self.edition.accent()
        } else {
            MUTED
        };
        container(
            row![
                label(
                    if done {
                        "✓".into()
                    } else {
                        index.to_string()
                    },
                    20.0,
                    Weight::Bold,
                    color
                )
                .width(28),
                column![
                    label(self.t(state).to_string(), 12.0, Weight::Semibold, color),
                    label(self.t(title).to_string(), 18.0, Weight::Semibold, INK),
                    content
                ]
                .spacing(10)
                .width(Length::Fill)
            ]
            .spacing(16),
        )
        .padding(18)
        .width(Length::Fill)
        .style(move |_| {
            container::Style::default().background(BG).border(Border {
                color: if next { color } else { LINE },
                width: 1.0,
                radius: 12.0.into(),
            })
        })
        .into()
    }

    fn fenix_card(&self) -> Element<'_, Message> {
        let raw = self.data("fenix");
        let current = self.fresh("fenix") && s(raw, "runtime_path") == self.runtime();
        let data = if current { raw } else { &Value::Null };
        let progress = presentation::fenix(data, self.status());
        let mut content = column![
            self.paragraph(self.t("Richte die getesteten Fenix-Korrekturen ein und installiere dein Flugzeug mit dem offiziellen Installer.").to_string()),
            self.paragraph(self.t("MSFS 2024 · CPU-Anzeigen · Legacy-Readouts · Eigene Fenix-Lizenz erforderlich. Wetterradar ist im CPU-Modus nicht verfügbar.").to_string()),
            label(self.t(progress.title).to_string(), 18.0, Weight::Semibold, if progress.ready { self.edition.accent() } else { INK }),
            self.paragraph(self.t(progress.detail).to_string()),
        ].spacing(16);
        if progress.ready {
            content =
                content.push(self.link(self.t("Zur Übersicht und MSFS starten"), Page::Overview));
        }
        if !progress.busy.is_empty() {
            content = content.push(self.paragraph(self.t(progress.busy).to_string()));
        }
        if yes(data, "fenix_installed")
            || yes(data, "manager_installed")
            || yes(data, "fenix_running")
        {
            let mut controls = vec![];
            if data["state"] == "legacy" || (yes(data, "configured") && yes(data, "settings_ready"))
            {
                controls.push(("Fenix öffnen", Fenix("open")));
            }
            controls.push(("Fenix beenden", Fenix("stop")));
            content = content
                .push(
                    self.paragraph(
                        self.t(if yes(data, "fenix_running") {
                            "Fenix läuft"
                        } else {
                            "Fenix ist beendet"
                        })
                        .to_string(),
                    ),
                )
                .push(self.actions(&controls));
        }
        if data["state"] == "legacy" {
            content = content.push(self.paragraph(self.t("Deine vorhandenen lokalen Korrekturen bleiben aktiv. Eine automatische Übernahme in das neue Paket ist noch nicht vorgesehen.").to_string()));
        }
        for (index, done) in progress.steps.iter().copied().enumerate() {
            let (title, body) = match index {
                0 => ("Kompatibilität einrichten", column![
                    self.paragraph(self.t("Erstellt eine eigene Runner-Kopie, sichert das Windows-Profil und installiert bei Bedarf Microsoft .NET.").to_string()),
                    self.action(if yes(data,"can_retry") { "Einrichtung reparieren" } else if yes(data,"update_available") { "Patch aktualisieren" } else { "Patch einrichten" }, Fenix("install"))]),
                1 => ("Fenix installieren", column![
                    self.paragraph(self.t("Schritt 2 von 4: Lade den offiziellen Fenix-Installer herunter, wähle die EXE aus und installiere dein Flugzeug.").to_string()),
                    self.help_link("Offiziellen Installer im Fenix-Konto herunterladen",HelpLink::FenixInstaller),
                    self.input("Fenix-Installer-Datei", "installer_path", "/home/…/FenixInstaller.exe", Some(FenixPick("installer"))),
                    self.action("Installer starten",Fenix("installer"))]),
                2 => ("Fenix öffnen und anmelden", column![
                    self.paragraph(self.t("Melde dich in Fenix an und schließe das Programm danach vollständig. Deine Anmeldung und Lizenz prüft Fenix selbst.").to_string()),
                    self.paragraph(self.t("Falls @ mit AltGr+Q nicht klappt: Strg+Alt+Q probieren oder @ kopieren und mit Strg+V einfügen.").to_string()),
                    self.action("Fenix öffnen",Fenix("open"))]),
                _ => ("Anzeigen und automatischer Start", column![
                    self.paragraph(self.t("Stellt CPU-Anzeigen, Legacy-Readouts und den automatischen Fenix-Start ein.").to_string()),
                    self.action(if progress.ready { "Einstellungen erneut anwenden" } else { "Einrichtung abschließen" }, Fenix("configure"))]),
            };
            content = content.push(self.addon_step(
                index + 1,
                done,
                progress.next == index + 1,
                title,
                body.spacing(12),
            ));
        }
        content = content.push(self.job("fenix"))
            .push(row![self.action("Installer & Liveries",Fenix("manager")),self.refresh_button()].spacing(10).wrap())
            .push(self.paragraph(self.t("Öffnet den separaten Fenix-Manager zum Installieren, Aktualisieren und Verwalten von Liveries.").to_string()))
            .push(self.disclosure(Disclosure::FenixAdvanced,"Lokales Patch-Paket und Wiederherstellung","",column![
                self.input("Entpacktes Release (leer = geprüfter Download)","bundle_path",self.tr("Automatisch herunterladen","Download automatically"),Some(FenixPick("bundle"))),
                self.paragraph(self.t("Die Wiederherstellung verwendet das Profil von vor dem Patch. Das neuere Profil bleibt als Sicherung erhalten.").to_string()),
                self.action("Patch rückgängig machen",Fenix("restore"))].spacing(16)));
        content = content.push(self.help_link("Projekt und Anleitung", HelpLink::FenixProject));
        self.disclosure(Disclosure::Fenix, "Fenix A320", progress.title, content)
    }

    fn gsx_card(&self) -> Element<'_, Message> {
        let raw = self.data("gsx");
        let data = if self.fresh("gsx") && s(raw, "runtime_path") == self.runtime() {
            raw
        } else {
            &Value::Null
        };
        let progress = presentation::gsx(data);
        let mut content = column![self.paragraph(self.t(progress.detail).to_string())].spacing(16);
        let titles = [
            "FSDT-Installer vorbereiten",
            "GSX installieren und aktivieren",
            "Automatischen GSX-Start einrichten",
        ];
        let descriptions = [
            "Flightdeck lädt den geprüften Installer und richtet .NET ein. Das bisherige Windows-Profil bleibt als Sicherung erhalten.",
            "Wähle GSX Pro im FSDT-Installer, installiere es und aktiviere deine Lizenz. Schließe den Installer nach dem Download.",
            "Übernimmt den von FSDT angelegten Start mit MSFS. Prüfe anschließend das GSX-Menü und die Bodendienste im Simulator.",
        ];
        let actions = [
            (
                if yes(data, "prepared") {
                    "FSDT-Installer reparieren"
                } else {
                    "FSDT vorbereiten"
                },
                Gsx("prepare"),
            ),
            ("FSDT-Installer öffnen", Gsx("open")),
            ("Automatischen Start einrichten", Gsx("configure")),
        ];
        for (index, done) in progress.steps.iter().copied().enumerate() {
            content = content.push(
                self.addon_step(
                    index + 1,
                    done,
                    progress.next == index + 1,
                    titles[index],
                    column![
                        self.paragraph(self.t(descriptions[index]).to_string()),
                        self.action(actions[index].0, actions[index].1.clone())
                    ]
                    .spacing(12),
                ),
            );
        }
        let mut controls = vec![];
        if yes(data, "manager_running") {
            controls.push(("FSDT schließen", Gsx("stop")));
        }
        if yes(data, "configured") {
            controls.push(("GSX-Autostart ausschalten", Gsx("disable")));
        }
        if yes(data, "can_recover") {
            controls.push(("GSX-Vorbereitung wiederherstellen", Gsx("recover")));
        }
        content = content
            .push(self.job("gsx"))
            .push(self.actions(&controls))
            .push(self.refresh_button());
        content = content.push(self.help_link("GSX bei FSDreamTeam", HelpLink::Gsx));
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
            self.fenix_card(),self.gsx_card(),inventory,
            self.paragraph(self.tr("Fenix und GSX vollständig deinstallieren: Öffne den jeweiligen offiziellen Installer oben. Das Entfernen eines Community-Eintrags entfernt keine Windows-Begleitprogramme.","To fully uninstall Fenix or GSX, open its official installer above. Removing a Community entry does not uninstall Windows companion applications.")),
            self.paragraph(self.t("Diese Liste zeigt Dateien im Community-Ordner. Sie bestätigt weder eine Lizenz noch die Kompatibilität eines Add-ons mit MSFS oder Linux.").to_string())].spacing(16).into()
    }
}
