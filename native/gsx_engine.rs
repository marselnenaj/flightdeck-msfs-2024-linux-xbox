// SPDX-License-Identifier: MIT
//! Pinned FSDT engine updates, decoded into a fresh staging tree before overlay.
//! Licensing, the full Community product and simulator startup remain separate.
use crate::{Error, Result, error::require, files, games::Game, transaction as tx};
use rustix::fs::{self as rfs, AtFlags, Mode, OFlags};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};
use zip::{ZipArchive, read::HasZipMetadata};

const INVALID: &str = "Das GSX-Engine-Paket enthält ungültige Dateien.";
const BASE: &str =
    "https://github.com/virtualisoftware/fsdt-offline-installer/releases/download/2.4.8.0/";
const MAX_ENTRIES: usize = 8_000;
const MAX_TOTAL: u64 = 768 * 1024 * 1024;
const MAX_FILE: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub target: String,
    pub bytes: u64,
    pub sha256: String,
    pub entries: usize,
    pub unpacked_bytes: u64,
}
impl Package {
    pub fn url(&self) -> String {
        format!("{BASE}{}", self.name)
    }
}
#[derive(Deserialize)]
struct Lock {
    format: u64,
    release: String,
    packages: Vec<Package>,
}
pub fn packages() -> Result<Vec<Package>> {
    let lock: Lock = serde_json::from_slice(include_bytes!("../compat/gsx-engine.lock.json"))?;
    require(
        lock.format == 1 && lock.release == "2.4.8.0" && lock.packages.len() == 7,
        INVALID,
    )?;
    Ok(lock.packages)
}

/// Enforce the pinned length while streaming, before accepting any cache file.
pub fn download(package: &Package, target: &Path, cancel: &AtomicBool) -> Result<PathBuf> {
    tx::interrupted(cancel)?;
    name(&package.name)?;
    require(
        !package.name.contains('/')
            && package.bytes > 0
            && package.bytes <= 256 * 1024 * 1024
            && files::hex_digest(&package.sha256),
        INVALID,
    )?;
    if files::exists(target) {
        let cached = files::open_at(rfs::CWD, target, false, true)?;
        if cached.metadata()?.len() == package.bytes && files::digest(cached)? == package.sha256 {
            return Ok(target.into());
        }
    }
    let folder = files::private_dir(target.parent().ok_or(Error::Invalid(INVALID))?)?;
    let temporary = format!(".gsx-download-{}", uuid::Uuid::new_v4().simple());
    let result = (|| {
        let client = crate::http_client::builder(Duration::from_secs(60))
            .build()
            .map_err(|_| Error::Invalid("Der GSX-Download konnte nicht vorbereitet werden."))?;
        let mut response = client
            .get(package.url())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|_| {
                Error::Invalid("Die offizielle GSX-Engine konnte nicht heruntergeladen werden.")
            })?;
        require(
            response.url().scheme() == "https"
                && response.content_length().is_none_or(|n| n == package.bytes),
            INVALID,
        )?;
        let mut output = File::from(rfs::openat(
            &folder,
            &temporary,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        let mut count = 0u64;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 128 * 1024];
        loop {
            tx::interrupted(cancel)?;
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count = count
                .checked_add(n as u64)
                .filter(|n| *n <= package.bytes)
                .ok_or(Error::Invalid(INVALID))?;
            hash.update(&buffer[..n]);
            output.write_all(&buffer[..n])?;
        }
        require(
            count == package.bytes && hex::encode(hash.finalize()) == package.sha256,
            "Die Prüfsumme der GSX-Engine stimmt nicht. Das aktive Profil wurde nicht geändert.",
        )?;
        tx::interrupted(cancel)?;
        output.sync_all()?;
        rfs::renameat(
            &folder,
            &temporary,
            &folder,
            target.file_name().ok_or(Error::Invalid(INVALID))?,
        )?;
        folder.sync_all()?;
        Ok(target.to_path_buf())
    })();
    let _ = rfs::unlinkat(&folder, &temporary, AtFlags::empty());
    result
}

/// Windows and Unix must interpret every component identically.
fn name(raw: &str) -> Result<String> {
    require(files::relative(raw) && raw.len() <= 1024, INVALID)?;
    let parts: Vec<_> = raw.split('/').collect();
    require(
        parts.len() <= 32
            && parts.iter().all(|part| {
                let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
                !part.ends_with(['.', ' '])
                    && !part.chars().any(char::is_control)
                    && !part.contains([':', '*', '?', '"', '<', '>', '|'])
                    && !["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
                    && !((base.starts_with("COM") || base.starts_with("LPT"))
                        && base.len() == 4
                        && matches!(base.as_bytes()[3], b'1'..=b'9'))
            }),
        INVALID,
    )?;
    Ok(raw.into())
}
fn register(paths: &mut BTreeMap<String, (String, bool)>, path: &str, dir: bool) -> Result<()> {
    let components: Vec<_> = path.split('/').collect();
    let mut current = String::new();
    for (index, part) in components.iter().enumerate() {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        let directory = index + 1 < components.len() || dir;
        let key = current.to_lowercase();
        if let Some((spelling, was_directory)) = paths.get(&key) {
            require(spelling == &current && *was_directory == directory, INVALID)?;
        } else {
            paths.insert(key, (current.clone(), directory));
        }
    }
    Ok(())
}
#[derive(Clone)]
struct Entry {
    index: usize,
    path: String,
    dir: bool,
    size: u64,
}
struct Plan {
    archive: ZipArchive<File>,
    verification: File,
    package: Package,
    entries: Vec<Entry>,
}

/// Traverse each directory through owned descriptors, never following links.
fn directory(root: &File, relative: &str, create: bool) -> Result<File> {
    let mut current = root.try_clone()?;
    if relative.is_empty() {
        return Ok(current);
    }
    name(relative)?;
    for part in relative.split('/') {
        if create {
            match rfs::mkdirat(&current, part, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                Err(error) => return Err(error.into()),
            }
        }
        current = files::open_at(&current, part, true, false)?;
    }
    Ok(current)
}
fn parent(root: &File, relative: &str, create: bool) -> Result<(File, String)> {
    name(relative)?;
    let (prefix, leaf) = relative.rsplit_once('/').unwrap_or(("", relative));
    Ok((directory(root, prefix, create)?, leaf.into()))
}
fn plan(
    package: &Package,
    source: File,
    paths: &mut BTreeMap<String, (String, bool)>,
    declared_files: &mut BTreeSet<String>,
) -> Result<Plan> {
    name(&package.name)?;
    name(&package.target)?;
    require(
        !package.name.contains('/')
            && package.bytes <= 256 * 1024 * 1024
            && package.bytes > 0
            && files::hex_digest(&package.sha256)
            && source.metadata()?.len() == package.bytes,
        INVALID,
    )?;
    require(
        files::digest(source.try_clone()?)? == package.sha256,
        INVALID,
    )?;
    let mut source = source;
    source.seek(SeekFrom::Start(0))?;
    let verification = source.try_clone()?;
    let mut archive = ZipArchive::new(source).map_err(|_| Error::Invalid(INVALID))?;
    require(
        archive.len() == package.entries && archive.len() <= MAX_ENTRIES,
        INVALID,
    )?;
    archive
        .validate_header_consistency()
        .map_err(|_| Error::Invalid(INVALID))?;
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| Error::Invalid(INVALID))?;
        let dir = file.is_dir();
        let archive_name = file.name().map_err(|_| Error::Invalid(INVALID))?;
        let raw = if dir {
            archive_name
                .strip_suffix('/')
                .ok_or(Error::Invalid(INVALID))?
        } else {
            &archive_name
        };
        let relative = name(raw)?;
        let kind = file.unix_mode().unwrap_or(0) & 0o170000;
        require(
            !file.encrypted()
                && !file.is_symlink()
                && file.get_metadata().external_attributes & 0x400 == 0
                && (kind == 0 || kind == if dir { 0o040000 } else { 0o100000 })
                && file.size() <= MAX_FILE
                && (!dir || file.size() == 0)
                && seen.insert(relative.clone()),
            INVALID,
        )?;
        let path = format!("{}/{relative}", package.target);
        require(dir || declared_files.insert(path.clone()), INVALID)?;
        register(paths, &path, dir)?;
        total = total
            .checked_add(file.size())
            .filter(|v| *v <= MAX_TOTAL)
            .ok_or(Error::Invalid(INVALID))?;
        entries.push(Entry {
            index,
            path,
            dir,
            size: file.size(),
        });
    }
    require(
        total == package.unpacked_bytes && !entries.is_empty(),
        INVALID,
    )?;
    Ok(Plan {
        archive,
        verification,
        package: package.clone(),
        entries,
    })
}

/// The caller supplies a cloned prefix's manager and a new private work path.
/// The injected fetcher is also checked by size and SHA-256, including in tests.
/// No manager file is changed until every archive has passed preflight and CRC.
pub fn install_with(
    manager: &Path,
    work: &Path,
    packages: &[Package],
    cancel: &AtomicBool,
    mut fetch: impl FnMut(&Package) -> Result<PathBuf>,
) -> Result<()> {
    tx::interrupted(cancel)?;
    let destination = files::directory(manager, false)?;
    require(
        !files::exists(work) && !packages.is_empty() && packages.len() <= 7,
        INVALID,
    )?;
    let mut paths = BTreeMap::new();
    let mut declared_files = BTreeSet::new();
    let mut plans = Vec::new();
    let mut total = 0u64;
    let mut count = 0usize;
    for package in packages {
        tx::interrupted(cancel)?;
        let path = fetch(package)?;
        tx::interrupted(cancel)?;
        let planned = plan(
            package,
            files::open_at(rfs::CWD, path, false, false)?,
            &mut paths,
            &mut declared_files,
        )?;
        total = total
            .checked_add(package.unpacked_bytes)
            .filter(|v| *v <= MAX_TOTAL)
            .ok_or(Error::Invalid(INVALID))?;
        count = count
            .checked_add(package.entries)
            .filter(|v| *v <= MAX_ENTRIES)
            .ok_or(Error::Invalid(INVALID))?;
        plans.push(planned);
    }
    // Validate existing destination components before creating extracted output.
    // Missing directories are fine; an existing link, file or foreign node is not.
    for (relative, is_dir) in paths.values() {
        let mut current = destination.try_clone()?;
        let parts: Vec<_> = relative.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            match rfs::statat(&current, *part, AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => break,
                Err(error) => return Err(error.into()),
                Ok(_) => {
                    current =
                        files::open_at(&current, *part, index + 1 < parts.len() || *is_dir, false)?;
                }
            }
        }
    }
    tx::interrupted(cancel)?;
    let work_parent = files::directory(work.parent().ok_or(Error::Invalid(INVALID))?, false)?;
    let work_name = work.file_name().ok_or(Error::Invalid(INVALID))?;
    rfs::mkdirat(&work_parent, work_name, Mode::from_raw_mode(0o700))?;
    let output = files::directory(work, true)?;
    let mut copied = Vec::new();
    for planned in &mut plans {
        for entry in &planned.entries {
            tx::interrupted(cancel)?;
            if entry.dir {
                directory(&output, &entry.path, true)?;
                continue;
            }
            let (folder, leaf) = parent(&output, &entry.path, true)?;
            let mut file = File::from(rfs::openat(
                &folder,
                &leaf,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            let mut member = planned
                .archive
                .by_index(entry.index)
                .map_err(|_| Error::Invalid(INVALID))?;
            let mut bytes = 0u64;
            let mut buffer = [0u8; 128 * 1024];
            loop {
                tx::interrupted(cancel)?;
                let n = member.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                bytes = bytes
                    .checked_add(n as u64)
                    .filter(|n| *n <= entry.size)
                    .ok_or(Error::Invalid(INVALID))?;
                file.write_all(&buffer[..n])?;
            }
            require(bytes == entry.size, INVALID)?;
            file.sync_all()?;
            folder.sync_all()?;
            copied.push(entry.path.clone());
        }
        let source = &mut planned.verification;
        source.seek(SeekFrom::Start(0))?;
        require(
            files::digest(source.try_clone()?)? == planned.package.sha256,
            INVALID,
        )?;
    }
    // Overlay only decoded regular files. Atomic replacement never truncates a
    // cloned hard link or follows a user-controlled destination symlink.
    for relative in copied {
        tx::interrupted(cancel)?;
        let mut source = files::beneath(&output, &relative)?;
        let (folder, leaf) = parent(&destination, &relative, true)?;
        let temporary = format!(".flightdeck-gsx-{}", uuid::Uuid::new_v4().simple());
        let result: Result<()> = (|| {
            let mut file = File::from(rfs::openat(
                &folder,
                &temporary,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            let mut buffer = [0u8; 128 * 1024];
            loop {
                tx::interrupted(cancel)?;
                let n = source.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])?;
            }
            file.sync_all()?;
            rfs::renameat(&folder, &temporary, &folder, &leaf)?;
            folder.sync_all()?;
            Ok(())
        })();
        let _ = rfs::unlinkat(&folder, &temporary, AtFlags::empty());
        result?;
    }
    destination.sync_all()?;
    Ok(())
}

pub struct Status {
    pub ready: bool,
    pub missing: Vec<String>,
}
pub fn status(manager: Option<&Path>, game: Game) -> Status {
    let mut required = vec![
        "couatl64/couatl64_boot.exe",
        "couatl64/python37.dll",
        "couatl64/python37.zip",
        "couatl64/wx/__init__.py",
        "couatl64/wx/_core.cp37-win_amd64.pyd",
        "couatl/common/__init__.pye",
        "couatl/GSX/__init__.pye",
        "couatl/GSX/airports/__init__.pye",
        "couatl/GSX/sounds/Truck_Engine1.wav",
    ];
    required.push(match game {
        Game::Msfs2024 => "couatl64/couatl64_MSFS2024.exe",
        Game::Msfs2020 => "couatl64/couatl64_MSFS.exe",
    });
    let folder = manager.and_then(|p| files::directory(p, false).ok());
    let missing: Vec<_> = required
        .into_iter()
        .filter(|path| {
            !folder.as_ref().is_some_and(|root| {
                files::beneath(root, path)
                    .and_then(|f| Ok(f.metadata()?.len()))
                    .is_ok_and(|len| len > 0 && len <= MAX_FILE)
            })
        })
        .map(str::to_string)
        .collect();
    Status {
        ready: missing.is_empty(),
        missing,
    }
}

/// A manifest alone is only an installer stub. Require the bridge, actual
/// models/textures, and every file declared by the bounded package layout.
pub fn product_present(package: &Path) -> bool {
    (|| -> Result<()> {
        let root = files::directory(&package.canonicalize()?, false)?;
        let manifest: serde_json::Value = files::json_at(&root, "manifest.json", 128 * 1024)?;
        require(
            manifest["title"].as_str().is_some_and(|s| !s.is_empty())
                && manifest["package_version"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
            INVALID,
        )?;
        let layout: serde_json::Value = files::json_at(&root, "layout.json", 32 * 1024 * 1024)?;
        let entries = layout["content"]
            .as_array()
            .ok_or(Error::Invalid(INVALID))?;
        require(!entries.is_empty() && entries.len() <= 100_000, INVALID)?;
        let mut seen = BTreeSet::new();
        let (mut bridge, mut model, mut texture) = (false, false, false);
        for entry in entries {
            let path = name(entry["path"].as_str().ok_or(Error::Invalid(INVALID))?)?;
            require(seen.insert(path.to_lowercase()), INVALID)?;
            let expected = entry["size"].as_u64().ok_or(Error::Invalid(INVALID))?;
            require(
                files::beneath(&root, &path)?.metadata()?.len() == expected,
                INVALID,
            )?;
            let lower = path.to_lowercase();
            if expected > 0 {
                bridge |= lower.rsplit('/').next() == Some("fsdt-msfs-bridge.wasm");
                model |= lower.starts_with("simobjects/") && lower.ends_with(".gltf");
                texture |= lower.starts_with("simobjects/")
                    && lower.contains("/texture")
                    && lower.ends_with(".dds");
            }
        }
        require(bridge && model && texture, INVALID)
    })()
    .is_ok()
}
