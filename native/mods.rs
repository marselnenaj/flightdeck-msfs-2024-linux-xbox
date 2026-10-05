// SPDX-License-Identifier: MIT
//! Community discovery and reviewed removal of individual owned add-on entries.
use crate::{
    Error, Result, backend::Launcher, error::require, files, game_package, games::Game, process,
    runtime,
};
use crate::{backend::Context, owned_tree, wine_processes};
use rustix::fs::{self as rfs, AtFlags, Mode, RenameFlags};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use std::{
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    sync::Arc,
    time::{Duration, Instant},
};

const REMOVAL_STALE: &str =
    "Der Community-Eintrag hat sich geändert. Bitte die Deinstallation erneut prüfen.";
#[derive(Clone)]
pub struct Removal {
    id: String,
    runtime: PathBuf,
    folder: PathBuf,
    folder_id: [u64; 2],
    name: String,
    link: Option<(u64, u64, PathBuf)>,
    tree: Option<owned_tree::Inventory>,
    created: Instant,
}
fn removal_idle(ctx: &Context) -> Result<()> {
    crate::cloud_process_guard::check(ctx.root()?, &ctx.lease()?)?;
    wine_processes::idle(&ctx.root()?.join("local/msfs-prefix"))
}
fn removal_folder(root: &Path) -> Result<PathBuf> {
    let (locations, limited) = locations(root)?;
    let paths: std::collections::BTreeSet<_> = locations
        .into_iter()
        .map(|(path, _)| resolve(&path))
        .collect::<Result<_>>()?;
    require(!limited && paths.len() == 1, REMOVAL_STALE)?;
    paths
        .into_iter()
        .next()
        .ok_or(Error::Invalid(REMOVAL_STALE))
}
fn removal_plan(ctx: &Context, expected_root: &Path, name: String) -> Result<Value> {
    require(ctx.root()? == expected_root, REMOVAL_STALE)?;
    removal_idle(ctx)?;
    let root = ctx.root()?;
    let folder = removal_folder(root)?;
    let parent = files::directory(&folder, false)?;
    let info = parent.metadata()?;
    let path = folder.join(&name);
    let entry = fs::symlink_metadata(&path)?;
    require(
        entry.uid() == files::uid() && (entry.is_dir() || entry.is_symlink()),
        REMOVAL_STALE,
    )?;
    let link = if entry.is_symlink() {
        Some((entry.dev(), entry.ino(), fs::read_link(&path)?))
    } else {
        None
    };
    let tree = if link.is_none() {
        Some(owned_tree::inventory(&path, &ctx.cancel)?)
    } else {
        None
    };
    let bytes = tree.as_ref().map(|tree| tree.bytes).unwrap_or(0);
    let plan = Removal {
        id: ctx.id.clone(),
        runtime: root.into(),
        folder: folder.clone(),
        folder_id: [info.dev(), info.ino()],
        name: name.clone(),
        link,
        tree,
        created: Instant::now(),
    };
    let is_link = plan.link.is_some();
    let result = json!({"state":"ready","addon_id":name,"entry_path":path,"is_link":is_link,"bytes":bytes,"message":"Prüfe den Community-Eintrag. Erst nach deiner Bestätigung wird er entfernt."});
    // Publish the plan and ready state under the cancellation lock. A cancelled
    // preview must never regain its reservation when its worker finishes.
    let mut state = ctx.launcher.lock();
    ctx.interrupted()?;
    require(
        state
            .active
            .as_ref()
            .is_some_and(|active| active.id == ctx.id),
        REMOVAL_STALE,
    )?;
    if let Some(job) = state.jobs.get_mut("mods").and_then(Value::as_object_mut) {
        job.extend(result.as_object().expect("removal preview object").clone());
    }
    state.mod_removal = Some(plan);
    Ok(result)
}
pub fn preview_remove(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let root = app.root()?;
    require(
        data["runtime_path"].as_str() == root.to_str(),
        REMOVAL_STALE,
    )?;
    let name = crate::backend::string(data, "addon_id")?;
    require(
        !name.is_empty()
            && name.len() <= 255
            && !name.contains(['/', '\\'])
            && name != "."
            && name != ".."
            && !name.starts_with(".flightdeck-")
            && !name.chars().any(char::is_control),
        "Ungültiger Community-Eintrag.",
    )?;
    let snapshot = snapshot(app);
    require(
        snapshot["can_remove"] == true
            && snapshot["mods"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == name)),
        "Der Community-Eintrag kann gerade nicht entfernt werden. Beende zuerst das Spiel und laufende Einrichtungen.",
    )?;
    let ctx = app.reserve("mods", "remove", true)?;
    ctx.update(json!({"phase":"checking"}));
    let name = name.to_string();
    let id = ctx.id.clone();
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            removal_plan(&ctx, &root, name)
        }))
        .unwrap_or(Err(Error::Invalid(
            "Die Deinstallation konnte nicht geprüft werden.",
        )));
        // Success is already published atomically with the retained plan. Do
        // not finish it again: confirmation may already have started removal.
        if let Err(error) = outcome {
            ctx.launcher.finish(&ctx, Err(error), false);
        }
    });
    Ok(json!({"ok":true,"job_id":id}))
}
fn checked_removal(ctx: &Context, plan: &Removal) -> Result<Value> {
    removal_idle(ctx)?;
    require(
        plan.runtime == ctx.root()?
            && plan.id == ctx.id
            && plan.created.elapsed() < Duration::from_secs(900),
        REMOVAL_STALE,
    )?;
    require(
        removal_folder(ctx.root()?)? == plan.folder
            && owned_tree::identity(&plan.folder)? == plan.folder_id,
        REMOVAL_STALE,
    )?;
    let parent = files::directory(&plan.folder, false)?;
    let info = parent.metadata()?;
    require([info.dev(), info.ino()] == plan.folder_id, REMOVAL_STALE)?;
    let source = PathBuf::from(format!(
        "/proc/self/fd/{}/{}",
        parent.as_raw_fd(),
        plan.name
    ));
    let verify = |path: &Path| -> Result<()> {
        if let Some((dev, ino, target)) = &plan.link {
            let info = fs::symlink_metadata(path)?;
            require(
                info.is_symlink()
                    && [info.dev(), info.ino()] == [*dev, *ino]
                    && fs::read_link(path)? == *target,
                REMOVAL_STALE,
            )
        } else {
            require(
                Some(owned_tree::inventory(path, &ctx.cancel)?) == plan.tree,
                REMOVAL_STALE,
            )
        }
    };
    verify(&source)?;
    ctx.begin_commit("removing", "Der geprüfte Community-Eintrag wird entfernt …")?;
    let quarantine = format!(".flightdeck-removal-{}", uuid::Uuid::new_v4().simple());
    rfs::mkdirat(&parent, &quarantine, Mode::RWXU)?;
    let quarantine_dir = files::open_at(&parent, &quarantine, true, true)?;
    let target = PathBuf::from(format!(
        "/proc/self/fd/{}/entry",
        quarantine_dir.as_raw_fd()
    ));
    rfs::renameat_with(
        &parent,
        &plan.name,
        &quarantine_dir,
        "entry",
        RenameFlags::NOREPLACE,
    )?;
    parent.sync_all()?;
    let actual = plan.folder.join(&quarantine).join("entry");
    ctx.update(json!({"quarantine_path":actual}));
    // Recheck after atomic isolation. Never traverse a link, mount or replacement.
    if let Err(error) = verify(&target) {
        if rfs::renameat_with(
            &quarantine_dir,
            "entry",
            &parent,
            &plan.name,
            RenameFlags::NOREPLACE,
        )
        .is_ok()
        {
            let _ = rfs::unlinkat(&parent, &quarantine, AtFlags::REMOVEDIR);
            parent.sync_all()?;
            ctx.update(json!({"quarantine_path":null}));
        }
        return Err(error);
    }
    if plan.link.is_some() {
        rfs::unlinkat(&quarantine_dir, "entry", AtFlags::empty())?;
    } else {
        owned_tree::remove_at(
            &quarantine_dir,
            Path::new("entry"),
            plan.tree.as_ref().ok_or(Error::Invalid(REMOVAL_STALE))?.id,
        )?;
    }
    rfs::unlinkat(&parent, &quarantine, AtFlags::REMOVEDIR)?;
    parent.sync_all()?;
    ctx.launcher.lock().mod_removal = None;
    Ok(
        json!({"state":"complete","quarantine_path":null,"message":if plan.link.is_some(){"Die Community-Verknüpfung wurde entfernt. Die Originaldateien bleiben erhalten."}else{"Das Add-on wurde aus dem Community-Ordner entfernt."}}),
    )
}
pub fn remove(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let id = crate::backend::string(data, "job_id")?;
    let plan = app
        .lock()
        .mod_removal
        .clone()
        .filter(|plan| plan.id == id)
        .ok_or(Error::Invalid(REMOVAL_STALE))?;
    let ctx = app.continuation("mods", id)?;
    ctx.update(json!({"state":"running"}));
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            checked_removal(&ctx, &plan)
        }))
        .unwrap_or(Err(Error::Invalid(
            "Die Deinstallation wurde unterbrochen. Prüfe den angezeigten Zwischenordner.",
        )));
        ctx.launcher.finish(&ctx, outcome, false);
    });
    Ok(json!({"ok":true,"job_id":id}))
}
pub fn discard_remove(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let id = crate::backend::string(data, "job_id")?;
    let job = app.job("mods");
    require(
        job["state"] == "ready" || (job["state"] == "running" && job["phase"] == "checking"),
        REMOVAL_STALE,
    )?;
    let result = app.cancel("mods", id)?;
    app.lock().mod_removal = None;
    Ok(result)
}
fn children(path: &Path, limit: usize) -> Result<(Vec<fs::DirEntry>, bool)> {
    let mut entries = fs::read_dir(path)?
        .take(limit + 1)
        .collect::<std::io::Result<Vec<_>>>()?;
    let limited = entries.len() > limit;
    entries.truncate(limit);
    Ok((entries, limited))
}
fn resolve(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        Ok(path.canonicalize()?)
    } else {
        crate::transaction::resolve_new(path)
    }
}
fn case_path(root: &Path, parts: &[&str]) -> Result<PathBuf> {
    let mut root = root.to_path_buf();
    for part in parts {
        let mut candidate = root.join(part);
        if !files::exists(&candidate) && root.is_dir() {
            let (entries, limited) = children(&root, 256)?;
            let matches: Vec<_> = entries
                .into_iter()
                .filter(|v| v.file_name().to_string_lossy().to_lowercase() == part.to_lowercase())
                .collect();
            require(!limited && matches.len() <= 1, "Mehrdeutiger Windows-Pfad.")?;
            if let Some(found) = matches.first() {
                candidate = found.path();
            }
        }
        root = candidate;
    }
    resolve(&root)
}
pub fn configured_path(raw: &str, prefix: &Path) -> Result<PathBuf> {
    require(
        !raw.is_empty() && raw.len() <= 4096 && !raw.chars().any(char::is_control),
        "Ungültiger Community-Pfad.",
    )?;
    if raw.starts_with('/') {
        return resolve(Path::new(raw));
    }
    let bytes = raw.as_bytes();
    require(
        bytes.len() > 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && b"/\\".contains(&bytes[2]),
        "Ungültiger Community-Pfad.",
    )?;
    let parts: Vec<_> = raw[3..]
        .trim_end_matches(['\\', '/'])
        .split(['\\', '/'])
        .collect();
    require(
        parts
            .iter()
            .all(|p| !p.is_empty() && *p != "." && *p != ".."),
        "Ungültiger Community-Pfad.",
    )?;
    let drive = prefix
        .join("dosdevices")
        .join(format!("{}:", (bytes[0] as char).to_ascii_lowercase()))
        .canonicalize()?;
    case_path(&drive, &parts)
}
pub fn family(root: &Path) -> Result<Option<String>> {
    let game = Game::for_runtime(root)?;
    let folder = game.path(root);
    if !folder.join("MicrosoftGame.Config").exists()
        && !folder.join("MicrosoftGame.config").exists()
    {
        return Ok(None);
    }
    let Some(identity) = game_package::config(&folder)?.identity else {
        return Ok(None);
    };
    require(
        !identity.name.is_empty()
            && identity.name.len() <= 200
            && identity
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
            && !identity.publisher.is_empty(),
        "Ungültige Spielpaketidentität.",
    )?;
    use sha2::{Digest, Sha256};
    let raw: Vec<_> = identity
        .publisher
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let hash = Sha256::digest(raw);
    let value = u64::from_be_bytes(hash[..8].try_into().expect("fixed hash prefix")) as u128 * 2;
    let alphabet = b"0123456789abcdefghjkmnpqrstvwxyz";
    let mut suffix = String::new();
    for shift in (0_u8..=60).step_by(5).rev() {
        suffix.push(alphabet[((value >> shift) & 31) as usize] as char);
    }
    Ok(Some(format!("{}_{}", identity.name, suffix)))
}
pub fn locations(root: &Path) -> Result<(Vec<(PathBuf, &'static str)>, bool)> {
    let prefix = root.join("local/msfs-prefix");
    let mut candidates = Vec::new();
    let settings = root.join("private/runtime.json");
    if files::exists(&settings) {
        let config = runtime::value(&settings)?;
        require(config.is_object(), "Ungültige Runtime-Konfiguration.")?;
        for key in [
            "community_path",
            "CommunityLocation",
            "installed_packages_path",
            "InstalledPackagesPath",
        ] {
            if let Some(value) = config.get(key) {
                let path = configured_path(
                    value
                        .as_str()
                        .ok_or(Error::Invalid("Ungültiger Community-Pfad."))?,
                    &prefix,
                )?;
                candidates.push((
                    if ["community_path", "CommunityLocation"].contains(&key) {
                        path
                    } else {
                        path.join("Community")
                    },
                    "runtime_config",
                ));
            }
        }
    }
    let users = prefix.join("drive_c/users");
    if !users.is_dir() {
        return Ok((candidates, false));
    }
    let (profiles, limited) = children(&users, 16)?;
    let family = family(root)?;
    let game = Game::for_runtime(root)?;
    let pattern = regex::Regex::new(r#"(?m)^\s*InstalledPackagesPath\s+"([^"\r\n]+)"\s*$"#)
        .expect("constant UserCfg regex");
    for profile in profiles {
        if !profile.path().is_dir() {
            continue;
        }
        let base = profile.path();
        let mut configs = vec![
            base.join("AppData/Roaming")
                .join(game.user_config())
                .join("UserCfg.opt"),
        ];
        if let Some(family) = &family {
            configs.push(
                base.join("AppData/Local/Packages")
                    .join(family)
                    .join("LocalCache/UserCfg.opt"),
            );
        }
        for config in configs {
            if !files::exists(&config) {
                continue;
            }
            let text = game_package::text(&files::read_public(&config, 1024 * 1024)?)?;
            let paths: Vec<_> = pattern.captures_iter(&text).collect();
            require(
                paths.len() == 1,
                "Mehrdeutiger Paketordner in der Spielkonfiguration.",
            )?;
            let path = configured_path(&paths[0][1], &prefix)?;
            candidates.push((case_path(&path, &["Community"])?, "usercfg"));
        }
    }
    Ok((candidates, limited))
}
fn field(value: &Value, key: &str, fallback: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or(fallback)
        .chars()
        .take(512)
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .into()
}
fn inventory(folder: &Path) -> Result<(Vec<Value>, usize, bool)> {
    let (mut entries, limited) = children(folder, 256)?;
    let scanned = entries.len();
    entries.sort_by_key(|v| v.file_name().to_string_lossy().to_lowercase());
    let mut mods = Vec::new();
    for entry in entries {
        let path = entry.path();
        let linked = entry.file_type()?.is_symlink();
        if !path.is_dir() && !linked {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".flightdeck-") {
            continue;
        }
        let mut item = json!({"id":name,"name":name,"version":"","creator":"","content_type":"","status":"available","is_link":linked});
        let loaded = (|| -> Result<Value> {
            let directory = path.canonicalize()?;
            require(directory.is_dir(), "Kein Paketordner.")?;
            let value: Value = serde_json::from_slice(&files::read_public(
                &directory.join("manifest.json"),
                256 * 1024,
            )?)?;
            require(
                value.is_object()
                    && !field(&value, "title", "").is_empty()
                    && !field(&value, "package_version", "").is_empty(),
                "Ungültiges Paketmanifest.",
            )?;
            Ok(value)
        })();
        match loaded {
            Ok(value) => {
                item["name"] = json!(field(&value, "title", &name));
                item["version"] = json!(field(&value, "package_version", ""));
                item["creator"] = json!(field(&value, "creator", ""));
                item["content_type"] = json!(field(&value, "content_type", ""));
            }
            Err(Error::Io(e)) => {
                item["status"] = json!(if e.kind() == std::io::ErrorKind::NotFound && path.is_dir()
                {
                    "missing_manifest"
                } else {
                    "unreadable"
                })
            }
            Err(_) => item["status"] = json!("invalid_manifest"),
        };
        mods.push(item);
    }
    Ok((mods, scanned, limited))
}
pub fn snapshot(app: &Launcher) -> Value {
    let mut s = app.lock();
    let removable = Launcher::idle(&mut s).is_ok();
    let root = s.runtime.clone();
    let busy = s.active.is_some() || s.closing;
    let job = s.jobs.get("mods").cloned().unwrap_or(Value::Null);
    drop(s);
    let mut result = json!({"state":"unconfigured","message":"Zuerst eine Runtime einrichten oder auswählen.","folder_path":null,"source":null,"can_open":false,"mods":[],"count":0,"scanned_count":0,"limited":false});
    result["runtime_path"] = json!(root);
    result["can_remove"] = json!(false);
    result["job"] = job;
    let Some(root) = root else {
        return result;
    };
    let loaded = (|| -> Result<Value> {
        let (locations, limited) = locations(&root)?;
        let mut unique = BTreeMap::new();
        for (path, source) in locations {
            unique.insert(resolve(&path)?, source);
        }
        if limited || unique.len() > 1 {
            result["state"] = json!("ambiguous");
            result["limited"] = json!(limited);
            result["message"] = json!(
                "Mehrere oder zu viele Spielkonfigurationen gefunden. Den Community-Pfad bitte im Spiel prüfen."
            );
            return Ok(result.clone());
        }
        let Some((folder, source)) = unique.into_iter().next() else {
            result["state"] = json!("unknown");
            result["message"] = json!(
                "Der Community-Ordner ist noch nicht bekannt. MSFS einmal starten und den Paketordner im Spiel einrichten."
            );
            return Ok(result.clone());
        };
        result["folder_path"] = json!(folder);
        result["source"] = json!(source);
        if !folder.is_dir() {
            result["state"] = json!("missing");
            result["message"] = json!(
                "Der konfigurierte Community-Ordner existiert noch nicht. Den Paketordner bitte im Spiel prüfen."
            );
            return Ok(result.clone());
        }
        let (mods, scanned, limited) = inventory(&folder)?;
        result["state"] = json!("ready");
        result["message"] = json!(
            "Community-Ordner erkannt. Die Liste zeigt lokale Pakete, keine Kompatibilitätsprüfung."
        );
        result["count"] = json!(mods.len());
        result["mods"] = json!(mods);
        result["scanned_count"] = json!(scanned);
        result["limited"] = json!(limited);
        result["can_remove"] = json!(removable && files::directory(&folder, false).is_ok());
        result["can_open"] = json!(
            !busy
                && process::which("xdg-open").is_some()
                && (std::env::var_os("DISPLAY").is_some()
                    || std::env::var_os("WAYLAND_DISPLAY").is_some())
        );
        Ok(result.clone())
    })();
    loaded.unwrap_or_else(|_|{result["state"]=json!("unreadable");result["message"]=json!("Der Community-Ordner konnte nicht sicher gelesen werden. Spielkonfiguration und Zugriffsrechte prüfen.");result})
}
pub fn open_folder(app: &Launcher, data: &Value) -> Result<Value> {
    let root = app.root()?;
    require(
        data.get("runtime_path").is_none() || data["runtime_path"].as_str() == root.to_str(),
        "Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.",
    )?;
    let value = snapshot(app);
    require(
        value["can_open"] == true,
        "Der Community-Ordner kann gerade nicht geöffnet werden.",
    )?;
    let path = crate::backend::string(&value, "folder_path")?;
    let uri = reqwest::Url::from_file_path(path)
        .map_err(|_| Error::Invalid("Ungültiger Community-Pfad."))?;
    let s = app.lock();
    Launcher::open(&s)?;
    require(
        s.runtime.as_ref() == Some(&root) && s.active.is_none(),
        "Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.",
    )?;
    process::open_uri(uri.as_str())?;
    Ok(json!({"ok":true,"message":"Der Community-Ordner wird im Dateimanager geöffnet."}))
}
