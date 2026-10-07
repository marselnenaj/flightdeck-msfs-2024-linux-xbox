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
    run().map_err(|_| Failure::new("unsafe_session"))
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

// These protected native services routinely hide their environment. They are
// not Wine or Flightdeck save writers. Any unknown inaccessible process, or a
// command explicitly referring to this runtime, remains an unresolved writer.
fn unrelated_protected_service(command: &[u8], runtime: &Path) -> bool {
    let root = runtime.as_os_str().as_bytes();
    if command.windows(root.len()).any(|part| part == root) {
        return false;
    }
    let name = command.split(|byte| *byte == 0).next().unwrap_or_default();
    let name = name.rsplit(|byte| *byte == b'/').next().unwrap_or_default();
    matches!(
        name,
        b"systemd" | b"(sd-pam)" | b"fusermount3" | b"ssh-agent" | b"gpg-agent" | b"scdaemon"
    )
}

fn process_disappeared(error: &crate::Error) -> bool {
    matches!(error, crate::Error::Io(error)
        if error.kind() == std::io::ErrorKind::NotFound
            || error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()))
}

/// Check the entire owned session, including native Xodus helpers which can
/// outlive the Wine process tree. Retain a pidfd while inspecting each process;
/// an exited process or reused PID must never establish a live identity.
fn quiescent(runtime: &Path) -> Result<()> {
    use rustix::process::{Pid, PidfdFlags, pidfd_open};
    let prefix = runtime.join("local/msfs-prefix").canonicalize()?;
    let root = runtime.canonicalize()?;
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
        // /proc directory ownership changes for non-dumpable processes; read
        // real/effective UIDs rather than mistaking a protected user process for root.
        let ids = std::str::from_utf8(&status)
            .ok()
            .and_then(|text| text.lines().find_map(|line| line.strip_prefix("Uid:")))
            .map(|line| {
                line.split_whitespace()
                    .take(2)
                    .map(str::parse::<u32>)
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .transpose()
            .map_err(|_| Failure::new("unsafe_session"))?
            .ok_or_else(|| Failure::new("unsafe_session"))?;
        require(ids.len() == 2, "unsafe_session")?;
        if !ids.contains(&files::uid()) {
            continue;
        }
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
                require(
                    command
                        .as_ref()
                        .is_ok_and(|value| unrelated_protected_service(value, &root)),
                    "unsafe_session",
                )?;
                continue;
            }
        };
        for variable in environment.split(|byte| *byte == 0) {
            let matching = [
                (b"WINEPREFIX=".as_slice(), &prefix),
                (b"MSFS_LINUX_ROOT=".as_slice(), &root),
                (b"FLIGHTDECK_HELPER_RUNTIME=".as_slice(), &root),
            ];
            for (key, expected) in matching {
                if let Some(value) = variable.strip_prefix(key) {
                    let path = Path::new(std::ffi::OsStr::from_bytes(value));
                    require(
                        !value.is_empty() && path.canonicalize()? != *expected,
                        "unsafe_session",
                    )?;
                }
            }
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
/// Keep the fence until all runtime-bound writers are gone under this lease.
pub fn clear(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let identity = fence_identity(private)?;
        let value = read(private)?;
        let boot = boot_id()?;
        require(
            value.as_ref().is_some_and(|v| v.boot_id == boot),
            "unsafe_session",
        )?;
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
