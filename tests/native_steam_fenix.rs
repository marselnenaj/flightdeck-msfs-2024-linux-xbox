// SPDX-License-Identifier: MIT
//! Exercise Flightdeck's adapter against the pinned standalone engine, with
//! real files and transactions and synthetic Windows prerequisites only.
#![allow(clippy::unwrap_used)]
use fenix_patch::{
    core::{RunOptions, Wine},
    fsx,
    json::{self, Value},
    process::Env,
    steam,
    system::{Progress, System, silent},
};
use flightdeck::{
    games::Game,
    installations::{Client, Installation, Source},
    steam_fenix,
};
use serde_json::json as j;
use std::{
    ffi::OsString,
    fs::{self, File},
    os::unix::fs::{MetadataExt, symlink},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn put(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn digest(path: &Path) -> String {
    fsx::digest(path).unwrap()
}
fn hashes(base: &Path, names: &[&str]) -> serde_json::Value {
    names
        .iter()
        .map(|name| ((*name).to_string(), j!(digest(&base.join(name)))))
        .collect()
}
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}
struct NoWine {
    prefix: PathBuf,
    runner: PathBuf,
    env: Env,
}
impl Wine for NoWine {
    fn prefix(&self) -> &Path {
        &self.prefix
    }
    fn runner(&self) -> &Path {
        &self.runner
    }
    fn env(&self) -> &Env {
        &self.env
    }
    fn run(&self, _: &[OsString], _: &RunOptions<'_>) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn reg(&self, _: &str, _: &str, _: &str, _: &str) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn stop(&self) -> fenix_patch::Result<()> {
        Ok(())
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    bundle: PathBuf,
    steam_root: PathBuf,
    library: PathBuf,
    proton: PathBuf,
    lock: Mutex<serde_json::Value>,
    fail_rename: AtomicUsize,
    renames: AtomicUsize,
    busy: Mutex<Option<PathBuf>>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("Steam client");
        let library = base.join("second library");
        let bundle = base.join("bundle");
        let proton = root.join("compatibilitytools.d/pinned-proton");
        put(
            &root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{\"0\" {{\"path\" {}}} \"1\" {{\"path\" {}}}}}",
                j!(root),
                j!(library)
            ),
        );
        put(
            &root.join("config/config.vdf"),
            r#""InstallConfigStore"
{
 "Software"
 {
  "Valve"
  {
   "Steam"
   {
    "Unrelated" "preserve me"
    "CompatToolMapping"
    {
     "42"
     {
      "name" "other-tool"
      "config" ""
      "priority" "250"
     }
    }
   }
  }
 }
}
"#
            .lines()
            .map(|line| {
                let n = line.len() - line.trim_start_matches(' ').len();
                format!("{}{}\n", "\t".repeat(n), &line[n..])
            })
            .collect::<String>(),
        );

        for (name, bytes) in [
            ("proton", "launcher"),
            ("version", "fixture version"),
            ("files/bin/wine", "fixture Wine"),
            ("files/bin/wineserver", "old server"),
            ("files/lib/wine/x86_64-windows/ntdll.dll", "old ntdll"),
        ] {
            put(&proton.join(name), bytes);
        }
        let names = [
            "files/bin/wineserver",
            "files/lib/wine/x86_64-windows/ntdll.dll",
        ];
        for name in names {
            put(
                &bundle.join("payload/variants/fixture").join(name),
                format!("new {name}"),
            );
        }
        let helpers = [
            "FenixWindowGuard.exe",
            "FenixMCDURefresh.exe",
            "fenix-runtime",
        ];
        for name in helpers {
            put(
                &bundle.join("integration").join(name),
                format!("synthetic {name}"),
            );
        }
        let lock = j!({"format":1,"version":"0.1.1","runner_version":"base","files":{},"runner_files":{"absent":"0"},"integration":hashes(&bundle.join("integration"),&helpers),"variants":{"fixture":{"runner_version":"1 fixture version","files":hashes(&bundle.join("payload/variants/fixture"),&names),"runner_files":hashes(&proton,&[names[0],names[1],"files/bin/wine"])}}});
        put(&bundle.join("bundle.json"), lock.to_string());
        Self {
            _temp: temp,
            bundle,
            steam_root: root,
            library,
            proton,
            lock: Mutex::new(lock),
            fail_rename: AtomicUsize::new(0),
            renames: AtomicUsize::new(0),
            busy: Mutex::new(None),
        }
    }
    fn item(&self, game: Game) -> Installation {
        let id = if game == Game::Msfs2024 {
            "2537590"
        } else {
            "1250410"
        };
        put(
            &self.library.join(format!("steamapps/appmanifest_{id}.acf")),
            format!(
                "\"AppState\" {{\"appid\" \"{id}\" \"installdir\" {}}}",
                j!(game.id())
            ),
        );
        fs::create_dir_all(self.library.join("steamapps/common").join(game.id())).unwrap();
        let item = Installation::new(
            game,
            Source::Steam {
                client: Client::Native,
                root: self.steam_root.clone(),
                library: self.library.clone(),
                install_dir: game.id().into(),
            },
        )
        .unwrap();
        put(&item.prefix().join("system.reg"), "original registry");
        put(
            &item.target().unwrap().compat().join("version"),
            "original version",
        );
        fs::create_dir_all(item.prefix().join("drive_c/windows/system32")).unwrap();
        fs::create_dir_all(item.prefix().join("dosdevices")).unwrap();
        symlink(
            item.prefix().join("drive_c"),
            item.prefix().join("dosdevices/c:"),
        )
        .unwrap();
        put(&self.settings(&item).join("UserCfg.opt"), "settings");
        put(
            &self
                .settings(&item)
                .join("Packages/Community/aircraft/layout.json"),
            "large simulator content",
        );
        item
    }
    fn settings(&self, item: &Installation) -> PathBuf {
        item.prefix()
            .join("drive_c/users/steamuser/AppData/Roaming")
            .join(item.game.user_config())
    }
    fn tool(&self) -> PathBuf {
        self.steam_root
            .join("compatibilitytools.d/Proton-Fenix-A320-fixture")
    }
    fn action(&self, item: &Installation, op: &str) -> fenix_patch::Result<()> {
        steam_fenix::perform(self, item, op, &self.bundle, None, &silent)
    }
    fn state(&self, item: &Installation) -> Value {
        fsx::read_json(&item.target().unwrap().marker()).unwrap()
    }
    fn release(&self) {
        let mut lock = self.lock.lock().unwrap();
        let old = lock["version"].as_str().unwrap().to_string();
        lock["variants"]["fixture"]["previous_releases"] =
            j!({old:{"files":lock["variants"]["fixture"]["files"]}});
        lock["version"] = j!("0.1.2");
        let file = self
            .bundle
            .join("payload/variants/fixture/files/bin/wineserver");
        put(&file, "next server");
        lock["variants"]["fixture"]["files"]["files/bin/wineserver"] = j!(digest(&file));
        put(&self.bundle.join("bundle.json"), lock.to_string());
    }
}
impl System for Fixture {
    fn prepare_clr_loader(&self, wine: &dyn Wine) -> fenix_patch::Result<()> {
        // This adapter has synthetic registry/prerequisite steps and a
        // deliberately non-executable Wine file. Real migration is covered
        // by the CLR loader tests and the account-free Wine probe.
        assert!(wine.prefix().starts_with(self.bundle.parent().unwrap()));
        Ok(())
    }

    fn root(&self) -> &Path {
        &self.bundle
    }
    fn home(&self) -> PathBuf {
        self._temp.path().join("home")
    }
    fn host_check(&self) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn require_display(&self) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn steam_running(&self) -> bool {
        false
    }
    fn manifest(&self, variant: Option<&str>) -> fenix_patch::Result<Value> {
        let mut lock = self.lock.lock().unwrap().clone();
        if let Some(variant) = variant {
            let entry = lock["variants"][variant].as_object().unwrap().clone();
            lock.as_object_mut().unwrap().extend(entry);
            lock["variant"] = j!(variant);
        }
        json::loads(&lock.to_string())
    }
    fn wine<'a>(&'a self, prefix: &Path, runner: &Path, _: &'a File) -> Box<dyn Wine + 'a> {
        Box::new(NoWine {
            prefix: prefix.into(),
            runner: runner.into(),
            env: Env::default(),
        })
    }
    fn ensure_idle(&self, prefix: &Path) -> fenix_patch::Result<()> {
        if self.busy.lock().unwrap().as_deref() == Some(prefix) {
            Err(fenix_patch::Error::patch("synthetic other game running"))
        } else {
            Ok(())
        }
    }
    fn prepare_framework(
        &self,
        _: &dyn Wine,
        _: &Path,
        _: Progress<'_>,
        _: &str,
    ) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn prepare_desktop_runtime(
        &self,
        _: &dyn Wine,
        _: &Path,
        _: Progress<'_>,
    ) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn prepare_geometry(
        &self,
        _: &dyn Wine,
        _: &Path,
        _: &Path,
        _: Progress<'_>,
    ) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn graphics_and_fonts(&self, _: &Path, _: &Path, _: &dyn Wine) -> fenix_patch::Result<()> {
        Ok(())
    }
    fn proton_setup(
        &self,
        _: &Path,
        compat: &Path,
        _: &steam::Target,
        _: &File,
    ) -> fenix_patch::Result<()> {
        put(&compat.join("version"), "tool version");
        Ok(())
    }
    fn rename(&self, from: &Path, to: &Path) -> fenix_patch::Result<()> {
        let count = self.renames.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_rename.load(Ordering::SeqCst) == count {
            return Err(fenix_patch::Error::os_text("synthetic power loss"));
        }
        fsx::rename(from, to)
    }
}
#[test]
fn install_select_configure_update_restore_preserves_aircraft_and_original_proton() {
    let _guard = serial();
    let fx = Fixture::new();
    let item = fx.item(Game::Msfs2024);
    let aircraft = fx
        .settings(&item)
        .join("Packages/Community/aircraft/layout.json");
    let inode = fs::metadata(&aircraft).unwrap().ino();
    fx.action(&item, "install").unwrap();
    let state = fx.state(&item);
    fx.action(&item, "install").unwrap();
    assert_eq!(fx.state(&item), state);
    assert_eq!(fs::metadata(&aircraft).unwrap().ino(), inode);
    assert_eq!(
        fs::read_to_string(fx.proton.join("files/bin/wineserver")).unwrap(),
        "old server"
    );
    fx.action(&item, "select").unwrap();
    let config = fs::read_to_string(fx.steam_root.join("config/config.vdf")).unwrap();
    assert!(config.contains("preserve me") && config.contains("other-tool"));
    assert_eq!(
        steam::selected_tool(&item.target().unwrap()).as_deref(),
        Some("Proton-Fenix-A320-fixture")
    );
    put(
        &item
            .prefix()
            .join(fenix_patch::core::PROGRAM)
            .join("Fenix.exe"),
        "MZ",
    );
    put(
        &item
            .prefix()
            .join(fenix_patch::core::CONFIG)
            .join("fenixConfig.xml"),
        "<config/>",
    );
    put(
        &item
            .prefix()
            .join(fenix_patch::core::CONFIG)
            .join("persistancy.xml"),
        "<state/>",
    );
    fx.action(&item, "configure").unwrap();
    assert_eq!(fx.state(&item)["configured"], true);
    fx.release();
    fx.action(&item, "install").unwrap();
    assert_eq!(fx.state(&item)["version"], "0.1.2");
    assert_eq!(fs::metadata(&aircraft).unwrap().ino(), inode);
    put(&item.prefix().join("new-user-state"), "retain this");
    fx.action(&item, "restore").unwrap();
    assert_eq!(
        fs::read_to_string(item.prefix().join("system.reg")).unwrap(),
        "original registry"
    );
    assert_eq!(fs::metadata(&aircraft).unwrap().ino(), inode);
    assert!(!fx.tool().exists());
    assert_eq!(steam::selected_tool(&item.target().unwrap()), None);
    assert!(
        fs::read_dir(item.target().unwrap().state())
            .unwrap()
            .flatten()
            .any(|e| e.path().join("pfx/new-user-state").is_file())
    );
}
#[test]
fn both_simulators_share_one_tool_and_a_busy_user_blocks_its_update() {
    let _guard = serial();
    let fx = Fixture::new();
    let first = fx.item(Game::Msfs2024);
    let second = fx.item(Game::Msfs2020);
    fx.action(&first, "install").unwrap();
    fx.action(&second, "install").unwrap();
    assert_eq!(fx.state(&second)["tool_created"], false);
    fx.release();
    *fx.busy.lock().unwrap() = Some(second.prefix());
    assert!(fx.action(&first, "install").is_err());
    *fx.busy.lock().unwrap() = None;
    fx.action(&first, "install").unwrap();
    fx.action(&second, "install").unwrap();
    fx.action(&first, "restore").unwrap();
    assert!(fx.tool().is_dir());
    steam::verify_installed(&fx, &second.target().unwrap(), &fx.state(&second)).unwrap();
    fx.action(&second, "restore").unwrap();
    assert!(!fx.tool().exists());
}
#[test]
fn each_commit_interruption_recovers_without_losing_the_original_profile() {
    let _guard = serial();
    let fx = Fixture::new();
    let item = fx.item(Game::Msfs2024);
    for fail in 1..=3 {
        fx.renames.store(0, Ordering::SeqCst);
        fx.fail_rename.store(fail, Ordering::SeqCst);
        assert!(fx.action(&item, "install").is_err(), "step {fail}");
        fx.fail_rename.store(0, Ordering::SeqCst);
        fx.action(&item, "restore").unwrap();
        assert_eq!(
            fs::read_to_string(item.prefix().join("system.reg")).unwrap(),
            "original registry"
        );
        assert_eq!(
            fs::read_to_string(
                fx.settings(&item)
                    .join("Packages/Community/aircraft/layout.json")
            )
            .unwrap(),
            "large simulator content"
        );
        assert!(!fx.tool().exists());
    }
}

#[test]
fn an_observed_steam_flight_ends_only_its_own_fenix_companions() {
    use std::{
        os::unix::process::CommandExt,
        process::Command,
        time::{Duration, Instant},
    };
    let _guard = serial();
    let fx = Fixture::new();
    let item = fx.item(Game::Msfs2024);
    fx.action(&item, "install").unwrap();
    fx.action(&item, "select").unwrap();
    let app = flightdeck::backend::Launcher::new(fx._temp.path().join("launcher"), None).unwrap();
    app.lock()
        .installations
        .steam_roots
        .push(flightdeck::installations::SteamRoot {
            client: Client::Native,
            path: fx.steam_root.clone(),
        });
    flightdeck::installations::select(
        &app,
        &j!({"installation_id":item.id,"binding":item.context()}),
    )
    .unwrap();
    // A retained release descriptor has the same bounded shape as a real
    // independently installed patch; all synthetic hashes still get verified.
    let mut manifest = fx.lock.lock().unwrap().clone();
    let variant = manifest["variants"]["fixture"].as_object().unwrap().clone();
    manifest.as_object_mut().unwrap().extend(variant);
    manifest["repository"] = j!(flightdeck::fenix_update::PROJECT);
    manifest["prefix_files"] = j!({"drive_c/windows/system32/ntdll.dll":manifest["files"]["files/lib/wine/x86_64-windows/ntdll.dll"]});
    let private = app.state_dir.join("steam").join(&item.id).join("private");
    flightdeck::files::private_dir(&private).unwrap();
    flightdeck::files::atomic_json(
        &private.join("fenix-patch-release.json"),
        &j!({"version":"0.1.1","bundle_manifest":manifest,"bundle_release":{}}),
    )
    .unwrap();
    let mut game = Command::new("/bin/sleep")
        .arg0("FlightSimulator2024.exe")
        .arg("30")
        .env("WINEPREFIX", item.prefix())
        .env("WINE", fx.tool().join("files/bin/wine"))
        .spawn()
        .unwrap();
    let mut companion = Command::new("/bin/sleep")
        .arg0("FenixSystem.exe")
        .arg("30")
        .env("WINEPREFIX", item.prefix())
        .spawn()
        .unwrap();
    let other = fx._temp.path().join("unrelated-prefix");
    fs::create_dir(&other).unwrap();
    let mut unrelated = Command::new("/bin/sleep")
        .arg0("FenixSystem.exe")
        .arg("30")
        .env("WINEPREFIX", &other)
        .spawn()
        .unwrap();
    assert_eq!(app.status()["game"]["state"], "external");
    game.kill().unwrap();
    game.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while companion.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "observed Steam session left its Fenix companion running"
        );
        let _ = app.status();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    while app.status()["game"]["state"] != "stopped" {
        assert!(
            Instant::now() < deadline,
            "companion cleanup did not finish"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
