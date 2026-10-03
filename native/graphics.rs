// SPDX-License-Identifier: MIT
use crate::{Error, Result, error::require, files, process, runtime};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    time::Duration,
};
pub type Environment = BTreeMap<String, String>;
pub const MODES: [&str; 3] = ["auto", "compatibility", "features"];
pub fn settings(root: &Path) -> Result<String> {
    let path = root.join("private/graphics-settings.json");
    if !files::exists(&path) {
        return Ok("auto".into());
    }
    let value = runtime::value(&path)?;
    require(
        value["schema"] == 1
            && value["nvidia_mode"]
                .as_str()
                .is_some_and(|s| MODES.contains(&s)),
        "Die Grafikeinstellungen sind ungültig. Bitte unter Einrichtung erneut speichern.",
    )?;
    Ok(value["nvidia_mode"].as_str().unwrap_or("auto").into())
}
pub fn nvidia_present() -> bool {
    std::fs::read_dir("/sys/bus/pci/devices")
        .ok()
        .is_some_and(|entries| {
            entries.filter_map(|v| v.ok()).any(|entry| {
                let number = |name| {
                    std::fs::read_to_string(entry.path().join(name))
                        .ok()
                        .and_then(|v| {
                            u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok()
                        })
                };
                number("vendor") == Some(0x10de) && number("class").is_some_and(|v| v >> 16 == 3)
            })
        })
}
pub fn snapshot(root: Option<&Path>) -> Value {
    let present = nvidia_present();
    let mut value = json!({"nvidia_present":present,"available":present&&root.is_some_and(|r|r.join("runner/files/bin/wine").is_file()),"nvidia_mode":"auto","error":""});
    if let Some(root) = root {
        match settings(root) {
            Ok(mode) => value["nvidia_mode"] = json!(mode),
            Err(e) => value["error"] = json!(e.to_string()),
        }
    }
    value
}
pub fn child(
    mode: &str,
    env: Option<&Environment>,
    cancel: &AtomicBool,
    timeout: u64,
) -> Result<Value> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--native-probe", mode]).env("LC_ALL", "C");
    if let Some(env) = env {
        command.env_clear().envs(env);
    }
    let data = process::output(
        &mut command,
        Duration::from_secs(timeout),
        1024 * 1024,
        cancel,
    )?;
    Ok(serde_json::from_slice(&data)?)
}
pub fn probe(ids: bool) -> Value {
    let mut value = child("graphics", None, &AtomicBool::new(false), 5)
        .unwrap_or(json!({"status":"failed","devices":[]}));
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    value["session"] = json!(if ["x11", "wayland"].contains(&session.as_str()) {
        session.as_str()
    } else {
        "unknown"
    });
    if let Some(devices) = value["devices"].as_array_mut() {
        devices.truncate(16);
        devices.retain(|v| {
            v["name"].as_str().is_some_and(|s| {
                !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| (32..127).contains(&b))
            }) && v["vendor_id"]
                .as_u64()
                .is_some_and(|v| v <= u32::MAX as u64)
                && v["type"].as_u64().is_some_and(|v| v < 5)
        });
        if !ids {
            for device in devices.iter_mut() {
                if let Some(map) = device.as_object_mut() {
                    map.remove("device_uuid");
                }
            }
        }
        if !devices.is_empty() {
            let software = devices.iter().all(|v| v["type"] == 4);
            value["status"] = json!(if software { "software_only" } else { "ready" });
        }
    }
    value
}
fn append(env: &mut Environment, key: &str, text: &str, separator: &str) {
    let old = env
        .get(key)
        .map(|s| s.trim_end_matches([';', ' ', '\n']))
        .unwrap_or("");
    let value = if old.is_empty() {
        text.into()
    } else {
        format!("{old}{separator}{text}")
    };
    env.insert(key.into(), value);
}
pub fn apply_mode(env: &mut Environment, mode: &str) {
    if ["auto", "compatibility"].contains(&mode) {
        env.insert("PROTON_DISABLE_NVAPI".into(), "1".into());
        env.insert("DXVK_ENABLE_NVAPI".into(), "0".into());
        env.insert("PROTON_HIDE_NVIDIA_GPU".into(), "1".into());
        if !env
            .get("VKD3D_DISABLE_EXTENSIONS")
            .is_some_and(|s| s.split([';', ',']).any(|x| x == "VK_NV_low_latency2"))
        {
            append(env, "VKD3D_DISABLE_EXTENSIONS", "VK_NV_low_latency2", ";");
        }
        append(
            env,
            "DXVK_CONFIG",
            "dxvk.disableNvLowLatency2 = True; dxvk.latencySleep = False",
            "; ",
        );
        append(env, "WINEDLLOVERRIDES", "nvngx,_nvngx,*nvngx,*_nvngx=", ";");
    }
    if env
        .get("PROTON_HIDE_NVIDIA_GPU")
        .is_some_and(|s| s != "0" && !s.is_empty())
    {
        env.insert("WINE_HIDE_NVIDIA_GPU".into(), "1".into());
    }
    if env.get("WINE_HIDE_NVIDIA_GPU").is_some_and(|s| s == "1") {
        append(env, "DXVK_CONFIG", "dxgi.hideNvidiaGpu = True", "; ");
    }
}
pub fn select_adapter(env: &mut Environment, devices: &[Value]) -> &'static str {
    let selectors = [
        "DXVK_FILTER_DEVICE_NAME",
        "DXVK_FILTER_DEVICE_UUID",
        "VKD3D_FILTER_DEVICE_NAME",
        "VKD3D_VULKAN_DEVICE",
    ];
    if !selectors
        .iter()
        .any(|k| env.get(*k).is_some_and(|s| !s.is_empty()))
    {
        let discrete = devices
            .iter()
            .filter(|d| d["type"] == 2)
            .collect::<Vec<_>>();
        if discrete.len() == 1
            && discrete[0]["vendor_id"] == 0x10de
            && let Some(id) = discrete[0]["device_uuid"].as_str().filter(|s| {
                s.len() == 32
                    && s.bytes().all(|v| v.is_ascii_hexdigit())
                    && *s != "00000000000000000000000000000000"
            })
            && devices.iter().filter(|d| d["device_uuid"] == id).count() == 1
        {
            env.insert("DXVK_FILTER_DEVICE_UUID".into(), id.into());
            return "nvidia_uuid";
        }
        return "default";
    }
    if !["DXVK_FILTER_DEVICE_UUID", "VKD3D_VULKAN_DEVICE"]
        .iter()
        .any(|k| env.get(*k).is_some_and(|s| !s.is_empty()))
    {
        for (source, target) in [
            ("DXVK_FILTER_DEVICE_NAME", "VKD3D_FILTER_DEVICE_NAME"),
            ("VKD3D_FILTER_DEVICE_NAME", "DXVK_FILTER_DEVICE_NAME"),
        ] {
            if let Some(filter) = env.get(source).filter(|s| !s.is_empty()).cloned()
                && env.get(target).is_none_or(|s| s.is_empty())
                && devices
                    .iter()
                    .filter(|d| {
                        d["type"] != 4 && d["name"].as_str().is_some_and(|s| s.contains(&filter))
                    })
                    .count()
                    == 1
            {
                env.insert(target.into(), filter);
            }
        }
    }
    "explicit"
}
pub fn digest(path: &Path) -> Result<String> {
    let path = path.canonicalize()?;
    Ok(files::sha256(&files::read_public(
        &path,
        128 * 1024 * 1024,
    )?))
}
fn copy(source: &Path, target: &Path) -> Result<()> {
    let bytes = files::read_public(&source.canonicalize()?, 128 * 1024 * 1024)?;
    files::directory(
        target
            .parent()
            .ok_or(Error::Invalid("Ungültiger Grafikpfad."))?,
        false,
    )?;
    files::atomic(target, &bytes)
}
fn install_nvapi(root: &Path, directory: Option<&Path>) -> Result<Vec<String>> {
    let windows = root.join("local/msfs-prefix/drive_c/windows");
    let mut sources: BTreeMap<String, PathBuf> = [
        ("system32/nvapi64.dll", "x86_64-windows/nvapi64.dll"),
        ("system32/nvofapi64.dll", "x86_64-windows/nvofapi64.dll"),
        ("syswow64/nvapi.dll", "i386-windows/nvapi.dll"),
    ]
    .into_iter()
    .map(|(a, b)| (a.into(), root.join("runner/files/lib/wine/nvapi").join(b)))
    .collect();
    if let Some(dir) = directory {
        for name in ["nvngx.dll", "_nvngx.dll"] {
            if dir.join(name).is_file() {
                sources.insert(format!("system32/{name}"), dir.join(name));
            }
        }
    }
    let marker = root.join("private/nvidia-runtime.json");
    let previous = if files::exists(&marker) {
        runtime::value(&marker)?
    } else {
        json!({})
    };
    let previous = previous
        .as_object()
        .ok_or(Error::Invalid("Ungültige NVIDIA-Laufzeitbeschreibung."))?;
    require(
        previous.iter().all(|(k, v)| {
            [
                "system32/nvapi64.dll",
                "system32/nvofapi64.dll",
                "syswow64/nvapi.dll",
                "system32/nvngx.dll",
                "system32/_nvngx.dll",
            ]
            .contains(&k.as_str())
                && v.as_str().is_some_and(files::hex_digest)
        }),
        "Ungültige NVIDIA-Laufzeitbeschreibung.",
    )?;
    let hashes = sources
        .iter()
        .map(|(n, p)| Ok((n.clone(), digest(p)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut managed = previous.clone();
    let mut custom = Vec::new();
    for (name, source) in &sources {
        let target = windows.join(name);
        let current = if files::exists(&target) {
            Some(digest(&target)?)
        } else {
            None
        };
        if current.as_deref() == Some(hashes[name].as_str()) {
            managed.insert(name.clone(), json!(hashes[name]));
            continue;
        }
        if current.is_some() && current.as_deref() != previous.get(name).and_then(Value::as_str) {
            custom.push(name.rsplit('/').next().unwrap_or("").to_string());
            continue;
        }
        copy(source, &target)?;
        managed.insert(name.clone(), json!(hashes[name]));
    }
    for (name, hash) in previous {
        if !sources.contains_key(name) {
            let target = windows.join(name);
            if target.exists() && digest(&target).ok().as_deref() == hash.as_str() {
                std::fs::remove_file(target)?;
            }
            managed.remove(name);
        }
    }
    files::atomic_json(&marker, &managed)?;
    Ok(custom)
}
pub fn renderer(root: &Path) -> Result<&'static str> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("../resources/graphics"));
        candidates.push(parent.join("resources/graphics"));
    }
    if let Some(source) = crate::resources::source_root() {
        candidates.push(source.join("flightdeck/resources/graphics"));
    }
    let bundle = candidates.into_iter().find(|path| path.is_dir());
    let marker = root.join("private/renderer-runtime.json");
    let bundle = bundle.filter(|p| p.is_dir());
    if bundle.is_none() && !files::exists(&marker) {
        return Ok("runner");
    }
    let previous = if files::exists(&marker) {
        runtime::value(&marker)?
    } else {
        json!({})
    };
    let manifest = bundle
        .as_ref()
        .map(|p| runtime::value(&p.join("manifest.json")))
        .transpose()?;
    let names = if manifest.is_some() || previous.as_object().is_none_or(|m| m.is_empty()) {
        vec![
            "d3d12.dll",
            "d3d12core.dll",
            "dxgi.dll",
            "d3d11.dll",
            "d3d10core.dll",
        ]
    } else {
        previous
            .as_object()
            .ok_or(Error::Invalid("Ungültige Grafik-Laufzeitbeschreibung."))?
            .keys()
            .map(String::as_str)
            .collect()
    };
    require(
        names.iter().all(|n| {
            [
                "d3d12.dll",
                "d3d12core.dll",
                "dxgi.dll",
                "d3d11.dll",
                "d3d10core.dll",
            ]
            .contains(n)
        }),
        "Ungültige Grafik-Laufzeitbeschreibung.",
    )?;
    let mut sources = BTreeMap::new();
    let mut original = serde_json::Map::new();
    for name in &names {
        let source = root
            .join("runner/files/lib/wine")
            .join(if ["d3d12.dll", "d3d12core.dll"].contains(name) {
                "vkd3d-proton"
            } else {
                "dxvk"
            })
            .join("x86_64-windows")
            .join(name);
        original.insert((*name).into(), json!(digest(&source)?));
        sources.insert((*name).to_string(), source);
    }
    let mut expected = Value::Object(original.clone());
    let mut state = "runner";
    if let (Some(manifest), Some(bundle)) = (&manifest, &bundle) {
        require(
            manifest["schema"] == 2,
            "Ungültige Grafik-Laufzeitbeschreibung.",
        )?;
        for name in &names {
            require(
                manifest["files"][name]
                    .as_str()
                    .is_some_and(files::hex_digest)
                    && digest(&bundle.join(name))? == manifest["files"][name],
                "Die Grafik-Laufzeitdateien sind ungültig.",
            )?;
        }
        if manifest["base"] == Value::Object(original.clone()) {
            expected = manifest["files"].clone();
            state = "backport";
            for name in &names {
                sources.insert((*name).into(), bundle.join(name));
            }
        }
    }
    let system = root.join("local/msfs-prefix/drive_c/windows/system32");
    files::directory(&system, false)?;
    let mut current = BTreeMap::new();
    for name in &names {
        let target = system.join(name);
        let hash = if files::exists(&target) {
            Some(digest(&target)?)
        } else {
            None
        };
        if hash.as_ref().is_some_and(|v| {
            v != original[*name].as_str().unwrap_or("")
                && v != expected[name].as_str().unwrap_or("")
                && v != previous[name].as_str().unwrap_or("")
        }) {
            return Ok("custom");
        }
        current.insert((*name).to_string(), hash);
    }
    for name in &names {
        if current[*name].as_deref() != expected[name].as_str() {
            copy(&sources[*name], &system.join(name))?;
        }
    }
    if state == "backport" {
        files::atomic_json(&marker, &expected)?;
    } else if files::exists(&marker) {
        std::fs::remove_file(marker)?;
    }
    Ok(state)
}
pub fn prepare(root: &Path, mut env: Environment) -> Result<(Environment, Value)> {
    if !root.join("runner/files/bin/wine").is_file() {
        return Ok((env, json!({"nvidia":"custom_runner"})));
    }
    if !nvidia_present() {
        return Ok((env, json!({"nvidia":"not_present"})));
    }
    let mode = settings(root)?;
    apply_mode(&mut env, &mode);
    let mut report = probe(true);
    require(
        report["status"] == "ready"
            && report["devices"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v["vendor_id"] == 0x10de && v["type"] != 4)),
        "NVIDIA wurde erkannt, aber Vulkan ist nicht verfügbar. Bitte den empfohlenen NVIDIA-Treiber der Distribution installieren und Linux neu starten.",
    )?;
    if !files::exists(&root.join("private/proton-selection.json")) {
        renderer(root)?;
    }
    let devices = report["devices"]
        .as_array_mut()
        .ok_or(Error::Invalid("Ungültige Grafikausgabe."))?;
    let selection = select_adapter(&mut env, devices);
    for device in devices {
        if let Some(map) = device.as_object_mut() {
            map.remove("device_uuid");
        }
    }
    report["adapter_selection"] = json!(selection);
    report["nvidia_mode"] = json!(mode);
    report["hide_nvidia"] = json!(env.get("WINE_HIDE_NVIDIA_GPU").is_some_and(|s| s == "1"));
    for (k, v) in [
        ("__GLVND_DISALLOW_PATCHING", "1"),
        ("DXVK_LOG_LEVEL", "info"),
        ("VKD3D_DEBUG", "warn"),
    ] {
        env.entry(k.into()).or_insert(v.into());
    }
    if env
        .get("PROTON_DISABLE_NVAPI")
        .is_some_and(|s| s != "0" && !s.is_empty())
        || env.get("DXVK_ENABLE_NVAPI").is_some_and(|s| s == "0")
    {
        env.insert("DXVK_ENABLE_NVAPI".into(), "0".into());
        append(
            &mut env,
            "WINEDLLOVERRIDES",
            "nvapi,nvapi64,nvofapi64,*nvapi,*nvapi64,*nvofapi64=",
            ";",
        );
        report["nvidia"] = json!("disabled");
        report["game_settings"] = crate::graphics_settings::prepare(root, true)?;
        return Ok((env, report));
    }
    let directory = env
        .get("NVIDIA_WINE_DLL_DIR")
        .cloned()
        .or_else(|| {
            child("nvidia-directory", None, &AtomicBool::new(false), 5)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
        })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.join("nvngx.dll").is_file());
    let custom = install_nvapi(root, directory.as_deref())?;
    let overrides = env.get("WINEDLLOVERRIDES").cloned().unwrap_or_default();
    let present = overrides
        .split(';')
        .filter_map(|s| s.split_once('='))
        .flat_map(|(s, _)| s.split(','))
        .map(|s| s.trim().to_lowercase().trim_end_matches(".dll").to_string())
        .collect::<Vec<_>>();
    for (name, mode) in [
        ("nvapi", "n"),
        ("nvapi64", "n"),
        ("nvofapi64", "n"),
        ("nvcuda", "b"),
    ] {
        if !present.iter().any(|s| s == name) {
            append(&mut env, "WINEDLLOVERRIDES", &format!("{name}={mode}"), ";");
        }
    }
    env.entry("DXVK_ENABLE_NVAPI".into()).or_insert("1".into());
    if let Some(dir) = &directory {
        env.insert(
            "NVIDIA_WINE_DLL_DIR".into(),
            dir.to_string_lossy().into_owned(),
        );
    }
    report["nvidia"] = json!("ready");
    report["ngx_available"] = json!(directory.is_some());
    report["custom_dlls"] = json!(custom);
    report["game_settings"] = crate::graphics_settings::prepare(root, false)?;
    Ok((env, report))
}
