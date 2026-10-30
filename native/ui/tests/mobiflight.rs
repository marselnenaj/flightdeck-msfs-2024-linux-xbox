// SPDX-License-Identifier: MIT
mod common;
use flightdeck_ui::{Action, Disclosure, Language, Message, Page, settings};
use iced::Size;
use serde_json::json;

fn fixture(language: Language) -> flightdeck_ui::App {
    let mut app = common::localized_fixture(language);
    app.page = Page::Mods;
    app.snapshot.insert("mobiflight", json!({"state":"available","runtime_path":"/synthetic/msfs2024","profile_token":"selected-profile","installed":true,"hid_fix":"ready","payload_available":true,"wasm_installed":true,"wasm_events_ready":true,"startup":false,"idle":true,"can_change":true,"device_access":{"state":"ready"},"winebus":{"state":"ready"}}));
    let _ = app.update(Message::Toggle(Disclosure::Mobiflight));
    app
}

#[test]
fn actions_bind_current_profile_and_require_idle_and_verified_payload() {
    let mut app = fixture(Language::En);
    let request = app
        .request(&Action::Mobiflight("startup-enable"))
        .expect("ready profile");
    assert_eq!(request.body["runtime_path"], "/synthetic/msfs2024");
    assert_eq!(request.body["profile_token"], "selected-profile");
    app.snapshot.get_mut("mobiflight").expect("fixture")["hid_fix"] = json!("unsupported");
    assert!(app.request(&Action::Mobiflight("startup-enable")).is_none());
    app.snapshot.get_mut("mobiflight").expect("fixture")["can_change"] = json!(false);
    assert!(app.request(&Action::Mobiflight("prepare")).is_none());
    app.snapshot.get_mut("mobiflight").expect("fixture")["can_change"] = json!(true);
    app.snapshot.get_mut("mobiflight").expect("fixture")["runtime_path"] =
        json!("/different/profile");
    for operation in [
        "open",
        "install",
        "prepare",
        "configure-hid",
        "startup-enable",
        "startup-disable",
        "stop",
    ] {
        assert!(app.request(&Action::Mobiflight(operation)).is_none());
    }
}

#[test]
fn bilingual_card_opens_app_and_marks_simulator_connection_unconfirmed()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let app = fixture(language);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 2200.0), app.view());
        ui.find(if language == Language::De {
            "App vorhanden · Verbindung unbestätigt"
        } else {
            "App detected · connection unconfirmed"
        })?;
        ui.click(if language == Language::De {
            "MobiFlight öffnen"
        } else {
            "Open MobiFlight"
        })?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Mobiflight("open")))
        ));
    }
    Ok(())
}

#[test]
fn incomplete_module_offers_repair_without_reinstalling_app()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        for missing in ["wasm_installed", "wasm_events_ready"] {
            let mut app = fixture(language);
            app.snapshot.get_mut("mobiflight").expect("fixture")[missing] = json!(false);
            let mut ui =
                iced_test::Simulator::with_size(settings(), Size::new(1280.0, 2200.0), app.view());
            ui.find(if language == Language::De {
                "App vorhanden · Einrichtung unvollständig"
            } else {
                "App detected · setup incomplete"
            })?;
            ui.click(if language == Language::De {
                "MobiFlight vorbereiten / reparieren"
            } else {
                "Prepare / repair MobiFlight"
            })?;
            assert!(matches!(
                ui.into_messages().next(),
                Some(Message::Action(Action::Mobiflight("prepare")))
            ));
            assert!(app.request(&Action::Mobiflight("install")).is_none());
        }
    }
    Ok(())
}

#[test]
#[ignore = "writes MobiFlight screenshots with the production software renderer"]
fn capture_mobiflight() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/native-ui");
    std::fs::create_dir_all(&directory)?;
    for (language, code) in [(Language::De, "de"), (Language::En, "en")] {
        for width in [750, 1280] {
            let app = iced::application(
                move || fixture(language),
                |_: &mut flightdeck_ui::App, _: Message| iced::Task::none(),
                flightdeck_ui::App::view,
            )
            .settings(settings())
            .theme(|_: &flightdeck_ui::App| flightdeck_ui::theme());
            let shot = iced_test::screenshot(
                &app,
                &flightdeck_ui::theme(),
                Size::new(width as f32, 900.0),
                1.0,
                std::time::Duration::from_millis(80),
            );
            image::save_buffer(
                directory.join(format!("mobiflight-{code}-{width}.png")),
                &shot.rgba,
                width,
                900,
                image::ColorType::Rgba8,
            )?;
        }
    }
    Ok(())
}
