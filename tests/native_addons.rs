// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, fenix, files, gsx, transaction as tx};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};
fn write(path: &Path, data: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, data).unwrap();
}
fn fixture(base: &Path) -> (PathBuf, std::sync::Arc<Launcher>) {
    let root = base.join("runtime");
    let runner = base.join("runner");
    let bin = runner.join("files/bin");
    for name in ["wine", "wineserver"] {
        write(&bin.join(name), b"#!/bin/sh\nexit 0\n");
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    write(&root.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    write(
        &root.join("local/msfs-prefix/system.reg"),
        b"retained original registry",
    );
    files::private_dir(&root.join("local/msfs-prefix/drive_c/windows/system32")).unwrap();
    files::private_dir(&root.join("private")).unwrap();
    symlink(runner, root.join("runner")).unwrap();
    let app = Launcher::new(base.join("state"), Some(root.to_str().unwrap())).unwrap();
    (root, app)
}

#[test]
fn fenix_job_history_follows_the_selected_runtime_without_erasing_other_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(&temp.path().join("first"));
    let (other, _) = fixture(&temp.path().join("second"));
    for selected in [&root, &other] {
        write(
            &selected.join("private/fenix-compat.json"),
            b"{\"format\":1}",
        );
    }
    for state in ["complete", "failed", "cancelled"] {
        let job = json!({"id":"previous-fenix-job","operation":"installer","state":state,"runtime_path":root,"message":"Retained result from the first profile"});
        app.lock().jobs.insert("fenix".into(), job.clone());
        assert_eq!(fenix::snapshot(&app)["job"], job);

        app.configure(other.to_str().unwrap()).unwrap();
        let snapshot = fenix::snapshot(&app);
        assert_eq!(snapshot["runtime_path"], json!(other));
        assert!(snapshot["job"].is_null(), "foreign job leaked: {snapshot}");
        assert_eq!(app.job("fenix"), job);

        app.configure(root.to_str().unwrap()).unwrap();
        assert_eq!(fenix::snapshot(&app)["job"], job);
    }
    app.lock()
        .jobs
        .insert("fenix".into(), json!({"state":"failed"}));
    assert!(fenix::snapshot(&app)["job"].is_null());
}

#[test]
fn fenix_repair_stop_cancels_the_owned_hook_and_preserves_the_profile() {
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let prefix = root.join("local/msfs-prefix");
    let current = prefix.join("drive_c/users/steamuser/AppData/Local/FenixApp/current");
    write(&current.join("FenixApp.exe"), b"MZfixture");
    write(&current.join("sq.version"), b"<package><metadata><id>FenixApp</id><version>1.0.286</version><mainExe>FenixApp.exe</mainExe></metadata></package>");
    write(&root.join("private/fenix-compat.json"), b"{\"format\":1}");
    let executable = temp.path().join("runner/files/bin/wine");
    write(&executable, b"#!/bin/sh\nif [ \"$2\" = --veloapp-install ]; then\n touch \"$WINEPREFIX/hook-started\"\n exec sleep 30\nfi\nexit 0\n");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fenix::snapshot(&app)["can_repair_installer"], true);
    let before = fenix::identity(&prefix).unwrap();
    fenix::start(&app, "repair", &json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !prefix.join("hook-started").exists() {
        assert!(
            Instant::now() < deadline,
            "repair hook did not start: {}",
            app.job("fenix")
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fenix::snapshot(&app)["can_stop"], true);
    fenix::start(&app, "stop", &json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.job("fenix")["state"] == "running" {
        assert!(Instant::now() < deadline, "Stop did not cancel repair");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.job("fenix")["state"], "cancelled");
    let evidence = flightdeck::fenix_diagnostics::load(&root);
    assert_eq!(evidence["status"], "cancelled");
    assert!(evidence["hook_exit_code"].is_null());
    assert_eq!(fenix::identity(&prefix).unwrap(), before);
    assert_eq!(
        fs::read(prefix.join("system.reg")).unwrap(),
        b"retained original registry"
    );
}
#[test]
fn gsx_recovers_each_interrupted_publish_boundary_and_keeps_the_prior_marker() {
    for phase in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let (root, app) = fixture(temp.path());
        let token = "0123456789abcdef0123456789abcdef";
        let active = root.join("local/msfs-prefix");
        let original = fenix::identity(&active).unwrap();
        let stage = root.join("local").join(format!(".gsx-prefix-{token}"));
        let backup = root
            .join("local")
            .join(format!("msfs-prefix.before-gsx-{token}"));
        files::private_dir(&stage.join("drive_c/windows/system32")).unwrap();
        write(&stage.join("system.reg"), b"partial setup");
        let prior = json!({"format":1,"id":"11111111111111111111111111111111","state":"ready"});
        let state = json!({"format":1,"id":token,"state":if phase==0{"preparing"}else{"committing"},"original_prefix_id":original,"staged_prefix_id":fenix::identity(&stage).unwrap(),"prior":prior});
        files::atomic_json(&root.join(gsx::MARKER), &state).unwrap();
        if phase > 0 {
            tx::publish(&active, &backup).unwrap();
        }
        if phase == 2 {
            tx::publish(&stage, &active).unwrap();
        }
        let snapshot = gsx::snapshot(&app);
        assert_eq!(snapshot["can_recover"], true);
        assert_eq!(snapshot["can_change"], true);
        let ctx = app.reserve("gsx", "recover", true).unwrap();
        gsx::recover(&ctx).unwrap();
        assert_eq!(fenix::identity(&active).unwrap(), original);
        assert_eq!(gsx::marker(&root).unwrap().unwrap(), prior);
        assert_eq!(
            fs::read(active.join("system.reg")).unwrap(),
            b"retained original registry"
        );
    }
}
#[test]
fn gsx_recovery_refuses_a_replaced_original_backup() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let active = root.join("local/msfs-prefix");
    let id = fenix::identity(&active).unwrap();
    files::atomic_json(&root.join(gsx::MARKER),&json!({"format":1,"id":"0123456789abcdef0123456789abcdef","state":"committing","original_prefix_id":[0,0]})).unwrap();
    let ctx = app.reserve("gsx", "recover", true).unwrap();
    assert!(gsx::recover(&ctx).is_err());
    assert_eq!(fenix::identity(&active).unwrap(), id);
}
fn gsx_installation(root: &Path) {
    let prefix = root.join("local/msfs-prefix");
    let manager = prefix.join("drive_c/Program Files (x86)/Addon Manager");
    write(&manager.join("Couatl_Updater.exe"), b"MZ updater");
    write(&manager.join("couatl64/couatl64_boot.exe"), b"MZ companion");
    files::private_dir(&prefix.join("dosdevices")).unwrap();
    symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
    let community = root.join("Community");
    write(
        &community.join("fsdreamteam-gsx-pro/manifest.json"),
        br#"{"title":"GSX Pro","package_version":"1.0.0"}"#,
    );
    files::atomic_json(
        &root.join("private/runtime.json"),
        &json!({"game_id":"msfs2024","community_path":community}),
    )
    .unwrap();
    write(&prefix.join("drive_c/users/pilot/AppData/Roaming/Microsoft Flight Simulator 2024/exe.xml"),br#"<SimBase.Document><Launch.Addon><Name>Other addon</Name><Path>Other.exe</Path></Launch.Addon><Launch.Addon><Name>FSDT</Name><Path>C:\Program Files (x86)\Addon Manager\couatl64\couatl64_boot.exe</Path><CommandLine>preserved arguments</CommandLine></Launch.Addon></SimBase.Document>"#);
}
#[test]
fn gsx_uses_the_official_start_entry_and_retains_other_addons() {
    let temp = tempfile::tempdir().unwrap();
    let (root, _) = fixture(temp.path());
    gsx_installation(&root);
    gsx::configure(&root, true).unwrap();
    let path=root.join("local/msfs-prefix/drive_c/users/pilot/AppData/Roaming/Microsoft Flight Simulator 2024/exe.xml");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("Other addon"));
    assert!(text.contains("preserved arguments"));
    assert!(text.contains("<Disabled>False</Disabled>"));
    gsx::configure(&root, false).unwrap();
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("<Disabled>True</Disabled>")
    );
    assert_eq!(
        flightdeck::runtime::value(&root.join(gsx::STARTUP)).unwrap()["enabled"],
        false
    );
}
#[test]
fn custom_fsdt_installations_outside_the_profile_are_not_run() {
    let temp = tempfile::tempdir().unwrap();
    let (root, _) = fixture(temp.path());
    let prefix = root.join("local/msfs-prefix");
    let outside = temp.path().join("foreign manager");
    write(&outside.join("Couatl_Updater.exe"), b"MZ");
    write(
        &prefix.join("user.reg"),
        format!(
            "[Software\\\\Fsdreamteam] 123\n\"root\"={}\n",
            json!(outside)
        )
        .as_bytes(),
    );
    assert!(gsx::manager(&prefix).is_err());
    assert!(outside.join("Couatl_Updater.exe").is_file());
}
#[test]
fn fenix_retry_requires_the_untouched_original_and_script_backups() {
    let temp = tempfile::tempdir().unwrap();
    let (root, _) = fixture(temp.path());
    let work = "local/fenix-patch-20261003T120000-01234567";
    let backup = "private/fenix-patch-backup-20261003T120000-01234567";
    files::private_dir(&root.join(work)).unwrap();
    for name in ["launch-msfs.sh", "xodus-wine-launch"] {
        write(&root.join("tools").join(name), b"unchanged original");
        write(&root.join(backup).join(name), b"unchanged original");
    }
    let mut state = json!({"state":"preparing","work":work,"backup":backup,"previous_prefix":"local/msfs-prefix.before-fenix-20261003T120000-01234567","original_prefix_id":fenix::identity(&root.join("local/msfs-prefix")).unwrap(),"previous_runner":fs::read_link(root.join("runner")).unwrap()});
    assert!(fenix::retryable(&root, &state));
    state["state"] = json!("committing");
    assert!(!fenix::retryable(&root, &state));
    state["state"] = json!("preparing");
    write(&root.join("tools/launch-msfs.sh"), b"user modification");
    assert!(!fenix::retryable(&root, &state));
}
#[test]
fn fenix_restore_keeps_the_newer_profile_and_restores_original_scripts() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let prefix = root.join("local/msfs-prefix");
    let id = fenix::identity(&prefix).unwrap();
    let previous = "local/msfs-prefix.before-fenix-20261003T120000-01234567";
    tx::publish(&prefix, &root.join(previous)).unwrap();
    files::private_dir(&prefix.join("drive_c/windows/system32")).unwrap();
    write(
        &prefix.join("after-install.dat"),
        b"user aircraft and settings",
    );
    let backup = "private/fenix-patch-backup-20261003T120000-01234567";
    for name in ["launch-msfs.sh", "xodus-wine-launch"] {
        write(&root.join(backup).join(name), b"original launcher");
        write(&root.join("tools").join(name), b"new launcher");
    }
    let work = "local/fenix-patch-20261003T120000-01234567";
    files::private_dir(&root.join(work)).unwrap();
    files::atomic_json(&root.join(fenix::MARKER),&json!({"state":"installed","backup":backup,"work":work,"previous_prefix":previous,"previous_runner":fs::read_link(root.join("runner")).unwrap(),"original_prefix_id":id})).unwrap();
    let ctx = app.reserve("fenix", "restore", true).unwrap();
    fenix::restore(&ctx).unwrap();
    assert_eq!(fenix::identity(&prefix).unwrap(), id);
    let restored: Value =
        flightdeck::runtime::value(&root.join(backup).join("restored.json")).unwrap();
    assert_eq!(
        fs::read(
            root.join(restored["retained_prefix"].as_str().unwrap())
                .join("after-install.dat")
        )
        .unwrap(),
        b"user aircraft and settings"
    );
    assert_eq!(
        fs::read(root.join("tools/launch-msfs.sh")).unwrap(),
        b"original launcher"
    );
}
