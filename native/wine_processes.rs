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
                if Path::new(std::ffi::OsStr::from_bytes(prefix)).canonicalize()? != self.prefix {
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
        self.collect()?;
        if self
            .handles
            .values()
            .any(|v| alive(&v.fd) && !INFRASTRUCTURE.contains(&v.name.as_str()))
        {
            return Ok(());
        }
        self.send(&["wineserver"], Signal::INT)?;
        self.wait(INFRASTRUCTURE, 3)
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
pub fn status(prefix: &Path) -> (bool, bool) {
    Processes::new(prefix)
        .map(|v| (v.any(Some(FENIX)), v.any(Some(GAMES))))
        .unwrap_or((false, false))
}
pub fn stop(root: &Path, names: &[&str]) -> Result<()> {
    let mut processes = Processes::new(&root.join("local/msfs-prefix"))?;
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
            let mut c = Command::new(crate::runtime::wine(&root.join("runner")));
            c.arg("taskkill.exe");
            for name in named {
                c.args(["/IM", &name]);
            }
            c.env_clear()
                .envs(wine::environment(&processes.prefix, &root.join("runner")))
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
