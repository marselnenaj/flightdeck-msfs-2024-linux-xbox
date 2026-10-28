//! Orbx controls stay bound to the current profile and never take another worker.
mod common;
use flightdeck_ui::{Action, Disclosure, Language, Message, Page, settings};
use iced::Size;
use serde_json::json;
fn fixture(language: Language) -> flightdeck_ui::App {
    let mut app = common::localized_fixture(language);
    app.page = Page::Mods;
    app.snapshot.insert("orbx",json!({"state":"available","runtime_path":"/synthetic/msfs2024","installed":true,"configured":true,"settings_found":true,"can_change":true,"idle":true,"running":false,"worker_busy":false}));
    let _ = app.update(Message::Toggle(Disclosure::Orbx));
    app
}
#[test]
fn current_profile_can_open_and_busy_other_worker_cannot_be_taken() {
    let mut app = fixture(Language::En);
    assert_eq!(
        app.request(&Action::Orbx("open"))
            .expect("current profile may open Orbx")
            .body["runtime_path"],
        "/synthetic/msfs2024"
    );
    app.snapshot.get_mut("orbx").expect("Orbx fixture")["worker_busy"] = json!(true);
    assert!(app.request(&Action::Orbx("open")).is_none());
    assert!(app.request(&Action::Orbx("stop")).is_none());
    app.snapshot.get_mut("orbx").expect("Orbx fixture")["worker_busy"] = json!(false);
    app.snapshot.get_mut("orbx").expect("Orbx fixture")["runtime_path"] = json!("/other/profile");
    assert!(app.request(&Action::Orbx("open")).is_none());
}
#[test]
fn renders_open_action_in_both_languages() -> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let app = fixture(language);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1500.0), app.view());
        ui.click(if language == Language::De {
            "Orbx Central öffnen"
        } else {
            "Open Orbx Central"
        })?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Orbx("open")))
        ));
    }
    Ok(())
}
