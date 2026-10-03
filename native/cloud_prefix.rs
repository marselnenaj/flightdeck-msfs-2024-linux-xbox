// SPDX-License-Identifier: MIT
//! Reuse only a verified, stopped private Wine prefix; never authentication or processes.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_fs as fs, cloud_process_guard, files,
};
use rustix::fs::{self as rfs, AtFlags, FlockOperation, Mode, OFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
const STATE: &str = "prefix-state.json";
pub type Libraries = BTreeMap<String, Vec<u8>>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    binding: String,
    boot: String,
    phase: String,
}
fn directory(parent: &File, name: &str) -> Result<File> {
    let folder = files::open_at(parent, name, true, false)?;
    require(folder.metadata()?.mode() & 0o022 == 0, "local_storage")?;
    Ok(folder)
}
fn material(parent: &File, name: &str, maximum: usize) -> Result<Vec<u8>> {
    let file = files::open_at(parent, name, false, false)?;
    let metadata = file.metadata()?;
    require(metadata.mode() & 0o022 == 0, "local_storage")?;
    let data = files::read_file(file, maximum)?;
    require(data.len() as u64 == metadata.len(), "local_storage")?;
    Ok(data)
}
fn system(work: &File) -> Result<File> {
    let mut folder = work.try_clone()?;
    for name in ["prefix", "drive_c", "windows", "system32"] {
        folder = directory(&folder, name)?;
    }
    Ok(folder)
}
fn valid(work: &File, libraries: &Libraries) -> Result<()> {
    let prefix = directory(work, "prefix")?;
    for name in ["system.reg", "user.reg", "userdef.reg"] {
        require(
            material(&prefix, name, 32 * 1024 * 1024)?.starts_with(b"WINE REGISTRY Version 2"),
            "local_storage",
        )?;
    }
    let devices = directory(&prefix, "dosdevices")?;
    require(
        rfs::readlinkat(&devices, "c:", Vec::new())?.to_bytes() == b"../drive_c"
            && rfs::readlinkat(&devices, "z:", Vec::new())?.to_bytes() == b"/",
        "local_storage",
    )?;
    let system = system(work)?;
    for (name, payload) in libraries {
        require(
            files::sha256(&material(&system, name, 32 * 1024 * 1024)?) == files::sha256(payload),
            "local_storage",
        )?;
    }
    Ok(())
}
pub(crate) fn remove_tree(
    parent: &File,
    name: &str,
    budget: &mut usize,
    depth: usize,
) -> Result<()> {
    require(depth <= 64 && *budget > 0, "local_storage")?;
    let folder = directory(parent, name)?;
    fs::linked(parent, name, &folder)?;
    for entry in rfs::Dir::read_from(&folder)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| Failure::new("local_storage"))?;
        if name == "." || name == ".." {
            continue;
        }
        *budget = budget
            .checked_sub(1)
            .ok_or_else(|| Failure::new("local_storage"))?;
        let m = rfs::statat(&folder, name, AtFlags::SYMLINK_NOFOLLOW)?;
        require(m.st_uid == files::uid(), "local_storage")?;
        if rfs::FileType::from_raw_mode(m.st_mode) == rfs::FileType::Directory {
            remove_tree(&folder, name, budget, depth + 1)?;
        } else {
            rfs::unlinkat(&folder, name, AtFlags::empty())?;
        }
    }
    fs::linked(parent, name, &folder)?;
    rfs::unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    Ok(())
}
fn store(work: &File, binding: &str, boot: &str, phase: &str) -> Result<()> {
    files::atomic_at(
        work,
        STATE,
        &serde_json::to_vec(&Record {
            schema: 1,
            binding: binding.into(),
            boot: boot.into(),
            phase: phase.into(),
        })?,
    )?;
    Ok(())
}
pub struct Prefix {
    pub work: PathBuf,
    pub reusable: bool,
    work_fd: File,
    private: File,
    _lock: Option<File>,
    libraries: Libraries,
    binding: Option<String>,
    boot: Option<String>,
    temporary: Option<String>,
    stopped: bool,
}
impl Prefix {
    pub fn new(runtime: &Path, binding: &str, libraries: Libraries) -> Result<Self> {
        require(
            files::hex_digest(binding)
                && libraries
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>()
                    == [
                        "xgameruntime.dll",
                        "xgameruntime_original.dll",
                        "xodus_store_test.dll",
                    ]
                    .into()
                && libraries
                    .values()
                    .all(|v| !v.is_empty() && v.len() <= 32 * 1024 * 1024),
            "local_storage",
        )?;
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        let cached = (|| -> Result<(File, File, bool, String)> {
            let boot = cloud_process_guard::boot_id()?;
            let cache = fs::child(&private, "cloud-helper-prefixes", true)?;
            let work = fs::child(&cache, binding, true)?;
            let lock = File::from(rfs::openat(
                &work,
                "use.lock",
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            let m = lock.metadata()?;
            require(
                m.is_file() && m.uid() == files::uid() && m.mode() & 0o077 == 0 && m.nlink() == 1,
                "local_storage",
            )?;
            rfs::flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
            let previous: Option<Record> = fs::optional(&work, STATE, 512)?
                .as_deref()
                .map(cloud::decode)
                .transpose()?;
            if let Some(p) = &previous {
                require(
                    p.schema == 1
                        && p.binding == binding
                        && matches!(p.phase.as_str(), "ready" | "in_use")
                        && cloud::guid(&p.boot)
                        && p.boot == p.boot.to_ascii_lowercase()
                        && p.boot != "00000000-0000-0000-0000-000000000000",
                    "local_storage",
                )?;
                require(p.phase != "in_use" || p.boot != boot, "local_storage")?;
            }
            let warm = previous.as_ref().is_some_and(|p| p.phase == "ready")
                && valid(&work, &libraries).is_ok();
            if !warm {
                match rfs::statat(&work, "prefix", AtFlags::SYMLINK_NOFOLLOW) {
                    Err(rustix::io::Errno::NOENT) => (),
                    Err(e) => return Err(e.into()),
                    Ok(_) => {
                        require(previous.is_some(), "local_storage")?;
                        remove_tree(&work, "prefix", &mut 100000, 0)?;
                        work.sync_all()?;
                    }
                }
            }
            store(&work, binding, &boot, "in_use")?;
            fs::linked(&private, "cloud-helper-prefixes", &cache)?;
            fs::linked(&cache, binding, &work)?;
            Ok((work, lock, warm, boot))
        })();
        fs::same(runtime, &root, false)?;
        fs::linked(&root, "private", &private)?;
        match cached {
            Ok((work_fd, lock, reusable, boot)) => Ok(Self {
                work: runtime.join("private/cloud-helper-prefixes").join(binding),
                work_fd,
                private,
                _lock: Some(lock),
                reusable,
                libraries,
                binding: Some(binding.into()),
                boot: Some(boot),
                temporary: None,
                stopped: false,
            }),
            Err(_) => {
                let name = format!("cloud-reader-{}", uuid::Uuid::new_v4().simple());
                rfs::mkdirat(&private, &name, Mode::from_raw_mode(0o700))?;
                let work_fd = fs::child(&private, &name, false)?;
                Ok(Self {
                    work: runtime.join("private").join(&name),
                    work_fd,
                    private,
                    _lock: None,
                    reusable: false,
                    libraries,
                    binding: None,
                    boot: None,
                    temporary: Some(name),
                    stopped: false,
                })
            }
        }
    }
    pub fn path(&self) -> PathBuf {
        self.work.join("prefix")
    }
    pub fn install_libraries(&self) -> Result<()> {
        fs::same(&self.work, &self.work_fd, true)?;
        let system = system(&self.work_fd)?;
        for (name, data) in &self.libraries {
            files::atomic_at(&system, name, data)?;
        }
        system.sync_all()?;
        Ok(())
    }
    /// Must follow confirmed cleanup of both owned children and this prefix's server.
    pub fn complete(&mut self) {
        self.stopped = true;
        if let (Some(binding), Some(boot)) = (&self.binding, &self.boot)
            && fs::same(&self.work, &self.work_fd, true).is_ok()
            && valid(&self.work_fd, &self.libraries).is_ok()
        {
            let _ = store(&self.work_fd, binding, boot, "ready");
        }
    }
}
impl Drop for Prefix {
    fn drop(&mut self) {
        if self.stopped
            && let Some(name) = &self.temporary
        {
            let _ = remove_tree(&self.private, name, &mut 100000, 0);
            let _ = self.private.sync_all();
        }
    }
}
