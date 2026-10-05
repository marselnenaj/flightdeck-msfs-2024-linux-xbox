// SPDX-License-Identifier: MIT
//! Descriptor-relative inventory and removal of reviewed, quarantined trees.
use crate::{Error, Result, error::require, files, transaction};
use rustix::fs::{self as fs, AtFlags, FileType};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::File,
    os::unix::{ffi::OsStringExt, fs::MetadataExt},
    path::Path,
    sync::atomic::AtomicBool,
};
const FOREIGN: &str =
    "Der Ordner enthält fremde Dateien oder eingebundene Laufwerke. Er wird nicht entfernt.";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub id: [u64; 2],
    pub fingerprint: String,
    pub bytes: u64,
}
pub fn identity(path: &Path) -> Result<[u64; 2]> {
    let info = files::directory(path, false)?.metadata()?;
    Ok([info.dev(), info.ino()])
}
fn names(folder: &File) -> Result<Vec<OsString>> {
    let mut result = Vec::new();
    for entry in fs::Dir::read_from(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if name != b"." && name != b".." {
            require(result.len() < 1_000_000, FOREIGN)?;
            result.push(OsString::from_vec(name.to_vec()));
        }
    }
    result.sort();
    Ok(result)
}
fn checked(parent: &File, name: &OsString, device: u64) -> Result<fs::Stat> {
    let stat = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    require(
        stat.st_dev == device
            && stat.st_uid == files::uid()
            && matches!(
                FileType::from_raw_mode(stat.st_mode),
                FileType::RegularFile | FileType::Directory | FileType::Symlink
            ),
        FOREIGN,
    )?;
    Ok(stat)
}
struct Walk<'a> {
    device: u64,
    count: usize,
    bytes: u64,
    hash: Sha256,
    cancel: &'a AtomicBool,
}
impl Walk<'_> {
    fn scan(&mut self, folder: &File, path: &Path, depth: usize) -> Result<()> {
        require(depth <= 256, FOREIGN)?;
        for name in names(folder)? {
            transaction::interrupted(self.cancel)?;
            self.count += 1;
            require(self.count <= 1_000_000, FOREIGN)?;
            let stat = checked(folder, &name, self.device)?;
            let kind = FileType::from_raw_mode(stat.st_mode);
            let relative = path.join(&name);
            // Length-delimited raw names avoid ambiguity and accept valid Unix
            // names without exporting any of them in the fingerprint.
            let path = relative.as_os_str().as_encoded_bytes();
            self.hash.update((path.len() as u64).to_le_bytes());
            self.hash.update(path);
            for number in [
                stat.st_ino,
                stat.st_mode as u64,
                stat.st_size as u64,
                stat.st_mtime as u64,
                stat.st_mtime_nsec as u64,
                stat.st_ctime as u64,
                stat.st_ctime_nsec as u64,
            ] {
                self.hash.update(number.to_le_bytes());
            }
            if kind == FileType::RegularFile {
                self.bytes = self
                    .bytes
                    .checked_add(stat.st_size as u64)
                    .ok_or(Error::Invalid(FOREIGN))?;
            } else if kind == FileType::Symlink {
                let target = fs::readlinkat(folder, &name, Vec::new())?;
                self.hash
                    .update((target.as_bytes().len() as u64).to_le_bytes());
                self.hash.update(target.as_bytes());
            } else {
                let child = files::open_at(folder, &name, true, false)?;
                let info = child.metadata()?;
                require(
                    [info.dev(), info.ino()] == [stat.st_dev, stat.st_ino],
                    FOREIGN,
                )?;
                self.scan(&child, &relative, depth + 1)?;
            }
        }
        Ok(())
    }
}
pub fn inventory(root: &Path, cancel: &AtomicBool) -> Result<Inventory> {
    let folder = files::directory(root, false)?;
    let info = folder.metadata()?;
    let mut walk = Walk {
        device: info.dev(),
        count: 0,
        bytes: 0,
        hash: Sha256::new(),
        cancel,
    };
    walk.scan(&folder, Path::new(""), 0)?;
    require(identity(root)? == [info.dev(), info.ino()], FOREIGN)?;
    Ok(Inventory {
        id: [info.dev(), info.ino()],
        fingerprint: hex::encode(walk.hash.finalize()),
        bytes: walk.bytes,
    })
}
fn remove_contents(folder: &File, device: u64, count: &mut usize, depth: usize) -> Result<()> {
    require(depth <= 256, FOREIGN)?;
    for name in names(folder)? {
        *count += 1;
        require(*count <= 1_000_000, FOREIGN)?;
        let before = checked(folder, &name, device)?;
        let directory = FileType::from_raw_mode(before.st_mode) == FileType::Directory;
        if directory {
            let child = files::open_at(folder, &name, true, false)?;
            let info = child.metadata()?;
            require(
                [info.dev(), info.ino()] == [before.st_dev, before.st_ino],
                FOREIGN,
            )?;
            remove_contents(&child, device, count, depth + 1)?;
        }
        let after = checked(folder, &name, device)?;
        require(
            (before.st_dev, before.st_ino, before.st_mode)
                == (after.st_dev, after.st_ino, after.st_mode),
            FOREIGN,
        )?;
        fs::unlinkat(
            folder,
            &name,
            if directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        )?;
    }
    folder.sync_all()?;
    Ok(())
}
/// Only call after quarantine and configuration publication. The reviewed root
/// identity prevents deleting a replacement; links and mounts are never entered.
pub fn remove(root: &Path, expected: [u64; 2]) -> Result<()> {
    let parent = files::directory(root.parent().ok_or(Error::Invalid(FOREIGN))?, false)?;
    let name = root.file_name().ok_or(Error::Invalid(FOREIGN))?;
    remove_at(&parent, Path::new(name), expected)
}
/// Remove a reviewed entry from an already pinned, private quarantine directory.
pub fn remove_at(parent: &File, name: &Path, expected: [u64; 2]) -> Result<()> {
    require(
        name.components().count() == 1 && name.file_name().is_some(),
        FOREIGN,
    )?;
    let folder = files::open_at(parent, name, true, false)?;
    let info = folder.metadata()?;
    require([info.dev(), info.ino()] == expected, FOREIGN)?;
    remove_contents(&folder, info.dev(), &mut 0, 0)?;
    let linked = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    require([linked.st_dev, linked.st_ino] == expected, FOREIGN)?;
    fs::unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    parent.sync_all()?;
    Ok(())
}
