// SPDX-License-Identifier: MIT
//! Read only bounded Community configuration and package manifests.
use crate::{
    Error, Result, backend::Launcher, error::require, files, game_package, games::Game, process,
    runtime,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
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
    let s = app.lock();
    let root = s.runtime.clone();
    let busy = s.active.is_some() || s.closing;
    drop(s);
    let mut result = json!({"state":"unconfigured","message":"Zuerst eine Runtime einrichten oder auswählen.","folder_path":null,"source":null,"can_open":false,"mods":[],"count":0,"scanned_count":0,"limited":false});
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
