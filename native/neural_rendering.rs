// SPDX-License-Identifier: MIT
//! Opt-in Linux HIP binding. Overlays exist only in the licensed launch view.
use crate::{
    Error, Result, error::require, files, games::Game, graphics, neural_assets, wine_processes,
};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::Write,
    os::{fd::AsRawFd, unix::fs::symlink},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const SETTINGS: &str = "private/neural-rendering.json";
const CHANGED: &str = "The neural-rendering inputs or runner changed. Reconfigure the experimental profile or disable it with 'neural-rendering --runtime PATH disable'.";

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    runtime: PathBuf,
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Show the installation's experimental profile without loading drivers.
    Status,
    /// Test the local bundle, selected Wine runner and ROCm; does not launch MSFS.
    Probe {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        hip_library: PathBuf,
    },
    /// Opt into experimental AMD neural rendering (MSFS rendering is unverified).
    EnableExperimental {
        #[arg(long)]
        bundle: PathBuf,
        /// Complete cache from neural-weights or the pinned upstream converter.
        #[arg(long)]
        weights: PathBuf,
        #[arg(long)]
        hip_library: PathBuf,
    },
    /// Use the existing renderer again; never needs the experimental inputs.
    Disable,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    bundle: PathBuf,
    weights: PathBuf,
    weights_sha256: String,
    hip_library: PathBuf,
    hip_sha256: String,
    runner: PathBuf,
    ntdll_sha256: String,
    gpu: String,
    arch: String,
    #[serde(default)]
    pci_bus_id: String,
    #[serde(default)]
    vulkan_uuid: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    schema: u8,
    profile: Option<Profile>,
}

fn settings(root: &Path) -> Result<Option<Profile>> {
    if !files::exists(&root.join(SETTINGS)) {
        return Ok(None);
    }
    let value: Settings = files::json(&root.join(SETTINGS), 64 * 1024)?;
    require(value.schema == 1, CHANGED)?;
    if let Some(p) = &value.profile {
        require(
            [&p.bundle, &p.weights, &p.hip_library, &p.runner]
                .iter()
                .all(|v| valid_path(v))
                && [&p.weights_sha256, &p.hip_sha256, &p.ntdll_sha256]
                    .iter()
                    .all(|v| files::hex_digest(v))
                && !p.gpu.is_empty()
                && p.gpu.len() <= 256
                && !p.gpu.chars().any(char::is_control)
                && !p.gpu.contains([';', '"'])
                && valid_pci(&p.pci_bus_id)
                && valid_uuid(&p.vulkan_uuid)
                && ["gfx1030", "gfx1200", "gfx1201"].contains(&p.arch.as_str()),
            CHANGED,
        )?;
    }
    Ok(value.profile)
}
fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|v| {
            v.len() <= 4096 && !v.contains([':', '\\', '\0']) && !v.chars().any(char::is_control)
        })
}
fn canonical(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    require(
        valid_path(&path),
        "Neural-rendering paths must be absolute UTF-8 paths without colons, backslashes or control characters.",
    )?;
    Ok(path)
}
fn hip_digest(path: &Path) -> Result<String> {
    let bytes = files::read_public(path, 128 * 1024 * 1024)?;
    require(
        bytes.get(..6) == Some(b"\x7fELF\x02\x01") && bytes.get(18..20) == Some(&[62, 0]),
        "Select the native x86_64 ROCm libamdhip64.so.7 library.",
    )?;
    Ok(files::sha256(&bytes))
}
fn runner(root: &Path) -> Result<(PathBuf, String)> {
    let runner = root.join("runner").canonicalize()?;
    for dir in ["files/lib/wine", "files/lib64/wine"] {
        let path = runner.join(dir).join("x86_64-windows/ntdll.dll");
        if !path.is_file() {
            continue;
        }
        let bytes = files::read_public(&path.canonicalize()?, 32 * 1024 * 1024)?;
        require(
            neural_assets::exports(&bytes, b"__wine_get_unix_env"),
            "This runner lacks the Wine Unix-environment bridge. Select a compatible Proton version before enabling neural rendering.",
        )?;
        return Ok((runner, files::sha256(&bytes)));
    }
    Err(Error::Invalid(
        "The selected runner has no supported 64-bit Wine ntdll. Neural rendering is unavailable.",
    ))
}
fn hardware(library: &Path) -> Result<Value> {
    let env: graphics::Environment = std::env::vars().collect();
    let mut hip_env = env.clone();
    hip_env.insert(
        "FLIGHTDECK_HIP_LIBRARY".into(),
        library.to_string_lossy().into_owned(),
    );
    if let Some(parent) = library.parent() {
        let mut paths: Vec<_> = [parent.join("rocm_sysdeps/lib"), parent.join("llvm/lib")]
            .into_iter()
            .filter(|p| p.is_dir())
            .collect();
        if !paths.is_empty() {
            if let Some(old) = env.get("LD_LIBRARY_PATH") {
                paths.extend(std::env::split_paths(old));
            }
            hip_env.insert(
                "LD_LIBRARY_PATH".into(),
                std::env::join_paths(paths)
                    .map_err(|_| Error::Invalid(CHANGED))?
                    .into_string()
                    .map_err(|_| Error::Invalid(CHANGED))?,
            );
        }
    }
    // Drivers run in children even from CLI/launch preparation. Failure/timeout
    // never makes an unsupported machine appear ready.
    let hip = graphics::child("hip", Some(&hip_env), &AtomicBool::new(false), 20)
        .map_err(|_| Error::Invalid("ROCm/HIP 7 failed its device or memory check. Install a matching ROCm runtime and check GPU access."))?;
    let candidates: Vec<_> = hip["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["supported"] == true && v["memory_roundtrip"] == true)
        .collect();
    require(
        candidates.len() == 1,
        "This experiment requires exactly one compatible HIP GPU: gfx1030, gfx1200 or gfx1201. The native bundle must also match its architecture.",
    )?;
    let chosen = candidates[0];
    let vulkan = graphics::child("graphics", Some(&env), &AtomicBool::new(false), 5)?;
    let mut matched = chosen.clone();
    matched["vulkan_uuid"] = json!(vulkan_uuid(chosen, &vulkan)?);
    Ok(matched)
}
fn valid_uuid(value: &str) -> bool {
    value.len() == 32
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value.bytes().any(|b| b != b'0')
}
fn valid_pci(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 12
        && b[4] == b':'
        && b[7] == b':'
        && b[10] == b'.'
        && b.iter()
            .enumerate()
            .all(|(i, b)| [4, 7, 10].contains(&i) || b.is_ascii_hexdigit())
        && matches!(b[8], b'0' | b'1')
        && (b'0'..=b'7').contains(&b[11])
}
fn vulkan_uuid(hip: &Value, vulkan: &Value) -> Result<String> {
    let pci = hip["pci_bus_id"]
        .as_str()
        .filter(|v| valid_pci(v))
        .ok_or(Error::Invalid("The HIP GPU has no valid PCI identity."))?;
    let devices = vulkan["devices"]
        .as_array()
        .ok_or(Error::Invalid(CHANGED))?;
    let matches: Vec<_> = devices
        .iter()
        .filter(|d| {
            d["vendor_id"] == 0x1002
                && d["type"] == 2
                && d["pci_bus_id"]
                    .as_str()
                    .is_some_and(|v| v.eq_ignore_ascii_case(pci))
        })
        .collect();
    require(
        matches.len() == 1,
        "The HIP GPU cannot be matched unambiguously to an AMD Vulkan PCI device.",
    )?;
    let uuid = matches[0]["device_uuid"]
        .as_str()
        .filter(|v| valid_uuid(v))
        .ok_or(Error::Invalid(
            "The matching Vulkan GPU has no valid device UUID.",
        ))?;
    require(
        devices
            .iter()
            .filter(|d| {
                d["device_uuid"]
                    .as_str()
                    .is_some_and(|v| v.eq_ignore_ascii_case(uuid))
            })
            .count()
            == 1,
        "The Vulkan GPU UUID is ambiguous.",
    )?;
    Ok(uuid.to_ascii_lowercase())
}
fn profile_check(root: &Path, profile: &Profile) -> Result<()> {
    require(
        Game::for_runtime(root)? == Game::Msfs2024,
        "The experimental neural-rendering profile currently targets MSFS 2024 only.",
    )?;
    let (runner, hash) = runner(root)?;
    require(
        runner == profile.runner
            && hash == profile.ntdll_sha256
            && hip_digest(&profile.hip_library)? == profile.hip_sha256,
        CHANGED,
    )?;
    let gpu = hardware(&profile.hip_library)?;
    require(
        gpu["name"] == profile.gpu
            && gpu["arch"] == profile.arch
            && gpu["pci_bus_id"] == profile.pci_bus_id
            && gpu["vulkan_uuid"] == profile.vulkan_uuid,
        CHANGED,
    )
}

pub fn status(root: &Path) -> Result<Value> {
    let p = settings(root)?;
    Ok(
        json!({"mode":if p.is_some(){"amd_hip_experimental"}else{"off"},
        "gpu":p.as_ref().map(|p|&p.gpu),"arch":p.as_ref().map(|p|&p.arch),
        "pci_bus_id":p.as_ref().map(|p|&p.pci_bus_id),"vulkan_uuid":p.as_ref().map(|p|&p.vulkan_uuid),
        "bundle_version":p.as_ref().map(|p|if p.arch=="gfx1030"{"0.2.6.1-flightdeck.gfx1030.4"}else{"0.2.6.1"}),"msfs_rendering_verified":false,
        "scope":"configuration_only"}),
    )
}
pub fn cli(options: Options) -> Result<Value> {
    let root = canonical(&options.runtime)?;
    match options.action {
        Action::Status => status(&root),
        Action::Probe {
            bundle,
            hip_library,
        } => {
            let bundle = canonical(&bundle)?;
            let data = neural_assets::bundle(&bundle)?;
            runner(&root)?;
            let library = canonical(&hip_library)?;
            hip_digest(&library)?;
            let gpu = hardware(&library)?;
            neural_assets::check_architecture(&data, gpu["arch"].as_str().unwrap_or(""))?;
            Ok(json!({"state":"prerequisites_checked","gpu":gpu,"msfs_rendering_verified":false}))
        }
        Action::Disable => {
            files::private_dir(&root.join("private"))?;
            let _lease = files::Lease::acquire(&root.join("private/play.lock"), true)?;
            wine_processes::idle(&root.join("local/msfs-prefix"))?;
            files::atomic_json(
                &root.join(SETTINGS),
                &Settings {
                    schema: 1,
                    profile: None,
                },
            )?;
            status(&root)
        }
        Action::EnableExperimental {
            bundle,
            weights,
            hip_library,
        } => {
            files::private_dir(&root.join("private"))?;
            let _lease = files::Lease::acquire(&root.join("private/play.lock"), true)?;
            wine_processes::idle(&root.join("local/msfs-prefix"))?;
            require(
                Game::for_runtime(&root)? == Game::Msfs2024,
                "The experimental neural-rendering profile currently targets MSFS 2024 only.",
            )?;
            let bundle = canonical(&bundle)?;
            let weights = canonical(&weights)?;
            let hip_library = canonical(&hip_library)?;
            let data = neural_assets::bundle(&bundle)?;
            let (runner, ntdll_sha256) = runner(&root)?;
            let hip_sha256 = hip_digest(&hip_library)?;
            let gpu = hardware(&hip_library)?;
            neural_assets::check_architecture(&data, gpu["arch"].as_str().unwrap_or(""))?;
            let weights_sha256 = neural_assets::weights(&weights, None)?;
            let profile = Profile {
                bundle,
                weights,
                weights_sha256,
                hip_library,
                hip_sha256,
                runner,
                ntdll_sha256,
                gpu: gpu["name"].as_str().ok_or(Error::Invalid(CHANGED))?.into(),
                arch: gpu["arch"].as_str().ok_or(Error::Invalid(CHANGED))?.into(),
                pci_bus_id: gpu["pci_bus_id"]
                    .as_str()
                    .ok_or(Error::Invalid(CHANGED))?
                    .into(),
                vulkan_uuid: gpu["vulkan_uuid"]
                    .as_str()
                    .ok_or(Error::Invalid(CHANGED))?
                    .into(),
            };
            collisions(&Game::for_runtime(&root)?.path(&root))?;
            files::atomic_json(
                &root.join(SETTINGS),
                &Settings {
                    schema: 1,
                    profile: Some(profile),
                },
            )?;
            status(&root)
        }
    }
}

const OVERLAYS: [(&str, &str); 5] = [
    ("bin/ReShade64.dll", "d3d12.dll"),
    ("bin/dlss5-d3d12.dll", "dlss5-d3d12.dll"),
    ("bin/d3d12core.dll", "d3d12core.dll"),
    ("bin/dlss5_hip.dll", "dlss5_hip.dll"),
    ("bin/dlss5-amd.addon64", "dlss5-amd.addon64"),
];
fn collisions(view: &Path) -> Result<()> {
    for entry in fs::read_dir(view)? {
        let name = entry?.file_name().to_string_lossy().to_lowercase();
        require(
            !OVERLAYS.iter().any(|(_, target)| name == *target)
                && ![
                    "reshade.ini",
                    "dlss5-amd",
                    "dxgi.dll",
                    "version.dll",
                    "winmm.dll",
                    ".dlssnr-linux",
                ]
                .contains(&name.as_str())
                && !name.ends_with(".addon64"),
            "An existing graphics proxy or add-on conflicts with neural rendering. Remove it with its own installer before enabling this profile.",
        )?;
    }
    Ok(())
}

fn overrides(value: &str) -> String {
    let mut result = Vec::new();
    for entry in value.split(';') {
        let Some((names, mode)) = entry.split_once('=') else {
            continue;
        };
        for name in names.split(',') {
            let key = name.trim().trim_start_matches('*').to_ascii_lowercase();
            let key = key.strip_suffix(".dll").unwrap_or(&key);
            if !["version", "dlss5_hip", "d3d12", "d3d12core", "dxgi"].contains(&key) {
                result.push(format!("{name}={mode}"));
            }
        }
    }
    result.push("version=b;dlss5_hip=n;d3d12=n,b;d3d12core=n,b;dxgi=n".into());
    result.join(";")
}

/// Retain the sealed preload image until Wine exits. /proc/PID/fd is free of
/// whitespace even when the runtime/package path contains spaces. The fd is
/// CLOEXEC and need not be lent to Wine or any account/service helper.
pub struct Preload {
    _image: File,
}
fn overlay(
    view: &Path,
    logs: &Path,
    profile: &Profile,
    bundle: &BTreeMap<String, Vec<u8>>,
    env: &mut BTreeMap<OsString, OsString>,
) -> Result<Preload> {
    require(
        valid_uuid(&profile.vulkan_uuid)
            && !profile.gpu.is_empty()
            && profile.gpu.len() <= 256
            && !profile.gpu.chars().any(char::is_control)
            && !profile.gpu.contains([';', '"']),
        CHANGED,
    )?;
    require(
        view.to_string_lossy().encode_utf16().count() < 210,
        "The launch path is too long for this experimental ReShade add-on.",
    )?;
    collisions(view)?;
    for key in [
        "DXVK_FILTER_DEVICE_UUID",
        "VKD3D_VULKAN_DEVICE",
        "DXVK_FILTER_DEVICE_NAME",
        "VKD3D_FILTER_DEVICE_NAME",
        "VK_ICD_FILENAMES",
        "VK_DRIVER_FILES",
        "DRI_PRIME",
        "HIP_VISIBLE_DEVICES",
        "ROCR_VISIBLE_DEVICES",
        "CUDA_VISIBLE_DEVICES",
    ] {
        require(
            env.get(std::ffi::OsStr::new(key))
                .is_none_or(|v| v.is_empty()),
            "Remove inherited GPU selectors before testing neural rendering; Flightdeck must match the HIP and Vulkan GPU.",
        )?;
    }
    for (source, target) in OVERLAYS {
        files::atomic(
            &view.join(target),
            bundle.get(source).ok_or(Error::Invalid(CHANGED))?,
        )?;
    }
    files::atomic(
        &view.join("ReShade.ini"),
        b"[PROXY]\nEnableProxyLibrary=1\nProxyLibrary=.\\dlss5-d3d12.dll\n",
    )?;
    let lab = view.join("DLSS5-AMD");
    files::private_dir(&lab)?;
    files::atomic(
        &lab.join("native-game-flags.txt"),
        bundle
            .get("flags/native-game-flags.txt")
            .ok_or(Error::Invalid(CHANGED))?,
    )?;
    symlink(&profile.weights, lab.join("native-game-tiled-assets"))?;
    files::private_dir(logs)?;
    symlink(logs, lab.join("logs"))?;
    let mut image = File::from(rustix::fs::memfd_create(
        c"flightdeck-neural-bridge",
        rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
    )?);
    image.write_all(
        bundle
            .get("bin/libdlss5_hip.so")
            .ok_or(Error::Invalid(CHANGED))?,
    )?;
    rustix::fs::fcntl_add_seals(
        &image,
        rustix::fs::SealFlags::WRITE
            | rustix::fs::SealFlags::GROW
            | rustix::fs::SealFlags::SHRINK
            | rustix::fs::SealFlags::SEAL,
    )?;
    let preload = format!("/proc/{}/fd/{}", std::process::id(), image.as_raw_fd());
    let old = env
        .get(std::ffi::OsStr::new("LD_PRELOAD"))
        .cloned()
        .unwrap_or_default();
    let mut preload = OsString::from(preload);
    if !old.is_empty() {
        preload.push(":");
        preload.push(old);
    }
    let dlls = overrides(
        env.get(std::ffi::OsStr::new("WINEDLLOVERRIDES"))
            .and_then(|v| v.to_str())
            .unwrap_or(""),
    );
    env.retain(|k, _| !k.to_string_lossy().starts_with("DLSS5_"));
    env.insert("LD_PRELOAD".into(), preload);
    env.insert("WINEDLLOVERRIDES".into(), dlls.into());
    env.insert("DLSS5_HIP".into(), "1".into());
    env.insert(
        "DLSS5_HIP_LIBRARY".into(),
        profile.hip_library.as_os_str().into(),
    );
    env.insert(
        "DLSS5_HIP_WEIGHTS".into(),
        profile.weights.as_os_str().into(),
    );
    env.insert("DLSS5_TEMPORAL_BLEND".into(), "0.5".into());
    env.insert("DLSS5_MH_PROD".into(), "1".into());
    if profile.arch == "gfx1030" {
        // Measured faster with the software MMA backend; RDNA4 keeps its fused kernel.
        env.insert("DLSS5_C32_SPLIT".into(), "1".into());
    }
    if let Some(parent) = profile.hip_library.parent() {
        let dirs: Vec<_> = [parent.join("rocm_sysdeps/lib"), parent.join("llvm/lib")]
            .into_iter()
            .filter(|p| p.is_dir())
            .collect();
        if !dirs.is_empty() {
            env.insert(
                "DLSS5_HIP_DEP_DIRS".into(),
                std::env::join_paths(dirs).map_err(|_| Error::Invalid(CHANGED))?,
            );
        }
    }
    // DXVK and native HIP may describe the same PCI device differently. Select
    // the physical Vulkan UUID first, then give that sole adapter its actual HIP
    // product name for the upstream bridge's exact-name API. Vendor/device IDs
    // and the adapter LUID stay intact; D3D12 follows the selected DXGI adapter.
    env.insert(
        "DXVK_FILTER_DEVICE_UUID".into(),
        profile.vulkan_uuid.clone().into(),
    );
    let config = env
        .get(std::ffi::OsStr::new("DXVK_CONFIG"))
        .map(|v| v.to_str().ok_or(Error::Invalid(CHANGED)))
        .transpose()?
        .unwrap_or("");
    let config = format!("{config}; dxgi.customDeviceDesc = \"{}\"", profile.gpu);
    env.insert("DXVK_CONFIG".into(), config.into());
    Ok(Preload { _image: image })
}

/// Called after constructing the per-launch image tree, immediately before Wine.
/// The account broker, launcher, Wine health probes and helpers keep their env.
pub fn prepare(
    root: &Path,
    view: &Path,
    env: &mut BTreeMap<OsString, OsString>,
) -> Result<Option<Preload>> {
    let Some(profile) = settings(root)? else {
        return Ok(None);
    };
    profile_check(root, &profile)?;
    let bundle = neural_assets::bundle(&profile.bundle)?;
    neural_assets::check_architecture(&bundle, &profile.arch)?;
    neural_assets::weights(&profile.weights, Some(&profile.weights_sha256))?;
    let preload = overlay(
        view,
        &root.join("private/neural-rendering-logs"),
        &profile,
        &bundle,
        env,
    )?;
    eprintln!(
        "Flightdeck: experimental AMD neural-rendering bridge enabled; MSFS image output remains unverified. Private HIP logs: private/neural-rendering-logs."
    );
    Ok(Some(preload))
}

#[cfg(test)]
#[path = "neural_tests.rs"]
mod tests;
