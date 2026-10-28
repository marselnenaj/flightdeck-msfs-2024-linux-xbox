// SPDX-License-Identifier: MIT
//! Public companion workflows bind to the selected real Steam prefix/runner.
#![allow(clippy::unwrap_used)]
use flightdeck::{
    api,
    backend::Launcher,
    files,
    games::Game,
    installations::{self, Client, Installation, Source},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

fn put(path: &Path, content: impl AsRef<[u8]>) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, content.as_ref()).unwrap();
}
struct Fixture {
    _temp: tempfile::TempDir,
    app: Arc<Launcher>,
    steam: PathBuf,
    library: PathBuf,
    items: Vec<Installation>,
    xbox: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let steam = temp.path().join("Steam client");
        let library = temp.path().join("external library");
        files::private_dir(&steam.join("steamapps")).unwrap();
        files::private_dir(&library.join("steamapps")).unwrap();
        put(
            &steam.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{\"0\" {{\"path\" {}}} \"1\" {{\"path\" {}}}}}",
                json!(steam),
                json!(library)
            ),
        );
        let app = Launcher::new(temp.path().join("state"), None).unwrap();
        let xbox = temp.path().join("saved Xbox runtime");
        put(&xbox.join("tools/play-msfs.sh"), "#!/bin/sh\n");
        put(
            &xbox.join("private/runtime.json"),
            "{\"game_id\":\"msfs2024\"}",
        );
        app.configure(xbox.to_str().unwrap()).unwrap();
        app.lock()
            .installations
            .steam_roots
            .push(installations::SteamRoot {
                client: Client::Native,
                path: steam.clone(),
            });
        let mut items = Vec::new();
        for game in Game::ALL {
            let item = Installation::new(
                game,
                Source::Steam {
                    client: Client::Native,
                    root: steam.clone(),
                    library: library.clone(),
                    install_dir: game.id().into(),
                },
            )
            .unwrap();
            files::private_dir(&item.game_path()).unwrap();
            put(
                &library.join(format!("steamapps/appmanifest_{}.acf", item.appid())),
                format!(
                    "\"AppState\" {{\"appid\" \"{}\" \"installdir\" \"{}\" \"StateFlags\" \"4\"}}",
                    item.appid(),
                    game.id()
                ),
            );
            Self::prefix(&item);
            items.push(item);
        }
        let f = Self {
            _temp: temp,
            app,
            steam,
            library,
            items,
            xbox,
        };
        f.runner("fixture-proton");
        f.selection("fixture-proton");
        f
    }
    fn prefix(item: &Installation) {
        let prefix = item.prefix();
        files::private_dir(&prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        files::private_dir(&prefix.join("drive_c/windows/system32")).unwrap();
        put(&prefix.join("system.reg"), "WINE REGISTRY Version 2\n");
        put(
            &Self::settings(item).join("UserCfg.opt"),
            "InstalledPackagesPath \"C:\\Packages\"\r\nOtherSetting 1\r\n",
        );
        files::private_dir(&prefix.join("drive_c/Packages/Community")).unwrap();
    }
    fn settings(item: &Installation) -> PathBuf {
        item.prefix()
            .join("drive_c/users/steamuser/AppData/Roaming")
            .join(item.game.user_config())
    }
    fn runner(&self, name: &str) -> PathBuf {
        let path = self.steam.join("compatibilitytools.d").join(name);
        for file in ["proton", "files/bin/wine", "files/bin/wineserver"] {
            put(&path.join(file), "#!/bin/sh\nexit 0\n");
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path.join(file), fs::Permissions::from_mode(0o700)).unwrap();
        }
        put(&path.join("version"), "fixture runner 11.0\n");
        put(
            &path.join("compatibilitytool.vdf"),
            format!(
                "\"compatibilitytools\" {{\"compat_tools\" {{\"{name}\" {{\"install_path\" \".\"}}}}}}"
            ),
        );
        path
    }
    fn selection(&self, name: &str) {
        let config = r#""InstallConfigStore"
{
 "Software" {
  "Valve" {
   "Steam" {
    "CompatToolMapping" {
     "2537590" {"name" "RUNNER"}
     "1250410" {"name" "RUNNER"}
    }
   }
  }
 }
}
"#
        .replace("RUNNER", name);
        put(&self.steam.join("config/config.vdf"), config);
    }
    fn select(&self, item: &Installation) {
        installations::select(
            &self.app,
            &json!({"installation_id":item.id,"binding":item.context()}),
        )
        .unwrap();
    }
    fn status(&self, kind: &str) -> Value {
        api::get(&self.app, &format!("/api/{kind}"))
            .unwrap()
            .unwrap()
    }
    fn action(&self, kind: &str, op: &str, status: &Value) -> flightdeck::Result<Value> {
        api::post(
            &self.app,
            &format!("/api/{kind}/{op}"),
            &json!({"runtime_path":status["runtime_path"],"profile_token":status["profile_token"]}),
            "en",
        )
        .unwrap()
    }
    fn settled(&self, kind: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = self.status(kind);
            if value["job"]["state"] != "running" {
                return value;
            }
            assert!(Instant::now() < deadline, "job timed out: {value}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn both_steam_editions_configure_orbx_navigraph_and_gsx_without_touching_saved_xbox() {
    let f = Fixture::new();
    let original = fs::read(f.app.state_dir.join("config.json")).unwrap();
    for item in &f.items {
        f.select(item);
        for kind in ["orbx", "navigraph", "gsx"] {
            let status = f.status(kind);
            assert_eq!(status["state"], "available", "{kind}: {status}");
            assert_eq!(status["source"], "steam");
            assert_eq!(status["runtime_path"], item.context());
            assert_eq!(status["game"], item.game.id());
        }
        let settings = Fixture::settings(item);
        let user_cfg = fs::read(settings.join("UserCfg.opt")).unwrap();
        let status = f.status("orbx");
        f.action("orbx", "configure", &status).unwrap();
        let done = f.settled("orbx");
        assert_eq!(done["job"]["state"], "complete", "{done}");
        assert_eq!(done["configured"], true);
        let simlink = item
            .prefix()
            .join("drive_c/Program Files/Navigraph/Simlink/NavigraphSimlink.exe");
        put(&simlink, "synthetic application");
        let manager = item
            .prefix()
            .join("drive_c/Program Files (x86)/Addon Manager");
        put(&manager.join("Couatl_Updater.exe"), "synthetic manager");
        put(&manager.join("couatl64_msfs.exe"), "synthetic companion");
        put(&settings.join("exe.xml"),br#"<SimBase.Document><Disabled>False</Disabled><Launch.Addon><Name>GSX</Name><Disabled>True</Disabled><Path>C:\Program Files (x86)\Addon Manager\couatl64_msfs.exe</Path></Launch.Addon><Launch.Addon><Name>Other app</Name><Path>C:\Other.exe</Path></Launch.Addon></SimBase.Document>"#);
        put(
            &item
                .prefix()
                .join("drive_c/Packages/Community/fsdreamteam-gsx-pro/manifest.json"),
            br#"{"title":"GSX fixture","package_version":"1.0"}"#,
        );
        let status = f.status("navigraph");
        f.action("navigraph", "configure", &status).unwrap();
        let done = f.settled("navigraph");
        assert_eq!(done["job"]["state"], "complete", "{done}");
        assert_eq!(done["startup"], true);
        let status = f.status("gsx");
        f.action("gsx", "configure", &status).unwrap();
        let done = f.settled("gsx");
        assert_eq!(done["job"]["state"], "complete", "{done}");
        let xml = fs::read_to_string(settings.join("exe.xml")).unwrap();
        assert!(
            xml.contains("Other app")
                && xml.contains("NavigraphSimlink.exe")
                && xml.contains("couatl64_msfs.exe")
        );
        assert_eq!(fs::read(settings.join("UserCfg.opt")).unwrap(), user_cfg);
        assert!(
            !f.app
                .state_dir
                .join("steam")
                .join(&item.id)
                .join("addon-apps/local/msfs-prefix")
                .exists()
        );
    }
    assert_eq!(
        fs::read(f.app.state_dir.join("config.json")).unwrap(),
        original
    );
    assert!(!f.xbox.join("local").exists());
    assert!(f.library.join("steamapps/compatdata").is_dir());
}

#[test]
fn changed_proton_and_recreated_profile_reject_stale_companion_actions() {
    let f = Fixture::new();
    let item = &f.items[0];
    f.select(item);
    let before = f.status("orbx");
    f.runner("other-proton");
    f.selection("other-proton");
    assert!(f.action("orbx", "configure", &before).is_err());
    assert!(!f.app.state_dir.join("steam").join(&item.id).exists());
    let current = f.status("orbx");
    assert_ne!(before["profile_token"], current["profile_token"]);
    fs::rename(item.prefix(), item.prefix().with_file_name("retained-pfx")).unwrap();
    Fixture::prefix(item);
    assert!(f.action("orbx", "configure", &current).is_err());
    let fresh = f.status("orbx");
    f.action("orbx", "configure", &fresh).unwrap();
    assert_eq!(f.settled("orbx")["job"]["state"], "complete");
    let other = &f.items[1];
    assert!(
        !other
            .prefix()
            .join("drive_c/users/steamuser/AppData/Roaming/Orbx")
            .exists()
    );
}

#[test]
fn steam_prefix_lock_blocks_mutations_and_preserves_steam_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let item = &f.items[0];
    f.select(item);
    let lock = item.prefix().parent().unwrap().join("pfx.lock");
    put(&lock, "unchanged");
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    rustix::fs::flock(&held, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    f.action("orbx", "configure", &f.status("orbx")).unwrap();
    assert_eq!(f.settled("orbx")["job"]["state"], "failed");
    assert_eq!(
        fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(fs::read_to_string(&lock).unwrap(), "unchanged");
    drop(held);
    f.action("orbx", "configure", &f.status("orbx")).unwrap();
    assert_eq!(f.settled("orbx")["job"]["state"], "complete");
}

#[test]
fn steam_runner_resolution_covers_global_builtin_recorded_and_ambiguous_tools() {
    let f = Fixture::new();
    let item = &f.items[0];
    f.select(item);
    let config = f.steam.join("config/config.vdf");
    let explicit = fs::read_to_string(&config).unwrap();
    put(
        &config,
        explicit
            .replace("\"2537590\"", "\"0\"")
            .replace("\"1250410\" {\"name\" \"fixture-proton\"}", ""),
    );
    assert_eq!(f.status("orbx")["state"], "available");
    let runner = f.steam.join("compatibilitytools.d/fixture-proton");
    let builtin = f.library.join("steamapps/common/Proton - Experimental");
    fs::create_dir_all(builtin.parent().unwrap()).unwrap();
    fs::rename(&runner, &builtin).unwrap();
    fs::remove_file(builtin.join("compatibilitytool.vdf")).unwrap();
    f.selection("proton_experimental");
    assert_eq!(f.status("orbx")["state"], "available");
    fs::remove_file(&config).unwrap();
    files::private_dir(&builtin.join("files/share/fonts")).unwrap();
    put(
        &item.prefix().parent().unwrap().join("config_info"),
        format!("11.0-0\n{}\n", builtin.join("files/share/fonts").display()),
    );
    assert_eq!(f.status("orbx")["state"], "available");
    f.runner("fixture-proton");
    let duplicate = f.runner("duplicate-tool");
    put(
        &duplicate.join("compatibilitytool.vdf"),
        "\"compatibilitytools\" {\"compat_tools\" {\"fixture-proton\" {\"install_path\" \".\"}}}",
    );
    f.selection("fixture-proton");
    assert_eq!(f.status("orbx")["state"], "unavailable");
}

#[test]
fn steam_gsx_recovers_all_publication_boundaries_and_blocks_game_launch() {
    use flightdeck::{fenix, gsx, transaction};
    for phase in 0..3 {
        let f = Fixture::new();
        let item = &f.items[0];
        f.select(item);
        let root = f
            .app
            .state_dir
            .join("steam")
            .join(&item.id)
            .join("addon-apps");
        let prefix = item.prefix();
        let id = fenix::identity(&prefix).unwrap();
        let token = "0123456789abcdef0123456789abcdef";
        let stage = prefix.with_file_name(format!(".flightdeck-gsx-{token}"));
        let backup = prefix.with_file_name(format!("pfx.before-gsx-{token}"));
        files::private_dir(&stage.join("drive_c/windows/system32")).unwrap();
        put(&stage.join("system.reg"), "partial setup");
        let runner = f.steam.join("compatibilitytools.d/fixture-proton");
        let staged_id = fenix::identity(&stage).unwrap();
        let prior = json!({"format":1,"state":"ready","id":"11111111111111111111111111111111"});
        put(&root.join(gsx::MARKER),serde_json::to_vec(&json!({"format":1,"state":"committing","id":token,"original_prefix_id":id,"staged_prefix_id":staged_id,"runner":runner,"prior":prior})).unwrap());
        if phase > 0 {
            transaction::publish(&prefix, &backup).unwrap();
        }
        if phase == 2 {
            transaction::publish(&stage, &prefix).unwrap();
        }
        assert_eq!(f.app.status()["game"]["can_start"], false);
        assert!(flightdeck::steam_session::launch(&f.app).is_err());
        // Recovery closes a staging server using its recorded runner even after
        // the user selected another Proton version in Steam.
        f.runner("replacement-proton");
        f.selection("replacement-proton");
        let status = f.status("gsx");
        assert_eq!(status["can_recover"], true, "{status}");
        f.action("gsx", "recover", &status).unwrap();
        let done = f.settled("gsx");
        assert_eq!(done["job"]["state"], "complete", "{done}");
        assert_eq!(fenix::identity(&prefix).unwrap(), id);
        assert_eq!(gsx::marker(&root).unwrap().unwrap(), prior);
        assert_eq!(f.app.status()["game"]["can_start"], true);
    }
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn companion(prefix: &Path, name: &str) -> Child {
    use std::{os::unix::process::CommandExt, process::Command};
    Child(
        Command::new("/bin/sleep")
            .arg0(name)
            .arg("30")
            .env("WINEPREFIX", prefix)
            .spawn()
            .unwrap(),
    )
}
#[test]
fn a_steam_game_exit_cleans_only_enabled_companions_in_its_real_profile() {
    let f = Fixture::new();
    let item = &f.items[0];
    let prefix = item.prefix();
    let root = f
        .app
        .state_dir
        .join("steam")
        .join(&item.id)
        .join("addon-apps");
    for marker in [flightdeck::navigraph::STARTUP, flightdeck::gsx::STARTUP] {
        put(&root.join(marker),serde_json::to_vec(&json!({"format":1,"enabled":true,"prefix_id":flightdeck::fenix::identity(&prefix).unwrap()})).unwrap());
    }
    let mut game = companion(&prefix, item.game.executable());
    let mut simlink = companion(&prefix, "NavigraphSimlink.exe");
    let mut couatl = companion(&prefix, "couatl64_msfs2024.exe");
    let mut orbx = companion(&prefix, "Orbx Central.exe");
    let mut foreign = companion(&f.items[1].prefix(), "NavigraphSimlink.exe");
    f.select(item);
    assert_eq!(f.app.status()["game"]["state"], "external");
    game.0.kill().unwrap();
    game.0.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while simlink.0.try_wait().unwrap().is_none() || couatl.0.try_wait().unwrap().is_none() {
        let _ = f.app.status();
        assert!(Instant::now() < deadline, "companion cleanup timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(simlink.0.try_wait().unwrap().is_some());
    assert!(couatl.0.try_wait().unwrap().is_some());
    assert!(orbx.0.try_wait().unwrap().is_none());
    assert!(foreign.0.try_wait().unwrap().is_none());
}

#[test]
fn navigraph_browser_return_uses_the_waiting_steam_profile_and_its_environment() {
    let f = Fixture::new();
    f.select(&f.items[1]);
    f.select(&f.items[0]);
    for item in &f.items {
        put(
            &item
                .prefix()
                .join("drive_c/Program Files/Navigraph/Simlink/NavigraphSimlink.exe"),
            "synthetic application",
        );
    }
    let link = "navigraph-traffic-desktop://callback?code=synthetic-test";
    assert!(
        flightdeck::navigraph::forward(&f.app.state_dir, link).is_err(),
        "two installed profiles require a waiting app"
    );
    let item = &f.items[1];
    let _waiting = companion(&item.prefix(), "NavigraphSimlink.exe");
    let runner = f.steam.join("compatibilitytools.d/fixture-proton");
    put(
        &runner.join("files/bin/wine"),
        "#!/bin/sh\nprintf '%s\\n' \"$WINEPREFIX\" \"$SteamAppId\" \"$STEAM_COMPAT_DATA_PATH\" \"$STEAM_COMPAT_CLIENT_INSTALL_PATH\" \"$1\" \"$2\" > \"$WINEPREFIX/callback-observation\"\n",
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        runner.join("files/bin/wine"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    flightdeck::navigraph::forward(&f.app.state_dir, link).unwrap();
    let captured = fs::read_to_string(item.prefix().join("callback-observation")).unwrap();
    let rows: Vec<_> = captured.lines().collect();
    assert_eq!(rows[0], item.prefix().to_str().unwrap());
    assert_eq!(rows[1], item.appid());
    assert_eq!(rows[2], item.prefix().parent().unwrap().to_str().unwrap());
    assert_eq!(rows[3], f.steam.to_str().unwrap());
    assert!(rows[4].ends_with("NavigraphSimlink.exe"));
    assert_eq!(rows[5], link);
    assert!(!f.items[0].prefix().join("callback-observation").exists());
}

#[test]
#[ignore = "explicit account-free Wine probe; requires an existing Proton and official Simlink installer"]
fn real_wine_simlink_install_into_an_isolated_steam_profile() {
    use std::{os::unix::fs::symlink, process::Command, sync::atomic::AtomicBool};
    let runner = PathBuf::from(
        std::env::var_os("FLIGHTDECK_TEST_STEAM_RUNNER").expect("existing Proton directory"),
    );
    let installer = PathBuf::from(
        std::env::var_os("FLIGHTDECK_TEST_SIMLINK_INSTALLER").expect("official Simlink installer"),
    );
    let f = Fixture::new();
    let item = &f.items[0];
    let selected = f.steam.join("compatibilitytools.d/fixture-proton");
    fs::remove_dir_all(selected.join("files")).unwrap();
    symlink(runner.join("files"), selected.join("files")).unwrap();
    // Let real Wine create its entire profile, including the Unix drive map.
    fs::remove_dir_all(item.prefix()).unwrap();
    struct Cleanup {
        prefix: PathBuf,
        runner: PathBuf,
        log: PathBuf,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let cancel = AtomicBool::new(false);
            if let Ok(wine) =
                flightdeck::wine::Wine::new(&self.prefix, &self.runner, &self.log, &cancel)
            {
                let _ = wine.stop_staged();
            }
        }
    }
    let _cleanup = Cleanup {
        prefix: item.prefix(),
        runner: runner.clone(),
        log: f._temp.path().join("cleanup.log"),
    };
    let log = fs::File::create(f._temp.path().join("wineboot.log")).unwrap();
    let mut boot = Command::new(flightdeck::runtime::wine(&runner));
    boot.args(["wineboot.exe", "-u"])
        .env_clear()
        .envs(flightdeck::wine::environment(&item.prefix(), &runner))
        .env("WINEDLLOVERRIDES", "winemenubuilder.exe=d;mscoree,mshtml=d")
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    let status =
        flightdeck::process::run(&mut boot, Duration::from_secs(180), &AtomicBool::new(false))
            .unwrap();
    assert!(
        status.success(),
        "isolated Wine initialization failed: {}",
        fs::read_to_string(f._temp.path().join("wineboot.log")).unwrap_or_default()
    );
    flightdeck::wine::Wine::new(
        &item.prefix(),
        &runner,
        &f._temp.path().join("flush.log"),
        &AtomicBool::new(false),
    )
    .unwrap()
    .stop_staged()
    .unwrap();
    fenix_patch::steam::checked(&item.target().unwrap(), false).expect("real profile validation");
    put(
        &Fixture::settings(item).join("UserCfg.opt"),
        "InstalledPackagesPath \"C:\\Packages\"\r\nOtherSetting 1\r\n",
    );
    files::private_dir(&item.prefix().join("drive_c/Packages/Community")).unwrap();
    f.select(item);
    let before = flightdeck::fenix::identity(&item.prefix()).unwrap();
    let user_cfg = fs::read(Fixture::settings(item).join("UserCfg.opt")).unwrap();
    let status = f.status("navigraph");
    api::post(&f.app,"/api/navigraph/install-simlink",&json!({"runtime_path":status["runtime_path"],"profile_token":status["profile_token"],"installer_path":installer}),"en").unwrap().unwrap();
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let status = f.status("navigraph");
        if status["job"]["state"] != "running" {
            if status["job"]["state"] != "complete" {
                let path = f
                    .app
                    .state_dir
                    .join("steam")
                    .join(&item.id)
                    .join("addon-apps/private/navigraph-simlink-setup.log");
                eprintln!(
                    "Isolated account-free installer log: {}",
                    fs::read_to_string(path)
                        .unwrap_or_default()
                        .chars()
                        .take(4096)
                        .collect::<String>()
                );
            }
            assert_eq!(status["job"]["state"], "complete", "{status}");
            assert_eq!(status["simlink_installed"], true);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "official Simlink install timed out"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(flightdeck::fenix::identity(&item.prefix()).unwrap(), before);
    assert_eq!(
        fs::read(Fixture::settings(item).join("UserCfg.opt")).unwrap(),
        user_cfg
    );
    let cancel = AtomicBool::new(false);
    flightdeck::wine::Wine::new(
        &item.prefix(),
        &runner,
        &f._temp.path().join("cleanup.log"),
        &cancel,
    )
    .unwrap()
    .stop_staged()
    .unwrap();
    println!(
        "Official Simlink installed into isolated Steam-shaped profile with the selected real Wine; profile identity and simulator settings retained. No simulator flight or sign-in tested."
    );
}
