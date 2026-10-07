// SPDX-License-Identifier: MIT
//! Explicit local drafts only. No email sender, account access or network.
use crate::{
    Error, Result, backend::Launcher, diagnostics, error::require, files, log_reader::regex,
    ordered_json,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
};
const MAXIMUM: usize = 512 * 1024;
const CATEGORIES: [&str; 5] = ["graphics", "cloud", "marketplace", "installation", "other"];
const OBSERVATIONS: [&str; 4] = [
    "menus_visible",
    "main_view_black",
    "second_window_works",
    "second_window_crashes",
];
const FIELDS: [&str; 21] = [
    "context",
    "vr",
    "run_found",
    "auth_http",
    "local_save_init",
    "store_calls",
    "store_catalog",
    "store_session",
    "store_check",
    "exit",
    "cloud_sync",
    "graphics",
    "audio",
    "user_calls",
    "policy_cache",
    "signature_policy",
    "network_security",
    "log_coverage",
    "summary_limited",
    "proton",
    "fenix",
];
const UNREADABLE: &str =
    "Der gespeicherte Bericht konnte nicht gelesen werden. Bitte einen neuen Bericht vorbereiten.";
// Serialize same-process reads/discards and publications. The launcher lock
// separately pins the selected edition during evidence capture.
static DRAFT: Mutex<()> = Mutex::new(());
pub fn email_address(value: &str) -> bool {
    value.len()<=254&&!value.contains("..")&&regex(r"^[A-Za-z0-9.!#$%'+/=_`{|}~-]{1,64}@[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?\.[A-Za-z]{2,63}$").is_match(value)
}
fn recipient() -> Value {
    let value =
        std::env::var("FLIGHTDECK_SUPPORT_EMAIL").unwrap_or("contact@flightdeck-app.com".into());
    if email_address(&value) {
        json!(value)
    } else {
        Value::Null
    }
}
pub fn read(path: &Path) -> Result<Option<Value>> {
    let file = match files::open_at(rustix::fs::CWD, path, false, true) {
        Ok(v) => v,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let bytes = files::read_file(file, MAXIMUM)?;
    let draft = crate::cloud::json(&bytes)?;
    let r = &draft["report"];
    require(
        draft.as_object().is_some_and(|m| m.len() == 2)
            && r["schema"] == 1
            && r["id"]
                .as_str()
                .is_some_and(|s| regex(r"^[a-f0-9]{32}$").is_match(s))
            && r["category"]
                .as_str()
                .is_some_and(|s| CATEGORIES.contains(&s))
            && r["description"].is_string()
            && r["observations"].is_array()
            && r["system"].is_object()
            && r["diagnostics"].is_object()
            && draft["sha256"] == files::sha256(&ordered_json::report_bytes(&bytes)?),
        UNREADABLE,
    )?;
    Ok(Some(draft))
}
pub fn snapshot(app: &Arc<Launcher>) -> Value {
    let _lock = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    match read(&app.state_dir.join("problem-report.json")) {
        Ok(draft) => json!({"recipient":recipient(),"draft":draft,"unreadable":false}),
        Err(_) => json!({"recipient":recipient(),"draft":null,"unreadable":true}),
    }
}
pub fn system_info() -> Value {
    let mut value = json!({"distribution":null,"release":null,"architecture":null,"kernel":null});
    if let Some(raw) = ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|p| {
            Path::new(p)
                .canonicalize()
                .ok()
                .and_then(|p| files::read_public(&p, 65536).ok())
        })
    {
        for line in String::from_utf8_lossy(&raw).lines() {
            if let Some((key, text)) = line.split_once('=') {
                let text = text.trim_matches('"').trim_matches('\'');
                let key = match key {
                    "ID" => "distribution",
                    "VERSION_ID" => "release",
                    _ => continue,
                };
                if regex(r"^[A-Za-z0-9._+-]{1,100}$").is_match(text) {
                    value[key] = json!(text);
                }
            }
        }
    }
    for (key, arg) in [("architecture", "-m"), ("kernel", "-r")] {
        if let Ok(output) = std::process::Command::new("uname").arg(arg).output() {
            let text = String::from_utf8_lossy(&output.stdout);
            let text = text.trim();
            if output.status.success() && regex(r"^[A-Za-z0-9._+-]{1,100}$").is_match(text) {
                value[key] = json!(text);
            }
        }
    }
    value
}
pub fn prepare(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    require(
        data.as_object().is_some_and(|m| {
            m.keys().all(|k| {
                ["category", "description", "observations", "runtime_path"].contains(&k.as_str())
            })
        }),
        "Der Fehlerbericht enthält unbekannte Felder.",
    )?;
    let category = data["category"]
        .as_str()
        .filter(|s| CATEGORIES.contains(s))
        .ok_or(Error::Invalid("Bitte eine Fehlerkategorie auswählen."))?;
    let description = data["description"]
        .as_str()
        .filter(|s| {
            (10..=4000).contains(&s.trim().chars().count())
                && !s.chars().any(|c| c < ' ' && c != '\n' && c != '\t')
        })
        .ok_or(Error::Invalid(
            "Bitte den Fehler mit 10 bis 4000 Zeichen beschreiben.",
        ))?;
    let empty = Vec::new();
    let observations = match data.get("observations") {
        None => &empty,
        Some(v) => v.as_array().ok_or(Error::Invalid(
            "Die Angaben zum Grafikfehler sind ungültig.",
        ))?,
    };
    require(
        observations.len() <= 4
            && observations
                .iter()
                .all(|v| v.as_str().is_some_and(|s| OBSERVATIONS.contains(&s)))
            && (category == "graphics" || observations.is_empty()),
        "Die Angaben zum Grafikfehler sind ungültig.",
    )?;
    let observations: BTreeSet<_> = observations.iter().filter_map(Value::as_str).collect();
    let s = app.lock();
    Launcher::open(&s)?;
    require(
        data["runtime_path"] == json!(s.runtime.as_deref().and_then(|p| p.to_str())),
        "Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.",
    )?;
    let mut evidence = diagnostics::from_state(&s);
    if let Some(summary) = evidence["summary"].as_object_mut() {
        summary.retain(|key, _| FIELDS.contains(&key.as_str()));
    }
    let checks: Vec<_> = evidence["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            row["id"]
                .as_str()
                .is_some_and(|s| regex(r"^[a-z][a-z0-9_-]{0,39}$").is_match(s))
                && row.get("ok").is_some_and(|v| v.is_null() || v.is_boolean())
        })
        .map(|row| json!({"id":row["id"],"ok":row["ok"]}))
        .collect();
    let report = json!({"schema":1,"id":uuid::Uuid::new_v4().simple().to_string(),"created_at":files::now(),"category":category,"description":description.trim(),"observations":observations,"system":system_info(),"diagnostics":{"generated_at":evidence["generated_at"],"summary":evidence["summary"],"checks":checks}});
    drop(s);
    let raw = ordered_json::encoded(&report)?;
    require(
        raw.len() <= MAXIMUM - 1024,
        "Der Fehlerbericht ist zu groß. Bitte den lokalen Diagnoseexport verwenden.",
    )?;
    let draft = json!({"report":report,"sha256":files::sha256(&raw)});
    let raw = ordered_json::encoded(&draft)?;
    require(
        raw.len() <= MAXIMUM,
        "Der Fehlerbericht ist zu groß. Bitte den lokalen Diagnoseexport verwenden.",
    )?;
    let _lock = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    files::atomic(&app.state_dir.join("problem-report.json"), &raw)?;
    Ok(json!({"ok":true,"recipient":recipient(),"draft":draft,"unreadable":false}))
}
pub fn discard(app: &Arc<Launcher>, id: &str) -> Result<Value> {
    let _lock = DRAFT.lock().unwrap_or_else(|e| e.into_inner());
    let path = app.state_dir.join("problem-report.json");
    let draft = read(&path).map_err(|_| Error::Invalid(UNREADABLE))?;
    require(
        draft.as_ref().is_some_and(|v| v["report"]["id"] == id),
        "Dieser Bericht ist nicht mehr aktuell. Bitte die Ansicht neu laden.",
    )?;
    let folder = files::directory(&app.state_dir, true)?;
    rustix::fs::unlinkat(&folder, "problem-report.json", rustix::fs::AtFlags::empty())?;
    folder.sync_all()?;
    Ok(json!({"ok":true,"recipient":recipient(),"draft":null,"unreadable":false}))
}
