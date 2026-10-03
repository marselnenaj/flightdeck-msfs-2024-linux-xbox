// SPDX-License-Identifier: MIT
//! Anchored private storage. Every path component and publication is checked.
use crate::{
    cloud::{Failure, Result, require},
    files,
};
use rustix::fs::{self as rfs, AtFlags, FlockOperation, Mode, OFlags, RenameFlags};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};

pub fn open(path: &Path, private: bool) -> Result<File> {
    require(path.is_absolute(), "local_storage")?;
    let mut folder = File::from(rfs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    for part in path.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(name) => {
                folder = File::from(rfs::openat(
                    &folder,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?)
            }
            _ => return Err(Failure::new("local_storage")),
        }
    }
    let m = folder.metadata()?;
    require(
        m.uid() == files::uid() && (!private || m.mode() & 0o077 == 0),
        "local_storage",
    )?;
    Ok(folder)
}
pub fn child(parent: &File, name: &str, create: bool) -> Result<File> {
    component(name)?;
    if create {
        match rfs::mkdirat(parent, name, Mode::from_raw_mode(0o700)) {
            Ok(()) => parent.sync_all()?,
            Err(rustix::io::Errno::EXIST) => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(files::open_at(parent, name, true, true)?)
}
pub fn component(name: &str) -> Result<()> {
    require(
        !name.is_empty() && !name.contains(['/', '\0']) && name != "." && name != "..",
        "local_storage",
    )
}
pub fn read(parent: &File, name: &str, maximum: usize) -> Result<Vec<u8>> {
    component(name)?;
    let mut file = files::open_at(parent, name, false, true)?;
    let before = file.metadata()?;
    require(before.len() <= maximum as u64, "unsupported")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    require(
        bytes.len() as u64 == before.len()
            && bytes.len() <= maximum
            && (
                before.len(),
                before.mtime(),
                before.mtime_nsec(),
                before.ctime(),
                before.ctime_nsec(),
            ) == (
                after.len(),
                after.mtime(),
                after.mtime_nsec(),
                after.ctime(),
                after.ctime_nsec(),
            ),
        "changed",
    )?;
    Ok(bytes)
}
pub fn optional(parent: &File, name: &str, maximum: usize) -> Result<Option<Vec<u8>>> {
    component(name)?;
    match rfs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(e) => Err(e.into()),
        Ok(_) => read(parent, name, maximum).map(Some),
    }
}
pub fn linked(parent: &File, name: &str, child: &File) -> Result<()> {
    component(name)?;
    let a = child.metadata()?;
    let b = rfs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    require(
        rfs::FileType::from_raw_mode(b.st_mode) == rfs::FileType::Directory
            && a.dev() == b.st_dev
            && a.ino() == b.st_ino,
        "changed",
    )
}
pub fn same(path: &Path, folder: &File, private: bool) -> Result<()> {
    let a = open(path, private)?.metadata()?;
    let b = folder.metadata()?;
    require((a.dev(), a.ino()) == (b.dev(), b.ino()), "changed")
}
pub fn write(parent: &File, name: &str, bytes: &[u8], mode: u32) -> Result<()> {
    component(name)?;
    let mut out = File::from(rfs::openat(
        parent,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(mode),
    )?);
    out.write_all(bytes)?;
    out.sync_all()?;
    Ok(())
}
pub fn publish(parent: &File, from: &str, to: &str) -> Result<()> {
    component(from)?;
    component(to)?;
    rfs::renameat_with(parent, from, parent, to, RenameFlags::NOREPLACE)?;
    parent
        .sync_all()
        .map_err(|_| Failure::new("durability_unknown"))
}
pub fn unlink(parent: &File, name: &str, directory: bool) -> Result<()> {
    component(name)?;
    match rfs::unlinkat(
        parent,
        name,
        if directory {
            AtFlags::REMOVEDIR
        } else {
            AtFlags::empty()
        },
    ) {
        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn names(parent: &File) -> Result<std::collections::BTreeSet<String>> {
    let mut result = std::collections::BTreeSet::new();
    for e in rfs::Dir::read_from(parent)? {
        let e = e?;
        let name = e
            .file_name()
            .to_str()
            .map_err(|_| Failure::new("local_storage"))?;
        if name != "." && name != ".." {
            require(result.len() < 100000, "local_storage")?;
            result.insert(name.into());
        }
    }
    Ok(result)
}
pub fn lease(private: &File, lease: &File) -> Result<()> {
    let test = || -> Result<()> {
        let a = lease.metadata()?;
        let probe = files::open_at(private, "play.lock", false, true)?;
        let b = probe.metadata()?;
        require(
            a.is_file()
                && a.uid() == files::uid()
                && a.nlink() == 1
                && a.mode() & 0o077 == 0
                && (a.dev(), a.ino()) == (b.dev(), b.ino()),
            "invalid_lock",
        )?;
        match rfs::flock(&probe, FlockOperation::NonBlockingLockExclusive) {
            Err(rustix::io::Errno::WOULDBLOCK) => (),
            Ok(()) => {
                rfs::flock(&probe, FlockOperation::Unlock)?;
                return Err(Failure::new("invalid_lock"));
            }
            Err(_) => return Err(Failure::new("invalid_lock")),
        }
        rfs::flock(lease, FlockOperation::NonBlockingLockExclusive)?;
        Ok(())
    };
    test().map_err(|_| Failure::new("invalid_lock"))
}
