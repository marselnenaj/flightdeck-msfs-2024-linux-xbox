use flightdeck_ui::{Action, App, Edition, Language, Message, Page, settings, theme};
use iced::{Size, Task};
use serde_json::json;
use std::time::Duration;

mod common;
use common::fixture;

#[test]
fn stopped_online_launches_but_unknown_external_or_disconnected_never_do() {
    let mut app = fixture();
    assert!(app.request(&Action::Launch).is_some());
    for state in ["unknown", "external", "starting", "running", "stopping"] {
        app.snapshot.get_mut("status").expect("status")["game"]["state"] = json!(state);
        assert!(app.request(&Action::Launch).is_none());
    }
    app = fixture();
    app.online = false;
    assert!(app.request(&Action::Launch).is_none());
    app = fixture();
    app.pending = true;
    assert!(app.request(&Action::Launch).is_none());
    app = fixture();
    app.snapshot.get_mut("status").expect("status")["setup"]["busy"] = json!(true);
    assert!(app.request(&Action::Launch).is_none());
    app = fixture();
    app.snapshot.get_mut("status").expect("status")["runtime"]["configured"] = json!(false);
    assert!(app.request(&Action::Launch).is_none());
    app.language = Language::En;
    assert!(!app.launch_note().contains("synced automatically"));
}
#[test]
fn stale_fenix_or_foreign_runtime_cannot_be_changed() {
    let mut app = fixture();
    assert!(app.request(&Action::Fenix("configure")).is_some());
    app.snapshot.get_mut("fenix").expect("fenix")["runtime_path"] = json!("/synthetic/other");
    assert!(app.request(&Action::Fenix("configure")).is_none());
    app = fixture();
    app.snapshot.get_mut("fenix").expect("fenix")["_error"] = json!("offline");
    assert!(app.request(&Action::Fenix("configure")).is_none());
}
#[test]
fn jobs_plans_and_rollback_are_bound_to_reviewed_ids() {
    let app = fixture();
    let import = app.request(&Action::Cloud("import")).expect("import");
    assert_eq!(
        import.body,
        json!({"plan_id":"reviewed-plan","choice":"cloud"})
    );
    assert!(import.confirmation.is_some());
    let upload = app.request(&Action::Cloud("upload")).expect("upload");
    assert_eq!(upload.body["choice"], "local");
    assert_eq!(
        app.request(&Action::Cloud("restore"))
            .expect("restore")
            .body["backup_id"],
        "owned-backup"
    );
    assert_eq!(
        app.request(&Action::Launcher("install"))
            .expect("install")
            .body["check_id"],
        "checked-launcher"
    );
    assert!(
        app.request(&Action::Fenix("restore"))
            .expect("restore")
            .confirmation
            .is_some()
    );
}
#[test]
fn conflict_blocks_playing_locally_and_requires_a_specific_choice() {
    let mut app = fixture();
    app.snapshot.get_mut("status").expect("status")["cloud"] = json!({"state":"attention","enabled":true,"request_id":"cloud-request","can_retry":true,"can_play_local":true,"conflict":true});
    assert!(app.request(&Action::Launch).is_none());
    assert!(app.request(&Action::Automatic("play-local")).is_none());
    let request = app.request(&Action::Automatic("cloud")).expect("resolve");
    assert_eq!(
        request.body,
        json!({"request_id":"cloud-request","choice":"cloud"})
    );
    assert!(request.confirmation.is_some());
}
#[test]
fn running_cloud_job_can_be_cancelled_without_unlocking_other_actions() {
    let mut app = fixture();
    app.snapshot.get_mut("status").expect("status")["setup"]["busy"] = json!(true);
    app.snapshot.get_mut("cloud-saves").expect("cloud")["job"] =
        json!({"state":"running","id":"cloud-job"});
    app.snapshot.get_mut("cloud-saves").expect("cloud")["can_cancel"] = json!(true);
    assert_eq!(
        app.request(&Action::Cloud("cancel")).expect("cancel").body["job_id"],
        "cloud-job"
    );
    assert!(app.request(&Action::Backup).is_none());
}
#[test]
fn setup_pause_resume_and_start_use_the_server_job() {
    let mut app = fixture();
    app.snapshot.get_mut("setup").expect("setup")["job"] =
        json!({"id":"download","state":"installing","phase":"download","can_pause":true});
    assert_eq!(
        app.request(&Action::SetupControl("pause"))
            .expect("pause")
            .body["job_id"],
        "download"
    );
    assert!(app.request(&Action::SetupControl("resume")).is_none());
    app.snapshot.get_mut("setup").expect("setup")["job"] =
        json!({"id":"checked","state":"ready","mode":"prepare"});
    assert_eq!(
        app.request(&Action::SetupStart).expect("start").body["check_id"],
        "checked"
    );
}
#[test]
fn setup_selection_does_not_relabel_the_active_game() {
    let mut app = fixture();
    let _ = app.update(Message::Select(Edition::Msfs2020));
    assert_eq!(app.edition, Edition::Msfs2024);
    assert_eq!(app.forms.get("game_id"), "msfs2020");
    assert_eq!(app.page, Page::Setup);
}
#[test]
fn setup_edition_buttons_change_the_form_in_both_directions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture();
    app.page = Page::Setup;
    app.forms
        .text
        .insert("destination_path", "/synthetic/custom-msfs2024".into());
    for (name, id) in [("MSFS 2020", "msfs2020"), ("MSFS 2024", "msfs2024")] {
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(750.0, 900.0), app.view());
        ui.click(name)?;
        let message = ui.into_messages().next().expect("form selection");
        assert!(matches!(&message, Message::Field("game_id", selected) if selected == id));
        let _ = app.update(message);
        assert_eq!(app.forms.get("game_id"), id);
        assert_eq!(
            app.request(&Action::SetupCheck).expect("check").body["game_id"],
            id
        );
        assert_eq!(app.edition, Edition::Msfs2024);
        assert_eq!(app.runtime(), "/synthetic/msfs2024");
        assert_eq!(app.forms.get("destination_path"), "");
    }
    Ok(())
}
#[test]
fn language_switch_during_mutation_does_not_lose_completion() {
    let mut app = fixture();
    app.pending = true;
    let _ = app.update(Message::Language(Language::En));
    assert_eq!(app.language, Language::De);
    assert!(app.pending);
}
#[test]
fn reviewed_maintenance_cannot_run_with_changed_options_or_runtime() {
    let mut app = fixture();
    app.snapshot.get_mut("maintenance").expect("maintenance")["job"] = json!({
        "state":"ready", "operation":"uninstall", "id":"reviewed-removal",
        "runtime_path":"/synthetic/msfs2024", "keep_data":true, "delete_packages":false
    });
    assert_eq!(
        app.request(&Action::MaintenanceStart)
            .expect("reviewed")
            .body["job_id"],
        "reviewed-removal"
    );
    app.forms.flags.insert("delete_packages", true);
    assert!(app.request(&Action::MaintenanceStart).is_none());
    assert!(app.request(&Action::MaintenanceDiscard).is_some());
    app.forms.flags.insert("delete_packages", false);
    app.snapshot.get_mut("maintenance").expect("maintenance")["job"]["runtime_path"] =
        json!("/synthetic/other");
    assert!(app.request(&Action::MaintenanceStart).is_none());
}
#[test]
fn cancelling_automatic_sync_uses_the_current_request_while_launch_stays_locked() {
    let mut app = fixture();
    app.snapshot.get_mut("status").expect("status")["setup"]["busy"] = json!(true);
    app.snapshot.get_mut("status").expect("status")["cloud"] = json!({
        "state":"syncing", "enabled":true, "request_id":"active-sync", "can_cancel":true
    });
    assert_eq!(
        app.request(&Action::Automatic("cancel-auto"))
            .expect("cancel")
            .body["request_id"],
        "active-sync"
    );
    assert!(app.request(&Action::Launch).is_none());
    assert!(app.request(&Action::Backup).is_none());
}
#[test]
fn confirmation_rejects_a_replaced_plan_and_blocks_underlying_controls() {
    let mut app = fixture();
    let _ = app.update(Message::Action(Action::Cloud("import")));
    assert!(app.confirmation.is_some());
    let _ = app.update(Message::Navigate(Page::Mods));
    let _ = app.update(Message::Field(
        "description",
        "not allowed behind modal".into(),
    ));
    assert_eq!(app.page, Page::Overview);
    assert_eq!(app.forms.get("description"), "");
    app.snapshot.get_mut("cloud-saves").expect("cloud")["plan"]["id"] = json!("different-plan");
    let _ = app.update(Message::Confirm);
    assert!(!app.pending);
    assert!(app.confirmation.is_none());
    assert!(
        app.notice
            .as_deref()
            .expect("notice")
            .contains("Status hat sich geändert")
    );
}
#[test]
fn stale_responses_cannot_overwrite_a_newer_navigation_or_mutation() {
    let mut app = fixture();
    let _ = app.update(Message::Navigate(Page::Mods));
    let _ = app.update(Message::Loaded(0, Err("old request".into())));
    assert!(app.notice.is_none());
    let _ = app.update(Message::Loaded(1, Ok(fixture().snapshot)));
    assert!(app.online);
    let _ = app.update(Message::Completed(
        1,
        Action::Backup,
        Ok(json!({"ok":true})),
    ));
    assert!(!app.online);
    assert!(app.request(&Action::Launch).is_none());
}
#[test]
fn ready_setup_reserves_launch_and_setup_inputs_until_started_or_cancelled() {
    let mut app = fixture();
    app.snapshot.get_mut("setup").expect("setup")["job"] =
        json!({"state":"ready", "mode":"install", "id":"ready-setup"});
    assert!(app.request(&Action::Launch).is_none());
    assert!(!app.can_edit_field("game_id"));
    assert!(app.request(&Action::SetupStart).is_some());
    assert!(app.request(&Action::SetupControl("cancel")).is_some());
}
#[test]
fn idle_polling_slows_down_but_work_and_reconnect_stay_responsive() {
    let mut app = fixture();
    assert_eq!(app.poll_interval(), Duration::from_secs(10));
    app.snapshot.get_mut("launcher-update").expect("updates")["job"] =
        json!({"state":"running","operation":"check"});
    assert_eq!(app.poll_interval(), Duration::from_secs(2));
    app = fixture();
    app.snapshot.get_mut("status").expect("status")["setup"]["busy"] = json!(true);
    assert_eq!(app.poll_interval(), Duration::from_secs(2));
    app = fixture();
    app.online = false;
    assert_eq!(app.poll_interval(), Duration::from_secs(2));
}
#[test]
fn report_edits_invalidate_export_and_mail_uses_only_the_reviewed_draft() {
    let mut app = fixture();
    app.snapshot.insert("problem-reports",json!({"recipient":"support@example.test", "draft":{"report":{"id":"local-draft", "description":"Übung & details?\nsecond line"}}}));
    let uri = app.mail_uri().expect("mail draft");
    assert!(uri.starts_with("mailto:support@example.test?subject="));
    assert!(uri.contains("%C3%9Cbung%20%26%20details%3F"));
    assert!(!uri.contains("\n"));
    let _ = app.update(Message::Field("category", "graphics".into()));
    assert!(app.report_text().is_none());
    assert!(app.mail_uri().is_none());
}
#[test]
fn diagnostics_export_excludes_unrelated_response_fields() {
    let mut app = fixture();
    app.snapshot.insert("diagnostics",json!({"summary":{"run_found":false},"checks":[],"generated_at":null,"csrf_token":"private-test-placeholder","unrelated":"must not export"}));
    let report = app.diagnostics_text().expect("report");
    assert!(!report.contains("csrf_token"));
    assert!(!report.contains("unrelated"));
}
#[test]
fn unknown_actions_are_rejected() {
    let app = fixture();
    for action in [
        Action::Fenix("unknown"),
        Action::Gsx("unknown"),
        Action::Cloud("unknown"),
        Action::Game("unknown"),
        Action::Launcher("unknown"),
    ] {
        assert!(app.request(&action).is_none());
    }
}
#[test]
fn every_screen_is_a_real_native_view_in_both_languages() -> Result<(), Box<dyn std::error::Error>>
{
    for language in [Language::De, Language::En] {
        for (page, de, en) in [
            (Page::Overview, "Simulator starten", "Start simulator"),
            (Page::Setup, "Simulator einrichten", "Set up simulator"),
            (Page::Updates, "Simulator-Updates", "Simulator updates"),
            (Page::Saves, "Cloud-Spielstände", "Cloud saves"),
            (Page::Mods, "Fenix A320", "Fenix A320"),
            (Page::Diagnostics, "Problem melden", "Report a problem"),
        ] {
            let mut app = fixture();
            app.language = language;
            app.page = page;
            let mut ui =
                iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
            ui.find(if language == Language::De { de } else { en })?;
        }
    }
    Ok(())
}
#[test]
fn native_launch_button_emits_the_real_action() -> Result<(), Box<dyn std::error::Error>> {
    let app = fixture();
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
    ui.click("Simulator starten")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Action(Action::Launch))
    ));
    Ok(())
}
#[test]
#[ignore = "Explicit native screenshot capture into build/native-ui"]
fn capture_all_native_screens() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/native-ui");
    std::fs::create_dir_all(&directory)?;
    for language in [Language::De, Language::En] {
        for (page, name) in [
            (Page::Overview, "overview"),
            (Page::Setup, "setup"),
            (Page::Updates, "updates"),
            (Page::Saves, "saves"),
            (Page::Mods, "mods"),
            (Page::Diagnostics, "diagnostics"),
        ] {
            for (width, height) in [(750, 900), (960, 700), (1536, 1024)] {
                let application = iced::application(
                    move || {
                        let mut app = common::localized_fixture(language);
                        app.page = page;
                        app
                    },
                    |_: &mut App, _: Message| Task::none(),
                    App::view,
                )
                .settings(settings())
                .theme(|_: &App| theme());
                let screenshot = iced_test::screenshot(
                    &application,
                    &theme(),
                    Size::new(width as f32, height as f32),
                    1.0,
                    Duration::from_millis(80),
                );
                image::save_buffer(
                    directory.join(format!("{name}-{}-{width}.png", language.code())),
                    &screenshot.rgba,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )?;
            }
        }
    }
    for (name, language, scale) in [
        ("overview-2020", Language::De, 1.0),
        ("confirmation", Language::En, 1.0),
        ("overview-hidpi", Language::En, 2.0),
        ("unconfigured", Language::En, 1.0),
    ] {
        let application =
            iced::application(
                move || {
                    let mut app = common::localized_fixture(language);
                    match name {
                        "overview-2020" => {
                            app.edition = Edition::Msfs2020;
                            app.snapshot.get_mut("status").expect("status")["runtime"]["game_id"] =
                                json!("msfs2020");
                            app.snapshot.get_mut("status").expect("status")["runtime"]["path"] =
                                json!("/synthetic/msfs2020");
                            app.snapshot.get_mut("status").expect("status")["runtime"]["checks"]
                                [0]["detail"] =
                                json!("MSFS 2020 und MicrosoftGame.Config vorhanden.");
                        }
                        "confirmation" => {
                            let _ = app.update(Message::Action(Action::Cloud("import")));
                        }
                        "unconfigured" => {
                            app.snapshot.get_mut("status").expect("status")["runtime"] =
                                json!({"configured":false,"ready":false,"checks":[]});
                            app.snapshot.get_mut("status").expect("status")["game"]["can_start"] =
                                json!(false);
                            app.snapshot.get_mut("status").expect("status")["cloud"]["enabled"] =
                                json!(false);
                            app.page = Page::Setup;
                        }
                        _ => {}
                    }
                    app
                },
                |_: &mut App, _: Message| Task::none(),
                App::view,
            )
            .settings(settings())
            .theme(|_: &App| theme());
        let screenshot = iced_test::screenshot(
            &application,
            &theme(),
            Size::new(1280.0, 900.0),
            scale,
            Duration::from_millis(80),
        );
        image::save_buffer(
            directory.join(format!("{name}.png")),
            &screenshot.rgba,
            (1280.0 * scale) as u32,
            (900.0 * scale) as u32,
            image::ColorType::Rgba8,
        )?;
    }
    Ok(())
}
