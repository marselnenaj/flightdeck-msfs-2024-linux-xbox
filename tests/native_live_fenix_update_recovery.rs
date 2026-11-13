// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
//! Explicit Fenix patch update checks with the pinned runner, real Wine and the
//! public 0.1.4/0.1.5 payloads in new account-free runtimes.
//!
//! Inputs: REPRO_ROOT (new directory), REPRO_BUNDLE_014 (extracted 0.1.4 patch),
//! REPRO_ZIP_015 and REPRO_DESCRIPTOR_015 (0.1.5 release ZIP and
//! flightdeck-update.json), REPRO_RUNNER_ARCHIVE (pinned runner tar.xz) and
//! optionally REPRO_CACHE (public .NET installers). fenix_016_update_chain also
//! needs REPRO_ZIP_016 and REPRO_DESCRIPTOR_016 for the next release. A stand-in Wine program
//! replaces the licensed Fenix app; MSFS itself is not started.
use flightdeck::{backend::Launcher, bootstrap, fenix, fenix_update as update, files, resources};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn input(key: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(key).unwrap_or_else(|| panic!("missing {key}")))
}

fn job(app: &Arc<Launcher>, operation: &str, data: Value) -> Value {
    let started = fenix::start(app, operation, &data);
    eprintln!("[repro] start {operation}: {started:?}");
    if started.is_err() {
        return json!({"start_error": started.err().map(|e| e.to_string())});
    }
    let at = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let state = {
            let mut s = app.lock();
            Launcher::poll(&mut s);
            (
                s.jobs.get("fenix").cloned().unwrap_or(Value::Null),
                s.active.is_some(),
            )
        };
        if !state.1 && state.0["state"] != "running" {
            return state.0;
        }
        if at.elapsed() > Duration::from_secs(3 * 3600) {
            panic!("job {operation} did not finish");
        }
    }
}

fn listing(root: &Path) -> Value {
    let mut out = vec![];
    for dir in ["local", "private"] {
        if let Ok(entries) = fs::read_dir(root.join(dir)) {
            let mut names: Vec<_> = entries
                .filter_map(|e| e.ok())
                .map(|e| format!("{dir}/{}", e.file_name().to_string_lossy()))
                .collect();
            names.sort();
            out.extend(names);
        }
    }
    json!(out)
}

fn report(root: &Path, step: &str, extra: Value, app: &Arc<Launcher>) {
    let marker =
        files::json::<Value>(&root.join(fenix::MARKER), 4 * 1024 * 1024).unwrap_or_default();
    let verify = if marker.is_null() {
        Value::Null
    } else {
        json!(fenix::verify_installed(root, &marker).map_err(|e| e.to_string()))
    };
    let snap = fenix::snapshot(app);
    let value = json!({
        "step": step,
        "extra": extra,
        "marker_state": marker["state"], "marker_version": marker["version"],
        "configured": marker["configured"], "verify_installed": verify,
        "snapshot": {"state":snap["state"],"installed":snap["installed"],"configured":snap["configured"],
            "message":snap["message"],"version":snap["version"],"installed_version":snap["installed_version"],
            "update_available":snap["update_available"],"fenix_installed":snap["fenix_installed"]},
        "listing": listing(root),
    });
    eprintln!("[repro] {}", serde_json::to_string_pretty(&value).unwrap());
    let path = root.join(format!("private/repro-{step}.json"));
    let _ = fs::write(path, serde_json::to_vec_pretty(&value).unwrap());
}

struct Setup {
    root: PathBuf,
    runner: PathBuf,
    prefix: PathBuf,
    state_dir: PathBuf,
    app: Arc<Launcher>,
    bundle014: PathBuf,
    zip015: PathBuf,
    descriptor015: PathBuf,
}

fn setup(key: &str) -> Setup {
    let root = std::path::absolute(input("REPRO_ROOT")).unwrap().join(key);
    assert!(!files::exists(&root), "use a new runtime directory");
    let bundle014 = input("REPRO_BUNDLE_014");
    let zip015 = input("REPRO_ZIP_015");
    let descriptor015 = input("REPRO_DESCRIPTOR_015");
    let archive = input("REPRO_RUNNER_ARCHIVE");
    let lock = resources::json("compat/bootstrap.lock.json").unwrap();

    files::private_dir(&root.join("private")).unwrap();
    files::private_dir(&root.join("local")).unwrap();
    fs::write(
        root.join("private/synthetic-probe"),
        b"Flightdeck isolated test\n",
    )
    .unwrap();
    fs::write(
        root.join("private/runtime.json"),
        b"{\"game_id\":\"msfs2024\"}\n",
    )
    .unwrap();
    if let Some(cache) = std::env::var_os("REPRO_CACHE") {
        let cache = PathBuf::from(cache);
        let destination = root.join("private/fenix-downloads");
        files::private_dir(&destination).unwrap();
        for entry in fs::read_dir(cache).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
    let unpacked = root.join("local/original-runner");
    files::private_dir(&unpacked).unwrap();
    bootstrap::extract(&archive, &unpacked, &AtomicBool::new(false)).unwrap();
    let runner = unpacked.join(lock["runner"]["directory"].as_str().unwrap());
    std::os::unix::fs::symlink(&runner, root.join("runner")).unwrap();
    let prefix = root.join("local/msfs-prefix");
    bootstrap::prepare_prefix(&runner, &prefix, &AtomicBool::new(false)).unwrap();
    resources::install_scripts(&root.join("tools")).unwrap();

    let state_dir = root.join("private/repro-state");
    let app = Launcher::new(state_dir.clone(), None).unwrap();
    app.lock().runtime = Some(root.clone());

    Setup {
        root,
        runner,
        prefix,
        state_dir,
        app,
        bundle014,
        zip015,
        descriptor015,
    }
}

/// Offer a release exactly as discovery finds it on GitHub.
fn candidate(x: &Setup, zip: &Path, descriptor: &Path, version: &str) {
    let bytes = fs::read(descriptor).unwrap();
    let meta = json!({"size":bytes.len(),"sha256":files::sha256(&bytes),"version":version});
    let candidate = update::descriptor(&bytes, &meta).unwrap();
    x.app.lock().fenix_updates.candidate = candidate;
    files::private_dir(&x.state_dir.join("fenix-bundles")).unwrap();
    fs::copy(
        zip,
        x.state_dir.join(format!("fenix-bundles/{version}.zip")),
    )
    .unwrap();
}

fn candidate015(x: &Setup) {
    candidate(x, &x.zip015, &x.descriptor015, "0.1.5");
}

fn install014(x: &Setup) {
    let ctx = x.app.reserve("fenix", "install", true).unwrap();
    let installed = fenix::install(&ctx, &x.bundle014).map_err(|e| e.to_string());
    ctx.launcher
        .finish(&ctx, Ok(json!({"state":"complete"})), false);
    report(
        &x.root,
        "1-install-014",
        json!({"result": installed}),
        &x.app,
    );
    assert!(installed.is_ok());
}

/// What a user who ran MSFS once and signed in to Fenix has in the profile.
fn signed_in_fenix(x: &Setup) {
    let fenix_exe = x
        .prefix
        .join("drive_c/Program Files/FenixSim A320/Fenix.exe");
    fs::create_dir_all(fenix_exe.parent().unwrap()).unwrap();
    fs::copy(
        x.runner.join("files/lib/wine/x86_64-windows/hostname.exe"),
        &fenix_exe,
    )
    .unwrap();
    fs::create_dir_all(x.prefix.join("drive_c/Program Files/FenixSim A320/deps")).unwrap();
    fs::create_dir_all(
        x.prefix
            .join("drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024"),
    )
    .unwrap();
    let config = x.prefix.join("drive_c/ProgramData/Fenix/FenixSim A320");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("fenixConfig.xml"), "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FenixConfig><displayMode>CPU</displayMode></FenixConfig>\n").unwrap();
    fs::write(
        config.join("persistancy.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Persistancy></Persistancy>\n",
    )
    .unwrap();
}

#[test]
#[ignore = "requires the pinned runner, public Fenix payloads and Wine; creates new prefixes"]
fn fenix_015_configured_user() {
    let x = setup("configured");
    install014(&x);
    signed_in_fenix(&x);
    let configure = job(&x.app, "configure", json!({}));
    report(
        &x.root,
        "2-configure-014",
        json!({"job": configure}),
        &x.app,
    );
    let open = job(&x.app, "open", json!({}));
    report(&x.root, "3-open-014", json!({"job": open}), &x.app);
    candidate015(&x);
    let open = job(&x.app, "open", json!({}));
    report(&x.root, "4-open-after-015", json!({"job": open}), &x.app);
    let open = job(&x.app, "open", json!({}));
    report(&x.root, "5-open-again", json!({"job": open}), &x.app);
    let configure = job(&x.app, "configure", json!({}));
    report(
        &x.root,
        "6-configure-015",
        json!({"job": configure}),
        &x.app,
    );
}

#[test]
#[ignore = "requires the pinned runner, public Fenix payloads and Wine; creates new prefixes"]
fn fenix_015_field_report() {
    let x = setup("fresh");
    let (root, runner, prefix, state_dir, app, bundle014, zip015, descriptor015) = (
        x.root.clone(),
        x.runner.clone(),
        x.prefix.clone(),
        x.state_dir.clone(),
        x.app.clone(),
        x.bundle014.clone(),
        x.zip015.clone(),
        x.descriptor015.clone(),
    );
    // 1. Fenix patch 0.1.4 through the production transaction (embedded release).
    let ctx = app.reserve("fenix", "install", true).unwrap();
    let installed = fenix::install(&ctx, &bundle014).map_err(|e| e.to_string());
    ctx.launcher
        .finish(&ctx, Ok(json!({"state":"complete"})), false);
    report(&root, "1-install-014", json!({"result": installed}), &app);
    assert!(installed.is_ok());

    // Stand-in for the licensed Fenix app: a Wine program that exits normally.
    let fenix_exe = prefix.join("drive_c/Program Files/FenixSim A320/Fenix.exe");
    fs::create_dir_all(fenix_exe.parent().unwrap()).unwrap();
    fs::copy(
        runner.join("files/lib/wine/x86_64-windows/hostname.exe"),
        &fenix_exe,
    )
    .unwrap();

    let open = job(&app, "open", json!({}));
    report(&root, "2-open-014", json!({"job": open}), &app);

    // 2. Discovery finds 0.1.5 exactly as GitHub publishes it.
    let bytes = fs::read(&descriptor015).unwrap();
    let meta = json!({"size":bytes.len(),"sha256":files::sha256(&bytes),"version":"0.1.5"});
    let candidate = update::descriptor(&bytes, &meta).unwrap();
    app.lock().fenix_updates.candidate = candidate;
    files::private_dir(&state_dir.join("fenix-bundles")).unwrap();
    fs::copy(&zip015, state_dir.join("fenix-bundles/0.1.5.zip")).unwrap();

    // 3. The next Fenix launch applies 0.1.5 first.
    let open = job(&app, "open", json!({}));
    report(&root, "3-open-after-015", json!({"job": open}), &app);

    // 4. Open again (field: "always says Last operation failed").
    let open = job(&app, "open", json!({}));
    report(&root, "4-open-again", json!({"job": open}), &app);

    // 5. Restore original profile, then set up again as users do.
    let restore = job(&app, "restore", json!({}));
    report(&root, "5-restore", json!({"job": restore}), &app);
    let install = job(&app, "install", json!({}));
    report(&root, "6-install-again", json!({"job": install}), &app);
    fs::create_dir_all(fenix_exe.parent().unwrap()).unwrap();
    if !fenix_exe.exists() {
        fs::copy(
            runner.join("files/lib/wine/x86_64-windows/hostname.exe"),
            &fenix_exe,
        )
        .unwrap();
    }
    let open = job(&app, "open", json!({}));
    report(&root, "7-open-after-reinstall", json!({"job": open}), &app);
    let configure = job(&app, "configure", json!({}));
    report(&root, "8-configure", json!({"job": configure}), &app);
}

fn staging_dirs(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(root.join("local"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.starts_with("fenix-patch-") || n.starts_with("msfs-prefix.before-fenix-update-")
        })
        .collect();
    names.sort();
    names
}

#[test]
#[ignore = "requires the pinned runner, public Fenix payloads and Wine; creates new prefixes"]
fn fenix_015_recovery() {
    use std::os::unix::fs::PermissionsExt;
    let x = setup("recovery");
    install014(&x);
    signed_in_fenix(&x);
    let configure = job(&x.app, "configure", json!({}));
    assert_eq!(configure["state"], "complete", "{configure}");
    candidate015(&x);
    let before = staging_dirs(&x.root);

    // (a) The automatic update fails after its journal was written: the copy
    // of the profile cannot be completed.
    let blocker = x.prefix.join("drive_c/unreadable-user-file.bin");
    fs::write(&blocker, b"x").unwrap();
    fs::set_permissions(&blocker, fs::Permissions::from_mode(0o000)).unwrap();
    let open = job(&x.app, "open", json!({}));
    report(
        &x.root,
        "a-open-failed-update",
        json!({"job": open, "staging": staging_dirs(&x.root), "status": x.app.lock().fenix_updates.status.clone()}),
        &x.app,
    );
    assert_eq!(
        open["state"], "complete",
        "Fenix must open with the installed patch: {open}"
    );
    let marker: Value = files::json(&x.root.join(fenix::MARKER), 4 << 20).unwrap();
    assert_eq!(marker["state"], "installed");
    assert_eq!(marker["version"], "0.1.4");
    assert_eq!(
        staging_dirs(&x.root),
        before,
        "no unpublished staging copy is left"
    );
    assert_eq!(x.app.lock().fenix_updates.status["state"], "failed");
    let open = job(&x.app, "open", json!({}));
    assert_eq!(open["state"], "complete", "{open}");
    fs::set_permissions(&blocker, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&blocker).unwrap();

    // (b) A failed update journal left by Flightdeck 0.3.10 (state "preparing").
    let root = &x.root;
    let previous = marker.clone();
    let work = "local/fenix-patch-20261109T090000-12345678";
    let backup = "private/fenix-patch-backup-20261109T090000-12345678";
    files::private_dir(&root.join(work)).unwrap();
    files::private_dir(&root.join(backup)).unwrap();
    flightdeck::process::copy_tree(
        &x.prefix,
        &root.join(work).join("prefix"),
        &AtomicBool::new(false),
    )
    .unwrap();
    for name in flightdeck::fenix_bundle::LAUNCH_FILES {
        fs::copy(root.join("tools").join(name), root.join(backup).join(name)).unwrap();
    }
    files::atomic_json(&root.join(backup).join("previous-patch.json"), &previous).unwrap();
    let mut pending = previous.clone();
    pending["state"] = json!("preparing");
    pending["version"] = json!("0.1.5");
    pending["bundle_manifest"] = x.app.lock().fenix_updates.candidate["bundle"].clone();
    pending["work"] = json!(work);
    pending["upgrade_backup"] = json!(backup);
    pending["upgrade_previous_prefix"] =
        json!("local/msfs-prefix.before-fenix-update-20261109T090000-12345678");
    files::atomic_json(&root.join(fenix::MARKER), &pending).unwrap();
    report(root, "b0-stuck", json!({}), &x.app);
    assert_eq!(fenix::snapshot(&x.app)["installed"], false);
    // An old pre-update copy left by an earlier successful update, named by
    // the installed state that update started from.
    let old = "local/msfs-prefix.before-fenix-update-20261030T120000-abcdef12";
    files::private_dir(&root.join(old).join("drive_c")).unwrap();
    let earlier = root.join("private/fenix-patch-backup-20261101T120000-abcdef34");
    files::private_dir(&earlier).unwrap();
    files::atomic_json(
        &earlier.join("previous-patch.json"),
        &json!({"state": "installed", "upgrade_previous_prefix": old}),
    )
    .unwrap();
    // A copy of an interrupted commit holds the newer profile and is kept.
    let kept = "msfs-prefix.before-fenix-update-20261102T120000-abcdef56";
    files::private_dir(&root.join("local").join(kept).join("drive_c")).unwrap();

    let check = update::check(&x.app, true);
    eprintln!("[repro] forced check: {check:?}");
    let at = Instant::now();
    while x.app.lock().fenix_updates.worker.is_some() && at.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(200));
    }
    if x.app.lock().fenix_updates.candidate.is_null() {
        candidate015(&x);
    }
    let open = job(&x.app, "open", json!({}));
    report(
        root,
        "b1-open-recovers-and-updates",
        json!({"job": open, "staging": staging_dirs(root)}),
        &x.app,
    );
    assert_eq!(open["state"], "complete", "{open}");
    let marker: Value = files::json(&root.join(fenix::MARKER), 4 << 20).unwrap();
    assert_eq!(marker["state"], "installed");
    assert_eq!(marker["version"], "0.1.5");
    assert_eq!(marker["configured"], true);
    fenix::verify_installed(root, &marker).unwrap();
    assert_eq!(
        staging_dirs(root)
            .into_iter()
            .filter(|n| n.starts_with("msfs-prefix.before-fenix-update-"))
            .collect::<Vec<_>>(),
        vec![kept.to_string()]
    );
    assert!(!root.join(work).exists(), "abandoned staging copy removed");
    let open = job(&x.app, "open", json!({}));
    assert_eq!(open["state"], "complete", "{open}");
}

#[test]
#[ignore = "requires the pinned runner, public Fenix payloads and Wine; creates new prefixes"]
fn fenix_016_update_chain() {
    let x = setup("chain");
    install014(&x);
    signed_in_fenix(&x);
    let configure = job(&x.app, "configure", json!({}));
    assert_eq!(configure["state"], "complete", "{configure}");
    let installed = |version: &str| {
        let marker: Value = files::json(&x.root.join(fenix::MARKER), 4 << 20).unwrap();
        assert_eq!(marker["state"], "installed");
        assert_eq!(marker["version"], version);
        assert_eq!(marker["configured"], true);
        fenix::verify_installed(&x.root, &marker).unwrap();
    };
    for (version, zip, descriptor) in [
        ("0.1.5", x.zip015.clone(), x.descriptor015.clone()),
        (
            "0.1.6",
            input("REPRO_ZIP_016"),
            input("REPRO_DESCRIPTOR_016"),
        ),
    ] {
        candidate(&x, &zip, &descriptor, version);
        let open = job(&x.app, "open", json!({}));
        report(
            &x.root,
            &format!("c-open-{version}"),
            json!({"job": open, "staging": staging_dirs(&x.root), "status": x.app.lock().fenix_updates.status.clone()}),
            &x.app,
        );
        assert_eq!(open["state"], "complete", "{open}");
        installed(version);
        assert!(
            staging_dirs(&x.root)
                .iter()
                .all(|n| !n.starts_with("msfs-prefix.before-fenix-update-")),
            "{:?}",
            staging_dirs(&x.root)
        );
    }
    let open = job(&x.app, "open", json!({}));
    assert_eq!(open["state"], "complete", "{open}");
    let configure = job(&x.app, "configure", json!({}));
    assert_eq!(configure["state"], "complete", "{configure}");
    installed("0.1.6");
}
