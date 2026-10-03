// SPDX-License-Identifier: MIT
//! Synthetic diagnostics and owned helper processes; no accounts or game.
#![allow(clippy::unwrap_used)]
use flightdeck::{
    backend::Launcher, files, graphics_diagnostics as gd, log_reader, run_diagnostics, store_check,
    store_diagnostics,
};
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::symlink, process::Command, time::Duration};
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
    let text = concat!(
        "[xodus-store-query] kind=1 hr=00000000 time_ms=1790623215379\n",
        "[xodus-store] XStoreCreateContext account=private-token hr=00000000\n",
        "[xodus-gamesave] local_init enabled=1 sync_on_demand=0 hr=00000000\n",
        "xodus-title-auth: host=xsts.auth.xboxlive.com status=401\n",
        "xodus-user-policy-cache: stage=title_fetch hr=80072EE2\n",
        "xodus-signature-policy: call=4 host=private-title.playfabapi.com matched=0 has_policy=0 index=0 version=0 supported=0 token_only=0 hr=80004001\n",
        "xodus-user-api: signature.policy call=0 hr=80004001\n",
        "xodus-user-api: XUserGetTokenAndSignatureAsync.complete call=0 hr=80004001\n",
        "[xodus-network] security scheme=https host=private.xboxlive.com policy=fetch-failed result=80072ee2\n",
        "[xodus-network] security scheme=https host=xboxlive.com.private policy=unmatched result=80070490\n",
        "[xodus-network] security scheme=https host=private policy=private result=80004001\n",
        "[xodus-store-catalog] stage=mapping hr=80004001 time_ms=1790623216000 reason=sku-selection\n",
        "[xodus-store-catalog] stage=mapping hr=80004001 time_ms=1790623216003 reason=private-token\n",
        "[xodus-store-catalog] stage=inventory-mapping hr=80004001 time_ms=1790623216001 reason=product-language\n",
        "00ac:err:mmdevapi:init_driver:No driver from L\"private-path\" could be initialized.\n",
        "00ac:warn:pulse:pulse_contextcallback:Context failed: private-server\n",
        "00ac:warn:mmdevapi:get_mmdevice_by_activatepath:Failed to get requested device (private-device): 80070490\n",
        "00ac:err:xaudio2:private_function:private-account\n",
        "123:00ac:err:vkd3d-proton:vkd3d_create_device:Failed, vr -4. private-token\n",
        "DXVK: v2.7.1\ninfo:vkd3d-proton: vkd3d-proton - build: abcdef12+.\n",
        "info:vkd3d-proton: vkd3d-proton - applicationVersion: 3.0.0.\n",
        "Application is presenting user index 0, but it has never been rendered to.\n",
        "DXVK: No adapters found VK_ERROR_DEVICE_LOST VK_ERROR_PRIVATE\n",
        "xodus-user-api: private-token call=0 hr=80004001\nAuthorization: Bearer private-token\n",
        "xodus-wine-launch: wine_pid=123 signal=11 shell_exit_code=139 elapsed_seconds=2.250\n"
    );
    files::atomic(&path, text.as_bytes()).unwrap();
    files::atomic(&t.path().join("service.log"),b"[flightdeck-store-event] time_ms=1790623216002 seq=2 phase=checkout_ready outcome=passed\n").unwrap();
    let actual = run_diagnostics::read(&path).unwrap().0;
    let reference=Command::new("python3").args(["-c","import json,sys;from pathlib import Path;from flightdeck import run_diagnostics,store_diagnostics;print(json.dumps([run_diagnostics.read(Path(sys.argv[1]))[0],store_diagnostics.session(Path(sys.argv[1]).parent)]))"]).arg(&path).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference: Value = serde_json::from_slice(&reference.stdout).unwrap();
    assert_eq!(actual, reference[0]);
    assert_eq!(store_diagnostics::session(t.path()), reference[1]);
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
    let body = "import os,sys;assert os.environ['XDG_DATA_HOME']==os.getcwd()+'/private/xdg/data';assert 'FLIGHTDECK_STORE_LAUNCH' not in os.environ;assert sys.stdin.buffer.read()==b'{}';print('{\"stage\":\"account\",\"state\":\"passed\",\"code\":\"local_session\"}',flush=True)";
    let code = store_check::check_process(
        &ctx,
        Command::new("python3")
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
        Command::new("python3").args(["-c", "print('private-token')"]),
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
    let body = "import os,signal,time\nif os.fork()==0:\n def done(*_):\n  open('private/stopped','w').write('yes');os._exit(0)\n signal.signal(signal.SIGTERM,done)\n open('private/ready','w').write('yes')\n while True:time.sleep(.02)\nelse:\n while not os.path.exists('private/ready'):time.sleep(.01)\n os._exit(0)\n";
    assert_eq!(
        store_check::check_process(
            &ctx,
            Command::new("python3").args(["-c", body]),
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
    let code = "import json,hashlib,sys,os\nr={'schema':1,'id':'a'*32,'created_at':'2026-10-03T12:00:00+00:00','category':'graphics','description':'Menü läd nicht 😕','observations':['menus_visible'],'system':{},'diagnostics':{'floats':[1.0,0.000001,1e-5,1e-4,1e16,1e20,-0.0],'summary':{}}}\ndef encoded(v):return (json.dumps(v,ensure_ascii=False,indent=2)+'\\n').encode()\nd={'report':r,'sha256':hashlib.sha256(encoded(r)).hexdigest()}\nwith open(sys.argv[1],'wb') as f:f.write(encoded(d))\nos.chmod(sys.argv[1],0o600)\n";
    assert!(
        Command::new("python3")
            .args(["-c", code])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
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
