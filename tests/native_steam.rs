// SPDX-License-Identifier: MIT
//! Installation identity, storefront boundaries and observed Steam sessions.
#![allow(clippy::unwrap_used)]
use flightdeck::{
    api,
    backend::Launcher,
    files,
    games::Game,
    installations::{self, Client, Installation, Registry, Source, SteamRoot},
    steam_session,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
fn put(path: &Path, bytes: impl AsRef<[u8]>) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, bytes.as_ref()).unwrap();
}
struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    library: PathBuf,
    app: Arc<Launcher>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let root = home.join(".local/share/Steam");
        let library = temp.path().join("second library");
        files::private_dir(&root.join("steamapps")).unwrap();
        files::private_dir(&library.join("steamapps")).unwrap();
        put(
            &root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{\"0\" {{\"path\" {}}} \"1\" {{\"path\" {}}} }}",
                json!(root),
                json!(library)
            ),
        );
        let app = Launcher::new(temp.path().join("state"), None).unwrap();
        app.lock().installations.steam_roots.push(SteamRoot {
            client: Client::Native,
            path: root.clone(),
        });
        Self {
            _temp: temp,
            home,
            root,
            library,
            app,
        }
    }
    fn game(&self, game: Game, initialized: bool) -> Installation {
        let id = match game {
            Game::Msfs2020 => "1250410",
            Game::Msfs2024 => "2537590",
        };
        let dir = format!("MSFS {}", game.id());
        put(
            &self
                .library
                .join("steamapps")
                .join(format!("appmanifest_{id}.acf")),
            format!(
                "\"AppState\" {{\"appid\" \"{id}\" \"installdir\" {} \"StateFlags\" \"4\"}}",
                json!(dir)
            ),
        );
        files::private_dir(&self.library.join("steamapps/common").join(&dir)).unwrap();
        let item = Installation::new(
            game,
            Source::Steam {
                client: Client::Native,
                root: self.root.clone(),
                library: self.library.clone(),
                install_dir: dir,
            },
        )
        .unwrap();
        if initialized {
            put(&item.prefix().join("system.reg"), b"synthetic registry");
        }
        item
    }
    fn select(&self, item: &Installation) {
        api::post(
            &self.app,
            "/api/installations/select",
            &json!({"installation_id":item.id,"binding":item.context()}),
            "en",
        )
        .unwrap()
        .unwrap();
    }
}
#[test]
fn discovers_both_editions_in_secondary_library_before_the_first_proton_start() {
    let _guard = serial();
    let f = Fixture::new();
    let a = f.game(Game::Msfs2024, false);
    let b = f.game(Game::Msfs2020, true);
    let before = fs::read(f.root.join("steamapps/libraryfolders.vdf")).unwrap();
    let items = installations::discover(&f.home, &Registry::default(), &BTreeMap::new(), None);
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|i| i.id == a.id));
    assert!(items.iter().any(|i| i.id == b.id));
    assert_eq!(a.public(false)["needs_first_launch"], true);
    assert_eq!(b.public(false)["needs_first_launch"], false);
    assert_eq!(
        fs::read(f.root.join("steamapps/libraryfolders.vdf")).unwrap(),
        before
    );
    assert!(!a.prefix().exists());
    assert!(!f.root.join("config").exists());
}
#[test]
fn canonical_aliases_are_deduplicated_but_flatpak_is_a_separate_client() {
    let _guard = serial();
    let f = Fixture::new();
    f.game(Game::Msfs2024, true);
    files::private_dir(&f.home.join(".steam")).unwrap();
    symlink(&f.root, f.home.join(".steam/root")).unwrap();
    let flat = f
        .home
        .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
    put(
        &flat.join("steamapps/appmanifest_2537590.acf"),
        b"\"AppState\" {\"appid\" \"2537590\" \"installdir\" \"MSFS\"}",
    );
    files::private_dir(&flat.join("steamapps/common/MSFS")).unwrap();
    let items = installations::discover(&f.home, &Registry::default(), &BTreeMap::new(), None);
    assert_eq!(items.len(), 2);
    assert_ne!(items[0].id, items[1].id);
    assert!(items.iter().any(|i| matches!(
        i.source,
        Source::Steam {
            client: Client::Flatpak,
            ..
        }
    )));
}
#[test]
fn duplicate_appids_in_one_client_cannot_prepare_or_launch_the_wrong_library() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    assert!(item.validate().is_ok());
    put(
        &f.root.join("steamapps/appmanifest_2537590.acf"),
        b"\"AppState\" {\"appid\" \"2537590\" \"installdir\" \"Old MSFS\"}",
    );
    files::private_dir(&f.root.join("steamapps/common/Old MSFS")).unwrap();
    assert!(item.validate().is_err());
    assert!(
        api::post(
            &f.app,
            "/api/installations/select",
            &json!({"installation_id":item.id,"binding":item.context()}),
            "en",
        )
        .unwrap()
        .is_err()
    );
    assert!(!f.app.state_dir.join("installations.json").exists());
    fs::remove_file(f.root.join("steamapps/appmanifest_2537590.acf")).unwrap();
    assert!(item.validate().is_ok());
}
#[test]
fn a_manual_sandbox_client_cannot_be_bound_to_an_unrelated_native_root() {
    let _guard = serial();
    let f = Fixture::new();
    put(
        &f.root.join("config/config.vdf"),
        b"\"InstallConfigStore\" {}",
    );
    for client in ["flatpak", "snap"] {
        assert!(
            api::post(
                &f.app,
                "/api/installations/add-steam-root",
                &json!({"steam_root":f.root,"client":client}),
                "en",
            )
            .unwrap()
            .is_err()
        );
    }
    assert!(!f.app.state_dir.join("installations.json").exists());
}
#[test]
fn malformed_manifests_cannot_redirect_a_simulator_or_escape_a_library() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, false);
    for text in [
        "\"AppState\" {\"appid\" \"1250410\" \"installdir\" \"MSFS\"}",
        "\"AppState\" {\"appid\" \"2537590\" \"installdir\" \"../outside\"}",
        "\"AppState\" {\"appid\" \"2537590\" \"appid\" \"2537590\" \"installdir\" \"MSFS\"}",
    ] {
        put(&f.library.join("steamapps/appmanifest_2537590.acf"), text);
        assert!(item.validate().is_err());
        assert!(
            installations::discover(&f.home, &Registry::default(), &BTreeMap::new(), None)
                .is_empty()
        );
    }
}
#[test]
fn steam_selection_persists_without_replacing_or_mutating_the_xbox_runtime() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    let xbox = f._temp.path().join("xbox");
    put(&xbox.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    put(
        &xbox.join("private/runtime.json"),
        b"{\"game_id\":\"msfs2024\"}",
    );
    f.app.configure(xbox.to_str().unwrap()).unwrap();
    let legacy = fs::read(f.app.state_dir.join("config.json")).unwrap();
    f.select(&item);
    assert_eq!(f.app.lock().runtime, Some(xbox.clone()));
    assert!(f.app.root().is_err());
    assert_eq!(
        fs::read(f.app.state_dir.join("config.json")).unwrap(),
        legacy
    );
    let reboot = Launcher::new(f.app.state_dir.clone(), None).unwrap();
    assert_eq!(reboot.lock().steam.as_ref().unwrap().id, item.id);
    for route in [
        "/api/setup/check",
        "/api/cloud-saves/import",
        "/api/game-update/start",
        "/api/graphics",
        "/api/proton/select",
        "/api/maintenance/start",
    ] {
        assert!(
            api::post(&f.app, route, &json!({}), "en").unwrap().is_err(),
            "{route}"
        );
    }
    assert!(f.app.lock().active.is_none());
    assert_eq!(
        fs::read(f.app.state_dir.join("config.json")).unwrap(),
        legacy
    );
    assert_eq!(f.app.status()["cloud"]["enabled"], false);
    assert_eq!(
        api::get(&f.app, "/api/cloud-saves").unwrap().unwrap()["available"],
        false
    );
    f.app.configure(xbox.to_str().unwrap()).unwrap();
    assert!(f.app.lock().steam.is_none());
    assert_eq!(f.app.root().unwrap(), xbox);
}
#[test]
fn changed_library_or_recreated_prefix_invalidates_reviewed_actions() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    let old = item.context();
    let previous = item.prefix().with_file_name("old-pfx");
    fs::rename(item.prefix(), &previous).unwrap();
    put(&item.prefix().join("system.reg"), b"new registry");
    assert_ne!(old, item.context());
    assert!(
        api::post(
            &f.app,
            "/api/installations/select",
            &json!({"installation_id":item.id,"binding":old}),
            "en"
        )
        .unwrap()
        .is_err()
    );
    assert!(f.app.lock().steam.is_none());
}
#[test]
fn moving_a_library_retains_the_adopted_id_and_never_selects_a_missing_volume() {
    let _guard = serial();
    let f = Fixture::new();
    let old = f.game(Game::Msfs2024, true);
    f.select(&old);
    let moved = f._temp.path().join("moved library");
    fs::rename(&f.library, &moved).unwrap();
    put(
        &f.root.join("steamapps/libraryfolders.vdf"),
        format!("\"libraryfolders\" {{\"0\" {{\"path\" {}}}}}", json!(moved)),
    );
    let registry = f.app.lock().installations.clone();
    let found = installations::discover(&f.home, &registry, &BTreeMap::new(), None);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, old.id);
    assert_ne!(found[0].context(), old.context());
    assert!(old.validate().is_err());
    found[0].validate().unwrap();
    assert_eq!(
        found[0].game_path().parent().unwrap(),
        moved.join("steamapps/common")
    );
}
#[test]
fn launch_uses_the_matching_client_and_never_a_shell_command() {
    let _guard = serial();
    let f = Fixture::new();
    let script = f.root.join("steam.sh");
    put(&script, b"#!/bin/sh\nexit 0\n");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let mut item = f.game(Game::Msfs2020, true);
    for (client, program, args) in [
        (Client::Native, "steam", vec!["-applaunch", "1250410"]),
        (
            Client::Flatpak,
            "flatpak",
            vec!["run", "com.valvesoftware.Steam", "-applaunch", "1250410"],
        ),
        (
            Client::Snap,
            "snap",
            vec!["run", "steam", "-applaunch", "1250410"],
        ),
    ] {
        if let Source::Steam { client: c, .. } = &mut item.source {
            *c = client;
        }
        let cmd = steam_session::command(&item, false).unwrap();
        assert_eq!(
            cmd.get_program(),
            if client == Client::Native {
                script.as_os_str()
            } else {
                std::ffi::OsStr::new(program)
            }
        );
        assert_eq!(
            cmd.get_args()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>(),
            args
        );
    }
}

#[test]
fn a_custom_native_client_cannot_send_the_game_to_a_different_running_steam() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    let script = f.root.join("steam.sh");
    put(&script, b"#!/bin/sh\nexit 0\n");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let wrong = f._temp.path().join("other-client/steam");
    files::private_dir(wrong.parent().unwrap()).unwrap();
    fs::copy("/bin/sleep", &wrong).unwrap();
    let mut process = Command::new(wrong).arg("30").spawn().unwrap();
    assert!(steam_session::command(&item, false).is_err());
    process.kill().unwrap();
    process.wait().unwrap();
    let correct = f.root.join("ubuntu12_32/steam");
    files::private_dir(correct.parent().unwrap()).unwrap();
    fs::copy("/bin/sleep", &correct).unwrap();
    let mut process = Command::new(correct).arg("30").spawn().unwrap();
    assert_eq!(
        steam_session::command(&item, false).unwrap().get_program(),
        script.as_os_str()
    );
    process.kill().unwrap();
    process.wait().unwrap();
}

#[test]
fn manual_discovery_requires_a_client_directory_rather_than_a_games_library() {
    let _guard = serial();
    let f = Fixture::new();
    assert!(
        installations::add_root(&f.app, &json!({"steam_root":f.library,"client":"native"}))
            .is_err()
    );
    put(
        &f.root.join("config/config.vdf"),
        b"\"InstallConfigStore\" {}\n",
    );
    installations::add_root(&f.app, &json!({"steam_root":f.root,"client":"native"})).unwrap();
    assert_eq!(
        installations::load(&f.app.state_dir)
            .unwrap()
            .steam_roots
            .len(),
        1
    );
}
#[test]
fn client_acknowledgement_is_not_a_running_game_and_external_games_are_reattached() {
    use std::os::unix::process::CommandExt;
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    f.select(&item);
    f.app.lock().steam_session = Some(steam_session::Session {
        installation: item.id.clone(),
        requested: Instant::now(),
        seen: false,
        client: None,
        stopping: false,
    });
    assert_eq!(f.app.status()["game"]["state"], "starting");
    assert_eq!(f.app.status()["game"]["can_start"], false);
    f.app.lock().steam_session = None;
    let mut child = std::process::Command::new("/bin/sleep")
        .arg0("FlightSimulator2024.exe")
        .arg("30")
        .env("WINEPREFIX", item.prefix())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !steam_session::running(&item).unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(f.app.status()["game"]["state"], "external");
    assert_eq!(f.app.status()["game"]["can_start"], false);
    let reboot = Launcher::new(f.app.state_dir.clone(), None).unwrap();
    assert_eq!(reboot.status()["game"]["state"], "external");
    steam_session::stop(&f.app).unwrap();
    let _ = child.wait();
    assert_eq!(f.app.status()["game"]["state"], "stopped");
}

#[test]
fn selection_journal_recovers_both_halves_before_loading_the_legacy_configuration() {
    let _guard = serial();
    let f = Fixture::new();
    let steam = f.game(Game::Msfs2024, true);
    let xbox = f._temp.path().join("xbox-original");
    put(&xbox.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
    put(
        &xbox.join("private/runtime.json"),
        b"{\"game_id\":\"msfs2024\"}",
    );
    f.app.configure(xbox.to_str().unwrap()).unwrap();
    let legacy: serde_json::Value =
        files::json(&f.app.state_dir.join("config.json"), 65536).unwrap();
    let mut registry = f.app.lock().installations.clone();
    registry.selected = Some(steam.id.clone());
    registry.entries.push(steam.clone());
    // The process was interrupted after publishing config.json, before the
    // corresponding installations.json. No source is inferred from an edition.
    put(
        &f.app.state_dir.join("installation-selection.json"),
        json!({"schema":1,"registry":registry,"legacy":legacy}).to_string(),
    );
    let reboot = Launcher::new(f.app.state_dir.clone(), None).unwrap();
    assert_eq!(reboot.lock().steam.as_ref().unwrap().id, steam.id);
    assert_eq!(reboot.lock().runtime.as_ref(), Some(&xbox));
    assert_eq!(
        files::json::<serde_json::Value>(&f.app.state_dir.join("config.json"), 65536).unwrap(),
        legacy
    );
    assert!(!f.app.state_dir.join("installation-selection.json").exists());
    let reboot = Launcher::new(f.app.state_dir.clone(), None).unwrap();
    assert_eq!(reboot.lock().steam.as_ref().unwrap().id, steam.id);
}

#[test]
fn old_xbox_configuration_migrates_without_changing_the_rollback_format() {
    let _guard = serial();
    let f = Fixture::new();
    let mut known = BTreeMap::new();
    for game in [Game::Msfs2020, Game::Msfs2024] {
        let xbox = f._temp.path().join(game.id());
        put(&xbox.join("tools/play-msfs.sh"), b"#!/bin/sh\n");
        put(
            &xbox.join("private/runtime.json"),
            json!({"game_id":game.id()}).to_string(),
        );
        known.insert(game.id(), xbox);
    }
    let config = json!({"schema":1,"runtime_path":known["msfs2024"],"runtimes":known}).to_string();
    put(&f.app.state_dir.join("config.json"), &config);
    let app = Launcher::new(f.app.state_dir.clone(), None).unwrap();
    assert_eq!(app.lock().installations.entries.len(), 2);
    assert!(app.lock().steam.is_none());
    assert_eq!(
        fs::read_to_string(f.app.state_dir.join("config.json")).unwrap(),
        config
    );
    assert_eq!(app.root().unwrap(), known["msfs2024"]);
    let xbox2020 = app
        .lock()
        .installations
        .entries
        .iter()
        .find(|i| i.game == Game::Msfs2020)
        .unwrap()
        .clone();
    installations::select(
        &app,
        &json!({"installation_id":xbox2020.id,"binding":xbox2020.context()}),
    )
    .unwrap();
    assert_eq!(app.root().unwrap(), known["msfs2020"]);
    let registry = installations::load(&f.app.state_dir).unwrap();
    assert_eq!(
        registry.selected,
        registry
            .entries
            .iter()
            .find(|i| i.game == Game::Msfs2020)
            .map(|i| i.id.clone())
    );
}

#[test]
fn a_launch_timeout_remains_visible_and_a_different_profile_is_never_stopped() {
    use std::os::unix::process::CommandExt;
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    f.select(&item);
    f.app.lock().steam_session = Some(steam_session::Session {
        installation: item.id.clone(),
        requested: Instant::now() - Duration::from_secs(181),
        seen: false,
        client: None,
        stopping: false,
    });
    assert_eq!(f.app.status()["game"]["state"], "stopped");
    assert!(
        !f.app.status()["game"]["message"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    let other = f._temp.path().join("another-prefix");
    files::private_dir(&other).unwrap();
    let mut child = std::process::Command::new("/bin/sleep")
        .arg0("FlightSimulator2024.exe")
        .arg("30")
        .env("WINEPREFIX", &other)
        .spawn()
        .unwrap();
    assert!(!steam_session::running(&item).unwrap());
    assert!(steam_session::stop(&f.app).is_err());
    assert!(child.try_wait().unwrap().is_none());
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn a_sandboxed_process_is_bound_to_the_directory_in_its_mount_namespace() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    let sandbox = f._temp.path().join("sandbox-prefix");
    files::private_dir(&sandbox).unwrap();
    let mut child = std::process::Command::new("bwrap")
        .args(["--ro-bind", "/", "/", "--bind"])
        .arg(&sandbox)
        .arg(item.prefix())
        .args([
            "--",
            "bash",
            "-c",
            "exec -a FlightSimulator2024.exe /bin/sleep 30",
        ])
        .env("WINEPREFIX", item.prefix())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while !flightdeck::wine_processes::Processes::new(&sandbox)
        .unwrap()
        .any(Some(&["flightsimulator2024.exe"]))
    {
        assert!(
            child.try_wait().unwrap().is_none(),
            "namespace fixture exited"
        );
        assert!(
            Instant::now() < end,
            "namespace process was not attributed to its real prefix"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!steam_session::running(&item).unwrap());
    flightdeck::wine_processes::terminate_named(
        &sandbox,
        &["flightsimulator2024.exe"],
        Duration::from_secs(2),
    )
    .unwrap();
    let _ = child.wait();
}

#[test]
fn community_uses_the_selected_steam_profile_and_rejects_ambiguous_configuration() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    let drive = item.prefix().join("drive_c");
    files::private_dir(&item.prefix().join("dosdevices")).unwrap();
    files::private_dir(&drive).unwrap();
    symlink(&drive, item.prefix().join("dosdevices/c:")).unwrap();
    let config = drive
        .join("users/steamuser/AppData/Roaming")
        .join(item.game.user_config())
        .join("UserCfg.opt");
    put(&config, b"InstalledPackagesPath \"C:\\Packages\"\n");
    put(
        &drive.join("Packages/Community/fixture/manifest.json"),
        json!({"title":"Fixture aircraft","package_version":"1.2.3","creator":"Synthetic"})
            .to_string(),
    );
    f.select(&item);
    let result = api::get(&f.app, "/api/mods").unwrap().unwrap();
    assert_eq!(result["runtime_path"], item.context());
    assert_eq!(result["state"], "ready");
    assert_eq!(
        result["folder_path"],
        drive.join("Packages/Community").to_string_lossy().as_ref()
    );
    assert_eq!(result["mods"][0]["name"], "Fixture aircraft");
    assert_eq!(result["can_remove"], false);
    let second = drive
        .join("users/another/AppData/Roaming")
        .join(item.game.user_config())
        .join("UserCfg.opt");
    put(&second, b"InstalledPackagesPath \"C:\\OtherPackages\"\n");
    assert_eq!(
        api::get(&f.app, "/api/mods").unwrap().unwrap()["state"],
        "ambiguous"
    );
}

#[test]
fn cancellation_is_bound_to_the_selected_profile_and_current_fenix_job() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    f.select(&item);
    let ctx = f.app.reserve("fenix", "installer", false).unwrap();
    ctx.update(json!({"runtime_path":item.context()}));
    assert!(
        api::post(&f.app, "/api/fenix/cancel", &json!({"job_id":"old"}), "en")
            .unwrap()
            .is_err()
    );
    assert!(!ctx.cancel.load(std::sync::atomic::Ordering::Relaxed));
    api::post(&f.app, "/api/fenix/cancel", &json!({"job_id":ctx.id}), "en")
        .unwrap()
        .unwrap();
    assert!(ctx.cancel.load(std::sync::atomic::Ordering::Relaxed));
    f.app.finish(&ctx, Err(flightdeck::Error::Cancelled), false);
    assert_eq!(f.app.job("fenix")["state"], "cancelled");
    assert!(item.prefix().join("system.reg").is_file());
}

#[test]
fn owned_patch_jobs_survive_their_profile_swap_but_old_hook_evidence_does_not() {
    let _guard = serial();
    let f = Fixture::new();
    let item = f.game(Game::Msfs2024, true);
    f.select(&item);
    let original = item.context();
    let generation = item.prefix_generation();
    let evidence = f
        .app
        .state_dir
        .join("steam")
        .join(&item.id)
        .join("profiles")
        .join(&generation);
    files::private_dir(&evidence.join("private")).unwrap();
    let mut attempt = flightdeck::fenix_diagnostics::begin(&evidence, "repair", "proton").unwrap();
    attempt.status = "failed".into();
    attempt.completed_at = Some(files::now());
    attempt.failure = Some("hook_nonzero".into());
    attempt.hook_exit_code = Some(82);
    flightdeck::fenix_diagnostics::save(&evidence, &attempt).unwrap();
    assert_eq!(
        flightdeck::steam_fenix::latest_attempt(&f.app.state_dir, &item)["status"],
        "failed"
    );
    let ctx = f.app.reserve("fenix", "install", false).unwrap();
    fs::rename(item.prefix(), item.prefix().with_file_name("original-pfx")).unwrap();
    put(&item.prefix().join("system.reg"), "new registry");
    assert_ne!(item.context(), original);
    assert_ne!(item.prefix_generation(), generation);
    let snapshot = flightdeck::steam_fenix::snapshot(&f.app);
    assert_eq!(snapshot["job"]["id"], ctx.id);
    assert!(snapshot["last_attempt"].is_null());
    api::post(&f.app, "/api/fenix/cancel", &json!({"job_id":ctx.id}), "en")
        .unwrap()
        .unwrap();
    f.app.finish(&ctx, Err(flightdeck::Error::Cancelled), false);
}
