// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{neural_assets, process};
use std::{
    os::unix::fs::MetadataExt,
    process::{Command, Stdio},
    time::Duration,
};

fn profile(base: &Path) -> Profile {
    let weights = base.join("weights");
    files::private_dir(&weights).unwrap();
    Profile {
        bundle: base.into(),
        weights,
        weights_sha256: "a".repeat(64),
        hip_library: base.join("libamdhip64.so.7"),
        hip_sha256: "b".repeat(64),
        runner: base.into(),
        ntdll_sha256: "c".repeat(64),
        gpu: "AMD Radeon RX 9070 XT".into(),
        arch: "gfx1201".into(),
        pci_bus_id: "0000:03:00.0".into(),
        vulkan_uuid: "abcdef0123456789abcdef0123456789".into(),
    }
}
fn bundle() -> BTreeMap<String, Vec<u8>> {
    OVERLAYS
        .iter()
        .map(|(name, _)| (name.to_string(), format!("synthetic {name}").into_bytes()))
        .chain([
            (
                "flags/native-game-flags.txt".into(),
                b"DLSS5_HIP=1\n".to_vec(),
            ),
            ("bin/libdlss5_hip.so".into(), b"synthetic preload".to_vec()),
        ])
        .collect()
}

#[test]
fn disabled_profile_never_touches_view_or_environment() {
    let temp = tempfile::tempdir().unwrap();
    let mut env = BTreeMap::from([("LD_PRELOAD".into(), "existing.so".into())]);
    let before = env.clone();
    assert!(
        prepare(temp.path(), &temp.path().join("does-not-exist"), &mut env)
            .unwrap()
            .is_none()
    );
    assert_eq!(env, before);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn overlay_is_private_preserves_gdk_overrides_and_preload_lifetime() {
    let temp = tempfile::Builder::new()
        .prefix("neural test ")
        .tempdir()
        .unwrap();
    let view = temp.path().join("view with spaces");
    files::private_dir(&view).unwrap();
    files::atomic(&view.join("original.dat"), b"unchanged").unwrap();
    let p = profile(temp.path());
    let mut env = BTreeMap::from([
        (
            "WINEDLLOVERRIDES".into(),
            "*D3D12.dll,xgameruntime=n;version=n;kernel32=b;dxgi=b".into(),
        ),
        ("DLSS5_HIP_BRIDGE".into(), "stale-pointer".into()),
        ("LD_LIBRARY_PATH".into(), "/existing".into()),
        ("DXVK_CONFIG".into(), "dxgi.maxFrameLatency = 1".into()),
    ]);
    let before = env.clone();
    let lease = overlay(&view, &temp.path().join("logs"), &p, &bundle(), &mut env).unwrap();
    assert_eq!(
        env.get(std::ffi::OsStr::new("LD_LIBRARY_PATH")),
        before.get(std::ffi::OsStr::new("LD_LIBRARY_PATH"))
    );
    assert!(!env.contains_key(std::ffi::OsStr::new("DLSS5_HIP_BRIDGE")));
    assert!(!env.contains_key(std::ffi::OsStr::new("DXVK_FILTER_DEVICE_NAME")));
    assert!(!env.contains_key(std::ffi::OsStr::new("VKD3D_FILTER_DEVICE_NAME")));
    assert_eq!(
        env[std::ffi::OsStr::new("DXVK_FILTER_DEVICE_UUID")],
        OsString::from(&p.vulkan_uuid)
    );
    assert_eq!(
        env[std::ffi::OsStr::new("DXVK_CONFIG")],
        OsString::from(
            "dxgi.maxFrameLatency = 1; dxgi.customDeviceDesc = \"AMD Radeon RX 9070 XT\""
        )
    );
    let dlls = env[std::ffi::OsStr::new("WINEDLLOVERRIDES")]
        .to_str()
        .unwrap();
    assert!(dlls.contains("xgameruntime=n") && dlls.contains("kernel32=b"));
    assert!(!dlls.contains("dxgi=b") && !dlls.contains("version=n"));
    assert!(dlls.contains("dxgi=n"));
    let preload = PathBuf::from(&env[std::ffi::OsStr::new("LD_PRELOAD")]);
    assert!(!preload.to_str().unwrap().contains(char::is_whitespace));
    assert_eq!(fs::read(&preload).unwrap(), b"synthetic preload");
    assert!(
        rustix::fs::fcntl_get_seals(&lease._image)
            .unwrap()
            .contains(rustix::fs::SealFlags::WRITE)
    );
    assert_eq!(fs::read(view.join("original.dat")).unwrap(), b"unchanged");
    assert_eq!(
        fs::read_link(view.join("DLSS5-AMD/native-game-tiled-assets")).unwrap(),
        p.weights
    );
    let output = Command::new("cat").arg(&preload).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"synthetic preload");
    let identity = lease._image.metadata().unwrap();
    drop(lease);
    // Parallel tests may immediately reuse this descriptor number. Check the
    // original object, not whether /proc/PID/fd/N has since acquired a new owner.
    assert!(
        !fs::metadata(&preload)
            .is_ok_and(|m| { (m.dev(), m.ino()) == (identity.dev(), identity.ino()) })
    );
    fs::remove_dir_all(&view).unwrap();
    assert!(p.weights.is_dir());
    assert!(temp.path().join("logs").is_dir());
}

#[test]
fn collisions_and_gpu_selectors_fail_before_overlay_writes() {
    let temp = tempfile::tempdir().unwrap();
    let p = profile(temp.path());
    for name in [
        "D3D12.DLL",
        "DXGI.dll",
        "ReShade.ini",
        "DLSS5-AMD",
        "other.addon64",
        "version.dll",
    ] {
        let view = temp.path().join("view");
        files::private_dir(&view).unwrap();
        files::atomic(&view.join(name), b"foreign").unwrap();
        assert!(
            overlay(
                &view,
                &temp.path().join("logs"),
                &p,
                &bundle(),
                &mut BTreeMap::new()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&view).unwrap().count(), 1);
        assert_eq!(fs::read(view.join(name)).unwrap(), b"foreign");
        fs::remove_dir_all(view).unwrap();
    }
    let view = temp.path().join("view");
    files::private_dir(&view).unwrap();
    let mut env = BTreeMap::from([("DXVK_FILTER_DEVICE_UUID".into(), "different-card".into())]);
    assert!(overlay(&view, &temp.path().join("logs"), &p, &bundle(), &mut env).is_err());
    assert_eq!(fs::read_dir(view).unwrap().count(), 0);
}

#[test]
fn kernel_policy_follows_the_bound_architecture_and_discards_inherited_debug_modes() {
    for arch in ["gfx1030", "gfx1201"] {
        let temp = tempfile::tempdir().unwrap();
        let mut p = profile(temp.path());
        p.arch = arch.into();
        let view = temp.path().join("view");
        files::private_dir(&view).unwrap();
        let mut env = BTreeMap::from([
            ("DLSS5_C32_SPLIT".into(), "unexpected".into()),
            ("DLSS5_C32_PROD".into(), "1".into()),
            ("DLSS5_VIT_FFN_SPLIT".into(), "1".into()),
        ]);
        let _lease = overlay(&view, &temp.path().join("logs"), &p, &bundle(), &mut env).unwrap();
        assert_eq!(
            env.get(std::ffi::OsStr::new("DLSS5_C32_SPLIT"))
                .and_then(|v| v.to_str()),
            (arch == "gfx1030").then_some("1")
        );
        assert!(!env.contains_key(std::ffi::OsStr::new("DLSS5_C32_PROD")));
        assert!(!env.contains_key(std::ffi::OsStr::new("DLSS5_VIT_FFN_SPLIT")));
    }
}

#[test]
fn incomplete_and_traversing_weight_manifests_fail() {
    let mut manifest = json!({"schema":neural_assets::WEIGHT_SCHEMA,"source_sha256":"e16bcf15e16e13f527491cdf7845b2fe6521a738d8f7c9c721866a8496e1fc8e","upstream_commit":"15799b1600d57b849597a44be53ac892b7a2faea","layout_mode":"amd-consumer-derived","runtime_ready":true,"experimental_runtime_ready":true,"nvidia_equivalence":false,"tables":{}});
    let lock: Value =
        serde_json::from_str(include_str!("../compat/neural-weights.lock.json")).unwrap();
    for (name, count) in neural_assets::table_sizes() {
        manifest["tables"][&name] = json!({"count":count,"sha256":lock["tables"][&name]["sha256"]});
    }
    assert!(neural_assets::validate_weight_manifest(&manifest).is_ok());
    manifest["tables"]["../escape.f32"] = json!({"count":4,"sha256":"a".repeat(64)});
    assert!(neural_assets::validate_weight_manifest(&manifest).is_err());
    manifest["tables"]
        .as_object_mut()
        .unwrap()
        .remove("../escape.f32");
    manifest["tables"]["head-matrix.f32"]["count"] = json!(1);
    assert!(neural_assets::validate_weight_manifest(&manifest).is_err());
}

#[test]
fn malformed_settings_and_fake_export_names_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    files::private_dir(&temp.path().join("private")).unwrap();
    files::atomic_json(
        &temp.path().join(SETTINGS),
        &json!({"schema":999,"profile":null}),
    )
    .unwrap();
    assert!(status(temp.path()).is_err());
    for data in [
        b"MZ__wine_get_unix_env\0".as_slice(),
        b"\x7fELF__wine_get_unix_env\0",
        b"",
    ] {
        assert!(!neural_assets::exports(data, b"__wine_get_unix_env"));
    }
}

#[test]
fn pe_export_requires_a_real_function_and_valid_ordinal() {
    let mut pe = vec![0_u8; 1024];
    pe[..2].copy_from_slice(b"MZ");
    fn put(d: &mut [u8], o: usize, v: u32) {
        d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    put(&mut pe, 60, 128);
    pe[128..132].copy_from_slice(b"PE\0\0");
    pe[132..134].copy_from_slice(&0x8664_u16.to_le_bytes());
    pe[134..136].copy_from_slice(&1_u16.to_le_bytes());
    pe[148..150].copy_from_slice(&240_u16.to_le_bytes());
    pe[152..154].copy_from_slice(&0x20b_u16.to_le_bytes());
    put(&mut pe, 260, 16);
    put(&mut pe, 264, 0x1000);
    put(&mut pe, 268, 0x150);
    // One .edata section, export directory and independent code address.
    put(&mut pe, 392 + 12, 0x1000);
    put(&mut pe, 392 + 16, 512);
    put(&mut pe, 392 + 20, 512);
    put(&mut pe, 512 + 20, 1);
    put(&mut pe, 512 + 24, 1);
    put(&mut pe, 512 + 28, 0x1100);
    put(&mut pe, 512 + 32, 0x1110);
    put(&mut pe, 512 + 36, 0x1120);
    put(&mut pe, 768, 0x1180);
    put(&mut pe, 784, 0x1130);
    let name = b"__wine_get_unix_env";
    pe[816..816 + name.len()].copy_from_slice(name);
    assert!(neural_assets::exports(&pe, name));
    pe[800] = 1;
    assert!(!neural_assets::exports(&pe, name));
    pe[800] = 0;
    put(&mut pe, 768, 0x1040);
    assert!(!neural_assets::exports(&pe, name));
    put(&mut pe, 768, 0xfffffff0);
    assert!(!neural_assets::exports(&pe, name));
}

#[test]
fn disable_recovers_even_when_the_saved_profile_is_corrupt() {
    let temp = tempfile::tempdir().unwrap();
    files::private_dir(&temp.path().join("private")).unwrap();
    files::atomic(&temp.path().join(SETTINGS), b"corrupt").unwrap();
    assert!(status(temp.path()).is_err());
    let result = cli(Options {
        runtime: temp.path().into(),
        action: Action::Disable,
    })
    .unwrap();
    assert_eq!(result["mode"], "off");
}

#[test]
fn adapter_match_uses_pci_identity_across_different_driver_names() {
    let hip = json!({"name":"AMD Radeon RX 6900 XT","pci_bus_id":"0000:03:00.0"});
    let uuid = "abcdef0123456789abcdef0123456789";
    let gpu = json!({"name":"Radeon RX 6800/6800 XT / 6900 XT (radv)","vendor_id":0x1002,"type":2,"pci_bus_id":"0000:03:00.0","device_uuid":uuid});
    let vk = json!({"devices":[gpu.clone(),{"name":"Intel iGPU","vendor_id":0x8086,"type":1,"pci_bus_id":"0000:00:02.0"}]});
    assert_eq!(vulkan_uuid(&hip, &vk).unwrap(), uuid);
    for (key, value) in [
        ("pci_bus_id", json!("0000:04:00.0")),
        ("pci_bus_id", Value::Null),
        ("device_uuid", json!("0".repeat(32))),
        ("device_uuid", json!("abcd")),
        ("vendor_id", json!(0x10de)),
        ("type", json!(4)),
    ] {
        let mut bad = vk.clone();
        bad["devices"][0][key] = value;
        assert!(vulkan_uuid(&hip, &bad).is_err(), "{key}");
    }
    assert!(vulkan_uuid(&hip, &json!({"devices":[gpu.clone(),gpu.clone()]})).is_err());
    let mut duplicate_uuid = gpu.clone();
    duplicate_uuid["pci_bus_id"] = json!("0000:04:00.0");
    for duplicate in [uuid.to_string(), uuid.to_ascii_uppercase()] {
        duplicate_uuid["device_uuid"] = json!(duplicate);
        assert!(
            vulkan_uuid(
                &hip,
                &json!({"devices":[gpu.clone(),duplicate_uuid.clone()]})
            )
            .is_err()
        );
    }
    for pci in [
        "",
        "0000:03:00.8",
        "0000:03:20.0",
        "0000:3:00.0",
        "0000:03:00.x",
    ] {
        assert!(!valid_pci(pci), "{pci}");
    }
}

#[test]
#[ignore = "requires the pinned local bundle, a Wine runner and MinGW; no GPU or model needed"]
fn real_wine_bridge_calls_linux_through_sealed_preload() {
    wine_bridge(BridgeMode::Abi);
}

#[test]
#[ignore = "requires the gfx1030 bundle, HIP 7, RX 6900 XT, Wine and test_network synthetic tables"]
fn real_wine_bridge_runs_synthetic_gfx1030_frames() {
    wine_bridge(BridgeMode::Synthetic);
}

#[test]
#[ignore = "requires the gfx1030 bundle, real weight cache, native reference RGB, HIP, RX 6900 XT and Wine"]
fn real_wine_bridge_runs_model_gfx1030_frames() {
    wine_bridge(BridgeMode::Model);
}

#[test]
#[ignore = "requires source archive, DXGI, launcher, gfx1030 bundle, real weights, HIP, RX 6900 XT, Wine and MinGW C++"]
fn real_wine_d3d12_texture_runs_model_before_same_list_consumer() {
    wine_bridge(BridgeMode::Texture);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BridgeMode {
    Abi,
    Synthetic,
    Model,
    Texture,
}

fn texture_headers(temp: &Path) -> PathBuf {
    let archive = PathBuf::from(
        std::env::var_os("FLIGHTDECK_TEST_NR_SOURCE_ARCHIVE")
            .expect("FLIGHTDECK_TEST_NR_SOURCE_ARCHIVE"),
    );
    let bytes = files::read_public(&archive, 128 * 1024 * 1024).unwrap();
    let lock: Value =
        serde_json::from_str(include_str!("../compat/neural-rendering.lock.json")).unwrap();
    assert_eq!(
        files::sha256(&bytes),
        lock["rdna2"]["source_archive_sha256"]
    );
    let verified = temp.join("verified-source.tar.gz");
    files::atomic(&verified, &bytes).unwrap();
    let source = temp.join("source");
    files::private_dir(&source).unwrap();
    let extraction = Command::new("tar")
        .arg("-xzf")
        .arg(verified)
        .arg("-C")
        .arg(&source)
        .args([
            "--strip-components=1",
            "--wildcards",
            "*/src/*",
            "*/hip/include/*",
        ])
        .output()
        .unwrap();
    assert!(extraction.status.success(), "{extraction:?}");
    source.join("src")
}

fn texture_runtime(view: &Path, wine: &Path) {
    // A raw Proton prefix has Wine's DXGI/wined3d, while Flightdeck installs
    // its verified DXVK renderer. Exercise that same DLL in this isolated view.
    let path = PathBuf::from(
        std::env::var_os("FLIGHTDECK_TEST_NR_DXGI").expect("FLIGHTDECK_TEST_NR_DXGI"),
    );
    let bytes = files::read_public(&path, 32 * 1024 * 1024).unwrap();
    let lock: Value = serde_json::from_str(include_str!("../compat/graphics.lock.json")).unwrap();
    assert_eq!(files::sha256(&bytes), lock["files"]["dxgi.dll"]);
    files::atomic(&view.join("dxgi.dll"), &bytes).unwrap();
    // Proton's default prefix supplies these dependencies of its built-in
    // D3DCompiler. A bare wineboot prefix does not. Use the selected runner's
    // own files so the actual ReShade add-on also loads during the texture test.
    let libraries = wine
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("lib/vkd3d/x86_64-windows");
    for name in [
        "libvkd3d-1.dll",
        "libvkd3d-shader-1.dll",
        "libvkd3d-utils-1.dll",
    ] {
        let bytes = files::read_public(&libraries.join(name), 32 * 1024 * 1024).unwrap();
        println!("runner_dependency={name} sha256={}", files::sha256(&bytes));
        files::atomic(&view.join(name), &bytes).unwrap();
    }
}

fn wine_bridge(mode: BridgeMode) {
    let inference = mode != BridgeMode::Abi;
    let real_model = matches!(mode, BridgeMode::Model | BridgeMode::Texture);
    let source = PathBuf::from(
        std::env::var_os("FLIGHTDECK_TEST_NR_BUNDLE").expect("FLIGHTDECK_TEST_NR_BUNDLE"),
    );
    let wine = PathBuf::from(
        std::env::var_os("FLIGHTDECK_TEST_NR_WINE").expect("FLIGHTDECK_TEST_NR_WINE"),
    );
    let temp = tempfile::Builder::new()
        .prefix("neural bridge ")
        .tempdir()
        .unwrap();
    let view = temp.path().join("view with spaces");
    files::private_dir(&view).unwrap();
    let mut env: BTreeMap<_, _> = std::env::vars_os().collect();
    for k in [
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
        "LD_PRELOAD",
    ] {
        env.remove(std::ffi::OsStr::new(k));
    }
    let bundle = neural_assets::bundle(&source).unwrap();
    let mut p = profile(temp.path());
    if inference {
        p.weights = PathBuf::from(
            std::env::var_os(if real_model {
                "FLIGHTDECK_TEST_NR_WEIGHTS"
            } else {
                "FLIGHTDECK_TEST_NR_SYNTHETIC_WEIGHTS"
            })
            .expect("FLIGHTDECK_TEST_NR_(SYNTHETIC_)WEIGHTS"),
        )
        .canonicalize()
        .unwrap();
        p.hip_library = PathBuf::from(
            std::env::var_os("FLIGHTDECK_TEST_NR_HIP_LIBRARY")
                .expect("FLIGHTDECK_TEST_NR_HIP_LIBRARY"),
        )
        .canonicalize()
        .unwrap();
        p.arch = "gfx1030".into();
        p.gpu = "AMD Radeon RX 6900 XT".into();
        neural_assets::check_architecture(&bundle, &p.arch).unwrap();
        if real_model {
            neural_assets::weights(&p.weights, None).unwrap();
        }
        if mode == BridgeMode::Texture {
            let launcher = std::env::var_os("FLIGHTDECK_TEST_NR_LAUNCHER")
                .expect("FLIGHTDECK_TEST_NR_LAUNCHER");
            let probe = |kind| {
                let mut command = Command::new(&launcher);
                command
                    .args(["--native-probe", kind])
                    .env_clear()
                    .envs(&env)
                    .env("FLIGHTDECK_HIP_LIBRARY", &p.hip_library);
                if kind == "hip" {
                    let parent = p.hip_library.parent().unwrap();
                    let mut paths: Vec<_> =
                        [parent.join("rocm_sysdeps/lib"), parent.join("llvm/lib")]
                            .into_iter()
                            .filter(|p| p.is_dir())
                            .collect();
                    if let Some(old) = env.get(std::ffi::OsStr::new("LD_LIBRARY_PATH")) {
                        paths.extend(std::env::split_paths(old));
                    }
                    command.env("LD_LIBRARY_PATH", std::env::join_paths(paths).unwrap());
                }
                let output = process::output(
                    &mut command,
                    Duration::from_secs(25),
                    1024 * 1024,
                    &AtomicBool::new(false),
                )
                .unwrap();
                serde_json::from_slice::<Value>(&output).unwrap()
            };
            let hip = probe("hip");
            let hip = hip["devices"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["name"] == p.gpu && d["memory_roundtrip"] == true)
                .unwrap();
            p.vulkan_uuid = vulkan_uuid(hip, &probe("graphics")).unwrap();
            p.pci_bus_id = hip["pci_bus_id"].as_str().unwrap().into();
        }
        if mode == BridgeMode::Model {
            let reference = PathBuf::from(
                std::env::var_os("FLIGHTDECK_TEST_NR_REFERENCE")
                    .expect("FLIGHTDECK_TEST_NR_REFERENCE"),
            );
            let bytes = files::read_public(&reference, 1920 * 1080 * 3 * 4).unwrap();
            assert_eq!(bytes.len(), 1920 * 1080 * 3 * 4);
            files::atomic(&view.join("reference.rgb"), &bytes).unwrap();
        }
    }
    let _preload = overlay(&view, &temp.path().join("logs"), &p, &bundle, &mut env).unwrap();
    if mode == BridgeMode::Texture {
        texture_runtime(&view, &wine);
    }
    let mut compiler = Command::new(if mode == BridgeMode::Texture {
        "x86_64-w64-mingw32-g++"
    } else {
        "x86_64-w64-mingw32-gcc"
    });
    if mode == BridgeMode::Texture {
        compiler
            .args(["-O2", "-std=c++17", "-static", "-I"])
            .arg(texture_headers(temp.path()))
            .args([
                "tests/graphics/neural-texture.cpp",
                "-ld3d12",
                "-ldxgi",
                "-lole32",
                "-luuid",
                "-o",
            ]);
    } else {
        compiler.args([
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "tests/graphics/neural-bridge.c",
            "-o",
        ]);
    }
    let compiler = compiler.arg(view.join("probe.exe")).output().unwrap();
    assert!(
        compiler.status.success(),
        "{}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    let prefix = temp.path().join("prefix");
    let server = wine.parent().unwrap().join("wineserver");
    struct Stop {
        server: PathBuf,
        prefix: PathBuf,
    }
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = Command::new(&self.server)
                .arg("-k")
                .env("WINEPREFIX", &self.prefix)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _stop = Stop {
        server,
        prefix: prefix.clone(),
    };
    env.insert("WINEPREFIX".into(), prefix.into_os_string());
    env.insert("WINEDEBUG".into(), "-all,err+all".into());
    env.insert("WINEESYNC".into(), "0".into());
    env.insert("WINEFSYNC".into(), "0".into());
    env.insert("WINE_DISABLE_FAST_SYNC".into(), "1".into());
    if mode == BridgeMode::Texture {
        let mut overrides = env[std::ffi::OsStr::new("WINEDLLOVERRIDES")].clone();
        overrides.push(";mscoree,mshtml=");
        env.insert("WINEDLLOVERRIDES".into(), overrides);
    } else {
        env.insert(
            "WINEDLLOVERRIDES".into(),
            "mscoree,mshtml=;dlss5_hip=n".into(),
        );
    }
    let mut command = Command::new(&wine);
    command
        .arg(view.join("probe.exe"))
        .current_dir(&view)
        .env_clear()
        .envs(env);
    if inference && mode != BridgeMode::Texture {
        command
            .arg(if real_model { "--model" } else { "--synthetic" })
            .arg(&p.weights);
        if real_model {
            command.arg("reference.rgb");
        }
    }
    let log_path = temp.path().join("probe.log");
    let log = File::create(&log_path).unwrap();
    command.stdout(log.try_clone().unwrap()).stderr(log);
    let status = process::run(
        &mut command,
        Duration::from_secs(50),
        &AtomicBool::new(false),
    );
    let output = fs::read_to_string(log_path).unwrap();
    if mode == BridgeMode::Texture {
        for path in [
            view.join("ReShade.log"),
            temp.path().join("logs/native-game-oneshot.txt"),
        ] {
            if let Ok(bytes) = files::read_public(&path, 2 * 1024 * 1024) {
                let log = String::from_utf8_lossy(&bytes);
                let tail: Vec<_> = log.lines().rev().take(40).collect();
                println!(
                    "{}\n{}",
                    path.file_name().unwrap().to_string_lossy(),
                    tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                );
            }
        }
    }
    assert!(
        status.as_ref().is_ok_and(|s| s.success()),
        "{status:?}\n{output}"
    );
    if mode == BridgeMode::Texture {
        assert_eq!(
            output.matches("reference_bytes=8294400 exact=1").count(),
            3,
            "{output}"
        );
    } else {
        assert!(output.contains("PASS PE-to-Linux frame ABI"), "{output}");
    }
    if inference && mode != BridgeMode::Texture {
        assert!(
            output.contains(if real_model {
                "PASS Wine-to-gfx1030 real-model frames"
            } else {
                "PASS Wine-to-gfx1030 synthetic frames"
            }),
            "{output}"
        );
    }
    println!("{output}");
}
