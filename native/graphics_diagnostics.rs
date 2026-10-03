// SPDX-License-Identifier: MIT
//! Graphics intent and bounded evidence. No raw registry values, paths or log lines are exported.
use crate::{Result, files, games::Game, graphics, log_reader::regex};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
const DLLS: [&str; 11] = [
    "dxgi",
    "d3d11",
    "d3d10core",
    "d3d12",
    "d3d12core",
    "nvapi",
    "nvapi64",
    "nvofapi64",
    "nvngx",
    "_nvngx",
    "nvcuda",
];
const MODES: [&str; 6] = [
    "native",
    "builtin",
    "native,builtin",
    "builtin,native",
    "disabled",
    "other",
];
const FILTERS: [&str; 4] = [
    "DXVK_FILTER_DEVICE_NAME",
    "DXVK_FILTER_DEVICE_UUID",
    "VKD3D_FILTER_DEVICE_NAME",
    "VKD3D_VULKAN_DEVICE",
];
const LIBRARIES: [(&str, &str, &str); 7] = [
    ("dxgi", "dxvk", "dxgi"),
    ("d3d11", "dxvk", "d3d11"),
    ("d3d10core", "dxvk", "d3d10core"),
    ("d3d12", "vkd3d-proton", "d3d12"),
    ("d3d12core", "vkd3d-proton", "d3d12core"),
    ("nvapi64", "nvapi", "nvapi64"),
    ("nvofapi64", "nvapi", "nvofapi64"),
];
fn dll(name: &str) -> bool {
    DLLS.contains(&name.strip_prefix('*').unwrap_or(name))
}
fn mode(value: &str) -> String {
    let value = value
        .split(',')
        .map(|p| match p.trim().to_ascii_lowercase().as_str() {
            "n" => "native".into(),
            "b" => "builtin".into(),
            "d" | "" => "disabled".into(),
            other => other.to_owned(),
        })
        .collect::<Vec<String>>()
        .join(",");
    if MODES.contains(&value.as_str()) {
        value
    } else {
        "other".into()
    }
}
pub fn environment_overrides(env: &BTreeMap<String, String>) -> Value {
    let mut result = serde_json::Map::new();
    let raw = env
        .get("WINEDLLOVERRIDES")
        .map(String::as_str)
        .unwrap_or("");
    let raw = &raw[..raw.floor_char_boundary(raw.len().min(65536))];
    for entry in raw.split(';') {
        if let Some((names, value)) = entry.split_once('=') {
            for name in names.split(',') {
                let name = name.trim().to_ascii_lowercase();
                let name = name.strip_suffix(".dll").unwrap_or(&name);
                if dll(name) {
                    result.insert(name.into(), json!(mode(value)));
                }
            }
        }
    }
    result.into()
}
pub fn launch_record(
    root: &Path,
    report: &Value,
    env: &BTreeMap<String, String>,
    state: &str,
    at: &str,
) -> Value {
    let mut known = BTreeMap::new();
    if let Some(devices) = report["devices"].as_array() {
        for device in devices {
            if let Some(name) = device["name"].as_str() {
                known.insert(
                    name,
                    match device["vendor_id"].as_u64() {
                        Some(0x10de) => "nvidia",
                        Some(0x1002) => "amd",
                        Some(0x8086) => "intel",
                        _ => "other_device",
                    },
                );
            }
        }
    }
    let mut filters = BTreeMap::new();
    for key in FILTERS {
        if let Some(value) = env.get(key).filter(|v| !v.is_empty()) {
            filters.insert(key, known.get(value.as_str()).copied().unwrap_or("custom"));
        }
    }
    if report["adapter_selection"] == "nvidia_uuid"
        && env
            .get("DXVK_FILTER_DEVICE_UUID")
            .is_some_and(|v| !v.is_empty())
    {
        filters.insert("DXVK_FILTER_DEVICE_UUID", "nvidia");
    }
    let mut result = json!({"schema":1,"at":at,"launcher_version":crate::VERSION,"game_id":Game::for_runtime(root).map(|g|g.id()).unwrap_or("unknown"),"state":state,"nvidia":report["nvidia"].as_str().unwrap_or("unknown"),"gpu_filters":filters,"dll_overrides":environment_overrides(env),"nvapi_mode":match env.get("DXVK_ENABLE_NVAPI").map(String::as_str){Some("1")=>"enabled",Some("0")=>"disabled",_=>"unspecified"},"hide_nvidia":env.get("WINE_HIDE_NVIDIA_GPU").is_some_and(|v|v=="1"),"proton":crate::proton::diagnostic(Some(root))});
    if report["ngx_available"].is_boolean() {
        result["ngx_available"] = report["ngx_available"].clone();
    }
    if report["nvidia_mode"]
        .as_str()
        .is_some_and(|mode| graphics::MODES.contains(&mode))
    {
        result["nvidia_mode"] = report["nvidia_mode"].clone();
    }
    result
}
pub fn save_launch(root: &Path, value: &Value) -> bool {
    let write = || -> Result<()> {
        let folder = files::directory(&root.join("private"), true)?;
        files::atomic_at(&folder, "graphics-launch.json", &serde_json::to_vec(value)?)
    };
    write().is_ok()
}
pub fn load_launch(root: &Path) -> Value {
    let read = || -> Option<Value> {
        let data = files::read(&root.join("private/graphics-launch.json"), 8192).ok()?;
        let v = crate::cloud::json(&data).ok()?;
        if v["schema"] != 1
            || !matches!(
                v["state"].as_str(),
                Some("preparing" | "preparation_failed" | "prepared" | "spawned" | "spawn_failed")
            )
            || !matches!(
                v["nvidia"].as_str(),
                Some("ready" | "disabled" | "not_present" | "custom_runner" | "unknown")
            )
            || Game::select(v["game_id"].as_str()?).is_err()
            || !matches!(
                v["nvapi_mode"].as_str(),
                Some("enabled" | "disabled" | "unspecified")
            )
        {
            return None;
        }
        if !regex(r"^[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}(?:(?:a|b|rc|\.dev|\.post)[0-9]{1,3}|-[A-Za-z0-9][A-Za-z0-9.-]{0,30})?$").is_match(v["launcher_version"].as_str()?)||!regex(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?\+00:00$").is_match(v["at"].as_str()?)||chrono::DateTime::parse_from_rfc3339(v["at"].as_str()?).is_err(){return None;}
        let mut out: serde_json::Map<String, Value> = [
            "at",
            "launcher_version",
            "game_id",
            "state",
            "nvidia",
            "nvapi_mode",
        ]
        .into_iter()
        .map(|k| (k.into(), v[k].clone()))
        .collect();
        for key in ["gpu_filters", "dll_overrides"] {
            let map = v[key].as_object()?;
            for (name, value) in map {
                let value = value.as_str()?;
                if key == "gpu_filters" {
                    if !FILTERS.contains(&name.as_str())
                        || !["nvidia", "amd", "intel", "other_device", "custom"].contains(&value)
                    {
                        return None;
                    }
                } else if !dll(name) || !MODES.contains(&value) {
                    return None;
                }
            }
            out.insert(key.into(), v[key].clone());
        }
        for key in ["ngx_available", "hide_nvidia"] {
            if v[key].is_boolean() {
                out.insert(key.into(), v[key].clone());
            }
        }
        if v["nvidia_mode"]
            .as_str()
            .is_some_and(|mode| graphics::MODES.contains(&mode))
        {
            out.insert("nvidia_mode".into(), v["nvidia_mode"].clone());
        }
        let p = &v["proton"];
        if matches!(
            p["mode"].as_str(),
            Some("proton" | "flightdeck" | "unavailable")
        ) && matches!(
            p["loader"].as_str(),
            Some("portable" | "native" | "unknown")
        ) && p["version"]
            .as_str()
            .is_some_and(|s| regex(r"^[A-Za-z0-9_. +()-]{1,128}$").is_match(s))
        {
            out.insert(
                "proton".into(),
                json!({"mode":p["mode"],"version":p["version"],"loader":p["loader"]}),
            );
        }
        Some(out.into())
    };
    read().unwrap_or(Value::Null)
}
fn registry(path: &Path, executable: &str) -> Value {
    let bytes = match files::read(path, 8 * 1024 * 1024) {
        Ok(v) => v,
        Err(crate::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return json!({"status":"missing"});
        }
        Err(_) => return json!({"status":"unavailable"}),
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return json!({"status":"unavailable"});
    };
    let mut result = json!({"status":"read","global":{},"game":{}});
    let mut section = None;
    let global = r"software\\wine\\dlloverrides";
    let game = format!(
        "software\\\\wine\\\\appdefaults\\\\{}\\\\dlloverrides",
        executable.to_ascii_lowercase()
    );
    let entry = regex(r#"^"([^"\\]{1,32})"="([^"\\]{0,64})"$"#);
    for line in text.lines() {
        if let Some(part) = line.strip_prefix('[') {
            let name = part.split(']').next().unwrap_or("").to_ascii_lowercase();
            section = if name == global {
                Some("global")
            } else if name == game {
                Some("game")
            } else {
                None
            };
        } else if let Some(section) = section
            && let Some(c) = entry.captures(line)
        {
            let name = c[1].to_ascii_lowercase();
            let name = name.strip_suffix(".dll").unwrap_or(&name);
            if dll(name) {
                result[section][name] = json!(mode(&c[2]));
            }
        }
    }
    result
}
pub fn prefix_summary(root: &Path) -> Value {
    let inspect = || -> Result<Value> {
        let prefix = root.join("local/msfs-prefix");
        crate::proton::managed(root, &prefix.join("drive_c/windows/system32"))?;
        let game = Game::for_runtime(root)?;
        let system = prefix.join("drive_c/windows/system32");
        let runner = root.join("runner/files/lib/wine");
        let mut libraries = BTreeMap::new();
        for (name, component, library) in LIBRARIES {
            let path = system.join(format!("{name}.dll"));
            let current = graphics::digest(&path);
            let expected = graphics::digest(
                &runner
                    .join(component)
                    .join("x86_64-windows")
                    .join(format!("{library}.dll")),
            );
            let state = match (current, expected) {
                (Ok(a), Ok(b)) if a == b => "matches_runner",
                (Ok(a), Ok(_)) => {
                    if graphics::digest(&runner.join("x86_64-windows").join(format!("{name}.dll")))
                        .is_ok_and(|b| a == b)
                    {
                        "wine_builtin"
                    } else {
                        "different"
                    }
                }
                (Err(crate::Error::Io(e)), _) if e.kind() == std::io::ErrorKind::NotFound => {
                    "missing"
                }
                (_, Err(crate::Error::Io(e))) if e.kind() == std::io::ErrorKind::NotFound => {
                    if path.exists() {
                        "runner_unavailable"
                    } else {
                        "missing"
                    }
                }
                _ => "unavailable",
            };
            libraries.insert(name, state);
        }
        for name in ["nvngx", "_nvngx"] {
            libraries.insert(
                name,
                match graphics::digest(&system.join(format!("{name}.dll"))) {
                    Ok(_) => "present",
                    Err(crate::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                        "missing"
                    }
                    Err(_) => "unavailable",
                },
            );
        }
        Ok(
            json!({"status":"inspected","libraries":libraries,"user_overrides":registry(&prefix.join("user.reg"),game.executable()),"system_overrides":registry(&prefix.join("system.reg"),game.executable())}),
        )
    };
    inspect().unwrap_or(json!({"status":"unavailable"}))
}
pub fn log_summary(text: &str) -> Value {
    let codes: BTreeSet<String> = [
        "OUT_OF_HOST_MEMORY",
        "OUT_OF_DEVICE_MEMORY",
        "INITIALIZATION_FAILED",
        "DEVICE_LOST",
        "MEMORY_MAP_FAILED",
        "LAYER_NOT_PRESENT",
        "EXTENSION_NOT_PRESENT",
        "FEATURE_NOT_PRESENT",
        "INCOMPATIBLE_DRIVER",
        "TOO_MANY_OBJECTS",
        "FORMAT_NOT_SUPPORTED",
        "SURFACE_LOST_KHR",
        "OUT_OF_DATE_KHR",
    ]
    .into_iter()
    .map(|s| format!("VK_ERROR_{s}"))
    .chain(
        [
            "DEVICE_REMOVED",
            "DEVICE_HUNG",
            "DEVICE_RESET",
            "DRIVER_INTERNAL_ERROR",
            "UNSUPPORTED",
        ]
        .into_iter()
        .map(|s| format!("DXGI_ERROR_{s}")),
    )
    .collect();
    let found: BTreeSet<_> = regex(r"\b(?:VK_ERROR_|DXGI_ERROR_)[A-Z_]+\b")
        .find_iter(text)
        .map(|m| m.as_str())
        .filter(|v| codes.contains(*v))
        .collect();
    let mut components = Vec::new();
    for (name, pattern) in [
        ("vkd3d-proton", r"\b(?:info|warn|err):vkd3d-proton:"),
        ("dxvk", r"\bDXVK: v[0-9]"),
        ("dxvk-nvapi", r"\bDXVK-NVAPI\b"),
    ] {
        if regex(pattern).is_match(text) {
            components.push(name);
        }
    }
    let mut versions = BTreeMap::new();
    for (name, pattern) in [
        (
            "dxvk",
            r"\bDXVK: (v[0-9]{1,3}\.[0-9]{1,3}(?:\.[0-9]{1,3})?(?:-[0-9]{1,7}-g[0-9a-f]{7,40})?(?:-flightdeck-ll1)?)(?:\s|$)",
        ),
        (
            "vkd3d-proton",
            r"\bvkd3d-proton - applicationVersion: ([0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3})\.(?:\s|$)",
        ),
        (
            "vkd3d-proton-build",
            r"\bvkd3d-proton - build: ([0-9a-f]{7,40}\+?)\.(?:\s|$)",
        ),
    ] {
        let values: BTreeSet<_> = regex(pattern)
            .captures_iter(text)
            .map(|c| c[1].to_owned())
            .collect();
        if !values.is_empty() {
            versions.insert(name, values.into_iter().take(8).collect::<Vec<_>>());
        }
    }
    let mut observations = BTreeMap::new();
    for (name, pattern) in [
        (
            "present_without_render",
            r"\bApplication is presenting user index [0-9]{1,10}, but it has never been rendered to\.",
        ),
        ("adapter_name_filter_skips", r"\bSkipping: Device filter\b"),
        ("adapter_uuid_filter_skips", r"\bSkipping: UUID filter\b"),
        ("no_dxvk_adapters", r"\bDXVK: No adapters found\b"),
    ] {
        let count = regex(pattern).find_iter(text).count();
        if count > 0 {
            observations.insert(name, count);
        }
    }
    json!({"scope":"bounded_log_excerpt","observed_components":components,"error_symbols":found,"observed_versions":versions,"observations":observations})
}
