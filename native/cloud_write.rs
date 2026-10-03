// SPDX-License-Identifier: MIT
//! Exclusive cloud writes with owner fencing, bounded cleanup and exact readback.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_import,
    cloud_storage::{Scope, Transport},
    files,
    save_state::{self, Container, State},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub struct LeaseReply {
    pub status: u16,
    pub owner_change_id: String,
    pub quota_bytes: u64,
}
pub trait Operations: Transport {
    fn acquire(&mut self, timeout: Duration) -> Result<LeaseReply>;
    fn renew(&mut self, timeout: Duration) -> Result<LeaseReply>;
    /// Cleanup ignores cancellation but must honor its own bounded timeout.
    fn release(&mut self, timeout: Duration) -> Result<u16>;
    fn upload_atom(&mut self, payload: &[u8], timeout: Duration) -> Result<String>;
    fn put_container(
        &mut self,
        name: &str,
        display: &str,
        modified: u64,
        atoms: &BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<u16>;
    fn delete_container(&mut self, name: &str, timeout: Duration) -> Result<u16>;
    /// Complete, twice-rechecked state; force bypasses caches for written containers.
    fn remote(&mut self, timeout: Duration, force: &BTreeSet<String>) -> Result<State>;
}
pub struct Limits {
    pub operations: usize,
    pub blob_bytes: usize,
    pub total_bytes: usize,
    pub timeout: Duration,
    pub deadline: Duration,
    pub cleanup: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            operations: 8192,
            blob_bytes: 64 * 1024 * 1024,
            total_bytes: save_state::QUOTA,
            timeout: Duration::from_secs(30),
            deadline: Duration::from_secs(600),
            cleanup: Duration::from_secs(10),
        }
    }
}
/// Only this module can construct a receipt, after exact remote readback.
/// It cannot be deserialized from a request, journal or downloaded snapshot.
pub struct Receipt {
    binding: String,
    source: String,
    pub(crate) before: String,
    changed: usize,
    containers: usize,
    blobs: usize,
    total: usize,
    released: bool,
}
impl Receipt {
    pub fn remote_before_digest(&self) -> &str {
        &self.before
    }
    pub fn verifies(&self, binding: &str, source: &str) -> bool {
        self.binding == binding && self.source == source
    }
    pub fn summary(&self) -> Value {
        json!({"committed":true,"changed_containers":self.changed,"container_count":self.containers,"blob_count":self.blobs,"total_bytes":self.total,"lease_released":self.released,"protocolproof":"connected-storage-lock-v1-readback"})
    }
    pub fn lease_released(&self) -> bool {
        self.released
    }
}
fn entry_digest(name: &str, entry: Option<&Container>) -> Result<Option<String>> {
    entry
        .map(|v| {
            cloud_import::digest(&State {
                generation: 0,
                containers: [(name.into(), v.clone())].into(),
            })
        })
        .transpose()
}
struct Write<'a, T: Operations, F: Fn() -> Result<()>> {
    ops: &'a mut T,
    assert_local: F,
    cancel: &'a AtomicBool,
    limits: &'a Limits,
    deadline: Instant,
    operations: usize,
    committed: usize,
    mutation_attempted: bool,
    acquired: bool,
    owner: Option<String>,
    mutated: BTreeSet<String>,
}
impl<T: Operations, F: Fn() -> Result<()>> Write<'_, T, F> {
    fn check(&self) -> Result<Duration> {
        let remaining = cloud::remaining(self.cancel, self.deadline)?;
        (self.assert_local)().map_err(|_| Failure::new("local_changed"))?;
        Ok(remaining)
    }
    fn budget(&mut self) -> Result<Duration> {
        let remaining = self.check()?.min(self.limits.timeout);
        self.operations += 1;
        require(self.operations <= self.limits.operations, "bounds")?;
        Ok(remaining)
    }
    fn remote(&mut self, force: &BTreeSet<String>) -> Result<State> {
        let timeout = self.check()?;
        let state = self.ops.remote(timeout, force)?;
        save_state::encode(&state).map_err(|_| Failure::new("invalid_input"))?;
        self.check()?;
        Ok(state)
    }
    fn lease(&mut self, reply: LeaseReply, first: bool) -> Result<u64> {
        match reply.status {
            401 | 403 => {
                self.acquired = false;
                return Err(Failure::new("authentication").status(reply.status));
            }
            409 => {
                self.acquired = false;
                return Err(Failure::new("lease_lost").status(409));
            }
            200 | 201 => (),
            _ => return Err(Failure::new("transport").status(reply.status)),
        }
        require(
            !reply.owner_change_id.is_empty()
                && reply.owner_change_id.len() <= 1024
                && !reply
                    .owner_change_id
                    .chars()
                    .any(|c| c < ' ' || c == '\u{7f}')
                && reply.quota_bytes > 0,
            "invalid_response",
        )?;
        if first {
            self.owner = Some(reply.owner_change_id);
            self.acquired = true;
        } else if reply.status == 201 {
            self.acquired = true;
            return Err(Failure::new("lease_lost"));
        } else if self.owner.as_ref() != Some(&reply.owner_change_id) {
            self.acquired = false;
            return Err(Failure::new("lease_lost"));
        } else {
            self.acquired = true;
        }
        self.check()?;
        Ok(reply.quota_bytes)
    }
    fn renew(&mut self) -> Result<()> {
        let timeout = self.budget()?;
        // An unconfirmed renewal must never release a possibly foreign lease.
        self.acquired = false;
        let reply = self.ops.renew(timeout)?;
        self.lease(reply, false)?;
        Ok(())
    }
    fn confirm(&mut self, name: &str, expected: Option<&Container>) -> Result<()> {
        self.renew()?;
        let actual = self.remote(&[name.into()].into())?;
        require(
            entry_digest(name, actual.containers.get(name))? == entry_digest(name, expected)?,
            "readback",
        )?;
        self.renew()?;
        self.committed += 1;
        Ok(())
    }
    fn commit(
        &mut self,
        name: &str,
        entry: Option<&Container>,
        atoms: &BTreeMap<String, String>,
    ) -> Result<()> {
        self.renew()?;
        self.check()?;
        self.mutation_attempted = true;
        self.mutated.insert(name.into());
        let timeout = self.budget()?;
        let reply = if let Some(entry) = entry {
            self.ops.put_container(
                &format!("{name},savedgame"),
                &entry.display_name,
                entry.modified,
                atoms,
                timeout,
            )
        } else {
            self.ops
                .delete_container(&format!("{name},savedgame"), timeout)
        };
        match reply {
            Ok(409) => {
                self.acquired = false;
                return Err(Failure::new("lease_lost").status(409));
            }
            Ok(status @ (401 | 403)) => {
                self.acquired = false;
                return Err(Failure::new("authentication").status(status));
            }
            // A lost or negative response may follow a successful commit.
            // Read it back under the same owner; never replay the mutation.
            _ => (),
        }
        self.confirm(name, entry)
    }
}
pub fn upload<T: Operations>(
    scope: &Scope,
    ops: &mut T,
    local: &State,
    expected: &str,
    assert_local: impl Fn() -> Result<()>,
    cancel: &AtomicBool,
    limits: &Limits,
) -> Result<Receipt> {
    require(ops.scope() == scope, "invalid_scope")?;
    require(files::hex_digest(expected), "invalid_input")?;
    save_state::encode(local).map_err(|_| Failure::new("invalid_input"))?;
    let source = cloud_import::digest(local)?;
    let (containers, blobs, total) = cloud_import::counts(local);
    require(
        total <= limits.total_bytes
            && local
                .containers
                .values()
                .flat_map(|c| c.blobs.values())
                .all(|b| b.len() <= limits.blob_bytes),
        "bounds",
    )?;
    let mut run = Write {
        ops,
        assert_local,
        cancel,
        limits,
        deadline: Instant::now() + limits.deadline,
        operations: 0,
        committed: 0,
        mutation_attempted: false,
        acquired: false,
        owner: None,
        mutated: BTreeSet::new(),
    };
    let result = (|| -> Result<()> {
        let timeout = run.budget()?;
        let lease = run.ops.acquire(timeout)?;
        let quota = run.lease(lease, true)?;
        require(total as u64 <= quota, "quota")?;
        let remote = run.remote(&BTreeSet::new())?;
        require(cloud_import::digest(&remote)? == expected, "conflict")?;
        run.renew()?;
        let names: BTreeSet<_> = local
            .containers
            .keys()
            .chain(remote.containers.keys())
            .cloned()
            .collect();
        for name in names {
            let target = local.containers.get(&name);
            let old = remote.containers.get(&name);
            if target.is_some()
                && old.is_some()
                && entry_digest(&name, target)? == entry_digest(&name, old)?
            {
                continue;
            }
            let mut atoms = BTreeMap::new();
            let mut ids = BTreeSet::new();
            if let Some(target) = target {
                run.renew()?;
                for (blob, payload) in &target.blobs {
                    let timeout = run.budget()?;
                    let atom = run.ops.upload_atom(payload, timeout)?;
                    require(
                        cloud::guid(&atom) && ids.insert(atom.to_ascii_lowercase()),
                        "invalid_response",
                    )?;
                    atoms.insert(blob.clone(), atom);
                    run.check()?;
                }
            }
            run.commit(&name, target, &atoms)?;
        }
        run.renew()?;
        let forced = run.mutated.clone();
        require(
            cloud_import::digest(&run.remote(&forced)?)? == source,
            "readback",
        )?;
        run.renew()?;
        Ok(())
    })();
    if result
        .as_ref()
        .err()
        .is_some_and(|e| matches!(e.http_status, Some(401 | 403 | 409)))
    {
        run.acquired = false;
    }
    let released = run.acquired
        && run
            .ops
            .release(limits.cleanup)
            .is_ok_and(|s| matches!(s, 200 | 204));
    if let Err(mut error) = result {
        error.committed_containers = run.committed;
        error.recovery_required = run.mutation_attempted;
        return Err(error);
    }
    Ok(Receipt {
        binding: scope.binding(),
        source,
        before: expected.into(),
        changed: run.committed,
        containers,
        blobs,
        total,
        released,
    })
}
