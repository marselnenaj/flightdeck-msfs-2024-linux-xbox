// SPDX-License-Identifier: MIT
//! Durable copies and directory publication for owned installation transactions.
use crate::{Error, Result, error::require, files};
use rustix::fs::{self as rfs, AtFlags, CWD, Mode, OFlags, RenameFlags};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub fn interrupted(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
pub fn digest(path: &Path) -> Result<String> {
    files::digest(files::open_at(CWD, path, false, false)?)
}
pub fn owned_file(root: &Path, path: &Path) -> Result<File> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("Der Runtime-Pfad ist ungültig."))?;
    files::beneath(
        &files::directory(root, false)?,
        relative
            .to_str()
            .ok_or(Error::Invalid("Der Runtime-Pfad ist ungültig."))?,
    )
}
pub fn publish(source: &Path, destination: &Path) -> Result<()> {
    rfs::renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE)?;
    files::directory(
        destination
            .parent()
            .ok_or(Error::Invalid("Ungültiger Zielpfad."))?,
        false,
    )?
    .sync_all()?;
    Ok(())
}
pub fn exchange(first: &Path, second: &Path) -> Result<()> {
    rfs::renameat_with(CWD, first, CWD, second, RenameFlags::EXCHANGE)?;
    for path in [first, second] {
        files::directory(
            path.parent()
                .ok_or(Error::Invalid("Ungültiger Zielpfad."))?,
            false,
        )?
        .sync_all()?;
    }
    Ok(())
}
/// Copy from an already checked descriptor. Never follow the destination entry.
pub fn copy(
    mut source: File,
    target: &Path,
    expected: Option<&str>,
    cancel: &AtomicBool,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    let metadata = source.metadata()?;
    require(metadata.is_file(), "Die Quelle ist keine reguläre Datei.")?;
    let parent_path = target
        .parent()
        .ok_or(Error::Invalid("Ungültiger Zielpfad."))?;
    files::private_dir(parent_path)?;
    let parent = files::directory(parent_path, false)?;
    let name = target
        .file_name()
        .ok_or(Error::Invalid("Ungültiger Zielpfad."))?;
    let temp = format!(".flightdeck-copy-{}", uuid::Uuid::new_v4().simple());
    let result = (|| {
        let mut output = File::from(rfs::openat(
            &parent,
            &temp,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            interrupted(cancel)?;
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
            output.write_all(&buffer[..count])?;
        }
        if let Some(expected) = expected {
            require(
                hex::encode(hash.finalize()) == expected,
                "Eine Komponente wurde während der Kopie verändert.",
            )?;
        }
        rfs::fchmod(&output, Mode::from_raw_mode(metadata.mode() & 0o700))?;
        output.sync_all()?;
        rfs::renameat(&parent, &temp, &parent, name)?;
        parent.sync_all()?;
        Ok(())
    })();
    let _ = rfs::unlinkat(&parent, &temp, AtFlags::empty());
    result
}
pub fn copy_path(
    source: &Path,
    target: &Path,
    expected: Option<&str>,
    cancel: &AtomicBool,
) -> Result<()> {
    copy(
        files::open_at(CWD, source, false, false)?,
        target,
        expected,
        cancel,
    )
}
pub fn new_directory(parent: &Path, prefix: &str) -> Result<PathBuf> {
    if !files::exists(parent) {
        files::private_dir(parent)?;
    }
    let path = parent.join(format!("{prefix}{}", uuid::Uuid::new_v4().simple()));
    let directory = files::directory(parent, false)?;
    rfs::mkdirat(
        &directory,
        path.file_name()
            .ok_or(Error::Invalid("Ungültiger Zielpfad."))?,
        Mode::from_raw_mode(0o700),
    )?;
    directory.sync_all()?;
    Ok(path)
}
pub fn resolve_new(path: &Path) -> Result<PathBuf> {
    require(
        path.is_absolute() && !files::exists(path),
        "Der Zielordner existiert bereits. Bitte einen neuen Ordner wählen.",
    )?;
    let mut suffix = Vec::new();
    let mut existing = path;
    while !files::exists(existing) {
        suffix.push(
            existing
                .file_name()
                .ok_or(Error::Invalid("Ungültiger Zielpfad."))?
                .to_os_string(),
        );
        existing = existing
            .parent()
            .ok_or(Error::Invalid("Ungültiger Zielpfad."))?;
    }
    let mut result = existing.canonicalize()?;
    for part in suffix.iter().rev() {
        result.push(part);
    }
    require(
        result
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
        "Ungültiger Zielpfad.",
    )?;
    Ok(result)
}
pub fn prefix_system32(prefix: &Path) -> Result<PathBuf> {
    let root = files::directory(prefix, false)?;
    let drive = files::open_at(&root, "drive_c", true, false)?;
    let windows = files::open_at(&drive, "windows", true, false)?;
    files::open_at(&windows, "system32", true, false)?;
    Ok(prefix.join("drive_c/windows/system32"))
}
pub fn walk(root: &Path, cancel: &AtomicBool) -> Result<Vec<PathBuf>> {
    files::directory(root, false)?;
    let mut folders = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    while let Some(folder) = folders.pop() {
        interrupted(cancel)?;
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            require(
                entries.len() < 1_000_000,
                "Der Profilordner enthält zu viele Dateien.",
            )?;
            if kind.is_dir() {
                folders.push(entry.path());
            }
            entries.push(entry.path());
        }
    }
    Ok(entries)
}
pub fn prefix_size(root: &Path, cancel: &AtomicBool) -> Result<u64> {
    let mut total = 0_u64;
    for path in walk(root, cancel)? {
        let m = fs::symlink_metadata(path)?;
        if m.is_file() {
            total = total
                .checked_add(m.len())
                .ok_or(Error::Invalid("Die Profilkopie ist zu groß."))?;
        }
    }
    Ok(total)
}
/// Relative internal links keep copied profiles valid after the final rename.
pub fn relocate_prefix_links(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    for link in walk(destination, cancel)? {
        if !fs::symlink_metadata(&link)?.is_symlink() {
            continue;
        }
        let target = fs::read_link(&link)?;
        if let Ok(relative) = target.strip_prefix(source) {
            let parent = link
                .parent()
                .ok_or(Error::Invalid("Ungültiger Profilpfad."))?
                .strip_prefix(destination)
                .map_err(|_| Error::Invalid("Ungültiger Profilpfad."))?;
            let target_parts: Vec<_> = relative.components().collect();
            let parent_parts: Vec<_> = parent.components().collect();
            let common = target_parts
                .iter()
                .zip(&parent_parts)
                .take_while(|(a, b)| a == b)
                .count();
            let mut new_target = PathBuf::new();
            for _ in common..parent_parts.len() {
                new_target.push("..");
            }
            for part in &target_parts[common..] {
                new_target.push(part.as_os_str());
            }
            if new_target.as_os_str().is_empty() {
                new_target.push(".");
            }
            fs::remove_file(&link)?;
            std::os::unix::fs::symlink(new_target, link)?;
        }
    }
    Ok(())
}
pub fn free_bytes(path: &Path) -> Result<u64> {
    let mut path = path;
    while !path.exists() {
        path = path
            .parent()
            .ok_or(Error::Invalid("Ungültiger Zielpfad."))?;
    }
    let stats = rfs::statvfs(path)?;
    Ok(stats.f_bavail.saturating_mul(stats.f_frsize))
}
