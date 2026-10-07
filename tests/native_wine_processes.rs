// SPDX-License-Identifier: MIT
//! Exercise prefix-scoped cleanup with disposable processes, without launching Wine.
#![allow(clippy::unwrap_used)]
use flightdeck::wine_processes;
use std::{
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture(Child);
impl Fixture {
    fn start(prefix: &Path, name: &str) -> Self {
        fs::create_dir_all(prefix).unwrap();
        let ready = prefix.join(format!("{name}.ready"));
        let child = Command::new("/usr/bin/python3")
            .arg0(name)
            .args([
                "-c",
                "import signal,sys,time; from pathlib import Path; \
                 signal.signal(signal.SIGINT,signal.SIG_IGN); \
                 signal.signal(signal.SIGTERM,signal.SIG_IGN); \
                 Path(sys.argv[1]).write_text('ready'); time.sleep(60)",
            ])
            .arg(&ready)
            .env("WINEPREFIX", prefix)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut result = Self(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(result.0.try_wait().unwrap().is_none());
            assert!(Instant::now() < deadline, "fixture did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }
        result
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn orphaned_wine_infrastructure_is_reaped_without_touching_another_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    let prefix = root.join("local/msfs-prefix");
    let mut orphan = Fixture::start(&prefix, "winedevice.exe");
    let mut unrelated = Fixture::start(&temp.path().join("other-prefix"), "winedevice.exe");
    assert!(wine_processes::idle(&prefix).is_err());
    let started = Instant::now();
    wine_processes::stop(&root, &[]).unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(
        orphan.0.try_wait().unwrap().is_some(),
        "orphan survived cleanup"
    );
    wine_processes::idle(&prefix).unwrap();
    assert!(unrelated.0.try_wait().unwrap().is_none());
}

#[test]
fn infrastructure_is_preserved_while_an_application_uses_the_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let prefix = temp.path().join("local/msfs-prefix");
    let mut infrastructure = Fixture::start(&prefix, "winedevice.exe");
    let mut application = Fixture::start(&prefix, "installer.exe");
    wine_processes::stop(temp.path(), &[]).unwrap();
    assert!(infrastructure.0.try_wait().unwrap().is_none());
    assert!(application.0.try_wait().unwrap().is_none());
}
