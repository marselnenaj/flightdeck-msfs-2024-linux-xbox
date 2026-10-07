//! Native Flightdeck desktop interface, backed by the verified local service.
mod addons;
mod client;
mod controller;
mod model;
mod pages;
mod presentation;
mod release_notes;
mod setup_page;
pub use client::{Client, Request, Snapshot};
use iced::{Subscription, Task};
pub use model::Action;
pub type Connector = std::sync::Arc<dyn Fn() -> Result<Client, String> + Send + Sync>;
use model::{Forms, s, yes};
use serde_json::Value;

mod typography;
include!(concat!(env!("OUT_DIR"), "/icons.rs"));

use iced::font::Weight;
use iced::widget::{
    Column, Space, button, column, container, image, pick_list, responsive, row, rule, scrollable,
    stack, svg,
};
use iced::{
    Background, Border, Color, ContentFit, Element, Font, Length, Padding, Settings, Size, Theme,
    alignment, border,
};
use std::sync::LazyLock;

const BG: Color = iced::color!(0x09131d);
const INK: Color = iced::color!(0xf3f6fb);
const MUTED: Color = iced::color!(0xa6b8c9);
const LINE: Color = iced::color!(0x213b4b);
const FONT: Font = Font::with_name("Manrope");
const FONT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/manrope-variable.ttf"));
static SCENE_2024: LazyLock<image::Handle> =
    LazyLock::new(|| scene(include_bytes!("../../../ui/flight-panorama.png")));
static SCENE_2020: LazyLock<image::Handle> =
    LazyLock::new(|| scene(include_bytes!("../../../ui/flight-panorama-2020.png")));
fn scene(bytes: &'static [u8]) -> image::Handle {
    // The renderer evicts hidden images. Keep decoded pixels so returning to
    // Overview does not synchronously decompress the panorama on every visit.
    let pixels = ::image::load_from_memory(bytes)
        .expect("embedded panorama")
        .into_rgba8();
    image::Handle::from_rgba(pixels.width(), pixels.height(), pixels.into_raw())
}
static MARK: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(include_bytes!("../../../ui/mark.svg").as_slice()));

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Edition {
    #[default]
    Msfs2024,
    Msfs2020,
}

impl Edition {
    pub fn id(self) -> &'static str {
        match self {
            Self::Msfs2024 => "msfs2024",
            Self::Msfs2020 => "msfs2020",
        }
    }
    fn year(self) -> &'static str {
        match self {
            Self::Msfs2024 => "2024",
            Self::Msfs2020 => "2020",
        }
    }
    fn name(self) -> String {
        format!("Microsoft Flight Simulator {}", self.year())
    }
    fn accent(self) -> Color {
        match self {
            Self::Msfs2024 => iced::color!(0x64e7f2),
            Self::Msfs2020 => iced::color!(0xf5c873),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    De,
    En,
}
impl Language {
    pub fn code(self) -> &'static str {
        if self == Self::En { "en" } else { "de" }
    }
}
impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::De => "DE",
            Self::En => "EN",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Setup,
    Updates,
    Saves,
    Mods,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Disclosure {
    Graphics,
    Vr,
    Proton,
    Maintenance,
    Removal,
    AdvancedSetup,
    Fenix,
    FenixAdvanced,
    Gsx,
    Cloud,
    Report,
    Diagnostics,
    LauncherRollback,
}
impl Disclosure {
    fn group(self) -> Option<u8> {
        match self {
            Self::Graphics | Self::Vr | Self::Proton | Self::Maintenance => Some(0),
            Self::Fenix | Self::Gsx => Some(1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HelpLink {
    FenixInstaller,
    FenixProject,
    Gsx,
    Vr,
    Install,
}
impl HelpLink {
    fn url(self) -> &'static str {
        match self {
            Self::FenixInstaller => "https://fenixsim.com/dashboard/",
            Self::FenixProject => "https://github.com/marselnenaj/fenix-a320-linux-patch",
            Self::Gsx => "https://www.fsdreamteam.com/products_gsxpro.html",
            Self::Vr => {
                "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/blob/main/docs/vr.md"
            }
            Self::Install => {
                "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/blob/main/docs/install.md"
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Select(Edition),
    Language(Language),
    Navigate(Page),
    Toggle(Disclosure),
    OpenHelp(HelpLink),
    OpenReleaseLink(String),
    Action(Action),
    Tick,
    Refresh,
    Loaded(u64, Result<Snapshot, String>),
    Reconnected(u64, Result<Client, String>),
    Completed(u64, Action, Result<Value, String>),
    StartupCompleted(u64, String, Result<Value, String>),
    Field(&'static str, String),
    Flag(&'static str, bool),
    Discover(&'static str),
    Discovered(u64, &'static str, Result<Value, String>),
    Confirm,
    CancelConfirm,
    CopyReport,
    MailReport,
    EditDescription(iced::widget::text_editor::Action),
    SaveReport,
    SaveDiagnostics,
    CopyDiagnostics,
    Focus(bool),
    Exported(Result<Option<String>, String>),
    Dismiss,
}

pub struct App {
    pub edition: Edition,
    pub language: Language,
    pub page: Page,
    pub notice: Option<String>,
    pub client: Option<Client>,
    connector: Option<Connector>,
    reconnect: bool,
    pub snapshot: Snapshot,
    pub forms: Forms,
    pub online: bool,
    pub pending: bool,
    pending_action: Option<Action>,
    polling: bool,
    poll_task: Option<iced::task::Handle>,
    poll_count: u8,
    generation: u64,
    pub confirmation: Option<(Action, client::Request)>,
    pub startup_checked: bool,
    startup_attempt: Option<std::time::Instant>,
    startup_inflight: Option<u64>,
    startup_sequence: u64,
    startup_retry: bool,
    pub restarting: bool,
    pub discoveries: std::collections::BTreeMap<&'static str, Value>,
    pub expanded: std::collections::BTreeSet<Disclosure>,
    proton_active: Option<(bool, String, String)>,
    connect_after_check: Option<String>,
    report_dirty: bool,
    description: iced::widget::text_editor::Content,
    exporting: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new(Edition::default())
    }
}

impl App {
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            language: Language::De,
            page: Page::Overview,
            notice: None,
            client: None,
            connector: None,
            reconnect: false,
            snapshot: Snapshot::new(),
            forms: Forms::default(),
            online: false,
            pending: false,
            pending_action: None,
            polling: false,
            poll_task: None,
            poll_count: 0,
            generation: 0,
            confirmation: None,
            startup_checked: false,
            startup_attempt: None,
            startup_inflight: None,
            startup_sequence: 0,
            startup_retry: false,
            restarting: false,
            discoveries: Default::default(),
            expanded: Default::default(),
            proton_active: None,
            connect_after_check: None,
            report_dirty: false,
            description: iced::widget::text_editor::Content::new(),
            exporting: false,
        }
    }

    fn tr<'a>(&self, de: &'a str, en: &'a str) -> &'a str {
        match self.language {
            Language::De => de,
            Language::En => en,
        }
    }

    fn page_name(&self, page: Page) -> &'static str {
        match page {
            Page::Overview => self.tr("Übersicht", "Overview"),
            Page::Setup => self.tr("Einrichtung", "Setup"),
            Page::Updates => "Updates",
            Page::Saves => self.tr("Spielstände", "Saves"),
            Page::Mods => "Mods",
            Page::Diagnostics => self.tr("Diagnose", "Diagnostics"),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        responsive(move |size| self.layout(size)).into()
    }

    fn layout(&self, size: Size) -> Element<'_, Message> {
        let compact = size.width <= 1180.0;
        // Tiling compositors can ignore a window's logical minimum at HiDPI.
        let rail = size.width < 960.0;
        let side_width = if rail {
            76.0
        } else if compact {
            210.0
        } else {
            236.0
        };
        let horizontal = if compact {
            22.0
        } else if size.width >= 1600.0 {
            40.0
        } else {
            26.0
        };
        let hero_height = if size.width >= 1600.0 { 460.0 } else { 414.0 };
        // Match the original desktop content width, including its 15 px scrollbar gutter.
        let content_width = (size.width - side_width - horizontal * 2.0 - 15.0).max(300.0);
        let top = if compact { 24.0 } else { 28.0 };
        let mut main = column![self.header(compact), Space::new().height(21)];
        if let Some(notice) = self.notice.as_deref().or_else(|| {
            self.status()["service"]["message"]
                .as_str()
                .filter(|s| !s.is_empty())
        }) {
            main = main
                .push(
                    container(
                        row![
                            label(notice, 14.0, Weight::Normal, INK),
                            Space::new().width(Length::Fill),
                            button("×").on_press(Message::Dismiss)
                        ]
                        .spacing(12),
                    )
                    .padding(14)
                    .style(card_style),
                )
                .push(Space::new().height(15));
        }
        if self.page == Page::Overview {
            main = main
                .push(self.edition_picker())
                .push(Space::new().height(15))
                .push(self.hero(content_width, compact, hero_height))
                .push(Space::new().height(20))
                .push(self.overview_details(content_width, compact))
                .push(Space::new().height(20));
        } else {
            main = main.push(self.page_view());
        }
        let content = container(main.width(content_width))
            .padding(Padding {
                top,
                right: horizontal + 15.0,
                bottom: 32.0,
                left: horizontal,
            })
            .width(Length::Fill);
        let scroll = scrollable(content)
            .height(Length::Fill)
            .width(Length::Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(10)
                    .scroller_width(9)
                    .margin(3),
            ))
            .style(|theme, status| {
                let mut style = scrollable::default(theme, status);
                style.vertical_rail.background = Some(iced::color!(0x242424).into());
                style.vertical_rail.scroller.background = iced::color!(0x969696).into();
                style
            });
        let sidebar = if rail {
            self.sidebar_rail()
        } else {
            self.sidebar(side_width, compact)
        };
        let base: Element<'_, Message> = container(row![sidebar, scroll].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style::default().background(BG).color(INK))
            .into();
        if let Some((_, request)) = &self.confirmation {
            stack![
                base,
                iced::widget::opaque(
                    container(container(self.confirmation_view(request)).max_width(680))
                        .padding(30)
                        .center(Length::Fill)
                        .style(|_| container::Style::default()
                            .background(Color::from_rgba8(0, 0, 0, 0.72)))
                )
            ]
            .into()
        } else {
            base
        }
    }

    fn sidebar_rail(&self) -> Element<'_, Message> {
        let mut content = column![
            container(svg(MARK.clone()).width(38).height(38)).center_x(Length::Fill),
            Space::new().height(20)
        ]
        .spacing(8);
        for (page, name) in [
            (Page::Overview, "home"),
            (Page::Setup, "settings"),
            (Page::Updates, "download"),
            (Page::Saves, "folder"),
            (Page::Mods, "mods"),
            (Page::Diagnostics, "pulse"),
        ] {
            let active = page == self.page;
            let color = if active { self.edition.accent() } else { MUTED };
            let control = button(container(icon(name, 24.0, color)).center(Length::Fill))
                .width(60)
                .height(50)
                .padding(0)
                .on_press(Message::Navigate(page))
                .style(move |_, status| button::Style {
                    background: Some(
                        if active || matches!(status, button::Status::Hovered) {
                            iced::color!(0x19333f)
                        } else {
                            Color::TRANSPARENT
                        }
                        .into(),
                    ),
                    border: Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            content = content.push(iced::widget::tooltip(
                control,
                container(label(self.page_name(page), 14.0, Weight::Medium, INK))
                    .padding(10)
                    .style(card_style),
                iced::widget::tooltip::Position::Right,
            ));
        }
        content = content
            .push(Space::new().height(Length::Fill))
            .push(container(icon("monitor", 24.0, MUTED)).center_x(Length::Fill));
        container(content)
            .padding([28, 8])
            .width(76)
            .height(Length::Fill)
            .style(|_| container::Style::default().background(iced::color!(0x101e2a)))
            .into()
    }

    fn sidebar(&self, width: f32, compact: bool) -> Element<'_, Message> {
        let accent = self.edition.accent();
        let logo_size = if compact { 35.0 } else { 43.0 };
        let brand = container(
            row![
                svg(MARK.clone()).width(logo_size).height(logo_size),
                label(
                    "flightdeck",
                    if compact { 20.0 } else { 23.0 },
                    Weight::Bold,
                    INK
                )
                .weight(750)
                .tracking(-0.8)
            ]
            .spacing(9)
            .align_y(alignment::Vertical::Center),
        )
        .padding(Padding {
            top: 12.0,
            right: 25.0,
            bottom: 37.0,
            left: if compact { 21.0 } else { 25.0 },
        });
        let mut navigation = Column::new().spacing(7);
        for (page, name) in [
            (Page::Overview, "home"),
            (Page::Setup, "settings"),
            (Page::Updates, "download"),
            (Page::Saves, "folder"),
            (Page::Mods, "mods"),
            (Page::Diagnostics, "pulse"),
        ] {
            let active = page == self.page;
            let color = if active {
                accent
            } else {
                iced::color!(0xb7c9dc)
            };
            let item = row![
                container(Space::new()).width(3).height(56).style(move |_| {
                    container::Style::default().background(if active {
                        accent
                    } else {
                        Color::TRANSPARENT
                    })
                }),
                container(
                    row![
                        icon(name, 24.0, color),
                        label(
                            self.page_name(page),
                            if compact { 14.0 } else { 15.0 },
                            Weight::Medium,
                            color
                        )
                    ]
                    .spacing(if compact { 14 } else { 18 })
                    .align_y(alignment::Vertical::Center)
                )
                .padding(Padding {
                    top: 0.0,
                    right: 15.0,
                    bottom: 0.0,
                    left: if compact { 22.0 } else { 30.0 }
                })
                .height(56)
                .align_y(alignment::Vertical::Center)
            ];
            navigation = navigation.push(
                button(item)
                    .on_press(Message::Navigate(page))
                    .padding(0)
                    .width(Length::Fill)
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
                            radius: border::Radius {
                                top_left: 0.0,
                                top_right: 5.0,
                                bottom_right: 5.0,
                                bottom_left: 0.0,
                            },
                            ..Border::default()
                        },
                        ..button::Style::default()
                    }),
            );
        }
        let footer = container(column![
            container(
                row![
                    icon("flask", 24.0, MUTED),
                    label(
                        self.tr("Experimentell", "Experimental"),
                        14.0,
                        Weight::Normal,
                        MUTED
                    )
                ]
                .spacing(20)
                .align_y(alignment::Vertical::Center)
            )
            .padding(Padding {
                top: 20.0,
                right: 0.0,
                bottom: 25.0,
                left: 0.0
            }),
            separator(),
            container(
                row![
                    icon("monitor", 24.0, MUTED),
                    label(
                        self.tr("Lokal auf deinem Rechner", "Local on your computer"),
                        12.0,
                        Weight::Normal,
                        MUTED
                    )
                ]
                .spacing(18)
                .align_y(alignment::Vertical::Center)
            )
            .padding(Padding {
                top: 23.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0
            }),
            container(label(
                concat!("Flightdeck ", env!("FLIGHTDECK_UI_VERSION")),
                10.0,
                Weight::Normal,
                iced::color!(0x678498)
            ))
            .padding(Padding {
                top: 15.0,
                right: 0.0,
                bottom: 0.0,
                left: 42.0
            })
        ])
        .padding(Padding {
            top: 0.0,
            right: 16.0,
            bottom: 0.0,
            left: 24.0,
        });
        container(
            column![brand, navigation, Space::new().height(Length::Fill), footer]
                .height(Length::Fill),
        )
        .width(width)
        .height(Length::Fill)
        .padding(Padding {
            top: 27.0,
            right: 8.0,
            bottom: 20.0,
            left: 0.0,
        })
        .style(|_| {
            container::Style::default().background(gradient(
                115.0,
                iced::color!(0x101e2a),
                iced::color!(0x0d1923),
            ))
        })
        .into()
    }

    fn header(&self, compact: bool) -> Element<'_, Message> {
        let attention =
            self.online && self.status()["cloud"]["state"] == "attention" || self.session_failed();
        let language = pick_list(
            [Language::De, Language::En],
            Some(self.language),
            Message::Language,
        )
        .text_size(12)
        .font(FONT)
        .padding([9, 10])
        .width(72)
        .style(|_, _| pick_list::Style {
            text_color: MUTED,
            placeholder_color: MUTED,
            handle_color: MUTED,
            background: iced::color!(0x101f2b).into(),
            border: Border {
                color: LINE,
                width: 1.0,
                radius: 6.0.into(),
            },
        });
        let status = row![
            icon(
                if attention || !self.online {
                    "info"
                } else {
                    "check-circle"
                },
                25.0,
                if attention {
                    iced::color!(0xf2cb78)
                } else if self.online {
                    self.edition.accent()
                } else {
                    MUTED
                }
            ),
            label(
                if attention {
                    self.tr("Hinweis beachten", "Action needed")
                } else if self.online {
                    self.tr("Flightdeck bereit", "Flightdeck ready")
                } else {
                    self.tr("Verbindung wird hergestellt …", "Connecting …")
                },
                13.0,
                Weight::Normal,
                iced::color!(0xc4d6e7)
            )
        ]
        .spacing(9)
        .align_y(alignment::Vertical::Center);
        container(
            row![
                label(
                    self.page_name(self.page),
                    if compact { 34.0 } else { 40.0 },
                    Weight::Bold,
                    INK
                )
                .line_height(1.2)
                .tracking(-1.3),
                Space::new().width(Length::Fill),
                row![language, status]
                    .spacing(18)
                    .align_y(alignment::Vertical::Center)
            ]
            .align_y(alignment::Vertical::Center),
        )
        .height(55)
        .align_y(alignment::Vertical::Center)
        .padding([0, if compact { 0 } else { 10 }])
        .into()
    }

    fn edition_picker(&self) -> Element<'_, Message> {
        responsive(move |size| {
            let choices = container(
                row![
                    self.edition_button(Edition::Msfs2024),
                    self.edition_button(Edition::Msfs2020)
                ]
                .spacing(10),
            )
            .width(560.0_f32.min((size.width - 36.0).max(0.0)));
            let title = label(
                self.tr("Simulator auswählen", "Select simulator"),
                14.0,
                Weight::Semibold,
                MUTED,
            );
            let content: Element<'_, Message> = if size.width < 760.0 {
                column![title, choices].spacing(12).into()
            } else {
                row![title, choices]
                    .spacing(20)
                    .align_y(alignment::Vertical::Center)
                    .into()
            };
            container(content)
                .padding([14, 18])
                .width(Length::Fill)
                .style(|_| {
                    container::Style::default()
                        .background(iced::color!(0x101f2b))
                        .border(Border {
                            color: LINE,
                            width: 1.0,
                            radius: 12.0.into(),
                        })
                })
                .into()
        })
        .into()
    }

    fn edition_button(&self, edition: Edition) -> Element<'_, Message> {
        let active = if self.page == Page::Setup {
            self.forms.get("game_id") == edition.id()
        } else {
            self.edition == edition
        };
        let accent = if self.page == Page::Setup {
            edition.accent()
        } else {
            self.edition.accent()
        };
        let message = if self.page == Page::Setup {
            (self.online && self.can_edit_field("game_id") && !active)
                .then(|| Message::Field("game_id", edition.id().into()))
        } else {
            (self.request(&Action::Select(edition)).is_some() && !active)
                .then_some(Message::Select(edition))
        };
        let unavailable = message.is_none() && !active;
        let active_bg = if edition == Edition::Msfs2020 {
            iced::color!(0x3a2e22)
        } else {
            iced::color!(0x173542)
        };
        button(
            row![
                label(
                    format!("MSFS {}", edition.year()),
                    17.0,
                    Weight::Bold,
                    if unavailable { MUTED } else { INK }
                ),
                Space::new().width(Length::Fill),
                label(
                    if self.pending_action == Some(Action::Select(edition)) {
                        self.tr("Wird vorbereitet …", "Preparing …")
                    } else if self.page == Page::Setup && active {
                        self.tr("Ausgewählt", "Selected")
                    } else if self.page == Page::Setup {
                        self.tr("Auswählen", "Select")
                    } else if active {
                        self.tr("Aktiv", "Active")
                    } else if unavailable {
                        self.tr("Nicht verfügbar", "Unavailable")
                    } else {
                        self.tr("Wechseln", "Switch")
                    },
                    12.0,
                    Weight::Normal,
                    if active { accent } else { MUTED }
                )
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center),
        )
        .on_press_maybe(message)
        .height(52)
        .width(Length::Fill)
        .padding([10, 17])
        .style(move |_, status| button::Style {
            background: Some(
                if active {
                    active_bg
                } else if matches!(status, button::Status::Disabled) {
                    iced::color!(0x101b25)
                } else if matches!(status, button::Status::Hovered) {
                    iced::color!(0x19333f)
                } else {
                    iced::color!(0x0b1a25)
                }
                .into(),
            ),
            text_color: if unavailable { MUTED } else { INK },
            border: Border {
                color: if active || matches!(status, button::Status::Hovered) {
                    accent
                } else if matches!(status, button::Status::Disabled) {
                    LINE
                } else {
                    iced::color!(0x496579)
                },
                width: if active { 2.0 } else { 1.0 },
                radius: 9.0.into(),
            },
            ..button::Style::default()
        })
        .into()
    }

    fn hero(&self, width: f32, compact: bool, height: f32) -> Element<'_, Message> {
        let accent = self.edition.accent();
        let attention = self.status()["cloud"]["state"] == "attention";
        let warning = attention || self.session_failed();
        let launch_message = self.launch_message();
        let launch_ink = if launch_message.is_some() {
            iced::color!(0x061721)
        } else {
            MUTED
        };
        let left = if compact { 31.0 } else { 45.0 };
        let title_size = if compact { 35.0 } else { 42.0 };
        let scene = if self.edition == Edition::Msfs2020 {
            SCENE_2020.clone()
        } else {
            SCENE_2024.clone()
        };
        let art = image(scene)
            .width(width)
            .height(height)
            .content_fit(ContentFit::Cover)
            .border_radius(12);
        let shade = container(Space::new())
            .width(Length::Fill)
            .height(height)
            .style(|_| {
                container::Style::default()
                    .background(iced::Gradient::Linear(
                        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                            .add_stop(0.0, Color::from_rgba8(5, 16, 26, 0.90))
                            .add_stop(0.28, Color::from_rgba8(5, 16, 26, 0.52))
                            .add_stop(0.58, Color::from_rgba8(5, 16, 26, 0.0)),
                    ))
                    .border(Border {
                        radius: 12.0.into(),
                        ..Border::default()
                    })
            });
        let launch = button(
            container(
                row![
                    icon(if attention { "info" } else { "play" }, 28.0, launch_ink),
                    label(self.launch_label(), 18.0, Weight::Bold, launch_ink)
                ]
                .spacing(13)
                .align_y(alignment::Vertical::Center),
            )
            .center(Length::Fill),
        )
        .width(300)
        .height(62)
        .padding([13, 34])
        .on_press_maybe(launch_message)
        .style(move |_, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Disabled) {
                    iced::color!(0x203443)
                } else if matches!(status, button::Status::Hovered) {
                    Color { a: 0.9, ..accent }
                } else {
                    accent
                }
                .into(),
            ),
            text_color: launch_ink,
            border: Border {
                radius: 8.0.into(),
                color: if matches!(status, button::Status::Disabled) {
                    iced::color!(0x496579)
                } else {
                    Color::TRANSPARENT
                },
                width: if matches!(status, button::Status::Disabled) {
                    1.0
                } else {
                    0.0
                },
            },
            ..button::Style::default()
        });
        let recovery = Action::Automatic("retry");
        let launch_actions: Element<'_, Message> = if self.status()["cloud"]["error_code"]
            == "unsafe_session"
            && self.request(&recovery).is_some()
        {
            row![
                launch,
                button(label(
                    self.tr("Sitzung prüfen", "Check session"),
                    15.0,
                    Weight::Semibold,
                    INK
                ))
                .padding([14, 18])
                .on_press(Message::Action(recovery))
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center)
            .into()
        } else {
            launch.into()
        };
        let content = column![
            label(self.edition.name(), title_size, Weight::Bold, INK)
                .line_height(1.16)
                .tracking(-1.2)
                .width(
                    (if compact { 535.0_f32 } else { 522.0_f32 })
                        .min((width - left - 24.0).max(200.0))
                ),
            Space::new().height(7),
            label(
                self.tr("Xbox-PC-Version · Linux", "Xbox PC version · Linux"),
                18.0,
                Weight::Normal,
                iced::color!(0xbdd1e5)
            ),
            Space::new().height(28),
            row![
                icon(
                    if warning { "info" } else { "check-circle" },
                    28.0,
                    if warning {
                        iced::color!(0xf2cb78)
                    } else {
                        accent
                    }
                ),
                label(
                    self.launch_state(),
                    15.0,
                    Weight::Normal,
                    iced::color!(0xc9d8e5)
                )
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center),
            Space::new().height(22),
            launch_actions,
            Space::new().height(9),
            label(
                self.launch_note(),
                12.0,
                Weight::Normal,
                iced::color!(0xb3c7d7)
            )
            .width(450)
        ];
        let foreground = container(content)
            .padding(Padding {
                top: 44.0,
                right: 0.0,
                bottom: 65.0,
                left,
            })
            .width(Length::Fill);
        let bottom = container(
            row![
                row![
                    icon("monitor", 24.0, iced::color!(0xc4d6e7)),
                    label(
                        self.tr("Läuft lokal unter Linux", "Runs locally on Linux"),
                        14.0,
                        Weight::Normal,
                        iced::color!(0xc4d6e7)
                    )
                ]
                .spacing(16)
                .align_y(alignment::Vertical::Center),
                Space::new().width(Length::Fill),
                label(
                    self.tr("Lokale Sitzung", "Local session"),
                    11.0,
                    Weight::Normal,
                    iced::color!(0xc4d6e7)
                )
            ]
            .align_y(alignment::Vertical::Center),
        )
        .width(Length::Fill)
        .height(height)
        .align_y(alignment::Vertical::Bottom)
        .padding(Padding {
            top: 0.0,
            right: 33.0,
            bottom: 26.0,
            left,
        });
        // tiny-skia 0.14 does not apply image border radii. The vector frame
        // supplies the same rounded corners on both CPU and GPU renderers.
        let frame = hero_frame(width, height);
        stack![
            container(Space::new()).width(width).height(height),
            art,
            shade,
            frame,
            foreground,
            bottom
        ]
        .width(width)
        .height(height)
        .clip(true)
        .into()
    }

    pub(crate) fn link(&self, title: &'static str, page: Page) -> Element<'_, Message> {
        let accent = self.edition.accent();
        column![
            separator(),
            Space::new().height(19),
            button(
                row![
                    label(title, 14.0, Weight::Semibold, accent).weight(600),
                    icon("arrow", 19.0, accent)
                ]
                .spacing(10)
                .align_y(alignment::Vertical::Center)
            )
            .on_press(Message::Navigate(page))
            .padding(0)
            .style(button::text)
        ]
        .into()
    }
}

fn label<'a>(
    value: impl Into<std::borrow::Cow<'a, str>>,
    size: f32,
    weight: Weight,
    color: Color,
) -> typography::Label<'a> {
    let weight = match weight {
        Weight::Normal => 400,
        Weight::Medium => 550,
        Weight::Semibold => 650,
        Weight::Bold => 700,
        _ => 400,
    };
    typography::Label::new(value, size, weight, color)
}

fn separator<'a>() -> Element<'a, Message> {
    rule::horizontal(1)
        .style(|_| rule::Style {
            color: LINE,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

fn gradient(degrees: f32, first: Color, last: Color) -> Background {
    iced::Gradient::Linear(
        iced::gradient::Linear::new(degrees.to_radians())
            .add_stop(0.0, first)
            .add_stop(1.0, last),
    )
    .into()
}

fn card_style(_: &Theme) -> container::Style {
    container::Style::default()
        .background(gradient(
            120.0,
            iced::color!(0x101e2a),
            iced::color!(0x10202b),
        ))
        .border(Border {
            color: LINE,
            width: 1.0,
            radius: 12.0.into(),
        })
}

fn hero_frame<'a>(width: f32, height: f32) -> Element<'a, Message> {
    let right = width - 12.0;
    let bottom = height - 12.0;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><path fill=\"#09131d\" d=\"M0 12V0H12A12 12 0 0 0 0 12ZM{right} 0H{width}V12A12 12 0 0 0 {right} 0ZM{width} {bottom}V{height}H{right}A12 12 0 0 0 {width} {bottom}ZM12 {height}H0V{bottom}A12 12 0 0 0 12 {height}Z\"/><rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"11.5\" fill=\"none\" stroke=\"#213b4b\"/></svg>",
        width - 1.0,
        height - 1.0
    );
    svg::Svg::new(svg::Handle::from_memory(svg.into_bytes()))
        .width(width)
        .height(height)
        .into()
}

fn icon<'a>(name: &str, size: f32, color: Color) -> Element<'a, Message> {
    // Use the existing icon paths rather than introducing a different icon set.
    let contents = icon_source(name);
    let contents = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"{}\" stroke=\"white\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{contents}</svg>",
        if name == "play" { "white" } else { "none" },
        if name == "play" { 1.0 } else { 1.8 }
    );
    svg(svg::Handle::from_memory(contents.into_bytes()))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

pub fn settings() -> Settings {
    typography::load_font(FONT_BYTES);
    Settings {
        default_font: FONT,
        fonts: vec![FONT_BYTES.into()],
        default_text_size: 16.into(),
        ..Settings::default()
    }
}

pub fn theme() -> Theme {
    Theme::custom(
        "Flightdeck",
        iced::theme::Palette {
            background: BG,
            text: INK,
            primary: iced::color!(0x1b4658),
            success: iced::color!(0x7ce98b),
            danger: iced::color!(0xff9199),
            warning: iced::color!(0xf2cb78),
        },
    )
}

pub fn run(client: Client, language: Language, connector: Option<Connector>) -> iced::Result {
    iced::application(
        move || {
            let mut app = App::new(Edition::default());
            app.client = Some(client.clone());
            app.connector = connector.clone();
            app.language = language;
            let task = app.refresh();
            (app, task)
        },
        App::update,
        App::view,
    )
    .title("Flightdeck")
    .settings(settings())
    .theme(|_: &App| theme())
    .subscription(App::subscription)
    .window(iced::window::Settings {
        size: (1280.0, 900.0).into(),
        min_size: Some((960.0, 700.0).into()),
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "flightdeck".into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}
