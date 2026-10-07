// SPDX-License-Identifier: MIT
//! Hardware-free graphics contracts retained after retiring the Python suite.
#![allow(clippy::unwrap_used)]
use flightdeck::{files, graphics, vr};
use serde_json::json;
use std::{collections::BTreeMap, fs};

#[test]
fn nvidia_with_igpu_and_software_renderer_uses_uuid_despite_vendor_hiding() {
    let devices = vec![
        json!({"name":"NVIDIA GeForce RTX 4080","vendor_id":0x10de,"type":2,"device_uuid":"0123456789abcdef0123456789abcdef"}),
        json!({"name":"AMD integrated","vendor_id":0x1002,"type":1,"device_uuid":"11111111111111111111111111111111"}),
        json!({"name":"llvmpipe","vendor_id":0,"type":4,"device_uuid":"22222222222222222222222222222222"}),
    ];
    for mode in ["auto", "compatibility"] {
        let mut env = BTreeMap::from([
            ("WINEDLLOVERRIDES".into(), "xgameruntime=n".into()),
            (
                "VKD3D_DISABLE_EXTENSIONS".into(),
                "VK_KHR_maintenance9".into(),
            ),
        ]);
        graphics::apply_mode(&mut env, mode);
        assert_eq!(graphics::select_adapter(&mut env, &devices), "nvidia_uuid");
        assert_eq!(env["DXVK_FILTER_DEVICE_UUID"], devices[0]["device_uuid"]);
        assert!(!env.contains_key("DXVK_FILTER_DEVICE_NAME"));
        assert!(!env.contains_key("VKD3D_FILTER_DEVICE_NAME"));
        assert_eq!(env["WINE_HIDE_NVIDIA_GPU"], "1");
        assert_eq!(env["DXVK_ENABLE_NVAPI"], "0");
        assert!(env["WINEDLLOVERRIDES"].starts_with("xgameruntime=n;"));
        assert_eq!(
            env["VKD3D_DISABLE_EXTENSIONS"],
            "VK_KHR_maintenance9;VK_NV_low_latency2"
        );
    }
}

#[test]
fn ambiguous_adapters_and_explicit_gpu_choices_are_preserved() {
    let gpu = json!({"name":"NVIDIA RTX","vendor_id":0x10de,"type":2,"device_uuid":"0123456789abcdef0123456789abcdef"});
    for devices in [
        vec![gpu.clone(), gpu.clone()],
        vec![json!({"type":2,"vendor_id":0x10de,"device_uuid":"00000000000000000000000000000000"})],
    ] {
        let mut env = BTreeMap::new();
        assert_eq!(graphics::select_adapter(&mut env, &devices), "default");
        assert!(env.is_empty());
    }
    let mut env = BTreeMap::from([
        ("VKD3D_VULKAN_DEVICE".into(), "1".into()),
        ("DXVK_FILTER_DEVICE_UUID".into(), "user-choice".into()),
    ]);
    let original = env.clone();
    graphics::apply_mode(&mut env, "features");
    assert_eq!(graphics::select_adapter(&mut env, &[gpu]), "explicit");
    assert_eq!(env, original);
}

#[test]
fn renderer_round_trip_preserves_custom_files_and_rejects_corrupt_bundles() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    let bundle = temp.path().join("bundle");
    let system = root.join("local/msfs-prefix/drive_c/windows/system32");
    files::private_dir(&system).unwrap();
    files::private_dir(&root.join("private")).unwrap();
    files::private_dir(&bundle).unwrap();
    let mut base = json!({});
    let mut patched = json!({});
    for name in [
        "d3d12.dll",
        "d3d12core.dll",
        "dxgi.dll",
        "d3d11.dll",
        "d3d10core.dll",
    ] {
        let renderer = if name.starts_with("d3d12") {
            "vkd3d-proton"
        } else {
            "dxvk"
        };
        let original = root.join(format!(
            "runner/files/lib/wine/{renderer}/x86_64-windows/{name}"
        ));
        files::private_dir(original.parent().unwrap()).unwrap();
        fs::write(&original, name).unwrap();
        fs::write(system.join(name), name).unwrap();
        let data = format!("patched {name}");
        fs::write(bundle.join(name), &data).unwrap();
        base[name] = json!(files::sha256(name.as_bytes()));
        patched[name] = json!(files::sha256(data.as_bytes()));
    }
    files::atomic_json(
        &bundle.join("manifest.json"),
        &json!({"schema":2,"base":base,"files":patched}),
    )
    .unwrap();
    assert_eq!(
        graphics::renderer_from(&root, Some(&bundle)).unwrap(),
        "backport"
    );
    assert_eq!(
        fs::read(system.join("dxgi.dll")).unwrap(),
        b"patched dxgi.dll"
    );
    assert_eq!(graphics::renderer_from(&root, None).unwrap(), "runner");
    assert_eq!(fs::read(system.join("dxgi.dll")).unwrap(), b"dxgi.dll");
    assert!(!root.join("private/renderer-runtime.json").exists());
    fs::write(system.join("d3d11.dll"), b"user supplied library").unwrap();
    assert_eq!(
        graphics::renderer_from(&root, Some(&bundle)).unwrap(),
        "custom"
    );
    assert_eq!(fs::read(system.join("dxgi.dll")).unwrap(), b"dxgi.dll");
    assert_eq!(
        fs::read(system.join("d3d11.dll")).unwrap(),
        b"user supplied library"
    );
    fs::write(bundle.join("dxgi.dll"), b"corrupt download").unwrap();
    assert!(graphics::renderer_from(&root, Some(&bundle)).is_err());
    assert_eq!(fs::read(system.join("dxgi.dll")).unwrap(), b"dxgi.dll");
}

#[test]
fn disabled_vr_leaves_environment_alone_and_invalid_explicit_runtime_never_falls_back() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = temp.path().join("active.json");
    files::atomic_json(
        &manifest,
        &json!({"runtime":{"library_path":"C:\\windows\\openxr.dll"}}),
    )
    .unwrap();
    let env = BTreeMap::from([(
        "XR_RUNTIME_JSON".into(),
        manifest.to_string_lossy().into_owned(),
    )]);
    assert_eq!(
        vr::prepare(
            temp.path(),
            env.clone(),
            &std::sync::atomic::AtomicBool::new(false)
        )
        .unwrap(),
        env
    );
    let selected = vr::choose("auto", &env).unwrap();
    assert_eq!(selected["path"], manifest.to_str().unwrap());
    assert_eq!(selected["active"], true);
    assert_eq!(selected["valid"], false);
    fs::remove_file(manifest).unwrap();
    assert_eq!(vr::choose("auto", &env).unwrap()["valid"], false);
}
