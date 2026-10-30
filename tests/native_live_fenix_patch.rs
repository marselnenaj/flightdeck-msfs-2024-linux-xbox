// SPDX-License-Identifier: MIT
//! Explicit account-free patch setup using a pinned runner and a new runtime.
use flightdeck::{backend::Launcher, bootstrap, fenix, fenix_setup, files, resources, wine::Wine};
use serde_json::json;
use std::{
    fs,
    path::{Component, PathBuf},
    sync::atomic::AtomicBool,
};

#[test]
#[ignore = "requires the pinned original runner archive and public Fenix payload; creates a new prefix"]
fn fresh_fenix_patch_setup() {
    let input = |key| PathBuf::from(std::env::var_os(key).expect("explicit test input"));
    let root = std::path::absolute(input("FLIGHTDECK_TEST_PATCH_ROOT")).expect("root");
    let payload = input("FLIGHTDECK_TEST_PATCH_BUNDLE");
    let archive = input("FLIGHTDECK_TEST_PATCH_ARCHIVE");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("repo");
    assert!(root.starts_with(repo.join("build")));
    assert!(
        root.components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
    );
    assert!(!files::exists(&root), "must use a new runtime directory");
    let lock = resources::json("compat/bootstrap.lock.json").expect("bootstrap lock");
    assert_eq!(
        files::digest(fs::File::open(&archive).expect("archive")).expect("archive hash"),
        lock["runner"]["archive_sha256"]
    );
    files::private_dir(&root.join("private")).expect("private");
    files::private_dir(&root.join("local")).expect("local");
    fs::write(
        root.join("private/synthetic-probe"),
        b"Flightdeck isolated test\n",
    )
    .expect("marker");
    fs::write(
        root.join("private/runtime.json"),
        b"{\"game_id\":\"msfs2024\"}\n",
    )
    .expect("metadata");
    if let Some(cache) = std::env::var_os("FLIGHTDECK_TEST_PATCH_CACHE") {
        let cache = PathBuf::from(cache);
        let destination = root.join("private/fenix-downloads");
        files::private_dir(&destination).expect("private download cache");
        for name in [
            "dotNetFx40_Full_x86_x64.exe",
            "NDP48-x86-x64-AllOS-ENU.exe",
            "Windows6.1-KB2670838-x64.msu",
            "msdelta.dll",
            "windowsdesktop-runtime-10.0.13-win-x64.exe",
        ] {
            fs::copy(cache.join(name), destination.join(name)).expect("cached public prerequisite");
        }
    }
    let unpacked = root.join("local/original-runner");
    files::private_dir(&unpacked).expect("runner extraction directory");
    bootstrap::extract(&archive, &unpacked, &AtomicBool::new(false)).expect("original archive");
    let runner = unpacked.join(
        lock["runner"]["directory"]
            .as_str()
            .expect("runner directory"),
    );
    std::os::unix::fs::symlink(&runner, root.join("runner")).expect("runner link");
    bootstrap::prepare_prefix(
        &runner,
        &root.join("local/msfs-prefix"),
        &AtomicBool::new(false),
    )
    .expect("fresh Wine prefix");
    let prefix = root.join("local/msfs-prefix");
    let cancel = AtomicBool::new(false);
    let mut font_wine = Wine::new(
        &prefix,
        &runner,
        &root.join("private/font-probe.log"),
        &cancel,
    )
    .expect("font probe Wine");
    font_wine.without_fenix();
    font_wine
        .stop_staged()
        .expect("stop before editing the new registry");
    let fonts = prefix.join("drive_c/windows/Fonts");
    assert!(!fonts.is_symlink());
    fs::create_dir_all(&fonts).expect("new font directory");
    let shared = runner.join("files/share/wine/fonts/tahoma.ttf");
    let shared_hash =
        files::digest(fs::File::open(&shared).expect("source font")).expect("source font hash");
    for name in ["tahoma.ttf", "tahomabd.ttf"] {
        let target = fonts.join(name);
        if files::exists(&target) {
            fs::remove_file(&target).expect("replace only the new test font");
        }
    }
    std::os::unix::fs::symlink(&shared, fonts.join("tahoma.ttf")).expect("shared font link");
    std::os::unix::fs::symlink("missing.ttf", fonts.join("tahomabd.ttf"))
        .expect("broken font link");
    let registry = fs::read_to_string(prefix.join("system.reg")).expect("new registry");
    let registry = registry
        .lines()
        .filter(|line| {
            !line.starts_with("\"Tahoma (TrueType)\"")
                && !line.starts_with("\"Tahoma Bold (TrueType)\"")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(prefix.join("system.reg"), registry).expect("remove test font registrations");
    fenix_setup::ui_fonts(&font_wine).expect("materialize fonts and register with real Wine");
    font_wine.stop_staged().expect("stop font probe");
    for name in ["tahoma.ttf", "tahomabd.ttf"] {
        assert!(
            fonts
                .join(name)
                .symlink_metadata()
                .expect("font metadata")
                .is_file()
        );
    }
    assert_eq!(
        files::digest(fs::File::open(&shared).expect("source font")).expect("hash"),
        shared_hash
    );
    resources::install_scripts(&root.join("tools")).expect("original launchers");
    let app = Launcher::new(root.join("private/patch-probe-state"), None).expect("isolated state");
    app.lock().runtime = Some(root.clone());
    let ctx = app.reserve("fenix", "install", true).expect("reservation");
    let result = fenix::install(&ctx, &payload);
    let marker = files::json::<serde_json::Value>(&root.join(fenix::MARKER), 1024 * 1024)
        .unwrap_or_default();
    let report = json!({"passed":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string),
        "patch_state":marker["state"],"version":marker["version"],
        "previous_prefix_exists":marker["previous_prefix"].as_str().is_some_and(|p|root.join(p).is_dir()),
        "linked_fonts_materialized":true,"shared_fonts_unchanged":true});
    fs::write(
        root.join("private/patch-probe-result.json"),
        serde_json::to_vec_pretty(&report).expect("report"),
    )
    .expect("save result");
    assert!(result.is_ok(), "{report}");
    assert_eq!(marker["state"], "installed");
    assert_eq!(report["previous_prefix_exists"], true);
    if let Some(previous) = std::env::var_os("FLIGHTDECK_TEST_PATCH_PREVIOUS_ARCHIVE") {
        interrupted_upgrade(&ctx, &payload, &PathBuf::from(previous), marker);
    }
}

// Seed the preceding public release in the fresh, account-free prefix. Its
// Windows modules are byte-identical; only the native font helper changed.
fn interrupted_upgrade(
    ctx: &flightdeck::backend::Context,
    payload: &std::path::Path,
    archive: &std::path::Path,
    mut previous: serde_json::Value,
) {
    use flightdeck::{fenix_bundle as bundle, process, transaction as tx};
    let root = ctx.root().expect("test root");
    let mut zip =
        zip::ZipArchive::new(fs::File::open(archive).expect("old public ZIP")).expect("ZIP");
    let manifest_name = (0..zip.len())
        .find_map(|i| {
            let entry = zip.by_index(i).expect("entry");
            let name = entry.name().expect("member name");
            name.ends_with("/bundle.json").then(|| name.into_owned())
        })
        .expect("old manifest");
    let old: serde_json::Value =
        serde_json::from_reader(zip.by_name(&manifest_name).expect("old manifest")).expect("JSON");
    let old_lock =
        bundle::select_variant(old.clone(), previous["variant"].as_str()).expect("old variant");
    let current = bundle::manifest(previous["variant"].as_str()).expect("current manifest");
    let pinned = &current["previous_releases"][old["version"].as_str().expect("old version")];
    assert_eq!(pinned["files"], old_lock["files"]);
    assert_eq!(pinned["prefix_files"], old_lock["prefix_files"]);
    assert_eq!(pinned["integration"], old_lock["integration"]);
    let helper_name = format!(
        "{}integration/fenix-runtime",
        manifest_name.strip_suffix("bundle.json").expect("ZIP root")
    );
    let mut helper = Vec::new();
    std::io::Read::read_to_end(
        &mut zip.by_name(&helper_name).expect("old helper"),
        &mut helper,
    )
    .expect("helper bytes");
    assert_eq!(
        files::sha256(&helper),
        old_lock["integration"]["fenix-runtime"]
    );
    bundle::write(
        &root.join("local/msfs-prefix/drive_c/windows/system32/fenix-runtime"),
        &helper,
        0o755,
    )
    .expect("old native helper");
    previous["version"] = old["version"].clone();
    previous["bundle_manifest"] = old;
    previous["bundle_release"] = serde_json::Value::Null;
    // A reboot may renumber st_dev for the original pre-Fenix restore point.
    previous["original_prefix_id"][0] = json!(999999);
    files::atomic_json(&root.join(fenix::MARKER), &previous).expect("old installed marker");
    fenix::verify_installed(root, &previous).expect("verified preceding public patch");
    let prefix = root.join("local/msfs-prefix");
    let active_id = fenix::identity(&prefix).expect("active identity");
    let aircraft = "installed-aircraft-and-settings-sentinel";
    fs::write(
        prefix.join(aircraft),
        b"licensed aircraft and user preferences retained",
    )
    .expect("synthetic user data");
    let work = "local/fenix-patch-20261026T010000-12345678";
    let backup = "private/fenix-patch-backup-20261026T010000-12345678";
    files::private_dir(&root.join(work)).expect("work");
    files::private_dir(&root.join(backup)).expect("backup");
    process::copy_tree(&prefix, &root.join(work).join("prefix"), &ctx.cancel)
        .expect("interrupted staging copy");
    for name in bundle::LAUNCH_FILES {
        fs::copy(root.join("tools").join(name), root.join(backup).join(name))
            .expect("script backup");
    }
    files::atomic_json(&root.join(backup).join("previous-patch.json"), &previous)
        .expect("previous state");
    let mut pending = previous.clone();
    pending["state"] = json!("preparing");
    pending["version"] = current["version"].clone();
    pending["bundle_manifest"] = bundle::manifest(None).expect("target manifest");
    pending["work"] = json!(work);
    pending["upgrade_backup"] = json!(backup);
    pending["upgrade_previous_prefix"] =
        json!("local/msfs-prefix.before-fenix-update-20261026T010000-12345678");
    files::atomic_json(&root.join(fenix::MARKER), &pending).expect("interrupted update");
    assert!(fenix::retryable(root, &pending));
    // Full validation must reject changed installed modules before rewriting the
    // marker, even though the inexpensive UI eligibility check passes.
    let runner = root.join("runner").canonicalize().expect("runner");
    let module = runner.join("files/lib/wine/x86_64-windows/kernel32.dll");
    let saved = fs::read(&module).expect("original kernel32");
    files::atomic(&module, b"modified module").expect("synthetic modification");
    assert!(fenix::install(ctx, payload).is_err());
    assert_eq!(
        files::json::<serde_json::Value>(&root.join(fenix::MARKER), 1024 * 1024)
            .expect("unchanged pending marker"),
        pending
    );
    assert_eq!(
        fenix::identity(&prefix).expect("untouched active profile"),
        active_id
    );
    files::atomic(&module, &saved).expect("restore test module");
    fenix::install(ctx, payload).expect("recover and update through production transaction");
    let installed = files::json::<serde_json::Value>(&root.join(fenix::MARKER), 1024 * 1024)
        .expect("new installed marker");
    assert_eq!(installed["state"], "installed");
    assert_eq!(installed["version"], current["version"]);
    fenix::verify_installed(root, &installed).expect("verified updated patch");
    assert_eq!(
        fs::read(prefix.join(aircraft)).expect("retained aircraft/settings"),
        b"licensed aircraft and user preferences retained"
    );
    assert_eq!(
        fenix::identity(
            &root.join(
                installed["upgrade_previous_prefix"]
                    .as_str()
                    .expect("rollback")
            )
        )
        .expect("retained old prefix"),
        active_id
    );
    assert_eq!(
        tx::digest(&prefix.join("drive_c/windows/system32/fenix-runtime")).expect("new helper"),
        current["integration"]["fenix-runtime"]
    );
    files::atomic_json(&root.join("private/upgrade-recovery-result.json"), &json!({"passed":true,"from":previous["version"],"to":installed["version"],"changed_module_rejected":true,"old_profile_retained":true,"aircraft_and_settings_retained":true})).expect("report");
}
