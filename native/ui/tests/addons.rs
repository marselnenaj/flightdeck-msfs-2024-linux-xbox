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

fn persisted_repair_failure(language: Language) -> App {
    let mut app = fixture("fenix-local", language);
    let data = app.snapshot.get_mut("fenix").expect("fenix");
    data["job"] = serde_json::Value::Null;
    data["can_repair_installer"] = json!(true);
    data["last_attempt"] = json!({
        "schema":1, "scope":"latest_recorded_attempt", "operation":"repair",
        "status":"failed", "failure":"hook_nonzero", "package_version":"1.0.286",
        "started_at":"2026-10-08T10:00:00Z", "completed_at":"2026-10-08T10:00:02Z",
        "hook":"install", "hook_exit_code":82, "process_exit_code":82,
        "evidence_complete":true, "runner":"flightdeck"
    });
    app
}

#[test]
fn persisted_repair_failure_survives_empty_jobs_and_offers_diagnostics_first()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let mut app = persisted_repair_failure(language);
        let (failed, ready, diagnostic, repair) = if language == Language::De {
            (
                "Fenix-Reparatur fehlgeschlagen",
                "Fenix lokal eingerichtet",
                "Diagnose öffnen",
                "Fenix-App reparieren",
            )
        } else {
            (
                "Fenix repair failed",
                "Fenix locally configured",
                "Open diagnostics",
                "Repair Fenix app",
            )
        };
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        ui.find(failed)?;
        assert!(ui.find(ready).is_err());
        assert!(ui.find(repair).is_err());
        ui.find(if language == Language::De {
            "✓ Linux-Patch"
        } else {
            "✓ Linux patch"
        })?;
        ui.click(diagnostic)?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Navigate(Page::Diagnostics))
        ));
        // This is Fenix feedback, not a new prerequisite for the base simulator.
        assert!(app.request(&Action::Launch).is_some());
        let _ = app.update(Message::Toggle(Disclosure::FenixManage));
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1600.0), app.view());
        ui.click(repair)?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Fenix("repair")))
        ));
    }
    Ok(())
}

#[test]
fn failed_repair_without_a_manager_keeps_reinstallation_in_management()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let mut app = persisted_repair_failure(language);
        let data = app.snapshot.get_mut("fenix").expect("fenix");
        for key in [
            "manager_installed",
            "fenix_installed",
            "settings_ready",
            "configured",
            "can_repair_installer",
        ] {
            data[key] = json!(false);
        }
        let (diagnostic, installer_file, reinstall) = if language == Language::De {
            (
                "Diagnose öffnen",
                "Fenix-Installer-Datei",
                "Installer erneut ausführen",
            )
        } else {
            (
                "Open diagnostics",
                "Fenix installer file",
                "Run installer again",
            )
        };
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        assert!(ui.find(installer_file).is_err());
        ui.click(diagnostic)?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Navigate(Page::Diagnostics))
        ));
        assert!(app.request(&Action::Fenix("manager")).is_none());
        assert!(app.request(&Action::Fenix("repair")).is_none());
        assert!(app.request(&Action::Fenix("installer")).is_none());

        let _ = app.update(Message::Toggle(Disclosure::FenixManage));
        let _ = app.update(Message::Field(
            "installer_path",
            "/synthetic/FenixInstaller.exe".into(),
        ));
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1600.0), app.view());
        ui.find(installer_file)?;
        ui.click(reinstall)?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Fenix("installer")))
        ));
        let request = app
            .request(&Action::Fenix("installer"))
            .expect("explicit installer selection");
        assert_eq!(request.path, "fenix/installer");
        assert_eq!(
            request.body["installer_path"],
            "/synthetic/FenixInstaller.exe"
        );
    }
    Ok(())
}

#[test]
fn current_fenix_work_and_newer_completion_take_precedence_over_old_failure()
-> Result<(), Box<dyn std::error::Error>> {
    for job in [
        json!({"state":"running", "operation":"repair"}),
        json!({"state":"complete", "operation":"repair"}),
        json!({"state":"complete", "operation":"installer"}),
    ] {
        let mut app = persisted_repair_failure(Language::En);
        let data = app.snapshot.get_mut("fenix").expect("fenix");
        data["job"] = job;
        data["last_attempt_superseded"] = json!(data["job"]["state"] == "complete");
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        assert!(ui.find("Fenix repair failed").is_err());
        assert!(ui.find("Open diagnostics").is_err());
    }
    for status in ["running", "succeeded", "cancelled", "unknown"] {
        let mut app = persisted_repair_failure(Language::En);
        app.snapshot.get_mut("fenix").expect("fenix")["last_attempt"]["status"] = json!(status);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        ui.find("Fenix locally configured")?;
        assert!(ui.find("Open diagnostics").is_err());
        assert!(ui.find("Close Fenix").is_err());
    }
    // A configuration-only job does not establish that the failed hook passed.
    let mut app = persisted_repair_failure(Language::En);
    app.snapshot.get_mut("fenix").expect("fenix")["job"] =
        json!({"state":"complete", "operation":"configure"});
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.find("Fenix repair failed")?;
    drop(ui);
    // The backend compares timestamps. An older or undated complete job must
    // not turn the newer persisted failure into a successful setup claim.
    for superseded in [serde_json::Value::Null, json!(false)] {
        let data = app.snapshot.get_mut("fenix").expect("fenix");
        data["job"] = json!({"state":"complete", "operation":"repair"});
        data["last_attempt_superseded"] = superseded;
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        ui.find("Fenix repair failed")?;
        ui.find("Open diagnostics")?;
        assert!(ui.find("Fenix locally configured").is_err());
    }
    let data = app.snapshot.get_mut("fenix").expect("fenix");
    data["last_attempt"] = serde_json::Value::Null;
    data["job"] = json!({"state":"failed", "operation":"repair"});
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.find("Fenix repair failed")?;
    ui.find("Open diagnostics")?;
    assert!(ui.find("Repair Fenix app").is_err());
    Ok(())
}

#[test]
fn persisted_installer_failure_can_offer_a_first_repair_but_stale_data_cannot_act()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = persisted_repair_failure(Language::En);
    app.snapshot.get_mut("fenix").expect("fenix")["last_attempt"]["operation"] = json!("installer");
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.find("Last operation failed")?;
    ui.click("Repair Fenix app")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Action(Action::Fenix("repair")))
    ));
    for foreign in [false, true] {
        let mut app = persisted_repair_failure(Language::En);
        let data = app.snapshot.get_mut("fenix").expect("fenix");
        if foreign {
            data["runtime_path"] = json!("/synthetic/other-runtime");
        } else {
            data["_error"] = json!("offline");
        }
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        assert!(ui.find("Fenix repair failed").is_err());
        assert!(ui.find("Open diagnostics").is_err());
        assert!(app.request(&Action::Fenix("repair")).is_none());
    }
    Ok(())
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
