// SPDX-License-Identifier: MIT
//! Official Orbx Central in the existing selected MSFS profile and runner.
//! Never fabricate another simulator's settings or copy account credentials.
mod archive;
use crate::{
    Error, Result, addon_apps,
    addon_apps::Profile,
    backend::{Context, Launcher, string},
    bootstrap,
    error::require,
    fenix_setup, files,
    games::Game,
    navigraph, runtime, transaction as tx, wine_processes,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
const PROGRAM: &str = "drive_c/Program Files/Orbx Central";
const EXE: &str = "Orbx Central.exe";
const WORKER: &str = "saturn-win/OrbxCentralWorker.exe";
const RECEIPT: &str = ".flightdeck-orbx.json";
const JOURNAL: &str = "private/orbx-install.json";
pub const PROCESSES: &[&str] = &["orbx central.exe", "orbxcentralworker.exe"];
const PORT: u16 = 59483;
const OTHER_INSTANCE: &str = "Die lokale Orbx-Verbindung wird bereits verwendet. Schließe andere Orbx-Central-Instanzen und versuche es erneut.";
fn lock() -> Result<Value> {
    Ok(serde_json::from_str(include_str!(
        "../compat/orbx.lock.json"
    ))?)
}
fn contained(path: &Path, prefix: &Path) -> Result<()> {
    require(
        path.canonicalize()?.starts_with(prefix.canonicalize()?),
        "Der Orbx-Ordner muss im ausgewählten Windows-Profil liegen.",
    )
}
fn complete(path: &Path, prefix: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    contained(path, prefix)?;
    for name in [EXE, "resources/app.asar", WORKER] {
        let file = path.join(name);
        if !file.is_file() {
            return Ok(false);
        }
        contained(&file, prefix)?;
        require(
            files::open_at(rustix::fs::CWD, &file, false, false)?
                .metadata()?
                .len()
                <= 512 * 1024 * 1024,
            "Ungültige Orbx-Programmdatei.",
        )?;
    }
    Ok(true)
}
pub fn manager(prefix: &Path) -> Result<Option<PathBuf>> {
    let mut candidates = vec![
        prefix.join(PROGRAM),
        prefix.join("drive_c/Program Files (x86)/Orbx Central"),
    ];
    if prefix.join("drive_c/users").is_dir() {
        for user in fenix_setup::user_folders(prefix)? {
            candidates.push(user.join("AppData/Local/Programs/Orbx Central"));
            candidates.push(user.join("AppData/Local/Programs/orbx-central"));
        }
    }
    let mut found = BTreeSet::new();
    for path in candidates {
        if complete(&path, prefix)? {
            found.insert(path.canonicalize()?);
        }
    }
    require(
        found.len() <= 1,
        "Orbx Central wurde mehrfach in diesem Windows-Profil gefunden.",
    )?;
    Ok(found.into_iter().next())
}
fn windows_path(path: &Path, prefix: &Path) -> Result<String> {
    let base = prefix.join("drive_c").canonicalize()?;
    let path = path.canonicalize()?;
    let relative = path
        .strip_prefix(base)
        .map_err(|_| Error::Invalid("Ungültiger Orbx-Simulatorpfad."))?;
    Ok(format!(
        "C:\\{}",
        relative.to_string_lossy().replace('/', "\\")
    ))
}
fn config(path: &Path) -> Result<Value> {
    let value = if files::exists(path) {
        serde_json::from_slice(&files::read(path, 2 * 1024 * 1024)?)?
    } else {
        json!({})
    };
    require(value.is_object(), "Ungültige Orbx-Einstellungen.")?;
    Ok(value)
}
fn save_config(root: &Path, path: &Path, previous: &Value, next: &Value) -> Result<()> {
    if previous == next {
        return Ok(());
    }
    if files::exists(path) {
        files::atomic(
            &root.join("private").join(format!(
                "orbx-config-{}.json",
                uuid::Uuid::new_v4().simple()
            )),
            &files::read(path, 2 * 1024 * 1024)?,
        )?;
    }
    files::private_dir(
        path.parent()
            .ok_or(Error::Invalid("Ungültiger Orbx-Einstellungspfad."))?,
    )?;
    files::atomic_json(path, next)
}
fn settings(profile: &Profile) -> Result<(PathBuf, PathBuf, Game)> {
    let prefix = &profile.prefix;
    let folder = navigraph::settings_folder_for(profile)?;
    let users = prefix.join("drive_c/users");
    let relative = folder
        .strip_prefix(&users)
        .map_err(|_| Error::Invalid("Ungültiger Orbx-Simulatorpfad."))?;
    let user = relative
        .components()
        .next()
        .ok_or(Error::Invalid("Ungültiger Orbx-Simulatorpfad."))?;
    Ok((
        users.join(user.as_os_str()).join("AppData/Roaming/Orbx"),
        folder,
        profile.game,
    ))
}
fn override_key(game: Game) -> &'static str {
    match game {
        Game::Msfs2020 => "msfs",
        Game::Msfs2024 => "msfs24",
    }
}
fn configured(profile: &Profile) -> Result<bool> {
    let (folder, sim, game) = settings(profile)?;
    let value = config(&folder.join("simfinder/path-overrides.json"))?;
    Ok(value[override_key(game)] == windows_path(&sim.join("UserCfg.opt"), &profile.prefix)?)
}
fn configure(profile: &Profile) -> Result<()> {
    let root = &profile.root;
    let (folder, sim, game) = settings(profile)?;
    let path = folder.join("simfinder/path-overrides.json");
    let before = config(&path)?;
    let mut after = before.clone();
    after[override_key(game)] = json!(windows_path(&sim.join("UserCfg.opt"), &profile.prefix)?);
    save_config(root, &path, &before, &after)?;
    let path = folder.join("Central/central.json");
    let before = config(&path)?;
    let mut after = before.clone();
    if after["general"].is_null() {
        after["general"] = json!({});
    }
    require(
        after["general"].is_object(),
        "Ungültige Orbx-Einstellungen.",
    )?;
    // Use the pinned complete application. The NSIS web updater's close check
    // fails under Wine even when Central is no longer running.
    after["general"]["updateCentralOnLaunch"] = json!(false);
    // Wine may display an undocked white tray host. Closing Central must quit
    // instead of hiding its only usable window in that host. Central reads
    // these settings on startup; all configuration actions require an idle profile.
    after["general"]["showInTray"] = json!(false);
    after["general"]["startOnLaunch"] = json!(false);
    save_config(root, &path, &before, &after)
}
fn worker_available() -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], PORT)),
        Duration::from_millis(150),
    )
    .is_err()
}
fn safe_stage(root: &Path, name: &str) -> Result<PathBuf> {
    require(
        name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit()),
        "Ungültige Orbx-Einrichtungssicherung.",
    )?;
    Ok(root.join("private").join(format!("orbx-stage-{name}")))
}
fn stage(profile: &Profile, id: &str) -> Result<PathBuf> {
    let xbox = safe_stage(&profile.root, id)?;
    Ok(if profile.item.is_some() {
        profile
            .prefix
            .parent()
            .ok_or(Error::Invalid("Ungültiges Steam-Profil."))?
            .join(format!(".flightdeck-orbx-stage-{id}"))
    } else {
        xbox
    })
}
fn program_backup(profile: &Profile, id: &str) -> Result<PathBuf> {
    safe_stage(&profile.root, id)?;
    Ok(if profile.item.is_some() {
        profile
            .prefix
            .parent()
            .ok_or(Error::Invalid("Ungültiges Steam-Profil."))?
            .join(format!(".flightdeck-orbx-before-{id}"))
    } else {
        profile
            .root
            .join("private")
            .join(format!("orbx-program-backup-{id}"))
    })
}
fn recover(profile: &Profile) -> Result<()> {
    let root = &profile.root;
    let journal = root.join(JOURNAL);
    if !files::exists(&journal) {
        return Ok(());
    }
    let value = runtime::value(&journal)?;
    require(
        value["format"] == 1,
        "Ungültige Orbx-Einrichtungssicherung.",
    )?;
    let staged = stage(profile, string(&value, "id")?)?;
    let prefix = &profile.prefix;
    let target = prefix.join(PROGRAM);
    let backup = program_backup(profile, string(&value, "id")?)?;
    if !files::exists(&target)
        && files::exists(&backup)
        && crate::fenix::identity(prefix).is_ok_and(|id| value["prefix_id"] == json!(id))
    {
        files::directory(&backup, false)?;
        tx::publish(&backup, &target)?;
    }
    // Staging contains program files only and is outside the simulator profile.
    // It is never a game/profile backup. Do not follow a substituted directory.
    if files::exists(&staged) {
        files::directory(&staged, true)?;
        fs::remove_dir_all(&staged)?;
    }
    fs::remove_file(&journal)?;
    Ok(())
}
fn managed_files_match(
    directory: &Path,
    spec: &Value,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<bool> {
    let Ok(receipt) = runtime::value(&directory.join(RECEIPT)) else {
        // An interrupted or damaged managed receipt is repairable. Retain the
        // previous program directory before publishing a freshly verified one.
        return Ok(false);
    };
    if receipt["format"] != 1 || receipt["archive_sha256"] != spec["archive_sha256"] {
        return Ok(false);
    }
    let Some(files) = receipt["files"]
        .as_object()
        .filter(|m| !m.is_empty() && m.len() <= 12_000)
    else {
        return Ok(false);
    };
    for (name, expected) in files {
        tx::interrupted(cancel)?;
        let relative = Path::new(name);
        if !relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            return Ok(false);
        }
        let path = directory.join(relative);
        match tx::owned_file(directory, &path).and_then(crate::files::digest) {
            Ok(actual) if expected.as_str() == Some(actual.as_str()) => (),
            _ => return Ok(false),
        }
    }
    Ok(true)
}
fn install(ctx: &Context) -> Result<()> {
    let profile = Profile::selected(&ctx.launcher)?;
    let root = &profile.root;
    let prefix = profile.prefix.clone();
    recover(&profile)?;
    let spec = lock()?;
    if let Some(directory) = manager(&prefix)?
        && (!files::exists(&directory.join(RECEIPT))
            || managed_files_match(&directory, &spec, &ctx.cancel)?)
    {
        configure(&profile)?;
        return Ok(());
    }
    let url = string(&spec, "archive_url")?;
    let hash = string(&spec, "archive_sha256")?;
    let path = ctx
        .launcher
        .state_dir
        .join("addon-downloads/orbx")
        .join(format!("{}.7z", string(&spec, "version")?));
    ctx.progress("Das offizielle Orbx-Programmpaket wird heruntergeladen und geprüft …");
    let package = bootstrap::download(url, hash, &path, &ctx.cancel, |transfer| {
        ctx.update(json!({"transfer":transfer}))
    })?;
    require(
        tx::free_bytes(&prefix)? > 3 * 1024 * 1024 * 1024,
        "Für die Orbx-Einrichtung fehlen mindestens 3 GiB freier Speicherplatz.",
    )?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let staged = stage(&profile, &id)?;
    files::atomic_json(
        &root.join(JOURNAL),
        &json!({"format":1,"id":id,"state":"preparing","prefix_id":crate::fenix::identity(&prefix)?}),
    )?;
    ctx.progress("Orbx Central wird im vorhandenen Windows-Profil eingerichtet …");
    let result = (|| {
        let hashes = archive::extract(&package, &staged, &ctx.cancel)?;
        for required in [EXE, "resources/app.asar", WORKER] {
            require(
                hashes.contains_key(required),
                "Das Orbx-Programmpaket ist unvollständig.",
            )?;
        }
        files::atomic_json(
            &staged.join(RECEIPT),
            &json!({"format":1,"version":spec["version"],"archive_sha256":hash,"files":hashes}),
        )?;
        let target = prefix.join(PROGRAM);
        files::private_dir(
            target
                .parent()
                .ok_or(Error::Invalid("Ungültiger Orbx-Programmpfad."))?,
        )?;
        profile.current(&ctx.launcher)?;
        wine_processes::idle(&prefix)?;
        ctx.begin_commit("committing", "Orbx Central wird aktiviert …")?;
        // Retain a complete previous program directory, including unknown files.
        // Orbx account data and scenery are outside this directory.
        if files::exists(&target) {
            files::directory(&target, false)?;
            let backup = program_backup(&profile, &id)?;
            tx::publish(&target, &backup)?;
        }
        tx::publish(&staged, &target)?;
        fs::remove_file(root.join(JOURNAL))?;
        configure(&profile)?;
        Ok(())
    })();
    if result.is_err() && files::exists(&staged) {
        let _ = fs::remove_dir_all(&staged);
    }
    result
}
fn open(ctx: &Context) -> Result<()> {
    let profile = Profile::selected(&ctx.launcher)?;
    let prefix = profile.prefix.clone();
    let directory = manager(&prefix)?.ok_or(Error::Invalid("Installiere zuerst Orbx Central."))?;
    // Central uses a fixed worker port even across separate Wine profiles.
    // Never stop another profile merely to make that port available.
    let _worker = files::Lease::acquire(&ctx.launcher.state_dir.join("orbx-worker.lock"), true)?;
    require(worker_available(), OTHER_INSTANCE)?;
    configure(&profile)?;
    let wine = profile.wine(ctx, "orbx-central.log")?;
    fenix_setup::desktop_fonts(&wine).map_err(|error|if matches!(error,Error::Cancelled){error}else{Error::Invalid("Die Schriftvorbereitung für Orbx ist fehlgeschlagen. Details stehen im privaten Orbx-Protokoll.")})?;
    wine.reg(
        r"HKCU\Software\Wine\Explorer",
        "ShowSystray",
        "0",
        "REG_DWORD",
    )?;
    ctx.progress("Melde dich in Orbx Central an und verwalte deine Szenerien. Schließe Orbx danach vollständig.");
    addon_apps::interactive(
        ctx,
        &profile,
        &wine,
        &[
            &directory.join(EXE),
            Path::new("--disable-gpu"),
            Path::new("--disable-features=HideCursorWhileTyping"),
        ],
        PROCESSES,
        "Orbx Central wurde unerwartet beendet. Details stehen im privaten Orbx-Protokoll.",
    )
}
pub fn snapshot(app: &Launcher) -> Value {
    let selected = Profile::selected(app);
    let context = Launcher::context(&app.lock());
    let (busy, playing, closing, job) = selected
        .as_ref()
        .map(|p| addon_apps::job(app, "orbx", p))
        .unwrap_or((false, false, false, Value::Null));
    let mut value = json!({"state":"unavailable","runtime_path":context,"installed":false,"configured":false,"settings_found":false,"running":false,"worker_busy":false,"idle":false,"can_change":false,"can_stop":false,"verified_in_simulator":false,"job":job,"message":""});
    if let Ok(profile) = &selected {
        value["profile_token"] = json!(profile.revision);
        value["source"] = json!(if profile.item.is_some() {
            "steam"
        } else {
            "xbox"
        });
        value["installation_id"] = json!(profile.item.as_ref().map(|i| &i.id));
        let result = (|| -> Result<()> {
            profile.validate(false)?;
            let prefix = &profile.prefix;
            let directory = manager(prefix)?;
            let processes = wine_processes::Processes::new(prefix)?;
            let running = processes.any(Some(PROCESSES));
            let idle = wine_processes::idle(prefix).is_ok();
            value["state"] = json!("available");
            value["game"] = json!(profile.game.id());
            value["installed"] = json!(directory.is_some());
            value["configured"] = json!(configured(profile).unwrap_or(false));
            value["settings_found"] = json!(settings(profile).is_ok());
            value["idle"] = json!(idle);
            value["running"] = json!(running);
            value["worker_busy"] = json!(!running && !worker_available());
            value["can_change"] = json!(idle && !busy && !playing && !closing);
            let interactive =
                job["state"] == "running" && job["operation"] == "open" && job["stopping"] != true;
            value["can_stop"] = json!(
                running
                    && !playing
                    && !closing
                    && (interactive || !busy)
                    && !processes.any(Some(wine_processes::GAMES))
            );
            if let Some(directory) = directory
                && let Ok(receipt) = runtime::value(&directory.join(RECEIPT))
            {
                value["version"] = receipt["version"].clone();
            }
            Ok(())
        })();
        if let Err(error) = result {
            value["message"] = json!(error.to_string());
        }
    }
    if let Err(error) = selected {
        value["message"] = json!(error.to_string());
    }
    value
}
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    require(
        ["install", "configure", "open", "stop"].contains(&operation),
        "Unbekannter Orbx-Vorgang.",
    )?;
    let profile = Profile::selected(app)?;
    require(
        profile.context == string(data, "runtime_path")?,
        "Die ausgewählte Runtime hat sich geändert. Orbx-Status neu laden.",
    )?;
    if profile.item.is_some() {
        require(
            data["profile_token"] == profile.revision,
            "Die Installation oder Proton-Auswahl hat sich geändert. Add-on-Status neu laden.",
        )?;
    }
    if operation == "stop" {
        require(
            snapshot(app)["can_stop"] == true,
            "Beende MSFS und laufende Installationen, bevor du Orbx schließt.",
        )?;
        let mut state = app.lock();
        Launcher::open(&state)?;
        if state.active.as_ref().is_some_and(|a| a.kind == "orbx")
            && state
                .jobs
                .get("orbx")
                .is_some_and(|j| j["state"] == "running" && j["operation"] == "open")
        {
            if let Some(active) = &state.active {
                active.pause.store(true, Ordering::Relaxed);
            }
            if let Some(job) = state.jobs.get_mut("orbx") {
                job["stopping"] = json!(true);
            }
            return Ok(json!({"ok":true}));
        }
    } else {
        profile.validate(false)?;
    }
    let operation = operation.to_string();
    app.start_job("orbx",&operation.clone(),profile.item.is_none(),move|ctx|{
        profile.current(&ctx.launcher)?;
        let _held=profile.lease()?;
        if operation!="stop"{profile.validate(false)?;wine_processes::idle(&profile.prefix)?;}
        match operation.as_str(){"install"=>install(ctx)?,"configure"=>configure(&profile)?,"open"=>open(ctx)?,_=>profile.stop(PROCESSES)?}
        Ok(json!({"state":"complete","message":match operation.as_str(){"install"=>"Orbx Central ist im vorhandenen Windows-Profil eingerichtet.","configure"=>"Orbx Central verwendet jetzt die Einstellungen dieser MSFS-Installation.",_=>"Orbx Central wurde geschlossen."},"stopping":false}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_config_preserves_simulator_settings_and_other_edition() {
        let root = tempfile::tempdir().expect("valid Orbx test fixture");
        let private = root.path().join("private");
        files::private_dir(&private).expect("valid Orbx test fixture");
        let user = root
            .path()
            .join("local/msfs-prefix/drive_c/users/steamuser");
        let sim = user.join("AppData/Roaming/Microsoft Flight Simulator 2024");
        files::private_dir(&sim).expect("valid Orbx test fixture");
        let original = b"InstalledPackagesPath \"C:\\Packages\"\r\nOtherSetting 1\r\n";
        fs::write(sim.join("UserCfg.opt"), original).expect("valid Orbx test fixture");
        let settings = user.join("AppData/Roaming/Orbx/simfinder/path-overrides.json");
        files::private_dir(settings.parent().expect("valid Orbx test fixture"))
            .expect("valid Orbx test fixture");
        files::atomic_json(
            &settings,
            &json!({"msfs":"C:\\original\\UserCfg.opt","unrelated":"preserved"}),
        )
        .expect("valid Orbx test fixture");
        configure(&Profile::xbox(root.path()).expect("Xbox fixture"))
            .expect("valid Orbx test fixture");
        let changed: Value = runtime::value(&settings).expect("valid Orbx test fixture");
        assert_eq!(changed["msfs"], "C:\\original\\UserCfg.opt");
        assert_eq!(changed["unrelated"], "preserved");
        assert!(
            changed["msfs24"]
                .as_str()
                .expect("valid Orbx test fixture")
                .ends_with("Microsoft Flight Simulator 2024\\UserCfg.opt")
        );
        assert_eq!(
            fs::read(sim.join("UserCfg.opt")).expect("valid Orbx test fixture"),
            original
        );
        let central = runtime::value(&user.join("AppData/Roaming/Orbx/Central/central.json"))
            .expect("valid Orbx test fixture");
        assert_eq!(central["general"]["showInTray"], false);
        assert_eq!(central["general"]["startOnLaunch"], false);
        assert_eq!(central["general"]["updateCentralOnLaunch"], false);
        let backups = fs::read_dir(&private)
            .expect("valid Orbx test fixture")
            .count();
        configure(&Profile::xbox(root.path()).expect("Xbox fixture"))
            .expect("valid Orbx test fixture");
        assert_eq!(
            fs::read_dir(&private)
                .expect("valid Orbx test fixture")
                .count(),
            backups
        );
    }
    #[test]
    fn interrupted_publication_restores_only_the_original_profiles_program() {
        let root = tempfile::tempdir().expect("valid Orbx test fixture");
        files::private_dir(&root.path().join("private")).expect("valid Orbx test fixture");
        let prefix = root.path().join("local/msfs-prefix");
        files::private_dir(&prefix.join("drive_c/Program Files")).expect("valid Orbx test fixture");
        let id = "b".repeat(32);
        let backup = root
            .path()
            .join("private")
            .join(format!("orbx-program-backup-{id}"));
        files::private_dir(&backup).expect("valid Orbx test fixture");
        fs::write(backup.join("retained"), b"old program").expect("valid Orbx test fixture");
        files::atomic_json(
            &root.path().join(JOURNAL),
            &json!({"format":1,"id":id,"prefix_id":crate::fenix::identity(&prefix).expect("valid Orbx test fixture")}),
        )
        .expect("valid Orbx test fixture");
        recover(&Profile::xbox(root.path()).expect("Xbox fixture"))
            .expect("valid Orbx test fixture");
        assert_eq!(
            fs::read(prefix.join(PROGRAM).join("retained")).expect("valid Orbx test fixture"),
            b"old program"
        );
        // A record from another prefix must never restore files into this one.
        fs::rename(prefix.join(PROGRAM), &backup).expect("valid Orbx test fixture");
        files::atomic_json(
            &root.path().join(JOURNAL),
            &json!({"format":1,"id":id,"prefix_id":[0,0]}),
        )
        .expect("valid Orbx test fixture");
        recover(&Profile::xbox(root.path()).expect("Xbox fixture"))
            .expect("valid Orbx test fixture");
        assert!(backup.is_dir());
        assert!(!prefix.join(PROGRAM).exists());
    }
    #[test]
    fn interrupted_stage_never_becomes_a_profile_backup() {
        let root = tempfile::tempdir().expect("valid Orbx test fixture");
        files::private_dir(&root.path().join("private")).expect("valid Orbx test fixture");
        assert!(safe_stage(root.path(), "../game").is_err());
        let stage = safe_stage(root.path(), &"a".repeat(32)).expect("valid Orbx test fixture");
        files::private_dir(&stage).expect("valid Orbx test fixture");
        fs::write(stage.join("partial"), b"temporary").expect("valid Orbx test fixture");
        files::atomic_json(
            &root.path().join(JOURNAL),
            &json!({"format":1,"id":"a".repeat(32),"state":"preparing"}),
        )
        .expect("valid Orbx test fixture");
        recover(&Profile::xbox(root.path()).expect("Xbox fixture"))
            .expect("valid Orbx test fixture");
        assert!(!stage.exists());
        assert!(!root.path().join(JOURNAL).exists());
    }
    #[test]
    #[ignore = "explicit real-profile setup: installs official Orbx into the selected existing profile"]
    fn real_existing_profile_install_uses_no_profile_or_runner_copy() {
        let root = PathBuf::from(
            std::env::var_os("FLIGHTDECK_TEST_ORBX_RUNTIME").expect("explicit existing runtime"),
        );
        let prefix = root.join("local/msfs-prefix");
        let before = crate::fenix::identity(&prefix).expect("valid Orbx test fixture");
        let runner = fs::read_link(root.join("runner")).expect("valid Orbx test fixture");
        let settings_before = navigraph::settings_folder(&root)
            .expect("valid Orbx test fixture")
            .join("UserCfg.opt");
        let user_cfg = fs::read(&settings_before).expect("valid Orbx test fixture");
        let state = tempfile::tempdir().expect("valid Orbx test fixture");
        let app = Launcher::new(
            state.path().to_path_buf(),
            Some(root.to_str().expect("valid Orbx test fixture")),
        )
        .expect("valid Orbx test fixture");
        let ctx = app
            .reserve("orbx", "install", true)
            .expect("valid Orbx test fixture");
        addon_apps::validate(&root).expect("valid Orbx test fixture");
        wine_processes::idle(&prefix).expect("valid Orbx test fixture");
        let result = install(&ctx);
        match result {
            Ok(()) => app.finish(&ctx, Ok(json!({"state":"complete"})), false),
            Err(error) => {
                app.finish(&ctx, Err(Error::Invalid("Orbx probe failed")), false);
                panic!("{error}");
            }
        }
        assert_eq!(
            crate::fenix::identity(&prefix).expect("valid Orbx test fixture"),
            before
        );
        assert_eq!(
            fs::read_link(root.join("runner")).expect("valid Orbx test fixture"),
            runner
        );
        assert_eq!(
            fs::read(&settings_before).expect("valid Orbx test fixture"),
            user_cfg
        );
        assert!(manager(&prefix).expect("valid Orbx test fixture").is_some());
        assert!(
            configured(&Profile::xbox(&root).expect("Xbox fixture"))
                .expect("valid Orbx test fixture")
        );
        println!(
            "Existing prefix, selected runner and original UserCfg.opt preserved; official Orbx program and selected-edition detection installed."
        );
    }
    #[test]
    #[ignore = "explicit real-profile launch: opens installed Orbx and closes only its named processes"]
    fn real_existing_profile_launch_and_scoped_close() {
        let root = PathBuf::from(
            std::env::var_os("FLIGHTDECK_TEST_ORBX_RUNTIME").expect("explicit existing runtime"),
        );
        let prefix = root.join("local/msfs-prefix");
        let before = crate::fenix::identity(&prefix).expect("valid Orbx test fixture");
        let state = tempfile::tempdir().expect("valid Orbx test fixture");
        let app = Launcher::new(
            state.path().to_path_buf(),
            Some(root.to_str().expect("valid Orbx test fixture")),
        )
        .expect("valid Orbx test fixture");
        let ctx = app
            .reserve("orbx", "open", true)
            .expect("valid Orbx test fixture");
        addon_apps::validate(&root).expect("valid Orbx test fixture");
        wine_processes::idle(&prefix).expect("valid Orbx test fixture");
        assert!(
            worker_available(),
            "Close the existing Orbx worker before this explicit probe."
        );
        let mut ready = false;
        let result = std::thread::scope(|scope| {
            let task = scope.spawn(|| open(&ctx));
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while std::time::Instant::now() < deadline && !task.is_finished() {
                if !worker_available()
                    && wine_processes::Processes::new(&prefix)
                        .expect("valid Orbx test fixture")
                        .any(Some(PROCESSES))
                {
                    ready = true;
                    std::thread::sleep(Duration::from_secs(5));
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            ctx.pause.store(true, Ordering::Relaxed);
            task.join().expect("valid Orbx test fixture")
        });
        app.finish(&ctx, result.map(|_| json!({"state":"complete"})), false);
        assert!(
            ready,
            "Orbx did not reach its worker in the selected profile."
        );
        assert!(
            !wine_processes::Processes::new(&prefix)
                .expect("valid Orbx test fixture")
                .any(Some(PROCESSES))
        );
        assert!(worker_available());
        assert_eq!(
            crate::fenix::identity(&prefix).expect("valid Orbx test fixture"),
            before
        );
        println!(
            "Orbx opened in the existing game profile; worker ready and scoped close released its processes and port. Sign-in and scenery installation are not covered."
        );
    }
}
