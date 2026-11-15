// SPDX-License-Identifier: MIT
//! One local archive of the logs needed when Fenix does not start: the launch
//! entry, Flightdeck's Fenix logs and the logs Fenix writes in the profile.
//!
//! Unlike problem reports this keeps raw log text, so it stays local and the
//! person reviews it before sharing. Only .log/.txt/.json/.xml files are taken,
//! never symlinks, with a tail of large files and a bound on the total.
use crate::{Error, Result, backend::Launcher, error::require, fenix_autostart, files, process};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

const FILE_LIMIT: u64 = 4 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 300;
const KEPT: usize = 5;
const EXTENSIONS: &[&str] = &["log", "txt"];
// Activation and account data stay out even when named like a log.
const EXCLUDED: &[&str] = &["licen", "activation", "token", "credential", "password"];

pub struct Sources {
    pub prefix: PathBuf,
    pub folder: &'static str,
    /// Flightdeck's own files for this installation, with their archive names.
    pub own: Vec<(PathBuf, String)>,
    pub summary: Value,
}

fn wanted(path: &Path, extensions: &[&str]) -> bool {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase();
    path.extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| extensions.contains(&v.to_lowercase().as_str()))
        && !EXCLUDED.iter().any(|v| name.contains(v))
}

/// Regular files below `dir`, newest first, without following symlinks.
fn walk(dir: &Path, extensions: &[&str], found: &mut Vec<(PathBuf, std::time::SystemTime)>) {
    let mut queue = vec![(dir.to_path_buf(), 0)];
    let mut seen = 0;
    while let Some((dir, depth)) = queue.pop() {
        let Ok(items) = fs::read_dir(&dir) else {
            continue;
        };
        for item in items.filter_map(|v| v.ok()) {
            seen += 1;
            if seen > 50000 {
                return;
            }
            let Ok(meta) = fs::symlink_metadata(item.path()) else {
                continue;
            };
            if meta.is_dir() && depth < 6 {
                queue.push((item.path(), depth + 1));
            } else if meta.is_file() && wanted(&item.path(), extensions) {
                found.push((
                    item.path(),
                    meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                ));
            }
        }
    }
}

/// The last `FILE_LIMIT` bytes of a regular file.
fn tail(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let start = meta.len().saturating_sub(FILE_LIMIT);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn fenix_files(prefix: &Path) -> Vec<(PathBuf, String)> {
    let mut result = Vec::new();
    let mut found = Vec::new();
    let program_data = prefix.join("drive_c/ProgramData/Fenix");
    walk(&program_data, EXTENSIONS, &mut found);
    let mut roots = vec![(program_data, "fenix/ProgramData/Fenix".to_string())];
    if let Ok(users) = crate::fenix_setup::user_folders(prefix) {
        for user in users {
            for base in ["AppData/Local", "AppData/Roaming"] {
                let Ok(items) = fs::read_dir(user.join(base)) else {
                    continue;
                };
                for item in items.filter_map(|v| v.ok()) {
                    let name = item.file_name().to_string_lossy().into_owned();
                    if name.to_lowercase().starts_with("fenix")
                        && fs::symlink_metadata(item.path()).is_ok_and(|m| m.is_dir())
                    {
                        walk(&item.path(), EXTENSIONS, &mut found);
                        roots.push((item.path(), format!("fenix/{base}/{name}")));
                    }
                }
            }
        }
    }
    found.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (path, _) in found.into_iter().take(MAX_FILES) {
        if let Some((root, name)) = roots.iter().find(|(root, _)| path.starts_with(root))
            && let Ok(relative) = path.strip_prefix(root)
        {
            result.push((path.clone(), format!("{name}/{}", relative.display())));
        }
    }
    result
}

fn add(archive: &mut tar::Builder<GzEncoder<fs::File>>, name: &str, bytes: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o600);
    header.set_mtime(chrono::Utc::now().timestamp().max(0) as u64);
    header.set_cksum();
    archive.append_data(&mut header, format!("fenix-logs/{name}"), bytes)?;
    Ok(())
}

/// Write the archive into `out` and return its path.
pub fn write(sources: &Sources, out: &Path) -> Result<PathBuf> {
    files::private_dir(out)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = out.join(format!("fenix-logs-{stamp}.tar.gz"));
    require(
        !files::exists(&path),
        "Bitte einen Moment warten und erneut versuchen.",
    )?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode_private()
        .open(&path)?;
    let mut archive = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    let mut total = 0u64;
    let mut included = Vec::new();
    let settings = fenix_autostart::settings_folder(&sources.prefix, sources.folder).ok();
    let mut entries = sources.own.clone();
    if let Some(settings) = &settings {
        entries.push((settings.join("exe.xml"), "msfs/exe.xml".into()));
    }
    entries.extend(fenix_files(&sources.prefix));
    for (path, name) in entries {
        if total >= TOTAL_LIMIT {
            break;
        }
        if let Some(bytes) = tail(&path) {
            total += bytes.len() as u64;
            add(&mut archive, &name, &bytes)?;
            included.push(name);
        }
    }
    let mut summary = sources.summary.clone();
    summary["created_at"] = json!(files::now());
    summary["launcher_version"] = json!(crate::VERSION);
    summary["autostart"] = json!(fenix_autostart::status(&sources.prefix, sources.folder));
    summary["settings_folder_found"] = json!(settings.is_some());
    summary["bootstrapper_found"] = json!(fenix_autostart::bootstrapper(&sources.prefix).is_some());
    summary["files"] = json!(included);
    summary["limited"] = json!(total >= TOTAL_LIMIT);
    add(
        &mut archive,
        "summary.json",
        &serde_json::to_vec_pretty(&summary)?,
    )?;
    archive.into_inner()?.finish()?.sync_all()?;
    prune(out);
    Ok(path)
}

fn prune(out: &Path) {
    let mut archives: Vec<_> = fs::read_dir(out)
        .into_iter()
        .flatten()
        .filter_map(|v| v.ok())
        .filter(|v| {
            let name = v.file_name().to_string_lossy().into_owned();
            name.starts_with("fenix-logs-") && name.ends_with(".tar.gz")
        })
        .map(|v| v.path())
        .collect();
    archives.sort();
    let excess = archives.len().saturating_sub(KEPT);
    for old in archives.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
}

trait Private {
    fn mode_private(&mut self) -> &mut Self;
}
impl Private for fs::OpenOptions {
    fn mode_private(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.mode(0o600)
    }
}

fn newest_game_log(private: &Path) -> Option<PathBuf> {
    let mut runs: Vec<_> = fs::read_dir(private)
        .ok()?
        .filter_map(|v| v.ok())
        .filter(|v| v.file_name().to_string_lossy().starts_with("run-"))
        .filter(|v| v.file_type().is_ok_and(|t| t.is_dir()))
        .map(|v| v.path())
        .collect();
    runs.sort();
    runs.pop().map(|v| v.join("game.log"))
}

fn finish(app: &Launcher, sources: Sources) -> Result<Value> {
    let out = app.state_dir.join("fenix-logs");
    let path = write(&sources, &out)?;
    let opened = reqwest::Url::from_file_path(&out)
        .ok()
        .is_some_and(|uri| process::open_uri(uri.as_str()).is_ok());
    Ok(json!({
        "ok": true,
        "path": path,
        "message": if opened {
            "Fenix-Logs gespeichert, der Ordner wird geöffnet. Prüfe die Dateien vor dem Teilen und entferne Konto- und Lizenzangaben."
        } else {
            "Fenix-Logs gespeichert unter ~/.local/state/flightdeck/fenix-logs. Prüfe die Dateien vor dem Teilen und entferne Konto- und Lizenzangaben."
        },
    }))
}

/// The selected Xbox runtime.
pub fn export(app: &Arc<Launcher>) -> Result<Value> {
    let root = app.root()?;
    let private = root.join("private");
    let mut own = vec![
        (
            private.join("fenix-app.log"),
            "flightdeck/fenix-app.log".to_string(),
        ),
        (
            private.join("fenix-autostart.json"),
            "flightdeck/fenix-autostart.json".to_string(),
        ),
        (
            private.join("fenix-installer-result.json"),
            "flightdeck/fenix-installer-result.json".to_string(),
        ),
        (
            root.join(crate::fenix::MARKER),
            "flightdeck/fenix-linux-patch.json".to_string(),
        ),
    ];
    if let Some(log) = newest_game_log(&private) {
        own.push((log, "flightdeck/game.log".to_string()));
    }
    finish(
        app,
        Sources {
            prefix: root.join("local/msfs-prefix"),
            folder: fenix_autostart::FOLDER_2024,
            own,
            summary: json!({"source":"xbox"}),
        },
    )
}

/// The selected Steam installation.
pub fn steam_export(app: &Arc<Launcher>) -> Result<Value> {
    let item = app
        .lock()
        .steam
        .clone()
        .ok_or(Error::Invalid("Keine Steam-Installation ausgewählt."))?;
    item.validate()?;
    let target = item.target()?;
    let mut found = Vec::new();
    walk(&target.state(), &["log", "txt", "json"], &mut found);
    found.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut own: Vec<_> = found
        .into_iter()
        .take(40)
        .filter_map(|(path, _)| {
            let name = path
                .strip_prefix(target.state())
                .ok()?
                .display()
                .to_string();
            Some((path, format!("fenix-patch/{name}")))
        })
        .collect();
    if let Some((_, _, private)) = crate::steam_fenix::autostart_target(&app.state_dir, &item) {
        own.push((
            private.join("fenix-autostart.json"),
            "flightdeck/fenix-autostart.json".to_string(),
        ));
    }
    finish(
        app,
        Sources {
            prefix: item.prefix(),
            folder: target.settings(),
            own,
            summary: json!({"source":"steam","game":item.game.id()}),
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn put(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn archive_holds_launch_entry_and_fenix_logs_but_no_license_files() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix");
        let settings = prefix
            .join("drive_c/users/pilot/AppData/Roaming")
            .join(fenix_autostart::FOLDER_2024);
        put(&settings.join("exe.xml"), b"<SimBase.Document/>");
        put(
            &prefix.join("drive_c/ProgramData/Fenix/FenixSim A320/logs/system.log"),
            b"bootstrapper started",
        );
        put(
            &prefix.join("drive_c/ProgramData/Fenix/FenixSim A320/license.txt"),
            b"secret",
        );
        put(
            &prefix.join("drive_c/ProgramData/Fenix/FenixSim A320/fenixConfig.xml"),
            b"<x/>",
        );
        put(
            &prefix.join("drive_c/users/pilot/AppData/Local/FenixApp/app.log"),
            b"app",
        );
        let large = vec![b'a'; (FILE_LIMIT + 10) as usize];
        put(&prefix.join("drive_c/ProgramData/Fenix/big.log"), &large);
        std::os::unix::fs::symlink(
            "/etc/hostname",
            prefix.join("drive_c/ProgramData/Fenix/link.log"),
        )
        .unwrap();
        let own_log = temp.path().join("fenix-app.log");
        put(&own_log, b"own");
        let sources = Sources {
            prefix: prefix.clone(),
            folder: fenix_autostart::FOLDER_2024,
            own: vec![
                (own_log, "flightdeck/fenix-app.log".into()),
                (
                    temp.path().join("absent.log"),
                    "flightdeck/absent.log".into(),
                ),
            ],
            summary: json!({"source":"xbox"}),
        };
        let out = temp.path().join("out");
        let path = write(&sources, &out).unwrap();
        let mut names = std::collections::BTreeMap::new();
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&path).unwrap()));
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().display().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            names.insert(name, bytes);
        }
        assert!(names.contains_key("fenix-logs/msfs/exe.xml"));
        assert!(names.contains_key("fenix-logs/flightdeck/fenix-app.log"));
        assert!(
            names.contains_key("fenix-logs/fenix/ProgramData/Fenix/FenixSim A320/logs/system.log")
        );
        assert!(names.contains_key("fenix-logs/fenix/AppData/Local/FenixApp/app.log"));
        assert_eq!(
            names["fenix-logs/fenix/ProgramData/Fenix/big.log"].len() as u64,
            FILE_LIMIT
        );
        assert!(!names.keys().any(|v| v.contains("license")
            || v.contains("link.log")
            || v.contains("fenixConfig")
            || v.contains("absent")));
        let summary: Value = serde_json::from_slice(&names["fenix-logs/summary.json"]).unwrap();
        assert_eq!(summary["autostart"], "missing");
        assert_eq!(summary["settings_folder_found"], true);
        assert_eq!(summary["bootstrapper_found"], false);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn keeps_only_the_newest_archives() {
        let temp = tempfile::tempdir().unwrap();
        for n in 0..8 {
            put(
                &temp
                    .path()
                    .join(format!("fenix-logs-2026111{n}-000000.tar.gz")),
                b"x",
            );
        }
        put(&temp.path().join("other.txt"), b"x");
        prune(temp.path());
        let mut left: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|v| v.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), KEPT + 1);
        assert_eq!(left[0], "fenix-logs-20261113-000000.tar.gz");
    }
}
