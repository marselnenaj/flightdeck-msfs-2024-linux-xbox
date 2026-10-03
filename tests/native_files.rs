// SPDX-License-Identifier: MIT
use flightdeck::{files, games::Game, integrity};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::atomic::AtomicBool,
};

#[test]
fn runtime_metadata_accepts_legacy_but_rejects_invalid_and_linked_identity() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    assert_eq!(Game::for_runtime(root).expect("legacy"), Game::Msfs2024);
    fs::create_dir(root.join("private")).expect("private");
    let config = root.join("private/runtime.json");
    fs::write(&config, b"{\"game_id\":\"msfs2020\"}").expect("config");
    assert_eq!(Game::for_runtime(root).expect("2020"), Game::Msfs2020);
    for bad in [
        "[]",
        "null",
        "{\"game_id\":42}",
        "{\"game_id\":\"unknown\"}",
    ] {
        fs::write(&config, bad).expect("config");
        assert!(Game::for_runtime(root).is_err());
    }
    fs::remove_file(&config).expect("remove");
    symlink(root.join("outside.json"), &config).expect("symlink");
    assert!(Game::for_runtime(root).is_err());
}

#[test]
fn atomic_settings_and_exclusive_lease_survive_early_drop() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    files::private_dir(root).expect("private settings directory");
    let config = root.join("config.json");
    files::atomic_json(&config, &json!({"schema":1})).expect("write");
    assert_eq!(
        files::json::<Value>(&config, 1024).expect("read")["schema"],
        1
    );
    assert_eq!(
        fs::metadata(&config)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let lock = root.join("play.lock");
    let first = files::Lease::acquire(&lock, true).expect("lease");
    assert!(files::Lease::acquire(&lock, false).is_err());
    drop(first);
    assert!(files::Lease::acquire(&lock, false).is_ok());
    fs::remove_file(&lock).expect("remove");
    symlink(&config, &lock).expect("symlink");
    assert!(files::Lease::acquire(&lock, true).is_err());
}

fn fixture(root: &std::path::Path) -> Value {
    let journal = root.join(".xodus-resume");
    fs::create_dir(&journal).expect("journal");
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o700)).expect("mode");
    fs::write(journal.join("lock"), b"").expect("lock");
    fs::create_dir(root.join("data")).expect("data");
    let mut entries = vec![];
    for (name, bytes) in [
        (".xodus-streaming.msixvc", b"sealed package".as_slice()),
        ("data/content", b"content".as_slice()),
    ] {
        fs::write(root.join(name), bytes).expect("file");
        entries.push(json!({"name":name,"length":bytes.len(),"sha256":files::sha256(bytes)}));
    }
    let digest = files::sha256(b"package identity");
    files::atomic_json(
        &journal.join("package.json"),
        &json!({"format":1,"package_sha256":digest}),
    )
    .expect("package");
    let value = json!({"format":1,"source":"xodus-completed-download-v1","package_sha256":digest,"files":entries});
    files::atomic_json(&journal.join("integrity.json"), &value).expect("index");
    value
}

#[test]
fn original_checksums_detect_changes_missing_files_and_symlink_escapes() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    fixture(root);
    let cancel = AtomicBool::new(false);
    assert!(
        integrity::verify(root, &cancel, |_| {})
            .expect("verify")
            .healthy
    );
    fs::write(root.join("data/content"), b"changed").expect("change");
    assert_eq!(
        integrity::verify(root, &cancel, |_| {})
            .expect("verify")
            .changed,
        1
    );
    fs::remove_file(root.join("data/content")).expect("remove");
    assert_eq!(
        integrity::verify(root, &cancel, |_| {})
            .expect("verify")
            .missing,
        1
    );
    let external = tempfile::tempdir().expect("external");
    fs::write(external.path().join("content"), b"content").expect("external file");
    fs::remove_dir(root.join("data")).expect("remove directory");
    symlink(external.path(), root.join("data")).expect("symlink");
    assert_eq!(
        integrity::verify(root, &cancel, |_| {})
            .expect("verify")
            .unreadable,
        1
    );
    assert_eq!(
        fs::read(external.path().join("content")).expect("external read"),
        b"content"
    );
    assert!(integrity::verify(root, &AtomicBool::new(true), |_| {}).is_err());
}

#[test]
fn malformed_indexes_and_active_downloads_never_supply_a_baseline() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let mut index = fixture(root);
    fs::set_permissions(
        root.join(".xodus-resume/lock"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("mode");
    assert!(files::Lease::acquire(&root.join(".xodus-resume/lock"), false).is_err());
    fs::set_permissions(
        root.join(".xodus-resume/lock"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("mode");
    let lock = files::Lease::acquire(&root.join(".xodus-resume/lock"), false).expect("lock");
    assert!(!integrity::available(root));
    drop(lock);
    index["files"][1]["name"] = json!("../escape");
    files::atomic_json(&root.join(".xodus-resume/integrity.json"), &index).expect("index");
    assert!(!integrity::available(root));
}
