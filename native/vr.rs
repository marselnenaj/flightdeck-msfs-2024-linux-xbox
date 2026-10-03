// SPDX-License-Identifier: MIT
use crate::{
    Error, Result,
    error::require,
    files,
    graphics::{self, Environment},
    process, runtime,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::Duration,
};
pub const MODES: [&str; 5] = ["off", "auto", "wivrn", "steamvr", "monado"];
pub fn message(state: &str) -> &'static str {
    match state {
        "off" => "VR ist ausgeschaltet. Der Simulator startet wie bisher.",
        "detected" => "VR-System gefunden. Verbinde das Headset und prüfe die Verbindung.",
        "missing" => {
            "Kein passendes VR-System gefunden. Starte WiVRn, SteamVR oder Monado und verbinde dein Headset."
        }
        "invalid" => {
            "Die OpenXR-Konfiguration ist nicht nutzbar. Wähle die aktive Runtime in deinem VR-Programm erneut aus."
        }
        "bridge_missing" => {
            "Im gewählten Runner fehlen OpenXR-Komponenten. Verwende den mit Flightdeck eingerichteten Runner."
        }
        "checking" => "VR-System und Headset werden geprüft …",
        "ready" => {
            "OpenXR erreicht das Headset und dessen Vulkan-Grafikkarte. Bild und Tracking prüfst du anschließend im Simulator."
        }
        "headset_missing" => {
            "Die VR-Runtime antwortet, aber kein Headset ist verfügbar. Verbinde oder aktiviere das Headset und prüfe erneut."
        }
        "loader_missing" => {
            "Der Linux-OpenXR-Loader fehlt oder kann nicht geladen werden. Installiere die OpenXR-Pakete deiner Distribution."
        }
        "timeout" => {
            "Das VR-System antwortet nicht. Starte es mit verbundenem Headset neu und prüfe erneut."
        }
        "cancelled" => "VR-Prüfung abgebrochen.",
        _ => {
            "OpenXR oder Vulkan konnte nicht initialisiert werden. Prüfe dein VR-Programm und den Grafiktreiber."
        }
    }
}
pub fn settings(root: Option<&Path>) -> Result<String> {
    let Some(root) = root else {
        return Ok("off".into());
    };
    let path = root.join("private/vr-settings.json");
    if !files::exists(&path) {
        return Ok("off".into());
    }
    let value = runtime::value(&path)?;
    require(
        value["schema"] == 1 && value["mode"].as_str().is_some_and(|s| MODES.contains(&s)),
        "Die VR-Einstellungen sind ungültig. Bitte unter Einrichtung erneut speichern.",
    )?;
    Ok(value["mode"].as_str().unwrap_or("off").into())
}
fn public_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&files::read_public(
        &path.canonicalize()?,
        65536,
    )?)?)
}
fn absolute(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 4096 && !value.chars().any(|c| c < ' ')
}
pub fn discover(env: &Environment) -> Vec<Value> {
    let home = env
        .get("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(files::home);
    let config = env
        .get("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".config"));
    let data = env
        .get("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".local/share"));
    let mut paths = Vec::new();
    let explicit = env.get("XR_RUNTIME_JSON");
    if let Some(path) = explicit {
        paths.push((PathBuf::from(path), true));
    }
    let mut roots = vec![config.clone()];
    roots.extend(
        env.get("XDG_CONFIG_DIRS")
            .map(String::as_str)
            .unwrap_or("/etc/xdg")
            .split(':')
            .filter(|s| absolute(s))
            .map(PathBuf::from),
    );
    roots.push("/etc".into());
    for root in roots {
        for name in ["active_runtime.x86_64.json", "active_runtime.json"] {
            paths.push((root.join("openxr/1").join(name), true));
        }
    }
    for root in [
        &data,
        Path::new("/usr/local/share"),
        Path::new("/usr/share"),
    ] {
        if let Ok(entries) = std::fs::read_dir(root.join("openxr/1")) {
            let mut entries = entries
                .filter_map(|v| v.ok())
                .take(256)
                .filter(|v| v.path().extension().is_some_and(|s| s == "json"))
                .map(|v| v.path())
                .collect::<Vec<_>>();
            entries.sort();
            for path in entries.into_iter().take(32) {
                let active = path
                    .file_name()
                    .is_some_and(|s| s.to_string_lossy().starts_with("active_runtime"));
                paths.push((path, active));
            }
        }
    }
    let openvr = env
        .get("VR_PATHREG_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or(config.join("openvr/openvrpaths.vrpath"));
    if let Ok(value) = public_json(&openvr)
        && let Some(entries) = value["runtime"].as_array()
    {
        for root in entries
            .iter()
            .take(8)
            .filter_map(Value::as_str)
            .filter(|s| absolute(s))
        {
            paths.push((PathBuf::from(root).join("steamxr_linux64.json"), false));
        }
    }
    for root in [
        data.join("Steam"),
        home.join(".steam/steam"),
        home.join(".steam/root"),
    ] {
        paths.push((
            root.join("steamapps/common/SteamVR/steamxr_linux64.json"),
            false,
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut active_seen = false;
    let mut result = Vec::new();
    for (path, active) in paths {
        let is_explicit = explicit.is_some_and(|p| Path::new(p) == path);
        if !files::exists(&path) && !is_explicit {
            continue;
        }
        let is_active = active && !active_seen;
        if active {
            active_seen = true;
        }
        let validated = (|| -> Result<Value> {
            require(
                !is_explicit || explicit.is_some_and(|s| absolute(s)),
                message("invalid"),
            )?;
            let resolved = path.canonicalize()?;
            let value = public_json(&resolved)?;
            let lib = value["runtime"]["library_path"]
                .as_str()
                .ok_or(Error::Invalid(message("invalid")))?;
            require(
                !lib.is_empty()
                    && lib.len() <= 4096
                    && !lib.chars().any(|c| c < ' ')
                    && !lib.to_lowercase().ends_with(".dll")
                    && !lib.contains(':'),
                message("invalid"),
            )?;
            if lib.contains('/') {
                require(
                    if lib.starts_with('/') {
                        Path::new(lib).is_file()
                    } else {
                        resolved.parent().is_some_and(|p| p.join(lib).is_file())
                    },
                    message("invalid"),
                )?;
            }
            let identity = format!(
                "{} {} {}",
                resolved.display(),
                lib,
                value["runtime"]["name"].as_str().unwrap_or("")
            )
            .to_lowercase();
            let provider = ["wivrn", "steamvr", "monado"]
                .into_iter()
                .find(|s| identity.contains(s))
                .unwrap_or("other");
            Ok(json!({"provider":provider,"path":resolved,"active":is_active,"valid":true}))
        })();
        match validated {
            Ok(value) => {
                if seen.insert(value["path"].clone().to_string()) {
                    result.push(value);
                }
            }
            Err(_) => result
                .push(json!({"provider":"other","path":path,"active":is_active,"valid":false})),
        }
    }
    result
}
pub fn choose(mode: &str, env: &Environment) -> Option<Value> {
    let candidates = discover(env);
    if mode == "auto" {
        if let Some(active) = candidates.iter().find(|v| v["active"] == true) {
            return Some(active.clone());
        }
        let mut valid = candidates.into_iter().filter(|v| v["valid"] == true);
        let first = valid.next();
        if valid.next().is_none() { first } else { None }
    } else {
        candidates
            .into_iter()
            .find(|v| v["provider"] == mode && v["valid"] == true)
    }
}
pub fn bridge(root: Option<&Path>) -> bool {
    root.is_some_and(|r| {
        [
            "runner/files/bin/wine",
            "runner/files/lib/wine/x86_64-windows/wineopenxr.dll",
            "runner/files/lib/wine/x86_64-unix/wineopenxr.so",
        ]
        .iter()
        .all(|p| r.join(p).is_file())
    })
}
pub fn snapshot(root: Option<&Path>) -> Value {
    let env = std::env::vars().collect();
    let providers = discover(&env)
        .iter()
        .filter(|v| v["valid"] == true)
        .filter_map(|v| v["provider"].as_str().map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>();
    let (mut mode, mut error) = ("off".to_string(), String::new());
    let state = match settings(root) {
        Err(e) => {
            error = e.to_string();
            "invalid"
        }
        Ok(value) => {
            mode = value;
            if mode == "off" {
                "off"
            } else if !bridge(root) {
                "bridge_missing"
            } else {
                match choose(&mode, &env) {
                    None => "missing",
                    Some(v) if v["valid"] == true => "detected",
                    _ => "invalid",
                }
            }
        }
    };
    json!({"mode":mode,"available":root.is_some(),"state":state,"providers":providers,"error":error,"message":message(state)})
}
pub fn launcher_snapshot(s: &crate::backend::State) -> Value {
    let mut result = snapshot(s.runtime.as_deref());
    if let Some(job) = s.jobs.get("vr").filter(|j| {
        j["runtime_path"].as_str() == s.runtime.as_deref().and_then(|p| p.to_str())
            && j["mode"] == result["mode"]
    }) {
        if job["state"] == "running" {
            result["state"] = json!("checking");
            result["message"] = json!(message("checking"));
        } else if job["check"].is_object() {
            result["check"] = job["check"].clone();
            result["state"] = job["check"]["state"].clone();
            result["message"] = job["check"]["message"].clone();
        }
    }
    result
}
pub fn host_environment(mut env: Environment) -> Environment {
    let config = env
        .get("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env.get("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(files::home)
                .join(".config")
        });
    let registration = env.get("VR_PATHREG_OVERRIDE").cloned().unwrap_or_else(|| {
        config
            .join("openvr/openvrpaths.vrpath")
            .to_string_lossy()
            .into_owned()
    });
    if absolute(&registration) && Path::new(&registration).is_file() {
        env.entry("VR_PATHREG_OVERRIDE".into())
            .or_insert(registration);
    }
    env
}
pub fn probe(selected: &Value, env: Environment, cancel: &AtomicBool) -> Value {
    let mut env = host_environment(env);
    if let Some(path) = selected["path"].as_str() {
        env.insert("XR_RUNTIME_JSON".into(), path.into());
    } else {
        return json!({"state":"invalid"});
    }
    let value = match graphics::child("vr", Some(&env), cancel, 15) {
        Ok(v) => v,
        Err(Error::Cancelled) => return json!({"state":"cancelled"}),
        Err(_) => return json!({"state":"timeout"}),
    };
    if !value["state"]
        .as_str()
        .is_some_and(|s| ["ready", "failed", "headset_missing", "loader_missing"].contains(&s))
    {
        return json!({"state":"failed"});
    }
    if value["state"] == "ready"
        && (!["vendor_id", "device_id"]
            .iter()
            .all(|k| value[k].as_u64().is_some_and(|n| n <= u32::MAX as u64))
            || !value["device_uuid"]
                .as_str()
                .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            || !["instance_extensions", "device_extensions"]
                .iter()
                .all(|k| {
                    value[k].as_array().is_some_and(|a| {
                        a.len() <= 256
                            && a.iter().all(|v| {
                                v.as_str().is_some_and(|s| {
                                    s.starts_with("VK_")
                                        && s.len() <= 203
                                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                                })
                            })
                    })
                }))
    {
        return json!({"state":"failed"});
    }
    value
}
pub fn public(value: &Value) -> Value {
    let state = value["state"].as_str().unwrap_or("failed");
    json!({"state":state,"message":message(state),"checked_at":files::now()})
}
pub fn prepare(root: &Path, env: Environment) -> Result<Environment> {
    let mode = settings(Some(root))?;
    if mode == "off" {
        return Ok(env);
    }
    let mut env = host_environment(env);
    require(bridge(Some(root)), message("bridge_missing"))?;
    let selected = choose(&mode, &env).ok_or(Error::Invalid(message("missing")))?;
    require(selected["valid"] == true, message("invalid"))?;
    require(
        ![
            "DXVK_FILTER_DEVICE_NAME",
            "VKD3D_FILTER_DEVICE_NAME",
            "VKD3D_VULKAN_DEVICE",
        ]
        .iter()
        .any(|k| env.get(*k).is_some_and(|s| !s.is_empty())),
        "Entferne manuelle GPU-Namens- und Indexfilter, damit Spiel und VR-System dieselbe Grafikkarte verwenden können.",
    )?;
    let result = probe(&selected, env.clone(), &AtomicBool::new(false));
    require(
        result["state"] == "ready",
        message(result["state"].as_str().unwrap_or("failed")),
    )?;
    let folder = root.join("private/vr");
    files::private_dir(&folder)?;
    let manifest = folder.join("wineopenxr64.json");
    files::atomic_json(
        &manifest,
        &json!({"file_format_version":"1.0.0","runtime":{"library_path":r"C:\windows\system32\wineopenxr.dll"}}),
    )?;
    env.insert(
        "XR_RUNTIME_JSON".into(),
        selected["path"].as_str().unwrap_or("").into(),
    );
    env.insert(
        "WINEXR_RUNTIME_JSON".into(),
        format!("Z:{}", manifest.to_string_lossy().replace('/', r"\")),
    );
    let id = result["device_uuid"].as_str().unwrap_or("");
    if id != "00000000000000000000000000000000" {
        require(
            env.get("DXVK_FILTER_DEVICE_UUID")
                .is_none_or(|s| s.to_lowercase().replace('-', "") == id),
            "Spiel und VR-System verwenden verschiedene Grafikkarten. Wähle dieselbe GPU und prüfe erneut.",
        )?;
        env.insert("DXVK_FILTER_DEVICE_UUID".into(), id.into());
    }
    let mut instances = result["instance_extensions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for name in ["VK_KHR_surface", "VK_KHR_win32_surface"] {
        if !instances.iter().any(|v| v == name) {
            instances.push(json!(name));
        }
    }
    let names = |v: &[Value]| {
        v.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let registry = format!(
        "Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\VR]\n\"state\"=dword:00000001\n\"is_hmd_present\"=dword:00000001\n\"openxr_vulkan_instance_extensions\"=\"{}\"\n\"openxr_vulkan_device_extensions\"=\"{}\"\n\"openxr_vulkan_device_vid\"=dword:{:08x}\n\"openxr_vulkan_device_pid\"=dword:{:08x}\n",
        names(&instances),
        names(
            result["device_extensions"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        result["vendor_id"].as_u64().unwrap_or(0),
        result["device_id"].as_u64().unwrap_or(0)
    );
    let reg = folder.join(format!(".vr-{}.reg", uuid::Uuid::new_v4().simple()));
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(registry.encode_utf16().flat_map(u16::to_le_bytes));
    process::private_write(&reg, &bytes)?;
    let outcome = process::run(
        Command::new(runtime::wine(&root.join("runner")))
            .args(["regedit", "/S"])
            .arg(&reg)
            .env_clear()
            .envs(&env)
            .env("WINEPREFIX", root.join("local/msfs-prefix"))
            .env("WINEDEBUG", "-all")
            .env("WINEESYNC", "0")
            .env("WINEFSYNC", "0")
            .env_remove("WINE_DLL_FILE_MAP")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        Duration::from_secs(20),
        &AtomicBool::new(false),
    );
    let _ = std::fs::remove_file(reg);
    require(
        outcome.is_ok_and(|s| s.success()),
        "Die OpenXR-Anbindung konnte nicht eingerichtet werden. Beende Programme dieser Spielumgebung und versuche es erneut.",
    )?;
    Ok(env)
}
