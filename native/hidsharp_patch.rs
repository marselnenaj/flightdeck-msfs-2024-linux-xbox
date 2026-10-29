// SPDX-License-Identifier: MIT
//! One pinned, prebuilt compatibility DLL; no compiler or arbitrary IL patching at runtime.
use crate::{Error, Result, error::require, files, resources, transaction as tx};
use std::{path::Path, sync::atomic::AtomicBool};

pub const ORIGINAL: &str = "79f2bc8ddff102e6dbede84b2c211a3aea2ed675bf762fa0f02ecb65f61ee812";
pub const PATCHED: &str = "5942bf310242e880fcfafb3dfcdda86d2a9a0bc6080c962dcd82a3221c4ed9aa";

pub fn state(path: &Path) -> &'static str {
    match tx::digest(path).as_deref() {
        Ok(PATCHED) => "ready",
        Ok(ORIGINAL) => "required",
        Ok(_) => "unsupported",
        Err(_) => "missing",
    }
}
pub fn verified_payload(bytes: &[u8]) -> bool {
    bytes.len() == 238080 && files::sha256(bytes) == PATCHED && bytes.starts_with(b"MZ")
}
pub fn payload_status() -> serde_json::Value {
    let bytes = resources::asset("mobiflight/HidSharp.dll");
    serde_json::json!({"schema":1,"bundled":bytes.is_some_and(verified_payload),"sha256":bytes.map(files::sha256),"expected_sha256":PATCHED})
}
pub fn payload() -> Result<Vec<u8>> {
    let bytes = if let Some(bytes) = resources::asset("mobiflight/HidSharp.dll") {
        bytes.to_vec()
    } else {
        // Developer builds may use the maintainer's generated, equally pinned payload.
        let root = resources::source_root().ok_or(Error::Invalid(
            "Die geprüfte MobiFlight-HID-Komponente fehlt in dieser Flightdeck-Version.",
        ))?;
        files::read(&root.join("build/mobiflight/HidSharp.dll"), 256 * 1024)?
    };
    require(
        verified_payload(&bytes),
        "Die MobiFlight-HID-Komponente hat eine ungültige Prüfsumme.",
    )?;
    Ok(bytes)
}
pub fn apply(directory: &Path, backup_root: &Path, cancel: &AtomicBool) -> Result<()> {
    let path = directory.join("HidSharp.dll");
    if state(&path) == "ready" {
        return Ok(());
    }
    require(
        state(&path) == "required",
        "Diese HidSharp-Version wird vom geprüften MobiFlight-HID-Fix noch nicht unterstützt. Die DLL bleibt unverändert.",
    )?;
    let bytes = payload()?;
    tx::interrupted(cancel)?;
    files::private_dir(backup_root)?;
    let backup = backup_root.join(format!("HidSharp-{ORIGINAL}.dll"));
    if files::exists(&backup) {
        require(
            tx::digest(&backup)? == ORIGINAL,
            "Die MobiFlight-HID-Sicherung ist beschädigt.",
        )?;
    } else {
        tx::copy(
            tx::owned_file(directory, &path)?,
            &backup,
            Some(ORIGINAL),
            cancel,
        )?;
    }
    require(
        tx::digest(&path)? == ORIGINAL,
        "Die MobiFlight-Dateien wurden während der Vorbereitung verändert.",
    )?;
    files::atomic(&path, &bytes)
}
