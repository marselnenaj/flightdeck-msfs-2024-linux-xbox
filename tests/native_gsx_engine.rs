// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{
    backend::Launcher,
    fenix, files,
    games::Game,
    gsx,
    gsx_engine::{self, Package},
    transaction as tx,
};
use serde_json::json;
use std::{
    fs,
    io::{Cursor, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn write(path: &Path, bytes: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, bytes).unwrap();
}
fn archive(
    base: &Path,
    label: &str,
    target: &str,
    entries: &[(&str, &[u8])],
    link: bool,
) -> (Package, PathBuf) {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o600);
    let mut total = 0;
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
        total += bytes.len() as u64;
    }
    if link {
        zip.add_symlink("link", "outside", options).unwrap();
        total += 7;
    }
    let bytes = zip.finish().unwrap().into_inner();
    let path = base.join(label);
    write(&path, &bytes);
    (
        Package {
            name: label.into(),
            target: target.into(),
            bytes: bytes.len() as u64,
            sha256: files::sha256(&bytes),
            entries: entries.len() + usize::from(link),
            unpacked_bytes: total,
        },
        path,
    )
}
fn install(
    base: &Path,
    package: Package,
    source: PathBuf,
    cancel: &AtomicBool,
) -> flightdeck::Result<()> {
    gsx_engine::install_with(
        &base.join("manager"),
        &base.join("work"),
        &[package],
        cancel,
        |_| Ok(source.clone()),
    )
}
#[test]
fn conflicting_zip_headers_never_publish_an_engine_file() {
    let control = tempfile::tempdir().unwrap();
    files::private_dir(&control.path().join("manager")).unwrap();
    let (package, source) = archive(
        control.path(),
        "headers.zip",
        "engine",
        &[("safe.dll", b"engine")],
        false,
    );
    install(control.path(), package, source, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        fs::read(control.path().join("manager/engine/safe.dll")).unwrap(),
        b"engine"
    );
    for changed in ["name", "compression", "flags"] {
        let temp = tempfile::tempdir().unwrap();
        files::private_dir(&temp.path().join("manager")).unwrap();
        let (mut package, source) = archive(
            temp.path(),
            "headers.zip",
            "engine",
            &[("safe.dll", b"engine")],
            false,
        );
        let mut bytes = fs::read(&source).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        match changed {
            "name" => bytes[30..38].copy_from_slice(b"../x.dll"),
            "compression" => bytes[8..10].copy_from_slice(&8_u16.to_le_bytes()),
            "flags" => bytes[6] ^= 8,
            _ => unreachable!(),
        }
        // Exercise the parser behind the digest gate with a self-consistent
        // test pin, not merely rejection of an incorrect download checksum.
        package.sha256 = files::sha256(&bytes);
        write(&source, &bytes);
        assert!(
            install(temp.path(), package, source, &AtomicBool::new(false)).is_err(),
            "{changed}"
        );
        assert!(!temp.path().join("manager/engine/safe.dll").exists());
        assert!(!temp.path().join("manager/x.dll").exists());
        assert!(!temp.path().join("x.dll").exists());
    }
}
#[test]
fn official_lock_has_exact_https_release_identities_and_bounded_sizes() {
    let packages = gsx_engine::packages().unwrap();
    assert_eq!(packages.len(), 7);
    assert_eq!(packages.iter().map(|p| p.bytes).sum::<u64>(), 444_579_543);
    assert_eq!(
        packages.iter().map(|p| p.unpacked_bytes).sum::<u64>(),
        592_020_166
    );
    assert!(packages.iter().all(|p| p.url().starts_with(
        "https://github.com/virtualisoftware/fsdt-offline-installer/releases/download/2.4.8.0/"
    ) && files::hex_digest(&p.sha256)));
    assert_eq!(
        packages
            .iter()
            .find(|p| p.name == "GSX.zip.001")
            .unwrap()
            .target,
        "couatl/GSX"
    );
    assert_eq!(
        packages
            .iter()
            .find(|p| p.name == "couatl64_wx.zip.001")
            .unwrap()
            .target,
        "couatl64/wx"
    );
}
/// Optional static compatibility check against already-downloaded official
/// archives. This never downloads, runs Wine or touches a simulator profile.
#[test]
#[ignore = "requires GSX_ENGINE_ARCHIVES pointing at the seven pinned ZIPs"]
fn actual_official_archives_decode_into_the_observed_engine_layout() {
    let archives = PathBuf::from(std::env::var_os("GSX_ENGINE_ARCHIVES").unwrap());
    let scratch = tempfile::tempdir_in(&archives).unwrap();
    let manager = scratch.path().join("manager");
    files::private_dir(&manager).unwrap();
    let packages = gsx_engine::packages().unwrap();
    gsx_engine::install_with(
        &manager,
        &scratch.path().join("decoded"),
        &packages,
        &AtomicBool::new(false),
        |package| Ok(archives.join(&package.name)),
    )
    .unwrap();
    assert!(gsx_engine::status(Some(&manager), Game::Msfs2024).ready);
    assert!(gsx_engine::status(Some(&manager), Game::Msfs2020).ready);
    assert!(!manager.join("MSFS/fsdreamteam-gsx-pro").exists());
}
#[test]
fn complete_decode_precedes_overlay_and_preserves_unrelated_userdata() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    write(&base.join("manager/couatl64/engine.exe"), b"old");
    write(&base.join("manager/activation.lic"), b"user activation");
    let (package, source) = archive(
        base,
        "engine.zip",
        "couatl64",
        &[("engine.exe", b"new engine"), ("wx/__init__.py", b"module")],
        false,
    );
    install(base, package, source, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        fs::read(base.join("manager/couatl64/engine.exe")).unwrap(),
        b"new engine"
    );
    assert_eq!(
        fs::read(base.join("manager/couatl64/wx/__init__.py")).unwrap(),
        b"module"
    );
    assert_eq!(
        fs::read(base.join("manager/activation.lic")).unwrap(),
        b"user activation"
    );
}
#[test]
fn traversal_case_collisions_file_parents_and_symlinks_are_rejected_before_output() {
    type Scenario<'a> = (Vec<(&'a str, &'a [u8])>, bool);
    let scenarios: Vec<Scenario<'_>> = vec![
        (vec![("../escape", b"bad")], false),
        (vec![("/absolute", b"bad")], false),
        (vec![("C:/escape", b"bad")], false),
        (vec![("engine:stream", b"bad")], false),
        (vec![("bad\\name", b"bad")], false),
        (vec![("engine.", b"bad")], false),
        (vec![("NUL.txt", b"bad")], false),
        (vec![("Engine.exe", b"one"), ("engine.exe", b"two")], false),
        (vec![("folder", b"one"), ("folder/file", b"two")], false),
        (vec![("engine.exe", b"new")], true),
    ];
    for (entries, link) in scenarios {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        write(&base.join("manager/couatl64/engine.exe"), b"old");
        let (package, source) = archive(base, "engine.zip", "couatl64", &entries, link);
        assert!(install(base, package, source, &AtomicBool::new(false)).is_err());
        assert!(!base.join("work").exists());
        assert_eq!(
            fs::read(base.join("manager/couatl64/engine.exe")).unwrap(),
            b"old"
        );
    }
}
#[test]
fn foreign_destination_links_and_wrong_hash_or_length_never_change_manager() {
    for failure in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        write(&base.join("outside/engine.exe"), b"outside");
        files::private_dir(&base.join("manager")).unwrap();
        if failure == 0 {
            symlink(base.join("outside"), base.join("manager/couatl64")).unwrap();
        }
        let (mut package, source) = archive(
            base,
            "engine.zip",
            "couatl64",
            &[("engine.exe", b"new")],
            false,
        );
        if failure == 1 {
            package.sha256 = "0".repeat(64);
        }
        if failure == 2 {
            package.bytes += 1;
        }
        assert!(install(base, package, source, &AtomicBool::new(false)).is_err());
        assert!(!base.join("work").exists());
        assert_eq!(
            fs::read(base.join("outside/engine.exe")).unwrap(),
            b"outside"
        );
    }
}
#[test]
fn corrupted_crc_in_later_file_does_not_overlay_an_earlier_valid_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    write(&base.join("manager/couatl64/first.exe"), b"old");
    let (mut package, source) = archive(
        base,
        "engine.zip",
        "couatl64",
        &[
            ("first.exe", b"first new"),
            ("second.exe", b"CORRUPT THIS DATA"),
        ],
        false,
    );
    let mut bytes = fs::read(&source).unwrap();
    let offset = bytes
        .windows(17)
        .position(|v| v == b"CORRUPT THIS DATA")
        .unwrap();
    bytes[offset] ^= 1;
    write(&source, &bytes);
    package.sha256 = files::sha256(&bytes);
    assert!(install(base, package, source, &AtomicBool::new(false)).is_err());
    assert_eq!(
        fs::read(base.join("manager/couatl64/first.exe")).unwrap(),
        b"old"
    );
    assert!(!base.join("manager/couatl64/second.exe").exists());
}
#[test]
fn cancellation_after_fetch_leaves_no_extracted_output() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    files::private_dir(&base.join("manager")).unwrap();
    let (package, source) = archive(
        base,
        "engine.zip",
        "couatl64",
        &[("engine.exe", b"new")],
        false,
    );
    let cancel = AtomicBool::new(false);
    assert!(
        gsx_engine::install_with(
            &base.join("manager"),
            &base.join("work"),
            &[package],
            &cancel,
            |_| {
                cancel.store(true, Ordering::Relaxed);
                Ok(source.clone())
            }
        )
        .is_err()
    );
    assert!(!base.join("work").exists());
    assert!(!base.join("manager/couatl64").exists());
}
#[test]
fn special_or_oversized_zip_metadata_is_rejected_before_output() {
    for special in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        files::private_dir(&base.join("manager")).unwrap();
        let (mut package, source) = archive(
            base,
            "engine.zip",
            "couatl64",
            &[("engine.exe", b"new")],
            false,
        );
        let mut bytes = fs::read(&source).unwrap();
        let central = bytes.windows(4).position(|v| v == b"PK\x01\x02").unwrap();
        if special {
            bytes[central + 38..central + 42].copy_from_slice(&(0o010600u32 << 16).to_le_bytes());
        } else {
            let oversized = 128 * 1024 * 1024 + 1;
            bytes[central + 24..central + 28].copy_from_slice(&(oversized as u32).to_le_bytes());
            package.unpacked_bytes = oversized;
        }
        package.sha256 = files::sha256(&bytes);
        write(&source, &bytes);
        assert!(install(base, package, source, &AtomicBool::new(false)).is_err());
        assert!(!base.join("work").exists());
    }
}
#[test]
fn pinned_private_cache_is_reused_without_network_and_links_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    let (package, source) = archive(
        base,
        "engine.zip",
        "couatl64",
        &[("engine.exe", b"new")],
        false,
    );
    assert_eq!(
        gsx_engine::download(&package, &source, &AtomicBool::new(false)).unwrap(),
        source
    );
    symlink(&source, base.join("linked-cache")).unwrap();
    assert!(
        gsx_engine::download(
            &package,
            &base.join("linked-cache"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
}
const ENGINE: &[&str] = &[
    "couatl64/couatl64_boot.exe",
    "couatl64/python37.dll",
    "couatl64/python37.zip",
    "couatl64/wx/__init__.py",
    "couatl64/wx/_core.cp37-win_amd64.pyd",
    "couatl/common/__init__.pye",
    "couatl/GSX/__init__.pye",
    "couatl/GSX/airports/__init__.pye",
    "couatl/GSX/sounds/Truck_Engine1.wav",
    "couatl64/couatl64_MSFS2024.exe",
    "couatl64/couatl64_MSFS.exe",
];
#[test]
fn engine_status_requires_the_selected_simulator_and_real_modules() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    for path in ENGINE {
        write(&base.join(path), b"payload");
    }
    assert!(gsx_engine::status(Some(base), Game::Msfs2024).ready);
    fs::remove_file(base.join("couatl64/couatl64_MSFS2024.exe")).unwrap();
    assert!(!gsx_engine::status(Some(base), Game::Msfs2024).ready);
    assert!(gsx_engine::status(Some(base), Game::Msfs2020).ready);
    fs::remove_file(base.join("couatl/GSX/__init__.pye")).unwrap();
    symlink(
        base.join("couatl/common/__init__.pye"),
        base.join("couatl/GSX/__init__.pye"),
    )
    .unwrap();
    assert!(!gsx_engine::status(Some(base), Game::Msfs2020).ready);
}
fn product(root: &Path) {
    write(
        &root.join("manifest.json"),
        br#"{"title":"GSX Pro","package_version":"1.0"}"#,
    );
    let paths = [
        "modules/fsdt-msfs-bridge.wasm",
        "SimObjects/GroundVehicles/gsx/model/model.gltf",
        "SimObjects/GroundVehicles/gsx/texture/common.dds",
    ];
    for path in paths {
        write(&root.join(path), b"payload");
    }
    files::atomic_json(
        &root.join("layout.json"),
        &json!({"content":paths.map(|path| json!({"path":path,"size":7}))}),
    )
    .unwrap();
}
#[test]
fn manifest_stubs_and_partial_models_are_not_a_full_product() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    write(
        &base.join("manifest.json"),
        br#"{"title":"GSX Pro","package_version":"1.0"}"#,
    );
    assert!(!gsx_engine::product_present(base));
    product(base);
    assert!(gsx_engine::product_present(base));
    fs::remove_file(base.join("SimObjects/GroundVehicles/gsx/texture/common.dds")).unwrap();
    assert!(!gsx_engine::product_present(base));
    product(base);
    write(&base.join("modules/fsdt-msfs-bridge.wasm"), b"truncated");
    assert!(!gsx_engine::product_present(base));
}

fn fixture(base: &Path) -> (PathBuf, Arc<Launcher>) {
    let root = base.join("runtime");
    let prefix = root.join("local/msfs-prefix");
    let runner = base.join("runner");
    for name in ["wine", "wineserver"] {
        write(
            &runner.join("files/bin").join(name),
            b"#!/bin/sh\nexit 99\n",
        );
        fs::set_permissions(
            runner.join("files/bin").join(name),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    write(&root.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    files::private_dir(&root.join("private")).unwrap();
    files::private_dir(&prefix.join("drive_c/windows/system32")).unwrap();
    symlink(runner, root.join("runner")).unwrap();
    let reg = "[Software\\\\Microsoft\\\\NET Framework Setup\\\\NDP\\\\v4\\\\Full] 123\n\"Release\"=dword:00080eb1\n[Software\\\\Wow6432Node\\\\Microsoft\\\\NET Framework Setup\\\\NDP\\\\v4\\\\Full] 123\n\"Release\"=dword:00080eb1\n";
    write(&prefix.join("system.reg"), reg.as_bytes());
    for architecture in ["Framework", "Framework64"] {
        write(
            &prefix.join(format!(
                "drive_c/windows/Microsoft.NET/{architecture}/v4.0.30319/clr.dll"
            )),
            b"MZfixture",
        );
    }
    let manager = prefix.join("drive_c/Program Files (x86)/Addon Manager");
    write(&manager.join("Couatl_Updater.exe"), b"MZfixture");
    write(
        &manager.join("QlmLicenseLib.dll"),
        b"MZ official license library",
    );
    write(&manager.join("activation.lic"), b"retained user activation");
    let app = Launcher::new(base.join("state"), Some(root.to_str().unwrap())).unwrap();
    (root, app)
}
fn engine_archives(base: &Path) -> (Vec<Package>, Vec<PathBuf>) {
    let mut packages = Vec::new();
    let mut sources = Vec::new();
    for target in ["couatl64", "couatl"] {
        let entries: Vec<_> = ENGINE
            .iter()
            .filter_map(|path| {
                path.strip_prefix(&format!("{target}/"))
                    .map(|path| (path, b"MZfixture".as_slice()))
            })
            .collect();
        let (package, source) = archive(base, &format!("{target}.zip"), target, &entries, false);
        packages.push(package);
        sources.push(source);
    }
    (packages, sources)
}
#[test]
fn staged_engine_repair_retains_license_prior_setup_and_xml_without_claiming_product() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let prefix = root.join("local/msfs-prefix");
    let original = fenix::identity(&prefix).unwrap();
    let xml =
        prefix.join("drive_c/users/pilot/AppData/Roaming/Microsoft Flight Simulator 2024/exe.xml");
    write(&xml, b"<SimBase.Document><Launch.Addon><Name>Other app</Name><Path>Other.exe</Path></Launch.Addon></SimBase.Document>");
    let prior = json!({"format":1,"id":"11111111111111111111111111111111","state":"ready","backup":"retained previous reference"});
    files::atomic_json(&root.join(gsx::MARKER), &prior).unwrap();
    let (packages, sources) = engine_archives(temp.path());
    let ctx = app.reserve("gsx", "repair-engine", true).unwrap();
    gsx::repair_engine_with(&ctx, &packages, |p| {
        Ok(sources[packages.iter().position(|v| v.name == p.name).unwrap()].clone())
    })
    .unwrap();
    assert_ne!(fenix::identity(&prefix).unwrap(), original);
    assert_eq!(
        fs::read(prefix.join("drive_c/Program Files (x86)/Addon Manager/activation.lic")).unwrap(),
        b"retained user activation"
    );
    assert_eq!(fs::read(&xml).unwrap(), b"<SimBase.Document><Launch.Addon><Name>Other app</Name><Path>Other.exe</Path></Launch.Addon></SimBase.Document>");
    let marker = gsx::marker(&root).unwrap().unwrap();
    assert_eq!(marker["state"], "ready");
    assert_eq!(marker["prior"], prior);
    assert_eq!(marker["operation"], "repair-engine");
    let snapshot = gsx::snapshot(&app);
    assert_eq!(snapshot["engine_ready"], true, "{snapshot}");
    assert_eq!(snapshot["package_installed"], false);
    assert_eq!(snapshot["configured"], false);
    assert!(!root.join(gsx::STARTUP).exists());
    assert!(!fs::read_dir(&prefix).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gsx-engine-")
    }));
}
#[test]
fn cancelled_engine_repair_keeps_active_identity_and_recovers_prior_journal() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let prefix = root.join("local/msfs-prefix");
    let original = fenix::identity(&prefix).unwrap();
    let prior = json!({"format":1,"id":"11111111111111111111111111111111","state":"ready"});
    files::atomic_json(&root.join(gsx::MARKER), &prior).unwrap();
    let (packages, sources) = engine_archives(temp.path());
    let ctx = app.reserve("gsx", "repair-engine", true).unwrap();
    assert!(
        gsx::repair_engine_with(&ctx, &packages, |_| {
            ctx.cancel.store(true, Ordering::Relaxed);
            Ok(sources[0].clone())
        })
        .is_err()
    );
    assert_eq!(fenix::identity(&prefix).unwrap(), original);
    assert_eq!(gsx::marker(&root).unwrap().unwrap()["state"], "preparing");
    ctx.cancel.store(false, Ordering::Relaxed);
    gsx::recover(&ctx).unwrap();
    assert_eq!(gsx::marker(&root).unwrap().unwrap(), prior);
    assert_eq!(fenix::identity(&prefix).unwrap(), original);
}
#[test]
fn changed_runner_blocks_engine_commit_and_keeps_active_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let (root, app) = fixture(temp.path());
    let prefix = root.join("local/msfs-prefix");
    let original = fenix::identity(&prefix).unwrap();
    let (packages, sources) = engine_archives(temp.path());
    let ctx = app.reserve("gsx", "repair-engine", true).unwrap();
    let mut changed = false;
    assert!(
        gsx::repair_engine_with(&ctx, &packages, |p| {
            if !changed {
                changed = true;
                fs::remove_file(root.join("runner")).unwrap();
                symlink(temp.path().join("replacement-runner"), root.join("runner")).unwrap();
            }
            Ok(sources[packages.iter().position(|v| v.name == p.name).unwrap()].clone())
        })
        .is_err()
    );
    assert_eq!(fenix::identity(&prefix).unwrap(), original);
    assert_eq!(gsx::marker(&root).unwrap().unwrap()["state"], "preparing");
}
#[test]
fn engine_recovery_covers_every_publish_boundary_without_running_wine() {
    for phase in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let (root, app) = fixture(temp.path());
        let prefix = root.join("local/msfs-prefix");
        let original = fenix::identity(&prefix).unwrap();
        let token = "0123456789abcdef0123456789abcdef";
        let staged = root.join(format!("local/.gsx-prefix-{token}"));
        let backup = root.join(format!("local/msfs-prefix.before-gsx-{token}"));
        write(&staged.join("system.reg"), b"staged engine profile");
        files::private_dir(&staged.join("drive_c")).unwrap();
        let staged_id = fenix::identity(&staged).unwrap();
        let prior = json!({"format":1,"id":"11111111111111111111111111111111","state":"ready"});
        files::atomic_json(&root.join(gsx::MARKER), &json!({"format":1,"id":token,"state":if phase == 0 {"preparing"} else {"committing"},"operation":"repair-engine","original_prefix_id":original,"staged_prefix_id":staged_id,"prior":prior})).unwrap();
        if phase > 0 {
            tx::publish(&prefix, &backup).unwrap();
        }
        if phase == 2 {
            tx::publish(&staged, &prefix).unwrap();
        }
        let ctx = app.reserve("gsx", "recover", true).unwrap();
        gsx::recover(&ctx).unwrap();
        assert_eq!(fenix::identity(&prefix).unwrap(), original);
        assert_eq!(gsx::marker(&root).unwrap().unwrap(), prior);
    }
}
