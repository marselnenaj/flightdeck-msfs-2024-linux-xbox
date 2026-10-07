// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, files, game_package, game_update, games::Game, integrity};
use serde_json::json;
use std::{fs, os::unix::fs::symlink, path::Path};
fn write(path: &Path, data: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, data).unwrap();
}
fn game(root: &Path, version: &str) {
    write(&root.join("MicrosoftGame.Config"),format!(r#"<Game><Identity Name="Test.Game" Publisher="CN=Test" Version="{version}"/><Executable Name="FlightSimulator2024.exe"/><StoreId>9P38D19T7LRV</StoreId></Game>"#).as_bytes());
    write(&root.join("FlightSimulator2024.exe"), b"game fixture");
    write(&root.join(".xodus-streaming.msixvc"), b"package fixture");
}
#[test]
fn strict_package_projection_rejects_revision_and_extra_sensitive_fields() {
    let valid = json!({"schema":1,"store_id":"9P38D19T7LRV","version":"1.2.3.4","version_id":"1.2.3.4.00000000-0000-4000-8000-000000000001","content_id":"00000000-0000-4000-8000-000000000002","package_identity":"a".repeat(64),"size_bytes":1234});
    game_update::validate_info(&valid, Game::Msfs2024).unwrap();
    for (key, bad) in [
        (
            "version_id",
            json!("2.0.0.0.00000000-0000-4000-8000-000000000001"),
        ),
        ("size_bytes", json!(1234.0)),
        ("schema", json!(true)),
        ("store_id", json!("9NRRJLLXM68V")),
    ] {
        let mut value = valid.clone();
        value[key] = bad;
        assert!(game_update::validate_info(&value, Game::Msfs2024).is_err());
    }
    let mut extra = valid;
    extra["signed_url"] = json!("must never be projected");
    assert!(game_update::validate_info(&extra, Game::Msfs2024).is_err());
}
#[test]
fn repair_identity_is_bound_to_the_original_download_index() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    game(root, "1.2.3.4");
    let journal = root.join(".xodus-resume");
    files::private_dir(&journal).unwrap();
    write(&journal.join("lock"), b"");
    let rows: Vec<_> = [
        "MicrosoftGame.Config",
        "FlightSimulator2024.exe",
        ".xodus-streaming.msixvc",
    ]
    .iter()
    .map(|name| {
        let bytes = fs::read(root.join(name)).unwrap();
        json!({"name":name,"length":bytes.len(),"sha256":files::sha256(&bytes)})
    })
    .collect();
    let mut index = json!({"format":1,"source":"xodus-completed-download-v1","package_sha256":"a".repeat(64),"files":rows});
    files::atomic_json(&journal.join("integrity.json"), &index).unwrap();
    files::atomic_json(
        &journal.join("package.json"),
        &json!({"format":1,"package_sha256":"a".repeat(64)}),
    )
    .unwrap();
    integrity::record_installation(root, Game::Msfs2024).unwrap();
    write(&root.join("MicrosoftGame.Config"), b"damaged");
    assert_eq!(
        integrity::installed_identity(root, Game::Msfs2024)
            .unwrap()
            .version,
        "1.2.3.4"
    );
    assert!(integrity::installed_identity(root, Game::Msfs2020).is_err());
    index["files"][0]["sha256"] = json!("b".repeat(64));
    files::atomic_json(&journal.join("integrity.json"), &index).unwrap();
    assert!(integrity::installed_identity(root, Game::Msfs2024).is_err());
}
#[test]
fn pending_commit_receipt_recovers_a_real_atomic_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    let old = root.join("games/.MSFS2024-before-0123456789abcdef0123456789abcdef");
    let new = root.join("private/game-updates/update-test/game");
    game(&old, "1.0.0.0");
    game(&new, "2.0.0.0");
    symlink(&new, root.join("games/MSFS2024")).unwrap();
    write(&root.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    files::atomic_json(&root.join("private/game-update-pending.json"),&json!({"format":1,"previous_entry":old.file_name().unwrap().to_str().unwrap(),"new_target":new,"old_identity":game_package::installed(&old,Game::Msfs2024).unwrap(),"new_identity":game_package::installed(&new,Game::Msfs2024).unwrap()})).unwrap();
    let app = Launcher::new(temp.path().join("state"), Some(root.to_str().unwrap())).unwrap();
    game_update::rollback(&app).unwrap();
    assert_eq!(
        game_package::installed(&root.join("games/MSFS2024"), Game::Msfs2024)
            .unwrap()
            .version,
        "1.0.0.0"
    );
    assert_eq!(
        game_package::installed(&new, Game::Msfs2024)
            .unwrap()
            .version,
        "2.0.0.0"
    );
    assert!(app.lock().active.is_none());
}
#[test]
fn cancelling_an_old_ready_update_cannot_release_another_operation() {
    let temp = tempfile::tempdir().unwrap();
    let app = Launcher::new(temp.path().join("state"), None).unwrap();
    let ctx = app.reserve("setup", "check", false).unwrap();
    app.finish(&ctx, Ok(json!({"mode":"update","state":"ready"})), false);
    let other = app.reserve("proton", "select", false).unwrap();
    app.cancel("setup", &ctx.id).unwrap();
    assert_eq!(app.lock().active.as_ref().unwrap().id, other.id);
}
#[test]
fn a_commit_cannot_be_cancelled_between_check_and_exchange() {
    let temp = tempfile::tempdir().unwrap();
    let app = Launcher::new(temp.path().join("state"), None).unwrap();
    let ctx = app.reserve("setup", "update", false).unwrap();
    ctx.begin_commit("switch_update", "committing").unwrap();
    assert!(app.cancel("setup", &ctx.id).is_err());
    assert!(!ctx.cancel.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn update_capability_uses_local_presence_but_execution_still_verifies_checksum() {
    let temp = tempfile::tempdir().unwrap();
    let local = temp.path().join("bin/xodus-cli");
    write(
        &local,
        b"an unverified executable is only a capability hint",
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&local, fs::Permissions::from_mode(0o700)).unwrap();
    let (advertised, expected, features) = game_update::tools(temp.path(), false).unwrap();
    assert_eq!(advertised, local);
    assert!(
        features
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "package-info-json-v1")
    );
    assert!(flightdeck::game_install::verify_cli(&local, &expected).is_err());
    // A machine with the pinned bundle may select that verified fallback;
    // otherwise execution is unavailable. Neither case admits the local fake.
    if let Ok((selected, hash, _)) = game_update::tools(temp.path(), true) {
        assert_ne!(selected, local);
        assert_eq!(hash, expected);
        flightdeck::game_install::verify_cli(&selected, &hash).unwrap();
    }
}
