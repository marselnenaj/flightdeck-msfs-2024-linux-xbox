// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, files, mods};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
fn write(path: &Path, bytes: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, bytes).unwrap();
}
fn fixture(base: &Path) -> (PathBuf, Arc<Launcher>) {
    let root = base.join("runtime");
    let community = base.join("Community");
    files::private_dir(&community).unwrap();
    files::private_dir(&root.join("local/msfs-prefix/drive_c")).unwrap();
    write(
        &root.join("local/msfs-prefix/system.reg"),
        b"synthetic registry",
    );
    write(&root.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    files::private_dir(&root.join("private")).unwrap();
    files::atomic_json(
        &root.join("private/runtime.json"),
        &json!({"game_id":"msfs2024","community_path":community}),
    )
    .unwrap();
    let app = Launcher::new(base.join("state"), Some(root.to_str().unwrap())).unwrap();
    (community, app)
}
fn addon(folder: &Path) {
    write(
        &folder.join("manifest.json"),
        br#"{"title":"Synthetic aircraft","package_version":"1.0.0"}"#,
    );
    write(&folder.join("data/nested.txt"), b"addon data");
}
fn wait(app: &Arc<Launcher>, state: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let job = app.job("mods");
        if job["state"] == state {
            return job;
        }
        assert!(Instant::now() < deadline, "expected {state}: {job}");
        assert!(job["state"] != "failed" || state == "failed", "{job}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn preview(app: &Arc<Launcher>, name: &str) -> Value {
    mods::preview_remove(
        app,
        &json!({"runtime_path":app.root().unwrap(),"addon_id":name}),
    )
    .unwrap();
    wait(app, "ready")
}
#[test]
fn reviewed_addon_removal_leaves_neighbours_and_external_link_targets_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let (community, app) = fixture(temp.path());
    addon(&community.join("aircraft"));
    addon(&community.join("retained"));
    write(&temp.path().join("outside/keep.txt"), b"must survive");
    symlink(
        temp.path().join("outside"),
        community.join("aircraft/external"),
    )
    .unwrap();
    let plan = preview(&app, "aircraft");
    assert_eq!(plan["entry_path"], json!(community.join("aircraft")));
    assert_eq!(plan["is_link"], false);
    assert!(plan["bytes"].as_u64().unwrap() > 0);
    assert!(app.reserve("setup", "check", true).is_err());
    mods::remove(&app, &json!({"job_id":plan["id"]})).unwrap();
    wait(&app, "complete");
    assert!(!community.join("aircraft").exists());
    assert!(community.join("retained/manifest.json").is_file());
    assert_eq!(
        fs::read(temp.path().join("outside/keep.txt")).unwrap(),
        b"must survive"
    );
    assert_eq!(fs::read_dir(&community).unwrap().count(), 1);
}
#[test]
fn linked_addon_removal_unlinks_only_the_community_entry() {
    let temp = tempfile::tempdir().unwrap();
    let (community, app) = fixture(temp.path());
    let external = temp.path().join("official-installer-managed");
    addon(&external);
    symlink(&external, community.join("linked-aircraft")).unwrap();
    let plan = preview(&app, "linked-aircraft");
    assert_eq!(plan["is_link"], true);
    assert_eq!(plan["bytes"], 0);
    mods::remove(&app, &json!({"job_id":plan["id"]})).unwrap();
    wait(&app, "complete");
    assert!(fs::symlink_metadata(community.join("linked-aircraft")).is_err());
    assert!(external.join("manifest.json").is_file());
}
#[test]
fn changed_or_replaced_entries_are_never_deleted_with_an_old_plan() {
    for replacement in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (community, app) = fixture(temp.path());
        let path = community.join("aircraft");
        addon(&path);
        let plan = preview(&app, "aircraft");
        if replacement {
            fs::rename(&path, community.join("saved-original")).unwrap();
            addon(&path);
        } else {
            write(
                &path.join("new-user-data.txt"),
                b"new settings since review",
            );
        }
        mods::remove(&app, &json!({"job_id":plan["id"]})).unwrap();
        wait(&app, "failed");
        assert!(path.join("manifest.json").is_file());
        if !replacement {
            assert!(path.join("new-user-data.txt").is_file());
        }
    }
}
#[test]
fn changed_links_paths_jobs_and_cancelled_reviews_cannot_remove_an_addon() {
    let temp = tempfile::tempdir().unwrap();
    let (community, app) = fixture(temp.path());
    addon(&community.join("retained"));
    for name in [
        "../retained",
        "..",
        "/etc",
        "retained/manifest.json",
        ".flightdeck-removal-123",
    ] {
        assert!(
            mods::preview_remove(
                &app,
                &json!({"runtime_path":app.root().unwrap(),"addon_id":name})
            )
            .is_err()
        );
    }
    let plan = preview(&app, "retained");
    assert!(mods::remove(&app, &json!({"job_id":"wrong-preview"})).is_err());
    mods::discard_remove(&app, &json!({"job_id":plan["id"]})).unwrap();
    assert!(community.join("retained/manifest.json").is_file());
    assert!(mods::remove(&app, &json!({"job_id":plan["id"]})).is_err());
    let external = temp.path().join("original");
    addon(&external);
    let replacement = temp.path().join("replacement");
    addon(&replacement);
    symlink(&external, community.join("link")).unwrap();
    let plan = preview(&app, "link");
    fs::remove_file(community.join("link")).unwrap();
    symlink(&replacement, community.join("link")).unwrap();
    mods::remove(&app, &json!({"job_id":plan["id"]})).unwrap();
    wait(&app, "failed");
    assert!(
        fs::symlink_metadata(community.join("link"))
            .unwrap()
            .is_symlink()
    );
    assert!(replacement.join("manifest.json").is_file());
}

#[test]
fn cancelled_preview_releases_the_runtime_and_cannot_be_resumed() {
    let temp = tempfile::tempdir().unwrap();
    let (community, app) = fixture(temp.path());
    addon(&community.join("retained"));
    for i in 0..400 {
        write(&community.join(format!("retained/files/{i}")), b"synthetic");
    }
    let started = mods::preview_remove(
        &app,
        &json!({"runtime_path":app.root().unwrap(),"addon_id":"retained"}),
    )
    .unwrap();
    mods::discard_remove(&app, &json!({"job_id":started["job_id"]})).unwrap();
    wait(&app, "cancelled");
    assert!(community.join("retained/manifest.json").is_file());
    assert!(mods::remove(&app, &json!({"job_id":started["job_id"]})).is_err());
    let ctx = app.reserve("setup", "check", true).unwrap();
    app.finish(&ctx, Ok(json!({"state":"complete"})), false);
}
