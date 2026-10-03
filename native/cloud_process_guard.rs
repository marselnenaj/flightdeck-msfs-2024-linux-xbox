// SPDX-License-Identifier: MIT
//! A durable fence is cleared only after the exact supervisor completed cleanup.
use crate::{
    cloud::{self, Failure, Result, require},
    cloud_fs as fs, files,
};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path, sync::Mutex};
const NAME: &str = "cloud-interrupted-process.json";
static LOCK: Mutex<()> = Mutex::new(());
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    boot_id: String,
}
pub fn boot_id() -> Result<String> {
    let raw = files::read_public(Path::new("/proc/sys/kernel/random/boot_id"), 80)?;
    let value = std::str::from_utf8(&raw)
        .map_err(|_| Failure::new("unsafe_session"))?
        .strip_suffix('\n')
        .unwrap_or("");
    require(
        cloud::guid(value)
            && value == value.to_ascii_lowercase()
            && value != "00000000-0000-0000-0000-000000000000",
        "unsafe_session",
    )?;
    Ok(value.into())
}
fn read(private: &File) -> Result<Option<Record>> {
    let value: Option<Record> = fs::optional(private, NAME, 512)?
        .as_deref()
        .map(cloud::decode)
        .transpose()?;
    if let Some(v) = &value {
        require(
            v.schema == 1
                && cloud::guid(&v.boot_id)
                && v.boot_id == v.boot_id.to_ascii_lowercase()
                && v.boot_id != "00000000-0000-0000-0000-000000000000",
            "unsafe_session",
        )?;
    }
    Ok(value)
}
fn access(
    runtime: &Path,
    lease: &File,
    action: impl FnOnce(&File, &File) -> Result<()>,
) -> Result<()> {
    let run = || -> Result<()> {
        let _guard = LOCK.lock().map_err(|_| Failure::new("unsafe_session"))?;
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        fs::lease(&private, lease)?;
        action(&root, &private)?;
        fs::lease(&private, lease)?;
        fs::same(runtime, &root, false)?;
        fs::linked(&root, "private", &private)?;
        Ok(())
    };
    run().map_err(|_| Failure::new("unsafe_session"))
}
pub fn check(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let boot = boot_id()?;
        let previous = read(private)?;
        if let Some(v) = &previous {
            require(v.boot_id != boot, "unsafe_session")?;
            fs::same(runtime, root, false)?;
            fs::linked(root, "private", private)?;
            require(read(private)? == previous, "unsafe_session")?;
            fs::unlink(private, NAME, false)?;
            private.sync_all()?;
        }
        Ok(())
    })
}
pub fn mark(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let boot = boot_id()?;
        let previous = read(private)?;
        if previous.as_ref().is_some_and(|v| v.boot_id == boot) {
            private.sync_all()?;
            return Ok(());
        }
        let value = Record {
            schema: 1,
            boot_id: boot,
        };
        let temp = format!(".interrupted-process-{}", uuid::Uuid::new_v4().simple());
        let result = (|| {
            fs::write(private, &temp, &serde_json::to_vec(&value)?, 0o600)?;
            fs::same(runtime, root, false)?;
            fs::linked(root, "private", private)?;
            require(read(private)? == previous, "unsafe_session")?;
            if previous.is_none() {
                fs::publish(private, &temp, NAME)?;
            } else {
                rustix::fs::renameat(private, &temp, private, NAME)?;
                private.sync_all()?;
            }
            Ok(())
        })();
        let _ = fs::unlink(private, &temp, false);
        result
    })
}
/// Call only after a normal exit of the exact supervisor, or spawn failure.
pub fn clear(runtime: &Path, lease: &File) -> Result<()> {
    access(runtime, lease, |root, private| {
        let value = read(private)?;
        let boot = boot_id()?;
        require(
            value.as_ref().is_some_and(|v| v.boot_id == boot),
            "unsafe_session",
        )?;
        fs::same(runtime, root, false)?;
        fs::linked(root, "private", private)?;
        require(read(private)? == value, "unsafe_session")?;
        fs::unlink(private, NAME, false)?;
        private.sync_all()?;
        Ok(())
    })
}
