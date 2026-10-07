// SPDX-License-Identifier: MIT
//! Admission tests use incomplete local packages, so no network helper can run.
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, files, game_update, launcher_update, startup_updates};
use serde_json::json;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

#[test]
fn automatic_checks_keep_recent_results_and_defer_without_duplicate_workers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    files::private_dir(&root.join("tools")).unwrap();
    files::private_dir(&root.join("private")).unwrap();
    files::atomic(&root.join("tools/play-msfs.sh"), b"fixture").unwrap();
    let app = Launcher::new(temp.path().join("state"), Some(root.to_str().unwrap())).unwrap();
    let launcher_worker = Arc::new(AtomicBool::new(false));
    {
        let mut state = app.lock();
        // Represent an already-running public-release check. This keeps all
        // calls account-free and proves re-admission preserves its identity.
        state.launcher_updates.worker = Some(Arc::clone(&launcher_worker));
        state.launcher_updates.attempted = Some(Instant::now() - Duration::from_secs(3600));
        state.launcher_updates.job = json!({"id":"existing","operation":"check","state":"running"});
    }
    launcher_update::check_on_startup(&app).unwrap();
    assert_eq!(app.lock().launcher_updates.job["id"], "existing");
    assert!(Arc::ptr_eq(
        app.lock().launcher_updates.worker.as_ref().unwrap(),
        &launcher_worker
    ));
    for (status, seconds) in [("failed", 299), ("complete", 1799)] {
        let attempt = Instant::now() - Duration::from_secs(seconds);
        app.lock().startup_updates.records.insert(
            root.clone(),
            startup_updates::Record {
                attempt,
                value: json!({"state":status,"retained":true}),
            },
        );
        assert_eq!(startup_updates::check(&app).unwrap(), json!({"ok":true}));
        let state = app.lock();
        assert_eq!(state.startup_updates.records[&root].attempt, attempt);
        assert_eq!(state.startup_updates.records[&root].value["retained"], true);
        assert!(state.startup_updates.worker.is_none());
        assert!(state.active.is_none());
    }
    let old = Instant::now() - Duration::from_secs(301);
    app.lock().startup_updates.records.insert(
        root.clone(),
        startup_updates::Record {
            attempt: old,
            value: json!({"state":"failed"}),
        },
    );
    let reservation = app.reserve("fixture", "busy", true).unwrap();
    assert_eq!(startup_updates::check(&app).unwrap()["deferred"], true);
    assert_eq!(app.lock().startup_updates.records[&root].attempt, old);
    app.finish(&reservation, Ok(json!({"state":"complete"})), false);
    let game_worker = Arc::new(AtomicBool::new(false));
    app.lock().startup_updates.worker = Some(Arc::clone(&game_worker));
    assert_eq!(startup_updates::check(&app).unwrap()["deferred"], true);
    assert!(Arc::ptr_eq(
        app.lock().startup_updates.worker.as_ref().unwrap(),
        &game_worker
    ));
    assert_eq!(app.lock().startup_updates.records[&root].attempt, old);
    app.lock().startup_updates.worker = None;
    startup_updates::check(&app).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.lock().startup_updates.worker.is_some() {
        assert!(Instant::now() < deadline, "local metadata check timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = app.lock();
    assert!(state.startup_updates.records[&root].attempt > old);
    assert_eq!(
        state.startup_updates.records[&root].value["state"],
        "failed"
    );
    assert!(state.active.is_none());
    assert_eq!(state.launcher_updates.job["id"], "existing");
}

#[test]
fn terminal_manual_checks_do_not_suppress_automatic_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    files::private_dir(&root.join("tools")).unwrap();
    files::private_dir(&root.join("private")).unwrap();
    files::atomic(&root.join("tools/play-msfs.sh"), b"fixture").unwrap();
    let app = Launcher::new(temp.path().join("state"), Some(root.to_str().unwrap())).unwrap();
    app.lock().launcher_updates.worker = Some(Arc::new(AtomicBool::new(false)));
    for status in [
        "ready",
        "checking",
        "installing",
        "failed",
        "cancelled",
        "complete",
    ] {
        let job =
            json!({"mode":"update","runtime_path":root,"state":status,"error":"retained history"});
        {
            let mut state = app.lock();
            state.jobs.insert("setup".into(), job.clone());
            state.startup_updates.records.clear();
        }
        startup_updates::check(&app).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.lock().startup_updates.worker.is_some() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let state = app.lock();
        let terminal = ["failed", "cancelled", "complete"].contains(&status);
        assert_eq!(
            state.startup_updates.records.contains_key(&root),
            terminal,
            "{status}"
        );
        assert_eq!(state.jobs["setup"], job);
        assert!(state.active.is_none());
    }
}

#[test]
fn newer_background_metadata_overrides_terminal_summary_without_erasing_history() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    for directory in ["tools", "private", "games/MSFS2024", "bin"] {
        files::private_dir(&root.join(directory)).unwrap();
    }
    files::atomic(&root.join("tools/play-msfs.sh"), b"fixture").unwrap();
    files::atomic(&root.join("bin/xodus-cli"), b"not executed").unwrap();
    files::atomic(&root.join("games/MSFS2024/MicrosoftGame.Config"), br#"<Game><Identity Name="Test.Game" Publisher="CN=Test" Version="1.0.0.0"/><StoreId>9P38D19T7LRV</StoreId></Game>"#).unwrap();
    let app = Launcher::new(temp.path().join("state"), Some(root.to_str().unwrap())).unwrap();
    for status in ["failed", "cancelled", "complete"] {
        for available in [false, true] {
            let job = json!({"mode":"update","runtime_path":root,"state":status,"completed_at":"2026-01-01T12:00:00Z","error":"retained detail","latest_version":"1.0.0.0","update_available":!available});
            let discovered = json!({"state":"complete","installed_version":"1.0.0.0","latest_version":if available{"2.0.0.0"}else{"1.0.0.0"},"update_available":available,"checked_at":"2026-01-01T12:01:00Z"});
            {
                let mut state = app.lock();
                state.jobs.insert("setup".into(), job.clone());
                state.startup_updates.records.insert(
                    root.clone(),
                    startup_updates::Record {
                        attempt: Instant::now(),
                        value: discovered.clone(),
                    },
                );
            }
            let view = game_update::snapshot(&app);
            assert_eq!(view["available"], true, "{view}");
            assert_eq!(view["update_available"], available);
            assert_eq!(view["latest_version"], discovered["latest_version"]);
            assert_eq!(view["background_current"], true);
            assert_eq!(view["can_start"], false);
            assert_eq!(view["job"], job);
            for (key, value) in [
                ("checked_at", json!("2026-01-01T11:59:00Z")),
                ("installed_version", json!("0.9.0.0")),
            ] {
                let mut stale = discovered.clone();
                stale[key] = value;
                app.lock()
                    .startup_updates
                    .records
                    .get_mut(&root)
                    .unwrap()
                    .value = stale;
                let view = game_update::snapshot(&app);
                assert_eq!(view["background_current"], false);
                assert_eq!(view["update_available"], !available);
                assert_eq!(view["job"], job);
            }
            let mut older_attempt = discovered.clone();
            older_attempt["started_at"] = json!("2026-01-01T11:59:00Z");
            app.lock()
                .startup_updates
                .records
                .get_mut(&root)
                .unwrap()
                .value = older_attempt;
            assert_eq!(game_update::snapshot(&app)["background_current"], false);
            for background_state in ["checking", "failed"] {
                app.lock()
                    .startup_updates
                    .records
                    .get_mut(&root)
                    .unwrap()
                    .value = json!({
                    "state":background_state,"started_at":"2026-01-01T12:01:00Z",
                    "auth_required":background_state=="failed",
                    "error":if background_state=="failed"{"new check failed"}else{""}
                });
                let view = game_update::snapshot(&app);
                assert_eq!(view["background_current"], true);
                assert_eq!(view["background_checking"], background_state == "checking");
                assert_eq!(view["auth_required"], background_state == "failed");
                assert_eq!(view["update_available"], !available);
                assert_eq!(view["job"], job);
            }
            app.lock()
                .startup_updates
                .records
                .get_mut(&root)
                .unwrap()
                .value = discovered;
            app.lock().jobs.get_mut("setup").unwrap()["state"] = json!("ready");
            assert_eq!(game_update::snapshot(&app)["background_current"], false);
        }
    }
}
