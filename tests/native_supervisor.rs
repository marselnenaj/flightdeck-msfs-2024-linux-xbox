// SPDX-License-Identifier: MIT
//! Real process/Unix-socket tests; Python is only a disposable fake Xodus fixture.
#![allow(clippy::unwrap_used)]
use flightdeck::{files, process};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            net::UnixListener,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
const FIXTURE: &str = r#"#!/usr/bin/python3
import fcntl, json, os, signal, socket, struct, sys, time
from pathlib import Path
root=Path(os.environ['MSFS_LINUX_ROOT'])
private=root/'private'
expected=(private/'play.lock').stat()
for fd in list(Path('/proc/self/fd').iterdir()):
    try: info=fd.stat()
    except OSError: continue
    assert (info.st_dev,info.st_ino)!=(expected.st_dev,expected.st_ino), 'lease leaked into child'
with (private/'play.lock').open('rb') as probe:
    try: fcntl.flock(probe,fcntl.LOCK_EX|fcntl.LOCK_NB)
    except BlockingIOError: pass
    else: raise RuntimeError('supervisor released the lease')
if Path(sys.argv[0]).name=='xodus-service':
    stop=False
    def stopped(*_):
        global stop
        stop=True
    signal.signal(signal.SIGINT,stopped)
    signal.signal(signal.SIGTERM,stopped)
    path=Path(os.environ['XDG_RUNTIME_DIR'])/'xodus.sock'
    listener=socket.socket(socket.AF_UNIX)
    listener.bind(str(path));listener.listen();listener.settimeout(.1)
    try:
        while not stop:
            try: connection,_=listener.accept()
            except TimeoutError: continue
            with connection:
                connection.settimeout(1)
                data=connection.recv(1024)
                connection.sendall(data[:4]+struct.pack('<H',2)+data[6:])
    finally:
        listener.close();path.unlink(missing_ok=True)
        (private/'service-stopped').write_text('yes')
else:
    assert sys.argv[1]=='run'
    assert Path.cwd()==root/'games/MSFS2024'
    assert os.environ['XODUS_STORE_MARKET']=='AT'
    assert os.environ['FLIGHTDECK_HELPER_RUNTIME']==str(root)
    assert Path(sys.argv[3]).name=='xodus-wine-launch'
    (private/'game-started').write_text('yes')
    if os.environ.get('FLIGHTDECK_TEST_WAIT')=='1':
        while True: time.sleep(.1)
    sys.exit(7)
"#;
fn fixture(base: &Path) -> PathBuf {
    let root = base.join("runtime with spaces");
    for folder in [
        "private",
        "bin",
        "games/MSFS2024",
        "local/msfs-prefix/drive_c/windows/system32",
    ] {
        files::private_dir(&root.join(folder)).unwrap();
    }
    for name in ["xodus-service", "xodus-cli"] {
        files::atomic(&root.join("bin").join(name), FIXTURE.as_bytes()).unwrap();
        fs::set_permissions(
            root.join("bin").join(name),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    for name in ["FlightSimulator2024.exe", ".xodus-streaming.msixvc"] {
        files::atomic(
            &root.join("games/MSFS2024").join(name),
            b"synthetic encrypted package",
        )
        .unwrap();
    }
    files::atomic_json(
        &root.join("private/runtime.json"),
        &json!({"game_id":"msfs2024","market":"AT"}),
    )
    .unwrap();
    files::private_dir(&base.join("sockets")).unwrap();
    root
}
fn command(root: &Path, base: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"));
    c.args(["run-game", "--runtime"])
        .arg(root)
        .env("XDG_RUNTIME_DIR", base.join("sockets"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}
#[test]
fn native_loader_preserves_image_descriptors_and_applies_msfs_startup_arguments() {
    let result = Command::new("python3")
        .args(["tests/compat/loader-test.py", "-q"])
        .env(
            "FLIGHTDECK_TEST_BINARY",
            env!("CARGO_BIN_EXE_flightdeck-rust"),
        )
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn native_supervisor_owns_service_and_game_but_never_lends_them_the_lease() {
    let temp = tempfile::tempdir().unwrap();
    let root = fixture(temp.path());
    let lease = files::Lease::acquire(&root.join("private/play.lock"), true).unwrap();
    let mut c = command(&root, temp.path());
    c.arg("--lock-fd").arg(lease.0.as_raw_fd().to_string());
    let mut child = process::spawn(&mut c, Some(&lease.0)).unwrap();
    let status =
        process::wait(&mut child, Duration::from_secs(20), &AtomicBool::new(false)).unwrap();
    assert_eq!(status.code(), Some(7));
    assert!(root.join("private/service-stopped").exists());
    assert!(root.join("private/game-started").exists());
    assert!(files::Lease::acquire(&root.join("private/play.lock"), false).is_err());
    drop(lease);
    files::Lease::acquire(&root.join("private/play.lock"), false).unwrap();
}
#[test]
fn an_inherited_descriptor_without_the_exclusive_lease_cannot_launch() {
    let temp = tempfile::tempdir().unwrap();
    let root = fixture(temp.path());
    files::atomic(&root.join("private/play.lock"), b"").unwrap();
    let file =
        files::open_at(rustix::fs::CWD, root.join("private/play.lock"), false, true).unwrap();
    let mut c = command(&root, temp.path());
    c.arg("--lock-fd").arg(file.as_raw_fd().to_string());
    let mut child = process::spawn(&mut c, Some(&file)).unwrap();
    assert!(
        !process::wait(&mut child, Duration::from_secs(10), &AtomicBool::new(false))
            .unwrap()
            .success()
    );
    assert!(!root.join("private/game-started").exists());
}
#[test]
fn stopping_the_owned_supervisor_flushes_service_and_releases_the_profile() {
    let temp = tempfile::tempdir().unwrap();
    let root = fixture(temp.path());
    let mut c = command(&root, temp.path());
    c.env("FLIGHTDECK_TEST_WAIT", "1");
    let mut child = process::spawn(&mut c, None).unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while !root.join("private/game-started").exists() {
        assert!(Instant::now() < end);
        assert!(child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(20));
    }
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let status =
        process::wait(&mut child, Duration::from_secs(20), &AtomicBool::new(false)).unwrap();
    assert!(!status.success());
    assert!(root.join("private/service-stopped").exists());
    files::Lease::acquire(&root.join("private/play.lock"), false).unwrap();
}

fn socket_path(root: &Path, base: &Path) -> PathBuf {
    let digest = files::sha256(root.as_os_str().as_encoded_bytes());
    let directory = base
        .join("sockets")
        .join(format!("flightdeck-{}", &digest[..16]));
    files::private_dir(&directory).unwrap();
    directory.join("xodus.sock")
}

#[test]
fn stopping_during_socket_preparation_never_starts_service_or_game() {
    for signal in [rustix::process::Signal::TERM, rustix::process::Signal::INT] {
        for ready in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let root = fixture(temp.path());
            let path = socket_path(&root, temp.path());
            let listener = UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut child = process::spawn(&mut command(&root, temp.path()), None).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        assert!(child.try_wait().unwrap().is_none());
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accepting supervisor probe: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = vec![0; 8 + b"MSFS launcher probe".len()];
            stream.read_exact(&mut request).unwrap();
            // The supervisor has installed both handlers and is blocked in its
            // synchronous probe. Deliver Stop before allowing that probe to end.
            rustix::process::kill_process(
                rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
                signal,
            )
            .unwrap();
            if ready {
                request[4] = 2;
                stream.write_all(&request).unwrap();
            }
            // Without a response this leaves a disconnected socket, so the
            // supervisor would otherwise reclaim it and spawn a new service.
            drop(listener);
            drop(stream);
            let status =
                process::wait(&mut child, Duration::from_secs(20), &AtomicBool::new(false))
                    .unwrap();
            assert_eq!(
                status.code(),
                Some(if signal == rustix::process::Signal::TERM {
                    143
                } else {
                    130
                }),
                "signal={signal:?}, existing service ready={ready}"
            );
            for entry in fs::read_dir(root.join("private")).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name().as_encoded_bytes().starts_with(b"run-") {
                    // These files precede spawn, so even a child killed before
                    // its first Python instruction cannot conceal a regression.
                    assert!(!entry.path().join("service.log").exists());
                    assert!(!entry.path().join("game.log").exists());
                    assert!(!entry.path().join("xodus-wine-launch").exists());
                }
            }
            assert!(!root.join("private/game-started").exists());
            assert!(!root.join("private/service-stopped").exists());
            files::Lease::acquire(&root.join("private/play.lock"), false).unwrap();
        }
    }
}

#[test]
fn stale_owned_socket_is_replaced_before_launching_the_service() {
    let temp = tempfile::tempdir().unwrap();
    let root = fixture(temp.path());
    let path = socket_path(&root, temp.path());
    drop(UnixListener::bind(&path).unwrap());
    let mut child = process::spawn(&mut command(&root, temp.path()), None).unwrap();
    let status =
        process::wait(&mut child, Duration::from_secs(20), &AtomicBool::new(false)).unwrap();
    assert_eq!(status.code(), Some(7));
    assert!(root.join("private/game-started").exists());
    assert!(root.join("private/service-stopped").exists());
    assert!(!path.exists());
    files::Lease::acquire(&root.join("private/play.lock"), false).unwrap();
}

#[test]
fn live_unresponsive_socket_is_preserved_and_blocks_launch() {
    let temp = tempfile::tempdir().unwrap();
    let root = fixture(temp.path());
    let path = socket_path(&root, temp.path());
    let listener = UnixListener::bind(&path).unwrap();
    let before = path.symlink_metadata().unwrap();
    let mut child = process::spawn(&mut command(&root, temp.path()), None).unwrap();
    let status =
        process::wait(&mut child, Duration::from_secs(5), &AtomicBool::new(false)).unwrap();
    assert_eq!(status.code(), Some(1));
    let after = path.symlink_metadata().unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert!(!root.join("private/game-started").exists());
    assert!(!root.join("private/service-stopped").exists());
    files::Lease::acquire(&root.join("private/play.lock"), false).unwrap();
    drop(listener);
}

#[test]
fn bound_socket_before_listen_is_preserved_and_blocks_launch() {
    let temp = tempfile::tempdir().unwrap();
    let root = fixture(temp.path());
    let path = socket_path(&root, temp.path());
    let mut owner = Command::new("python3").args(["-c", "import socket,sys,time; s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); s.bind(sys.argv[1]); time.sleep(10)"]).arg(&path).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let before = path.symlink_metadata().unwrap();
    let mut child = process::spawn(&mut command(&root, temp.path()), None).unwrap();
    let status =
        process::wait(&mut child, Duration::from_secs(5), &AtomicBool::new(false)).unwrap();
    let after = path.symlink_metadata();
    let _ = owner.kill();
    let _ = owner.wait();
    assert_eq!(status.code(), Some(1));
    let after = after.unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert!(!root.join("private/game-started").exists());
    assert!(!root.join("private/service-stopped").exists());
}
