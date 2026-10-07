//! Update discovery presentation and release-note interaction regressions.
mod common;
use flightdeck_ui::{App, Language, Message, Page, settings, theme};
use iced::{Size, Task};
use serde_json::json;
use std::time::Duration;

const NOTES: &str = "## Improvements\n\nA **clear release summary** with a second sentence.\n\n- The Start button responds during background checks.\n- Both simulator editions can be selected.\n\n[Release details](https://example.com/releases/1)\n\n### Installation\n\n1. Check the available version.\n2. Choose when to download.\n\n```text\nflightdeck --version\n```\n\n### More changes\n\n";

fn notes_fixture(language: Language) -> App {
    let mut app = common::localized_fixture(language);
    app.page = Page::Updates;
    let update = app
        .snapshot
        .get_mut("launcher-update")
        .expect("launcher update");
    update["notes"] = json!(format!(
        "{NOTES}{}\nEnd of release notes.",
        "- Additional synthetic release information for the scrollable notes.\n".repeat(45)
    ));
    update["update_available"] = json!(true);
    update["release_url"] = json!("https://example.com/releases/1");
    app
}

#[test]
fn release_notes_render_headings_and_lists_with_an_explicit_safe_link_action() {
    let app = notes_fixture(Language::En);
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1300.0), app.view());
    ui.find("Flightdeck & simulator updates")
        .expect("neutral page title");
    ui.find("What's new?").expect("release notes heading");
    ui.find("Improvements")
        .expect("markdown heading without markers");
    ui.find("A clear release summary with a second sentence.")
        .expect("readable paragraph");
    ui.find("The Start button responds during background checks.")
        .expect("separate bullet text");
    assert!(ui.find("## Improvements").is_err());
    ui.click("Release details ↗")
        .expect("clickable explicit release link");
    assert!(
        matches!(ui.into_messages().next(), Some(Message::OpenReleaseLink(url)) if url == "https://example.com/releases/1")
    );
}

#[test]
fn overview_distinguishes_available_unknown_failed_and_stale_update_state() {
    let mut app = common::localized_fixture(Language::En);
    app.snapshot.insert(
        "launcher-update",
        json!({"update_available":true,"latest_version":"0.2.6"}),
    );
    app.snapshot.insert("game-update", json!({"available":true,"update_available":true,"latest_version":"1.7.0","background_current":true,"job":{"state":"failed","operation":"check"}}));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1800.0), app.view());
    ui.find("Flightdeck: Flightdeck update available · 0.2.6")
        .expect("launcher update on overview");
    ui.find("Simulator: A new game version is available · 1.7.0")
        .expect("new background result supersedes old job failure");
    ui.click("Open updates").expect("overview opens updates");
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Navigate(Page::Updates))
    ));
    app.snapshot
        .insert("launcher-update", json!({"update_available":null}));
    app.snapshot.insert("game-update", json!({"available":true,"startup_error":"Connection unavailable","background_current":true,"job":{"state":"complete","operation":"verify"}}));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1800.0), app.view());
    ui.find("Flightdeck: Updates have not been checked yet")
        .expect("unknown is not current");
    ui.find("Simulator: Update not completed")
        .expect("background failure remains visible");
    drop(ui);
    app.snapshot.insert(
        "launcher-update",
        json!({"update_available":false,"_error":"temporarily offline"}),
    );
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1800.0), app.view());
    ui.find("Flightdeck: Status unavailable")
        .expect("stale current is not shown as current");
}

#[test]
fn long_release_notes_scroll_inside_their_panel() {
    let app = notes_fixture(Language::En);
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1300.0), app.view());
    let start = ui.find("Improvements").expect("first heading").bounds();
    assert!(
        ui.find("End of release notes.")
            .expect("last paragraph")
            .visible_bounds()
            .is_none()
    );
    ui.point_at(start.center());
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 },
    })]);
    let end_after = ui
        .find("End of release notes.")
        .expect("scrolled final paragraph")
        .visible_bounds()
        .expect("scroll exposes the final paragraph");
    assert!(
        end_after.y >= start.y && end_after.y + end_after.height <= start.y + 300.0,
        "final paragraph is in the bounded notes viewport"
    );
}

#[test]
#[ignore = "Explicit native update screenshot capture into build/native-ui"]
fn capture_update_states() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/native-ui");
    std::fs::create_dir_all(&directory)?;
    for language in [Language::De, Language::En] {
        for state in ["notes", "available", "unknown"] {
            for width in [750, 1280] {
                let application = iced::application(move || {
                    let mut app = notes_fixture(language);
                    if state != "notes" {
                        app.page = Page::Overview;
                        app.snapshot.insert("launcher-update", if state == "available" { json!({"update_available":true,"latest_version":"0.2.6"}) } else { json!({"update_available":null}) });
                        app.snapshot.insert("game-update", if state == "available" { json!({"available":true,"update_available":true,"latest_version":"1.7.0"}) } else { json!({"available":true,"update_available":null}) });
                    }
                    app
                }, |_: &mut App, _: Message| Task::none(), App::view).settings(settings()).theme(|_: &App| theme());
                let height = 1300;
                let screenshot = iced_test::screenshot(
                    &application,
                    &theme(),
                    Size::new(width as f32, height as f32),
                    1.0,
                    Duration::from_millis(80),
                );
                image::save_buffer(
                    directory.join(format!("updates-{state}-{}-{width}.png", language.code())),
                    &screenshot.rgba,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )?;
            }
        }
    }
    Ok(())
}
