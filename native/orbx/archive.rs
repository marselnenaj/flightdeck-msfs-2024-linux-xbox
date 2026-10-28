// SPDX-License-Identifier: MIT
//! Preflight an entire official archive before creating any staged output.
use crate::{Error, Result, error::require, files, transaction as tx};
use rustix::fs::{Mode, OFlags};
use sevenz_rust2::{ArchiveEntry, ArchiveReader, Password};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
const INVALID: &str = "Das Orbx-Programmpaket enthält ungültige Dateien.";
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 12_000;
type Entries = BTreeMap<String, (bool, u64)>;

fn name(raw: &str) -> Result<String> {
    let name = raw.replace('\\', "/");
    require(
        !name.is_empty()
            && name.len() <= 2048
            && !name.starts_with('/')
            && !name.chars().any(char::is_control),
        INVALID,
    )?;
    let parts: Vec<_> = name.split('/').collect();
    require(
        parts.len() <= 32
            && parts.iter().all(|p| {
                let base = p.split('.').next().unwrap_or("").to_ascii_uppercase();
                !p.is_empty()
                    && *p != "."
                    && *p != ".."
                    && !p.ends_with(['.', ' '])
                    && !p.contains([':', '*', '?', '"', '<', '>', '|'])
                    && !["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
                    && !((base.starts_with("COM") || base.starts_with("LPT"))
                        && base.len() == 4
                        && matches!(base.as_bytes()[3], b'1'..=b'9'))
            }),
        INVALID,
    )?;
    Ok(name)
}
fn plan(entries: &[ArchiveEntry]) -> Result<(Entries, u64)> {
    require(!entries.is_empty() && entries.len() <= MAX_ENTRIES, INVALID)?;
    let mut paths = BTreeMap::<String, (String, bool)>::new();
    let mut actual = BTreeMap::new();
    let mut total = 0u64;
    for entry in entries {
        let path = name(&entry.name)?;
        let kind = (entry.windows_attributes >> 16) & 0o170000;
        require(
            !entry.is_anti_item
                && entry.windows_attributes & 0x400 == 0
                && (kind == 0
                    || kind
                        == if entry.is_directory {
                            0o040000
                        } else {
                            0o100000
                        })
                && entry.size <= MAX_FILE
                && (!entry.is_directory || entry.size == 0)
                && (entry.has_stream || entry.size == 0),
            INVALID,
        )?;
        require(
            actual
                .insert(path.clone(), (entry.is_directory, entry.size))
                .is_none(),
            INVALID,
        )?;
        let mut current = String::new();
        let components: Vec<_> = path.split('/').collect();
        for (i, part) in components.iter().enumerate() {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(part);
            let dir = i + 1 < components.len() || entry.is_directory;
            let key = current.to_lowercase();
            if let Some((spelling, prior_dir)) = paths.get(&key) {
                require(spelling == &current && *prior_dir == dir, INVALID)?;
            } else {
                paths.insert(key, (current.clone(), dir));
            }
        }
        total = total
            .checked_add(entry.size)
            .filter(|v| *v <= MAX_TOTAL)
            .ok_or(Error::Invalid(INVALID))?;
    }
    Ok((actual, total))
}

/// Only caller-created private staging directories are accepted, never a live app.
pub(super) fn extract(
    archive: &Path,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<BTreeMap<String, String>> {
    tx::interrupted(cancel)?;
    let source = files::open_at(rustix::fs::CWD, archive, false, false)?;
    let mut reader =
        ArchiveReader::new(source, Password::empty()).map_err(|_| Error::Invalid(INVALID))?;
    reader.set_thread_count(2);
    let (expected, total) = plan(&reader.archive().files)?;
    require(!files::exists(destination), INVALID)?;
    files::private_dir(destination)?;
    let result = (|| {
        let mut hashes = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut received = 0u64;
        let decoded = reader.for_each_entries(|entry, input| {
            let copy = (|| -> Result<()> {
                tx::interrupted(cancel)?;
                let relative = name(&entry.name)?;
                require(
                    expected.get(&relative) == Some(&(entry.is_directory, entry.size))
                        && seen.insert(relative.clone()),
                    INVALID,
                )?;
                let output = destination.join(PathBuf::from(&relative));
                if entry.is_directory {
                    files::private_dir(&output)?;
                    return Ok(());
                }
                let parent = files::private_dir(output.parent().ok_or(Error::Invalid(INVALID))?)?;
                let mut file = File::from(rustix::fs::openat(
                    &parent,
                    output.file_name().ok_or(Error::Invalid(INVALID))?,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )?);
                let mut count = 0u64;
                let mut hash = Sha256::new();
                let mut buffer = [0u8; 128 * 1024];
                loop {
                    tx::interrupted(cancel)?;
                    let n = input.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    count = count
                        .checked_add(n as u64)
                        .filter(|n| *n <= entry.size)
                        .ok_or(Error::Invalid(INVALID))?;
                    file.write_all(&buffer[..n])?;
                    hash.update(&buffer[..n]);
                }
                require(count == entry.size, INVALID)?;
                received = received
                    .checked_add(count)
                    .filter(|n| *n <= total)
                    .ok_or(Error::Invalid(INVALID))?;
                file.sync_all()?;
                hashes.insert(relative, hex::encode(hash.finalize()));
                Ok(())
            })();
            copy.map(|_| true)
                .map_err(|_| sevenz_rust2::Error::Other("Orbx extraction stopped".into()))
        });
        tx::interrupted(cancel)?;
        require(
            decoded.is_ok() && received == total && seen.len() == expected.len(),
            INVALID,
        )?;
        files::directory(destination, false)?.sync_all()?;
        Ok(hashes)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str, dir: bool) -> ArchiveEntry {
        ArchiveEntry {
            name: path.into(),
            is_directory: dir,
            ..Default::default()
        }
    }
    #[test]
    fn paths_and_windows_aliases_are_validated_before_writes() {
        for path in [
            "../outside",
            "/absolute",
            "C:\\outside",
            "\\\\server\\share",
            "a/../b",
            "a//b",
            "a:stream",
            "a/b.",
            "a/b ",
            "CON.txt",
            "aux",
            "COM1.log",
            "a/\n",
        ] {
            assert!(name(path).is_err(), "{path:?}");
        }
        for entries in [
            vec![entry("A/file", false), entry("a/other", false)],
            vec![entry("a", false), entry("a/file", false)],
            vec![entry("same", false), entry("same", false)],
        ] {
            assert!(plan(&entries).is_err());
        }
        assert!(plan(&[entry("resources/app.asar", false), entry("resources", true)]).is_ok());
    }
    #[test]
    fn special_items_and_size_overflow_are_rejected() {
        let mut anti = entry("delete", false);
        anti.is_anti_item = true;
        let mut link = entry("link", false);
        link.windows_attributes = 0x400;
        let mut unix = entry("link", false);
        unix.windows_attributes = 0o120777 << 16;
        let mut huge = entry("huge", false);
        huge.size = MAX_FILE + 1;
        huge.has_stream = true;
        for bad in [anti, link, unix, huge] {
            assert!(plan(&[bad]).is_err());
        }
    }
    #[test]
    #[ignore = "requires the exact official pinned Orbx archive; no account or game"]
    fn official_archive_extracts_with_all_crcs_without_host_7zip() {
        let source =
            std::env::var_os("FLIGHTDECK_TEST_ORBX_ARCHIVE").expect("official archive path");
        let source = Path::new(&source);
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../compat/orbx.lock.json"))
                .expect("valid Orbx test fixture");
        assert_eq!(
            tx::digest(source).expect("valid Orbx test fixture"),
            pin["archive_sha256"]
        );
        let root = tempfile::tempdir().expect("valid Orbx test fixture");
        let target = root.path().join("program");
        let files =
            extract(source, &target, &AtomicBool::new(false)).expect("valid Orbx test fixture");
        for required in [
            "Orbx Central.exe",
            "resources/app.asar",
            "saturn-win/OrbxCentralWorker.exe",
        ] {
            assert!(files.contains_key(required));
        }
        assert!(files.len() > 100);
        assert!(target.join("locales").is_dir());
    }
}
