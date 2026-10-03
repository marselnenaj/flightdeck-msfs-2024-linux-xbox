// SPDX-License-Identifier: MIT
//! Destructive operations run exclusively against disposable fixture trees.
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, files, maintenance, owned_tree};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
fn write(path: &Path, data: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, data).unwrap();
}
fn executable(path: &Path, text: &str) {
    write(path, text.as_bytes());
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn game(root: &Path) {
    write(&root.join("MicrosoftGame.Config"),br#"<Game><Identity Name="Test.Game" Publisher="CN=Test" Version="1.2.3.4"/><Executable Name="FlightSimulator2024.exe"/><StoreId>9P38D19T7LRV</StoreId></Game>"#);
    write(&root.join("FlightSimulator2024.exe"), b"synthetic game");
    write(&root.join(".xodus-streaming.msixvc"), b"synthetic package");
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    game: PathBuf,
    foreign: PathBuf,
    app: Arc<Launcher>,
}
impl Fixture {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("runtime");
        let prefix = root.join("local/msfs-prefix");
        for dir in [
            "private/local-saves",
            "games",
            "tools",
            "local/msfs-prefix/drive_c/windows/system32",
            "local/store-runtime/x86_64-windows",
            "runner/files/bin",
            "runner/files/lib/wine/x86_64-windows",
        ] {
            files::private_dir(&root.join(dir)).unwrap();
        }
        write(&root.join("tools/play-msfs.sh"), b"#!/bin/sh\nexit 0\n");
        write(
            &root.join("private/runtime.json"),
            br#"{"game_id":"msfs2024","market":"AT"}"#,
        );
        write(&root.join("private/local-saves/save"), b"precious progress");
        write(&prefix.join("system.reg"), b"old settings");
        write(&prefix.join("user.reg"), b"old user settings");
        executable(
            &root.join("runner/files/bin/wine"),
            "#!/usr/bin/python3\nimport os,sys\nfrom pathlib import Path\nbin=Path(sys.argv[0]).parent\nif (bin/'fail').exists():sys.exit(1)\nif sys.argv[1]=='wineboot':\n p=Path(os.environ['WINEPREFIX']);(p/'drive_c/windows/system32').mkdir(parents=True);(p/'drive_c/windows/syswow64').mkdir();(p/'system.reg').write_text('fresh settings');(p/'user.reg').write_text('fresh user settings')\n if (bin/'redirect').exists():(p/'drive_c/users').symlink_to((bin/'redirect').read_text())\n",
        );
        executable(
            &root.join("runner/files/bin/wineserver"),
            "#!/bin/sh\nexit 0\n",
        );
        for arch in ["x86_64-windows", "i386-windows"] {
            for (library, names) in [
                ("dxvk", vec!["dxgi", "d3d11", "d3d10core"]),
                ("vkd3d-proton", vec!["d3d12", "d3d12core"]),
            ] {
                for name in names {
                    write(
                        &root
                            .join("runner/files/lib/wine")
                            .join(library)
                            .join(arch)
                            .join(format!("{name}.dll")),
                        b"renderer",
                    );
                }
            }
        }
        let game_path = t.path().join("game");
        game(&game_path);
        symlink(&game_path, root.join("games/MSFS2024")).unwrap();
        let foreign = t.path().join("foreign");
        write(&foreign.join("keep"), b"untouched");
        symlink(&foreign, prefix.join("external")).unwrap();
        write(
            &prefix.join("drive_c/windows/system32/xgameruntime.dll"),
            b"bridge",
        );
        write(
            &root.join("local/store-runtime/x86_64-windows/xodus_store_test.dll"),
            b"builtin",
        );
        write(
            &root.join("runner/files/lib/wine/x86_64-windows/xgameruntime.dll"),
            b"original",
        );
        files::atomic_json(&root.join("private/import-manifest.json"),&json!({"artifacts":{"files":{"runtime/xgameruntime.dll":files::sha256(b"bridge"),"builtin/x86_64-windows/xodus_store_test.dll":files::sha256(b"builtin")}},"original_runtime_sha256":files::sha256(b"original")})).unwrap();
        let app = Launcher::new(t.path().join("state"), Some(root.to_str().unwrap())).unwrap();
        Self {
            _temp: t,
            root,
            game: game_path,
            foreign,
            app,
        }
    }
    fn done(&self) -> Value {
        let end = Instant::now() + Duration::from_secs(8);
        loop {
            if self.app.lock().active.is_none() {
                return self.app.job("maintenance");
            }
            assert!(Instant::now() < end, "maintenance did not complete");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn preview(&self, operation: &str, keep: bool, delete: bool) -> String {
        maintenance::preview(
            &self.app,
            &json!({"operation":operation,"keep_data":keep,"delete_packages":delete}),
        )
        .unwrap();
        let job = self.done();
        assert_eq!(job["state"], "ready", "{job}");
        job["id"].as_str().unwrap().to_owned()
    }
    fn start(&self, id: &str) -> Value {
        maintenance::start(&self.app, &json!({"job_id":id,"confirmed":true})).unwrap();
        self.done()
    }
}
#[test]
fn reviewed_uninstall_preserves_saves_external_targets_and_other_runtime() {
    let f = Fixture::new();
    let other = f._temp.path().join("other");
    files::private_dir(&other.join("private")).unwrap();
    files::private_dir(&other.join("games")).unwrap();
    write(&other.join("tools/play-msfs.sh"), b"#!/bin/sh\nexit 0\n");
    files::atomic_json(
        &other.join("private/runtime.json"),
        &json!({"game_id":"msfs2020"}),
    )
    .unwrap();
    symlink(&f.foreign, other.join("games/MSFS2020")).unwrap();
    f.app.lock().known.insert("msfs2020".into(), other.clone());
    let id = f.preview("uninstall", true, true);
    assert!(maintenance::start(&f.app, &json!({"job_id":id})).is_err());
    assert!(maintenance::start(&f.app, &json!({"job_id":"stale","confirmed":true})).is_err());
    assert!(f.game.exists());
    let job = f.start(&id);
    assert_eq!(job["state"], "complete", "{job}");
    let archive = Path::new(job["backup_path"].as_str().unwrap());
    assert_eq!(
        fs::read(archive.join("private/local-saves/save")).unwrap(),
        b"precious progress"
    );
    assert_eq!(fs::read(f.foreign.join("keep")).unwrap(), b"untouched");
    assert!(!f.root.exists());
    assert!(!f.game.exists());
    assert_eq!(f.app.root().unwrap(), other);
    assert_eq!(
        Launcher::new(f.app.state_dir.clone(), None)
            .unwrap()
            .root()
            .unwrap(),
        other
    );
}
#[test]
fn permanent_uninstall_never_follows_nested_wine_or_package_links() {
    let f = Fixture::new();
    symlink(&f.foreign, f.game.join("nested")).unwrap();
    let id = f.preview("uninstall", false, true);
    let job = f.start(&id);
    assert_eq!(job["state"], "complete", "{job}");
    assert!(job["backup_path"].is_null());
    assert!(!f.game.exists());
    assert!(!f.root.exists());
    assert_eq!(fs::read(f.foreign.join("keep")).unwrap(), b"untouched");
}
#[test]
fn keeping_packages_detaches_without_deleting_user_data() {
    let f = Fixture::new();
    assert!(
        maintenance::preview(
            &f.app,
            &json!({"operation":"uninstall","keep_data":false,"delete_packages":false})
        )
        .is_err()
    );
    let job = f.start(&f.preview("uninstall", true, false));
    assert_eq!(job["state"], "complete", "{job}");
    assert!(f.game.exists());
    assert!(
        Path::new(job["backup_path"].as_str().unwrap())
            .join("private/local-saves/save")
            .exists()
    );
}
#[test]
fn changed_or_discarded_preview_cannot_remove_files() {
    let f = Fixture::new();
    let id = f.preview("uninstall", true, true);
    write(&f.game.join("new-important-file"), b"keep");
    let job = f.start(&id);
    assert_eq!(job["state"], "failed", "{job}");
    assert!(f.root.exists());
    assert!(f.game.exists());
    let id = f.preview("uninstall", true, true);
    maintenance::discard(&f.app, &json!({"job_id":id})).unwrap();
    assert!(maintenance::start(&f.app, &json!({"job_id":id,"confirmed":true})).is_err());
}
#[test]
fn external_lease_and_newly_shared_package_block_removal() {
    let f = Fixture::new();
    let id = f.preview("uninstall", true, true);
    let lease = files::Lease::acquire(&f.root.join("private/play.lock"), false).unwrap();
    assert!(maintenance::start(&f.app, &json!({"job_id":id,"confirmed":true})).is_err());
    drop(lease);
    let other = f._temp.path().join("other");
    files::private_dir(&other.join("games")).unwrap();
    symlink(&f.game, other.join("games/MSFS2024")).unwrap();
    f.app.lock().known.insert("synthetic_other".into(), other);
    let job = f.start(&id);
    assert_eq!(job["state"], "failed", "{job}");
    assert!(f.game.exists());
    assert!(f.root.exists());
}
#[test]
fn previous_internal_game_package_is_removed_without_following_archive_links() {
    let f = Fixture::new();
    let old = f.root.join("private/game-updates/update-test/game");
    game(&old);
    symlink(
        &old,
        f.root
            .join("games")
            .join(format!(".MSFS2024-before-{}", "a".repeat(32))),
    )
    .unwrap();
    let id = f.preview("uninstall", true, true);
    assert_eq!(
        f.app.job("maintenance")["packages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let job = f.start(&id);
    assert_eq!(job["state"], "complete", "{job}");
    assert_eq!(
        fs::read_dir(
            Path::new(job["backup_path"].as_str().unwrap())
                .join("private/game-updates/update-test")
        )
        .unwrap()
        .count(),
        0
    );
}
#[test]
fn reset_preserves_downloads_cloud_recovery_and_can_be_restored() {
    let f = Fixture::new();
    let packages = f
        .root
        .join("local/msfs-prefix/drive_c/users/steamuser/Packages");
    write(&packages.join("Community/addon"), b"content");
    write(&packages.join("Official/sim"), b"downloaded content");
    files::atomic_json(
        &f.root.join("private/runtime.json"),
        &json!({"game_id":"msfs2024","market":"AT","installed_packages_path":packages}),
    )
    .unwrap();
    write(
        &f.root.join("private/synthetic-cloud-recovery.json"),
        b"pending sync",
    );
    let job = f.start(&f.preview("reset", true, true));
    assert_eq!(job["state"], "complete", "{job}");
    assert_eq!(
        fs::read(f.root.join("local/msfs-prefix/user.reg")).unwrap(),
        b"fresh user settings"
    );
    assert_eq!(
        fs::read(packages.join("Official/sim")).unwrap(),
        b"downloaded content"
    );
    assert!(fs::symlink_metadata(&packages).unwrap().is_symlink());
    assert_eq!(maintenance::snapshot(&f.app)["can_restore"], true);
    let job = f.start(&f.preview("restore", true, true));
    assert_eq!(job["state"], "complete", "{job}");
    assert_eq!(
        fs::read(f.root.join("local/msfs-prefix/user.reg")).unwrap(),
        b"old user settings"
    );
    assert_eq!(
        fs::read(f.root.join("private/local-saves/save")).unwrap(),
        b"precious progress"
    );
    assert_eq!(
        fs::read(f.root.join("private/synthetic-cloud-recovery.json")).unwrap(),
        b"pending sync"
    );
    assert_eq!(maintenance::snapshot(&f.app)["can_restore"], false);
}
#[test]
fn reset_failure_or_new_prefix_link_preserves_active_profile() {
    for redirected in [false, true] {
        let f = Fixture::new();
        if redirected {
            let packages = f
                .root
                .join("local/msfs-prefix/drive_c/users/steamuser/Packages");
            write(&packages.join("Community/addon"), b"content");
            files::atomic_json(
                &f.root.join("private/runtime.json"),
                &json!({"game_id":"msfs2024","market":"AT","installed_packages_path":packages}),
            )
            .unwrap();
            write(
                &f.root.join("runner/files/bin/redirect"),
                f.foreign.as_os_str().as_encoded_bytes(),
            );
        } else {
            write(&f.root.join("runner/files/bin/fail"), b"yes");
        }
        let job = f.start(&f.preview("reset", true, true));
        assert_eq!(job["state"], "failed", "{job}");
        assert_eq!(
            fs::read(f.root.join("local/msfs-prefix/user.reg")).unwrap(),
            b"old user settings"
        );
        assert_eq!(fs::read_dir(&f.foreign).unwrap().count(), 1);
        assert!(f.game.exists());
    }
}
#[test]
fn interrupted_reset_journal_validates_both_directory_identities() {
    let f = Fixture::new();
    let backup = f.root.join(format!(
        "local/environment-backup-{}/prefix",
        "b".repeat(32)
    ));
    files::private_dir(&backup).unwrap();
    let active = f.root.join("local/msfs-prefix");
    let record = json!({"schema":1,"backup":backup.strip_prefix(&f.root).unwrap(),"original_id":owned_tree::identity(&backup).unwrap(),"fresh_id":owned_tree::identity(&active).unwrap()});
    files::atomic_json(
        &f.root.join("private/environment-reset-pending.json"),
        &record,
    )
    .unwrap();
    assert_eq!(maintenance::restore_path(&f.root).unwrap(), backup);
    fs::rename(&backup, backup.with_file_name("old")).unwrap();
    files::private_dir(&backup).unwrap();
    assert!(maintenance::restore_path(&f.root).is_err());
}
#[test]
fn failed_configuration_prevents_any_uninstall_renames() {
    let f = Fixture::new();
    let id = f.preview("uninstall", true, true);
    fs::remove_file(f.app.state_dir.join("config.json")).unwrap();
    fs::create_dir(f.app.state_dir.join("config.json")).unwrap();
    let job = f.start(&id);
    assert_eq!(job["state"], "failed", "{job}");
    assert!(f.root.exists());
    assert!(f.game.exists());
    assert!(!f.root.join("private/uninstalled.json").exists());
}
#[test]
fn owned_tree_rejects_replacement_roots_and_leaves_symlink_targets() {
    let t = tempfile::tempdir().unwrap();
    let tree = t.path().join("tree");
    let foreign = t.path().join("foreign");
    write(&tree.join("data"), b"remove me");
    write(&foreign.join("keep"), b"keep me");
    symlink(&foreign, tree.join("link")).unwrap();
    let before = owned_tree::inventory(&tree, &AtomicBool::new(false)).unwrap();
    fs::rename(&tree, t.path().join("original")).unwrap();
    write(&tree.join("different"), b"keep new");
    assert!(owned_tree::remove(&tree, before.id).is_err());
    owned_tree::remove(&t.path().join("original"), before.id).unwrap();
    assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"keep me");
    assert_eq!(fs::read(tree.join("different")).unwrap(), b"keep new");
}
