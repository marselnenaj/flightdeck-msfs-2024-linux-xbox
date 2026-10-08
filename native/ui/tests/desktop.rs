use flightdeck_ui::{Action, App, Disclosure, Edition, Language, Message, Page, settings, theme};
use iced::{Size, Task};
use serde_json::json;
use std::time::Duration;

mod common;
use common::fixture;
// iced's stock pick-list menu does not expose text through widget operations.
// Select through real pointer events using the located control's row geometry.
fn choose(
    ui: &mut iced_test::Simulator<'_, Message>,
    field: &str,
    index: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let bounds = ui
        .click(iced_test::selector::id(field.to_string()))?
        .bounds();
    let point = iced::Point::new(
        bounds.center_x(),
        bounds.y + bounds.height * (index as f32 + 1.5),
    );
    ui.point_at(point);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: point,
    })]);
    ui.simulate(iced_test::simulator::click());
    Ok(())
}

#[test]
fn legacy_fenix_is_presented_as_an_existing_installation() -> Result<(), Box<dyn std::error::Error>>
{
    for language in [Language::De, Language::En] {
        let mut app = fixture();
        app.page = Page::Mods;
        app.language = language;
        app.snapshot.insert("fenix",json!({"state":"legacy","runtime_path":"/synthetic/msfs2024","installed":false,"fenix_installed":true,"settings_ready":true,"configured":false,"manager_installed":true,"idle":true,"can_change":true}));
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 1600.0), app.view());
        ui.find(if language == Language::De {
            "Vorhandene Fenix-Einrichtung"
        } else {
            "Existing Fenix setup"
        })?;
        ui.click("Fenix A320")?;
        let _ = app.update(ui.into_messages().next().expect("open Fenix"));
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(960.0, 1600.0), app.view());
        assert!(
            ui.find(if language == Language::De {
                "Patch einrichten"
            } else {
                "Set up patch"
            })
            .is_err()
        );
        ui.click(if language == Language::De {
            "Fenix öffnen"
        } else {
            "Open Fenix"
        })?;
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::Action(Action::Fenix("open")))
        ));
        assert!(app.request(&Action::Fenix("install")).is_none());
        assert!(app.request(&Action::Fenix("manager")).is_some());
    }
    Ok(())
}
#[test]
fn managed_fenix_readiness_requires_the_patch_and_configuration()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture();
    app.page = Page::Mods;
    app.language = Language::En;
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.find("Fenix locally configured")?;
    drop(ui);
    app.snapshot.get_mut("fenix").expect("fenix")["configured"] = json!(false);
    let _ = app.update(Message::Toggle(Disclosure::Fenix));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(960.0, 900.0), app.view());
    ui.find("Setup is not complete yet")?;
    assert!(ui.find("Fenix locally configured").is_err());
    ui.click("Finish setup")?;
    assert!(matches!(
        ui.into_messages().next(),
        Some(Message::Action(Action::Fenix("configure")))
    ));
    Ok(())
}
#[test]
fn proton_discovery_selects_the_active_runner_and_preserves_unapplied_choices() {
    let mut app = fixture();
    let mut snapshot = app.snapshot.clone();
    snapshot.get_mut("proton").expect("proton")["experimental"] = json!(true);
    snapshot.get_mut("proton").expect("proton")["selected"] = json!("cachyos-10.0-sunset");
    snapshot.get_mut("proton").expect("proton")["selected_path"] =
        json!("/synthetic/Steam/proton-cachyos");
    snapshot.get_mut("proton").expect("proton")["can_restore"] = json!(true);
    let _ = app.update(Message::Loaded(0, Ok(snapshot.clone())));
    assert_eq!(app.forms.get("proton"), "/synthetic/Steam/proton-cachyos");
    let _ = app.update(Message::Field("proton", "custom".into()));
    let _ = app.update(Message::Field(
        "proton_path",
        "/synthetic/other Proton".into(),
    ));
    let _ = app.update(Message::Loaded(0, Ok(snapshot)));
    assert_eq!(app.forms.get("proton"), "custom");
    assert_eq!(
        app.request(&Action::Proton).expect("custom").body["path"],
        "/synthetic/other Proton"
    );
    let mut changed = fixture().snapshot;
    changed.get_mut("status").expect("status")["runtime"]["path"] = json!("/synthetic/msfs2020");
    changed.get_mut("proton").expect("proton")["runtime_path"] = json!("/synthetic/msfs2020");
    let _ = app.update(Message::Loaded(0, Ok(changed)));
    assert_eq!(app.forms.get("proton"), "default");
    assert_eq!(app.forms.get("proton_path"), "");
    assert!(app.discoveries.is_empty());
}
#[test]
fn proton_fenix_compatibility_and_cloud_jobs_guard_runner_switches() {
    let mut app = fixture();
    app.snapshot.get_mut("proton").expect("proton")["fenix"] = json!(true);
    let _ = app.update(Message::Field(
        "proton",
        "/synthetic/Steam/Proton - Experimental".into(),
    ));
    assert_eq!(app.forms.get("proton"), "default");
    // Even an injected selection cannot bypass the action guard.
    app.forms
        .text
        .insert("proton", "/synthetic/Steam/Proton - Experimental".into());
    assert!(app.request(&Action::Proton).is_none());
    let _ = app.update(Message::Field(
        "proton",
        "/synthetic/Steam/proton-cachyos".into(),
    ));
    assert!(app.request(&Action::Proton).is_some());
    app.snapshot.get_mut("proton").expect("proton")["can_restore"] = json!(true);
    app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("syncing");
    assert!(app.request(&Action::Proton).is_none());
    assert!(app.request(&Action::ProtonDefault).is_none());
}
#[test]
fn proton_menu_contains_default_experimental_and_cachyos_without_manual_search()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture();
    app.page = Page::Setup;
    app.language = Language::En;
    let _ = app.update(Message::Toggle(Disclosure::Proton));
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 1500.0), app.view());
    ui.find("Flightdeck (Xodus)")?;
    assert!(ui.find("Proton folder").is_err());
    choose(&mut ui, "proton", 1)?;
    let _ = app.update(ui.into_messages().next().expect("Proton selection"));
    assert_eq!(
        app.request(&Action::Proton).expect("switch runner").body["path"],
        "/synthetic/Steam/Proton - Experimental"
    );
    let _ = app.update(Message::Toggle(Disclosure::Vr));
    assert!(!app.expanded.contains(&Disclosure::Proton));
    Ok(())
}
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
    for (_name, id) in [
        ("Microsoft Flight Simulator 2020", "msfs2020"),
        ("Microsoft Flight Simulator 2024", "msfs2024"),
    ] {
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 2200.0), app.view());
        choose(&mut ui, "game_id", if id == "msfs2020" { 1 } else { 0 })?;
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
            (Page::Setup, "MSFS installieren", "Install MSFS"),
            (
                Page::Updates,
                "Flightdeck · GitHub-Releases",
                "Flightdeck · GitHub releases",
            ),
            (Page::Saves, "Deine Spielstände", "Your saves"),
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
fn launch_acknowledges_immediately_and_keeps_feedback_until_status_arrives() {
    let mut app = fixture();
    app.language = Language::En;
    app.client = Some(
        flightdeck_ui::Client::new(1, "synthetic-session-0000000000000000".into()).expect("client"),
    );
    let _request = app.update(Message::Action(Action::Launch));
    assert!(app.pending);
    assert_eq!(app.launch_label(), "Preparing launch …");
    assert_eq!(app.launch_state(), "Preparing launch …");
    assert!(app.launch_note().contains("Launch requested"));
    assert!(app.launch_message().is_none());
    let _refresh = app.update(Message::Completed(
        1,
        Action::Launch,
        Ok(json!({"ok":true})),
    ));
    assert_eq!(app.launch_label(), "Preparing launch …");
    let mut snapshot = fixture().snapshot;
    snapshot.get_mut("status").expect("status")["cloud"] = json!({"enabled":true,"state":"syncing","phase":"before_start","message":"Comparing your saves. The simulator will start automatically."});
    let _ = app.update(Message::Loaded(1, Ok(snapshot.clone())));
    assert_eq!(app.launch_label(), "Syncing saves …");
    assert!(app.launch_note().contains("start automatically"));
    assert!(app.launch_message().is_none());
    snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("playing");
    snapshot.get_mut("status").expect("status")["game"] =
        json!({"state":"running","managed":true,"can_start":false,"can_stop":true});
    let _ = app.update(Message::Loaded(1, Ok(snapshot)));
    assert_eq!(app.launch_state(), "Simulator is running");
    assert_eq!(app.launch_label(), "Stop simulator");
}
#[test]
fn native_stop_click_keeps_shutdown_feedback_until_the_game_has_exited()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture();
    app.language = Language::En;
    app.client = Some(flightdeck_ui::Client::new(
        1,
        "synthetic-session-0000000000000000".into(),
    )?);
    app.snapshot.get_mut("status").expect("status")["game"] =
        json!({"state":"running","managed":true,"can_start":false,"can_stop":true});
    let request = app.request(&Action::Stop).expect("managed stop request");
    assert_eq!(request.path, "stop");
    assert_eq!(request.runtime, "/synthetic/msfs2024");
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
    ui.click("Stop simulator")?;
    let action = ui.into_messages().next().expect("stop button action");
    assert!(matches!(action, Message::Action(Action::Stop)));
    let _ = app.update(action);
    assert!(app.pending);
    assert_eq!(app.launch_label(), "Stopping simulator …");
    assert_eq!(app.launch_state(), "Stopping simulator …");
    assert!(app.launch_message().is_none());

    let _ = app.update(Message::Completed(1, Action::Stop, Ok(json!({"ok":true}))));
    assert!(!app.pending);
    assert_eq!(app.launch_label(), "Stopping simulator …");
    assert_eq!(app.launch_state(), "Stopping simulator …");
    let mut snapshot = app.snapshot.clone();
    snapshot.get_mut("status").expect("status")["game"] =
        json!({"state":"stopping","managed":true,"can_start":false,"can_stop":false});
    let _ = app.update(Message::Loaded(1, Ok(snapshot)));
    assert_eq!(app.launch_label(), "Stopping simulator …");
    assert_eq!(app.launch_state(), "Stopping simulator …");
    assert!(app.launch_note().contains("processes to close"));
    assert!(app.launch_message().is_none());

    let _ = app.update(Message::Loaded(1, Ok(fixture().snapshot)));
    assert_eq!(app.launch_label(), "Start simulator");
    assert_eq!(app.launch_state(), "Ready to start");
    assert!(matches!(
        app.launch_message(),
        Some(Message::Action(Action::Launch))
    ));
    Ok(())
}
#[test]
fn tabs_keep_cached_data_and_connectivity_while_refreshing() {
    let mut app = fixture();
    for page in [Page::Mods, Page::Setup, Page::Overview] {
        let _ = app.update(Message::Navigate(page));
        assert_eq!(app.page, page);
        assert!(app.online);
        assert!(app.fresh("fenix"));
        assert!(app.request(&Action::Launch).is_some());
    }
}
#[test]
fn existing_unready_edition_is_selected_instead_of_opening_a_new_installation() {
    let mut app = fixture();
    app.client = Some(
        flightdeck_ui::Client::new(1, "synthetic-session-0000000000000000".into()).expect("client"),
    );
    app.snapshot.get_mut("status").expect("status")["versions"]["msfs2020"] =
        json!({"installed":true,"ready":false,"path":"/synthetic/msfs2020"});
    let _request = app.update(Message::Select(Edition::Msfs2020));
    assert!(app.pending);
    assert_eq!(app.page, Page::Overview);
}
#[test]
fn inactive_cloud_attention_explains_blocked_launch_and_allows_another_simulator()
-> Result<(), Box<dyn std::error::Error>> {
    for (language, blocked) in [
        (Language::De, "Start blockiert"),
        (Language::En, "Launch blocked"),
    ] {
        let mut app = fixture();
        app.language = language;
        app.client = Some(flightdeck_ui::Client::new(
            1,
            "synthetic-session-0000000000000000".into(),
        )?);
        let reason = "The previous simulator session was interrupted.";
        app.snapshot.get_mut("status").expect("status")["cloud"] = json!({
            "state":"attention", "enabled":true, "error_code":"unsafe_session",
            "message":reason, "can_retry":false, "can_cancel":false,
            "can_play_local":false, "can_signin":false
        });
        app.snapshot.get_mut("status").expect("status")["versions"]["msfs2020"] =
            json!({"installed":true,"ready":true,"path":"/synthetic/msfs2020"});
        assert!(app.request(&Action::Launch).is_none());
        assert!(app.request(&Action::Select(Edition::Msfs2020)).is_some());
        assert_eq!(app.launch_label(), blocked);
        assert!(app.launch_state().starts_with(blocked));
        assert_eq!(app.launch_note(), reason);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
        ui.click(blocked)?;
        assert!(ui.into_messages().next().is_none());
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1280.0, 900.0), app.view());
        ui.click("MSFS 2020")?;
        let action = ui.into_messages().next().expect("edition switch");
        assert!(matches!(action, Message::Select(Edition::Msfs2020)));
        let _ = app.update(action);
        assert!(app.pending);
        assert_eq!(app.edition, Edition::Msfs2024);
    }
    Ok(())
}
#[test]
fn simulator_selection_remains_locked_during_active_sessions_and_work() {
    for state in ["syncing", "playing"] {
        let mut app = fixture();
        app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!(state);
        assert!(app.request(&Action::Select(Edition::Msfs2020)).is_none());
    }
    for state in ["starting", "running", "stopping", "external", "unknown"] {
        let mut app = fixture();
        app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("attention");
        app.snapshot.get_mut("status").expect("status")["game"]["state"] = json!(state);
        assert!(app.request(&Action::Select(Edition::Msfs2020)).is_none());
    }
    let mut app = fixture();
    app.snapshot.get_mut("status").expect("status")["cloud"]["state"] = json!("attention");
    app.snapshot.get_mut("status").expect("status")["setup"]["busy"] = json!(true);
    assert!(app.request(&Action::Select(Edition::Msfs2020)).is_none());
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
        ("fenix-legacy", Language::En, 1.0),
        ("fenix-ready", Language::En, 1.0),
        ("fenix-manager-en", Language::En, 1.0),
        ("fenix-manager-de", Language::De, 1.0),
        ("proton-choices", Language::En, 1.0),
        ("mod-removal", Language::De, 1.0),
        ("mod-link-removal", Language::En, 1.0),
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
                        "fenix-legacy" | "fenix-ready" => {
                            app.page = Page::Mods;
                            if name == "fenix-legacy" {
                                app.snapshot.get_mut("fenix").expect("fenix")["state"] =
                                    json!("legacy");
                                app.snapshot.get_mut("fenix").expect("fenix")["installed"] =
                                    json!(false);
                                app.snapshot.get_mut("fenix").expect("fenix")["configured"] =
                                    json!(false);
                            }
                            let _ = app.update(Message::Toggle(Disclosure::Fenix));
                        }
                        "fenix-manager-en" | "fenix-manager-de" => {
                            app.page = Page::Mods;
                            app.snapshot.insert("fenix", json!({
                                "state":"installed", "runtime_path":"/synthetic/msfs2024",
                                "installed":true, "manager_installed":true,
                                "fenix_installed":false, "settings_ready":false,
                                "configured":false, "idle":true, "can_change":true,
                                "can_restore":true, "can_repair_installer":true,
                                "job":{
                                    "state":"failed", "operation":"installer",
                                    "message":if language == Language::De {
                                        "Der vorherige Fenix-Installationsschritt ist fehlgeschlagen (synthetischer Test)."
                                    } else {
                                        "The previous Fenix install hook failed (synthetic test)."
                                    }
                                }
                            }));
                            let _ = app.update(Message::Toggle(Disclosure::Fenix));
                        }
                        "mod-removal" | "mod-link-removal" => {
                            app.page=Page::Mods;
                            app.snapshot.get_mut("mods").expect("mods")["count"]=json!(1);
                            app.snapshot.get_mut("mods").expect("mods")["mods"]=json!([{"id":"synthetic-aircraft","name":"Synthetic aircraft","version":"1.0"}]);
                            app.snapshot.get_mut("status").expect("status")["setup"]["busy"]=json!(true);
                            app.snapshot.get_mut("mods").expect("mods")["job"]=json!({"id":"reviewed-mod","runtime_path":"/synthetic/msfs2024","operation":"remove","state":"ready","addon_id":"synthetic-aircraft","entry_path":"/synthetic/Community/synthetic-aircraft","bytes":1024,"is_link":name=="mod-link-removal"});
                        }
                        "proton-choices" => {
                            app.page = Page::Setup;
                            let _ = app.update(Message::Toggle(Disclosure::Proton));
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
        // Capture the actual viewport; the current action and error must fit.
        let height = 900.0;
        let screenshot = iced_test::screenshot(
            &application,
            &theme(),
            Size::new(1280.0, height),
            scale,
            Duration::from_millis(80),
        );
        image::save_buffer(
            directory.join(format!("{name}.png")),
            &screenshot.rgba,
            (1280.0 * scale) as u32,
            (height * scale) as u32,
            image::ColorType::Rgba8,
        )?;
    }
    Ok(())
}

#[test]
#[ignore = "Explicit comparison with the historical web UI's synthetic API fixture"]
fn capture_parity_screens() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/ui-parity");
    std::fs::create_dir_all(&directory)?;
    for (page, name, expanded) in [
        (Page::Overview, "overview", None),
        (Page::Setup, "setup", None),
        (Page::Mods, "mods", None),
        (Page::Updates, "updates", None),
        (Page::Saves, "saves", None),
        (Page::Diagnostics, "diagnostics", None),
        (Page::Setup, "proton-card", Some(Disclosure::Proton)),
        (Page::Mods, "fenix-card", Some(Disclosure::Fenix)),
    ] {
        let application = iced::application(
            move || {
                let values: serde_json::Value =
                    serde_json::from_str(include_str!("fixtures/web-0.2.2.json"))
                        .expect("historical synthetic status");
                let mut app = App::default();
                app.startup_checked = true;
                app.page = page;
                let mut snapshot = flightdeck_ui::Snapshot::new();
                for key in [
                    "status",
                    "setup",
                    "setup/discover",
                    "proton",
                    "proton/discover",
                    "maintenance",
                    "fenix",
                    "gsx",
                    "mods",
                    "launcher-update",
                    "game-update",
                    "cloud-saves",
                    "diagnostics",
                    "store-check",
                    "problem-reports",
                ] {
                    snapshot.insert(key, values[key].clone());
                }
                let _ = app.update(Message::Loaded(0, Ok(snapshot)));
                if let Some(section) = expanded {
                    let _ = app.update(Message::Toggle(section));
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
            Size::new(1536.0, 1024.0),
            1.0,
            Duration::from_millis(80),
        );
        image::save_buffer(
            directory.join(format!("native-{name}.png")),
            &screenshot.rgba,
            1536,
            1024,
            image::ColorType::Rgba8,
        )?;
    }
    Ok(())
}

#[test]
fn setup_guide_layout_is_visible_at_desktop_width() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture();
    app.page = Page::Setup;
    let values: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/web-0.2.2.json")).expect("fixture");
    for key in ["status", "setup"] {
        app.snapshot.insert(key, values[key].clone());
    }
    app.forms.text.insert("mode", "existing".into());
    let mut ui = iced_test::Simulator::with_size(settings(), Size::new(1536.0, 1024.0), app.view());
    for text in [
        "In drei Schritten",
        "Deine Installation vorbereiten",
        "Verbinden",
    ] {
        let target = ui.find(text)?;
        assert!(target.bounds().width > 10.0);
        assert!(target.visible_bounds().is_some());
    }
    Ok(())
}

#[test]
fn mod_uninstall_reviews_the_exact_entry_and_rejects_changed_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    for language in [Language::De, Language::En] {
        let mut app = fixture();
        app.language = language;
        app.page = Page::Mods;
        app.snapshot.get_mut("mods").expect("mods")["mods"] =
            json!([{"id":"synthetic-aircraft","name":"Synthetic aircraft","version":"1.0"}]);
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1200.0, 1700.0), app.view());
        ui.click(if language == Language::De {
            "Deinstallation prüfen"
        } else {
            "Review uninstallation"
        })?;
        let Some(Message::Action(action)) = ui.into_messages().next() else {
            panic!("review action");
        };
        let request = app.request(&action).expect("review available");
        assert_eq!(request.path, "mods/preview-remove");
        assert_eq!(request.body["addon_id"], "synthetic-aircraft");
        app.snapshot.get_mut("mods").expect("mods")["job"] = json!({"id":"reviewed-mod","runtime_path":"/synthetic/msfs2024","operation":"remove","state":"ready","addon_id":"synthetic-aircraft","entry_path":"/synthetic/Community/synthetic-aircraft","bytes":1024,"is_link":false});
        app.snapshot.get_mut("status").expect("status")["setup"]["busy"] = json!(true);
        assert!(app.request(&Action::Launch).is_none());
        assert!(
            app.request(&Action::ModsPreview("synthetic-aircraft".into()))
                .is_none()
        );
        assert!(app.request(&Action::ModsDiscard).is_some());
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1200.0, 1700.0), app.view());
        ui.click(if language == Language::De {
            "Deinstallieren"
        } else {
            "Uninstall"
        })?;
        let _ = app.update(ui.into_messages().next().expect("remove action"));
        assert!(app.confirmation.is_some());
        let mut ui =
            iced_test::Simulator::with_size(settings(), Size::new(1200.0, 1700.0), app.view());
        ui.find("/synthetic/Community/synthetic-aircraft")?;
        drop(ui);
        app.snapshot.get_mut("mods").expect("mods")["job"]["id"] = json!("replacement-review");
        let _ = app.update(Message::Confirm);
        assert!(!app.pending);
        assert!(app.confirmation.is_none());
        assert!(app.notice.is_some());
        app.snapshot.get_mut("mods").expect("mods")["job"]["runtime_path"] =
            json!("/synthetic/another-runtime");
        assert!(app.request(&Action::ModsRemove).is_none());
    }
    Ok(())
}
