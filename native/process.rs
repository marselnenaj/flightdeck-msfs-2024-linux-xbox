// SPDX-License-Identifier: MIT
//! All subprocess creation passes through one lock, including descriptor lending.
use crate::{Error, Result, error::require};
use rustix::{
    io::{FdFlags, fcntl_getfd, fcntl_setfd},
    process::{Pid, Signal, kill_process},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
static SPAWN: Mutex<()> = Mutex::new(());
pub fn spawn(command: &mut Command, lease: Option<&File>) -> Result<Child> {
    spawn_files(command, &lease.into_iter().collect::<Vec<_>>())
}
pub fn spawn_files(command: &mut Command, files: &[&File]) -> Result<Child> {
    let _guard = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
    let mut changed = Vec::new();
    for file in files {
        let result = (|| {
            let flags = fcntl_getfd(*file)?;
            fcntl_setfd(*file, flags & !FdFlags::CLOEXEC)?;
            Ok::<_, rustix::io::Errno>(flags)
        })();
        match result {
            Ok(flags) => changed.push((*file, flags)),
            Err(error) => {
                for (file, flags) in changed {
                    let _ = fcntl_setfd(file, flags);
                }
                return Err(error.into());
            }
        }
    }
    let mut result = command.spawn();
    let mut restore_error = None;
    for (file, flags) in changed {
        if let Err(error) = fcntl_setfd(file, flags) {
            restore_error = Some(error);
        }
    }
    if let Some(error) = restore_error {
        if let Ok(child) = result.as_mut() {
            let _ = terminate(child);
        }
        return Err(error.into());
    }
    Ok(result?)
}
pub fn terminate(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process(pid, Signal::TERM);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    child.kill()?;
    child.wait()?;
    Ok(())
}
/// Terminate only a child group created with `process_group(0)` by this job.
pub fn terminate_group(child: &mut Child) -> Result<()> {
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = rustix::process::kill_process_group(pid, Signal::TERM);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let exited = child.try_wait()?.is_some();
            if exited
                && matches!(
                    rustix::process::test_kill_process_group(pid),
                    Err(rustix::io::Errno::SRCH)
                )
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }
        let _ = rustix::process::kill_process_group(pid, Signal::KILL);
    }
    child.wait()?;
    Ok(())
}
pub fn wait(child: &mut Child, timeout: Duration, cancel: &AtomicBool) -> Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(code) = child.try_wait()? {
            return Ok(code);
        }
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            terminate(child)?;
            return Err(if cancel.load(Ordering::Relaxed) {
                Error::Cancelled
            } else {
                Error::Invalid("Der Vorgang hat nicht rechtzeitig geantwortet.")
            });
        }
        thread::sleep(Duration::from_millis(40));
    }
}
pub fn run(command: &mut Command, timeout: Duration, cancel: &AtomicBool) -> Result<ExitStatus> {
    let mut child = spawn(command, None)?;
    wait(&mut child, timeout, cancel)
}
pub fn output(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let (status, bytes) = output_status(command, timeout, limit, cancel)?;
    require(
        status.success(),
        "Der Hilfsprozess konnte den Vorgang nicht abschließen.",
    )?;
    Ok(bytes)
}
pub fn output_status(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
    cancel: &AtomicBool,
) -> Result<(ExitStatus, Vec<u8>)> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = spawn(command, None)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(Error::Invalid("Der Prozess hat keinen Ausgabekanal."))?;
    rustix::fs::fcntl_setfl(
        &stdout,
        rustix::fs::fcntl_getfl(&stdout)? | rustix::fs::OFlags::NONBLOCK,
    )?;
    let deadline = Instant::now() + timeout;
    let mut data = Vec::new();
    let result = (|| {
        let mut buffer = [0_u8; 16384];
        let mut exited: Option<ExitStatus> = None;
        loop {
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        require(data.len() + n <= limit, "Die Prozessausgabe ist zu groß.")?;
                        data.extend_from_slice(&buffer[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            if let Some(code) = exited {
                return Ok((code, data));
            }
            exited = child.try_wait()?;
            if exited.is_some() {
                continue;
            } // Drain bytes written just before exit.
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            require(
                Instant::now() < deadline,
                "Der Vorgang hat nicht rechtzeitig geantwortet.",
            )?;
            thread::sleep(Duration::from_millis(15));
        }
    })();
    if result.is_err() {
        let _ = terminate_group(&mut child);
    }
    result
}
pub fn log(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    let info = file.metadata()?;
    require(
        info.is_file() && info.uid() == crate::files::uid() && info.nlink() == 1,
        "Die Protokolldatei ist ungültig.",
    )?;
    Ok(file)
}
pub fn which(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|p| p.join(name))
        .find(|p| p.is_file() && p.metadata().is_ok_and(|m| m.mode() & 0o111 != 0))
}
pub fn open_uri(uri: &str) -> Result<()> {
    let program = which("xdg-open").ok_or(Error::Invalid("Kein Linux-Ordneröffner verfügbar. Bitte xdg-utils installieren und Flightdeck in der grafischen Sitzung öffnen."))?;
    let mut child = spawn(
        Command::new(program)
            .arg(uri)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0),
        None,
    )?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub fn copy_tree(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    require(
        !crate::files::exists(destination),
        "Der Zielordner existiert bereits. Bitte einen neuen Ordner wählen.",
    )?;
    require(
        run(
            Command::new("cp")
                .args(["-a", "--reflink=auto", "--"])
                .arg(source)
                .arg(destination)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            Duration::from_secs(86400),
            cancel,
        )?
        .success(),
        "Die Profilkopie konnte nicht vollständig erstellt werden.",
    )
}
pub fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn raw_fd(file: &File) -> String {
    file.as_raw_fd().to_string()
}
