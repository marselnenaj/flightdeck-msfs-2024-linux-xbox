// SPDX-License-Identifier: MIT
//! Optional snapshot reuse. Live inventory and exact revision checks remain mandatory.
use crate::{
    cloud::{self, Result, require},
    cloud_fs as fs, cloud_import,
    cloud_storage::{self as cs, Cache, Limits, Scope, Snapshot, Transport},
    files,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path, sync::atomic::AtomicBool, time::Instant};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema: u32,
    scope_binding: String,
    snapshot_id: String,
    manifest_sha256: String,
    container_count: usize,
    blob_count: usize,
    total_bytes: usize,
}
fn load(runtime: &Path, scope: &Scope) -> Result<Cache> {
    let root = fs::open(runtime, false)?;
    let private = fs::child(&root, "private", false)?;
    let folder = fs::child(&private, "cloud-cache", false)?;
    let value: Pointer = cloud::decode(&fs::read(
        &folder,
        &format!("{}.json", scope.binding()),
        4096,
    )?)?;
    require(
        value.schema == 1
            && value.scope_binding == scope.binding()
            && cloud::identifier(&value.snapshot_id, "snapshot-")
            && files::hex_digest(&value.manifest_sha256),
        "local_storage",
    )?;
    let snap = Snapshot {
        path: runtime.join("private/cloud-saves").join(value.snapshot_id),
        scope_binding: scope.binding(),
        container_count: value.container_count,
        blob_count: value.blob_count,
        total_bytes: value.total_bytes,
        consistency: "rechecked-unlocked".into(),
    };
    let verified = cloud_import::snapshot(runtime, scope, &snap)?;
    require(verified.digest == value.manifest_sha256, "local_storage")?;
    Ok(verified.cache)
}
pub fn remember(runtime: &Path, scope: &Scope, snap: &Snapshot) -> Result<bool> {
    let verified = cloud_import::snapshot(runtime, scope, snap)?;
    let snapshot_id = snap
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| cloud::Failure::new("invalid_snapshot"))?
        .to_owned();
    let value = Pointer {
        schema: 1,
        scope_binding: scope.binding(),
        snapshot_id,
        manifest_sha256: verified.digest,
        container_count: snap.container_count,
        blob_count: snap.blob_count,
        total_bytes: snap.total_bytes,
    };
    let store = || -> Result<()> {
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        let folder = fs::child(&private, "cloud-cache", true)?;
        fs::same(runtime, &root, false)?;
        fs::linked(&root, "private", &private)?;
        fs::linked(&private, "cloud-cache", &folder)?;
        files::atomic_at(
            &folder,
            &format!("{}.json", scope.binding()),
            &serde_json::to_vec(&value)?,
        )?;
        Ok(())
    };
    Ok(store().is_ok())
}
pub fn download(
    runtime: &Path,
    client: &mut impl Transport,
    limits: &Limits,
    cancel: &AtomicBool,
    force: &BTreeSet<String>,
) -> Result<Snapshot> {
    let deadline = Instant::now() + limits.deadline;
    cloud::remaining(cancel, deadline)?;
    require(force.len() <= limits.containers * 2, "invalid_request")?;
    let mut cached = load(runtime, client.scope()).ok();
    for name in force {
        crate::save_state::name(name, true).map_err(|_| cloud::Failure::new("invalid_request"))?;
        if let Some(c) = &mut cached {
            c.containers.remove(&format!("{name},savedgame"));
        }
    }
    cloud::remaining(cancel, deadline)?;
    let root = fs::open(runtime, false)?;
    let private = fs::child(&root, "private", false)?;
    let snapshots = fs::child(&private, "cloud-saves", true)?;
    fs::same(runtime, &root, false)?;
    fs::linked(&root, "private", &private)?;
    fs::linked(&private, "cloud-saves", &snapshots)?;
    let limited = Limits {
        deadline: cloud::remaining(cancel, deadline)?,
        ..limits.clone()
    };
    let result = cs::download_cached(
        client,
        &runtime.join("private/cloud-saves"),
        &limited,
        cancel,
        cached.as_ref(),
    )?;
    cloud::remaining(cancel, deadline)?;
    let _ = remember(runtime, client.scope(), &result);
    cloud::remaining(cancel, deadline)?;
    Ok(result)
}
