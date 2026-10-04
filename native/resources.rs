// SPDX-License-Identifier: MIT
use crate::{Error, Result, files};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
include!("assets.rs");
pub fn asset(name: &str) -> Option<&'static [u8]> {
    ASSETS.iter().find(|v| v.0 == name).map(|v| v.1)
}
pub fn json(name: &str) -> Result<Value> {
    let value: Value = serde_json::from_slice(
        asset(name).ok_or(Error::Invalid("Eine Programmressource fehlt."))?,
    )?;
    Ok(value)
}
pub fn release_identity() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| std::fs::File::open(p).ok())
            .and_then(|f| files::digest(f).ok())
            .unwrap_or_else(|| files::sha256(crate::VERSION.as_bytes()))
    })
}
pub fn install_scripts(directory: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    files::private_dir(directory)?;
    ensure_helper(
        directory
            .parent()
            .ok_or(Error::Invalid("Ungültiger Runtime-Pfad."))?,
    )?;
    for (key, _) in ASSETS {
        let data = asset(key).ok_or(Error::Invalid("Eine Programmressource fehlt."))?;
        if let Some(name) = key.strip_prefix("scripts/runtime/") {
            let path = directory.join(name);
            files::atomic(&path, data)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}
/// Publish the native runtime helper before any wrapper can refer to it. The
/// two hashes also recover an interruption between journal and file replacement.
pub fn ensure_helper(root: &Path) -> Result<()> {
    use crate::{error::require, transaction as tx};
    use serde_json::json;
    use std::{fs, os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};
    let target = root.join("tools/flightdeck-helper");
    let marker = root.join("private/native-helper.json");
    files::directory(&root.join("tools"), false)?;
    files::directory(&root.join("private"), false)?;
    let exe = std::env::current_exe()?;
    // Cargo integration tests invoke the library from a test harness; standalone
    // runtime helpers must still contain the built launcher, never that harness.
    let exe = if exe
        .parent()
        .is_some_and(|p| p.file_name().is_some_and(|n| n == "deps"))
    {
        exe.parent()
            .and_then(Path::parent)
            .ok_or(Error::Invalid("Das native Programm fehlt."))?
            .join("flightdeck-rust")
    } else {
        exe
    };
    let expected = files::digest(fs::File::open(&exe)?)?;
    let actual = if files::exists(&target) {
        Some(files::digest(tx::owned_file(root, &target)?)?)
    } else {
        None
    };
    if actual.as_deref() == Some(&expected) {
        return Ok(());
    }
    let old = if files::exists(&marker) {
        files::json::<Value>(&marker, 65536)?
    } else {
        Value::Null
    };
    require(
        actual.as_ref().is_none_or(|hash| {
            old["schema"] == 1 && (old["sha256"] == *hash || old["previous_sha256"] == *hash)
        }),
        "Der native Runtime-Helfer wurde verändert. Es wird nichts überschrieben.",
    )?;
    files::atomic_json(
        &marker,
        &json!({"schema":1,"sha256":expected,"previous_sha256":actual}),
    )?;
    tx::copy(
        fs::File::open(&exe)?,
        &target,
        Some(&expected),
        &AtomicBool::new(false),
    )?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
    files::open_at(rustix::fs::CWD, &target, false, false)?.sync_all()?;
    Ok(())
}
pub fn source_root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    executable
        .ancestors()
        .skip(1)
        .take(4)
        .find(|root| {
            root.join("compat/bootstrap.lock.json").is_file()
                && root.join("scripts/runtime/play-msfs.sh").is_file()
        })
        .map(Path::to_path_buf)
}
