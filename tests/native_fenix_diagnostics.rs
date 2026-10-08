// SPDX-License-Identifier: MIT
//! Synthetic installer evidence only; no Wine, installer, account or game.
use flightdeck::{backend::Launcher, fenix_diagnostics as fd, files, problem_reports};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::symlink, path::Path};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("runtime");
    files::private_dir(&root.path().join("private")).expect("private");
    files::atomic_json(
        &root.path().join("private/runtime.json"),
        &json!({"game_id":"msfs2024"}),
    )
    .expect("identity");
    root
}

fn record(root: &Path) -> std::path::PathBuf {
    root.join("private/fenix-installer-result.json")
}

fn failed(root: &Path) -> fd::Attempt {
    let mut attempt = fd::begin(root, "installer", "flightdeck").expect("begin");
    attempt.completed_at = Some(files::now());
    attempt.status = "failed".into();
    attempt.failure = Some("hook_nonzero".into());
    attempt.package_version = Some("1.0.286".into());
    attempt.hook = Some("install".into());
    attempt.hook_exit_code = Some(3);
    // A successful bootstrapper does not establish a successful hook.
    attempt.process_exit_code = Some(0);
    attempt.evidence_complete = true;
    fd::save(root, &attempt).expect("failure evidence");
    attempt
}

#[test]
fn current_failure_survives_reload_and_retry_replaces_older_evidence() {
    let root = fixture();
    let failed = failed(root.path());
    let evidence = fd::load(root.path());
    assert_eq!(evidence["failure"], "hook_nonzero");
    assert_eq!(evidence["hook_exit_code"], 3);
    assert_eq!(evidence["process_exit_code"], 0);
    assert_eq!(evidence["scope"], "latest_recorded_attempt");
    assert_eq!(evidence["completed_at"], json!(failed.completed_at));

    let mut retry = fd::begin(root.path(), "repair", "experimental").expect("retry");
    assert_eq!(fd::load(root.path())["status"], "running");
    assert!(fd::load(root.path())["failure"].is_null());
    assert!(fd::load(root.path())["package_version"].is_null());
    assert!(fd::save(root.path(), &failed).is_err());
    retry.status = "succeeded".into();
    retry.completed_at = Some(files::now());
    retry.package_version = Some("1.0.286".into());
    retry.hook = Some("install".into());
    retry.hook_exit_code = Some(0);
    retry.evidence_complete = true;
    fd::save(root.path(), &retry).expect("successful repair");
    assert_eq!(fd::load(root.path())["status"], "succeeded");
    assert!(fd::load(root.path())["failure"].is_null());
}

#[test]
fn earlier_schema_one_attempts_without_exception_fields_remain_readable() {
    let root = fixture();
    let mut original = serde_json::to_value(failed(root.path())).expect("evidence");
    original
        .as_object_mut()
        .expect("object")
        .remove("managed_exception_types");
    original
        .as_object_mut()
        .expect("object")
        .remove("clr_exception_code");
    files::atomic_json(&record(root.path()), &original).expect("earlier schema one");
    let evidence = fd::load(root.path());
    assert_eq!(evidence["failure"], "hook_nonzero");
    assert_eq!(evidence["managed_exception_types"], json!([]));
    assert!(evidence["clr_exception_code"].is_null());
    let mut retry = fd::begin(root.path(), "repair", "flightdeck").expect("retry");
    retry.status = "failed".into();
    retry.completed_at = Some(files::now());
    retry.hook_exit_code = Some(82);
    retry.failure = Some("hook_nonzero".into());
    fd::save(root.path(), &retry).expect("new attempt");
    assert_eq!(fd::load(root.path())["failure"], "hook_nonzero");
}

#[test]
fn managed_exception_report_contains_only_fixed_symbols_not_messages_or_stack_paths() {
    let root = fixture();
    let mut attempt = failed(root.path());
    attempt.hook_exit_code = Some(82);
    attempt.failure = Some("managed_exception".into());
    attempt.managed_exception_types = vec![
        "System.TypeInitializationException".into(),
        "System.IO.FileNotFoundException".into(),
    ];
    attempt.clr_exception_code = Some("e0434352".into());
    fd::save(root.path(), &attempt).expect("observed symbols");
    files::atomic(&root.path().join("private/fenix-app.log"), b"Unhandled exception. System.IO.FileNotFoundException: PRIVATE_EXCEPTION_MESSAGE C:\\private\\account@example.test.dll\n").expect("private log");
    let state = tempfile::tempdir().expect("state");
    let app = Launcher::new(state.path().join("state"), None).expect("launcher");
    app.lock().runtime = Some(root.path().into());
    let data = json!({"category":"installation","description":"The Fenix hook failed.","runtime_path":root.path()});
    let result = problem_reports::prepare(&app, &data).expect("draft");
    let evidence = &result["draft"]["report"]["diagnostics"]["summary"]["fenix"];
    assert_eq!(
        evidence["managed_exception_types"],
        json!(attempt.managed_exception_types)
    );
    assert_eq!(evidence["clr_exception_code"], "e0434352");
    assert_eq!(evidence["failure"], "managed_exception");
    assert_eq!(evidence["evidence_complete"], true);
    let text = result.to_string();
    for private in [
        "PRIVATE_EXCEPTION_MESSAGE",
        "account@example.test",
        "C:\\private",
    ] {
        assert!(!text.contains(private));
    }
    let persisted = problem_reports::read(&app.state_dir.join("problem-report.json"))
        .expect("read draft")
        .expect("draft");
    assert_eq!(persisted, result["draft"]);
}

#[test]
fn support_draft_includes_only_selected_runtime_structured_fenix_evidence() {
    let root = fixture();
    failed(root.path());
    files::atomic(
        &root.path().join("private/fenix-app.log"),
        b"PRIVATE_LOG_MARKER account=fixture@example.test password=private-placeholder\n",
    )
    .expect("private log");
    let state = tempfile::tempdir().expect("state");
    let app = Launcher::new(state.path().join("state"), None).expect("launcher");
    {
        let mut s = app.lock();
        s.runtime = Some(root.path().into());
        s.jobs.insert("fenix".into(), json!({"state":"failed", "error":"PRIVATE_JOB_MARKER", "installer_path":"/private/fixture.exe"}));
    }
    let data = json!({"category":"installation","description":"The Fenix installer hook failed.","runtime_path":root.path()});
    let result = problem_reports::prepare(&app, &data).expect("support draft");
    let summary = &result["draft"]["report"]["diagnostics"]["summary"];
    assert_eq!(summary["context"]["diagnostics_schema"], 6);
    assert_eq!(summary["fenix"]["package_version"], "1.0.286");
    assert_eq!(summary["fenix"]["failure"], "hook_nonzero");
    assert_eq!(summary["fenix"]["runner"], "flightdeck");
    let text = result["draft"]["report"].to_string();
    for excluded in [
        "PRIVATE_LOG_MARKER",
        "PRIVATE_JOB_MARKER",
        "fixture@example.test",
        "private-placeholder",
        "/private/fixture.exe",
        root.path().to_str().expect("fixture path"),
    ] {
        assert!(!text.contains(excluded), "exported private fixture data");
    }
    let persisted = problem_reports::read(&app.state_dir.join("problem-report.json"))
        .expect("read draft")
        .expect("draft");
    assert_eq!(persisted, result["draft"]);

    // The report follows the selected runtime, not a stale job or another prefix.
    let other = fixture();
    app.lock().runtime = Some(other.path().into());
    assert!(problem_reports::prepare(&app, &data).is_err());
    let mut switched = data;
    switched["runtime_path"] = json!(other.path());
    let result = problem_reports::prepare(&app, &switched).expect("other runtime");
    assert!(result["draft"]["report"]["diagnostics"]["summary"]["fenix"].is_null());
}

#[test]
fn record_reader_rejects_unknown_fields_private_strings_and_invalid_types() {
    let root = fixture();
    let original = serde_json::to_value(failed(root.path())).expect("evidence");
    for (key, value) in [
        ("raw_log", json!("PRIVATE_LOG_MARKER")),
        ("runner", json!("/private/runner")),
        ("failure", json!("PRIVATE_ERROR_MARKER")),
        ("package_version", json!("1.0.286-private-token")),
        ("package_version", json!("1.0.286\naccount=fixture")),
        ("operation", json!("open /private/fixture")),
        ("hook", json!("install --token fixture")),
        ("hook_exit_code", json!("3 PRIVATE_CODE_MARKER")),
        ("process_exit_code", json!(4294967296u64)),
        ("evidence_complete", json!("true")),
        (
            "managed_exception_types",
            json!(["Private.CustomerException"]),
        ),
        (
            "managed_exception_types",
            json!(["System.IO.FileNotFoundException: PRIVATE_MESSAGE"]),
        ),
        (
            "managed_exception_types",
            json!(["System.Exception", "System.Exception"]),
        ),
        (
            "managed_exception_types",
            json!([
                "System.Exception",
                "System.ArgumentException",
                "System.ArgumentNullException",
                "System.ArgumentOutOfRangeException",
                "System.BadImageFormatException",
                "System.DllNotFoundException",
                "System.EntryPointNotFoundException",
                "System.InvalidOperationException",
                "System.IO.IOException"
            ]),
        ),
        (
            "managed_exception_types",
            json!(["System.Exception".repeat(300)]),
        ),
        ("managed_exception_types", json!("System.Exception")),
        ("managed_exception_types", Value::Null),
        ("clr_exception_code", json!("0xe0434352")),
        ("clr_exception_code", json!("e0434352 PRIVATE_CODE_MARKER")),
        ("clr_exception_code", json!("c0000005")),
        ("clr_exception_code", json!(82)),
        ("failure", json!("managed_exception")),
        ("schema", json!(2)),
        ("started_at", json!("2026-02-30T00:00:00Z")),
        ("completed_at", json!("2020-01-01T00:00:00Z")),
        ("completed_at", Value::Null),
    ] {
        let mut tampered = original.clone();
        tampered[key] = value;
        files::atomic_json(&record(root.path()), &tampered).expect("untrusted record");
        assert!(fd::load(root.path()).is_null(), "accepted invalid {key}");
    }
    for version in ["1.0.286", "1.0.286.1"] {
        assert!(fd::valid_version(version));
    }
    for version in [
        "",
        "286",
        "1.0",
        "1.0.286.1.2",
        "1.0.123456",
        "version 1.0.286",
    ] {
        assert!(!fd::valid_version(version));
    }
}

#[test]
fn missing_duplicate_oversized_and_linked_records_are_unavailable() {
    let root = fixture();
    assert!(fd::load(root.path()).is_null());
    let original = serde_json::to_string(&failed(root.path())).expect("evidence");
    let duplicate = original.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
    files::atomic(&record(root.path()), duplicate.as_bytes()).expect("duplicate key");
    assert!(fd::load(root.path()).is_null());
    files::atomic(&record(root.path()), &vec![b' '; 4097]).expect("large record");
    assert!(fd::load(root.path()).is_null());

    let other = fixture();
    failed(other.path());
    fs::remove_file(record(root.path())).expect("remove own record");
    symlink(record(other.path()), record(root.path())).expect("linked record");
    assert!(fd::load(root.path()).is_null());
    fs::remove_file(record(root.path())).expect("remove own link");
    fs::hard_link(record(other.path()), record(root.path())).expect("hard-linked record");
    assert!(fd::load(root.path()).is_null());
}

#[test]
fn cancellation_and_unknown_results_do_not_imply_a_hook_success() {
    let root = fixture();
    for status in ["cancelled", "unknown"] {
        let mut attempt = fd::begin(root.path(), "installer", "unknown").expect("begin");
        attempt.status = status.into();
        attempt.completed_at = Some(files::now());
        fd::save(root.path(), &attempt).expect("terminal result");
        let evidence = fd::load(root.path());
        assert_eq!(evidence["status"], status);
        assert_eq!(evidence["evidence_complete"], false);
        assert!(evidence["package_version"].is_null());
        assert!(evidence["hook"].is_null());
    }
    assert!(fd::begin(root.path(), "installer", "/private/runner").is_err());
    assert_eq!(fd::load(root.path())["status"], "unknown");
}
