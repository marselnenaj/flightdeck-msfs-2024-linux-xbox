//! Current add-on actions and normal-viewport captures; no account or simulator calls.
mod common;
use flightdeck_ui::{Action, App, Disclosure, Language, Message, Page, settings, theme};
use iced::{Size, Task};
use serde_json::json;
use std::time::Duration;

fn fixture(state: &str, language: Language) -> App {
    let mut app = common::localized_fixture(language);
    app.page = Page::Mods;
    if state.starts_with("fenix-") {
        let data = app.snapshot.get_mut("fenix").expect("fenix");
        data["idle"] = json!(true);
        if matches!(state, "fenix-manager" | "fenix-repair") {
            data["fenix_installed"] = json!(false);
            data["settings_ready"] = json!(false);
            data["configured"] = json!(false);
        }
        if state == "fenix-repair" {
            data["can_repair_installer"] = json!(true);
            data["job"] = json!({"state":"failed", "operation":"installer",
                "message": if language == Language::De {"Der letzte Einrichtungsschritt der Fenix-App ist fehlgeschlagen."} else {"The last Fenix app setup step failed."}});
        }
        if state == "fenix-running" {
            data["can_change"] = json!(false);
            data["can_stop"] = json!(true);
            data["fenix_running"] = json!(true);
            data["job"] = json!({"state":"running", "operation":"open"});
        }
        let _ = app.update(Message::Toggle(Disclosure::Fenix));
    } else {
        let data = app.snapshot.get_mut("gsx").expect("gsx");
        data["idle"] = json!(true);
        if state == "gsx-new" {
            for key in [
                "prepared",
                "package_installed",
                "startup_found",
                "configured",
            ] {
                data[key] = json!(false);
            }
        }
        if state == "gsx-missing-startup" {
            data["startup_found"] = json!(false);
            data["configured"] = json!(false);
        }
        if state == "gsx-running" {
            data["can_change"] = json!(false);
            data["can_stop"] = json!(true);
            data["manager_running"] = json!(true);
            data["job"] = json!({"state":"running", "operation":"open"});
        }
        let _ = app.update(Message::Toggle(Disclosure::Gsx));
    }
    app
}

#[test]
#[ignore = "writes add-on screenshots with the production software renderer"]
fn capture_addon_states() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/native-ui");
    std::fs::create_dir_all(&directory)?;
    for state in [
        "fenix-manager",
        "fenix-repair",
        "fenix-local",
        "fenix-running",
        "gsx-new",
        "gsx-configured",
        "gsx-running",
        "gsx-missing-startup",
    ] {
        for (language, code) in [(Language::De, "de"), (Language::En, "en")] {
            for width in [750, 1280] {
                let app = iced::application(
                    move || fixture(state, language),
                    |_: &mut App, _: Message| Task::none(),
                    App::view,
                )
                .settings(settings())
                .theme(|_: &App| theme());
                let shot = iced_test::screenshot(
                    &app,
                    &theme(),
                    Size::new(width as f32, 900.0),
                    1.0,
                    Duration::from_millis(80),
                );
                image::save_buffer(
                    directory.join(format!("addons-{state}-{code}-{width}.png")),
                    &shot.rgba,
                    width,
                    900,
                    image::ColorType::Rgba8,
                )?;
            }
        }
    }
    Ok(())
}

#[test]
fn gsx_shows_the_next_action_without_claiming_simulator_success()
-> Result<(), Box<dyn std::error::Error>> {
    for (state, title, operation) in [
        ("gsx-new", "Prepare FSDT", "prepare"),
        ("gsx-missing-startup", "Open FSDT installer", "open"),
        ("gsx-running", "Close FSDT", "stop"),
    ] {
        let app = fixture(state, Language::En);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
        ui.find("Simulator functionality unconfirmed")?;
        assert!(ui.find("Repair FSDT installer").is_err());
        ui.click(title)?;
        assert!(
            matches!(ui.into_messages().next(), Some(Message::Action(Action::Gsx(value))) if value == operation)
        );
        let request = app
            .request(&Action::Gsx(operation))
            .expect("next action allowed");
        assert_eq!(request.path, format!("gsx/{operation}"));
        assert_eq!(request.body, json!({"runtime_path":"/synthetic/msfs2024"}));
    }
    let app = fixture("gsx-configured", Language::En);
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
    ui.find("GSX locally configured · Functionality unconfirmed")?;
    ui.find("Simulator functionality unconfirmed")?;
    assert!(ui.find("Open FSDT installer").is_err());
    ui.click("Go to overview to start MSFS")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Navigate(Page::Overview))
    ));
    Ok(())
}

#[test]
fn gsx_management_is_explicit_and_blocked_or_stale_snapshots_cannot_act()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture("gsx-configured", Language::En);
    let _ = app.update(Message::Toggle(Disclosure::GsxManage));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1400.0), app.view());
    ui.click("Open FSDT installer")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Action(Action::Gsx("open")))
    ));
    drop(app);
    let mut app = fixture("gsx-missing-startup", Language::En);
    app.snapshot.get_mut("gsx").expect("gsx")["can_change"] = json!(false);
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
    ui.click("Open FSDT installer")?;
    assert!(ui.into_messages().next().is_none());
    assert!(app.request(&Action::Gsx("open")).is_none());
    for foreign in [false, true] {
        let mut app = fixture("gsx-missing-startup", Language::En);
        let data = app.snapshot.get_mut("gsx").expect("gsx");
        data["job"] = json!({"state":"failed", "message":"Previous runtime failure"});
        if foreign {
            data["runtime_path"] = json!("/synthetic/other-runtime");
        } else {
            data["_error"] = json!("offline");
        }
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
        assert!(ui.find("Previous runtime failure").is_err());
        assert!(ui.find("Open FSDT installer").is_err());
        assert!(app.request(&Action::Gsx("open")).is_none());
        assert!(app.request(&Action::Gsx("configure")).is_none());
    }
    Ok(())
}
