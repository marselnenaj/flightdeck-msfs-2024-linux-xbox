use flightdeck_ui::{Action, App, Disclosure, Language, Message, Page, settings};
use iced::Size;
use serde_json::json;

mod common;

fn manager_only(language: Language) -> App {
    let mut app = common::localized_fixture(language);
    app.page = Page::Mods;
    app.snapshot.insert(
        "fenix",
        json!({
            "state":"installed", "runtime_path":"/synthetic/msfs2024",
            "installed":true, "manager_installed":true, "fenix_installed":false,
            "settings_ready":false, "configured":false, "idle":true, "can_change":true,
            "job":{"state":"failed", "operation":"installer", "message":"Installer hook failed"}
        }),
    );
    let _ = app.update(Message::Toggle(Disclosure::Fenix));
    app
}

#[test]
fn existing_manager_opens_without_repeating_the_installer_download()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let app = manager_only(language);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        ui.find(if language == Language::De {
            "Fenix-App erkannt · Einrichtung unvollständig"
        } else {
            "Fenix app detected · Setup incomplete"
        })?;
        ui.find("Installer hook failed")?;
        assert!(
            ui.find(if language == Language::De {
                "Fenix lokal eingerichtet"
            } else {
                "Fenix locally configured"
            })
            .is_err()
        );
        assert!(ui.find(if language == Language::De {
            "Schritt 2 von 4: Lade den offiziellen Fenix-Installer herunter, wähle die EXE aus und installiere dein Flugzeug."
        } else {
            "Step 2 of 4: Download the official Fenix installer, select the EXE and install your aircraft."
        }).is_err());
        ui.click(if language == Language::De {
            "Fenix-App öffnen"
        } else {
            "Open Fenix app"
        })?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Fenix("manager")))
        ));
        let request = app
            .request(&Action::Fenix("manager"))
            .expect("existing app enabled");
        assert_eq!(request.path, "fenix/manager");
        assert_eq!(request.body, json!({"runtime_path":"/synthetic/msfs2024"}));
        assert!(app.request(&Action::Fenix("open")).is_none());
        assert!(app.request(&Action::Fenix("configure")).is_none());
    }
    Ok(())
}

#[test]
fn existing_manager_keeps_explicit_installer_and_runtime_guards()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = manager_only(Language::En);
    assert!(app.request(&Action::Fenix("installer")).is_none());
    let _ = app.update(Message::Toggle(Disclosure::FenixManage));
    let _ = app.update(Message::Field(
        "installer_path",
        "/synthetic/FenixInstaller.exe".into(),
    ));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.click("Run installer again")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Action(Action::Fenix("installer")))
    ));
    let request = app
        .request(&Action::Fenix("installer"))
        .expect("explicit installer enabled");
    assert_eq!(
        request.body["installer_path"],
        "/synthetic/FenixInstaller.exe"
    );

    let _ = app.update(Message::Toggle(Disclosure::FenixManage));
    app.snapshot.get_mut("fenix").expect("fenix")["can_change"] = json!(false);
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.click("Open Fenix app")?;
    assert!(ui.into_messages().next().is_none());
    assert!(app.request(&Action::Fenix("manager")).is_none());
    app.snapshot.get_mut("fenix").expect("fenix")["runtime_path"] = json!("/synthetic/msfs2020");
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    assert!(ui.find("Open Fenix app").is_err());
    Ok(())
}

#[test]
fn missing_manager_still_offers_the_official_installer() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = manager_only(Language::En);
    app.snapshot.get_mut("fenix").expect("fenix")["manager_installed"] = json!(false);
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.find("Run installer")?;
    assert!(ui.find("Open Fenix app").is_err());
    assert!(app.request(&Action::Fenix("manager")).is_none());
    Ok(())
}

#[test]
fn repair_is_an_explicit_action_for_a_verified_existing_app()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let mut app = manager_only(language);
        assert!(app.request(&Action::Fenix("repair")).is_none());
        app.snapshot.get_mut("fenix").expect("fenix")["can_repair_installer"] = json!(true);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
        ui.find(if language == Language::De {
            "Führt den Einrichtungsschritt der vorhandenen Fenix-App erneut aus. Dein Flugzeug wird dabei nicht installiert."
        } else {
            "Reruns setup for the existing Fenix app. This does not install your aircraft."
        })?;
        ui.click(if language == Language::De {
            "Fenix-App reparieren"
        } else {
            "Repair Fenix app"
        })?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Fenix("repair")))
        ));
        let request = app
            .request(&Action::Fenix("repair"))
            .expect("explicit verified repair");
        assert_eq!(request.path, "fenix/repair");
        assert_eq!(request.body, json!({"runtime_path":"/synthetic/msfs2024"}));
        assert!(app.request(&Action::Fenix("configure")).is_none());
        app.snapshot.get_mut("fenix").expect("fenix")["can_change"] = json!(false);
        assert!(app.request(&Action::Fenix("repair")).is_none());
    }
    Ok(())
}

#[test]
fn missing_manager_can_be_reinstalled_without_losing_existing_fenix_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = common::localized_fixture(Language::En);
    app.page = Page::Mods;
    app.snapshot.get_mut("fenix").expect("fenix")["manager_installed"] = json!(false);
    let _ = app.update(Message::Toggle(Disclosure::Fenix));
    let _ = app.update(Message::Toggle(Disclosure::FenixManage));
    assert!(app.request(&Action::Fenix("manager")).is_none());
    assert!(app.request(&Action::Fenix("installer")).is_none());
    let _ = app.update(Message::Field(
        "installer_path",
        "/synthetic/FenixInstaller.exe".into(),
    ));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1800.0), app.view());
    ui.find("Fenix locally configured")?;
    ui.click("Run installer again")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Action(Action::Fenix("installer")))
    ));
    let request = app
        .request(&Action::Fenix("installer"))
        .expect("explicit reinstall allowed");
    assert_eq!(request.body["runtime_path"], "/synthetic/msfs2024");
    assert_eq!(
        request.body["installer_path"],
        "/synthetic/FenixInstaller.exe"
    );
    app.snapshot.get_mut("fenix").expect("fenix")["runtime_path"] = json!("/synthetic/another");
    assert!(app.request(&Action::Fenix("installer")).is_none());
    Ok(())
}

#[test]
fn current_detection_error_is_visible_before_opening_management()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = common::localized_fixture(Language::En);
    app.page = Page::Mods;
    app.snapshot.insert(
        "fenix",
        json!({"state":"unavailable", "runtime_path":"/synthetic/msfs2024",
        "message":"Synthetic prerequisite could not be detected", "can_change":false}),
    );
    let _ = app.update(Message::Toggle(Disclosure::Fenix));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(750.0, 900.0), app.view());
    ui.find("Synthetic prerequisite could not be detected")?;
    assert!(app.request(&Action::Fenix("install")).is_none());
    assert!(app.request(&Action::Fenix("installer")).is_none());
    Ok(())
}
