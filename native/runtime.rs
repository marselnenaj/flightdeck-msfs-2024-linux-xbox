// SPDX-License-Identifier: MIT
//! Runtime discovery, persisted selection, readiness and local save backups.
use crate::{Error, Result, error::require, files, games::Game};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
pub fn validate(value: &str) -> Result<PathBuf> {
    require(
        !value.trim().is_empty() && value.len() <= 4096,
        "Bitte einen vorbereiteten Runtimeordner auswählen.",
    )?;
    let path = files::expand(value);
    require(path.is_absolute(), "Der Runtimepfad muss absolut sein.")?;
    let path = path.canonicalize()?;
    require(
        path.is_dir() && path.join("tools/play-msfs.sh").is_file(),
        "Hier fehlt tools/play-msfs.sh. Bitte zuerst die Runtime vorbereiten.",
    )?;
    require(
        path.metadata()?.uid() == files::uid(),
        "Bitte eine Runtime im eigenen Benutzerkonto auswählen.",
    )?;
    Ok(path)
}
pub fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
}
pub fn wine(runner: &Path) -> PathBuf {
    let p = runner.join("files/bin/wine64");
    if p.is_file() {
        p
    } else {
        runner.join("files/bin/wine")
    }
}
pub fn value(path: &Path) -> Result<Value> {
    files::json(path, 1024 * 1024)
}
pub fn component_state(root: &Path) -> &'static str {
    crate::components::state(Some(root))
}
pub fn checks(root: Option<&Path>) -> Vec<Value> {
    let game = root.map(Game::for_runtime).transpose();
    let mut checks = Vec::new();
    if let Err(error) = &game {
        checks.push(
            json!({"id":"version","label":"MSFS-Version","ok":false,"detail":error.to_string()}),
        );
    }
    let game = game.ok().flatten().unwrap_or(Game::Msfs2024);
    if let Some(root) = root
        && let Err(error) = crate::proton::check(root)
    {
        checks.push(
            json!({"id":"proton","label":"Proton-Version","ok":false,"detail":error.to_string()}),
        );
    }
    let game_path = format!("games/{}/{}", game.directory(), game.executable());
    for (id, label, name, exec) in [
        ("launcher", "Startprogramm", "tools/play-msfs.sh", true),
        (
            "game",
            "Eigenes MSFS-PC-Spielpaket",
            game_path.as_str(),
            false,
        ),
        (
            "prefix",
            "Wine-Umgebung",
            "local/msfs-prefix/system.reg",
            false,
        ),
        (
            "bridge",
            "Kompatibilitätsbibliothek",
            "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll",
            false,
        ),
    ] {
        let ok = root.is_some_and(|r| {
            let path = r.join(name);
            path.is_file() && (!exec || executable(&path))
        });
        checks.push(json!({"id":id,"label":label,"ok":ok,"detail":if ok {"Vorhanden"} else {"Runtime vorbereiten oder Pfad prüfen"}}));
    }
    let private_ok = root.is_some_and(|r| {
        files::directory(&r.join("private"), false).is_ok()
            && (!files::exists(&r.join("private/play.lock"))
                || files::open_at(rustix::fs::CWD, r.join("private/play.lock"), false, false)
                    .is_ok())
    });
    checks.push(json!({"id":"private","label":"Privater Datenordner","ok":private_ok,"detail":if private_ok {"Vorhanden"} else {"Privater Datenordner fehlt"}}));
    let components = root.map(component_state).unwrap_or("invalid");
    let ok = private_ok && !["interrupted", "pending", "invalid"].contains(&components);
    let detail = match components {
        "interrupted" => {
            "Unterbrochenes Komponentenupdate: flightdeck --refresh-components erneut ausführen."
        }
        "pending" => {
            "Neue Runtime-Komponenten bereit. Flightdeck neu öffnen oder flightdeck --refresh-components ausführen."
        }
        "invalid" => "Die Runtime-Komponentenbeschreibung ist ungültig.",
        "custom" => "Eigene Runtime-Komponenten",
        "unmanaged" => "Ältere Runtime ohne verwaltetes Komponentenupdate",
        _ => "Geprüft",
    };
    checks.push(json!({"id":"component_update","label":"Runtime-Komponentenupdate","ok":ok,"detail":detail}));
    if let Some(root) = root {
        for (name, id, label, message) in [
            (
                "fenix-linux-patch.json",
                "fenix_setup",
                "Fenix-Einrichtung",
                "Fenix-Einrichtung unvollständig. Unter Mods reparieren oder wiederherstellen.",
            ),
            (
                "gsx-setup.json",
                "gsx_setup",
                "GSX-Einrichtung",
                "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.",
            ),
        ] {
            let path = root.join("private").join(name);
            if files::exists(&path) {
                let complete = value(&path).is_ok_and(|v| {
                    v["state"]
                        == if id == "gsx_setup" {
                            "ready"
                        } else {
                            "installed"
                        }
                });
                checks.push(json!({"id":id,"label":label,"ok":complete,"detail":if complete {"Geprüft"} else {message}}));
            }
        }
    }
    checks
}
pub fn ready(root: &Path) -> bool {
    checks(Some(root)).iter().all(|v| v["ok"] == true)
}
pub fn versions(runtime: Option<&Path>, known: &BTreeMap<String, PathBuf>) -> Value {
    let mut result = serde_json::Map::new();
    for game in Game::ALL {
        let default = files::xdg("XDG_DATA_HOME", ".local/share")
            .join("flightdeck/runtimes")
            .join(game.id());
        let mut chosen = json!({"path":"","installed":false,"ready":false});
        for candidate in [
            runtime,
            known.get(game.id()).map(PathBuf::as_path),
            Some(default.as_path()),
        ]
        .into_iter()
        .flatten()
        {
            if validate(&candidate.to_string_lossy()).is_ok()
                && Game::for_runtime(candidate).ok() == Some(game)
            {
                let healthy = ready(candidate);
                if chosen["installed"] != true || (healthy && chosen["ready"] != true) {
                    chosen = json!({"path":candidate,"installed":true,"ready":healthy});
                }
                if healthy && Some(candidate) == runtime {
                    break;
                }
            }
        }
        result.insert(game.id().into(), chosen);
    }
    Value::Object(result)
}
pub fn regular_files(root: &Path, limit: usize) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut result = Vec::new();
    let mut visited = 0;
    while let Some(folder) = pending.pop() {
        if folder.symlink_metadata()?.file_type().is_symlink() {
            continue;
        }
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            visited += 1;
            require(visited <= limit, "Der Ordner enthält zu viele Dateien.")?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                result.push(entry.path());
            }
        }
    }
    result.sort();
    Ok(result)
}
pub fn saves(root: Option<&Path>, idle: bool) -> Value {
    let mut value = json!({"mode":"unavailable","available":false,"bytes":0,"files":0,"backups":0,"can_backup":false,"last_backup":null});
    let Some(root) = root else {
        return value;
    };
    let folder = root.join("private/local-saves");
    if root.join("private/local-saves.enabled").is_file()
        && files::directory(&folder, false).is_ok()
        && let Ok(paths) = regular_files(&folder, 100_000)
    {
        let paths = paths
            .iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| !s.ends_with(".lock") && !s.starts_with(".tmp"))
            })
            .collect::<Vec<_>>();
        value["mode"] = json!("local");
        value["available"] = json!(true);
        value["files"] = json!(paths.len());
        value["bytes"] = json!(
            paths
                .iter()
                .filter_map(|p| p.metadata().ok())
                .map(|m| m.len())
                .sum::<u64>()
        );
        value["can_backup"] = json!(idle && !paths.is_empty());
    }
    let backup = root.join("private/save-backups");
    if files::directory(&backup, false).is_ok()
        && let Ok(entries) = fs::read_dir(backup)
    {
        let mut backups = entries
            .take(100_000)
            .filter_map(|v| v.ok())
            .filter(|e| {
                e.file_name().to_string_lossy().starts_with("backup-")
                    && e.file_type().is_ok_and(|t| t.is_dir())
                    && e.path().join("manifest.json").is_file()
            })
            .collect::<Vec<_>>();
        backups.sort_by_key(|e| e.file_name());
        value["backups"] = json!(backups.len());
        if let Some(last) = backups.last() {
            let at = last
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(chrono::DateTime::<chrono::Utc>::from)
                .map(|d| d.to_rfc3339());
            value["last_backup"] =
                json!({"name":last.file_name().to_string_lossy(),"created_at":at});
        }
    }
    value
}
pub fn backup(root: &Path, allow_empty: bool) -> Result<Value> {
    let source = root.join("private/local-saves");
    if allow_empty && !files::exists(&source) {
        return Ok(Value::Null);
    }
    let source_fd = files::directory(&source, false)?;
    let destination = root.join("private/save-backups");
    let destination_fd = files::private_dir(&destination)?;
    let stage = destination.join(format!(".backup-{}", uuid::Uuid::new_v4().simple()));
    files::private_dir(&stage)?;
    let result = (|| {
        let mut hashes = BTreeMap::new();
        for path in regular_files(&source, 100_000)? {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.ends_with(".lock") || name.starts_with(".tmp") {
                continue;
            }
            let relative = path
                .strip_prefix(&source)
                .map_err(|_| Error::Invalid("Ungültiger Speicherpfad."))?;
            let relative = relative
                .to_str()
                .ok_or(Error::Invalid("Ungültiger Speicherpfad."))?;
            let mut input = files::beneath(&source_fd, relative)?;
            let target = stage.join("data").join(relative);
            files::private_dir(
                target
                    .parent()
                    .ok_or(Error::Invalid("Ungültiger Speicherpfad."))?,
            )?;
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
            let mut hash = sha2::Sha256::default();
            use sha2::Digest;
            let mut buffer = [0_u8; 65536];
            loop {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
                out.write_all(&buffer[..n])?;
            }
            out.sync_all()?;
            hashes.insert(relative.to_string(), hex::encode(hash.finalize()));
        }
        if hashes.is_empty() {
            require(
                allow_empty,
                "Es sind noch keine lokalen Spielstände vorhanden.",
            )?;
            return Ok(Value::Null);
        }
        files::atomic_json(
            &stage.join("manifest.json"),
            &json!({"schema":1,"created_at":files::now(),"files":hashes}),
        )?;
        fn sync_tree(folder: &Path) -> Result<()> {
            for entry in fs::read_dir(folder)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    sync_tree(&entry.path())?;
                }
            }
            files::directory(folder, true)?.sync_all()?;
            Ok(())
        }
        sync_tree(&stage)?;
        let name = chrono::Utc::now()
            .format("backup-%Y%m%d-%H%M%S-%6f")
            .to_string();
        rustix::fs::renameat_with(
            &destination_fd,
            stage
                .file_name()
                .ok_or(Error::Invalid("Ungültiger Speicherpfad."))?,
            &destination_fd,
            &name,
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        destination_fd.sync_all()?;
        Ok(json!({"ok":true,"backup":{"name":name,"created_at":files::now()},"files":hashes.len()}))
    })();
    if stage.is_dir() {
        fs::remove_dir_all(stage)?;
    }
    result
}
