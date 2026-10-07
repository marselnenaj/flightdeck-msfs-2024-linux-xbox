// SPDX-License-Identifier: MIT
use flightdeck::{cloud_process_guard as guard, files};
use std::{
    fs,
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn fixture() -> (tempfile::TempDir, files::Lease) {
    let root = tempfile::tempdir().expect("runtime");
    for folder in [
        "private/local-saves",
        "private/cloud-sessions",
        "local/msfs-prefix",
    ] {
        files::private_dir(&root.path().join(folder)).expect("private directory");
    }
    files::atomic(
        &root.path().join("private/local-saves/profile"),
        b"valuable local save",
    )
    .expect("save");
    files::atomic(
        &root.path().join("private/cloud-offline.pending"),
        b"offline journal",
    )
    .expect("offline");
    files::atomic(
        &root.path().join("private/cloud-sessions/pending.json"),
        b"session journal",
    )
    .expect("session");
    let lease = files::Lease::acquire(&root.path().join("private/play.lock"), true).expect("lease");
    guard::mark(root.path(), &lease.0).expect("fence");
    (root, lease)
}
fn bytes(root: &Path, name: &str) -> Vec<u8> {
    fs::read(root.join("private").join(name)).expect("read")
}

// One test intentionally serializes the inaccessible-process case: an unknown
// protected writer must block every runtime until its identity can be resolved.
#[test]
fn interrupted_session_recovers_only_after_writers_exit_and_backup_is_durable() {
    let (root, lease) = fixture();
    let root = root.path();
    let fence = bytes(root, "cloud-interrupted-process.json");
    let request = guard::recovery_id(root).expect("status").expect("pending");
    assert_eq!(
        guard::recovery_id(root).expect("status").as_ref(),
        Some(&request)
    );
    for key in ["WINEPREFIX", "MSFS_LINUX_ROOT", "FLIGHTDECK_HELPER_RUNTIME"] {
        let path = if key == "WINEPREFIX" {
            root.join("local/msfs-prefix")
        } else {
            root.to_path_buf()
        };
        let writer = Process(
            Command::new("sleep")
                .arg("30")
                .env(key, path)
                .spawn()
                .expect("writer"),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let marker = format!("{key}=").into_bytes();
        while !fs::read(format!("/proc/{}/environ", writer.0.id())).is_ok_and(|env| {
            env.split(|b| *b == 0)
                .any(|entry| entry.starts_with(&marker))
        }) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            guard::clear(root, &lease.0)
                .expect_err("a supervisor exit cannot release a live descendant")
                .code,
            "unsafe_session"
        );
        assert_eq!(
            guard::recover(root, &lease.0)
                .expect_err("live writer")
                .code,
            "unsafe_session"
        );
        assert_eq!(bytes(root, "cloud-interrupted-process.json"), fence);
        assert!(!root.join("private/save-backups").exists());
        drop(writer);
    }
    guard::clear(root, &lease.0).expect("all descendants have exited");
    assert!(!root.join("private/cloud-interrupted-process.json").exists());
    guard::mark(root, &lease.0).expect("next session fence");
    let ready = root.join("protected.ready");
    let writer = Process(Command::new("python3").args(["-c", "import ctypes,pathlib,sys,time; assert ctypes.CDLL(None).prctl(4,0)==0; pathlib.Path(sys.argv[1]).touch(); time.sleep(30)"])
        .arg(&ready).env("FLIGHTDECK_HELPER_RUNTIME", root).spawn().expect("protected writer"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        guard::recover(root, &lease.0)
            .expect_err("unknown inaccessible writer")
            .code,
        "unsafe_session"
    );
    assert_eq!(bytes(root, "cloud-interrupted-process.json"), fence);
    drop(writer);

    let unrelated = tempfile::tempdir().expect("other runtime");
    let other = Process(
        Command::new("sleep")
            .arg("30")
            .env("WINEPREFIX", unrelated.path())
            .spawn()
            .expect("other writer"),
    );
    let result = guard::recover(root, &lease.0).expect("idle recovery");
    assert_eq!(result["game_started"], false);
    assert_eq!(result["cloud_reconciled"], false);
    let backup = result["backup"]["backup"]["name"]
        .as_str()
        .expect("durable backup");
    let backup = root.join("private/save-backups").join(backup);
    assert_eq!(
        fs::read(backup.join("data/profile")).expect("backup"),
        b"valuable local save"
    );
    assert!(backup.join("manifest.json").is_file());
    assert_eq!(bytes(root, "local-saves/profile"), b"valuable local save");
    assert_eq!(bytes(root, "cloud-offline.pending"), b"offline journal");
    assert_eq!(
        bytes(root, "cloud-sessions/pending.json"),
        b"session journal"
    );
    assert!(root.join("private/cloud-process-recovery.json").is_file());
    assert!(!root.join("private/cloud-interrupted-process.json").exists());
    assert_eq!(guard::recovery_id(root).expect("status"), None);
    guard::check(root, &lease.0).expect("normal launch fence check");
    drop(other);

    guard::mark(root, &lease.0).expect("new interrupted session");
    assert_ne!(
        guard::recovery_id(root).expect("new id").as_ref(),
        Some(&request)
    );
    fs::rename(
        root.join("private/local-saves"),
        root.join("private/preserved-saves"),
    )
    .expect("move source");
    std::os::unix::fs::symlink("preserved-saves", root.join("private/local-saves"))
        .expect("symlink source");
    assert!(guard::recover(root, &lease.0).is_err());
    assert_eq!(bytes(root, "cloud-interrupted-process.json"), fence);
    assert_eq!(bytes(root, "cloud-offline.pending"), b"offline journal");
}
