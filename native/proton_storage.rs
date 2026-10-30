// SPDX-License-Identifier: MIT
//! Reusable runner copies and conservative reclamation of obsolete profiles.
use crate::{
    Error, Result,
    backend::{Context, string},
    error::require,
    fenix, fenix_bundle, files, owned_tree, proton, runtime, transaction as tx, wine_processes,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const INVALID: &str =
    "Die gespeicherte Proton-Kopie wurde verändert. Sie wurde nicht überschrieben.";
const CLEANUP: &str = "private/proton-cleanup.json";
const LATEST: &str = "private/proton-storage.json";

// Metadata for every entry detects changes outside the small launch-file hash
// list too, including an in-place update to a codec or a symlink target.
fn stamp(path: &Path, cancel: &AtomicBool) -> Result<String> {
    fn walk(
        path: &Path,
        hash: &mut Sha256,
        count: &mut usize,
        depth: usize,
        cancel: &AtomicBool,
    ) -> Result<()> {
        require(depth < 256 && *count < 1_000_000, INVALID)?;
        tx::interrupted(cancel)?;
        *count += 1;
        let m = path.symlink_metadata()?;
        for n in [
            m.dev(),
            m.ino(),
            m.mode() as u64,
            m.len(),
            m.mtime() as u64,
            m.mtime_nsec() as u64,
            m.ctime() as u64,
            m.ctime_nsec() as u64,
        ] {
            hash.update(n.to_le_bytes());
        }
        if m.is_symlink() {
            let link = fs::read_link(path)?;
            hash.update(link.as_os_str().as_encoded_bytes());
        } else if m.is_dir() {
            let mut names = fs::read_dir(path)?
                .take(100_001)
                .map(|e| e.map(|e| e.file_name()))
                .collect::<std::io::Result<Vec<_>>>()?;
            require(names.len() <= 100_000, INVALID)?;
            names.sort();
            for name in names {
                let bytes = name.as_encoded_bytes();
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
                walk(&path.join(name), hash, count, depth + 1, cancel)?;
            }
        } else {
            require(m.is_file(), INVALID)?;
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    walk(path, &mut hash, &mut 0, 0, cancel)?;
    Ok(hex::encode(hash.finalize()))
}

pub(crate) struct Slot {
    pub work: PathBuf,
    pub reused: bool,
    source_stamp: String,
}
impl Slot {
    pub fn open(ctx: &Context, candidate: &Value, payload: Option<&Path>) -> Result<Self> {
        let root = ctx.root()?;
        let source_stamp = stamp(Path::new(string(candidate, "files")?), &ctx.cancel)?;
        let patch = payload
            .map(|p| fenix_bundle::payload_manifest(p, None))
            .transpose()?;
        let key = files::sha256(&serde_json::to_vec(
            &json!({"candidate":candidate,"stamp":source_stamp,"patch":patch}),
        )?);
        let tests = root.join("local/proton-tests");
        files::private_dir(&tests)?;
        proton::managed(root, &tests)?;
        let work = tests.join(&key[..32]);
        let marker = work.join("cache.json");
        if !files::exists(&work) {
            files::private_dir(&work)?;
            files::atomic_json(
                &marker,
                &json!({"schema":1,"app":"flightdeck-proton-cache","key":key,"id":fenix::identity(&work)?,"source":candidate,"profiles":{}}),
            )?;
        }
        proton::managed(root, &work)?;
        let record = runtime::value(&marker)?;
        require(
            record["schema"] == 1
                && record["app"] == "flightdeck-proton-cache"
                && record["key"] == key
                && record["id"] == fenix::identity(&work)?,
            INVALID,
        )?;
        let runner = work.join("runner");
        let reused = if files::exists(&runner) {
            if record["runner_stamp"].is_null() {
                require(
                    !protected(&runner, &pins(root, &known(ctx))?)
                        && !wine_processes::path_in_use(&runner)?,
                    INVALID,
                )?;
                reclaim(root, &runner, &ctx.cancel)?;
                false
            } else {
                require(
                    record["runner_stamp"].as_str() == Some(&stamp(&runner, &ctx.cancel)?),
                    INVALID,
                )?;
                true
            }
        } else {
            false
        };
        Ok(Self {
            work,
            reused,
            source_stamp,
        })
    }
    pub fn profile(&self) -> Result<PathBuf> {
        ["previous-prefix", "next-prefix"].into_iter().map(|name| self.work.join(name)).find(|p| !files::exists(p)).ok_or(Error::Invalid("Die bisherigen Proton-Profile werden noch benötigt. Beende laufende Windows-Anwendungen und prüfe die Sitzung, bevor du erneut wechselst."))
    }
    pub fn record_profile(&self, path: &Path) -> Result<()> {
        if files::exists(path) {
            record_profile(&self.work.join("cache.json"), path)?;
        }
        Ok(())
    }
    pub fn seal(&self, candidate: &Value, cancel: &AtomicBool) -> Result<()> {
        require(
            stamp(Path::new(string(candidate, "files")?), cancel)? == self.source_stamp,
            "Proton wurde während der Vorbereitung aktualisiert. Bitte erneut auswählen.",
        )?;
        let mut record = runtime::value(&self.work.join("cache.json"))?;
        record["runner_stamp"] = stamp(&self.work.join("runner"), cancel)?.into();
        files::atomic_json(&self.work.join("cache.json"), &record)
    }
}

fn record_profile(marker: &Path, path: &Path) -> Result<()> {
    let parent = path.parent().ok_or(Error::Invalid(INVALID))?;
    let mut record = runtime::value(marker)?;
    require(
        record["schema"] == 1
            && record["app"] == "flightdeck-proton-cache"
            && record["id"] == fenix::identity(parent)?,
        INVALID,
    )?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(Error::Invalid(INVALID))?;
    let id = owned_tree::identity(path)?;
    // Retention is not permission to delete. A foreign entry, nested mount,
    // special file or unreadable descendant makes this profile ineligible for
    // automatic reclamation, but must not strand an already published switch.
    let inventory = owned_tree::inventory(path, &AtomicBool::new(false)).ok();
    require(
        owned_tree::identity(path)? == id && inventory.as_ref().is_none_or(|v| v.id == id),
        INVALID,
    )?;
    record["profiles"][name] = json!(id);
    if !record["profile_fingerprints"].is_object() {
        record["profile_fingerprints"] = json!({});
    }
    // Clear any fingerprint left by the prepared profile before exchange.
    // A null fingerprint always preserves the backup, including on later runs.
    record["profile_fingerprints"][name] = json!(inventory.map(|v| v.fingerprint));
    files::atomic_json(marker, &record)
}

pub(crate) fn retain_backup(
    root: &Path,
    path: &Path,
    runner: &str,
    base: Option<&str>,
    active: &Path,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        let marker = parent.join("cache.json");
        if files::exists(&marker) {
            record_profile(&marker, path)?;
        }
    }
    files::atomic_json(
        &root.join(LATEST),
        &json!({"schema":1,"backup":path.strip_prefix(root).map_err(|_|Error::Invalid(INVALID))?,"id":fenix::identity(path)?,"previous_runner":runner,"base_runner":base,"active_runner":active}),
    )
}

fn known(ctx: &Context) -> Vec<PathBuf> {
    ctx.launcher
        .lock()
        .known
        .values()
        .filter(|p| Some(p.as_path()) != ctx.runtime.as_deref())
        .cloned()
        .collect()
}

fn pins(root: &Path, others: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    fn active_links(root: &Path, owner: &Path, result: &mut BTreeSet<PathBuf>) -> Result<()> {
        fn walk(
            root: &Path,
            path: &Path,
            result: &mut BTreeSet<PathBuf>,
            count: &mut usize,
            depth: usize,
        ) -> Result<()> {
            require(depth < 256 && *count < 1_000_000, INVALID)?;
            for entry in fs::read_dir(path)? {
                *count += 1;
                require(*count <= 1_000_000, INVALID)?;
                let path = entry?.path();
                let m = path.symlink_metadata()?;
                if m.is_symlink() {
                    let target = fs::read_link(&path)?;
                    let target = if target.is_absolute() {
                        target
                    } else {
                        path.parent().ok_or(Error::Invalid(INVALID))?.join(target)
                    };
                    // Resolve relative .. components before checking whether a
                    // live Community/package link points into an old backup.
                    let mut normalized = PathBuf::new();
                    for component in target.components() {
                        match component {
                            std::path::Component::ParentDir => {
                                normalized.pop();
                            }
                            std::path::Component::CurDir => {}
                            _ => normalized.push(component.as_os_str()),
                        }
                    }
                    if normalized.starts_with(root.join("local/proton-tests"))
                        || normalized.starts_with(root.join("local/proton-trash"))
                    {
                        result.insert(normalized);
                    }
                } else if m.is_dir() {
                    walk(root, &path, result, count, depth + 1)?;
                }
            }
            Ok(())
        }
        let prefix = owner.join("local/msfs-prefix");
        if files::exists(&prefix) {
            proton::managed(owner, &prefix)?;
            walk(root, &prefix, result, &mut 0, 0)?;
        }
        let (locations, limited) = crate::mods::locations(owner)?;
        require(!limited, INVALID)?;
        for (path, _) in locations {
            if path.starts_with(root.join("local/proton-tests"))
                || path.starts_with(root.join("local/proton-trash"))
            {
                result.insert(path);
            }
        }
        Ok(())
    }
    fn values(
        root: &Path,
        owner: &Path,
        value: &Value,
        key: &str,
        migrated: bool,
        result: &mut BTreeSet<PathBuf>,
    ) {
        match value {
            Value::Object(items) => {
                for (key, v) in items {
                    values(root, owner, v, key, value["migrated"] == true, result);
                }
            }
            Value::Array(items) => {
                for v in items {
                    values(root, owner, v, key, migrated, result);
                }
            }
            Value::String(text) => {
                let path = if text.starts_with("local/proton-tests/")
                    || text.starts_with("local/proton-trash/")
                {
                    owner.join(text)
                } else {
                    PathBuf::from(text)
                };
                let tests = path.strip_prefix(root.join("local/proton-tests")).ok();
                if let Some(relative) =
                    tests.or_else(|| path.strip_prefix(root.join("local/proton-trash")).ok())
                    && relative
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    // Installed Fenix work points at its runner. Its original
                    // pre-patch prefix/backup are independently recorded.
                    if tests.is_some()
                        && relative.components().count() == 1
                        && (key == "work" || key == "backup" && migrated)
                    {
                        result.insert(path.join("runner"));
                        result.insert(path.join("prefix"));
                    } else {
                        result.insert(path);
                    }
                }
            }
            _ => {}
        }
    }
    fn scan(
        root: &Path,
        owner: &Path,
        directory: &Path,
        depth: usize,
        count: &mut usize,
        result: &mut BTreeSet<PathBuf>,
    ) -> Result<()> {
        require(depth < 12, INVALID)?;
        for entry in fs::read_dir(directory)? {
            *count += 1;
            require(*count <= 4096, INVALID)?;
            let path = entry?.path();
            let m = path.symlink_metadata()?;
            if m.is_symlink()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("fenix-") && n.contains("backup"))
            {
                return Err(Error::Invalid(INVALID));
            }
            if m.is_dir()
                && !m.is_symlink()
                && (depth > 0
                    || path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("fenix-") && n.contains("backup")))
            {
                scan(root, owner, &path, depth + 1, count, result)?;
            } else if path.extension().is_some_and(|v| v == "json")
                && path.file_name().is_none_or(|n| n != "proton-cleanup.json")
            {
                require(
                    m.is_file() && !m.is_symlink() && m.len() <= 2 * 1024 * 1024,
                    INVALID,
                )?;
                values(root, owner, &runtime::value(&path)?, "", false, result);
            }
        }
        Ok(())
    }
    let mut result = BTreeSet::new();
    // A dangling runner link is still a live reference; never clean through it.
    let target = fs::read_link(root.join("runner"))?;
    result.insert(if target.is_absolute() {
        target
    } else {
        root.join(target)
    });
    scan(root, root, &root.join("private"), 0, &mut 0, &mut result)?;
    active_links(root, root, &mut result)?;
    for other in others {
        let target = fs::read_link(other.join("runner"))?;
        result.insert(if target.is_absolute() {
            target
        } else {
            other.join(target)
        });
        scan(root, other, &other.join("private"), 0, &mut 0, &mut result)?;
        active_links(root, other, &mut result)?;
    }
    Ok(result)
}

fn protected(path: &Path, pins: &BTreeSet<PathBuf>) -> bool {
    pins.iter()
        .any(|pin| pin.starts_with(path) || path.starts_with(pin))
}

fn reclaim(root: &Path, path: &Path, cancel: &AtomicBool) -> Result<u64> {
    require(!crate::wine_processes::path_in_use(path)?, INVALID)?;
    let before = owned_tree::inventory(path, cancel)?;
    let trash = root.join("local/proton-trash");
    files::private_dir(&trash)?;
    proton::managed(root, &trash)?;
    let target = trash.join(uuid::Uuid::new_v4().simple().to_string());
    files::atomic_json(
        &root.join(CLEANUP),
        &json!({"schema":1,"trash":target.strip_prefix(root).map_err(|_|Error::Invalid(INVALID))?,"id":before.id,"fingerprint":before.fingerprint}),
    )?;
    tx::publish(path, &target)?;
    let checked = owned_tree::inventory(&target, cancel)?;
    require(checked == before, INVALID)?;
    owned_tree::remove(&target, before.id)?;
    fs::remove_file(root.join(CLEANUP))?;
    Ok(before.bytes)
}

fn resume_cleanup(root: &Path, cancel: &AtomicBool, pins: &BTreeSet<PathBuf>) -> Result<()> {
    if !files::exists(&root.join(CLEANUP)) {
        return Ok(());
    }
    let record = runtime::value(&root.join(CLEANUP))?;
    let name = string(&record, "trash")?;
    require(
        record["schema"] == 1
            && crate::log_reader::regex(r"^local/proton-trash/[0-9a-f]{32}$").is_match(name),
        INVALID,
    )?;
    let path = root.join(name);
    if files::exists(&path) {
        require(
            !protected(&path, pins) && !wine_processes::path_in_use(&path)?,
            INVALID,
        )?;
        proton::managed(root, &path)?;
        let inventory = owned_tree::inventory(&path, cancel)?;
        require(
            json!(inventory.id) == record["id"] && inventory.fingerprint == record["fingerprint"],
            INVALID,
        )?;
        owned_tree::remove(&path, inventory.id)?;
    }
    fs::remove_file(root.join(CLEANUP))?;
    Ok(())
}

fn unchanged_since(path: &Path, cutoff: (i64, i64), cancel: &AtomicBool) -> Result<bool> {
    fn walk(
        path: &Path,
        cutoff: (i64, i64),
        count: &mut usize,
        depth: usize,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        require(depth < 256 && *count < 1_000_000, INVALID)?;
        for entry in fs::read_dir(path)? {
            tx::interrupted(cancel)?;
            *count += 1;
            require(*count <= 1_000_000, INVALID)?;
            let path = entry?.path();
            let m = path.symlink_metadata()?;
            if (m.ctime(), m.ctime_nsec()) > cutoff {
                return Ok(false);
            }
            if m.is_dir() && !m.is_symlink() && !walk(&path, cutoff, count, depth + 1, cancel)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    walk(path, cutoff, &mut 0, 0, cancel)
}

fn cleanup(root: &Path, cancel: &AtomicBool, others: &[PathBuf]) -> Result<Value> {
    require(
        !files::exists(&root.join("private/proton-switch.json")),
        INVALID,
    )?;
    // If the active profile is missing, an old trial can be the only remaining
    // recoverable copy of the user's data. Do not infer that it is disposable.
    proton::managed(root, &root.join("local/msfs-prefix"))?;
    require(root.join("local/msfs-prefix/system.reg").is_file(), INVALID)?;
    wine_processes::idle(&root.join("local/msfs-prefix"))?;
    let pins = pins(root, others)?;
    resume_cleanup(root, cancel, &pins)?;
    let tests = root.join("local/proton-tests");
    if !files::exists(&tests) {
        return Ok(json!({"removed":0,"bytes":0}));
    }
    proton::managed(root, &tests)?;
    let mut removed = 0;
    let mut bytes = 0_u64;
    for entry in fs::read_dir(&tests)?.take(4097) {
        tx::interrupted(cancel)?;
        let path = entry?.path();
        if !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            n.len() == 32
                && n.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            continue;
        }
        proton::managed(root, &path)?;
        let marker = path.join("cache.json");
        if files::exists(&marker) {
            let record = runtime::value(&marker)?;
            if record["schema"] != 1
                || record["app"] != "flightdeck-proton-cache"
                || record["id"] != fenix::identity(&path)?
            {
                continue;
            }
            for name in ["previous-prefix", "next-prefix"] {
                let profile = path.join(name);
                if !files::exists(&profile)
                    || protected(&profile, &pins)
                    || record["profiles"][name] != fenix::identity(&profile)?
                    || !record["profile_fingerprints"][name]
                        .as_str()
                        .is_some_and(files::hex_digest)
                {
                    continue;
                }
                wine_processes::idle(&profile)?;
                let inventory = owned_tree::inventory(&profile, cancel)?;
                if record["profile_fingerprints"][name].as_str()
                    != Some(inventory.fingerprint.as_str())
                {
                    continue;
                }
                bytes += reclaim(root, &profile, cancel)?;
                removed += 1;
            }
        } else if files::exists(&path.join("provenance.json")) {
            let record = runtime::value(&path.join("provenance.json"))?;
            let known = record["source"]
                .as_str()
                .is_some_and(|p| Path::new(p).is_absolute())
                && record["files"].as_object().is_some_and(|m| {
                    !m.is_empty()
                        && m.iter().all(|(k, v)| {
                            files::relative(k) && v.as_str().is_some_and(files::hex_digest)
                        })
                })
                && proton::inspect(
                    path.join("runner")
                        .to_str()
                        .ok_or(Error::Invalid(INVALID))?,
                )
                .is_ok_and(|v| v["hashes"] == record["files"]);
            let names = fs::read_dir(&path)?
                .map(|e| e.map(|e| e.file_name()))
                .collect::<std::io::Result<Vec<_>>>()?;
            if !known
                || !names.iter().all(|n| {
                    n.to_str().is_some_and(|n| {
                        matches!(
                            n,
                            "runner"
                                | "previous-prefix"
                                | "xdg"
                                | "provenance.json"
                                | "fenix-setup.log"
                                | "fenix-settings-before-switch"
                                | "launch-msfs.sh"
                                | "xodus-wine-launch"
                                | "before-launch-msfs.sh"
                                | "before-xodus-wine-launch"
                        )
                    })
                })
            {
                continue;
            }
            let provenance = path.join("provenance.json").symlink_metadata()?;
            if !unchanged_since(&path, (provenance.ctime(), provenance.ctime_nsec()), cancel)? {
                continue;
            }
            for name in ["previous-prefix", "xdg"] {
                let profile = path.join(name);
                if profile.is_dir() {
                    wine_processes::idle(&profile)?;
                }
            }
            if protected(&path, &pins) {
                let profile = path.join("previous-prefix");
                if files::exists(&profile) && !protected(&profile, &pins) {
                    bytes += reclaim(root, &profile, cancel)?;
                    removed += 1;
                }
            } else {
                bytes += reclaim(root, &path, cancel)?;
                removed += 1;
            }
        }
    }
    Ok(json!({"removed":removed,"bytes":bytes}))
}

/// Run once per selected runtime and service lifetime, under the normal
/// exclusive runtime lease. A failed or ambiguous audit defers deletion.
pub(crate) fn on_startup(app: &std::sync::Arc<crate::backend::Launcher>) -> Result<()> {
    let mut state = app.lock();
    let Some(root) = state.runtime.clone() else {
        return Ok(());
    };
    if state
        .jobs
        .get("proton-cleanup")
        .is_some_and(|job| job["runtime_path"].as_str() == root.to_str())
        || !root.join("local/proton-tests").is_dir()
        || files::exists(&root.join("private/proton-switch.json"))
        || crate::backend::Launcher::idle(&mut state).is_err()
        || ["syncing", "playing", "attention"].contains(
            &crate::backend::string(&crate::cloud_sync::automatic(&state), "state").unwrap_or(""),
        )
    {
        return Ok(());
    }
    let ctx = app.reserve_locked(&mut state, "proton-cleanup", "cleanup", true)?;
    drop(state);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cleanup(&root, &ctx.cancel, &known(&ctx))
        }))
        .ok()
        .and_then(Result::ok)
        .unwrap_or(json!({"skipped":true}));
        let report = json!({"state":"complete","checked_at":files::now(),"cleanup":result});
        let _ = files::atomic_json(&root.join("private/proton-cleanup-last.json"), &report);
        ctx.launcher.finish(&ctx, Ok(report), false);
    });
    Ok(())
}

pub(crate) fn cleanup_for(ctx: &Context) -> Result<Value> {
    cleanup(ctx.root()?, &ctx.cancel, &known(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_cleanup_reaudits_live_references_before_removing_quarantined_data() {
        for live in [false, true] {
            let temp = tempfile::tempdir().expect("runtime");
            let root = temp.path();
            let cancel = AtomicBool::new(false);
            files::private_dir(&root.join("private")).expect("private");
            files::private_dir(&root.join("local/msfs-prefix")).expect("profile");
            fs::write(root.join("local/msfs-prefix/system.reg"), b"registry").expect("registry");
            let original = root.join("original-runner");
            files::private_dir(&original).expect("runner");
            let trash = root.join("local/proton-trash/0123456789abcdef0123456789abcdef");
            files::private_dir(&trash.join("runner")).expect("quarantine");
            fs::write(trash.join("personal-data"), b"preserved data").expect("data");
            let inventory = owned_tree::inventory(&trash, &cancel).expect("reviewed tree");
            files::atomic_json(&root.join(CLEANUP),&json!({"schema":1,"trash":trash.strip_prefix(root).expect("relative"),"id":inventory.id,"fingerprint":inventory.fingerprint})).expect("journal");
            std::os::unix::fs::symlink(
                if live { trash.join("runner") } else { original },
                root.join("runner"),
            )
            .expect("active runner");
            let result = cleanup(root, &cancel, &[]);
            if live {
                assert!(result.is_err());
                assert!(trash.join("personal-data").is_file());
                assert!(root.join(CLEANUP).is_file());
            } else {
                result.expect("resume");
                assert!(!trash.exists());
                assert!(!root.join(CLEANUP).exists());
            }
        }
    }
}
