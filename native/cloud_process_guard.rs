// SPDX-License-Identifier: MIT
//! A durable fence is cleared only after the exact supervisor completed cleanup.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_fs as fs, files,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs::File, os::unix::ffi::OsStrExt, path::Path, sync::Mutex};
const NAME: &str = "cloud-interrupted-process.json";
static LOCK: Mutex<()> = Mutex::new(());
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    boot_id: String,
}
pub fn boot_id() -> Result<String> {
    let raw = files::read_public(Path::new("/proc/sys/kernel/random/boot_id"), 80)?;
    let value = std::str::from_utf8(&raw)
        .map_err(|_| Failure::new("unsafe_session"))?
        .strip_suffix('\n')
        .unwrap_or("");
    require(
        cloud::guid(value)
            && value == value.to_ascii_lowercase()
            && value != "00000000-0000-0000-0000-000000000000",
        "unsafe_session",
    )?;
    Ok(value.into())
}
fn read(private: &File) -> Result<Option<Record>> {
    let value: Option<Record> = fs::optional(private, NAME, 512)?
        .as_deref()
        .map(cloud::decode)
        .transpose()?;
    if let Some(v) = &value {
        require(
            v.schema == 1
                && cloud::guid(&v.boot_id)
                && v.boot_id == v.boot_id.to_ascii_lowercase()
                && v.boot_id != "00000000-0000-0000-0000-000000000000",
            "unsafe_session",
        )?;
    }
    Ok(value)
}
/// Read-only status discovery after a launcher restart. Do not take LOCK here:
/// status must remain responsive while recovery is backing up large saves.
pub fn recovery_id(runtime: &Path) -> Result<Option<String>> {
    let root = fs::open(runtime, false)?;
    let private = fs::child(&root, "private", false)?;
    let value = read(&private)?;
    let identity = fence_identity(&private)?;
    let boot = boot_id()?;
    fs::same(runtime, &root, false)?;
    fs::linked(&root, "private", &private)?;
    Ok(value.filter(|v| v.boot_id == boot).map(|v| {
        format!(
            "interrupted-{}",
            hex::encode(Sha256::digest(
                format!("{}:{identity:?}", v.boot_id).as_bytes()
            ))
        )
    }))
}

fn fence_identity(private: &File) -> Result<Option<(u64, u64, i64, i64)>> {
    use rustix::fs::{AtFlags, statat};
    match statat(private, NAME, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(m) => Ok(Some((
            m.st_dev,
            m.st_ino,
            m.st_ctime,
            m.st_ctime_nsec as i64,
        ))),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn access<T>(
    runtime: &Path,
    lease: &File,
    action: impl FnOnce(&File, &File) -> Result<T>,
) -> Result<T> {
    let run = || -> Result<T> {
        let _guard = LOCK.lock().map_err(|_| Failure::new("unsafe_session"))?;
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        fs::lease(&private, lease)?;
        let result = action(&root, &private)?;
        fs::lease(&private, lease)?;
        fs::same(runtime, &root, false)?;
        fs::linked(&root, "private", &private)?;
        Ok(result)
    };
    run().map_err(|error| Failure {
        detail: error.detail,
        ..Failure::new("unsafe_session")
    })
}

fn process_alive(fd: &std::os::fd::OwnedFd) -> Result<bool> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    Ok(poll(
        &mut fds,
        Some(&Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )? == 0)
}

/// Whether a process looks like a possible writer of this session. Runtime
/// variables may be inherited, so also require a command line referring to this
/// runtime or a Wine, Xodus or Flightdeck program name. The same check applies
/// when the environment is unreadable: desktop authentication agents, key
/// stores and `sudo` must not hold the session merely by hiding their environment.
fn unattributed_writer(command: &[u8], kernel_name: &str, runtime: &Path) -> bool {
    let root = runtime.as_os_str().as_bytes();
    if command.windows(root.len()).any(|part| part == root) {
        return true;
    }
    let arguments: Vec<String> = command
        .split(|byte| *byte == 0)
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .collect();
    // The kernel's name is cut at 15 bytes; the command line names a Windows
    // program in full, with its Windows path.
    let program = crate::wine_processes::name(&arguments);
    [program, kernel_name.to_lowercase()].iter().any(|name| {
        name.ends_with(".exe")
            || ["wine", "xodus", "flightdeck"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            || crate::wine_processes::session_process(name)
    })
}

fn process_disappeared(error: &crate::Error) -> bool {
    matches!(error, crate::Error::Io(error)
        if error.kind() == std::io::ErrorKind::NotFound
            || error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()))
}

/// A live process still bound to this runtime. `confirmed` means its environment
/// names this runtime; otherwise its binding is empty or unreadable, or its
/// environment is inaccessible and it may be a Wine, Xodus or Flightdeck
/// process: its identity is unresolved, so it is reported but never signalled.
struct Writer {
    pid: u32,
    fd: std::os::fd::OwnedFd,
    name: String,
    state: String,
    confirmed: bool,
    /// Another Windows application in this profile, such as an installer.
    /// It is not part of the ended game session and may be mid-installation.
    application: bool,
}
impl Writer {
    fn detail(&self, spared: bool) -> Value {
        json!({
            "pid": self.pid,
            "name": self.name,
            "state": self.state,
            "identified": self.confirmed,
            "application": self.application,
            "spared": spared,
            "unkillable": self.state.starts_with('D'),
        })
    }
}
fn status_field<'a>(status: &'a str, key: &str) -> &'a str {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .map(str::trim)
        .unwrap_or("")
}
fn blocked(writer: &Writer, spared: bool) -> Failure {
    Failure {
        detail: Some(json!({"blocking_process": writer.detail(spared)})),
        ..Failure::new("unsafe_session")
    }
}

/// Check the entire owned session, including native Xodus helpers which can
/// outlive the Wine process tree. Retain a pidfd while inspecting each process;
/// an exited process or reused PID must never establish a live identity.
fn scan(runtime: &Path) -> Result<Vec<Writer>> {
    use rustix::process::{Pid, PidfdFlags, pidfd_open};
    let prefix = runtime.join("local/msfs-prefix").canonicalize()?;
    let root = runtime.canonicalize()?;
    let mut writers = Vec::new();
    for (count, entry) in std::fs::read_dir("/proc")?.enumerate() {
        require(count < 131072, "unsafe_session")?;
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let status = match files::read_public(&entry.path().join("status"), 65536) {
            Ok(value) => value,
            // An exit after open but before read returns ESRCH, not ENOENT.
            Err(error) if process_disappeared(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        let status = std::str::from_utf8(&status).map_err(|_| Failure::new("unsafe_session"))?;
        // /proc directory ownership changes for non-dumpable processes; read
        // real/effective UIDs rather than mistaking a protected user process for root.
        let ids = status_field(status, "Uid:")
            .split_whitespace()
            .take(2)
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Failure::new("unsafe_session"))?;
        require(ids.len() == 2, "unsafe_session")?;
        if !ids.contains(&files::uid()) {
            continue;
        }
        let name = status_field(status, "Name:").to_owned();
        let state = status_field(status, "State:").to_owned();
        let pid_raw = pid;
        let pid = i32::try_from(pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| Failure::new("unsafe_session"))?;
        let fd = match pidfd_open(pid, PidfdFlags::empty()) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::SRCH) => continue,
            Err(error) => return Err(error.into()),
        };
        if !process_alive(&fd)? {
            continue;
        }
        let environment = files::read_public(&entry.path().join("environ"), 2 * 1024 * 1024);
        if !process_alive(&fd)? {
            continue;
        }
        let environment = match environment {
            Ok(value) => value,
            Err(_) => {
                let command = files::read_public(&entry.path().join("cmdline"), 1024 * 1024);
                if !process_alive(&fd)? {
                    continue;
                }
                // A command line that cannot be read rules nothing out.
                if command
                    .as_ref()
                    .map_or(true, |value| unattributed_writer(value, &name, &root))
                {
                    writers.push(Writer {
                        pid: pid_raw,
                        fd,
                        name,
                        state,
                        confirmed: false,
                        application: false,
                    });
                }
                continue;
            }
        };
        let mut bound = false;
        // An empty or unresolvable binding cannot be attributed to this runtime
        // or ruled out: it blocks as before but is never signalled.
        let mut unresolved = false;
        let mut windows = false;
        for variable in environment.split(|byte| *byte == 0) {
            let matching = [
                (b"WINEPREFIX=".as_slice(), &prefix),
                (b"MSFS_LINUX_ROOT=".as_slice(), &root),
                (b"FLIGHTDECK_HELPER_RUNTIME=".as_slice(), &root),
            ];
            for (key, expected) in matching {
                if let Some(value) = variable.strip_prefix(key) {
                    let path = Path::new(std::ffi::OsStr::from_bytes(value));
                    match path.canonicalize() {
                        _ if value.is_empty() => unresolved = true,
                        Ok(path) if path == *expected => {
                            bound = true;
                            windows |= key == b"WINEPREFIX=";
                        }
                        Ok(_) => {}
                        // A path that does not exist cannot be this runtime.
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => unresolved = true,
                    }
                }
            }
        }
        if !bound && !unresolved {
            continue;
        }
        let command = files::read_public(&entry.path().join("cmdline"), 1024 * 1024);
        if !process_alive(&fd)? {
            continue;
        }
        // A bound or unresolvable environment alone does not prove session
        // membership: the variables are inherited, so any descendant of a
        // context that carried them — a sign-in browser, its desktop
        // integration host, a terminal — would block the fence without ever
        // being part of the session, and would even be signalled. Count only
        // a process that also looks like a session member, exactly as for a
        // process whose environment cannot be read: its command line names
        // this runtime, or it is a Wine, Xodus or Flightdeck program by name.
        // A command line that cannot be read rules nothing out.
        if command
            .as_ref()
            .is_ok_and(|value| !unattributed_writer(value, &name, &root))
        {
            continue;
        }
        // Report Windows processes by executable; the kernel name is truncated.
        let mut name = name;
        let mut application = false;
        if bound && windows {
            let arguments: Vec<_> = command
                .unwrap_or_default()
                .split(|byte| *byte == 0)
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .collect();
            let executable = crate::wine_processes::name(&arguments);
            if executable.ends_with(".exe") || executable == "fenix-webview2" {
                application = !crate::wine_processes::session_process(&executable);
                name = executable.chars().take(64).collect();
            }
        }
        writers.push(Writer {
            pid: pid_raw,
            fd,
            name,
            state,
            confirmed: bound,
            application,
        });
    }
    Ok(writers)
}
fn quiescent(runtime: &Path) -> Result<()> {
    match scan(runtime)?.first() {
        None => Ok(()),
        Some(writer) => Err(blocked(writer, false)),
    }
}
/// End what remains of an ended session: the game, Fenix and GSX companions,
/// Wine infrastructure and native helpers bound to this runtime. Unresolved processes are never
/// signalled. Another Windows application in the profile is ended only on the
/// user's explicit request (`applications`); otherwise it is left running and
/// reported, and nothing is signalled, because ending the Wine server would
/// end it too. A process stuck in the kernel survives SIGKILL; it stays
/// reported as the blocker and only a reboot releases it.
fn terminate(runtime: &Path, applications: bool) -> Result<()> {
    use rustix::process::{Signal, pidfd_send_signal};
    use std::time::{Duration, Instant};
    for (signal, seconds) in [(Signal::TERM, 2), (Signal::KILL, 3)] {
        let writers = scan(runtime)?;
        if !applications && let Some(writer) = writers.iter().find(|w| w.application) {
            return Err(blocked(writer, true));
        }
        let writers: Vec<_> = writers.into_iter().filter(|w| w.confirmed).collect();
        if writers.is_empty() {
            return Ok(());
        }
        for writer in &writers {
            let _ = pidfd_send_signal(&writer.fd, signal);
        }
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end
            && writers
                .iter()
                .any(|w| process_alive(&w.fd).unwrap_or(false))
        {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(())
}

/// Recovery never launches the simulator or contacts the cloud. Retain the
/// offline/session journals; the next launch performs their normal reconciliation.
pub fn recover(runtime: &Path, lease: &File) -> Result<Value> {
    recover_with(runtime, lease, |root| crate::runtime::backup(root, true))
}
fn recover_with(
    runtime: &Path,
    lease: &File,
    backup: impl FnOnce(&Path) -> crate::Result<Value>,
) -> Result<Value> {
    access(runtime, lease, |root, private| {
        let identity = fence_identity(private)?;
        let previous = read(private)?;
        terminate(runtime, true)?;
        quiescent(runtime)?;
        let backup = backup(runtime)?;
        quiescent(runtime)?;
        fs::lease(private, lease)?;
        fs::same(runtime, root, false)?;
        fs::linked(root, "private", private)?;
        require(
            read(private)? == previous && fence_identity(private)? == identity,
            "unsafe_session",
        )?;
        files::atomic_at(
            private,
            "cloud-process-recovery.json",
            &serde_json::to_vec(
                &json!({"schema":1,"checked_at":files::now(),"previous_guard":previous,"backup":backup}),
            )?,
        )?;
        if previous.is_some() {
            fs::unlink(private, NAME, false)?;
            private.sync_all()?;
        }
        Ok(json!({"recovered":true,"backup":backup,"cloud_reconciled":false,"game_started":false}))
    })
}
pub fn check(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let boot = boot_id()?;
        let previous = read(private)?;
        if let Some(v) = &previous {
            require(v.boot_id != boot, "unsafe_session")?;
            fs::same(runtime, root, false)?;
            fs::linked(root, "private", private)?;
            require(read(private)? == previous, "unsafe_session")?;
            fs::unlink(private, NAME, false)?;
            private.sync_all()?;
        }
        Ok(())
    })
}
pub fn mark(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let boot = boot_id()?;
        let previous = read(private)?;
        if previous.as_ref().is_some_and(|v| v.boot_id == boot) {
            private.sync_all()?;
            return Ok(());
        }
        let value = Record {
            schema: 1,
            boot_id: boot,
        };
        let temp = format!(".interrupted-process-{}", uuid::Uuid::new_v4().simple());
        let result = (|| {
            fs::write(private, &temp, &serde_json::to_vec(&value)?, 0o600)?;
            fs::same(runtime, root, false)?;
            fs::linked(root, "private", private)?;
            require(read(private)? == previous, "unsafe_session")?;
            if previous.is_none() {
                fs::publish(private, &temp, NAME)?;
            } else {
                rustix::fs::renameat(private, &temp, private, NAME)?;
                private.sync_all()?;
            }
            Ok(())
        })();
        let _ = fs::unlink(private, &temp, false);
        result
    })
}
/// A supervisor exit or spawn failure does not prove its descendants ended.
/// End the remainder of the session and keep the fence while any runtime-bound
/// writer, or another Windows application in the profile, is still alive.
pub fn clear(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let identity = fence_identity(private)?;
        let value = read(private)?;
        let boot = boot_id()?;
        require(
            value.as_ref().is_some_and(|v| v.boot_id == boot),
            "unsafe_session",
        )?;
        terminate(runtime, false)?;
        quiescent(runtime)?;
        fs::lease(private, lease)?;
        fs::same(runtime, root, false)?;
        fs::linked(root, "private", private)?;
        require(
            read(private)? == value && fence_identity(private)? == identity,
            "unsafe_session",
        )?;
        fs::unlink(private, NAME, false)?;
        private.sync_all()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_read_after_process_exit_is_not_an_unresolved_writer() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .expect("owned process"),
        );
        let status = File::open(format!("/proc/{}/status", child.0.id())).expect("live status");
        child.0.kill().expect("stop owned process");
        child.0.wait().expect("reap owned process");
        let error = files::read_file(status, 65536).expect_err("task exited after opening status");
        assert!(process_disappeared(&error));
        assert!(!process_disappeared(&crate::Error::Io(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        )));
        assert!(!process_disappeared(&crate::Error::Invalid(
            "unknown status"
        )));
    }

    #[test]
    fn recovery_retains_replaced_fence_even_with_identical_record() {
        let temp = tempfile::tempdir().expect("runtime");
        let root = temp.path();
        files::private_dir(&root.join("private")).expect("private");
        files::private_dir(&root.join("local/msfs-prefix")).expect("prefix");
        let lease = files::Lease::acquire(&root.join("private/play.lock"), true).expect("lease");
        mark(root, &lease.0).expect("fence");
        let path = root.join("private").join(NAME);
        let original = std::fs::read(&path).expect("record");
        let result = recover_with(root, &lease.0, |_| {
            files::atomic(&path, &original)?;
            Ok(Value::Null)
        });
        assert_eq!(result.expect_err("replaced fence").code, "unsafe_session");
        assert_eq!(std::fs::read(&path).expect("retained"), original);
        assert!(!root.join("private/cloud-process-recovery.json").exists());
    }
}

#[cfg(test)]
mod attribution {
    use super::*;

    fn held(command: &[&str], kernel_name: &str) -> bool {
        let command = command.join("\0").into_bytes();
        unattributed_writer(&command, kernel_name, Path::new("/data/runtimes/msfs2024"))
    }

    #[test]
    fn a_protected_desktop_service_is_not_held_against_the_session() {
        for (command, kernel_name) in [
            (
                &["/usr/lib/polkit-kde-authentication-agent-1"][..],
                "polkit-kde-auth",
            ),
            (
                &["/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1"],
                "polkit-gnome-au",
            ),
            (
                &["/usr/bin/gnome-keyring-daemon", "--daemonize", "--login"],
                "gnome-keyring-d",
            ),
            (
                &["/usr/bin/kwalletd6", "--pam-login", "7", "8"],
                "kwalletd6",
            ),
            (
                &[
                    "/usr/bin/ssh-agent",
                    "-D",
                    "-a",
                    "/run/user/1000/ssh-agent.socket",
                ],
                "ssh-agent",
            ),
            (&["gpg-agent", "--supervised"], "gpg-agent"),
            (&["sudo", "pacman", "-Syu"], "sudo"),
            (&["/usr/lib/systemd/systemd", "--user"], "systemd"),
            (&["(sd-pam)"], "(sd-pam)"),
            (&["keepassxc"], "keepassxc"),
            (&[""], ""),
        ] {
            assert!(!held(command, kernel_name), "{command:?}");
        }
    }

    #[test]
    fn what_refers_to_the_runtime_or_looks_like_its_processes_stays_unresolved() {
        for (command, kernel_name) in [
            // Anything that names this runtime, whatever it is called.
            (
                &[
                    "/usr/bin/helper",
                    "--state",
                    "/data/runtimes/msfs2024/private",
                ][..],
                "helper",
            ),
            (
                &["/data/runtimes/msfs2024/bin/xodus-service"],
                "xodus-service",
            ),
            // Wine and its Windows processes, of this or of another runtime.
            (&["/opt/runner/files/bin/wineserver"], "wineserver"),
            (
                &["/opt/runner/files/bin/wine64-preloader", "wineboot"],
                "wine64-preloade",
            ),
            (&[r"C:\windows\system32\services.exe"], "services.exe"),
            (
                &[r"Z:\games\MSFS2020\FlightSimulator.exe", "-FastLaunch"],
                "FlightSimulator",
            ),
            (
                &[r"C:\Program Files\FenixSim A320\FenixSystem.exe"],
                "FenixSystem.exe",
            ),
            // The kernel's name alone, when the command line says nothing.
            (&[""], "winedevice.exe"),
            (&["fenix-webview2"], "fenix-webview2"),
            // Xodus and Flightdeck programs started from elsewhere.
            (&["/tmp/copy/xodus-cli", "run"], "xodus-cli"),
            (
                &["/opt/flightdeck/bin/flightdeck", "run-game"],
                "flightdeck",
            ),
            (&["xodus-wine-launch"], "flightdeck-rust"),
        ] {
            assert!(held(command, kernel_name), "{command:?}");
        }
    }
}
