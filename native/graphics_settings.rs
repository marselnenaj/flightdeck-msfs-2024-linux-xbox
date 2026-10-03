// SPDX-License-Identifier: MIT
//! Saved NVIDIA options follow the effective launch profile, with per-field undo.
use crate::{Error, Result, error::require, files, games::Game, log_reader::regex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const MARKER: &str = ".flightdeck-nvidia-settings.json";
const LIMIT: usize = 1024 * 1024;
const INVALID: &str = "Die gespeicherten Grafikeinstellungen sind nicht eindeutig lesbar.";
pub type Undo = BTreeMap<String, String>;

fn option(key: &str) -> Option<(&'static [&'static str], &'static str)> {
    match key.strip_suffix("VR").unwrap_or(key) {
        "AntiAliasing" => Some((&["DLSS"], "TAA")),
        "Reflex" => Some((&["ON", "BOOST", "ONBOOST", "ON+BOOST"], "OFF")),
        "FrameGeneration" => Some((&["DLSSG"], "NONE")),
        _ => None,
    }
}
fn decode(bytes: &[u8]) -> Result<(String, usize)> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        require(bytes.len().is_multiple_of(2), INVALID)?;
        let little = bytes[0] == 0xff;
        let words: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        Ok((
            String::from_utf16(&words).map_err(|_| Error::Invalid(INVALID))?,
            if little { 2 } else { 3 },
        ))
    } else {
        let bom = usize::from(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        Ok((
            String::from_utf8(bytes[bom * 3..].to_vec()).map_err(|_| Error::Invalid(INVALID))?,
            bom,
        ))
    }
}
fn encode(text: &str, encoding: usize) -> Vec<u8> {
    match encoding {
        2 => [0xff, 0xfe]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
        3 => [0xfe, 0xff]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect(),
        1 => [0xef, 0xbb, 0xbf].into_iter().chain(text.bytes()).collect(),
        _ => text.as_bytes().to_vec(),
    }
}
pub fn transform(
    bytes: &[u8],
    original: &Undo,
    compatibility: bool,
) -> Result<(Vec<u8>, Undo, Vec<String>)> {
    require(
        original
            .iter()
            .all(|(k, v)| option(k).is_some_and(|(values, _)| values.contains(&v.as_str()))),
        INVALID,
    )?;
    let (text, encoding) = decode(bytes)?;
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
    let (mut depth, mut video, mut found) = (0_i32, false, false);
    let mut fields = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        if stripped.starts_with('{') {
            require(!video, INVALID)?;
            if stripped == "{Video" && depth == 0 {
                require(!found, INVALID)?;
                video = true;
                found = true;
            }
            depth += 1;
        } else if stripped == "}" {
            depth -= 1;
            require(depth >= 0, INVALID)?;
            video = false;
        } else if video
            && stripped
                .split([' ', '\t'])
                .next()
                .and_then(option)
                .is_some()
        {
            let captures = regex(r"^([ \t]*)(\w+)([ \t]+)([^ \t\r\n]{1,32})([ \t]*)(\r?\n)?$")
                .captures(line)
                .ok_or(Error::Invalid(INVALID))?;
            let field = (
                index,
                captures[1].to_owned(),
                captures[3].to_owned(),
                captures[4].to_owned(),
                captures[5].to_owned(),
                captures.get(6).map(|m| m.as_str()).unwrap_or("").to_owned(),
            );
            require(
                fields.insert(captures[2].to_owned(), field).is_none(),
                INVALID,
            )?;
        }
    }
    require(found && depth == 0, INVALID)?;
    let (mut saved, mut changed) = (Undo::new(), Vec::new());
    for (key, (index, indent, space, value, trailing, newline)) in fields {
        let (unsupported, safe) = option(&key).ok_or(Error::Invalid(INVALID))?;
        let mut replacement = value.as_str();
        if compatibility {
            if unsupported.contains(&value.as_str()) {
                saved.insert(key.clone(), value.clone());
                replacement = safe;
            } else if value == safe
                && let Some(old) = original.get(&key)
            {
                saved.insert(key.clone(), old.clone());
            }
        } else if value == safe
            && let Some(old) = original.get(&key)
        {
            replacement = old;
        }
        if replacement != value {
            lines[index] = format!("{indent}{key}{space}{replacement}{trailing}{newline}");
            changed.push(key);
        }
    }
    Ok((encode(&lines.concat(), encoding), saved, changed))
}
fn missing(error: &Error) -> bool {
    matches!(error, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
}
fn case_child(parent: &Path, name: &str) -> Result<PathBuf> {
    files::directory(parent, false)?;
    let entries = fs::read_dir(parent)?
        .take(257)
        .collect::<std::io::Result<Vec<_>>>()?;
    require(entries.len() <= 256, INVALID)?;
    let matches: Vec<_> = entries
        .into_iter()
        .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .collect();
    require(matches.len() <= 1, INVALID)?;
    matches
        .first()
        .map(|e| e.path())
        .ok_or_else(|| Error::Io(std::io::ErrorKind::NotFound.into()))
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    schema: u32,
    original: Undo,
}
struct Prepared {
    data: Vec<u8>,
    edited: Vec<u8>,
    original: Undo,
    saved: Undo,
    changed: Vec<String>,
}
fn inspect(config: &Path, compatibility: bool) -> Result<Prepared> {
    let data = files::read(config, LIMIT)?;
    let backup: Option<Backup> = match files::json(&config.with_file_name(MARKER), 4096) {
        Ok(value) => Some(value),
        Err(e) if missing(&e) => None,
        Err(e) => return Err(e),
    };
    require(backup.as_ref().is_none_or(|b| b.schema == 1), INVALID)?;
    let original = backup.map(|b| b.original).unwrap_or_default();
    let (edited, saved, changed) = transform(&data, &original, compatibility)?;
    Ok(Prepared {
        data,
        edited,
        original,
        saved,
        changed,
    })
}
pub fn prepare(root: &Path, compatibility: bool) -> Result<Value> {
    prepare_inner(root, compatibility).map_err(|_| Error::Invalid(
        "Die NVIDIA-Spieleinstellungen konnten nicht vorbereitet werden. Die gesicherten Werte bleiben erhalten.",
    ))
}
fn prepare_inner(root: &Path, compatibility: bool) -> Result<Value> {
    let mut report = json!({"changed_files":0,"changed_options":[],"skipped_files":0});
    if Game::for_runtime(root)? != Game::Msfs2024 {
        return Ok(report);
    }
    let discover = || -> Result<_> {
        let mut folder = root.to_path_buf();
        for name in ["local", "msfs-prefix", "drive_c", "users"] {
            folder = case_child(&folder, name)?;
        }
        files::directory(&folder, false)?;
        let mut profiles = fs::read_dir(folder)?
            .take(17)
            .map(|e| e.map(|v| v.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        require(profiles.len() <= 16, INVALID)?;
        profiles.sort();
        Ok(profiles)
    };
    let profiles = match discover() {
        Ok(result) => result,
        Err(e) if missing(&e) => return Ok(report),
        Err(_) => {
            report["skipped_files"] = json!(1);
            return Ok(report);
        }
    };
    // The Roaming location does not depend on valid optional package metadata.
    let family = crate::mods::family(root).ok().flatten();
    let mut paths = vec![vec![
        "AppData",
        "Roaming",
        "Microsoft Flight Simulator 2024",
        "UserCfg.opt",
    ]];
    if let Some(family) = family.as_deref() {
        paths.push(vec![
            "AppData",
            "Local",
            "Packages",
            family,
            "LocalCache",
            "UserCfg.opt",
        ]);
    }
    let mut changes = std::collections::BTreeSet::new();
    for profile in profiles {
        for parts in &paths {
            let read = || -> Result<_> {
                let mut config = profile.clone();
                for part in parts {
                    config = case_child(&config, part)?;
                }
                let inspected = inspect(&config, compatibility)?;
                Ok((config, inspected))
            };
            let (
                config,
                Prepared {
                    data,
                    edited,
                    original,
                    saved,
                    changed,
                },
            ) = match read() {
                Ok(result) => result,
                Err(e) if missing(&e) => continue,
                Err(_) => {
                    report["skipped_files"] =
                        json!(report["skipped_files"].as_u64().unwrap_or(0) + 1);
                    continue;
                }
            };
            require(
                files::read(&config, LIMIT)? == data,
                "Die Grafikeinstellungen wurden während der Vorbereitung verändert.",
            )?;
            let marker = config.with_file_name(MARKER);
            if !saved.is_empty() && saved != original {
                files::atomic_json(
                    &marker,
                    &Backup {
                        schema: 1,
                        original: saved.clone(),
                    },
                )?;
            }
            if edited != data {
                files::atomic(&config, &edited)?;
                report["changed_files"] = json!(report["changed_files"].as_u64().unwrap_or(0) + 1);
                changes.extend(changed);
            }
            if saved.is_empty() && files::exists(&marker) {
                fs::remove_file(marker)?;
            }
        }
    }
    report["changed_options"] = json!(changes);
    Ok(report)
}
