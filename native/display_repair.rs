// SPDX-License-Identifier: MIT
//! The only desktop mutation in the display preflight: append one private,
//! uniquely marked Lua override, reload config-only, remove precisely that
//! override, and reload again. Never reset/restart the compositor or kill apps.
use super::{Adapter, Failure, failure};
use crate::{Error, Result, files, graphics, process};
use rustix::fs::{self as rfs, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const VERSION: &str = "0.56.2";
const COMMIT: &str = "efb50993780079460b0cbed1363e2166a2de1d9f";
const MARKER: &str = "-- flightdeck-x11-repair:";
const JOURNAL: &str = "display-preflight-repair.json";
const BACKUP: &str = "display-preflight-backup.lua";
const MAX_CONFIG: usize = 1024 * 1024;

/// Session paths are never returned in public diagnostics.
#[derive(Clone, Debug)]
pub struct Desktop {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub runtime_dir: PathBuf,
    pub signature: String,
}
#[derive(Clone, Debug)]
pub struct Compositor {
    pub uid: u32,
    pub trusted_executable: bool,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    /// Local X11 listener numbers whose socket descriptors this process holds.
    pub x11_displays: Vec<u32>,
}
pub(super) struct NativeAdapter;

fn trusted_executable(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0 && m.mode() & 0o111 != 0
    })
}
fn bounded_public(path: &Path, limit: usize) -> Result<Vec<u8>> {
    files::read_public(path, limit)
}
fn x11_listeners(directory: &Path) -> Result<Vec<u32>> {
    let mut sockets = BTreeSet::new();
    for (count, entry) in fs::read_dir(directory.join("fd"))?.enumerate() {
        check(count < 65536, Failure::Unsupported)?;
        let entry = entry?;
        match fs::read_link(entry.path()) {
            Ok(target) => {
                sockets.insert(target);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let bytes = bounded_public(&directory.join("net/unix"), 8 * 1024 * 1024)?;
    let mut displays = BTreeSet::new();
    for line in String::from_utf8_lossy(&bytes).lines().skip(1) {
        let row: Vec<_> = line.split_whitespace().collect();
        if row.len() != 8
            || row[4] != "0001"
            || u32::from_str_radix(row[3], 16).unwrap_or(0) & 0x10000 == 0
            || !sockets.contains(&PathBuf::from(format!("socket:[{}]", row[6])))
        {
            continue;
        }
        let path = row[7].strip_prefix('@').unwrap_or(row[7]);
        if let Some(number) = path.strip_prefix("/tmp/.X11-unix/X")
            && !number.is_empty()
            && number.bytes().all(|b| b.is_ascii_digit())
            && let Ok(number) = number.parse::<u32>()
        {
            displays.insert(number);
        }
    }
    Ok(displays.into_iter().collect())
}
impl Adapter for NativeAdapter {
    fn probe(&mut self, env: &graphics::Environment, cancel: &AtomicBool) -> Result<Value> {
        graphics::child("display", Some(env), cancel, 3)
    }
    fn desktop(&self) -> Result<Desktop> {
        let home = files::home();
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", files::uid())));
        Ok(Desktop {
            home,
            config_home,
            runtime_dir,
            signature: std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default(),
        })
    }
    fn trusted_hyprctl(&self) -> bool {
        trusted_executable(Path::new("/usr/bin/hyprctl"))
    }
    fn hyprctl(&mut self, args: &[&str], cancel: &AtomicBool) -> Result<Vec<u8>> {
        if !self.trusted_hyprctl() {
            return Err(failure(Failure::Unsupported));
        }
        let desktop = self.desktop()?;
        validate_desktop(&desktop)?;
        let mut command = Command::new("/usr/bin/hyprctl");
        // Do not let Wine or inherited loader variables alter the system tool.
        command
            .args(args)
            .env_clear()
            .env("LC_ALL", "C")
            .env("HOME", &desktop.home)
            .env("XDG_RUNTIME_DIR", &desktop.runtime_dir)
            .env("HYPRLAND_INSTANCE_SIGNATURE", &desktop.signature);
        process::output(&mut command, Duration::from_secs(3), 1024 * 1024, cancel)
    }
    fn compositor(&self, pid: u32) -> Result<Compositor> {
        let directory = PathBuf::from(format!("/proc/{pid}"));
        let metadata = fs::metadata(&directory)?;
        let target = fs::read_link(directory.join("exe"))?;
        let arguments = bounded_public(&directory.join("cmdline"), 64 * 1024)?
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        let environment = bounded_public(&directory.join("environ"), 1024 * 1024)?
            .split(|b| *b == 0)
            .filter_map(|s| {
                let value = std::str::from_utf8(s).ok()?;
                let (key, value) = value.split_once('=')?;
                // These are the only compositor environment values needed to
                // establish which configuration this process actually loads.
                ["HOME", "XDG_CONFIG_HOME", "OMARCHY_PATH"]
                    .contains(&key)
                    .then(|| (key.to_owned(), value.to_owned()))
            })
            .collect();
        Ok(Compositor {
            uid: metadata.uid(),
            trusted_executable: target.file_name().is_some_and(|s| s == "Hyprland")
                && trusted_executable(&target),
            arguments,
            environment,
            x11_displays: x11_listeners(&directory)?,
        })
    }
    fn xwayland_present(&self) -> Result<bool> {
        for (index, entry) in fs::read_dir("/proc")?.enumerate() {
            if index > 262144 {
                return Err(failure(Failure::Unsupported));
            }
            let entry = entry?;
            if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
                continue;
            }
            let path = entry.path();
            let info = match fs::metadata(&path) {
                Ok(info) => info,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if info.uid() != files::uid() {
                continue;
            }
            let comm = match bounded_public(&path.join("comm"), 4096) {
                Ok(value) => value,
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if String::from_utf8_lossy(&comm).trim() == "Xwayland"
                || fs::read_link(path.join("exe"))
                    .ok()
                    .is_some_and(|p| p.file_name().is_some_and(|s| s == "Xwayland"))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn check(value: bool, kind: Failure) -> Result<()> {
    if value { Ok(()) } else { Err(failure(kind)) }
}
fn validate_desktop(desktop: &Desktop) -> Result<()> {
    check(
        desktop.home.is_absolute()
            && desktop.config_home == desktop.home.join(".config")
            && desktop.runtime_dir.is_absolute()
            && !desktop.signature.is_empty()
            && desktop.signature.len() <= 160
            && desktop
                .signature
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)),
        Failure::Unsupported,
    )?;
    let runtime = secure_directory(&desktop.runtime_dir)?;
    check(
        runtime.metadata()?.uid() == files::uid(),
        Failure::Unsupported,
    )
}

/// Open every component without following symlinks, retaining directory FDs.
/// Root-owned sticky ancestors (e.g. /tmp in tests) cannot replace our owned
/// child. The actual home/config directories must be owned by this user.
fn secure_directory(path: &Path) -> Result<File> {
    check(path.is_absolute(), Failure::UnsafeConfig)?;
    let mut directory = File::from(rfs::openat(
        rfs::CWD,
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                directory = File::from(rfs::openat(
                    &directory,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
                let info = directory.metadata()?;
                check(
                    (info.uid() == files::uid() && info.mode() & 0o022 == 0)
                        || (info.uid() == 0
                            && (info.mode() & 0o022 == 0 || info.mode() & 0o1000 != 0)),
                    Failure::UnsafeConfig,
                )?;
            }
            _ => return Err(failure(Failure::UnsafeConfig)),
        }
    }
    Ok(directory)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    device: u64,
    inode: u64,
    mode: u32,
    length: u64,
    modified: i64,
    nanos: i64,
    changed: i64,
    changed_nanos: i64,
}
impl Identity {
    fn of(info: &fs::Metadata) -> Self {
        Self {
            device: info.dev(),
            inode: info.ino(),
            mode: info.mode() & 0o7777,
            length: info.len(),
            modified: info.mtime(),
            nanos: info.mtime_nsec(),
            changed: info.ctime(),
            changed_nanos: info.ctime_nsec(),
        }
    }
}
struct Config {
    path: PathBuf,
    parent: File,
    parent_identity: (u64, u64),
    bytes: Vec<u8>,
    identity: Identity,
}
impl Config {
    fn open(desktop: &Desktop) -> Result<Self> {
        let home = secure_directory(&desktop.home)?;
        check(
            home.metadata()?.uid() == files::uid(),
            Failure::UnsafeConfig,
        )?;
        let path = desktop.home.join(".config/hypr/hyprland.lua");
        let parent = secure_directory(
            path.parent()
                .ok_or_else(|| failure(Failure::UnsafeConfig))?,
        )?;
        let info = parent.metadata()?;
        check(info.uid() == files::uid(), Failure::UnsafeConfig)?;
        let (bytes, identity) = Self::read_current(&parent)?;
        Ok(Self {
            path,
            parent,
            parent_identity: (info.dev(), info.ino()),
            bytes,
            identity,
        })
    }
    fn read_current(parent: &File) -> Result<(Vec<u8>, Identity)> {
        let mut file = File::from(rfs::openat(
            parent,
            "hyprland.lua",
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let info = file.metadata()?;
        check(
            info.is_file()
                && info.uid() == files::uid()
                && info.nlink() == 1
                && info.mode() & 0o022 == 0
                && info.mode() & 0o7000 == 0
                && info.len() <= MAX_CONFIG as u64,
            Failure::UnsafeConfig,
        )?;
        let identity = Identity::of(&info);
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_CONFIG as u64 + 1)
            .read_to_end(&mut bytes)?;
        check(
            bytes.len() <= MAX_CONFIG
                && Identity::of(&file.metadata()?) == identity
                && std::str::from_utf8(&bytes).is_ok()
                && !bytes.contains(&0),
            Failure::UnsafeConfig,
        )?;
        Ok((bytes, identity))
    }
    fn current(&self) -> Result<()> {
        let parent = secure_directory(
            self.path
                .parent()
                .ok_or_else(|| failure(Failure::UnsafeConfig))?,
        )?;
        let info = parent.metadata()?;
        check(
            (info.dev(), info.ino()) == self.parent_identity,
            Failure::UnsafeConfig,
        )?;
        let (bytes, identity) = Self::read_current(&self.parent)?;
        check(
            identity == self.identity && bytes == self.bytes,
            Failure::UnsafeConfig,
        )
    }
    fn replace(&self, bytes: &[u8]) -> Result<()> {
        self.current()?;
        let name = format!(".flightdeck-display-{}", uuid::Uuid::new_v4().simple());
        let result = (|| {
            let mut file = File::from(rfs::openat(
                &self.parent,
                &name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            file.write_all(bytes)?;
            rfs::fchmod(&file, Mode::from_raw_mode(self.identity.mode))?;
            file.sync_all()?;
            // Recheck the exact bytes/inode after preparing the replacement.
            // An editor's atomic save is never overwritten based on old data.
            self.current()?;
            rfs::renameat(&self.parent, &name, &self.parent, "hyprland.lua")?;
            self.parent.sync_all()?;
            Ok(())
        })();
        let _ = rfs::unlinkat(&self.parent, &name, AtFlags::empty());
        result
    }
}

fn lua_omarchy(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.starts_with("--"))
        .collect();
    lines.iter().any(|line| {
        *line == "require(\"default.hypr.omarchy\")" || *line == "require('default.hypr.omarchy')"
    }) && lines.iter().any(|line| {
        line.contains("/default/hypr/bootstrap.lua")
            && line.starts_with("dofile(")
            && line.contains("/usr/share/omarchy")
    })
}

fn query(adapter: &mut impl Adapter, args: &[&str]) -> Result<Value> {
    serde_json::from_slice(&adapter.hyprctl(args, &AtomicBool::new(false))?)
        .map_err(|_| failure(Failure::Unsupported))
}
fn config_clean(adapter: &mut impl Adapter) -> Result<()> {
    let bytes = adapter.hyprctl(&["configerrors"], &AtomicBool::new(false))?;
    check(
        bytes.iter().all(u8::is_ascii_whitespace),
        Failure::ConfigErrors,
    )
}
fn current_compositor(adapter: &mut impl Adapter, desktop: &Desktop) -> Result<Compositor> {
    validate_desktop(desktop)?;
    check(adapter.trusted_hyprctl(), Failure::Unsupported)?;
    let version = query(adapter, &["-j", "version"])?;
    check(
        version["version"] == VERSION && version["commit"] == COMMIT && version["dirty"] == false,
        Failure::Unsupported,
    )?;
    let instances = query(adapter, &["-j", "instances"])?;
    let instances = instances
        .as_array()
        .ok_or_else(|| failure(Failure::Unsupported))?;
    let matching: Vec<_> = instances
        .iter()
        .filter(|v| v["instance"].as_str() == Some(&desktop.signature))
        .collect();
    check(matching.len() == 1, Failure::Unsupported)?;
    let pid = matching[0]["pid"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= i32::MAX as u64)
        .ok_or_else(|| failure(Failure::Unsupported))? as u32;
    let compositor = adapter.compositor(pid)?;
    check(
        compositor.uid == files::uid()
            && compositor.trusted_executable
            && !compositor.arguments.is_empty()
            && compositor.arguments.iter().skip(1).all(|arg| {
                arg != "-c"
                    && !arg.starts_with("-c")
                    && arg != "--config"
                    && !arg.starts_with("--config=")
            })
            && compositor
                .environment
                .get("HOME")
                .is_some_and(|p| Path::new(p) == desktop.home)
            && compositor
                .environment
                .get("XDG_CONFIG_HOME")
                .is_none_or(|p| Path::new(p) == desktop.config_home)
            && compositor
                .environment
                .get("OMARCHY_PATH")
                .is_none_or(|p| p == "/usr/share/omarchy"),
        Failure::Unsupported,
    )?;
    Ok(compositor)
}
fn no_x11_clients(adapter: &mut impl Adapter) -> Result<()> {
    let clients = query(adapter, &["-j", "clients"])?;
    let clients = clients
        .as_array()
        .ok_or_else(|| failure(Failure::Unsupported))?;
    check(
        clients.len() <= 65536 && clients.iter().all(|client| client["xwayland"] == false),
        Failure::Busy,
    )?;
    check(!adapter.xwayland_present()?, Failure::Busy)
}
fn enabled(adapter: &mut impl Adapter) -> Result<bool> {
    let option = query(adapter, &["-j", "getoption", "xwayland:enabled"])?;
    if let Some(value) = option["int"].as_i64() {
        check(value == 0 || value == 1, Failure::Unsupported)?;
        return Ok(value == 1);
    }
    option["bool"]
        .as_bool()
        .ok_or_else(|| failure(Failure::Unsupported))
}
fn reload(adapter: &mut impl Adapter) -> Result<()> {
    let bytes = adapter.hyprctl(&["reload", "config-only"], &AtomicBool::new(false))?;
    check(
        String::from_utf8_lossy(&bytes).trim() == "ok",
        Failure::RestorePending,
    )?;
    config_clean(adapter)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    token: String,
    home: PathBuf,
    original_sha256: String,
    original_mode: u32,
    phase: String,
}
fn block(token: &str) -> String {
    format!(
        "\n{MARKER}{token}:begin\nhl.config({{ xwayland = {{ enabled = false }} }})\n{MARKER}{token}:end\n"
    )
}
pub(super) fn pending(state_dir: &Path) -> bool {
    files::exists(&state_dir.join(JOURNAL))
}
pub(super) fn lock(state_dir: &Path) -> Result<files::Lease> {
    files::private_dir(state_dir)?;
    let directory = secure_directory(state_dir)?;
    check(
        directory.metadata()?.uid() == files::uid(),
        Failure::RestorePending,
    )?;
    files::Lease::acquire(&state_dir.join("display-preflight.lock"), true)
}
fn save(state_dir: &Path, journal: &Journal) -> Result<()> {
    files::atomic_json(&state_dir.join(JOURNAL), journal)
}
fn clear(state_dir: &Path) -> Result<()> {
    let parent = files::directory(state_dir, true)?;
    for name in [BACKUP, JOURNAL] {
        if files::exists(&state_dir.join(name)) {
            // Refuse a replaced symlink/hardlink rather than following it.
            files::open_at(&parent, name, false, true)?;
            rfs::unlinkat(&parent, name, AtFlags::empty())?;
            parent.sync_all()?;
        }
    }
    Ok(())
}

/// Restore current bytes minus our exact block; never copy the backup over an
/// editor's later content. The backup proves journal identity and remains until
/// the normal configuration has been reloaded successfully.
pub(super) fn restore(adapter: &mut impl Adapter, state_dir: &Path) -> Result<bool> {
    if !pending(state_dir) {
        return Ok(false);
    }
    let result: Result<bool> = (|| {
        let mut journal: Journal = files::json(&state_dir.join(JOURNAL), 16 * 1024)?;
        check(
            journal.schema == 1
                && journal.token.len() == 32
                && journal
                    .token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && files::hex_digest(&journal.original_sha256)
                && journal.original_mode & !0o777 == 0
                && journal.original_mode & 0o022 == 0
                && ["prepared", "installed", "restoring", "completed"]
                    .contains(&journal.phase.as_str()),
            Failure::RestorePending,
        )?;
        let desktop = adapter.desktop()?;
        check(desktop.home == journal.home, Failure::RestorePending)?;
        if journal.phase == "completed" {
            clear(state_dir)?;
            return Ok(false);
        }
        let config = Config::open(&desktop)?;
        let owned = block(&journal.token);
        let text =
            std::str::from_utf8(&config.bytes).map_err(|_| failure(Failure::RestorePending))?;
        let count = text.matches(&owned).count();
        check(count <= 1, Failure::RestorePending)?;
        if count == 0 && journal.phase == "prepared" && !text.contains(&journal.token) {
            journal.phase = "completed".into();
            save(state_dir, &journal)?;
            clear(state_dir)?;
            return Ok(false);
        }
        let backup = files::read(&state_dir.join(BACKUP), MAX_CONFIG)?;
        check(
            files::sha256(&backup) == journal.original_sha256,
            Failure::RestorePending,
        )?;
        // If a user edited the owned block itself, retain evidence and leave it
        // for explicit repair instead of deleting a guessed span of Lua.
        check(
            count == 1 || !text.contains(&journal.token),
            Failure::RestorePending,
        )?;
        if count == 1 {
            let restored = text.replacen(&owned, "", 1);
            journal.phase = "restoring".into();
            save(state_dir, &journal)?;
            config.replace(restored.as_bytes())?;
        }
        // Undo our exact file change even after logout/upgrade. If no supported
        // live compositor can safely reload it, keep the private journal and
        // report pending restoration, with no disabling block left on disk.
        current_compositor(adapter, &desktop)?;
        reload(adapter)?;
        journal.phase = "completed".into();
        save(state_dir, &journal)?;
        clear(state_dir)?;
        Ok(true)
    })();
    result.map_err(|_| failure(Failure::RestorePending))
}

pub(super) fn repair(
    adapter: &mut impl Adapter,
    state_dir: &Path,
    cancel: &AtomicBool,
    display: u32,
) -> Result<()> {
    let desktop = adapter
        .desktop()
        .map_err(|_| failure(Failure::Unsupported))?;
    let compositor =
        current_compositor(adapter, &desktop).map_err(|_| failure(Failure::Unsupported))?;
    check(
        compositor.x11_displays.contains(&display),
        Failure::Unsupported,
    )?;
    config_clean(adapter)?;
    no_x11_clients(adapter)?;
    check(enabled(adapter)?, Failure::Unsupported)?;
    let config = Config::open(&desktop).map_err(|_| failure(Failure::UnsafeConfig))?;
    check(
        lua_omarchy(&config.bytes) && !String::from_utf8_lossy(&config.bytes).contains(MARKER),
        Failure::UnsafeConfig,
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    check(
        !pending(state_dir) && !files::exists(&state_dir.join(BACKUP)),
        Failure::RestorePending,
    )?;
    let mut journal = Journal {
        schema: 1,
        token: uuid::Uuid::new_v4().simple().to_string(),
        home: desktop.home.clone(),
        original_sha256: files::sha256(&config.bytes),
        original_mode: config.identity.mode,
        phase: "prepared".into(),
    };
    save(state_dir, &journal)?;
    // The prepared journal exists before the backup. A crash here is known to
    // precede configuration mutation and can be cleared without a backup.
    files::atomic(&state_dir.join(BACKUP), &config.bytes)?;
    let operation = (|| {
        // Last checks immediately before mutation. A client/process that
        // appeared while the journal was prepared prevents the toggle.
        no_x11_clients(adapter)?;
        let compositor = current_compositor(adapter, &desktop)?;
        check(
            compositor.x11_displays.contains(&display),
            Failure::Unsupported,
        )?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut temporary = config.bytes.clone();
        temporary.extend_from_slice(block(&journal.token).as_bytes());
        config.replace(&temporary)?;
        journal.phase = "installed".into();
        save(state_dir, &journal)?;
        reload(adapter)?;
        check(!enabled(adapter)?, Failure::RestorePending)?;
        Ok(())
    })();
    // Cleanup intentionally ignores cancellation: once a desktop override was
    // installed, restoring it takes priority over abandoning the launch.
    restore(adapter, state_dir)?;
    operation?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    Ok(())
}
