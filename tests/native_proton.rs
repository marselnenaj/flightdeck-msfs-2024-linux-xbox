// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, fenix, fenix_bundle, files, proton, transaction as tx};
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
/// Small AMD64 PE with a real export directory. Its private copy must keep
/// every byte except the module's internal export name.
fn json_dll(version: &str, name: &str) -> Vec<u8> {
    let mut data = vec![0; 0x800];
    data[..2].copy_from_slice(b"MZ");
    for (offset, value) in [
        (60, 0x80_u32),
        (0x80, 0x4550),
        (0x98 + 108, 16),
        (0x98 + 112, 0x1000),
        (0x98 + 116, 0x100),
        (0x188 + 12, 0x1000),
        (0x188 + 16, 0x600),
        (0x188 + 20, 0x200),
        (0x200 + 12, 0x1040),
        (0x200 + 20, 1),
        (0x200 + 24, 1),
        (0x200 + 28, 0x1028),
        (0x200 + 32, 0x102c),
        (0x200 + 36, 0x1030),
        (0x228, 0x1100),
        (0x22c, 0x1060),
    ] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [(0x84, 0x8664_u16), (0x86, 1), (0x94, 240), (0x98, 0x20b)] {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    data[0x240..0x240 + name.len()].copy_from_slice(name.as_bytes());
    let factory = b"DllGetActivationFactory\0";
    data[0x260..0x260 + factory.len()].copy_from_slice(factory);
    data[0x300] = 0xc3;
    data[0x400..0x400 + version.len()].copy_from_slice(version.as_bytes());
    data
}
fn runner(path: &Path, version: &str) {
    for name in proton::JSON_LIBRARIES {
        write(
            &path.join("files/lib/wine/x86_64-windows").join(name),
            &json_dll(version, name),
        );
    }
    for name in ["wine", "wineserver"] {
        let target = path.join("files/bin").join(name);
        write(&target, b"#!/bin/sh\nexit 0\n");
        fs::set_permissions(target, fs::Permissions::from_mode(0o700)).unwrap();
    }
    write(&path.join("version"), version.as_bytes());
    for arch in ["x86_64-windows", "i386-windows"] {
        for (lib, names) in [
            ("dxvk", &["dxgi", "d3d11", "d3d10core"][..]),
            ("vkd3d-proton", &["d3d12", "d3d12core"][..]),
        ] {
            for name in names {
                write(
                    &path.join(format!("files/lib/wine/{lib}/{arch}/{name}.dll")),
                    format!("{version}/{name}").as_bytes(),
                );
            }
        }
    }
}
fn fixture(base: &Path) -> (PathBuf, std::sync::Arc<Launcher>) {
    let root = base.join("runtime");
    let original = base.join("original-runner");
    runner(&original, "synthetic-original");
    write(&root.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    write(
        &root.join("tools/launch-msfs.sh"),
        flightdeck::resources::asset("scripts/runtime/launch-msfs.sh").unwrap(),
    );
    for name in ["system.reg", "user.reg"] {
        write(
            &root.join("local/msfs-prefix").join(name),
            b"original registry",
        );
    }
    files::private_dir(&root.join("local/msfs-prefix/drive_c/windows/syswow64")).unwrap();
    files::private_dir(&root.join("private")).unwrap();
    let system = root.join("local/msfs-prefix/drive_c/windows/system32");
    for name in proton::BRIDGE {
        write(&system.join(name), name.as_bytes());
    }
    let hashes: Value = proton::BRIDGE
        .into_iter()
        .map(|v| (v.to_string(), json!(files::sha256(v.as_bytes()))))
        .collect();
    files::atomic_json(&root.join("private/import-manifest.json"),&json!({"artifacts":{"files":{"runtime/xgameruntime.dll":hashes["xgameruntime.dll"],"builtin/x86_64-windows/xodus_store_test.dll":hashes["xodus_store_test.dll"]}},"original_runtime_sha256":hashes["xgameruntime_original.dll"]})).unwrap();
    symlink(&original, root.join("runner")).unwrap();
    let app = Launcher::new(base.join("state"), Some(root.to_str().unwrap())).unwrap();
    (root, app)
}
#[test]
fn current_native_wrapper_allows_the_public_proton_switch_without_fenix() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let candidate = temp.path().join("alternative");
    runner(&candidate, "synthetic-selected");
    assert!(proton::supports_switch(&root).unwrap());
    proton::start(
        &app,
        &json!({"runtime_path":root,"mode":"proton","path":candidate}),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let job = app.job("proton");
        if job["state"] == "complete" {
            break;
        }
        assert_ne!(job["state"], "failed", "{job}");
        assert!(std::time::Instant::now() < deadline, "{job}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(proton::snapshot(&app)["selected"], "synthetic-selected");
    for (source, target) in proton::JSON_LIBRARIES
        .into_iter()
        .zip(["fd-json.dll", "fd-types.dll"])
    {
        assert_eq!(
            fs::read(
                root.join("local/msfs-prefix/drive_c/windows/system32")
                    .join(target)
            )
            .unwrap(),
            json_dll("synthetic-original", target)
        );
        assert_eq!(
            fs::read(
                root.join("runner/files/lib/wine/x86_64-windows")
                    .join(source)
            )
            .unwrap(),
            json_dll("synthetic-selected", source)
        );
        assert_eq!(
            fs::read(candidate.join("files/lib/wine/x86_64-windows").join(source)).unwrap(),
            json_dll("synthetic-selected", source)
        );
    }
    write(
        &root.join("tools/launch-msfs.sh"),
        b"#!/bin/sh\necho unknown legacy launcher\n",
    );
    assert!(!proton::supports_switch(&root).unwrap());
    write(
        &root.join("tools/launch-msfs.sh"),
        b"#!/bin/sh\nexport FLIGHTDECK_PROTON_LOADER=portable\n",
    );
    assert!(proton::supports_switch(&root).unwrap());
}
#[test]
fn existing_selected_profiles_gain_json_support_without_replacing_runner_dlls() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let candidate = temp.path().join("alternative");
    runner(&candidate, "synthetic-selected");
    let ctx = app.reserve("proton", "select", true).unwrap();
    let (fresh, selected) = proton::prepare(
        &ctx,
        &proton::inspect(candidate.to_str().unwrap()).unwrap(),
        None,
        false,
    )
    .unwrap();
    proton::switch(&ctx, &fresh, &selected, false).unwrap();
    app.finish(&ctx, Ok(json!({"state":"complete"})), false);
    let prefix = root.join("local/msfs-prefix");
    let json = prefix.join("drive_c/windows/system32/fd-json.dll");
    fs::remove_file(prefix.join(".flightdeck-json.json")).unwrap();
    fs::write(&json, b"old private copy").unwrap();
    proton::prepare_game_runtime(&root).unwrap();
    assert_eq!(
        fs::read(&json).unwrap(),
        json_dll("synthetic-original", "fd-json.dll")
    );
    assert_eq!(
        fs::read(root.join("runner/files/lib/wine/x86_64-windows/windows.web.dll")).unwrap(),
        json_dll("synthetic-selected", "windows.web.dll")
    );
    // A subsequent Wine update may restore stock WinRT registrations. The
    // marker alone must not make those stub factories appear repaired.
    fs::write(prefix.join("system.reg"), b"stock registrations").unwrap();
    proton::prepare_game_runtime(&root).unwrap();
    assert!(prefix.join(".flightdeck-json.json").is_file());
    let original_json = temp
        .path()
        .join("original-runner/files/lib/wine/x86_64-windows/windows.web.dll");
    for offset in [60, 0x84, 0x188 + 20, 0x200 + 12, 0x228, 0x240, 0x260] {
        let mut invalid = json_dll("synthetic-original", "windows.web.dll");
        invalid[offset..offset + 4].fill(0xff);
        fs::write(&original_json, invalid).unwrap();
        fs::write(&json, b"must remain on validation failure").unwrap();
        assert!(
            proton::prepare_game_runtime(&root).is_err(),
            "offset {offset}"
        );
        assert_eq!(
            fs::read(&json).unwrap(),
            b"must remain on validation failure"
        );
    }
    fs::write(
        &original_json,
        json_dll("synthetic-original", "windows.web.dll"),
    )
    .unwrap();
    fs::remove_file(
        temp.path()
            .join("original-runner/files/lib/wine/x86_64-windows/wintypes.dll"),
    )
    .unwrap();
    fs::write(&json, b"must remain on validation failure").unwrap();
    assert!(proton::prepare_game_runtime(&root).is_err());
    assert_eq!(
        fs::read(&json).unwrap(),
        b"must remain on validation failure"
    );
}
#[test]
fn discovery_returns_displayable_labels_for_steam_and_custom_runners() {
    let temp = tempfile::tempdir().unwrap();
    let common = temp.path().join(".local/share/Steam/steamapps/common");
    let custom = temp.path().join(".local/share/Steam/compatibilitytools.d");
    runner(
        &common.join("Proton - Experimental"),
        "experimental-11.0-test",
    );
    runner(
        &custom.join("proton-cachyos-10.0-sunset"),
        "cachyos-10.0-sunset",
    );
    runner(&custom.join("Proton für Tests"), "synthetic-unicode");
    let choices = proton::discover(temp.path());
    for name in [
        "Proton - Experimental",
        "proton-cachyos-10.0-sunset",
        "Proton für Tests",
    ] {
        let item = choices
            .iter()
            .find(|item| item["label"] == name)
            .expect("runner label is a JSON string");
        assert!(item["path"].as_str().unwrap().ends_with(name));
        assert!(item["version"].is_string());
        assert!(item["fenix"].is_boolean());
    }
}
#[test]
fn repeated_switch_and_default_restore_carry_forward_new_addons() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let candidate = temp.path().join("new-runner");
    runner(&candidate, "synthetic-alternative");
    let baseline = proton::inspect(
        root.join("runner")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .unwrap();
    let ctx = app.reserve("proton", "select", true).unwrap();
    let (fresh, selected) = proton::prepare(
        &ctx,
        &proton::inspect(candidate.to_str().unwrap()).unwrap(),
        None,
        false,
    )
    .unwrap();
    proton::switch(&ctx, &fresh, &selected, false).unwrap();
    app.finish(&ctx, Ok(json!({"state":"complete"})), false);
    let backup = root.join(selected["base_prefix"].as_str().unwrap());
    assert!(backup.join("system.reg").is_file());
    assert!(proton::selection(&root).unwrap().is_some());
    assert_eq!(proton::snapshot(&app)["selected_path"], json!(candidate));
    let addon = "local/msfs-prefix/drive_c/Program Files/SyntheticAddon/after-switch.dat";
    write(&root.join(addon), b"user add-on after first switch");
    let ctx = app.reserve("proton", "select", true).unwrap();
    let (fresh, selected2) = proton::prepare(
        &ctx,
        &proton::inspect(candidate.to_str().unwrap()).unwrap(),
        None,
        false,
    )
    .unwrap();
    assert_eq!(selected2["base_prefix"], selected["base_prefix"]);
    proton::switch(&ctx, &fresh, &selected2, false).unwrap();
    app.finish(&ctx, Ok(json!({"state":"complete"})), false);
    assert_eq!(
        fs::read(root.join(addon)).unwrap(),
        b"user add-on after first switch"
    );
    let ctx = app.reserve("proton", "restore", true).unwrap();
    let (fresh, prepared) = proton::prepare(&ctx, &baseline, None, true).unwrap();
    proton::switch(&ctx, &fresh, &prepared, true).unwrap();
    assert!(proton::selection(&root).unwrap().is_none());
    assert_eq!(
        fs::read(root.join(addon)).unwrap(),
        b"user add-on after first switch"
    );
    for name in proton::BRIDGE {
        assert_eq!(
            tx::digest(
                &root
                    .join("local/msfs-prefix/drive_c/windows/system32")
                    .join(name)
            )
            .unwrap(),
            files::sha256(name.as_bytes())
        );
    }
    assert!(backup.exists());
}
fn journal(root: &Path) -> (PathBuf, Value) {
    let work = root.join("local/proton-tests/0123456789abcdef0123456789abcdef");
    let prefix = work.join("previous-prefix");
    files::private_dir(&prefix).unwrap();
    write(&prefix.join("probe"), b"new profile");
    runner(&work.join("runner"), "synthetic-new");
    let selected = json!({"schema":1,"version":"synthetic-new","base_runner":root.join("runner").canonicalize().unwrap(),"runner":"local/proton-tests/0123456789abcdef0123456789abcdef/runner","base_prefix":"local/proton-tests/0123456789abcdef0123456789abcdef/previous-prefix","base_hashes":proton::bridge_hashes(root).unwrap()});
    let data = json!({"schema":1,"backup":selected["base_prefix"],"runner":work.join("runner"),"previous_runner":root.join("runner").canonicalize().unwrap(),"before":fenix::identity(&root.join("local/msfs-prefix")).unwrap(),"after":fenix::identity(&prefix).unwrap(),"selection":selected,"fenix_state":null,"scripts":{}});
    files::atomic_json(&root.join("private/proton-switch.json"), &data).unwrap();
    (prefix, data)
}
#[test]
fn interrupted_exchange_recovers_idempotently_before_or_after_directory_swap() {
    for exchanged in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (root, _) = fixture(temp.path());
        let (backup, data) = journal(&root);
        if exchanged {
            tx::exchange(&root.join("local/msfs-prefix"), &backup).unwrap();
        }
        proton::recover(&root).unwrap();
        proton::recover(&root).unwrap();
        assert_eq!(
            fenix::identity(&root.join("local/msfs-prefix")).unwrap(),
            data["after"]
        );
        assert_eq!(fenix::identity(&backup).unwrap(), data["before"]);
        assert!(proton::selection(&root).unwrap().is_some());
        assert!(!root.join("private/proton-switch.json").exists());
    }
}
#[test]
fn unreclaimable_backup_does_not_strand_switch_recovery_or_return_to_default() {
    use std::os::unix::fs::FileTypeExt;

    for already_published in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (root, app) = fixture(temp.path());
        let candidate = temp.path().join("alternative");
        runner(&candidate, "synthetic-selected");
        write(&root.join("local/msfs-prefix/addon"), b"user add-on");
        let ctx = app.reserve("proton", "select", true).unwrap();
        let (fresh, selected) = proton::prepare(
            &ctx,
            &proton::inspect(candidate.to_str().unwrap()).unwrap(),
            None,
            false,
        )
        .unwrap();
        let prefix = root.join("local/msfs-prefix");
        // A special entry fails the same conservative removal inventory as a
        // foreign owner or nested mount, without changing ownership or mounts.
        rustix::fs::mknodat(
            rustix::fs::CWD,
            prefix.join("keep-fifo"),
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .unwrap();
        if already_published {
            // Recreate the durable state left by an older release after the
            // exchange and settings publication, before backup recording.
            files::atomic_json(
                &root.join("private/proton-switch.json"),
                &json!({
                    "schema":1,"backup":selected["base_prefix"],
                    "runner":root.join(selected["runner"].as_str().unwrap()),
                    "previous_runner":root.join("runner").canonicalize().unwrap(),
                    "base_runner":selected["base_runner"],
                    "before":fenix::identity(&prefix).unwrap(),
                    "after":fenix::identity(&fresh).unwrap(),
                    "selection":selected,"fenix_state":null,"scripts":selected["scripts"]
                }),
            )
            .unwrap();
            tx::exchange(&prefix, &fresh).unwrap();
            fenix::replace_link(
                &root.join("runner"),
                &root.join(selected["runner"].as_str().unwrap()),
            )
            .unwrap();
            files::atomic_json(&root.join("private/proton-selection.json"), &selected).unwrap();
        } else {
            proton::switch(&ctx, &fresh, &selected, false).unwrap();
        }
        proton::recover(&root).unwrap();
        proton::recover(&root).unwrap();
        assert!(!root.join("private/proton-switch.json").exists());
        assert_eq!(proton::snapshot(&app)["selected"], "synthetic-selected");
        let record: Value =
            files::json(&fresh.parent().unwrap().join("cache.json"), 65536).unwrap();
        let name = fresh.file_name().unwrap().to_str().unwrap();
        assert_eq!(record["profiles"][name], fenix::identity(&fresh).unwrap());
        assert!(record["profile_fingerprints"][name].is_null());
        assert!(
            fresh
                .join("keep-fifo")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_fifo()
        );
        assert!(
            flightdeck::owned_tree::inventory(&fresh, &std::sync::atomic::AtomicBool::new(false))
                .is_err()
        );
        app.finish(&ctx, Ok(json!({"state":"complete"})), false);
        // Exercise the public Return button path, including optional cleanup.
        let result = select(&app, &root, None);
        assert_ne!(result["cleanup"]["skipped"], true, "{result}");
        proton::check(&root).unwrap();
        assert!(proton::selection(&root).unwrap().is_none());
        assert_eq!(
            fs::read(root.join("local/msfs-prefix/addon")).unwrap(),
            b"user add-on"
        );
        assert_eq!(fs::read(fresh.join("addon")).unwrap(), b"user add-on");
        assert!(
            fresh
                .join("keep-fifo")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_fifo()
        );
    }
}
#[test]
fn changed_prefix_or_launch_script_stops_recovery_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let (root, _) = fixture(temp.path());
    let (backup, mut data) = journal(&root);
    let before = fenix::identity(&root.join("local/msfs-prefix")).unwrap();
    data["before"] = json!([0, 0]);
    files::atomic_json(&root.join("private/proton-switch.json"), &data).unwrap();
    assert!(proton::recover(&root).is_err());
    assert_eq!(
        fenix::identity(&root.join("local/msfs-prefix")).unwrap(),
        before
    );
    data["before"] = before.clone();
    let source = backup.parent().unwrap().join("launch-msfs.sh");
    write(&source, fenix_bundle::script("launch-msfs.sh").unwrap());
    // The native wrapper is now the normal fixture; replace it to simulate
    // an external edit that matches neither side of the recovery journal.
    write(
        &root.join("tools/launch-msfs.sh"),
        b"#!/bin/sh\necho externally modified\n",
    );
    data["scripts"] = json!({"launch-msfs.sh":{"before":"0".repeat(64),"after":tx::digest(&source).unwrap(),"source":source.strip_prefix(&root).unwrap()}});
    files::atomic_json(&root.join("private/proton-switch.json"), &data).unwrap();
    assert!(proton::recover(&root).is_err());
    assert_eq!(
        fenix::identity(&root.join("local/msfs-prefix")).unwrap(),
        before
    );
}
#[test]
fn current_loader_is_independent_of_the_older_fenix_archive() {
    let bytes = fenix_bundle::script("xodus-wine-launch").unwrap();
    assert!(String::from_utf8_lossy(bytes).contains("flightdeck-helper\" wine-launch"));
    assert!(!String::from_utf8_lossy(bytes).contains("python"));
    assert_ne!(
        files::sha256(bytes),
        fenix_bundle::manifest(None).unwrap()["integration"]["xodus-wine-launch"]
    );
}
#[test]
fn runner_escapes_and_ambiguous_selection_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("runner");
    runner(&path, "test");
    let file = path.join("files/bin/wine");
    fs::remove_file(&file).unwrap();
    symlink("/bin/sh", &file).unwrap();
    assert!(proton::inspect(path.to_str().unwrap()).is_err());
    assert!(proton::validate_selection(&json!({"schema":true})).is_err());
}

fn select(app: &std::sync::Arc<Launcher>, root: &Path, candidate: Option<&Path>) -> Value {
    proton::start(app, &json!({"runtime_path":root,"mode":if candidate.is_some(){"proton"}else{"default"},"path":candidate})).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let job = app.job("proton");
        if job["state"] == "complete" {
            return job;
        }
        assert_ne!(job["state"], "failed", "{job}");
        assert!(std::time::Instant::now() < deadline, "{job}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn alternating_builds_reuse_runners_keep_latest_addons_and_bound_profile_backups() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let a = temp.path().join("proton-a");
    let b = temp.path().join("proton-b");
    runner(&a, "synthetic-a");
    runner(&b, "synthetic-b");
    let mut identities = std::collections::BTreeMap::new();
    for (number, candidate) in [
        Some(a.as_path()),
        Some(b.as_path()),
        Some(a.as_path()),
        None,
        Some(b.as_path()),
        Some(a.as_path()),
        None,
        Some(b.as_path()),
        None,
        Some(a.as_path()),
    ]
    .into_iter()
    .enumerate()
    {
        let job = select(&app, &root, candidate);
        let active = root.join("runner").canonicalize().unwrap();
        let id = fenix::identity(&active).unwrap();
        if let Some(old) = identities.insert(candidate.map(Path::to_path_buf), id.clone()) {
            assert_eq!(old, id, "runner copied again");
        }
        for old in 0..number {
            assert!(
                root.join(format!("local/msfs-prefix/addon-{old}"))
                    .is_file()
            );
        }
        write(
            &root.join(format!("local/msfs-prefix/addon-{number}")),
            b"latest user installation",
        );
        assert_ne!(job["cleanup"]["skipped"], true, "cycle {number}: {job}");
        let profiles: Vec<_> = fs::read_dir(root.join("local/proton-tests"))
            .unwrap()
            .flat_map(|e| {
                ["previous-prefix", "next-prefix"].map(|n| e.as_ref().unwrap().path().join(n))
            })
            .filter(|p| p.exists())
            .collect();
        assert_eq!(profiles.len(), 1, "{profiles:?}");
        assert!(root.join("local/msfs-prefix").is_dir());
    }
    assert_eq!(
        fs::read_dir(root.join("local/proton-tests"))
            .unwrap()
            .count(),
        3
    );
    // A build changed in place gets a separate copy, including changes outside
    // inspect()'s small launch-file list.
    write(&a.join("files/new-codec.so"), b"new codec build");
    select(&app, &root, Some(&a));
    assert_eq!(
        fs::read_dir(root.join("local/proton-tests"))
            .unwrap()
            .count(),
        4
    );
}

#[test]
fn legacy_cleanup_removes_only_unreferenced_recognized_slots_and_never_follows_links() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let mut slots = Vec::new();
    for number in 0..6 {
        let work = root.join(format!("local/proton-tests/{number:032x}"));
        runner(&work.join("runner"), "synthetic-old");
        write(&work.join("previous-prefix/addon"), b"old user profile");
        files::atomic_json(&work.join("provenance.json"),&json!({"source":temp.path().join("source"),"version":"synthetic-old","files":proton::inspect(work.join("runner").to_str().unwrap()).unwrap()["hashes"]})).unwrap();
        slots.push(work);
    }
    files::atomic_json(
        &root.join("private/retained.json"),
        &json!({"restore":slots[1].join("previous-prefix")}),
    )
    .unwrap();
    write(
        &slots[2].join("unexpected-personal-data"),
        b"preserve unknown entries",
    );
    let external = temp.path().join("outside");
    write(&external.join("keep"), b"external data");
    symlink(&external, slots[0].join("previous-prefix/external-link")).unwrap();
    // A changed runner invalidates legacy provenance; preserve the whole slot.
    write(&slots[3].join("runner/files/bin/wine"), b"changed runner");
    // A second configured simulator uses this runner, even while idle.
    let other = temp.path().join("other-runtime");
    files::private_dir(&other.join("private")).unwrap();
    symlink(slots[4].join("runner"), other.join("runner")).unwrap();
    app.lock().known.insert("msfs2020".into(), other);
    symlink(
        slots[5].join("previous-prefix"),
        root.join("local/msfs-prefix/linked-community-package"),
    )
    .unwrap();
    let provenance = slots[0].join("provenance.json");
    let record: Value = files::json(&provenance, 65536).unwrap();
    files::atomic_json(&provenance, &record).unwrap();
    let candidate = temp.path().join("new");
    runner(&candidate, "synthetic-new");
    select(&app, &root, Some(&candidate));
    assert!(!slots[0].exists());
    assert!(external.join("keep").is_file());
    assert!(slots[4].join("runner/files/bin/wine").is_file());
    assert!(!slots[4].join("previous-prefix").exists());
    assert!(slots[5].join("previous-prefix/addon").is_file());
    for slot in &slots[1..4] {
        assert!(slot.join("previous-prefix/addon").is_file());
    }
}

#[test]
fn deleted_old_trial_runner_does_not_prevent_return_to_default_or_lose_current_addons() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let candidate = temp.path().join("alternative");
    runner(&candidate, "synthetic-selected");
    select(&app, &root, Some(&candidate));
    write(&root.join("local/msfs-prefix/addon"), b"new addon");
    fs::remove_dir_all(root.join("local/proton-tests")).unwrap();
    assert!(proton::check(&root).is_err());
    select(&app, &root, None);
    proton::check(&root).unwrap();
    assert!(proton::selection(&root).unwrap().is_none());
    assert_eq!(
        fs::read(root.join("local/msfs-prefix/addon")).unwrap(),
        b"new addon"
    );
}

#[test]
fn user_modified_backup_is_retained_when_later_switches_make_it_obsolete() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    runner(&a, "synthetic-a");
    runner(&b, "synthetic-b");
    select(&app, &root, Some(&a));
    let backup = root.join(
        proton::selection(&root).unwrap().unwrap()["base_prefix"]
            .as_str()
            .unwrap(),
    );
    write(
        &backup.join("user-added-after-backup"),
        b"keep this newer personal data",
    );
    select(&app, &root, Some(&b));
    assert_eq!(
        fs::read(backup.join("user-added-after-backup")).unwrap(),
        b"keep this newer personal data"
    );
}
