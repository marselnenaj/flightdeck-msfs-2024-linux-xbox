// SPDX-License-Identifier: MIT
//! Cloud-first session policy, independently testable without a game or account.
use crate::{
    cloud::{Failure, Result, require},
    cloud_cache, cloud_fs as fs,
    cloud_import::{self as ci, Choice, Plan},
    cloud_policy::{self, Action, SaveSet},
    cloud_session::{self as session, Phase, Record},
    cloud_storage as cs,
    cloud_write::{self as cw, Operations},
    files,
};
use std::{collections::BTreeSet, fs::File, path::Path, sync::atomic::AtomicBool};
pub struct Attention {
    pub error: Failure,
    pub plan: Option<Box<Plan>>,
}
impl From<Failure> for Attention {
    fn from(error: Failure) -> Self {
        Self { error, plan: None }
    }
}
impl From<crate::Error> for Attention {
    fn from(error: crate::Error) -> Self {
        Failure::from(error).into()
    }
}
impl Attention {
    fn conflict(plan: &Plan) -> Self {
        Self {
            error: Failure::new("conflict"),
            plan: Some(Box::new(plan.clone())),
        }
    }
}
pub type Outcome<T> = std::result::Result<T, Attention>;
pub struct Request<'a> {
    pub runtime: &'a Path,
    pub lease: &'a File,
    pub cancel: &'a AtomicBool,
    pub review: Option<(&'a Plan, &'a Choice)>,
    pub expected_binding: Option<&'a str>,
}
const OFFLINE: &str = "cloud-offline.pending";
const OFFLINE_BODY: &[u8] = b"Flightdeck local session pending\n";
pub fn offline(runtime: &Path, lease: &File, set: bool) -> Result<bool> {
    let root = fs::open(runtime, false)?;
    let private = fs::child(&root, "private", false)?;
    fs::lease(&private, lease)?;
    if set {
        fs::same(runtime, &root, false)?;
        fs::linked(&root, "private", &private)?;
        files::atomic_at(&private, OFFLINE, OFFLINE_BODY)?;
        return Ok(true);
    }
    let value = fs::optional(&private, OFFLINE, 128)?;
    if let Some(bytes) = &value {
        require(bytes == OFFLINE_BODY, "local_storage")?;
    }
    Ok(value.is_some())
}
pub fn enable_local(runtime: &Path, lease: &File) -> Result<()> {
    let root = fs::open(runtime, false)?;
    let private = fs::child(&root, "private", false)?;
    fs::lease(&private, lease)?;
    fs::child(&private, "local-saves", true)?;
    if fs::optional(&private, "local-saves.enabled", 4096)?.is_none() {
        fs::write(&private, "local-saves.enabled", b"enabled\n", 0o600)?;
    }
    fs::same(runtime, &root, false)?;
    fs::linked(&root, "private", &private)?;
    private.sync_all()?;
    Ok(())
}
fn finish<T: Operations>(
    request: &Request<'_>,
    client: &mut T,
    record: &Record,
    expected: &str,
    playing: bool,
) -> Outcome<()> {
    let scope = client.scope().clone();
    let local = ci::export_local(request.runtime, &scope, request.lease, request.cancel)?;
    let record = session::update(
        request.runtime,
        &scope,
        record,
        Phase::Uploading,
        request.lease,
        Some(&local),
    )?;
    let receipt = cw::upload(
        &scope,
        client,
        &local.state()?,
        expected,
        || local.assert_unchanged(),
        request.cancel,
        &cw::Limits::default(),
    )?;
    ci::record_common(&local, &receipt)?;
    require(receipt.lease_released(), "lease_lost")?;
    if playing {
        session::checkpoint(request.runtime, &scope, &record, &local, &receipt)?;
    } else {
        session::complete(request.runtime, &scope, &record, &local, &receipt)?;
    }
    Ok(())
}
fn reviewed(plan: &Plan, review: &Plan) -> bool {
    plan.binding == review.binding
        && plan.remote_digest == review.remote_digest
        && plan.local_digest == review.local_digest
}
pub fn before<T: Operations>(request: &Request<'_>, client: &mut T) -> Outcome<()> {
    let scope = client.scope().clone();
    let snap = cloud_cache::download(
        request.runtime,
        client,
        &cs::Limits::default(),
        request.cancel,
        &BTreeSet::new(),
    )?;
    let remote = ci::read_snapshot(request.runtime, &scope, &snap)?;
    let baseline = ci::load_baseline(request.runtime, &scope)?;
    let plan = ci::prepare(request.runtime, &scope, &snap, baseline.as_ref())?;
    let record = session::load(request.runtime, &scope)?;
    let local = {
        let export = ci::export_local(request.runtime, &scope, request.lease, request.cancel)?;
        export.state()?
    };
    let local_digest = ci::digest(&local)?;
    if let Some((review, choice)) = request.review {
        if !reviewed(&plan, review) {
            return Err(Attention::conflict(&plan));
        }
        if matches!(choice, Choice::Cloud) {
            require(
                ci::apply(
                    request.runtime,
                    &scope,
                    &plan,
                    &Choice::Cloud,
                    request.lease,
                    request.cancel,
                )?
                .durability_confirmed,
                "local_storage",
            )?;
        }
        let record = if let Some(record) = record {
            record
        } else {
            let export = ci::export_local(request.runtime, &scope, request.lease, request.cancel)?;
            session::begin(request.runtime, &scope, &snap, &export, Phase::Prepared)?
        };
        return finish(request, client, &record, &plan.remote_digest, true);
    }
    if let Some(record) = record {
        if plan.remote_digest != record.before_remote_digest
            && plan.remote_digest != record.target_digest
        {
            return Err(Attention::conflict(&plan));
        }
        if record.phase != Phase::Playing && local_digest != record.target_digest {
            return Err(Attention::conflict(&plan));
        }
        if plan.remote_digest == record.target_digest
            && plan.remote_digest != record.before_remote_digest
        {
            if local_digest != record.target_digest {
                return Err(Attention::conflict(&plan));
            }
            return finish(request, client, &record, &plan.remote_digest, true);
        }
        let export = ci::export_local(request.runtime, &scope, request.lease, request.cancel)?;
        session::update(
            request.runtime,
            &scope,
            &record,
            Phase::Playing,
            request.lease,
            Some(&export),
        )?;
        return Ok(());
    }
    if offline(request.runtime, request.lease, false)?
        && baseline.is_none()
        && !local.containers.is_empty()
        && local_digest != plan.remote_digest
        && !remote.containers.is_empty()
    {
        return Err(Attention::conflict(&plan));
    }
    let left = SaveSet::from_state(&scope.binding(), &local)?;
    let right = SaveSet::from_state(&scope.binding(), &remote)?;
    let old = baseline.as_ref().map(ci::Baseline::save_set).transpose()?;
    let policy = cloud_policy::decide(&scope.binding(), Some(&left), Some(&right), old.as_ref())?;
    if matches!(policy.action, Action::Conflict | Action::Blocked) {
        return Err(Attention::conflict(&plan));
    }
    if matches!(policy.action, Action::ImportCloud | Action::Merge) {
        let choice = if policy.action == Action::ImportCloud {
            Choice::Cloud
        } else {
            Choice::Automatic
        };
        require(
            ci::apply(
                request.runtime,
                &scope,
                &plan,
                &choice,
                request.lease,
                request.cancel,
            )?
            .durability_confirmed,
            "local_storage",
        )?;
    }
    let export = ci::export_local(request.runtime, &scope, request.lease, request.cancel)?;
    require(
        Some(SaveSet::from_state(&scope.binding(), &export.state()?)?) == policy.target,
        "changed",
    )?;
    let record = session::begin(request.runtime, &scope, &snap, &export, Phase::Prepared)?;
    session::update(
        request.runtime,
        &scope,
        &record,
        Phase::Playing,
        request.lease,
        Some(&export),
    )?;
    Ok(())
}
pub fn after<T: Operations>(request: &Request<'_>, client: &mut T) -> Outcome<()> {
    let scope = client.scope().clone();
    require(
        request
            .expected_binding
            .is_none_or(|b| scope.binding() == b),
        "invalid_scope",
    )?;
    let snap = cloud_cache::download(
        request.runtime,
        client,
        &cs::Limits::default(),
        request.cancel,
        &BTreeSet::new(),
    )?;
    let baseline = ci::load_baseline(request.runtime, &scope)?;
    let plan = ci::prepare(request.runtime, &scope, &snap, baseline.as_ref())?;
    let record =
        session::load(request.runtime, &scope)?.ok_or_else(|| Failure::new("invalid_scope"))?;
    let local_digest = {
        let export = ci::export_local(request.runtime, &scope, request.lease, request.cancel)?;
        export.content_digest.clone()
    };
    if let Some((review, choice)) = request.review {
        if !reviewed(&plan, review) {
            return Err(Attention::conflict(&plan));
        }
        if matches!(choice, Choice::Cloud) {
            require(
                ci::apply(
                    request.runtime,
                    &scope,
                    &plan,
                    &Choice::Cloud,
                    request.lease,
                    request.cancel,
                )?
                .durability_confirmed,
                "local_storage",
            )?;
        }
    } else if plan.remote_digest != record.before_remote_digest
        && plan.remote_digest != local_digest
    {
        return Err(Attention::conflict(&plan));
    }
    finish(request, client, &record, &plan.remote_digest, false)
}
