// SPDX-License-Identifier: MIT
//! Prefix-bound process identities, retained as pidfds through companion cleanup.
use crate::{Result, error::require, files, process, wine};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal},
};
use std::{
    collections::BTreeMap,
    os::{fd::OwnedFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
pub const FENIX: &[&str] = &[
    "fenix.exe",
    "fenixapp.exe",
    "fenixbootstrapper.exe",
    "fenixsystem.exe",
    "fenixdisplay.exe",
    "fenixcdu.exe",
    "fenixwizzard.exe",
    "fenix.gqlgateway.exe",
    "fenixwindowguard.exe",
    "fenix-webview2",
];
pub const GAMES: &[&str] = &["flightsimulator.exe", "flightsimulator2024.exe"];
const INFRASTRUCTURE: &[&str] = &[
    "wineserver",
    "services.exe",
    "winedevice.exe",
    "svchost.exe",
    "plugplay.exe",
    "rpcss.exe",
    "explorer.exe",
    "tabtip.exe",
    "conhost.exe",
];
/// The game, its Fenix, GSX and Simlink companions and Wine's own services, as opposed
/// to another application, such as an installer, running in the same profile.
pub fn session_process(name: &str) -> bool {
    GAMES.contains(&name)
        || FENIX.contains(&name)
        || crate::gsx::COMPANIONS.contains(&name)
        || crate::navigraph::SIMLINK.contains(&name)
        || crate::mobiflight::PROCESSES.contains(&name)
        || INFRASTRUCTURE.contains(&name)
}
pub fn name(arguments: &[String]) -> String {
    let base = arguments
        .first()
        .map(|v| {
            v.trim()
                .trim_matches('"')
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_lowercase()
        })
        .unwrap_or_default();
    if base != "msedgewebview2.exe" {
        return base;
    }
    let mut flags = BTreeMap::new();
    for (i, arg) in arguments.iter().enumerate().skip(1) {
        let (key, value) = arg
            .split_once('=')
            .unwrap_or((arg, arguments.get(i + 1).map(String::as_str).unwrap_or("")));
        flags.insert(
            key.to_lowercase(),
            value.trim_matches('"').replace('/', "\\").to_lowercase(),
        );
    }
    let directory = flags.get("--user-data-dir").map(|v| {
        let mut parts = Vec::new();
        for part in v.split('\\') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        parts.join("\\")
    });
    if flags
        .get("--webview-exe-name")
        .is_some_and(|v| v == "fenixapp.exe")
        || directory.as_deref() == Some(r"c:\programdata\fenix\app\webview2\ebwebview")
    {
        "fenix-webview2".into()
    } else {
        base
    }
}
struct Handle {
    fd: OwnedFd,
    name: String,
}
fn alive(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    poll(
        &mut fds,
        Some(&Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )
    .is_ok_and(|n| n == 0)
}
pub struct Processes {
    prefix: PathBuf,
    handles: BTreeMap<i32, Handle>,
}
impl Processes {
    pub fn new(prefix: &Path) -> Result<Self> {
        let mut result = Self {
            prefix: prefix.canonicalize()?,
            handles: BTreeMap::new(),
        };
        result.collect()?;
        Ok(result)
    }
    pub fn collect(&mut self) -> Result<()> {
        self.handles.retain(|_, v| alive(&v.fd));
        for (count, entry) in std::fs::read_dir("/proc")?.enumerate() {
            require(
                count < 131072,
                "Too many processes to inspect this Windows profile.",
            )?;
            let entry = entry?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|v| v.parse::<i32>().ok())
                .filter(|v| *v > 0)
            else {
                continue;
            };
            if pid == std::process::id() as i32 || self.handles.contains_key(&pid) {
                continue;
            }
            let candidate = (|| -> Result<Option<Handle>> {
                if entry.metadata()?.uid() != files::uid() {
                    return Ok(None);
                }
                let Some(id) = Pid::from_raw(pid) else {
                    return Ok(None);
                };
                let fd = pidfd_open(id, PidfdFlags::empty())?;
                let environment =
                    files::read_public(&entry.path().join("environ"), 2 * 1024 * 1024)?;
                let Some(prefix) = environment
                    .split(|b| *b == 0)
                    .find_map(|v| v.strip_prefix(b"WINEPREFIX="))
                else {
                    return Ok(None);
                };
                use std::os::unix::ffi::OsStrExt;
                let prefix = Path::new(std::ffi::OsStr::from_bytes(prefix));
                // Flatpak and pressure-vessel can bind a different directory at
                // the same pathname. Compare the directory seen by the process,
                // before considering the host's interpretation of that string.
                let wanted = self.prefix.metadata()?;
                let observed = prefix
                    .strip_prefix("/")
                    .ok()
                    .and_then(|relative| entry.path().join("root").join(relative).metadata().ok());
                let matches = match observed {
                    Some(actual) => {
                        actual.is_dir()
                            && [actual.dev(), actual.ino()] == [wanted.dev(), wanted.ino()]
                    }
                    None => {
                        let same_namespace = entry
                            .path()
                            .join("ns/mnt")
                            .metadata()
                            .ok()
                            .zip(std::fs::metadata("/proc/self/ns/mnt").ok())
                            .is_some_and(|(a, b)| [a.dev(), a.ino()] == [b.dev(), b.ino()]);
                        same_namespace && prefix.canonicalize().is_ok_and(|p| p == self.prefix)
                    }
                };
                if !matches {
                    return Ok(None);
                }
                let raw = files::read_public(&entry.path().join("cmdline"), 1024 * 1024)?;
                let args: Vec<_> = raw
                    .split(|b| *b == 0)
                    .map(|v| String::from_utf8_lossy(v).into_owned())
                    .collect();
                if !alive(&fd) {
                    return Ok(None);
                }
                Ok(Some(Handle {
                    fd,
                    name: name(&args),
                }))
            })();
            if let Ok(Some(handle)) = candidate {
                self.handles.insert(pid, handle);
            }
        }
        Ok(())
    }
    pub fn any(&self, names: Option<&[&str]>) -> bool {
        self.handles
            .values()
            .any(|v| alive(&v.fd) && names.is_none_or(|names| names.contains(&v.name.as_str())))
    }
    pub fn applications_running(&self) -> bool {
        self.handles
            .values()
            .any(|v| alive(&v.fd) && !INFRASTRUCTURE.contains(&v.name.as_str()))
    }
    pub fn check_game(&self) -> Result<()> {
        require(
            !self.any(Some(GAMES)),
            "Beende MSFS, bevor du Fenix schließt.",
        )
    }
    pub fn identities(&self, names: &[&str]) -> Result<BTreeMap<String, (i32, u64)>> {
        let mut found = BTreeMap::new();
        for (pid, handle) in &self.handles {
            if !names.contains(&handle.name.as_str()) || !alive(&handle.fd) {
                continue;
            }
            let born = birth(*pid)?;
            if !alive(&handle.fd) {
                continue;
            }
            require(
                found.insert(handle.name.clone(), (*pid, born)).is_none(),
                "Ambiguous Fenix display services.",
            )?;
        }
        Ok(found)
    }
    fn wait(&mut self, names: &[&str], seconds: u64) -> Result<()> {
        let end = Instant::now() + Duration::from_secs(seconds);
        while self.any(Some(names)) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
            self.collect()?;
            self.check_game()?;
        }
        Ok(())
    }
    fn send(&mut self, names: &[&str], signal: Signal) -> Result<()> {
        self.collect()?;
        self.check_game()?;
        for handle in self.handles.values() {
            if names.contains(&handle.name.as_str()) && alive(&handle.fd) {
                let _ = pidfd_send_signal(&handle.fd, signal);
            }
        }
        Ok(())
    }
    fn finish_server(&mut self) -> Result<()> {
        for (names, signal, seconds) in [
            (&["wineserver"][..], Signal::INT, 3),
            (INFRASTRUCTURE, Signal::TERM, 1),
            (INFRASTRUCTURE, Signal::KILL, 1),
        ] {
            self.collect()?;
            // Wine services can outlive their server and escape the game group.
            // Only finish an otherwise idle prefix; another application may have
            // started since the previous stage. Retained pidfds prevent PID reuse.
            if self.applications_running() || !self.any(Some(INFRASTRUCTURE)) {
                return Ok(());
            }
            for handle in self.handles.values() {
                if names.contains(&handle.name.as_str()) && alive(&handle.fd) {
                    let _ = pidfd_send_signal(&handle.fd, signal);
                }
            }
            self.wait(INFRASTRUCTURE, seconds)?;
        }
        require(
            !self.any(Some(INFRASTRUCTURE)),
            "Windows background processes for this profile could not be fully stopped.",
        )
    }
}
pub fn birth(pid: i32) -> Result<u64> {
    let raw = files::read_public(&PathBuf::from(format!("/proc/{pid}/stat")), 65536)?;
    let text = String::from_utf8_lossy(&raw);
    text.rsplit_once(')')
        .and_then(|(_, s)| s.split_whitespace().nth(19))
        .and_then(|v| v.parse().ok())
        .ok_or(crate::Error::Invalid(
            "The process identity is unavailable.",
        ))
}
pub fn idle(prefix: &Path) -> Result<()> {
    if !prefix.exists() {
        return Ok(());
    }
    require(
        !Processes::new(prefix)?.any(None),
        "Close MSFS, Fenix and all installers for this profile first.",
    )
}
/// Signal retained identities in one profile; never Steam or other games.
pub fn terminate_named(prefix: &Path, names: &[&str], timeout: Duration) -> Result<()> {
    let mut processes = Processes::new(prefix)?;
    for handle in processes.handles.values() {
        if names.contains(&handle.name.as_str()) && alive(&handle.fd) {
            pidfd_send_signal(&handle.fd, Signal::TERM)?;
        }
    }
    let deadline = Instant::now() + timeout;
    while processes.any(Some(names)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        processes.collect()?;
    }
    require(
        !processes.any(Some(names)),
        "Der Prozess läuft noch. Bitte den Simulator oder Fenix direkt beenden.",
    )
}

/// A cached runner may be used with a prefix outside this runtime. Do not
/// reclaim it merely because the runtime's current prefix is idle.
pub(crate) fn path_in_use(path: &Path) -> Result<bool> {
    let path = path.canonicalize()?;
    for (count, entry) in std::fs::read_dir("/proc")?.enumerate() {
        require(
            count < 131072,
            "Too many processes to inspect this Windows profile.",
        )?;
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let proc = entry.path();
        if !proc.metadata().is_ok_and(|m| m.uid() == files::uid()) {
            continue;
        }
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
            .and_then(Pid::from_raw)
        else {
            continue;
        };
        let handle = match pidfd_open(pid, PidfdFlags::empty()) {
            Ok(handle) => handle,
            Err(rustix::io::Errno::SRCH) => continue,
            Err(error) => return Err(error.into()),
        };
        if !alive(&handle) {
            continue;
        }
        if std::fs::read_link(proc.join("exe")).is_ok_and(|p| p.starts_with(&path))
            && alive(&handle)
        {
            return Ok(true);
        }
        let bytes = match std::fs::read(proc.join("environ")) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || error.raw_os_error() == Some(3)
                    || !alive(&handle) =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        for item in bytes.split(|b| *b == 0) {
            let Some((key, value)) = item
                .iter()
                .position(|b| *b == b'=')
                .map(|i| (&item[..i], &item[i + 1..]))
            else {
                continue;
            };
            if [
                b"WINEPREFIX".as_slice(),
                b"WINE",
                b"WINELOADER",
                b"WINESERVER",
            ]
            .contains(&key)
                && Path::new(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(value))
                    .starts_with(&path)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
pub fn status(prefix: &Path) -> (bool, bool) {
    Processes::new(prefix)
        .map(|v| (v.any(Some(FENIX)), v.any(Some(GAMES))))
        .unwrap_or((false, false))
}
pub fn stop(root: &Path, names: &[&str]) -> Result<()> {
    stop_prefix(&root.join("local/msfs-prefix"), &root.join("runner"), names)
}
pub fn stop_prefix(prefix: &Path, runner: &Path, names: &[&str]) -> Result<()> {
    let mut processes = Processes::new(prefix)?;
    processes.check_game()?;
    if processes.any(Some(names)) {
        let named: Vec<_> = processes
            .handles
            .values()
            .filter(|v| {
                alive(&v.fd) && names.contains(&v.name.as_str()) && v.name.ends_with(".exe")
            })
            .map(|v| v.name.clone())
            .collect();
        if !named.is_empty() {
            let mut c = Command::new(crate::runtime::wine(runner));
            c.arg("taskkill.exe");
            for name in named {
                c.args(["/IM", &name]);
            }
            c.env_clear()
                .envs(wine::environment(&processes.prefix, runner))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _ = process::run(&mut c, Duration::from_secs(3), &AtomicBool::new(false));
        }
        processes.wait(names, 2)?;
        processes.send(names, Signal::TERM)?;
        processes.wait(names, 1)?;
        processes.send(names, Signal::KILL)?;
        processes.wait(names, 1)?;
        require(
            !processes.any(Some(names)),
            "Fenix konnte nicht vollständig beendet werden.",
        )?;
    }
    processes.finish_server()
}
