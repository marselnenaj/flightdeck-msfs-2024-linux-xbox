// SPDX-License-Identifier: MIT
//! Verify the sealed download index; never learn a baseline from installed files.
use crate::{Error, Result, error::require, files};
use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const UNAVAILABLE: &str = "Vollständiger Download-Prüfnachweis fehlt oder ist ungültig. Eine vollständige Reparatur ist erforderlich.";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Index {
    format: u32,
    source: String,
    package_sha256: String,
    files: Vec<Entry>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    length: u64,
    sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Package {
    format: u32,
    package_sha256: String,
}
struct Baseline {
    root: File,
    _journal: File,
    _lock: File,
    index: Index,
}
impl Baseline {
    fn open(game: &Path) -> Result<Self> {
        let root = files::directory(&game.canonicalize()?, false)?;
        let journal = files::open_at(&root, ".xodus-resume", true, true)?;
        let lock = files::open_at(&journal, "lock", false, false)?;
        flock(&lock, FlockOperation::NonBlockingLockShared)?;
        let index: Index = files::json_at(&journal, "integrity.json", 32 * 1024 * 1024)?;
        let package: Package = files::json_at(&journal, "package.json", 8192)?;
        require(
            index.format == 1
                && index.source == "xodus-completed-download-v1"
                && files::hex_digest(&index.package_sha256)
                && package.format == 1
                && package.package_sha256 == index.package_sha256
                && !index.files.is_empty()
                && index.files.len() <= 100001,
            UNAVAILABLE,
        )?;
        let mut seen = HashSet::new();
        for row in &index.files {
            require(
                files::relative(&row.name)
                    && (!row.name.starts_with(".xodus-") || row.name == ".xodus-streaming.msixvc")
                    && row.length < 8 * 1024u64.pow(4)
                    && files::hex_digest(&row.sha256)
                    && seen.insert(&row.name),
                UNAVAILABLE,
            )?;
        }
        require(
            index
                .files
                .iter()
                .any(|v| v.name == ".xodus-streaming.msixvc"),
            UNAVAILABLE,
        )?;
        Ok(Self {
            root,
            _journal: journal,
            _lock: lock,
            index,
        })
    }
}
#[derive(Clone, Default, Debug, Serialize)]
pub struct Report {
    pub checked: usize,
    pub missing: usize,
    pub changed: usize,
    pub unreadable: usize,
    pub total: usize,
    pub healthy: bool,
}
fn index_digest(index: &Index) -> Result<String> {
    // Value's maps are sorted recursively, matching the original canonical receipt.
    Ok(files::sha256(&serde_json::to_vec(&serde_json::to_value(
        index,
    )?)?))
}
pub fn record_installation(game: &Path, game_id: crate::games::Game) -> Result<()> {
    let baseline = Baseline::open(game)?;
    let configs: Vec<_> = baseline
        .index
        .files
        .iter()
        .filter(|row| ["MicrosoftGame.Config", "MicrosoftGame.config"].contains(&row.name.as_str()))
        .collect();
    require(
        configs.len() == 1 && configs[0].length <= 1024 * 1024,
        UNAVAILABLE,
    )?;
    let row = configs[0];
    let bytes = files::read_file(files::beneath(&baseline.root, &row.name)?, 1024 * 1024)?;
    require(
        bytes.len() as u64 == row.length && files::sha256(&bytes) == row.sha256,
        UNAVAILABLE,
    )?;
    let parsed = crate::game_package::parse(&bytes)?;
    require(parsed.store_id == game_id.store_id(), UNAVAILABLE)?;
    let identity = parsed.identity.ok_or(Error::Invalid(UNAVAILABLE))?;
    require(
        !identity.name.is_empty() && !identity.publisher.is_empty(),
        UNAVAILABLE,
    )?;
    crate::game_package::version(&identity.version)?;
    let mut value = serde_json::json!({"format":1,"index_sha256":index_digest(&baseline.index)?,"identity":identity});
    if game_id != crate::games::Game::Msfs2024 {
        value["format"] = serde_json::json!(2);
        value["game_id"] = serde_json::json!(game_id.id());
    }
    files::atomic_at(
        &baseline._journal,
        "installed.json",
        &serde_json::to_vec(&value)?,
    )
}
pub fn installed_identity(
    game: &Path,
    game_id: crate::games::Game,
) -> Result<crate::game_package::Identity> {
    match crate::game_package::installed(game, game_id) {
        Ok(identity) => Ok(identity),
        Err(original) => {
            let fallback = (|| {
                let baseline = Baseline::open(game)?;
                let saved: serde_json::Value =
                    files::json_at(&baseline._journal, "installed.json", 8192)?;
                let valid = saved
                    .as_object()
                    .is_some_and(|v| v.len() == if saved["format"] == 1 { 3 } else { 4 })
                    && ((saved["format"] == 1 && game_id == crate::games::Game::Msfs2024)
                        || (saved["format"] == 2 && saved["game_id"] == game_id.id()));
                require(
                    valid
                        && saved["index_sha256"] == index_digest(&baseline.index)?
                        && saved["identity"].as_object().is_some_and(|v| v.len() == 3),
                    UNAVAILABLE,
                )?;
                let identity: crate::game_package::Identity =
                    serde_json::from_value(saved["identity"].clone())?;
                require(
                    !identity.name.is_empty() && !identity.publisher.is_empty(),
                    UNAVAILABLE,
                )?;
                crate::game_package::version(&identity.version)?;
                Ok(identity)
            })();
            fallback.map_err(|_: Error| original)
        }
    }
}
pub fn available(game: &Path) -> bool {
    Baseline::open(game).is_ok()
}
pub fn verify(game: &Path, cancel: &AtomicBool, mut notify: impl FnMut(&Report)) -> Result<Report> {
    let baseline = Baseline::open(game).map_err(|_| Error::Invalid(UNAVAILABLE))?;
    let mut report = Report {
        total: baseline.index.files.len(),
        ..Default::default()
    };
    let mut buffer = vec![0u8; 1024 * 1024];
    for entry in &baseline.index.files {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let result = (|| -> Result<bool> {
            let mut file = files::beneath(&baseline.root, &entry.name)?;
            let before = file.metadata()?;
            if before.len() != entry.length {
                return Ok(false);
            }
            let mut hash = Sha256::new();
            let mut length = 0;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                length += n as u64;
                if length > entry.length {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            let after = file.metadata()?;
            let identity = |m: &std::fs::Metadata| {
                (
                    m.len(),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ctime(),
                    m.ctime_nsec(),
                )
            };
            Ok(length == entry.length
                && hex::encode(hash.finalize()) == entry.sha256
                && identity(&before) == identity(&after))
        })();
        match result {
            Ok(true) => {}
            Ok(false) => report.changed += 1,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => report.missing += 1,
            Err(_) => report.unreadable += 1,
        }
        report.checked += 1;
        notify(&report);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    report.healthy = report.missing + report.changed + report.unreadable == 0;
    Ok(report)
}
