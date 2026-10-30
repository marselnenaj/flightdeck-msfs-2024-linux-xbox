// SPDX-License-Identifier: MIT
//! Explicit offline, account-free Steam preparation with the actual pinned Wine.
#![allow(clippy::unwrap_used)]
use fenix_patch::system::System;
use flightdeck::{
    bootstrap, fenix_bundle, files,
    games::Game,
    installations::{Client, Installation, Source},
    process,
    steam_fenix::{self, Engine},
    transaction,
};
use serde_json::json;
use std::{
    fs::{self, File},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
fn input(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).expect("explicit live-test input"))
        .canonicalize()
        .unwrap()
}
fn put(path: &Path, bytes: impl AsRef<[u8]>) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, bytes.as_ref()).unwrap();
}
fn unpack(archive: &Path, output: &Path) -> serde_json::Value {
    let mut zipped = zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
    let roots: Vec<_> = (0..zipped.len())
        .filter_map(|i| {
            let file = zipped.by_index(i).unwrap();
            file.name()
                .unwrap()
                .strip_suffix("/bundle.json")
                .map(str::to_string)
        })
        .collect();
    assert_eq!(roots.len(), 1);
    let root = format!("{}/", roots[0]);
    for i in 0..zipped.len() {
        let mut entry = zipped.by_index(i).unwrap();
        let archive_name = entry.name().unwrap();
        let Some(name) = archive_name.strip_prefix(&root) else {
            continue;
        };
        if entry.is_dir()
            || !(name == "bundle.json"
                || name.starts_with("payload/")
                || name.starts_with("integration/"))
        {
            continue;
        }
        assert!(
            files::relative(name)
                && entry.size() <= 32 * 1024 * 1024
                && entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000)
        );
        let path = output.join(name);
        files::private_dir(path.parent().unwrap()).unwrap();
        let mut file = File::create(&path).unwrap();
        std::io::copy(&mut entry, &mut file).unwrap();
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(if entry.unix_mode().is_some_and(|m| m & 0o111 != 0) {
                0o700
            } else {
                0o600
            }),
        )
        .unwrap();
    }
    files::json(&output.join("bundle.json"), 2 * 1024 * 1024).unwrap()
}
#[test]
#[ignore = "requires explicit cached public Proton, patch archives, Microsoft prerequisite installers and geometry DLL; prepares a new synthetic Steam library"]
fn actual_steam_profile_setup_upgrade_and_restore() {
    let runner = input("FLIGHTDECK_STEAM_TEST_RUNNER");
    let previous = input("FLIGHTDECK_STEAM_TEST_PREVIOUS");
    let current = input("FLIGHTDECK_STEAM_TEST_CURRENT");
    let framework = input("FLIGHTDECK_STEAM_TEST_FRAMEWORK_CACHE");
    let desktop = input("FLIGHTDECK_STEAM_TEST_DESKTOP");
    let desktop8 = input("FLIGHTDECK_STEAM_TEST_DESKTOP8");
    let geometry = input("FLIGHTDECK_STEAM_TEST_GEOMETRY");
    let application = input("FLIGHTDECK_STEAM_TEST_FENIX_APP");
    let output = PathBuf::from(
        std::env::var_os("FLIGHTDECK_STEAM_TEST_OUTPUT").expect("explicit new output"),
    );
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    assert!(
        output.is_absolute() && output.starts_with(repo.join("build")) && !files::exists(&output)
    );
    files::private_dir(&output).unwrap();
    assert_eq!(
        transaction::digest(&current).unwrap(),
        flightdeck::resources::json("compat/fenix/release.json").unwrap()["sha256"]
    );
    assert_eq!(
        transaction::digest(&geometry).unwrap(),
        fenix_patch::core::GEOMETRY_SHA256
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let root = output.join("Steam client");
    let library = output.join("Steam library");
    let base = root.join("compatibilitytools.d/pinned-proton");
    files::private_dir(base.parent().unwrap()).unwrap();
    process::copy_tree(&runner, &base, &cancel).unwrap();
    put(
        &root.join("steamapps/libraryfolders.vdf"),
        format!(
            "\"libraryfolders\" {{\"0\" {{\"path\" {}}}}}",
            json!(library)
        ),
    );
    put(
        &library.join("steamapps/appmanifest_2537590.acf"),
        b"\"AppState\" {\"appid\" \"2537590\" \"installdir\" \"Synthetic MSFS\"}",
    );
    files::private_dir(&library.join("steamapps/common/Synthetic MSFS")).unwrap();
    let item = Installation::new(
        Game::Msfs2024,
        Source::Steam {
            client: Client::Native,
            root: root.clone(),
            library,
            install_dir: "Synthetic MSFS".into(),
        },
    )
    .unwrap();
    files::private_dir(item.prefix().parent().unwrap()).unwrap();
    bootstrap::prepare_prefix(&base, &item.prefix(), &cancel).unwrap();
    let original = transaction::digest(&item.prefix().join("system.reg")).unwrap();
    put(
        &item.prefix().join(fenix_patch::core::GEOMETRY_PATH),
        fs::read(geometry).unwrap(),
    );
    let settings = item
        .prefix()
        .join("drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024");
    put(&settings.join("UserCfg.opt"),b"InstalledPackagesPath \"C:\\users\\steamuser\\AppData\\Roaming\\Microsoft Flight Simulator 2024\\Packages\"\n");
    let aircraft = settings.join("Packages/Community/synthetic-aircraft/layout.json");
    put(&aircraft, b"synthetic content; no proprietary aircraft");
    let inode = aircraft.metadata().unwrap().ino();
    let target = item.target().unwrap();
    let downloads = target.state().join("downloads");
    files::private_dir(&downloads).unwrap();
    for (name, _, hash) in flightdeck::framework_repair::PACKAGES {
        let source = framework.join(name);
        assert_eq!(transaction::digest(&source).unwrap(), hash);
        fs::copy(source, downloads.join(name)).unwrap();
    }
    assert_eq!(
        transaction::digest(&desktop).unwrap(),
        fenix_patch::desktop::SHA256
    );
    fs::copy(desktop, downloads.join(fenix_patch::desktop::PACKAGE)).unwrap();
    let old_bundle = output.join("previous-payload");
    let old_lock = unpack(&previous, &old_bundle);
    assert_eq!(old_lock["version"], "0.1.0");
    let new_bundle = output.join("current-payload");
    let new_lock = unpack(&current, &new_bundle);
    assert_eq!(new_lock, fenix_bundle::manifest(None).unwrap());
    let old_system = Engine::new(old_bundle.clone(), old_lock, Arc::clone(&cancel));
    let new_system = Engine::new(new_bundle.clone(), new_lock, Arc::clone(&cancel));
    let progress = |message: &str| {
        println!("{message}");
        Ok(())
    };
    steam_fenix::perform(&old_system, &item, "install", &old_bundle, None, &progress).unwrap();
    let before: serde_json::Value = files::json(&target.marker(), 1024 * 1024).unwrap();
    assert_eq!(before["version"], "0.1.0");
    assert_eq!(aircraft.metadata().unwrap().ino(), inode);
    assert!(flightdeck::framework::status(&item.prefix()).ready);
    assert!(flightdeck::fenix_desktop::installed(&item.prefix()).unwrap());
    let upgraded_prefix = item.prefix().metadata().unwrap().ino();
    steam_fenix::perform(&new_system, &item, "install", &new_bundle, None, &progress).unwrap();
    let after: serde_json::Value = files::json(&target.marker(), 1024 * 1024).unwrap();
    assert_eq!(after["version"], "0.1.1");
    assert_eq!(item.prefix().metadata().unwrap().ino(), upgraded_prefix);
    assert_eq!(aircraft.metadata().unwrap().ino(), inode);
    assert!(flightdeck::framework::status(&item.prefix()).ready);
    assert!(flightdeck::fenix_desktop::installed(&item.prefix()).unwrap());
    let app_dir = item
        .prefix()
        .join("drive_c/users/steamuser/AppData/Local/FenixApp/current");
    files::private_dir(app_dir.parent().unwrap()).unwrap();
    process::copy_tree(&application, &app_dir, &cancel).unwrap();
    let app_version = flightdeck::fenix_installer::installed_version(&item.prefix()).unwrap();
    assert_eq!(app_version, "1.0.359");
    let tool = target.tools().join(after["tool"].as_str().unwrap());
    let hook_log = output.join("official-fenix-hook.log");
    let wine = flightdeck::wine::Wine::new(&item.prefix(), &tool, &hook_log, &cancel).unwrap();
    // The official 1.0.359 bootstrapper installs .NET 8 for its app. This
    // account-free test copies the public app payload instead of opening that
    // bootstrapper, so install its prerequisite explicitly from verified bytes.
    assert_eq!(
        transaction::digest(&desktop8).unwrap(),
        "c375dfd80a967405cfeff634912c1fccc56261ce3d9209ea473f60a21184d1cc"
    );
    wine.run(
        &[
            desktop8.as_os_str(),
            "/install".as_ref(),
            "/quiet".as_ref(),
            "/norestart".as_ref(),
        ],
        None,
        &[0, 194],
        Duration::from_secs(900),
    )
    .unwrap();
    wine.stop_staged().unwrap();
    let offset = wine.log.metadata().unwrap().len();
    let hook = flightdeck::fenix_installer::repair(&wine);
    let evidence = flightdeck::fenix_installer::scan_log(&hook_log, &wine.log, offset);
    wine.stop_staged().unwrap();
    assert!(
        hook.unwrap().success(),
        "official Fenix 1.0.359 install hook failed in the upgraded Steam profile"
    );
    evidence.unwrap().check().unwrap();
    let restored =
        steam_fenix::perform(&new_system, &item, "restore", &new_bundle, None, &progress);
    restored.unwrap();
    assert_eq!(
        transaction::digest(&item.prefix().join("system.reg")).unwrap(),
        original
    );
    assert_eq!(aircraft.metadata().unwrap().ino(), inode);
    assert!(!target.marker().exists());
    files::atomic_json(&output.join("result.json"),&json!({"passed":true,"previous":"0.1.0","current":"0.1.1","runner":"cachyos-11","shared_engine":true,"actual_proton_setup":true,"actual_dotnet_framework":true,"actual_desktop_runtime":true,"official_fenix_app":app_version,"official_install_hook_exit":0,"preserved_packages_inode":true,"preserved_prefix_on_upgrade":true,"restored_original_registry":true,"account_calls":false,"real_steam_client_modified":false})).unwrap();
    // Trait assertion keeps this test on the production System adapter.
    let _: &dyn System = &new_system;
}
