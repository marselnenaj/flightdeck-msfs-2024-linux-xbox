// SPDX-License-Identifier: MIT
//! Synthetic diagnostics and owned helper processes; no accounts or game.
#![allow(clippy::unwrap_used)]
#[path = "fixtures/programs.rs"]
mod programs;
use flightdeck::{
    backend::Launcher, files, graphics_diagnostics as gd, log_reader, run_diagnostics, store_check,
    store_diagnostics,
};
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::symlink, process::Command, time::Duration};
#[test]
fn existing_launch_logs_keep_the_actual_media_overlay_without_exporting_unknown_values() {
    for overlay in [
        "system_memory",
        "unsupported",
        "not_bundled",
        "disabled",
        "cache_changed",
    ] {
        let mut log = run_diagnostics::GameLog::default();
        log.consume(&format!("flightdeck-media: source=host h264=default overlay={overlay}\nMissing decoder: H.264 (Main Profile) private-video-caps\n"));
        let result = log.result(json!({"complete":true}));
        assert_eq!(result["media"]["configuration"]["overlay"], overlay);
        assert_eq!(result["media"]["observations"]["h264_decoder_missing"], 1);
        assert!(!result.to_string().contains("private-video-caps"));
    }
    let mut log = run_diagnostics::GameLog::default();
    log.consume("flightdeck-media: source=host h264=default overlay=private-value\n");
    let result = log.result(json!({"complete":true}));
    assert!(result["media"]["configuration"].get("overlay").is_none());
    assert!(!result.to_string().contains("private-value"));
}
#[test]
fn existing_wine_messages_name_only_known_missing_media_elements() {
    let mut log = run_diagnostics::GameLog::default();
    log.consume("winegstreamer: failed to create videoflip, are 64-bit GStreamer plugins installed?\nwinegstreamer: failed to create private_secret, are 64-bit GStreamer plugins installed?\n");
    let out = log.result(json!({"complete":true}));
    assert_eq!(out["media"]["missing_elements"], json!(["videoflip"]));
    assert_eq!(out["media"]["observations"]["missing_plugin"], 2);
    assert!(!out.to_string().contains("private_secret"));
}
#[test]
fn media_probe_details_only_export_fixed_names_and_states() {
    let mut log = run_diagnostics::GameLog::default();
    log.consume(
        "flightdeck-media-selection: reason=verified_host_fallback\n\
        flightdeck-media-probe: source=runner state=incomplete\n\
        flightdeck-media-element: source=runner name=avdec_h264 ready=0\n\
        flightdeck-media-probe: source=host state=ready\n\
        flightdeck-media-element: source=host name=avdec_h264 ready=1\n\
        flightdeck-media-element: source=host name=private_secret ready=0\n\
        flightdeck-media-probe: source=host state=private_secret\n\
        flightdeck-media-selection: reason=private_secret\n\
        flightdeck-media-element: source=private_secret name=qtdemux ready=0\n",
    );
    let out = log.result(json!({"complete":true}));
    assert_eq!(out["media"]["selection_reason"], "verified_host_fallback");
    assert_eq!(
        out["media"]["probes"]["runner"]["elements"]["avdec_h264"],
        false
    );
    assert_eq!(out["media"]["probes"]["host"]["state"], "ready");
    assert_eq!(
        out["media"]["probes"]["host"]["elements"]["avdec_h264"],
        true
    );
    assert!(!out.to_string().contains("private_secret"));
}
#[test]
fn xbox_startup_failures_keep_stage_and_result_without_private_log_text() {
    let mut log = run_diagnostics::GameLog::default();
    log.consume(
        "xodus-runtime-lifetime: initialize argc=4 clients=1 result=00000000\n\
        xodus-user-bridge: initialize result=00000000\n\
        xodus-user-load: stage=xbox_user_load hr=80004001 private-token\n\
        xodus-user-auth: stage=user_token hr=80004001 private-token\n\
        xodus-user-api: XUserAddAsync.service call=0 hr=80004001\n\
        xodus-user-api: XUserAddAsync.complete call=0 hr=80004001\n\
        xodus-runtime: System.GetXboxLiveSandboxId result=00000000\n\
        xodus-user-load: stage=private_secret hr=80004001\n\
        xodus-user-api: private_secret call=0 hr=80004001\n",
    );
    let out = log.result(json!({"complete":true}));
    assert_eq!(out["runtime_calls"].as_array().unwrap().len(), 5);
    assert_eq!(
        out["runtime_calls"][2],
        json!({"method":"XUserAddAsync","stage":"xbox_user_load","hresult":"80004001"})
    );
    assert_eq!(out["user_calls"].as_array().unwrap().len(), 2);
    assert!(!out.to_string().contains("private"));
}
#[test]
fn xbox_startup_results_are_bounded_across_log_chunks() {
    let mut log = run_diagnostics::GameLog::default();
    for n in 0..300 {
        log.consume(&format!("xodus-user-bridge: initialize result={n:08x}\n"));
    }
    let out = log.result(json!({"complete":true}));
    assert_eq!(out["runtime_calls"].as_array().unwrap().len(), 256);
    assert_eq!(out["summary_limited"], true);
}
#[test]
fn media_failures_and_placeholder_video_are_exported_without_private_log_text() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("game.log");
    files::atomic(
        &path,
        b"flightdeck-media: source=host h264=software_preferred\n\
        ERROR WINE Cannot decode caps video/x-h264, codec_data=SECRET-CODEC-DATA\n\
        ERROR protonmediaconverter Creating tag file placeholder-video-used.\n\
        ERROR Env MEDIACONV_VIDEO_TRANSCODED_FILE is not set. /private/account/token\n\
        WARN module_open failed: libvpx.so.6: cannot open shared object file: No such file or directory\n",
    )
    .unwrap();
    let result = run_diagnostics::read(&path).unwrap().0;
    assert_eq!(
        result["media"]["configuration"]["h264"],
        "software_preferred"
    );
    assert_eq!(result["media"]["observations"]["placeholder_video"], 1);
    assert_eq!(result["media"]["observations"]["h264_decode_failed"], 1);
    assert_eq!(
        result["media"]["observations"]["transcoded_video_unavailable"],
        1
    );
    assert!(!result.to_string().contains("SECRET"));
    assert_eq!(
        result["media"]["observations"]["plugin_library_unavailable"],
        1
    );
    assert!(!result.to_string().contains("/private/account"));
}
#[test]
fn bounded_scan_keeps_complete_lines_and_never_joins_omitted_fragments() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("game.log");
    files::atomic(&path, b"first\nsecond\nthird\nlast\n").unwrap();
    let mut output = String::new();
    let (c, _) =
        log_reader::scan_bounded(&path, 8, 8, Duration::from_secs(3), |s| output.push_str(s))
            .unwrap();
    assert_eq!(output, "first\nlast\n");
    assert_eq!(c["bytes_read"], 16);
    assert_eq!(c["complete"], false);
    files::atomic(&path, b"first\nsecond\nlast").unwrap();
    output.clear();
    let (c, _) =
        log_reader::scan_bounded(&path, 8, 12, Duration::from_secs(3), |s| output.push_str(s))
            .unwrap();
    assert_eq!(output, "first\nsecond\nlast\n");
    assert_eq!(c["complete"], true);
    files::atomic(&path, format!("{}end\n", "hidden\n".repeat(30)).as_bytes()).unwrap();
    output.clear();
    let (c, _) =
        log_reader::scan_bounded(&path, 1024, 8, Duration::ZERO, |s| output.push_str(s)).unwrap();
    assert_eq!(output, "end\n");
    assert_eq!(c["omitted_bytes"], 206);
}
#[test]
fn oversized_changed_and_symlink_logs_never_claim_complete() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("game.log");
    files::atomic(
        &path,
        format!(
            "{}[xodus-store-query] kind=0 hr=80004001\nOK\n",
            "x".repeat(300000)
        )
        .as_bytes(),
    )
    .unwrap();
    let mut out = String::new();
    let (c, _) = log_reader::scan(&path, |s| out.push_str(s)).unwrap();
    assert_eq!(out, "OK\n");
    assert_eq!(c["oversized_lines"], 1);
    assert_eq!(c["complete"], false);
    files::atomic(&path, b"first\n").unwrap();
    let (c, _) = log_reader::scan(&path, |_| {
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"new\n")
            .unwrap();
    })
    .unwrap();
    assert_eq!(c["changed_during_read"], true);
    assert_eq!(c["complete"], false);
    symlink(&path, t.path().join("linked.log")).unwrap();
    assert!(run_diagnostics::read(&t.path().join("linked.log")).is_err());
}
#[test]
fn diagnostic_evidence_matches_reference_and_discards_private_text() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("game.log");
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/legacy-python/diagnostics.json")).unwrap();
    files::atomic(&path, reference["game_log"].as_str().unwrap().as_bytes()).unwrap();
    files::atomic(
        &t.path().join("service.log"),
        reference["service_log"].as_str().unwrap().as_bytes(),
    )
    .unwrap();
    let actual = run_diagnostics::read(&path).unwrap().0;
    assert_eq!(actual, reference["run"]);
    assert_eq!(store_diagnostics::session(t.path()), reference["store"]);
    assert!(!actual.to_string().contains("private"));
}
#[test]
fn diagnostic_floods_are_bounded_and_marked() {
    let mut log = run_diagnostics::GameLog::default();
    for n in 0..300 {
        log.consume(&format!(
            "xodus-user-api: signature.policy call=0 hr={n:08x}\n"
        ));
    }
    let out = log.result(json!({"complete":true}));
    assert_eq!(out["user_calls"].as_array().unwrap().len(), 256);
    assert_eq!(out["summary_limited"], true);
    let t = tempfile::tempdir().unwrap();
    files::atomic(
        &t.path().join("game.log"),
        (0..300)
            .map(|n| {
                format!(
                    "[xodus-store-query] kind=0 hr=00000000 time_ms={}\n",
                    1790623216000u64 + n
                )
            })
            .collect::<String>()
            .as_bytes(),
    )
    .unwrap();
    files::atomic(&t.path().join("service.log"), b"").unwrap();
    let out = store_diagnostics::session(t.path());
    assert_eq!(out["events"].as_array().unwrap().len(), 256);
    assert_eq!(out["partial"], true);
    assert_eq!(out["events"][0]["time_ms"], 1790623216044u64);
}
#[test]
fn launch_records_keep_proton_and_nvidia_modes_without_device_identity() {
    let t = tempfile::tempdir().unwrap();
    files::private_dir(&t.path().join("private")).unwrap();
    files::atomic_json(
        &t.path().join("private/runtime.json"),
        &json!({"game_id":"msfs2024"}),
    )
    .unwrap();
    let env = [
        ("DXVK_FILTER_DEVICE_NAME".into(), "private-gpu".into()),
        (
            "WINEDLLOVERRIDES".into(),
            "*dxgi,d3d11.dll=n,b;secret=private;nvngx=;nvapi=x-private".into(),
        ),
    ]
    .into();
    for mode in flightdeck::graphics::MODES {
        let report = json!({"devices":[{"name":"private-gpu","vendor_id":0x10de}],"nvidia":"ready","nvidia_mode":mode});
        let mut record = gd::launch_record(t.path(), &report, &env, "spawned", &files::now());
        record["proton"] = json!({"mode":"proton","version":"experimental-11.0-20260924-x86_64","loader":"portable"});
        record["secret"] = json!("private-token");
        assert!(gd::save_launch(t.path(), &record));
        let saved = gd::load_launch(t.path());
        assert_eq!(saved["nvidia_mode"], mode);
        assert_eq!(saved["gpu_filters"]["DXVK_FILTER_DEVICE_NAME"], "nvidia");
        assert_eq!(saved["dll_overrides"]["*dxgi"], "native,builtin");
        assert_eq!(saved["proton"], record["proton"]);
        assert!(!saved.to_string().contains("private"));
        record["gpu_filters"]["private-device"] = json!("nvidia");
        assert!(gd::save_launch(t.path(), &record));
        assert!(gd::load_launch(t.path()).is_null());
    }
}
#[test]
fn prefix_diagnostics_restrict_registry_sections_and_compare_renderer_files() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    for name in [
        "private",
        "local/msfs-prefix/drive_c/windows/system32",
        "runner/files/lib/wine/dxvk/x86_64-windows",
    ] {
        files::private_dir(&root.join(name)).unwrap();
    }
    files::atomic_json(
        &root.join("private/runtime.json"),
        &json!({"game_id":"msfs2024"}),
    )
    .unwrap();
    for name in [
        "local/msfs-prefix/drive_c/windows/system32/dxgi.dll",
        "runner/files/lib/wine/dxvk/x86_64-windows/dxgi.dll",
    ] {
        files::atomic(&root.join(name), b"renderer").unwrap();
    }
    files::atomic(&root.join("local/msfs-prefix/user.reg"),b"[Software\\\\Wine\\\\DllOverrides]\n\"dxgi\"=\"native,builtin\"\n\"private\"=\"secret\"\n[Software\\\\Wine\\\\AppDefaults\\\\FlightSimulator2024.exe\\\\DllOverrides]\n\"nvapi64\"=\"\"\n[Software\\\\Private]\n\"dxgi\"=\"secret\"\n").unwrap();
    let r = gd::prefix_summary(root);
    assert_eq!(r["status"], "inspected");
    assert_eq!(r["libraries"]["dxgi"], "matches_runner");
    assert_eq!(r["user_overrides"]["global"]["dxgi"], "native,builtin");
    assert_eq!(r["user_overrides"]["game"]["nvapi64"], "disabled");
    assert!(!r.to_string().contains("secret"));
}
fn context(t: &std::path::Path) -> flightdeck::backend::Context {
    let root = t.join("runtime");
    files::private_dir(&root.join("private")).unwrap();
    let app = Launcher::new(t.join("state"), None).unwrap();
    app.lock().runtime = Some(root);
    let ctx = app.reserve("store-check", "check", true).unwrap();
    ctx.update(json!({"steps":[{"stage":"account","state":"pending","code":"checking"}]}));
    ctx
}
#[test]
fn store_events_reject_extra_fields_duplicate_keys_and_terminal_changes() {
    let mut rows = vec![json!({"stage":"account","state":"pending","code":"checking"})];
    for raw in [
        r#"{"stage":"account","state":"passed","code":"local_session","token":"private"}"#,
        r#"{"stage":"account","stage":"account","state":"passed","code":"local_session"}"#,
        r#"{"stage":"account","state":"passed","code":"verified"}"#,
    ] {
        assert!(!store_check::event(raw.as_bytes(), &mut rows, &["account"]));
    }
    assert!(store_check::event(
        br#"{"stage":"account","state":"failed","code":"expired"}"#,
        &mut rows,
        &["account"]
    ));
    assert!(!store_check::event(
        br#"{"stage":"account","state":"passed","code":"local_session"}"#,
        &mut rows,
        &["account"]
    ));
    assert_eq!(rows[0]["code"], "expired");
}
#[test]
fn store_protocol_uses_runtime_account_and_rejects_private_output() {
    let t = tempfile::tempdir().unwrap();
    let ctx = context(t.path());
    let body = r#"[ "$XDG_DATA_HOME" = "$(pwd -P)/private/xdg/data" ] && [ -z "${FLIGHTDECK_STORE_LAUNCH+set}" ] && [ "$(cat)" = '{}' ] && echo '{"stage":"account","state":"passed","code":"local_session"}'"#;
    let code = store_check::check_process(
        &ctx,
        Command::new("sh")
            .args(["-c", body])
            .env("FLIGHTDECK_STORE_LAUNCH", "private-token"),
        b"{}",
        &["account"],
        Duration::from_secs(3),
    )
    .unwrap();
    assert_eq!(code, "available");
    assert_eq!(
        ctx.launcher.job("store-check")["steps"][0]["state"],
        "passed"
    );
    let code = store_check::check_process(
        &ctx,
        Command::new("sh").args(["-c", "echo private-token"]),
        b"",
        &["account"],
        Duration::from_secs(3),
    )
    .unwrap();
    assert_eq!(code, "error");
    assert!(
        !store_check::report(&ctx.launcher.lock())
            .to_string()
            .contains("private-token")
    );
}
#[test]
fn store_timeout_cleans_up_descendants_even_after_leader_exit() {
    let t = tempfile::tempdir().unwrap();
    let ctx = context(t.path());
    assert_eq!(
        store_check::check_process(
            &ctx,
            &mut programs::peer("leader"),
            b"",
            &["account"],
            Duration::from_millis(400)
        )
        .unwrap(),
        "timeout"
    );
    assert!(ctx.root().unwrap().join("private/stopped").exists());
}
#[test]
fn stale_store_recovery_never_overwrites_previous_job_or_reserves_runtime() {
    let t = tempfile::tempdir().unwrap();
    let ctx = context(t.path());
    ctx.launcher
        .finish(&ctx, Ok(json!({"state":"failed"})), false);
    let before = ctx.launcher.job("store-check");
    assert!(
        store_check::start(&ctx.launcher, "en", true, &json!({"job_id":"old"}), false).is_err()
    );
    assert_eq!(ctx.launcher.job("store-check"), before);
    assert!(ctx.launcher.lock().active.is_none());
}
#[test]
fn legacy_support_drafts_preserve_order_unicode_float_hashes_and_reject_tampering() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("problem-report.json");
    // A draft exactly as the Python releases wrote it, with their float
    // spellings and the hash over their encoding of the report.
    files::atomic(
        &path,
        include_bytes!("fixtures/legacy-python/problem-report-draft.json"),
    )
    .unwrap();
    let mut draft = flightdeck::problem_reports::read(&path).unwrap().unwrap();
    assert_eq!(draft["report"]["description"], "Menü läd nicht 😕");
    draft["report"]["description"] = json!("changed contents");
    files::atomic_json(&path, &draft).unwrap();
    assert!(flightdeck::problem_reports::read(&path).is_err());
}
#[test]
fn support_draft_review_checks_scope_and_preserves_user_description() {
    let t = tempfile::tempdir().unwrap();
    let app = Launcher::new(t.path().join("state"), None).unwrap();
    let data = json!({"category":"graphics","description":"  Die Runtime ist nicht startbereit.\nMenü bleibt schwarz.  ","runtime_path":null,"observations":["menus_visible","menus_visible"]});
    let mut stale = data.clone();
    stale["runtime_path"] = json!("/private/another-runtime");
    assert!(flightdeck::problem_reports::prepare(&app, &stale).is_err());
    let result = flightdeck::problem_reports::prepare(&app, &data).unwrap();
    let id = result["draft"]["report"]["id"].as_str().unwrap();
    assert_eq!(
        result["draft"]["report"]["description"],
        data["description"].as_str().unwrap().trim()
    );
    assert_eq!(
        result["draft"]["report"]["observations"],
        json!(["menus_visible"])
    );
    assert_eq!(
        flightdeck::problem_reports::snapshot(&app)["draft"],
        result["draft"]
    );
    assert!(flightdeck::problem_reports::discard(&app, "old").is_err());
    flightdeck::problem_reports::discard(&app, id).unwrap();
    assert!(flightdeck::problem_reports::snapshot(&app)["draft"].is_null());
}
#[test]
fn support_drafts_retain_safe_xbox_startup_results() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("runtime");
    let run = root.join("private/run-20261021-120000-test");
    files::private_dir(&run).unwrap();
    files::atomic(
        &run.join("game.log"),
        b"xodus-user-load: stage=xbox_user_load hr=80004001 private-token\n",
    )
    .unwrap();
    let app = Launcher::new(t.path().join("state"), None).unwrap();
    app.lock().runtime = Some(root.clone());
    let out = flightdeck::problem_reports::prepare(&app, &json!({
        "runtime_path":root,"category":"other","description":"Alternative Proton startup fails.","observations":[]
    })).unwrap();
    let evidence = &out["draft"]["report"]["diagnostics"]["summary"];
    assert_eq!(
        evidence["context"]["diagnostics_schema"],
        run_diagnostics::SCHEMA
    );
    assert_eq!(
        evidence["runtime_calls"][0],
        json!({"method":"XUserAddAsync","stage":"xbox_user_load","hresult":"80004001"})
    );
    assert!(!out.to_string().contains("private-token"));
}
