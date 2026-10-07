// SPDX-License-Identifier: MIT
//! Account-bound, revision-checked local imports, backups and export leases.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_fs as fs,
    cloud_storage::{Atom, Cache, CachedContainer, Manifest, Scope, Snapshot},
    files,
    save_state::{self, State},
};
use rustix::fs::{self as rfs, AtFlags, FlockOperation, Mode, OFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, atomic::AtomicBool},
};

pub const MAX_STATE: usize = save_state::QUOTA + save_state::METADATA_LIMIT + 32;
type NamespaceIdentity = (u64, u64, String);
static ACTIVE: OnceLock<Mutex<BTreeSet<NamespaceIdentity>>> = OnceLock::new();
fn active() -> &'static Mutex<BTreeSet<NamespaceIdentity>> {
    ACTIVE.get_or_init(|| Mutex::new(BTreeSet::new()))
}
fn encode(state: &State) -> Result<Vec<u8>> {
    save_state::encode(state).map_err(|_| Failure::new("unsupported"))
}
fn decode(raw: &[u8]) -> Result<State> {
    save_state::decode(raw).map_err(|_| Failure::new("unsupported"))
}
pub fn digest(state: &State) -> Result<String> {
    save_state::content_digest(state).map_err(|_| Failure::new("unsupported"))
}
pub fn raw_digest(raw: Option<&[u8]>) -> String {
    raw.map(files::sha256).unwrap_or_else(|| "missing".into())
}
pub fn counts(state: &State) -> (usize, usize, usize) {
    (
        state.containers.len(),
        state.containers.values().map(|c| c.blobs.len()).sum(),
        state
            .containers
            .values()
            .flat_map(|c| c.blobs.values())
            .map(Vec::len)
            .sum(),
    )
}

pub(crate) struct Local<'a> {
    runtime: PathBuf,
    pub(crate) root: File,
    pub(crate) private: File,
    saves: File,
    pub(crate) folder: Option<File>,
    writer: Option<File>,
    identity: Option<NamespaceIdentity>,
    namespace: String,
    lease: Option<&'a File>,
}
impl Drop for Local<'_> {
    fn drop(&mut self) {
        // POSIX locks belong to the process. Close before permitting another
        // thread to open this inode; a late close would release its new lock.
        if let Ok(mut active) = active().lock() {
            drop(self.writer.take());
            if let Some(key) = &self.identity {
                active.remove(key);
            }
        }
    }
}
impl<'a> Local<'a> {
    pub(crate) fn open(
        runtime: &Path,
        namespace: &str,
        lease: Option<&'a File>,
        create: bool,
    ) -> Result<Self> {
        require(files::hex_digest(namespace), "invalid_scope")?;
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        require(
            fs::optional(&private, "local-saves.enabled", 4096)?.is_some(),
            "unsupported",
        )?;
        if let Some(lease) = lease {
            fs::lease(&private, lease)?;
        } else {
            require(!create, "invalid_lock")?;
        }
        let saves = fs::child(&private, "local-saves", false)?;
        let m = saves.metadata()?;
        let key = (m.dev(), m.ino(), namespace.to_owned());
        let mut active = active().lock().map_err(|_| Failure::new("busy"))?;
        require(!active.contains(&key), "busy")?;
        active.insert(key.clone());
        let mut local = Self {
            runtime: runtime.into(),
            root,
            private,
            saves,
            folder: None,
            writer: None,
            identity: Some(key),
            namespace: namespace.into(),
            lease,
        };
        // Release the bookkeeping mutex before fallible work, so Drop can
        // remove the reservation on every failure path.
        drop(active);
        if !create
            && matches!(
                rfs::statat(&local.saves, namespace, AtFlags::SYMLINK_NOFOLLOW),
                Err(rustix::io::Errno::NOENT)
            )
        {
            return Ok(local);
        }
        let folder = fs::child(&local.saves, namespace, create)?;
        let lock = File::from(rfs::openat(
            &folder,
            "writer.lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        let info = lock.metadata()?;
        require(
            info.is_file()
                && info.uid() == files::uid()
                && info.nlink() == 1
                && info.mode() & 0o077 == 0,
            "local_storage",
        )?;
        local.writer = Some(lock);
        rfs::fcntl_lock(
            local
                .writer
                .as_ref()
                .ok_or_else(|| Failure::new("local_storage"))?,
            FlockOperation::NonBlockingLockExclusive,
        )
        .map_err(|_| Failure::new("busy"))?;
        local.folder = Some(folder);
        local.recheck()?;
        Ok(local)
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        if let Some(lease) = self.lease {
            fs::lease(&self.private, lease)?;
        }
        fs::same(&self.runtime, &self.root, false)?;
        fs::linked(&self.root, "private", &self.private)?;
        fs::linked(&self.private, "local-saves", &self.saves)?;
        if let Some(folder) = &self.folder {
            fs::linked(&self.saves, &self.namespace, folder)?;
        }
        Ok(())
    }
    pub(crate) fn state(&self) -> Result<(Option<Vec<u8>>, State)> {
        let raw = if let Some(folder) = &self.folder {
            fs::optional(folder, "state.bin", MAX_STATE)?
        } else {
            None
        };
        let state = raw.as_deref().map(decode).transpose()?.unwrap_or_default();
        Ok((raw, state))
    }
    fn commit(&self, previous: &str, encoded: &[u8], cancel: &AtomicBool) -> Result<bool> {
        let folder = self
            .folder
            .as_ref()
            .ok_or_else(|| Failure::new("local_storage"))?;
        let name = format!(".cloud-import-{}", uuid::Uuid::new_v4().simple());
        let result = (|| {
            fs::write(folder, &name, encoded, 0o600)?;
            cloud::check(cancel)?;
            let (fresh, _) = self.state()?;
            require(raw_digest(fresh.as_deref()) == previous, "changed")?;
            require(self.lease.is_some(), "invalid_lock")?;
            self.recheck()?;
            rfs::renameat(folder, &name, folder, "state.bin")?;
            Ok(folder.sync_all().is_ok())
        })();
        let _ = fs::unlink(folder, &name, false);
        result
    }
}

pub(crate) struct Validated {
    pub state: State,
    pub digest: String,
    pub cache: Cache,
}
pub(crate) fn snapshot(runtime: &Path, scope: &Scope, snapshot: &Snapshot) -> Result<Validated> {
    let validate = || -> Result<Validated> {
        require(
            snapshot.scope_binding == scope.binding()
                && snapshot.consistency == "rechecked-unlocked",
            "invalid_scope",
        )?;
        require(
            snapshot.path.parent() == Some(runtime.join("private/cloud-saves").as_path())
                && snapshot
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| cloud::identifier(s, "snapshot-")),
            "invalid_snapshot",
        )?;
        let root = fs::open(&snapshot.path, true)?;
        let raw = fs::read(&root, "manifest.json", save_state::METADATA_LIMIT)?;
        let manifest: Manifest = cloud::decode(&raw)?;
        require(
            manifest.schema == 1
                && manifest.scope_binding == scope.binding()
                && manifest.consistency == "rechecked-unlocked"
                && manifest.containers.len() <= save_state::CONTAINER_LIMIT,
            "invalid_snapshot",
        )?;
        let blobs = fs::child(&root, "blobs", false)?;
        let mut state = State::default();
        let mut count = 0;
        let mut total = 0;
        let mut cache = Cache {
            binding: scope.binding(),
            containers: BTreeMap::new(),
        };
        for row in manifest.containers {
            let name = row
                .name
                .strip_suffix(",savedgame")
                .ok_or_else(|| Failure::new("invalid_snapshot"))?;
            save_state::name(name, true).map_err(|_| Failure::new("invalid_snapshot"))?;
            require(
                !state.containers.contains_key(name)
                    && !row.etag.is_empty()
                    && row.etag.len() <= 1024
                    && row.size <= save_state::QUOTA
                    && row.blobs.len() <= save_state::BLOB_LIMIT - count,
                "invalid_snapshot",
            )?;
            let modified = (row.client_file_time / 10_000_000)
                .checked_sub(11_644_473_600)
                .ok_or_else(|| Failure::new("unsupported"))?;
            let mut entry = save_state::Container {
                display_name: row.display_name.clone(),
                modified,
                blobs: BTreeMap::new(),
            };
            let mut cached = CachedContainer {
                metadata: row.metadata(),
                blobs: Vec::new(),
            };
            let mut atoms = BTreeSet::new();
            let mut size = 0;
            for blob in row.blobs {
                require(
                    blob.size <= 64 * 1024 * 1024
                        && files::hex_digest(&blob.sha256)
                        && blob.file == format!("blobs/{count:08x}.bin")
                        && cloud::guid(&blob.atom),
                    "invalid_snapshot",
                )?;
                save_state::name(&blob.name, false)
                    .map_err(|_| Failure::new("invalid_snapshot"))?;
                require(
                    !entry.blobs.contains_key(&blob.name)
                        && atoms.insert(blob.atom.to_ascii_lowercase()),
                    "invalid_snapshot",
                )?;
                let payload = fs::read(&blobs, &format!("{count:08x}.bin"), blob.size)?;
                require(
                    payload.len() == blob.size && files::sha256(&payload) == blob.sha256,
                    "invalid_snapshot",
                )?;
                count += 1;
                total += payload.len();
                size += payload.len();
                require(
                    count <= save_state::BLOB_LIMIT && total <= save_state::QUOTA,
                    "unsupported",
                )?;
                cached.blobs.push((
                    Atom {
                        atom: blob.atom,
                        name: blob.name.clone(),
                        size: Some(blob.size),
                    },
                    payload.clone(),
                ));
                entry.blobs.insert(blob.name, payload);
            }
            require(size == row.size, "invalid_snapshot")?;
            cached.blobs.sort_by(|a, b| a.0.name.cmp(&b.0.name));
            state.containers.insert(name.into(), entry);
            cache.containers.insert(row.name, cached);
        }
        require(
            (
                snapshot.container_count,
                snapshot.blob_count,
                snapshot.total_bytes,
            ) == (state.containers.len(), count, total)
                && fs::names(&root)? == ["manifest.json".into(), "blobs".into()].into()
                && fs::names(&blobs)? == (0..count).map(|n| format!("{n:08x}.bin")).collect(),
            "invalid_snapshot",
        )?;
        encode(&state)?;
        fs::same(&snapshot.path, &root, true)?;
        fs::linked(&root, "blobs", &blobs)?;
        Ok(Validated {
            state,
            digest: files::sha256(&raw),
            cache,
        })
    };
    validate().map_err(|e| {
        if matches!(e.code, "invalid_scope" | "unsupported") {
            e
        } else {
            Failure::new("invalid_snapshot")
        }
    })
}
pub fn read_snapshot(runtime: &Path, scope: &Scope, value: &Snapshot) -> Result<State> {
    Ok(snapshot(runtime, scope, value)?.state)
}
fn containers(state: &State) -> Result<BTreeMap<String, String>> {
    state
        .containers
        .iter()
        .map(|(name, value)| {
            Ok((
                name.clone(),
                digest(&State {
                    generation: 0,
                    containers: [(name.clone(), value.clone())].into(),
                })?,
            ))
        })
        .collect()
}
#[derive(Clone)]
pub struct Baseline {
    binding: String,
    containers: BTreeMap<String, String>,
}
impl Baseline {
    pub fn save_set(&self) -> Result<crate::cloud_policy::SaveSet> {
        Ok(crate::cloud_policy::SaveSet::new(
            &self.binding,
            self.containers.clone(),
        )?)
    }
}
fn baseline(binding: &str, state: &State) -> Result<Baseline> {
    Ok(Baseline {
        binding: binding.into(),
        containers: containers(state)?,
    })
}
#[derive(Clone)]
pub struct Plan {
    pub(crate) binding: String,
    pub(crate) namespace: String,
    pub(crate) local_digest: String,
    pub(crate) remote_digest: String,
    snapshot_digest: String,
    generation: u64,
    decisions: Vec<Decision>,
    runtime: PathBuf,
    pub(crate) snapshot: Snapshot,
    baseline: Option<Baseline>,
    counts: (usize, usize, usize, usize),
}
#[derive(Clone)]
struct Decision {
    name: String,
    action: &'static str,
    local: bool,
    remote: bool,
}
impl Plan {
    pub fn summary(&self) -> Value {
        json!({"local_exists":self.local_digest!="missing","container_count":self.counts.0,
            "blob_count":self.counts.1,"total_bytes":self.counts.2,"local_container_count":self.counts.3,
            "add_count":self.decisions.iter().filter(|d|!d.local&&d.remote).count(),
            "replace_count":self.decisions.iter().filter(|d|d.action!="unchanged"&&d.local&&d.remote).count(),
            "delete_count":self.decisions.iter().filter(|d|d.local&&!d.remote).count(),
            "unchanged_count":self.decisions.iter().filter(|d|d.action=="unchanged").count(),
            "conflict_count":self.decisions.iter().filter(|d|d.action=="conflict").count()})
    }
}
pub fn prepare(
    runtime: &Path,
    scope: &Scope,
    snap: &Snapshot,
    baseline: Option<&Baseline>,
) -> Result<Plan> {
    let binding = scope.binding();
    let namespace = scope.namespace()?;
    require(
        baseline.is_none_or(|b| b.binding == binding),
        "invalid_scope",
    )?;
    let remote = snapshot(runtime, scope, snap)?;
    let local = Local::open(runtime, &namespace, None, false)?;
    let (raw, state) = local.state()?;
    let left = containers(&state)?;
    let right = containers(&remote.state)?;
    let old = baseline.map(|b| &b.containers);
    let names: BTreeSet<_> = left
        .keys()
        .chain(right.keys())
        .chain(old.into_iter().flat_map(|o| o.keys()))
        .cloned()
        .collect();
    let decisions = names
        .into_iter()
        .map(|name| {
            let a = left.get(&name);
            let b = right.get(&name);
            let before = old.and_then(|o| o.get(&name));
            let action = if a == b {
                "unchanged"
            } else if (old.is_some() && a == before) || (old.is_none() && a.is_none()) {
                "cloud"
            } else if old.is_some() && b == before {
                "local"
            } else {
                "conflict"
            };
            Decision {
                name,
                action,
                local: a.is_some(),
                remote: b.is_some(),
            }
        })
        .collect();
    Ok(Plan {
        binding,
        namespace,
        local_digest: raw_digest(raw.as_deref()),
        remote_digest: digest(&remote.state)?,
        snapshot_digest: remote.digest,
        generation: state.generation,
        decisions,
        runtime: runtime.into(),
        snapshot: snap.clone(),
        baseline: baseline.cloned(),
        counts: (
            snap.container_count,
            snap.blob_count,
            snap.total_bytes,
            state.containers.len(),
        ),
    })
}
#[derive(Clone, Default)]
pub enum Choice {
    #[default]
    Automatic,
    Cloud,
    Local,
    Conflicts(BTreeMap<String, bool>),
}
fn select(local: &State, remote: &State, plan: &Plan, choice: &Choice) -> Result<State> {
    match choice {
        Choice::Cloud => {
            return Ok(State {
                generation: local.generation,
                containers: remote.containers.clone(),
            });
        }
        Choice::Local => return Ok(local.clone()),
        _ => (),
    }
    let empty = BTreeMap::new();
    let choices = if let Choice::Conflicts(c) = choice {
        c
    } else {
        &empty
    };
    let conflicts: BTreeSet<_> = plan
        .decisions
        .iter()
        .filter(|d| d.action == "conflict")
        .map(|d| &d.name)
        .collect();
    require(
        choices.keys().collect::<BTreeSet<_>>() == conflicts,
        "conflict",
    )?;
    let mut selected = State {
        generation: local.generation,
        containers: BTreeMap::new(),
    };
    for d in &plan.decisions {
        let from_cloud = choices.get(&d.name).copied().unwrap_or(d.action == "cloud");
        if let Some(entry) = (if from_cloud { remote } else { local })
            .containers
            .get(&d.name)
        {
            selected.containers.insert(d.name.clone(), entry.clone());
        }
    }
    Ok(selected)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackupMetadata {
    pub schema: u32,
    pub scope_binding: String,
    pub namespace: String,
    pub existed: bool,
    pub state_sha256: Option<String>,
    pub generation: u64,
    pub replacement_sha256: String,
    pub snapshot_sha256: Option<String>,
}
pub(crate) fn backup(
    private: &File,
    binding: &str,
    namespace: &str,
    raw: Option<&[u8]>,
    replacement: &[u8],
    snapshot_digest: Option<&str>,
) -> Result<String> {
    let parent = fs::child(private, "cloud-import-backups", true)?;
    let name = format!("backup-{}", uuid::Uuid::new_v4().simple());
    let temp = format!(".backup-{}", uuid::Uuid::new_v4().simple());
    rfs::mkdirat(&parent, &temp, Mode::from_raw_mode(0o700))?;
    let folder = fs::child(&parent, &temp, false)?;
    let mut published = false;
    let result = (|| {
        let generation = raw.map(decode).transpose()?.unwrap_or_default().generation;
        let metadata = BackupMetadata {
            schema: 1,
            scope_binding: binding.into(),
            namespace: namespace.into(),
            existed: raw.is_some(),
            state_sha256: raw.map(files::sha256),
            generation,
            replacement_sha256: files::sha256(replacement),
            snapshot_sha256: snapshot_digest.map(str::to_owned),
        };
        if let Some(raw) = raw {
            fs::write(&folder, "state.bin", raw, 0o400)?;
        }
        fs::write(&folder, "imported.bin", replacement, 0o400)?;
        fs::write(
            &folder,
            "manifest.json",
            &serde_json::to_vec(&metadata)?,
            0o400,
        )?;
        rfs::fchmod(&folder, Mode::from_raw_mode(0o500))?;
        folder.sync_all()?;
        rfs::renameat_with(&parent, &temp, &parent, &name, rfs::RenameFlags::NOREPLACE)?;
        published = true;
        parent.sync_all()?;
        Ok(name)
    })();
    if !published {
        let _ = rfs::fchmod(&folder, Mode::from_raw_mode(0o700));
        for name in ["state.bin", "imported.bin", "manifest.json"] {
            let _ = fs::unlink(&folder, name, false);
        }
        let _ = fs::unlink(&parent, &temp, true);
    }
    result
}
pub(crate) struct Backup {
    pub metadata: BackupMetadata,
    pub original: Option<Vec<u8>>,
    pub imported: Vec<u8>,
}
pub(crate) fn backup_data(
    private: &File,
    binding: &str,
    namespace: &str,
    id: &str,
) -> Result<Backup> {
    require(cloud::identifier(id, "backup-"), "invalid_plan")?;
    let parent = fs::child(private, "cloud-import-backups", false)?;
    let folder = fs::child(&parent, id, false)?;
    let metadata: BackupMetadata = cloud::decode(&fs::read(&folder, "manifest.json", 16384)?)?;
    require(
        metadata.schema == 1
            && metadata.scope_binding == binding
            && metadata.namespace == namespace
            && files::hex_digest(&metadata.replacement_sha256)
            && metadata
                .snapshot_sha256
                .as_deref()
                .is_none_or(files::hex_digest),
        "invalid_plan",
    )?;
    let mut expected: BTreeSet<String> = ["manifest.json".into(), "imported.bin".into()].into();
    if metadata.existed {
        expected.insert("state.bin".into());
    }
    require(fs::names(&folder)? == expected, "invalid_plan")?;
    let original = if metadata.existed {
        Some(fs::read(&folder, "state.bin", MAX_STATE)?)
    } else {
        None
    };
    let imported = fs::read(&folder, "imported.bin", MAX_STATE)?;
    require(
        original.as_deref().map(files::sha256) == metadata.state_sha256
            && files::sha256(&imported) == metadata.replacement_sha256
            && original
                .as_deref()
                .map(decode)
                .transpose()?
                .unwrap_or_default()
                .generation
                == metadata.generation,
        "invalid_plan",
    )?;
    decode(&imported)?;
    Ok(Backup {
        metadata,
        original,
        imported,
    })
}
fn publish_common(
    private: &File,
    folder: &File,
    binding: &str,
    namespace: &str,
    id: &str,
    encoded: &[u8],
    kind: &str,
) -> Result<()> {
    let receipts = fs::child(private, "cloud-import-receipts", true)?;
    let value = json!({"schema":1,"kind":kind,"scope_binding":binding,"namespace":namespace,"backup_id":id,"imported_sha256":files::sha256(encoded),"content_digest":digest(&decode(encoded)?)?});
    let data = serde_json::to_vec(&value)?;
    let name = format!("{id}.json");
    let temp = format!(".receipt-{}", uuid::Uuid::new_v4().simple());
    let result = (|| {
        fs::write(&receipts, &temp, &data, 0o400)?;
        fs::publish(&receipts, &temp, &name)?;
        files::atomic_at(
            folder,
            "cloud-baseline.json",
            &serde_json::to_vec(
                &json!({"schema":1,"scope_binding":binding,"namespace":namespace,"receipt":name,"sha256":files::sha256(&data)}),
            )?,
        )?;
        Ok(())
    })();
    let _ = fs::unlink(&receipts, &temp, false);
    result
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema: u32,
    scope_binding: String,
    namespace: String,
    receipt: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommonReceipt {
    schema: u32,
    kind: String,
    scope_binding: String,
    namespace: String,
    backup_id: String,
    imported_sha256: String,
    content_digest: String,
}
pub fn load_baseline(runtime: &Path, scope: &Scope) -> Result<Option<Baseline>> {
    let load = || -> Result<Option<Baseline>> {
        let binding = scope.binding();
        let namespace = scope.namespace()?;
        let local = Local::open(runtime, &namespace, None, false)?;
        let Some(folder) = &local.folder else {
            return Ok(None);
        };
        let Some(data) = fs::optional(folder, "cloud-baseline.json", 16384)? else {
            return Ok(None);
        };
        let pointer: Pointer = cloud::decode(&data)?;
        require(
            pointer.schema == 1
                && pointer.scope_binding == binding
                && pointer.namespace == namespace
                && pointer
                    .receipt
                    .strip_suffix(".json")
                    .is_some_and(|v| cloud::identifier(v, "backup-")),
            "invalid_plan",
        )?;
        let receipts = fs::child(&local.private, "cloud-import-receipts", false)?;
        let encoded = fs::read(&receipts, &pointer.receipt, 16384)?;
        require(files::sha256(&encoded) == pointer.sha256, "invalid_plan")?;
        let receipt: CommonReceipt = cloud::decode(&encoded)?;
        require(
            receipt.schema == 1
                && matches!(receipt.kind.as_str(), "import" | "sync")
                && receipt.scope_binding == binding
                && receipt.namespace == namespace
                && format!("{}.json", receipt.backup_id) == pointer.receipt,
            "invalid_plan",
        )?;
        let backup = backup_data(&local.private, &binding, &namespace, &receipt.backup_id)?;
        let state = decode(&backup.imported)?;
        require(
            receipt.imported_sha256 == backup.metadata.replacement_sha256
                && receipt.content_digest == digest(&state)?,
            "invalid_plan",
        )?;
        Ok(Some(baseline(&binding, &state)?))
    };
    load().map_err(|_| Failure::new("invalid_plan"))
}
pub struct ImportResult {
    pub imported: bool,
    pub backup_id: Option<String>,
    pub generation: u64,
    pub container_count: usize,
    pub blob_count: usize,
    pub total_bytes: usize,
    pub durability_confirmed: bool,
    pub baseline: Option<Baseline>,
}
impl ImportResult {
    pub fn summary(&self) -> Value {
        json!({"imported":self.imported,"backup_id":self.backup_id,"generation":self.generation,"container_count":self.container_count,"blob_count":self.blob_count,"total_bytes":self.total_bytes,"durability_confirmed":self.durability_confirmed})
    }
}
pub fn apply(
    runtime: &Path,
    scope: &Scope,
    plan: &Plan,
    choice: &Choice,
    lease: &File,
    cancel: &AtomicBool,
) -> Result<ImportResult> {
    let binding = scope.binding();
    let namespace = scope.namespace()?;
    require(
        plan.runtime == runtime && plan.binding == binding && plan.namespace == namespace,
        "invalid_scope",
    )?;
    cloud::check(cancel)?;
    let remote = snapshot(runtime, scope, &plan.snapshot)?;
    require(
        remote.digest == plan.snapshot_digest && digest(&remote.state)? == plan.remote_digest,
        "changed",
    )?;
    let local = Local::open(runtime, &namespace, Some(lease), true)?;
    let (raw, state) = local.state()?;
    require(
        raw_digest(raw.as_deref()) == plan.local_digest && state.generation == plan.generation,
        "changed",
    )?;
    let mut selected = select(&state, &remote.state, plan, choice)?;
    let (c, b, t) = counts(&selected);
    if digest(&selected)? == digest(&state)? {
        return Ok(ImportResult {
            imported: false,
            backup_id: None,
            generation: state.generation,
            container_count: c,
            blob_count: b,
            total_bytes: t,
            durability_confirmed: true,
            baseline: plan.baseline.clone(),
        });
    }
    selected.generation = state
        .generation
        .checked_add(1)
        .ok_or_else(|| Failure::new("unsupported"))?;
    let encoded = encode(&selected)?;
    cloud::check(cancel)?;
    let id = backup(
        &local.private,
        &binding,
        &namespace,
        raw.as_deref(),
        &encoded,
        Some(&plan.snapshot_digest),
    )?;
    let durable = local.commit(&plan.local_digest, &encoded, cancel)?;
    let mut result = ImportResult {
        imported: true,
        backup_id: Some(id.clone()),
        generation: selected.generation,
        container_count: c,
        blob_count: b,
        total_bytes: t,
        durability_confirmed: durable,
        baseline: None,
    };
    if digest(&selected)? == plan.remote_digest
        && let Some(folder) = &local.folder
    {
        match publish_common(
            &local.private,
            folder,
            &binding,
            &namespace,
            &id,
            &encoded,
            "import",
        ) {
            Ok(()) => result.baseline = Some(baseline(&binding, &selected)?),
            Err(_) => result.durability_confirmed = false,
        }
    }
    Ok(result)
}
pub fn restore(
    runtime: &Path,
    scope: &Scope,
    id: &str,
    lease: &File,
    cancel: &AtomicBool,
) -> Result<ImportResult> {
    let binding = scope.binding();
    let namespace = scope.namespace()?;
    cloud::check(cancel)?;
    let local = Local::open(runtime, &namespace, Some(lease), true)?;
    let old = backup_data(&local.private, &binding, &namespace, id)?;
    let (raw, state) = local.state()?;
    require(
        raw.is_some() && raw_digest(raw.as_deref()) == old.metadata.replacement_sha256,
        "changed",
    )?;
    let mut restored = old
        .original
        .as_deref()
        .map(decode)
        .transpose()?
        .unwrap_or_default();
    restored.generation = state
        .generation
        .checked_add(1)
        .ok_or_else(|| Failure::new("unsupported"))?;
    let encoded = encode(&restored)?;
    let backup = backup(
        &local.private,
        &binding,
        &namespace,
        raw.as_deref(),
        &encoded,
        None,
    )?;
    let durable = local.commit(&raw_digest(raw.as_deref()), &encoded, cancel)?;
    let (c, b, t) = counts(&restored);
    Ok(ImportResult {
        imported: true,
        backup_id: Some(backup),
        generation: restored.generation,
        container_count: c,
        blob_count: b,
        total_bytes: t,
        durability_confirmed: durable,
        baseline: None,
    })
}
pub struct LocalExport<'a> {
    pub(crate) binding: String,
    pub(crate) namespace: String,
    pub(crate) raw_digest: String,
    pub(crate) content_digest: String,
    pub(crate) generation: u64,
    pub(crate) encoded: Vec<u8>,
    pub(crate) local: Local<'a>,
}
impl LocalExport<'_> {
    pub(crate) fn runtime(&self) -> &Path {
        &self.local.runtime
    }
    pub(crate) fn lease(&self) -> Result<&File> {
        self.local.lease.ok_or_else(|| Failure::new("invalid_lock"))
    }
    pub fn state(&self) -> Result<State> {
        decode(&self.encoded)
    }
    pub fn assert_unchanged(&self) -> Result<()> {
        self.local.recheck()?;
        let (raw, _) = self.local.state()?;
        require(raw_digest(raw.as_deref()) == self.raw_digest, "changed")
    }
}
pub fn export_local<'a>(
    runtime: &Path,
    scope: &Scope,
    lease: &'a File,
    cancel: &AtomicBool,
) -> Result<LocalExport<'a>> {
    cloud::check(cancel)?;
    let namespace = scope.namespace()?;
    let local = Local::open(runtime, &namespace, Some(lease), true)?;
    let (raw, state) = local.state()?;
    Ok(LocalExport {
        binding: scope.binding(),
        namespace,
        raw_digest: raw_digest(raw.as_deref()),
        content_digest: digest(&state)?,
        generation: state.generation,
        encoded: match raw {
            Some(raw) => raw,
            None => encode(&state)?,
        },
        local,
    })
}
pub fn record_common(
    export: &LocalExport<'_>,
    receipt: &crate::cloud_write::Receipt,
) -> Result<Baseline> {
    export.assert_unchanged()?;
    require(
        receipt.verifies(&export.binding, &export.content_digest),
        "invalid_plan",
    )?;
    let (raw, state) = export.local.state()?;
    let id = backup(
        &export.local.private,
        &export.binding,
        &export.namespace,
        raw.as_deref(),
        &export.encoded,
        None,
    )?;
    export.assert_unchanged()?;
    publish_common(
        &export.local.private,
        export
            .local
            .folder
            .as_ref()
            .ok_or_else(|| Failure::new("local_storage"))?,
        &export.binding,
        &export.namespace,
        &id,
        &export.encoded,
        "sync",
    )?;
    baseline(&export.binding, &state)
}
