// SPDX-License-Identifier: MIT
//! Durable session intentions. A journal is never proof that an upload succeeded.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_fs as fs,
    cloud_import::{self as ci, LocalExport},
    cloud_storage::{Scope, Snapshot},
    cloud_write::Receipt,
    files, save_state,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::Path,
    sync::{Mutex, MutexGuard},
};
static LOCK: Mutex<()> = Mutex::new(());
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    Playing,
    Uploading,
    Pending,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Reference {
    id: String,
    manifest_sha256: String,
    container_count: usize,
    blob_count: usize,
    total_bytes: usize,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    schema: u32,
    scope_binding: String,
    namespace: String,
    session_id: String,
    revision: u64,
    pub(crate) phase: Phase,
    snapshot: Reference,
    original_remote_digest: String,
    pub(crate) before_remote_digest: String,
    pre_local_digest: String,
    pre_local_sha256: String,
    initial_backup_id: String,
    pub(crate) target_digest: String,
    target_sha256: String,
    target_local_sha256: String,
    target_backup_id: String,
    generation: u64,
    confirmed_backup_id: Option<String>,
}
struct Access<'a> {
    _guard: MutexGuard<'static, ()>,
    runtime: &'a Path,
    scope: &'a Scope,
    root: File,
    private: File,
    folder: Option<File>,
    namespace: String,
    lease: Option<&'a File>,
}
impl<'a> Access<'a> {
    fn new(
        runtime: &'a Path,
        scope: &'a Scope,
        lease: Option<&'a File>,
        export: Option<&LocalExport<'_>>,
    ) -> Result<Self> {
        let guard = LOCK.lock().map_err(|_| Failure::new("busy"))?;
        if let Some(e) = export {
            e.assert_unchanged()?;
            require(
                e.binding == scope.binding()
                    && e.namespace == scope.namespace()?
                    && e.runtime() == runtime,
                "invalid_scope",
            )?;
        }
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        if let Some(lease) = lease {
            fs::lease(&private, lease)?;
        }
        let folder = if lease.is_none()
            && matches!(
                rustix::fs::statat(
                    &private,
                    "cloud-sessions",
                    rustix::fs::AtFlags::SYMLINK_NOFOLLOW
                ),
                Err(rustix::io::Errno::NOENT)
            ) {
            None
        } else {
            Some(fs::child(&private, "cloud-sessions", lease.is_some())?)
        };
        let this = Self {
            _guard: guard,
            runtime,
            scope,
            root,
            private,
            folder,
            namespace: scope.namespace()?,
            lease,
        };
        this.check()?;
        Ok(this)
    }
    fn check(&self) -> Result<()> {
        fs::same(self.runtime, &self.root, false)?;
        fs::linked(&self.root, "private", &self.private)?;
        if let Some(folder) = &self.folder {
            fs::linked(&self.private, "cloud-sessions", folder)?;
        }
        if let Some(lease) = self.lease {
            fs::lease(&self.private, lease)?;
        }
        Ok(())
    }
    fn validated(&self, r: Record) -> Result<Record> {
        let binding = self.scope.binding();
        require(
            r.schema == 1
                && r.scope_binding == binding
                && r.namespace == self.namespace
                && cloud::identifier(&r.session_id, "")
                && r.revision <= i64::MAX as u64,
            "invalid_session",
        )?;
        for digest in [
            &r.original_remote_digest,
            &r.before_remote_digest,
            &r.pre_local_digest,
            &r.target_digest,
            &r.target_sha256,
        ] {
            require(files::hex_digest(digest), "invalid_session")?;
        }
        for digest in [&r.pre_local_sha256, &r.target_local_sha256] {
            require(
                digest == "missing" || files::hex_digest(digest),
                "invalid_session",
            )?;
        }
        require(
            cloud::identifier(&r.snapshot.id, "snapshot-")
                && files::hex_digest(&r.snapshot.manifest_sha256),
            "invalid_session",
        )?;
        let snapshot = Snapshot {
            path: self
                .runtime
                .join("private/cloud-saves")
                .join(&r.snapshot.id),
            scope_binding: binding.clone(),
            container_count: r.snapshot.container_count,
            blob_count: r.snapshot.blob_count,
            total_bytes: r.snapshot.total_bytes,
            consistency: "rechecked-unlocked".into(),
        };
        let remote = ci::snapshot(self.runtime, self.scope, &snapshot)?;
        require(
            remote.digest == r.snapshot.manifest_sha256
                && ci::digest(&remote.state)? == r.original_remote_digest,
            "invalid_session",
        )?;
        let initial = ci::backup_data(
            &self.private,
            &binding,
            &self.namespace,
            &r.initial_backup_id,
        )?;
        let initial_state = initial
            .original
            .as_deref()
            .map(save_state::decode)
            .transpose()?
            .unwrap_or_default();
        require(
            ci::raw_digest(initial.original.as_deref()) == r.pre_local_sha256
                && ci::digest(&initial_state)? == r.pre_local_digest,
            "invalid_session",
        )?;
        let target = ci::backup_data(
            &self.private,
            &binding,
            &self.namespace,
            &r.target_backup_id,
        )?;
        let state = save_state::decode(&target.imported)?;
        require(
            files::sha256(&target.imported) == r.target_sha256
                && state.generation == r.generation
                && ci::digest(&state)? == r.target_digest
                && target
                    .metadata
                    .state_sha256
                    .unwrap_or_else(|| "missing".into())
                    == r.target_local_sha256,
            "invalid_session",
        )?;
        if let Some(id) = &r.confirmed_backup_id {
            let confirmed = ci::backup_data(&self.private, &binding, &self.namespace, id)?;
            require(
                ci::digest(&save_state::decode(&confirmed.imported)?)? == r.before_remote_digest,
                "invalid_session",
            )?;
        } else {
            require(
                r.before_remote_digest == r.original_remote_digest,
                "invalid_session",
            )?;
        }
        Ok(r)
    }
    fn load(&self) -> Result<Option<Record>> {
        let Some(folder) = &self.folder else {
            return Ok(None);
        };
        let value = fs::optional(folder, &format!("{}.json", self.namespace), 32768)?
            .as_deref()
            .map(cloud::decode::<Record>)
            .transpose()?;
        value.map(|v| self.validated(v)).transpose()
    }
    fn current(&self, r: &Record) -> Result<Record> {
        let current = self.load()?;
        require(current.as_ref() == Some(r), "changed")?;
        require(r.revision < i64::MAX as u64, "invalid_session")?;
        Ok(r.clone())
    }
    fn store(&self, r: &Record, new: bool) -> Result<()> {
        require(self.lease.is_some(), "invalid_lock")?;
        self.check()?;
        let folder = self
            .folder
            .as_ref()
            .ok_or_else(|| Failure::new("local_storage"))?;
        let data = serde_json::to_vec(r)?;
        require(data.len() <= 32768, "invalid_session")?;
        let name = format!("{}.json", self.namespace);
        if new {
            let temp = format!(".session-{}", uuid::Uuid::new_v4().simple());
            let result = (|| {
                fs::write(folder, &temp, &data, 0o600)?;
                self.check()?;
                fs::publish(folder, &temp, &name)
            })();
            let _ = fs::unlink(folder, &temp, false);
            result
        } else {
            files::atomic_at(folder, &name, &data)?;
            Ok(())
        }
    }
}
fn capture(export: &LocalExport<'_>) -> Result<(String, String)> {
    export.assert_unchanged()?;
    let raw = (export.raw_digest != "missing").then_some(export.encoded.as_slice());
    let id = ci::backup(
        &export.local.private,
        &export.binding,
        &export.namespace,
        raw,
        &export.encoded,
        None,
    )?;
    export.assert_unchanged()?;
    Ok((id, files::sha256(&export.encoded)))
}
pub fn load(runtime: &Path, scope: &Scope) -> Result<Option<Record>> {
    let access = Access::new(runtime, scope, None, None)?;
    let result = access.load()?;
    access.check()?;
    Ok(result)
}
pub fn begin(
    runtime: &Path,
    scope: &Scope,
    snapshot: &Snapshot,
    export: &LocalExport<'_>,
    phase: Phase,
) -> Result<Record> {
    require(
        matches!(phase, Phase::Prepared | Phase::Playing),
        "invalid_session",
    )?;
    let access = Access::new(runtime, scope, Some(export.lease()?), Some(export))?;
    require(access.load()?.is_none(), "busy")?;
    let remote = ci::snapshot(runtime, scope, snapshot)?;
    let (target_backup_id, target_sha256) = capture(export)?;
    let remote_digest = ci::digest(&remote.state)?;
    let r = Record {
        schema: 1,
        scope_binding: scope.binding(),
        namespace: scope.namespace()?,
        session_id: uuid::Uuid::new_v4().simple().to_string(),
        revision: 0,
        phase,
        snapshot: Reference {
            id: snapshot
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| Failure::new("invalid_snapshot"))?
                .into(),
            manifest_sha256: remote.digest,
            container_count: snapshot.container_count,
            blob_count: snapshot.blob_count,
            total_bytes: snapshot.total_bytes,
        },
        original_remote_digest: remote_digest.clone(),
        before_remote_digest: remote_digest,
        pre_local_digest: export.content_digest.clone(),
        pre_local_sha256: export.raw_digest.clone(),
        initial_backup_id: target_backup_id.clone(),
        target_digest: export.content_digest.clone(),
        target_sha256,
        target_local_sha256: export.raw_digest.clone(),
        target_backup_id,
        generation: export.generation,
        confirmed_backup_id: None,
    };
    access.store(&r, true)?;
    Ok(r)
}
pub fn update(
    runtime: &Path,
    scope: &Scope,
    record: &Record,
    phase: Phase,
    lease: &File,
    export: Option<&LocalExport<'_>>,
) -> Result<Record> {
    require(
        phase != Phase::Uploading || export.is_some(),
        "invalid_session",
    )?;
    let access = Access::new(runtime, scope, Some(lease), export)?;
    let mut value = access.current(record)?;
    if let Some(e) = export {
        let (id, hash) = capture(e)?;
        value.target_backup_id = id;
        value.target_sha256 = hash;
        value.target_digest = e.content_digest.clone();
        value.target_local_sha256 = e.raw_digest.clone();
        value.generation = e.generation;
    }
    value.phase = phase;
    value.revision += 1;
    access.store(&value, false)?;
    Ok(value)
}
fn confirmed(value: &Record, export: &LocalExport<'_>, receipt: &Receipt) -> Result<()> {
    export.assert_unchanged()?;
    require(
        export.content_digest == value.target_digest
            && export.raw_digest == value.target_local_sha256
            && files::sha256(&export.encoded) == value.target_sha256,
        "changed",
    )?;
    require(
        receipt.verifies(&export.binding, &value.target_digest),
        "invalid_receipt",
    )
}
pub fn checkpoint(
    runtime: &Path,
    scope: &Scope,
    record: &Record,
    export: &LocalExport<'_>,
    receipt: &Receipt,
) -> Result<Record> {
    let access = Access::new(runtime, scope, Some(export.lease()?), Some(export))?;
    let mut value = access.current(record)?;
    confirmed(&value, export, receipt)?;
    value.before_remote_digest = value.target_digest.clone();
    value.confirmed_backup_id = Some(value.target_backup_id.clone());
    value.phase = Phase::Playing;
    value.revision += 1;
    access.store(&value, false)?;
    Ok(value)
}
pub fn complete(
    runtime: &Path,
    scope: &Scope,
    record: &Record,
    export: &LocalExport<'_>,
    receipt: &Receipt,
) -> Result<()> {
    let access = Access::new(runtime, scope, Some(export.lease()?), Some(export))?;
    let value = access.current(record)?;
    confirmed(&value, export, receipt)?;
    access.check()?;
    let folder = access
        .folder
        .as_ref()
        .ok_or_else(|| Failure::new("local_storage"))?;
    fs::unlink(folder, &format!("{}.json", access.namespace), false)?;
    folder.sync_all()?;
    Ok(())
}
