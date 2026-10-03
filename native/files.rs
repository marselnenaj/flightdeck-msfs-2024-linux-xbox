// SPDX-License-Identifier: MIT
//! Bounded, descriptor-relative filesystem access shared by the launcher.
use crate::{Error, Result, error::require};
use rustix::fs::{self as rfs, AtFlags, CWD, FlockOperation, Mode, OFlags};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::AsFd,
        unix::fs::{DirBuilderExt, MetadataExt},
    },
    path::{Component, Path, PathBuf},
};

const INVALID: &str = "Datei oder Ordner ist nicht sicher zugänglich.";

pub fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}
pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/nonexistent"))
}
pub fn xdg(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}
pub fn expand(value: &str) -> PathBuf {
    if value == "~" {
        home()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home().join(rest)
    } else {
        value.into()
    }
}
pub fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn directory(path: &Path, private: bool) -> Result<File> {
    let fd = rfs::openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let file = File::from(fd);
    let info = file.metadata()?;
    require(
        info.uid() == uid() && (!private || info.mode() & 0o077 == 0),
        INVALID,
    )?;
    Ok(file)
}

pub fn private_dir(path: &Path) -> Result<File> {
    if !exists(path) {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let file = directory(path, false)?;
    rfs::fchmod(&file, Mode::from_raw_mode(0o700))?;
    Ok(file)
}

pub fn open_at(
    parent: impl AsFd,
    name: impl AsRef<Path>,
    directory: bool,
    private: bool,
) -> Result<File> {
    let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    if directory {
        flags |= OFlags::DIRECTORY;
    }
    let file = File::from(rfs::openat(parent, name.as_ref(), flags, Mode::empty())?);
    let info = file.metadata()?;
    require(
        info.uid() == uid()
            && (if directory {
                info.is_dir()
            } else {
                info.is_file() && info.nlink() == 1
            })
            && (!private || info.mode() & 0o077 == 0),
        INVALID,
    )?;
    Ok(file)
}

pub fn read_file(mut file: File, maximum: usize) -> Result<Vec<u8>> {
    require(
        file.metadata()?.is_file() && file.metadata()?.len() <= maximum as u64,
        INVALID,
    )?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    require(bytes.len() <= maximum, INVALID)?;
    Ok(bytes)
}
pub fn read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    read_file(open_at(CWD, path, false, false)?, maximum)
}
/// Bounded read of explicitly chosen public resources, including root-owned
/// driver/runtime manifests. Mutable account/runtime data must use `read`.
pub fn read_public(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let fd = rfs::openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    read_file(File::from(fd), maximum)
}
pub fn json<T: DeserializeOwned>(path: &Path, maximum: usize) -> Result<T> {
    Ok(serde_json::from_slice(&read(path, maximum)?)?)
}
pub fn json_at<T: DeserializeOwned>(parent: impl AsFd, name: &str, maximum: usize) -> Result<T> {
    Ok(serde_json::from_slice(&read_file(
        open_at(parent, name, false, false)?,
        maximum,
    )?)?)
}

pub fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\\', ':', '\0'])
        && value
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
pub fn beneath(parent: &File, name: &str) -> Result<File> {
    require(relative(name), INVALID)?;
    let mut folder = parent.try_clone()?;
    let parts: Vec<_> = name.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        folder = open_at(&folder, part, true, false)?;
    }
    open_at(&folder, parts[parts.len() - 1], false, false)
}

pub fn atomic_at(parent: &File, name: &str, bytes: &[u8]) -> Result<()> {
    require(
        !name.is_empty() && !name.contains('/') && name != "." && name != "..",
        INVALID,
    )?;
    let temporary = format!(".flightdeck-{}", uuid::Uuid::new_v4().simple());
    let result = (|| {
        let mut file = File::from(rfs::openat(
            parent,
            &temporary,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        file.write_all(bytes)?;
        file.sync_all()?;
        rfs::renameat(parent, &temporary, parent, name)?;
        parent.sync_all()?;
        Ok(())
    })();
    let _ = rfs::unlinkat(parent, &temporary, AtFlags::empty());
    result
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = directory(path.parent().ok_or(Error::Invalid(INVALID))?, false)?;
    atomic_at(
        &parent,
        path.file_name()
            .and_then(|v| v.to_str())
            .ok_or(Error::Invalid(INVALID))?,
        bytes,
    )
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic(path, &bytes)
}
pub fn digest(mut file: File) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

/// An acquired lease owns its descriptor; dropping it releases the lock even on errors.
pub struct Lease(pub File);
impl Lease {
    pub fn acquire(path: &Path, create: bool) -> Result<Self> {
        let parent = directory(path.parent().ok_or(Error::Invalid(INVALID))?, true)?;
        let name = path.file_name().ok_or(Error::Invalid(INVALID))?;
        let mut flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
        if create {
            flags |= OFlags::CREATE;
        }
        let file = File::from(rfs::openat(
            &parent,
            name,
            flags,
            Mode::from_raw_mode(0o600),
        )?);
        let info = file.metadata()?;
        require(
            info.is_file() && info.nlink() == 1 && info.uid() == uid() && info.mode() & 0o077 == 0,
            INVALID,
        )?;
        rfs::flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|_| {
            Error::Invalid("Das Spiel oder ein anderer Runtimevorgang läuft bereits.")
        })?;
        let linked = rfs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
        require(
            linked.st_ino == info.ino() && linked.st_dev == info.dev(),
            INVALID,
        )?;
        Ok(Self(file))
    }
}
