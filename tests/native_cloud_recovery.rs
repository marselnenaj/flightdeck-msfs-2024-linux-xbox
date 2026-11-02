// SPDX-License-Identifier: MIT
#[path = "fixtures/programs.rs"]
mod programs;
use flightdeck::{cloud_process_guard as guard, files};
use std::{
    fs,
    os::unix::process::CommandExt,
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
/// A process named like a Windows executable and bound to a Wine prefix.
fn windows(name: &str, prefix: &Path) -> Process {
    use std::os::unix::process::CommandExt;
    let process = Process(
        Command::new("sleep")
            .arg0(name)
            .arg("30")
            .env("WINEPREFIX", prefix)
            .spawn()
            .expect("windows process"),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read(format!("/proc/{}/environ", process.0.id())).is_ok_and(|env| {
        env.split(|b| *b == 0)
            .any(|e| e.starts_with(b"WINEPREFIX="))
    }) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    process
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
    let request = guard::recovery_id(root).expect("status").expect("pending");
    assert_eq!(
        guard::recovery_id(root).expect("status").as_ref(),
        Some(&request)
    );
    // Confirmed runtime-bound stragglers are ended by recovery instead of
    // fencing the launch until a reboot. Recovery proves they are gone first.
    for key in ["WINEPREFIX", "MSFS_LINUX_ROOT", "FLIGHTDECK_HELPER_RUNTIME"] {
        let path = if key == "WINEPREFIX" {
            root.join("local/msfs-prefix")
        } else {
            root.to_path_buf()
        };
        let mut writer = Process(
            Command::new("sleep")
                .arg0("wineserver")
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
        let result = guard::recover(root, &lease.0).expect("recovery ends the straggler");
        assert_eq!(result["game_started"], false);
        assert!(
            writer.0.try_wait().expect("reap").is_some(),
            "{key} writer must have been ended"
        );
        assert!(!root.join("private/cloud-interrupted-process.json").exists());
        drop(writer);
        guard::mark(root, &lease.0).expect("next session fence");
    }
    let straggler = Process(
        Command::new("sleep")
            .arg0("wineserver")
            .arg("30")
            .env("WINEPREFIX", root.join("local/msfs-prefix"))
            .spawn()
            .expect("writer"),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read(format!("/proc/{}/environ", straggler.0.id())).is_ok_and(|env| {
        env.split(|b| *b == 0)
            .any(|e| e.starts_with(b"WINEPREFIX="))
    }) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut straggler = straggler;
    guard::clear(root, &lease.0).expect("a supervisor exit ends its remaining descendants");
    assert!(straggler.0.try_wait().expect("reap").is_some());
    drop(straggler);
    assert!(!root.join("private/cloud-interrupted-process.json").exists());
    guard::mark(root, &lease.0).expect("next session fence");
    let fence = bytes(root, "cloud-interrupted-process.json");

    // Fenix belongs to the simulator session and ends with it.
    let prefix = root.join("local/msfs-prefix");
    let mut fenix = windows("FenixApp.exe", &prefix);
    guard::clear(root, &lease.0).expect("Fenix ends with the game session");
    assert!(fenix.0.try_wait().expect("reap").is_some());
    drop(fenix);
    guard::mark(root, &lease.0).expect("next session fence");

    // Another Windows application in the profile is left running and reported
    // by the automatic release; only the explicit session check ends it.
    let mut installer = windows("Setup.exe", &prefix);
    let error = guard::clear(root, &lease.0).expect_err("an installer is not ended implicitly");
    assert_eq!(error.code, "unsafe_session");
    let blocking = &error.detail.expect("blocker detail")["blocking_process"];
    assert_eq!(blocking["pid"], installer.0.id());
    assert_eq!(blocking["name"], "setup.exe");
    assert_eq!(blocking["application"], true);
    assert_eq!(blocking["spared"], true);
    assert!(installer.0.try_wait().expect("reap").is_none());
    assert_eq!(bytes(root, "cloud-interrupted-process.json"), fence);
    guard::recover(root, &lease.0).expect("the session check ends the installer");
    assert!(installer.0.try_wait().expect("reap").is_some());
    drop(installer);
    guard::mark(root, &lease.0).expect("next session fence");

    // An empty binding cannot be attributed: it blocks but is never signalled.
    let mut unbound = windows("wineserver", Path::new(""));
    let error = guard::recover(root, &lease.0).expect_err("unresolved binding");
    let blocking = &error.detail.expect("blocker detail")["blocking_process"];
    assert_eq!(blocking["pid"], unbound.0.id());
    assert_eq!(blocking["identified"], false);
    assert!(unbound.0.try_wait().expect("reap").is_none());
    drop(unbound);
    // A prefix that does not exist cannot be this runtime and does not block.
    let _absent = windows("sleep", &root.join("local/absent-prefix"));
    guard::clear(root, &lease.0).expect("a missing prefix is not this runtime");
    guard::mark(root, &lease.0).expect("next session fence");

    // A bound process that does not look like a session member is not one:
    // the binding variables are inherited, so a desktop's browser-integration
    // host keeps an exported WINEPREFIX while having nothing to do with the
    // ended session. It must neither block the fence nor be signalled.
    let mut bystander = {
        use std::os::unix::process::CommandExt;
        Process(
            Command::new("sleep")
                .arg0("plasma-browser-integration-host")
                .arg("30")
                .env("WINEPREFIX", root.join("local/msfs-prefix"))
                .spawn()
                .expect("env-inheriting bystander"),
        )
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read(format!("/proc/{}/environ", bystander.0.id())).is_ok_and(|env| {
        env.split(|b| *b == 0)
            .any(|e| e.starts_with(b"WINEPREFIX="))
    }) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    guard::clear(root, &lease.0)
        .expect("a process that only inherited the binding is not a session writer");
    assert!(
        bystander.0.try_wait().expect("reap").is_none(),
        "bystander must never be signalled"
    );
    drop(bystander);
    guard::mark(root, &lease.0).expect("next session fence");

    let ready = root.join("protected.ready");
    let mut writer = Process(
        programs::peer("undumpable")
            .arg(&ready)
            .env("FLIGHTDECK_HELPER_RUNTIME", root)
            .spawn()
            .expect("protected writer"),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    // An unresolved protected process is reported, never signalled.
    let error = guard::recover(root, &lease.0).expect_err("unknown inaccessible writer");
    assert_eq!(error.code, "unsafe_session");
    let blocking = &error.detail.expect("blocker detail")["blocking_process"];
    assert_eq!(blocking["pid"], writer.0.id());
    assert_eq!(blocking["identified"], false);
    assert_eq!(blocking["unkillable"], false);
    assert_eq!(blocking["name"], "test-programs");
    assert!(
        writer.0.try_wait().expect("reap").is_none(),
        "protected process must survive"
    );
    assert_eq!(bytes(root, "cloud-interrupted-process.json"), fence);
    drop(writer);

    // A desktop's own protected services hide their environment as well: an
    // authentication agent, a key store. They are not held against the session,
    // or no profile on such a desktop could be released again before a reboot.
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let protected = |name: &str| {
        use std::os::unix::process::CommandExt;
        let ready = elsewhere
            .path()
            .join(name.rsplit('/').next().expect("name"));
        let process = Process(
            programs::peer("undumpable")
                .arg0(name)
                .arg(&ready)
                .spawn()
                .expect("protected process"),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            fs::read(format!("/proc/{}/environ", process.0.id())).is_err(),
            "the environment of {name} is hidden"
        );
        process
    };
    let mut agent = protected("/usr/lib/polkit-kde-authentication-agent-1");
    guard::clear(root, &lease.0).expect("a desktop agent does not hold the session");
    assert!(agent.0.try_wait().expect("reap").is_none());
    guard::mark(root, &lease.0).expect("next session fence");
    // What may be Wine's own stays unresolved, although nothing ties it here.
    let mut server = protected("wineserver");
    let error = guard::clear(root, &lease.0).expect_err("an unattributed Wine server");
    let blocking = &error.detail.expect("blocker detail")["blocking_process"];
    assert_eq!(blocking["pid"], server.0.id());
    assert_eq!(blocking["identified"], false);
    assert!(
        server.0.try_wait().expect("reap").is_none(),
        "never signalled"
    );
    assert!(agent.0.try_wait().expect("reap").is_none());
    assert_eq!(bytes(root, "cloud-interrupted-process.json"), fence);
    drop(server);
    drop(agent);

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
