// SPDX-License-Identifier: MIT
//! Repair recurring .NET damage in an independent profile before starting add-ons.
use crate::{
    Error, Result,
    backend::{Context, string},
    bootstrap,
    error::require,
    fenix, files, framework, framework_repair, gsx,
    log_reader::regex,
    owned_tree, process, proton, transaction as tx,
    wine::StagedWine,
    wine_processes,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const JOURNAL: &str = "private/framework-repair-pending.json";
const INVALID: &str = "Die unterbrochene .NET-Reparatur passt nicht mehr zur Spielumgebung. Die vorhandenen Sicherungen bleiben erhalten.";
pub fn healthy(prefix: &Path) -> bool {
    framework::status(prefix).ready
        && ["Framework", "Framework64"].iter().all(|architecture| {
            let check = || -> Result<bool> {
                let path = framework::framework_path(prefix, architecture, "csc.exe")?;
                let mut file = files::open_at(rustix::fs::CWD, path, false, false)?;
                let mut magic = [0; 2];
                file.read_exact(&mut magic)?;
                Ok(magic == *b"MZ")
            };
            check().unwrap_or(false)
        })
}
fn managed_addons(root: &Path) -> Result<bool> {
    let mut enabled = false;
    if files::exists(&root.join(fenix::MARKER)) {
        let state: Value = files::json(&root.join(fenix::MARKER), 2 * 1024 * 1024)?;
        require(
            state["state"] == "installed",
            "Fenix setup is incomplete. Restore or finish it in Flightdeck before starting MSFS.",
        )?;
        enabled = true;
    }
    if files::exists(&root.join(gsx::MARKER)) {
        let state: Value = files::json(&root.join(gsx::MARKER), 2 * 1024 * 1024)?;
        require(
            state["state"] == "ready",
            "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.",
        )?;
        enabled = true;
    }
    Ok(enabled)
}
/// Complete only the previously verified exchange, including after a crash
/// between the rename and journal removal. The original add-on restore point
/// and runner remain unchanged throughout this repair.
pub fn recover(root: &Path) -> Result<bool> {
    if !files::exists(&root.join(JOURNAL)) {
        return Ok(false);
    }
    let record: Value = crate::cloud::json(&files::read(&root.join(JOURNAL), 8192)?)?;
    require(
        record["schema"] == 1
            && record.as_object().is_some_and(|v| v.len() == 5)
            && record["backup"].as_str().is_some_and(|p| {
                regex(r"^local/\.framework-repair-[a-f0-9]{32}/prefix$").is_match(p)
            }),
        INVALID,
    )?;
    let prefix = root.join("local/msfs-prefix");
    let backup = root.join(string(&record, "backup")?);
    proton::managed(root, &prefix)?;
    proton::managed(root, &backup)?;
    require(
        root.join("runner").canonicalize()?.to_str() == record["runner"].as_str(),
        INVALID,
    )?;
    wine_processes::idle(&prefix)?;
    wine_processes::idle(&backup)?;
    let active = json!(owned_tree::identity(&prefix)?);
    let saved = json!(owned_tree::identity(&backup)?);
    if active == record["before"] && saved == record["after"] {
        require(healthy(&backup), INVALID)?;
        tx::exchange(&prefix, &backup)?;
    } else {
        require(
            active == record["after"] && saved == record["before"] && healthy(&prefix),
            INVALID,
        )?;
    }
    files::atomic_json(&root.join("private/framework-repair.json"), &record)?;
    fs::remove_file(root.join(JOURNAL))?;
    files::directory(&root.join("private"), true)?.sync_all()?;
    Ok(true)
}
fn repair(
    ctx: &Context,
    prepare: impl FnOnce(&mut StagedWine<'_>) -> Result<()>,
) -> Result<PathBuf> {
    let root = ctx.root()?;
    proton::check(root)?;
    let prefix = root.join("local/msfs-prefix");
    proton::managed(root, &prefix)?;
    wine_processes::idle(&prefix)?;
    let before = owned_tree::identity(&prefix)?;
    let runner = root.join("runner").canonicalize()?;
    let size = tx::prefix_size(&prefix, &ctx.cancel)?;
    require(
        tx::free_bytes(root)? > size.saturating_add(1024 * 1024 * 1024),
        "Für die automatische .NET-Reparatur fehlt Speicherplatz für eine Sicherung der Spielumgebung.",
    )?;
    ctx.progress("Microsoft .NET Framework wird automatisch repariert. Die bisherige Spielumgebung bleibt gesichert …");
    let stage = tx::new_directory(&root.join("local"), ".framework-repair-")?;
    let copied = stage.join("prefix");
    process::copy_tree(&prefix, &copied, &ctx.cancel)?;
    tx::relocate_prefix_links(&prefix, &copied, &ctx.cancel)?;
    let drive = copied.join("dosdevices/c:");
    files::directory(&copied.join("dosdevices"), false)?;
    if files::exists(&drive) {
        require(fs::symlink_metadata(&drive)?.is_symlink(), INVALID)?;
        fs::remove_file(&drive)?;
    }
    std::os::unix::fs::symlink("../drive_c", drive)?;
    let mut wine = StagedWine::new(root, &copied, &runner, &stage.join("repair.log"), ctx)?;
    let result = prepare(&mut wine);
    let stopped = wine.stop();
    result.and(stopped)?;
    drop(wine);
    ctx.interrupted()?;
    require(
        healthy(&copied)
            && owned_tree::identity(&prefix)? == before
            && root.join("runner").canonicalize()? == runner,
        INVALID,
    )?;
    wine_processes::idle(&prefix)?;
    // Do not change the parent launch job to a permanent commit phase: after
    // this bounded swap the original launch can still be cancelled normally.
    let record = json!({"schema":1,"backup":copied.strip_prefix(root).map_err(|_|Error::Invalid(INVALID))?,"runner":runner,"before":before,"after":owned_tree::identity(&copied)?});
    files::atomic_json(&root.join(JOURNAL), &record)?;
    recover(root)?;
    Ok(copied)
}
pub fn ensure(ctx: &Context) -> Result<bool> {
    let outcome = (|| -> Result<bool> {
        let root = ctx.root()?;
        let _lease = ctx.lease()?;
        let recovered = recover(root)?;
        if !managed_addons(root)? || healthy(&root.join("local/msfs-prefix")) {
            return Ok(recovered);
        }
        repair(ctx, |wine| {
            framework_repair::prepare(
                wine,
                |name, url, hash| {
                    bootstrap::download(
                        url,
                        hash,
                        &root.join("private/fenix-downloads").join(name),
                        &ctx.cancel,
                        |_| {},
                    )
                },
                |message| ctx.progress(message),
            )
        })?;
        Ok(true)
    })();
    outcome.map_err(|error| {
        if matches!(error, Error::Cancelled) {
            error
        } else {
            Error::Framework(Box::new(error))
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Launcher;
    use std::os::unix::fs::PermissionsExt;
    fn write(path: &Path, bytes: &[u8]) {
        files::private_dir(path.parent().expect("parent")).expect("dir");
        files::atomic(path, bytes).expect("write");
    }
    fn fixture(t: &Path) -> Context {
        let root = t.join("runtime");
        for dir in [
            "private",
            "local/msfs-prefix/drive_c/windows/system32",
            "local/msfs-prefix/dosdevices",
            "runner/files/bin",
        ] {
            files::private_dir(&root.join(dir)).expect("dir");
        }
        write(&root.join("local/msfs-prefix/system.reg"), b"old registry");
        write(
            &root.join("runner/files/bin/wineserver"),
            b"#!/bin/sh\nexit 0\n",
        );
        fs::set_permissions(
            root.join("runner/files/bin/wineserver"),
            fs::Permissions::from_mode(0o700),
        )
        .expect("mode");
        let app = Launcher::new(t.join("state"), None).expect("launcher");
        app.lock().runtime = Some(root);
        app.reserve("test-framework", "repair", true)
            .expect("reservation")
    }
    fn healthy_fixture(prefix: &Path) {
        let key = r"Software\\Microsoft\\NET Framework Setup\\NDP\\v4\\Full";
        write(
            &prefix.join("system.reg"),
            format!(
                "[{key}]\n\"Release\"=dword:00080eb1\n[{}]\n\"Release\"=dword:00080eb1\n",
                key.replace("Software\\\\", "Software\\\\Wow6432Node\\\\")
            )
            .as_bytes(),
        );
        for arch in ["Framework", "Framework64"] {
            for name in ["clr.dll", "csc.exe"] {
                write(
                    &prefix
                        .join("drive_c/windows/Microsoft.NET")
                        .join(arch)
                        .join("v4.0.30319")
                        .join(name),
                    b"MZ synthetic fixture",
                );
            }
        }
    }
    #[test]
    fn repair_publishes_verified_copy_and_keeps_original_restore_points() {
        let t = tempfile::tempdir().expect("temp");
        let ctx = fixture(t.path());
        let root = ctx.root().expect("root");
        let prefix = root.join("local/msfs-prefix");
        write(&prefix.join("addon-settings"), b"keep settings");
        write(
            &root.join(fenix::MARKER),
            br#"{"state":"installed","previous_prefix":"original-preserved"}"#,
        );
        let marker = fs::read(root.join(fenix::MARKER)).expect("marker");
        let backup = repair(&ctx, |wine| {
            healthy_fixture(&wine.wine.prefix);
            Ok(())
        })
        .expect("repair");
        assert!(healthy(&prefix));
        assert_eq!(
            fs::read(backup.join("system.reg")).expect("old"),
            b"old registry"
        );
        assert_eq!(
            fs::read(prefix.join("addon-settings")).expect("settings"),
            b"keep settings"
        );
        assert_eq!(fs::read(root.join(fenix::MARKER)).expect("marker"), marker);
        assert!(!ensure(&ctx).expect("healthy no-op"));
        assert!(!root.join(JOURNAL).exists());
    }
    #[test]
    fn failed_repair_never_replaces_active_profile() {
        let t = tempfile::tempdir().expect("temp");
        let ctx = fixture(t.path());
        let root = ctx.root().expect("root");
        let before = owned_tree::identity(&root.join("local/msfs-prefix")).expect("identity");
        assert!(repair(&ctx, |_| Err(Error::Invalid("synthetic setup failure"))).is_err());
        assert_eq!(
            owned_tree::identity(&root.join("local/msfs-prefix")).expect("identity"),
            before
        );
        assert_eq!(
            fs::read(root.join("local/msfs-prefix/system.reg")).expect("registry"),
            b"old registry"
        );
        assert!(!root.join(JOURNAL).exists());
    }
    #[test]
    fn interrupted_exchange_finishes_once_and_rejects_replacement_backup() {
        let t = tempfile::tempdir().expect("temp");
        let ctx = fixture(t.path());
        let root = ctx.root().expect("root");
        let prefix = root.join("local/msfs-prefix");
        let backup = repair(&ctx, |wine| {
            healthy_fixture(&wine.wine.prefix);
            Ok(())
        })
        .expect("repair");
        let record = fs::read(root.join("private/framework-repair.json")).expect("record");
        files::atomic(&root.join(JOURNAL), &record).expect("pending");
        assert!(recover(root).expect("complete after exchange"));
        files::atomic(&root.join(JOURNAL), &record).expect("pending");
        fs::rename(&backup, backup.with_file_name("retained")).expect("retain");
        files::private_dir(&backup).expect("replacement");
        assert!(recover(root).is_err());
        assert!(healthy(&prefix));
    }
}
