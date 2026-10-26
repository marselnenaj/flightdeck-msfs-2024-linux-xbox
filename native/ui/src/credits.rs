// SPDX-License-Identifier: MIT
//! Human-readable credits complement the complete per-release license inventory.
use crate::*;
use iced::widget::column;

struct Credit {
    name: &'static str,
    de: &'static str,
    en: &'static str,
    url: &'static str,
}
struct Group {
    de: &'static str,
    en: &'static str,
    entries: &'static [Credit],
}
const GROUPS: &[Group] = &[
    Group {
        de: "Flightdeck & Community",
        en: "Flightdeck & community",
        entries: &[
            Credit {
                name: "marselnenaj",
                de: "Entwicklung und Pflege von Flightdeck und der Linux-Integration.",
                en: "Development and maintenance of Flightdeck and its Linux integration.",
                url: "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox",
            },
            Credit {
                name: "OnlyCrayZ-Labs",
                de: "Wine-Media-Fix für Systemspeicher und Testfälle für den Taskqueue-Lebenszyklus.",
                en: "Wine system-memory media fix and taskqueue lifecycle test scenarios.",
                url: "https://github.com/OnlyCrayZ-Labs/MSFS2024-Linux",
            },
            Credit {
                name: "gut-engel",
                de: "Korrektur für das Microsoft-Anmeldefenster unter NVIDIA/Wayland.",
                en: "Fix for the Microsoft sign-in window on NVIDIA/Wayland.",
                url: "https://github.com/gut-engel",
            },
            Credit {
                name: "Flightdeck Community",
                de: "Tests auf unterschiedlichen Systemen, Fehlerberichte, Ideen und gegenseitige Unterstützung. Danke an alle Mitwirkenden!",
                en: "Testing across systems, bug reports, ideas and support for each other. Thank you to everyone contributing!",
                url: "https://discord.gg/fAhHR2DkqN",
            },
        ],
    },
    Group {
        de: "Kompatibilität & Spielintegration",
        en: "Compatibility & game integration",
        entries: &[
            Credit {
                name: "Xodus Gaming",
                de: "Xodus, Xbox-Anmeldung, Paketwerkzeuge und die Grundlage des angepassten Proton-Runners.",
                en: "Xodus, Xbox sign-in, package tools and the basis of the adapted Proton runner.",
                url: "https://github.com/xodus-gaming",
            },
            Credit {
                name: "Sightem / WineGDK",
                de: "Grundlage der Wine-/GDK-Schnittstellen und der Queue-/Async-Implementierung.",
                en: "Foundation for the Wine/GDK interfaces and queue/async implementation.",
                url: "https://github.com/Sightem/WineGDK",
            },
            Credit {
                name: "Wine & CodeWeavers",
                de: "Windows-Kompatibilität unter Linux und die Wine-Multimedia-Komponenten.",
                en: "Windows compatibility on Linux and Wine multimedia components.",
                url: "https://www.winehq.org/",
            },
            Credit {
                name: "Valve / Proton",
                de: "Proton und die Arbeit vieler Mitwirkender an Windows-Spielen unter Linux.",
                en: "Proton and the work of many contributors on Windows games on Linux.",
                url: "https://github.com/ValveSoftware/Proton",
            },
            Credit {
                name: "Microsoft / libHttpClient",
                de: "Ursprüngliche Queue-Implementierung, auf der die angepasste WineGDK-Fassung aufbaut.",
                en: "Original queue implementation underlying the adapted WineGDK version.",
                url: "https://github.com/microsoft/libHttpClient",
            },
            Credit {
                name: "xal-rs",
                de: "Authentifizierungsbibliothek innerhalb der Xodus-Integration.",
                en: "Authentication library used by the Xodus integration.",
                url: "https://github.com/xodus-gaming/xal-rs",
            },
            Credit {
                name: "tuxuser / eappx-rs",
                de: "Lesen des EAPPX-Paketformats.",
                en: "EAPPX package-format reader.",
                url: "https://github.com/tuxuser/eappx-rs",
            },
            Credit {
                name: "nlohmann / JSON for Modern C++",
                de: "JSON-Verarbeitung in den nativen Kompatibilitätskomponenten.",
                en: "JSON processing in the native compatibility components.",
                url: "https://github.com/nlohmann/json",
            },
            Credit {
                name: "Fenix A320 Linux Patch",
                de: "Fenix-Linux-Patch und gemeinsam verwendeter Installer-Code; Dank an alle Mitwirkenden.",
                en: "Fenix Linux adaptations and shared installer code; thanks to its contributors.",
                url: "https://github.com/marselnenaj/fenix-a320-linux-patch",
            },
        ],
    },
    Group {
        de: "Grafik & Medien",
        en: "Graphics & media",
        entries: &[
            Credit {
                name: "DXVK",
                de: "Direct3D-Übersetzung auf Vulkan; Projektteam und Mitwirkende.",
                en: "Direct3D translation to Vulkan; project team and contributors.",
                url: "https://github.com/doitsujin/dxvk",
            },
            Credit {
                name: "VKD3D-Proton",
                de: "Direct3D 12 auf Vulkan; Projektteam und Mitwirkende.",
                en: "Direct3D 12 on Vulkan; project team and contributors.",
                url: "https://github.com/HansKristian-Work/vkd3d-proton",
            },
            Credit {
                name: "GStreamer",
                de: "Multimedia-Pipeline für Wine und die jeweiligen System-Codecs.",
                en: "Multimedia pipeline for Wine and the system codecs.",
                url: "https://gstreamer.freedesktop.org/",
            },
            Credit {
                name: "FFmpeg",
                de: "Audio-/Video-Codecs über GStreamer oder den ausgewählten Proton-Runner.",
                en: "Audio/video codecs through GStreamer or the selected Proton runner.",
                url: "https://ffmpeg.org/",
            },
        ],
    },
    Group {
        de: "Desktop, Schrift & Gestaltung",
        en: "Desktop, typography & artwork",
        entries: &[
            Credit {
                name: "Rust",
                de: "Programmiersprache, Standardbibliothek und die vielen verwendeten Crates.",
                en: "Programming language, standard library and the many crates used by Flightdeck.",
                url: "https://www.rust-lang.org/",
            },
            Credit {
                name: "iced",
                de: "Native Oberfläche; Projektteam und Mitwirkende.",
                en: "Native interface; project team and contributors.",
                url: "https://github.com/iced-rs/iced",
            },
            Credit {
                name: "tiny-skia",
                de: "Software-Rendering der Launcher-Oberfläche.",
                en: "Software rendering of the launcher interface.",
                url: "https://github.com/linebender/tiny-skia",
            },
            Credit {
                name: "Manrope Project Authors",
                de: "Manrope von Mikhail Sharanda und Mirko Velimirovic, unter der SIL Open Font License 1.1.",
                en: "Manrope by Mikhail Sharanda and Mirko Velimirovic, under the SIL Open Font License 1.1.",
                url: "https://github.com/google/fonts/tree/b31870aff700ab7a1d74fa0c6887d95beb9e0037/ofl/manrope",
            },
            Credit {
                name: "Flightdeck-Artwork / OpenAI",
                de: "Eigene Flugzeugillustrationen mit OpenAI-Bilderzeugung; Flightdeck-Logo und Icons im Projekt. Bildherkunft und Lizenz sind dokumentiert.",
                en: "Original aircraft illustrations created with OpenAI image generation; Flightdeck logo and icons in the project. Artwork provenance and license are documented.",
                url: "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/blob/main/docs/artwork.md",
            },
        ],
    },
];

impl App {
    pub(crate) fn credits_navigation(&self, rail: bool) -> Element<'_, Message> {
        let active = self.page == Page::Credits;
        let color = if active { self.edition.accent() } else { MUTED };
        let symbol = icon("info", 22.0, color);
        let content: Element<'_, Message> = if rail {
            container(symbol).center(Length::Fill).into()
        } else {
            row![symbol, label("Credits", 14.0, Weight::Semibold, color)]
                .spacing(18)
                .align_y(alignment::Vertical::Center)
                .into()
        };
        let control = button(content)
            .padding(if rail {
                Padding::ZERO
            } else {
                Padding::from([10, 0])
            })
            .width(if rail {
                Length::Fixed(60.0)
            } else {
                Length::Fill
            })
            .height(44)
            .on_press(Message::Navigate(Page::Credits))
            .style(move |_, status| button::Style {
                background: Some(
                    if active || matches!(status, button::Status::Hovered) {
                        iced::color!(0x19333f)
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                text_color: color,
                border: Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        let control = container(control).id("credits-navigation");
        if rail {
            iced::widget::tooltip(
                control,
                container(label("Credits", 14.0, Weight::Medium, INK))
                    .padding(10)
                    .style(card_style),
                iced::widget::tooltip::Position::Right,
            )
            .into()
        } else {
            control.into()
        }
    }

    pub(crate) fn credits_page(&self) -> Element<'_, Message> {
        let mut content = column![self.paragraph(self.tr(
            "Flightdeck entsteht durch die Arbeit vieler Menschen und freier Projekte. Hier sagen wir Danke.",
            "Flightdeck is made possible by many people and open-source projects. Thank you for your work."
        ))].spacing(20);
        for group in GROUPS {
            let mut entries = Column::new().spacing(20);
            for credit in group.entries {
                entries = entries.push(
                    column![
                        self.help_link(credit.name, HelpLink::Project(credit.url)),
                        self.paragraph(self.tr(credit.de, credit.en)),
                    ]
                    .spacing(4),
                );
            }
            content = content.push(self.card(self.tr(group.de, group.en), entries));
        }
        content.push(self.card(self.tr("Alle Bibliotheken & Lizenzen","All libraries & licenses"),column![
            self.paragraph(self.tr(
                "Hier findest du die vollständigen Lizenztexte, alle Launcher-Bibliotheken mit Versionen und die Quellen dieser Flightdeck-Version.",
                "Find the complete license texts, every launcher library with its version, and the sources for this Flightdeck release here."
            )),
            self.help_link(self.tr("Bibliotheken mit Versionen","Libraries and versions"), HelpLink::DependencyInventory),
            self.help_link(self.tr("Launcher-Lizenztexte","Launcher license texts"), HelpLink::LauncherLicenses),
            self.help_link(self.tr("Rust-Standardbibliothek","Rust standard library"), HelpLink::RustLicenses),
            self.help_link(self.tr("Kompatibilitätslizenzen","Compatibility licenses"), HelpLink::RuntimeLicenses),
            self.help_link(self.tr("Grafiklizenzen","Graphics licenses"), HelpLink::GraphicsLicenses),
            self.help_link(self.tr("Release & vollständige Quellen","Release & complete sources"), HelpLink::ReleaseSources),
        ].spacing(8))).into()
    }
}

/// The installed executable lives in <package>/bin. Development binaries do
/// not use documents from an unrelated target directory.
pub(crate) fn installed_document(
    executable: &std::path::Path,
    relative: &str,
) -> Option<std::path::PathBuf> {
    let bin = executable.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let document = bin.parent()?.join(relative);
    std::fs::symlink_metadata(&document)
        .ok()?
        .is_file()
        .then_some(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn license_documents_use_the_actual_package_and_skip_development_or_missing_files() {
        let root = tempfile::tempdir().expect("root");
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        let document = root.path().join("THIRD-PARTY-NOTICES.txt");
        std::fs::write(&document, "notices").expect("notices");
        assert_eq!(
            installed_document(&bin.join("flightdeck"), "THIRD-PARTY-NOTICES.txt"),
            Some(document)
        );
        assert!(
            installed_document(
                &root.path().join("target/debug/flightdeck-rust"),
                "THIRD-PARTY-NOTICES.txt"
            )
            .is_none()
        );
        assert!(installed_document(&bin.join("flightdeck"), "missing.txt").is_none());
    }
}
