// SPDX-License-Identifier: MIT
//! Hash-bound component updates compatible with the existing recovery journal.
use crate::{
    Error, Result, backend::Launcher, error::require, files, resources, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
pub const FILES: [&str; 6] = [
    "bin/xodus-cli",
    "bin/xodus-service",
    "bin/flightdeck-connected-storage.exe",
    "runtime/xgameruntime.dll",
    "builtin/x86_64-windows/xodus_store_test.dll",
    "builtin/x86_64-unix/xodus_store_test.so",
];
pub const SCRIPTS: [&str; 6] = [
    "launch-msfs.sh",
    "play-msfs.sh",
    "runtime-env.sh",
    "xodus.sh",
    "xodus-service.sh",
    "xodus-wine-launch",
];
const MANIFEST: &str = "import-manifest.json";
const JOURNAL: &str = "component-update.json";
type Hashes = BTreeMap<String, String>;
pub fn targets(root: &Path) -> Vec<Vec<PathBuf>> {
    let system32 = root.join("local/msfs-prefix/drive_c/windows/system32");
    vec![
        vec![root.join(FILES[0])],
        vec![root.join(FILES[1])],
        vec![root.join(FILES[2])],
        vec![system32.join("xgameruntime.dll")],
        vec![
            root.join("local/store-runtime/x86_64-windows/xodus_store_test.dll"),
            system32.join("xodus_store_test.dll"),
        ],
        vec![root.join("local/store-runtime/x86_64-unix/xodus_store_test.so")],
    ]
}
fn hashes(value: &Value, names: &[&str]) -> Result<Hashes> {
    let values: Hashes = serde_json::from_value(value.clone())?;
    require(
        values.len() == names.len()
            && names
                .iter()
                .all(|n| values.get(*n).is_some_and(|v| files::hex_digest(v))),
        "Die Runtime hat keine vollständig geprüfte Komponentenliste.",
    )?;
    Ok(values)
}
fn read_manifest(root: &Path, path: &Path) -> Result<(Value, Hashes)> {
    let record: Value = serde_json::from_slice(&files::read_file(
        tx::owned_file(root, path)?,
        2 * 1024 * 1024,
    )?)?;
    require(
        record["format"] == 1,
        "Die Runtime-Komponentenbeschreibung ist ungültig.",
    )?;
    let installed = hashes(&record["artifacts"]["files"], &FILES)?;
    Ok((record, installed))
}
fn overlay_set(actual: &Hashes, releases: &[Hashes]) -> Result<bool> {
    let overlay = resources::json("compat/fenix/bundle.json")?;
    let mut known: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for name in ["launch-msfs.sh", "xodus-wine-launch"] {
        let mut values = vec![crate::backend::string(&overlay["integration"], name)?.to_string()];
        let accepted = overlay["accepted_scripts"][name]
            .as_array()
            .ok_or(Error::Invalid("Ungültige Fenix-Skriptliste."))?;
        for value in accepted {
            values.push(
                value
                    .as_str()
                    .filter(|v| files::hex_digest(v))
                    .ok_or(Error::Invalid("Ungültige Fenix-Skriptliste."))?
                    .into(),
            );
        }
        if let Some(previous) = overlay["previous_releases"].as_object() {
            for item in previous.values() {
                values.push(crate::backend::string(&item["integration"], name)?.to_string());
            }
        }
        known.insert(name, values);
    }
    Ok(releases.iter().any(|release| {
        actual.len() == release.len()
            && release.iter().all(|(name, expected)| {
                actual.get(name).is_some_and(|value| {
                    value == expected || known.get(name.as_str()).is_some_and(|v| v.contains(value))
                })
            })
    }))
}
fn scripts(root: &Path, record: &Value, lock: &Value) -> Result<Option<(Hashes, Hashes)>> {
    if lock["runtime_scripts"].is_null() {
        return Ok(Some((Hashes::new(), Hashes::new())));
    }
    let current = hashes(&lock["runtime_scripts"]["files"], &SCRIPTS)?;
    let mut previous = lock["runtime_scripts"]["upgrade_from"]
        .as_array()
        .ok_or(Error::Invalid(
            "Die Runtime-Skriptbeschreibung ist ungültig.",
        ))?
        .iter()
        .map(|v| hashes(v, &SCRIPTS))
        .collect::<Result<Vec<_>>>()?;
    let mut actual = Hashes::new();
    for name in SCRIPTS {
        let file = tx::owned_file(root, &root.join("tools").join(name))?;
        require(
            file.metadata()?.mode() & 0o100 != 0,
            "Ein Runtime-Skript ist nicht ausführbar.",
        )?;
        actual.insert(name.into(), files::digest(file)?);
    }
    previous.push(current.clone());
    if (!record["runtime_files"].is_null() && hashes(&record["runtime_files"], &SCRIPTS)? != actual)
        || (!previous.contains(&actual) && !overlay_set(&actual, &previous)?)
    {
        return Ok(None);
    }
    Ok(Some((actual, current)))
}
fn verify_targets(root: &Path, hashes: &Hashes) -> Result<()> {
    for (name, paths) in FILES.iter().zip(targets(root)) {
        for path in paths {
            require(
                files::digest(tx::owned_file(root, &path)?)? == hashes[*name],
                "Eine Runtime-Komponente wurde verändert. Es wird nichts überschrieben.",
            )?;
        }
    }
    Ok(())
}
fn verify_scripts(root: &Path, hashes: &Hashes, backup: bool) -> Result<()> {
    for (name, expected) in hashes {
        let path = if backup {
            root.join(format!("script-{name}"))
        } else {
            root.join("tools").join(name)
        };
        let file = tx::owned_file(root, &path)?;
        require(
            file.metadata()?.mode() & 0o100 != 0 && files::digest(file)? == *expected,
            "Ein Runtime-Skript wurde verändert. Es wird nichts überschrieben.",
        )?;
    }
    Ok(())
}
pub fn state(root: Option<&Path>) -> &'static str {
    let Some(root) = root else {
        return "unmanaged";
    };
    if files::exists(&root.join("private").join(JOURNAL)) {
        return "interrupted";
    }
    let manifest = root.join("private").join(MANIFEST);
    if !files::exists(&manifest) {
        return "unmanaged";
    }
    let result = (|| -> Result<&'static str> {
        let record: Value = serde_json::from_slice(&files::read_file(
            tx::owned_file(root, &manifest)?,
            2 * 1024 * 1024,
        )?)?;
        require(
            record["format"] == 1,
            "Die Runtime-Komponentenbeschreibung ist ungültig.",
        )?;
        let installed: Hashes = serde_json::from_value(record["artifacts"]["files"].clone())?;
        require(
            !installed.is_empty() && installed.values().all(|v| files::hex_digest(v)),
            "Ungültige Komponentenliste.",
        )?;
        let lock = resources::json("compat/bootstrap.lock.json")?;
        let current = hashes(&lock["native"]["files"], &FILES)?;
        let previous = lock["native"]["upgrade_from"]
            .as_array()
            .ok_or(Error::Invalid("Ungültige Komponentenliste."))?
            .iter()
            .map(|v| hashes(v, &FILES))
            .collect::<Result<Vec<_>>>()?;
        if installed != current && !previous.contains(&installed) {
            return Ok("custom");
        }
        let scripts = scripts(root, &record, &lock)?;
        Ok(if installed != current {
            "pending"
        } else if let Some((old, new)) = scripts {
            if old == new { "current" } else { "pending" }
        } else {
            "custom"
        })
    })();
    result.unwrap_or("invalid")
}
fn cleanup(private: &Path, backup: &Path) -> Result<()> {
    if files::exists(backup) {
        files::directory(backup, true)?;
        fs::remove_dir_all(backup)?;
    }
    fs::remove_file(private.join(JOURNAL))?;
    files::directory(private, true)?.sync_all()?;
    Ok(())
}
pub fn recover(root: &Path) -> Result<()> {
    let private = root.join("private");
    let path = private.join(JOURNAL);
    if !files::exists(&path) {
        return Ok(());
    }
    let journal: Value = serde_json::from_slice(&files::read_file(
        tx::owned_file(root, &path)?,
        1024 * 1024,
    )?)?;
    let name = journal["backup"]
        .as_str()
        .ok_or(Error::Invalid("Ungültiges Komponentenjournal."))?;
    require(
        matches!(journal["format"].as_u64(), Some(1 | 2))
            && name.strip_prefix(".component-").is_some_and(|v| {
                v.len() == 32
                    && v.bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            }),
        "Das unterbrochene Komponentenupdate hat ungültige Metadaten.",
    )?;
    let before = hashes(&journal["before"], &FILES)?;
    let after = hashes(&journal["after"], &FILES)?;
    let (before_scripts, after_scripts) = if journal["format"] == 2 {
        (
            hashes(&journal["scripts_before"], &SCRIPTS)?,
            hashes(&journal["scripts_after"], &SCRIPTS)?,
        )
    } else {
        (Hashes::new(), Hashes::new())
    };
    let manifest_hash = journal["manifest_sha256"]
        .as_str()
        .filter(|v| files::hex_digest(v))
        .ok_or(Error::Invalid("Ungültiges Komponentenjournal."))?;
    let backup = private.join(name);
    let manifest = private.join(MANIFEST);
    if let Ok((current, current_hashes)) = read_manifest(root, &manifest)
        && current_hashes == after
        && (after_scripts.is_empty() || current["runtime_files"] == json!(after_scripts))
        && verify_targets(root, &after).is_ok()
        && verify_scripts(root, &after_scripts, false).is_ok()
    {
        return cleanup(&private, &backup);
    }
    files::directory(&backup, true)?;
    require(
        files::digest(tx::owned_file(root, &backup.join(MANIFEST))?)? == manifest_hash,
        "Die Sicherung des Komponentenupdates wurde verändert.",
    )?;
    verify_scripts(&backup, &before_scripts, true)?;
    for name in before_scripts.keys() {
        tx::owned_file(root, &root.join("tools").join(name))?;
    }
    for (i, paths) in targets(root).iter().enumerate() {
        for (j, path) in paths.iter().enumerate() {
            require(
                files::digest(tx::owned_file(root, &backup.join(format!("{i}-{j}")))?)?
                    == before[FILES[i]],
                "Die Sicherung des Komponentenupdates wurde verändert.",
            )?;
            tx::owned_file(root, path)?;
        }
    }
    let cancel = AtomicBool::new(false);
    for (i, paths) in targets(root).iter().enumerate() {
        for (j, path) in paths.iter().enumerate() {
            tx::copy(
                tx::owned_file(root, &backup.join(format!("{i}-{j}")))?,
                path,
                Some(&before[FILES[i]]),
                &cancel,
            )?;
        }
    }
    for (name, expected) in &before_scripts {
        tx::copy(
            tx::owned_file(root, &backup.join(format!("script-{name}")))?,
            &root.join("tools").join(name),
            Some(expected),
            &cancel,
        )?;
    }
    tx::copy(
        tx::owned_file(root, &backup.join(MANIFEST))?,
        &manifest,
        Some(manifest_hash),
        &cancel,
    )?;
    cleanup(&private, &backup)
}
pub fn refresh_with(root: &Path, native: &Path, lock: &Value) -> Result<bool> {
    files::directory(&root.join("private"), true)?;
    recover(root)?;
    require(
        !files::exists(&root.join("private/proton-switch.json")),
        "Der Proton-Wechsel muss zuerst abgeschlossen werden.",
    )?;
    let new = hashes(&lock["native"]["files"], &FILES)?;
    let previous = lock["native"]["upgrade_from"]
        .as_array()
        .ok_or(Error::Invalid("Ungültige Komponentenliste."))?
        .iter()
        .map(|v| hashes(v, &FILES))
        .collect::<Result<Vec<_>>>()?;
    for (name, expected) in &new {
        require(
            files::digest(tx::owned_file(native, &native.join(name))?)? == *expected,
            "Das geprüfte neue Kompatibilitätspaket fehlt. Bitte den vollständigen Launcher installieren.",
        )?;
    }
    let private = root.join("private");
    let manifest = private.join(MANIFEST);
    let (mut record, old) = read_manifest(root, &manifest)?;
    verify_targets(root, &old)?;
    require(
        old == new || previous.contains(&old),
        "Diese Runtime verwendet eigene oder unbekannte Komponenten. Das automatische Update ist dafür nicht freigegeben.",
    )?;
    let (old_scripts, new_scripts) = scripts(root, &record, lock)?.unwrap_or_default();
    for (name, expected) in &new_scripts {
        require(
            resources::asset(&format!("scripts/runtime/{name}"))
                .is_some_and(|v| files::sha256(v) == *expected),
            "Ein neues Runtime-Skript stimmt nicht mit dem geprüften Launcher überein.",
        )?;
    }
    if new_scripts.get("play-msfs.sh").is_some_and(|hash| {
        resources::asset("scripts/runtime/play-msfs.sh")
            .is_some_and(|bytes| files::sha256(bytes) == *hash)
    }) {
        resources::ensure_helper(root)?;
    }
    if old == new && old_scripts == new_scripts {
        return Ok(false);
    }
    let backup = tx::new_directory(&private, ".component-")?;
    let cancel = AtomicBool::new(false);
    let mut started = false;
    let result = (|| {
        for (i, paths) in targets(root).iter().enumerate() {
            for (j, path) in paths.iter().enumerate() {
                tx::copy(
                    tx::owned_file(root, path)?,
                    &backup.join(format!("{i}-{j}")),
                    Some(&old[FILES[i]]),
                    &cancel,
                )?;
            }
        }
        for (name, expected) in &old_scripts {
            tx::copy(
                tx::owned_file(root, &root.join("tools").join(name))?,
                &backup.join(format!("script-{name}")),
                Some(expected),
                &cancel,
            )?;
        }
        let manifest_hash = files::digest(tx::owned_file(root, &manifest)?)?;
        tx::copy(
            tx::owned_file(root, &manifest)?,
            &backup.join(MANIFEST),
            Some(&manifest_hash),
            &cancel,
        )?;
        let mut journal = json!({"format":1,"backup":backup.file_name().and_then(|v|v.to_str()),"before":old,"after":new,"manifest_sha256":manifest_hash});
        if !new_scripts.is_empty() {
            journal["format"] = json!(2);
            journal["scripts_before"] = json!(old_scripts);
            journal["scripts_after"] = json!(new_scripts);
        }
        files::atomic_json(&private.join(JOURNAL), &journal)?;
        started = true;
        for (name, paths) in FILES.iter().zip(targets(root)) {
            for path in paths {
                tx::copy(
                    tx::owned_file(native, &native.join(name))?,
                    &path,
                    Some(&new[*name]),
                    &cancel,
                )?;
            }
        }
        for name in new_scripts.keys() {
            let path = root.join("tools").join(name);
            files::atomic(
                &path,
                resources::asset(&format!("scripts/runtime/{name}"))
                    .ok_or(Error::Invalid("Ein Startskript fehlt."))?,
            )?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        record["artifacts"] = json!({"format":1,"files":new,"features":lock["native"]["features"],"cli_features":lock["native"]["cli_features"],"native_archive_sha256":lock["native"]["archive_sha256"]});
        if !new_scripts.is_empty() {
            record["runtime_files"] = json!(new_scripts);
        }
        files::atomic_json(&manifest, &record)?;
        verify_targets(root, &new)?;
        verify_scripts(root, &new_scripts, false)?;
        cleanup(&private, &backup)?;
        Ok(true)
    })();
    if result.is_err() {
        if started {
            recover(root)?;
        } else {
            let _ = fs::remove_dir_all(backup);
        }
    }
    result
}
pub fn refresh(app: &Launcher) -> Result<bool> {
    let mut state = app.lock();
    Launcher::idle(&mut state)?;
    let root = state
        .runtime
        .as_deref()
        .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?;
    let _lease = files::Lease::acquire(&root.join("private/play.lock"), true)?;
    let lock = resources::json("compat/bootstrap.lock.json")?;
    let native=crate::bootstrap::native_path(&lock)?.ok_or(Error::Invalid("Das geprüfte neue Kompatibilitätspaket fehlt. Bitte den vollständigen Launcher installieren."))?;
    refresh_with(root, &native, &lock)
}
