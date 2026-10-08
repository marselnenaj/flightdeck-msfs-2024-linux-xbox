// SPDX-License-Identifier: MIT
//! Persisted, sanitized Fenix feedback without Wine or an installed simulator.
use flightdeck::{backend::Launcher, fenix, fenix_diagnostics as fd, files};
use serde_json::json;

#[test]
fn snapshot_reloads_only_validated_evidence_for_the_selected_runtime() {
    let root = tempfile::tempdir().expect("runtime");
    files::private_dir(&root.path().join("private")).expect("private");
    let mut attempt = fd::begin(root.path(), "repair", "flightdeck").expect("begin");
    attempt.status = "failed".into();
    attempt.completed_at = Some(files::now());
    attempt.failure = Some("hook_nonzero".into());
    attempt.package_version = Some("1.0.286".into());
    attempt.hook = Some("install".into());
    attempt.hook_exit_code = Some(82);
    attempt.process_exit_code = Some(82);
    attempt.evidence_complete = true;
    fd::save(root.path(), &attempt).expect("failure evidence");

    // A newly created service has no in-memory Fenix job. Even incomplete
    // runtime setup must not hide its validated latest installer evidence.
    let state = tempfile::tempdir().expect("service state");
    let app = Launcher::new(state.path().join("state"), None).expect("launcher");
    assert!(app.lock().jobs.is_empty());
    app.lock().runtime = Some(root.path().into());
    let snapshot = fenix::snapshot(&app);
    assert!(snapshot["job"].is_null());
    assert_eq!(snapshot["last_attempt"], fd::load(root.path()));
    assert_eq!(snapshot["last_attempt"]["status"], "failed");
    assert_eq!(snapshot["last_attempt"]["hook_exit_code"], 82);

    let other = tempfile::tempdir().expect("other runtime");
    app.lock().runtime = Some(other.path().into());
    assert!(fenix::snapshot(&app)["last_attempt"].is_null());
    app.lock().runtime = None;
    assert!(fenix::snapshot(&app)["last_attempt"].is_null());

    app.lock().runtime = Some(root.path().into());
    let mut untrusted = serde_json::to_value(attempt).expect("record");
    untrusted["raw_log"] = json!("PRIVATE_LOG_MARKER");
    files::atomic_json(
        &root.path().join("private/fenix-installer-result.json"),
        &untrusted,
    )
    .expect("invalid record");
    let snapshot = fenix::snapshot(&app);
    assert!(snapshot["last_attempt"].is_null());
    assert!(!snapshot.to_string().contains("PRIVATE_LOG_MARKER"));
}

#[test]
fn only_proven_newer_jobs_supersede_the_recorded_attempt() {
    let root = tempfile::tempdir().expect("runtime");
    files::private_dir(&root.path().join("private")).expect("private");
    let mut attempt = fd::begin(root.path(), "repair", "flightdeck").expect("begin");
    attempt.started_at = "2026-10-08T10:00:00Z".into();
    attempt.completed_at = Some("2026-10-08T10:00:02.1Z".into());
    attempt.status = "failed".into();
    attempt.failure = Some("hook_nonzero".into());
    files::atomic_json(
        &root.path().join("private/fenix-installer-result.json"),
        &serde_json::to_value(attempt).expect("attempt"),
    )
    .expect("record");
    assert_eq!(fd::load(root.path())["status"], "failed");
    let state = tempfile::tempdir().expect("service state");
    let app = Launcher::new(state.path().join("state"), None).expect("launcher");
    app.lock().runtime = Some(root.path().into());
    for (mut job, superseded) in [
        (
            json!({"state":"complete", "started_at":"2026-10-08T09:00:00Z", "completed_at":"2026-10-08T09:00:02Z"}),
            false,
        ),
        // Completing after this attempt started is not sufficient: the stored
        // failure may have been recorded later than that job's completion.
        (
            json!({"state":"complete", "completed_at":"2026-10-08T10:00:01Z"}),
            false,
        ),
        (
            json!({"state":"complete", "completed_at":"2026-10-08T10:00:02.100+00:00"}),
            true,
        ),
        (
            json!({"state":"failed", "started_at":"2026-10-08T10:00:03Z"}),
            true,
        ),
        (json!({"state":"complete"}), false),
        (json!({"state":"complete", "completed_at":"invalid"}), false),
    ] {
        job["operation"] = json!("repair");
        job["runtime_path"] = json!(root.path());
        app.lock().jobs.insert("fenix".into(), job);
        let snapshot = fenix::snapshot(&app);
        assert_eq!(snapshot["last_attempt_superseded"], superseded);
        assert_eq!(snapshot["last_attempt"]["status"], "failed");
    }
}
