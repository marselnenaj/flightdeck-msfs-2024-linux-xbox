//! Storefront-aware selection and the same Fenix workflow for Steam installations.
mod common;
use flightdeck_ui::{Action, App, Disclosure, Language, Message, Page, settings, theme};
use iced::{Size, Task};
use serde_json::json;
use std::time::Duration;
fn fixture(language: Language, state: &str) -> App {
    let mut app = common::localized_fixture(language);
    app.snapshot.get_mut("status").expect("status")["runtime"] = json!({"configured":true,"ready":true,"source":"steam","path":"steam:2024:revision","installation_id":"steam2024","installation_name":"Microsoft Flight Simulator 2024 · Steam","game_id":"msfs2024","game_path":"/synthetic/library/steamapps/common/MSFS 2024","prefix":"/synthetic/library/steamapps/compatdata/2537590/pfx","needs_first_launch":state=="first-launch"});
    app.snapshot.get_mut("status").expect("status")["cloud"] =
        json!({"enabled":false,"state":"idle"});
    app.snapshot.get_mut("status").expect("status")["game"] =
        json!({"state":"stopped","can_start":true,"can_stop":false,"managed":false});
    app.snapshot.insert("installations",json!({"selected_id":"steam2024","can_select":true,"installations":[
        {"id":"xbox2024","name":"Microsoft Flight Simulator 2024 · Xbox","source":"xbox","available":true,"context":"/synthetic/xbox2024"},
        {"id":"steam2024","name":"Microsoft Flight Simulator 2024 · Steam","source":"steam","available":true,"context":"steam:2024:revision"},
        {"id":"steam2020","name":"Microsoft Flight Simulator 2020 · Steam (Flatpak)","source":"steam","available":true,"context":"steam:2020:revision"}]}));
    app.snapshot.insert("fenix",json!({"source":"steam","runtime_path":"steam:2024:revision","state":if state=="available"||state=="first-launch"{"available"}else{"installed"},"installed":!["available","first-launch"].contains(&state),"version":"0.1.1","installed_version":"0.1.1","fenix_installed":true,"manager_installed":true,"settings_ready":true,"configured":false,"idle":true,"can_change":state!="first-launch","can_check_updates":true,"can_restore":true,"tool_selected":false,"steam_running":state=="steam-running","can_repair_installer":true,"patch_update":{"state":"complete"},"job":if state=="failed"{json!({"state":"failed","operation":"repair","error":"Typed setup failure"})}else{json!(null)}}));
    app.snapshot.insert("launcher-update",json!({"managed":true,"installed_version":env!("CARGO_PKG_VERSION"),"latest_version":env!("CARGO_PKG_VERSION"),"can_check":true,"can_install":false,"job":{"state":"complete"}}));
    app.page = Page::Mods;
    app
}
#[test]
fn simultaneous_xbox_and_steam_installations_use_the_reviewed_installation_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture(Language::De, "installed");
    let request = app
        .request(&Action::SelectInstallation("xbox2024".into()))
        .expect("select Xbox");
    assert_eq!(request.path, "installations/select");
    assert_eq!(request.body["installation_id"], "xbox2024");
    assert_eq!(request.body["binding"], "/synthetic/xbox2024");
    assert_eq!(request.runtime, "steam:2024:revision");
    let flat = app
        .request(&Action::SelectInstallation("steam2020".into()))
        .expect("select Flatpak");
    assert_eq!(flat.body["binding"], "steam:2020:revision");
    app.snapshot.get_mut("status").expect("status")["game"]["state"] = json!("external");
    assert!(
        app.request(&Action::SelectInstallation("xbox2024".into()))
            .is_none()
    );
    app.snapshot.get_mut("status").expect("status")["game"]["can_stop"] = json!(true);
    assert!(app.request(&Action::Stop).is_some());
    Ok(())
}
#[test]
fn steam_never_offers_xbox_setup_cloud_or_runner_writes_and_can_return_to_xbox_setup() {
    let app = fixture(Language::En, "installed");
    for action in [
        Action::SetupCheck,
        Action::SetupStart,
        Action::Backup,
        Action::Cloud("import"),
        Action::Game("check"),
        Action::ProtonDefault,
        Action::Store("sign-in"),
        Action::Graphics,
        Action::VrSave,
    ] {
        assert!(app.request(&action).is_none(), "{action:?}");
    }
    assert_eq!(
        app.request(&Action::XboxSetup).expect("Xbox setup").path,
        "installations/xbox-setup"
    );
    assert!(app.request(&Action::SteamOpen).is_some());
}
#[test]
fn steam_setup_keeps_real_errors_and_the_existing_fenix_installer_workflow_visible()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let mut app = fixture(language, "failed");
        let repair = if language == Language::De {
            "Fenix-App reparieren"
        } else {
            "Repair Fenix app"
        };
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1800.0), app.view());
        ui.find("Typed setup failure")?;
        ui.click(repair)?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Fenix("repair")))
        ));
        app.snapshot.get_mut("fenix").expect("Fenix")["steam_running"] = json!(true);
        assert!(app.request(&Action::Fenix("select")).is_none());
        assert!(app.request(&Action::Fenix("configure")).is_none());
        assert!(app.request(&Action::Fenix("repair")).is_none());
        assert!(app.request(&Action::Fenix("manager")).is_some());
        app.page = Page::Updates;
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(750.0, 1800.0), app.view());
        ui.find(if language == Language::De {
            "In Steam öffnen"
        } else {
            "Open in Steam"
        })?;
        assert!(
            ui.find(if language == Language::De {
                "Mit Microsoft anmelden und prüfen"
            } else {
                "Sign in with Microsoft & check"
            })
            .is_err()
        );
    }
    Ok(())
}
#[test]
fn typed_fenix_evidence_is_visible_in_the_diagnostic_summary()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = common::localized_fixture(Language::En);
    app.page = Page::Diagnostics;
    app.snapshot.insert("diagnostics",json!({"summary":{"fenix":{"operation":"repair","status":"failed","failure":"managed_exception","hook_exit_code":82,"managed_exception_types":["System.IO.FileNotFoundException"]}}}));
    let _ = app.update(Message::Toggle(Disclosure::Diagnostics));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 2400.0), app.view());
    ui.find("Fenix setup")?;
    Ok(())
}
#[test]
#[ignore = "writes Steam setup and discovery views with the production software renderer"]
fn capture_steam_states() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/native-ui");
    std::fs::create_dir_all(&root)?;
    for language in [Language::De, Language::En] {
        for state in [
            "installed",
            "first-launch",
            "available",
            "failed",
            "steam-running",
        ] {
            for (page, name) in [
                (Page::Overview, "overview"),
                (Page::Mods, "fenix"),
                (Page::Setup, "setup"),
                (Page::Updates, "updates"),
            ] {
                for width in [750, 1280] {
                    let app = iced::application(
                        move || {
                            let mut app = fixture(language, state);
                            app.page = page;
                            app
                        },
                        |_: &mut App, _: Message| Task::none(),
                        App::view,
                    )
                    .settings(settings())
                    .theme(|_: &App| theme());
                    let image = iced_test::screenshot(
                        &app,
                        &theme(),
                        Size::new(width as f32, 1000.0),
                        1.0,
                        Duration::from_millis(80),
                    );
                    image::save_buffer(
                        root.join(format!(
                            "steam-{name}-{state}-{}-{width}.png",
                            language.code()
                        )),
                        &image.rgba,
                        width,
                        1000,
                        image::ColorType::Rgba8,
                    )?;
                }
            }
        }
    }
    Ok(())
}
