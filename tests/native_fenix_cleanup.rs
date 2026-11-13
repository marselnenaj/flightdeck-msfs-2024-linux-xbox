// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
//! What completed Fenix patch updates leave behind is reclaimed; what Restore,
//! the active runner or an interrupted commit still needs is kept.
use flightdeck::{fenix, files};
use serde_json::json;
use std::{fs, os::unix::fs::symlink, path::Path};

fn dir(path: &Path) {
    files::private_dir(path).unwrap();
}

#[test]
fn superseded_runners_staging_and_completed_update_copies_are_removed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let stamp = |n: u8| format!("20261113T1200{n:02}-0000000{n}");
    let work = |n: u8| format!("local/fenix-patch-{}", stamp(n));
    let copy = |n: u8| format!("local/msfs-prefix.before-fenix-update-{}", stamp(n));
    for n in 1..=5 {
        dir(&root.join(work(n)).join("runner/files"));
    }
    dir(&root.join(work(3)).join("prefix/drive_c"));
    for n in 1..=3 {
        dir(&root.join(copy(n)).join("drive_c"));
    }
    dir(&root.join("local/original-runner/files"));
    dir(&root.join("local/msfs-prefix/drive_c"));
    dir(&root.join("local/msfs-prefix.before-fenix-20261001T120000-00000000"));
    dir(&root.join("private"));
    symlink(root.join(work(1)).join("runner"), root.join("runner")).unwrap();
    // The first update's backup names the state it started from, which itself
    // came from an update whose profile copy is copy(2).
    let backup = |n: u8| format!("private/fenix-patch-backup-{}", stamp(n));
    dir(&root.join(backup(1)));
    files::atomic_json(
        &root.join(backup(1)).join("previous-patch.json"),
        &json!({"state": "installed", "work": work(2), "upgrade_backup": backup(2),
            "upgrade_previous_prefix": copy(2)}),
    )
    .unwrap();
    files::atomic_json(
        &root.join(fenix::MARKER),
        &json!({"state": "installed", "work": work(1), "upgrade_backup": backup(1),
            "upgrade_previous_prefix": copy(1),
            "previous_runner": root.join("local/original-runner").to_str().unwrap(),
            "previous_prefix": "local/msfs-prefix.before-fenix-20261001T120000-00000000"}),
    )
    .unwrap();
    files::atomic_json(
        &root.join("private/proton-selection.json"),
        &json!({"schema": 2, "runner": format!("{}/runner", work(5))}),
    )
    .unwrap();
    fenix::prune_superseded(root);
    let exists = |path: String| root.join(path).exists();
    assert!(exists(work(1)), "active runner");
    assert!(!exists(work(2)), "runner of the previous patch");
    assert!(!exists(work(3)), "staging copy of a failed attempt");
    assert!(!exists(work(4)), "orphaned runner");
    assert!(exists(work(5)), "selected runner");
    assert!(
        !exists(copy(1)) && !exists(copy(2)),
        "completed update copies"
    );
    assert!(exists(copy(3)), "copy of an interrupted commit");
    for kept in [
        "local/original-runner",
        "local/msfs-prefix",
        "local/msfs-prefix.before-fenix-20261001T120000-00000000",
    ] {
        assert!(exists(kept.to_string()), "{kept}");
    }
}

#[test]
fn nothing_is_removed_while_an_update_is_pending() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let old = root.join("local/fenix-patch-20261113T120000-00000001");
    dir(&old);
    dir(&root.join("private"));
    files::atomic_json(&root.join(fenix::MARKER), &json!({"state": "preparing"})).unwrap();
    fenix::prune_superseded(root);
    assert!(old.exists());
    fs::remove_file(root.join(fenix::MARKER)).unwrap();
    fenix::prune_superseded(root);
    assert!(old.exists());
}
