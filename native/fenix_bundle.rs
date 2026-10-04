// SPDX-License-Identifier: MIT
//! Pinned overlay payloads; never execute or import downloaded installer scripts.
use crate::{
    Error, Result,
    backend::{Context, string},
    bootstrap,
    error::require,
    files, resources, runtime, transaction as tx,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
pub const LAUNCH_FILES: [&str; 2] = ["launch-msfs.sh", "xodus-wine-launch"];
// Kept in upstream manifests for old launchers; Rust owns display refresh now.
pub const LEGACY_DISPLAY_HELPER: &str = "fenix-display-refresh.py";
pub fn manifest(variant: Option<&str>) -> Result<Value> {
    let mut lock = resources::json("compat/fenix/bundle.json")?;
    if let Some(variant) = variant {
        let entry = lock["variants"][variant]
            .as_object()
            .ok_or(Error::Invalid(
                "No Fenix compatibility build is available for this Proton version.",
            ))?
            .clone();
        lock.as_object_mut()
            .ok_or(Error::Invalid("Invalid Fenix bundle."))?
            .extend(entry);
        lock["variant"] = variant.into();
    }
    Ok(lock)
}
pub fn contained(root: &Path, name: &str) -> Result<PathBuf> {
    require(files::relative(name), "Invalid installation path.")?;
    let root = root.canonicalize()?;
    let path = root.join(name);
    let mut parent = path
        .parent()
        .ok_or(Error::Invalid("Invalid installation path."))?;
    while !files::exists(parent) {
        parent = parent
            .parent()
            .ok_or(Error::Invalid("Invalid installation path."))?;
    }
    require(
        parent.canonicalize()?.starts_with(&root),
        "Installation directory points outside the selected profile.",
    )?;
    Ok(path)
}
pub fn write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .ok_or(Error::Invalid("Invalid installation path."))?;
    if !parent.exists() {
        files::private_dir(parent)?;
    }
    files::atomic(path, bytes)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}
fn map(value: &Value) -> Result<BTreeMap<String, String>> {
    let m: BTreeMap<String, String> = serde_json::from_value(value.clone())?;
    require(
        m.iter()
            .all(|(k, v)| files::relative(k) && files::hex_digest(v)),
        "Invalid Fenix checksum list.",
    )?;
    Ok(m)
}
pub fn version(runner: &Path) -> Result<String> {
    let data = files::read_public(&runner.join("version"), 4096)?;
    let raw = std::str::from_utf8(&data)
        .map_err(|_| Error::Invalid("Invalid Wine version."))?
        .trim();
    let value = raw
        .split_once(char::is_whitespace)
        .filter(|(a, _)| a.bytes().all(|b| b.is_ascii_digit()))
        .map(|(_, v)| v.trim_start())
        .unwrap_or(raw);
    require(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "Invalid Wine version.",
    )?;
    Ok(value.into())
}
pub fn verify_runner(runner: &Path, lock: &Value, patched: bool) -> Result<()> {
    let expected = string(lock, "runner_version")?;
    let expected = expected
        .split_once(char::is_whitespace)
        .filter(|(a, _)| a.bytes().all(|b| b.is_ascii_digit()))
        .map(|(_, v)| v.trim_start())
        .unwrap_or(expected);
    require(
        version(runner)? == expected,
        "The Wine runner version does not match this Fenix build.",
    )?;
    let mut hashes = map(&lock["runner_files"])?;
    if patched {
        hashes.extend(map(&lock["files"])?);
    }
    for (name, hash) in hashes {
        require(
            tx::digest(&contained(runner, &name)?)? == hash,
            "The Wine runner differs from the supported Fenix build.",
        )?;
    }
    Ok(())
}
pub fn runner_variant(runner: &Path, patched: bool) -> Result<Option<String>> {
    if verify_runner(runner, &manifest(None)?, patched).is_ok() {
        return Ok(None);
    }
    for name in manifest(None)?["variants"]
        .as_object()
        .into_iter()
        .flat_map(|v| v.keys())
    {
        if verify_runner(runner, &manifest(Some(name))?, patched).is_ok() {
            return Ok(Some(name.clone()));
        }
    }
    Err(Error::Invalid(
        "Für diese Proton-Version fehlt ein passender Fenix-Patch. Wähle eine Version mit Fenix-Unterstützung.",
    ))
}
fn wanted(lock: &Value) -> Result<BTreeMap<String, Option<String>>> {
    let mut wanted = BTreeMap::from([("bundle.json".into(), None)]);
    for (name, hash) in map(&lock["files"])? {
        wanted.insert(format!("payload/{name}"), Some(hash));
    }
    for (variant, value) in lock["variants"].as_object().into_iter().flatten() {
        for (name, hash) in map(&value["files"])? {
            wanted.insert(format!("payload/variants/{variant}/{name}"), Some(hash));
        }
    }
    for (name, hash) in map(&lock["integration"])? {
        if name == LEGACY_DISPLAY_HELPER {
            continue;
        }
        wanted.insert(format!("integration/{name}"), Some(hash));
    }
    Ok(wanted)
}
pub fn verify(bundle: &Path) -> Result<PathBuf> {
    let bundle = bundle.canonicalize()?;
    files::directory(&bundle, false)?;
    let lock = manifest(None)?;
    require(
        runtime::value(&bundle.join("bundle.json"))? == lock,
        "The patch bundle does not match this installer version.",
    )?;
    for (name, hash) in wanted(&lock)? {
        if let Some(hash) = hash {
            let file = tx::owned_file(&bundle, &bundle.join(name))?;
            require(
                file.metadata()?.len() <= 32 * 1024 * 1024 && files::digest(file)? == hash,
                "Fenix patch checksum mismatch.",
            )?;
        }
    }
    Ok(bundle)
}
pub fn obtain(cache: &Path, supplied: Option<&str>, ctx: &Context) -> Result<PathBuf> {
    if let Some(supplied) = supplied.filter(|s| !s.is_empty()) {
        let path = files::expand(supplied);
        require(
            path.is_absolute() && supplied.len() <= 4096,
            "Select the extracted Fenix patch release directory.",
        )?;
        return verify(&path);
    }
    let lock = manifest(None)?;
    let release = resources::json("compat/fenix/release.json")?;
    files::private_dir(cache)?;
    let target = cache.join(string(&lock, "version")?);
    if files::exists(&target) {
        return verify(&target);
    }
    ctx.progress("Das geprüfte Fenix-Paket wird heruntergeladen …");
    let archive = bootstrap::download(
        string(&release, "url")?,
        string(&release, "sha256")?,
        &cache.join(format!("{}.zip", string(&lock, "version")?)),
        &ctx.cancel,
        |_| {},
    )?;
    let stage = tx::new_directory(cache, ".fenix-extract-")?;
    let result = (|| {
        let wanted = wanted(&lock)?;
        let mut zipped = zip::ZipArchive::new(File::open(&archive)?)
            .map_err(|_| Error::Invalid("Invalid Fenix release archive."))?;
        require(zipped.len() < 10000, "Invalid Fenix release archive.")?;
        let mut seen = BTreeSet::new();
        let prefix = format!("{}/", string(&release, "archive_root")?);
        for i in 0..zipped.len() {
            ctx.interrupted()?;
            let mut entry = zipped
                .by_index(i)
                .map_err(|_| Error::Invalid("Invalid Fenix archive member."))?;
            let name = entry
                .name()
                .strip_prefix(&prefix)
                .unwrap_or(entry.name())
                .to_string();
            if !wanted.contains_key(&name) {
                continue;
            }
            require(
                seen.insert(name.clone())
                    && entry.size() <= 32 * 1024 * 1024
                    && entry.unix_mode().is_none_or(|v| v & 0o170000 != 0o120000),
                "Invalid file in the Fenix release archive.",
            )?;
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            require(
                bytes.len() <= 32 * 1024 * 1024,
                "Fenix archive member is too large.",
            )?;
            write(&contained(&stage, &name)?, &bytes, 0o600)?;
        }
        require(
            seen == wanted.keys().cloned().collect(),
            "Incomplete Fenix release archive.",
        )?;
        verify(&stage)?;
        tx::publish(&stage, &target)?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
pub fn script(name: &str) -> Result<&'static [u8]> {
    require(LAUNCH_FILES.contains(&name), "Unknown launch script.")?;
    let bytes = resources::asset(&format!("scripts/runtime/{name}"))
        .ok_or(Error::Invalid("Missing current Flightdeck launch script."))?;
    require(
        files::sha256(bytes)
            == resources::json("compat/bootstrap.lock.json")?["runtime_scripts"]["files"][name],
        "The launch script differs from the verified Flightdeck release.",
    )?;
    Ok(bytes)
}
pub fn accepted_script(
    name: &str,
    hash: &str,
    lock: &Value,
    upgrade: Option<&str>,
) -> Result<bool> {
    let scripts = resources::json("compat/bootstrap.lock.json")?["runtime_scripts"].clone();
    Ok(scripts["files"][name] == hash
        || scripts["upgrade_from"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|v| v[name] == hash)
        || lock["accepted_scripts"][name]
            .as_array()
            .into_iter()
            .flatten()
            .any(|v| v == hash)
        || upgrade.is_some_and(|v| lock["previous_releases"][v]["integration"][name] == hash))
}
pub fn overlay(
    prefix: &Path,
    runner: &Path,
    bundle: &Path,
    variant: Option<&str>,
    ctx: &Context,
) -> Result<()> {
    let lock = manifest(variant)?;
    let payload = if let Some(v) = variant {
        bundle.join("payload/variants").join(v)
    } else {
        bundle.join("payload")
    };
    for (name, hash) in map(&lock["files"])? {
        ctx.interrupted()?;
        let bytes = files::read(&contained(&payload, &name)?, 32 * 1024 * 1024)?;
        require(
            files::sha256(&bytes) == hash,
            "Fenix patch checksum mismatch.",
        )?;
        write(
            &contained(runner, &name)?,
            &bytes,
            if name.contains("/bin/") { 0o755 } else { 0o644 },
        )?;
        if name.starts_with("files/lib/wine/x86_64-windows/") {
            write(
                &contained(
                    prefix,
                    &format!(
                        "drive_c/windows/system32/{}",
                        Path::new(&name)
                            .file_name()
                            .and_then(|v| v.to_str())
                            .ok_or(Error::Invalid("Invalid DLL path."))?
                    ),
                )?,
                &bytes,
                0o644,
            )?;
        }
    }
    for name in ["FenixWindowGuard.exe", "FenixMCDURefresh.exe"] {
        if let Some(hash) = lock["integration"][name].as_str() {
            let bytes = files::read(&bundle.join("integration").join(name), 32 * 1024 * 1024)?;
            require(
                files::sha256(&bytes) == hash,
                "Fenix helper checksum mismatch.",
            )?;
            write(
                &contained(prefix, &format!("drive_c/windows/system32/{name}"))?,
                &bytes,
                0o644,
            )?;
        }
    }
    verify_runner(runner, &lock, true)
}
