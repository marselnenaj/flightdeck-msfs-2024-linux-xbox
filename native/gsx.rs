// SPDX-License-Identifier: MIT
//! Official FSDT setup in retained Wine copies; activation stays with FSDT.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    bootstrap,
    error::require,
    fenix, fenix_setup, files, framework, framework_repair,
    games::Game,
    mods, process, proton, runtime, transaction as tx,
    wine::{StagedWine, Wine},
    wine_processes,
    xml::{self, Element, Item},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
pub const MARKER: &str = "private/gsx-setup.json";
pub const STARTUP: &str = "private/gsx-startup.json";
const MANAGER_DIR: &str = "drive_c/Program Files (x86)/Addon Manager";
pub const MANAGERS: &[&str] = &[
    "couatl_updater.exe",
    "couatl_updater2.exe",
    "qlmlicensewizard.exe",
];
pub const COMPANIONS: &[&str] = &[
    "couatl64_boot.exe",
    "couatl64_msfs.exe",
    "couatl64_msfs2024.exe",
];
const INCOMPLETE: &str = "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.";
fn owned(path: &Path, root: &Path) -> Result<()> {
    files::directory(path, false)?;
    require(
        path.canonicalize()?.starts_with(root.canonicalize()?),
        "Der GSX-Ordner muss innerhalb des gewählten Windows-Profils liegen.",
    )
}
fn registry(prefix: &Path, key: &str, name: &str) -> Result<Option<String>> {
    let path = prefix.join("user.reg");
    if !files::exists(&path) {
        return Ok(None);
    }
    let raw = files::read(&path, 32 * 1024 * 1024)?;
    let text = String::from_utf8_lossy(&raw);
    let mut inside = false;
    let mut found = None;
    let section = format!("[{key}]").to_ascii_lowercase();
    for line in text.lines() {
        if line.starts_with('[') {
            inside = line.to_ascii_lowercase().starts_with(&section);
        } else if inside && let Some(value) = line.strip_prefix(&format!("\"{name}\"=")) {
            require(
                found.is_none(),
                "Die FSDT-Registry enthält mehrdeutige Einstellungen.",
            )?;
            found = Some(serde_json::from_str(value)?);
        }
    }
    Ok(found)
}
pub fn manager(prefix: &Path) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    if let Some(value) = registry(prefix, r"Software\\Fsdreamteam", "root")? {
        paths.push(mods::configured_path(&value, prefix)?);
    }
    paths.extend([
        prefix.join(MANAGER_DIR),
        prefix.join("drive_c/Program Files/Addon Manager"),
    ]);
    for path in paths {
        if path.exists() {
            owned(&path, &prefix.join("drive_c"))?;
            let exe = path.join("Couatl_Updater.exe");
            if files::exists(&exe) {
                require(
                    files::open_at(rustix::fs::CWD, &exe, false, false)?
                        .metadata()?
                        .len()
                        <= 180 * 1024 * 1024,
                    "Invalid FSDT manager executable.",
                )?;
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}
pub fn marker(root: &Path) -> Result<Option<Value>> {
    let path = root.join(MARKER);
    if !files::exists(&path) {
        return Ok(None);
    }
    let data = runtime::value(&path)?;
    require(
        data["format"].as_u64() == Some(1)
            && data["id"].as_str().is_some_and(|v| {
                v.len() == 32
                    && v.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            && ["preparing", "committing", "ready"].contains(&data["state"].as_str().unwrap_or("")),
        INCOMPLETE,
    )?;
    Ok(Some(data))
}
pub fn complete(root: &Path) -> bool {
    marker(root).is_ok_and(|v| v.is_none_or(|v| v["state"] == "ready"))
}
fn override_key(name: &str) -> String {
    format!(r"HKCU\Software\Wine\AppDefaults\{name}\DllOverrides")
}
pub fn prepare(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    fenix::validate(root, false)?;
    proton::check(root)?;
    require(complete(root), INCOMPLETE)?;
    let prefix = root.join("local/msfs-prefix");
    wine_processes::idle(&prefix)?;
    let runner = root.join("runner").canonicalize()?;
    manager(&prefix)?;
    let cache = ctx.launcher.state_dir.join("gsx-downloads");
    ctx.progress("Der offizielle FSDT-Installer wird heruntergeladen und geprüft …");
    let installer = bootstrap::download(
        "https://www.fsdreamteam.com/update/FSDT_Universal_Installer.exe",
        "4d0230ffbf3d0a8d69fb23dbf280be4783a13b87fb0f721c0e3a377eea114865",
        &cache.join("FSDT_Universal_Installer.exe"),
        &ctx.cancel,
        |_| {},
    )?;
    require(
        tx::free_bytes(root)?
            > tx::prefix_size(&prefix, &ctx.cancel)?.saturating_add(1024 * 1024 * 1024),
        "Für die GSX-Vorbereitung fehlt Speicherplatz für eine Profilkopie.",
    )?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let staged = root.join("local").join(format!(".gsx-prefix-{token}"));
    let backup = root
        .join("local")
        .join(format!("msfs-prefix.before-gsx-{token}"));
    let mut state = json!({"format":1,"id":token,"state":"preparing","original_prefix_id":fenix::identity(&prefix)?,"prior":marker(root)?});
    files::atomic_json(&root.join(MARKER), &state)?;
    ctx.progress("Das Windows-Profil wird für die GSX-Einrichtung kopiert …");
    process::copy_tree(&prefix, &staged, &ctx.cancel)?;
    tx::relocate_prefix_links(&prefix, &staged, &ctx.cancel)?;
    files::directory(&staged.join("dosdevices"), false)?;
    let drive = staged.join("dosdevices/c:");
    if files::exists(&drive) {
        require(
            drive.symlink_metadata()?.is_symlink(),
            "Invalid staged C: drive.",
        )?;
        fs::remove_file(&drive)?;
    }
    symlink("../drive_c", drive)?;
    let mut wine = StagedWine::new(
        root,
        &staged,
        &runner,
        &root.join("private").join(format!("gsx-setup-{token}.log")),
        ctx,
    )?;
    wine.wine.without_fenix();
    let result = (|| {
        framework_repair::prepare(
            &mut wine,
            |name, url, hash| {
                bootstrap::download(url, hash, &cache.join(name), &ctx.cancel, |_| {})
            },
            |s| ctx.progress(s),
        )?;
        ctx.progress("Der FSDT-Installer wird im kopierten Profil eingerichtet …");
        let directory = manager(&staged)?.unwrap_or(staged.join(MANAGER_DIR));
        let windows = |path: &Path| -> Result<String> {
            Ok(format!(
                "C:\\{}",
                path.strip_prefix(staged.join("drive_c"))
                    .map_err(|_| Error::Invalid("Invalid FSDT installation folder."))?
                    .to_string_lossy()
                    .replace('/', "\\")
            ))
        };
        let mut overrides = Vec::new();
        for name in ["Couatl_Updater.exe", "Couatl_Updater2.exe"] {
            overrides.push((
                name,
                registry(
                    &staged,
                    &format!(r"Software\\Wine\\AppDefaults\\{name}\\DllOverrides"),
                    "mscoree",
                )?,
            ));
        }
        let installed = (|| {
            for (name, _) in &overrides {
                wine.wine
                    .reg(&override_key(name), "mscoree", "", "REG_SZ")?;
            }
            wine.wine.run(
                &[
                    installer.to_string_lossy().into_owned(),
                    "/VERYSILENT".into(),
                    "/SUPPRESSMSGBOXES".into(),
                    "/NORESTART".into(),
                    "/SP-".into(),
                    format!("/DIR={}", windows(&directory)?),
                ],
                None,
                &[0],
                Duration::from_secs(1800),
            )?;
            Ok::<_, Error>(())
        })();
        wine.stop()?;
        let restored = (|| {
            for (name, old) in overrides {
                if let Some(old) = old {
                    wine.wine
                        .reg(&override_key(name), "mscoree", &old, "REG_SZ")?;
                } else {
                    wine.wine.run(
                        &["reg", "delete", &override_key(name), "/v", "mscoree", "/f"],
                        None,
                        &[0, 1],
                        Duration::from_secs(180),
                    )?;
                }
            }
            Ok::<_, Error>(())
        })();
        installed.and(restored)?;
        let directory = manager(&staged)?.ok_or(Error::Invalid(
            "Der FSDT-Installer wurde nicht vollständig eingerichtet.",
        ))?;
        let assembly = directory.join("QlmLicenseLib.dll");
        require(
            files::open_at(rustix::fs::CWD, &assembly, false, false)?
                .metadata()?
                .len()
                <= 64 * 1024 * 1024,
            "Invalid FSDT licensing library.",
        )?;
        let regasm = framework::framework_path(&staged, "Framework", "RegAsm.exe")?;
        wine.wine.run(
            &[
                regasm.to_string_lossy().into_owned(),
                "/codebase".into(),
                windows(&assembly)?,
            ],
            None,
            &[0],
            Duration::from_secs(1800),
        )?;
        Ok::<_, Error>(())
    })();
    let stopped = wine.stop();
    result.and(stopped)?;
    require(
        framework::status(&staged).ready && manager(&staged)?.is_some(),
        "Der FSDT-Installer wurde nicht vollständig eingerichtet.",
    )?;
    require(
        fenix::identity(&prefix)? == state["original_prefix_id"]
            && root.join("runner").canonicalize()? == runner,
        "Die Runtime hat sich während der GSX-Einrichtung geändert.",
    )?;
    wine_processes::idle(&prefix)?;
    ctx.begin_commit(
        "publish_runtime",
        "Die geprüfte FSDT-Umgebung wird aktiviert …",
    )?;
    state["state"] = json!("committing");
    state["staged_prefix_id"] = fenix::identity(&staged)?;
    files::atomic_json(&root.join(MARKER), &state)?;
    tx::publish(&prefix, &backup)?;
    tx::publish(&staged, &prefix)?;
    state["state"] = json!("ready");
    if let Some(value) = state.as_object_mut() {
        value.remove("prior");
    }
    files::atomic_json(&root.join(MARKER), &state)
}
pub fn recover(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    fenix::validate(root, true)?;
    let state = marker(root)?.ok_or(Error::Invalid(
        "Keine unterbrochene GSX-Einrichtung vorhanden.",
    ))?;
    require(
        state["state"] != "ready",
        "Keine unterbrochene GSX-Einrichtung vorhanden.",
    )?;
    let prefix = root.join("local/msfs-prefix");
    wine_processes::idle(&prefix)?;
    let staged = root
        .join("local")
        .join(format!(".gsx-prefix-{}", string(&state, "id")?));
    let backup = root
        .join("local")
        .join(format!("msfs-prefix.before-gsx-{}", string(&state, "id")?));
    if staged.exists() {
        owned(&staged, root)?;
        require(
            !prefix.exists() || fenix::identity(&staged)? != fenix::identity(&prefix)?,
            "Invalid GSX staging profile.",
        )?;
        let wine = Wine::new(
            &staged,
            &root.join("runner"),
            &root.join("private").join(format!(
                "gsx-recovery-{}.log",
                uuid::Uuid::new_v4().simple()
            )),
            &ctx.cancel,
        )?;
        wine.stop_staged()?;
    }
    if backup.exists() {
        owned(&backup, root)?;
        require(
            fenix::identity(&backup)? == state["original_prefix_id"],
            "Die GSX-Sicherung hat sich geändert; Wiederherstellung abgebrochen.",
        )?;
        if prefix.exists() {
            owned(&prefix, root)?;
            require(
                fenix::identity(&prefix)? == state["staged_prefix_id"],
                "Die GSX-Sicherung hat sich geändert; Wiederherstellung abgebrochen.",
            )?;
        }
    } else {
        require(
            fenix::identity(&prefix)? == state["original_prefix_id"],
            "Die GSX-Sicherung hat sich geändert; Wiederherstellung abgebrochen.",
        )?;
    }
    ctx.begin_commit(
        "publish_runtime",
        "Die GSX-Vorbereitung wird zurückgesetzt …",
    )?;
    if backup.exists() {
        if prefix.exists() {
            tx::publish(
                &prefix,
                &root.join("local").join(format!(
                    "msfs-prefix.after-gsx-{}",
                    uuid::Uuid::new_v4().simple()
                )),
            )?;
        }
        tx::publish(&backup, &prefix)?;
    }
    if !state["prior"].is_null() {
        files::atomic_json(&root.join(MARKER), &state["prior"])?;
    } else {
        fs::remove_file(root.join(MARKER))?;
        files::directory(&root.join("private"), false)?.sync_all()?;
    }
    Ok(())
}
pub fn community(root: &Path) -> Result<Option<PathBuf>> {
    let (locations, limited) = mods::locations(root)?;
    if limited {
        return Ok(None);
    }
    let paths: BTreeSet<_> = locations
        .into_iter()
        .map(|(p, _)| p.canonicalize())
        .collect::<std::io::Result<_>>()?;
    if paths.len() != 1 {
        return Ok(None);
    }
    let package = paths
        .iter()
        .next()
        .ok_or(Error::Invalid("Missing Community folder."))?
        .join("fsdreamteam-gsx-pro");
    if !package.is_dir() {
        return Ok(None);
    }
    let data: Value = files::json(&package.join("manifest.json"), 128 * 1024)?;
    Ok((data.is_object()
        && data["title"].as_str().is_some_and(|v| !v.is_empty())
        && data["package_version"]
            .as_str()
            .is_some_and(|v| !v.is_empty()))
    .then_some(package))
}
pub struct Startup {
    path: PathBuf,
    tree: Element,
    index: usize,
    raw: Vec<u8>,
}
fn text(element: &Element, name: &str) -> String {
    element.child(name).map(Element::text).unwrap_or_default()
}
fn enabled(element: &Element) -> bool {
    text(element, "Disabled").trim().to_lowercase() != "true"
}
pub fn startup(root: &Path, directory: Option<&Path>) -> Result<Option<Startup>> {
    let Some(directory) = directory else {
        return Ok(None);
    };
    let directory = directory.canonicalize()?;
    let prefix = root.join("local/msfs-prefix");
    let users = fenix_setup::user_folders(&prefix)?;
    require(
        users.len() <= 16,
        "Die FSDT-Starteinstellung ist nicht eindeutig. Im FSDT-Installer aktualisieren.",
    )?;
    let family = mods::family(root)?;
    let game = Game::for_runtime(root)?;
    let mut found = None;
    for user in users {
        let mut folders = vec![user.join("AppData/Roaming").join(game.user_config())];
        if let Some(family) = &family {
            folders.push(
                user.join("AppData/Local/Packages")
                    .join(family)
                    .join("LocalCache"),
            );
        }
        for folder in folders {
            let path = folder.join("exe.xml");
            if !files::exists(&path) {
                continue;
            }
            require(
                path.parent()
                    .ok_or(Error::Invalid("Invalid startup path."))?
                    .canonicalize()?
                    .starts_with(prefix.canonicalize()?),
                "The startup folder escapes the selected profile.",
            )?;
            let raw = files::read(&path, 2 * 1024 * 1024)?;
            let tree = xml::parse(&raw)?;
            for (index, item) in tree.items.iter().enumerate() {
                let Item::Element(addon) = item else {
                    continue;
                };
                if !addon.named("Launch.Addon") {
                    continue;
                }
                let value = text(addon, "Path");
                let value = value.trim().trim_matches('"');
                let executable_name = value
                    .replace('\\', "/")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if !COMPANIONS.contains(&executable_name.as_str()) {
                    continue;
                }
                let executable = mods::configured_path(value, &prefix)?;
                require(
                    executable.starts_with(&directory),
                    "Der GSX-Start verweist auf eine andere Installation.",
                )?;
                require(
                    files::open_at(rustix::fs::CWD, &executable, false, false)?
                        .metadata()?
                        .len()
                        <= 180 * 1024 * 1024,
                    "Invalid GSX startup executable.",
                )?;
                require(
                    found.is_none(),
                    "Die FSDT-Starteinstellung ist nicht eindeutig. Im FSDT-Installer aktualisieren.",
                )?;
                found = Some(Startup {
                    path: path.clone(),
                    tree: tree.clone(),
                    index,
                    raw: raw.clone(),
                });
            }
        }
    }
    Ok(found)
}
pub fn configure(root: &Path, enable: bool) -> Result<()> {
    require(complete(root), INCOMPLETE)?;
    let directory = manager(&root.join("local/msfs-prefix"))?;
    require(
        community(root)?.is_some(),
        "Installiere GSX zuerst im FSDT-Installer und prüfe die Community-Verknüpfung.",
    )?;
    let mut startup = startup(root, directory.as_deref())?.ok_or(Error::Invalid(
        "Der FSDT-Start fehlt. Führe im FSDT-Installer ein Update aus und prüfe erneut.",
    ))?;
    require(
        enabled(&startup.tree),
        "Der automatische Add-on-Start ist in exe.xml ausgeschaltet.",
    )?;
    files::atomic(
        &root
            .join("private")
            .join(format!("gsx-exe-{}.xml", uuid::Uuid::new_v4().simple())),
        &startup.raw,
    )?;
    if let Item::Element(addon) = &mut startup.tree.items[startup.index] {
        addon.set("Disabled", if enable { "False" } else { "True" });
    }
    files::atomic(&startup.path, &startup.tree.bytes()?)?;
    files::atomic_json(&root.join(STARTUP), &json!({"format":1,"enabled":enable}))
}
pub fn snapshot(app: &Launcher) -> Value {
    let (root, busy, playing, closing, job) = {
        let mut s = app.lock();
        Launcher::poll(&mut s);
        (
            s.runtime.clone(),
            s.active.is_some() || Launcher::external(&s),
            s.process.is_some(),
            s.closing,
            s.jobs
                .get("gsx")
                .cloned()
                .filter(|j| {
                    j["runtime_path"].as_str() == s.runtime.as_ref().and_then(|p| p.to_str())
                })
                .unwrap_or(Value::Null),
        )
    };
    let mut value = json!({"state":"unavailable","message":"GSX ist zunächst für MSFS 2024 verfügbar.","prepared":false,"package_installed":false,"startup_found":false,"configured":false,"can_recover":false,"idle":false,"verified_in_simulator":false,"runtime_path":root,"job":job,"busy":busy||playing,"manager_running":false,"can_change":false,"can_stop":false});
    if let Some(root) = root {
        let result = (|| -> Result<()> {
            fenix::validate(&root, true)?;
            proton::check(&root)?;
            let state = marker(&root)?;
            let prefix = root.join("local/msfs-prefix");
            if state.is_some_and(|v| v["state"] != "ready") {
                value["state"] = json!("interrupted");
                value["can_recover"] = json!(true);
                value["message"] = json!(INCOMPLETE);
            } else {
                let directory = manager(&prefix)?;
                let prepared = directory.is_some() && framework::status(&prefix).ready;
                let package = community(&root)?.is_some();
                let entry = startup(&root, directory.as_deref())?;
                let active = entry.as_ref().is_some_and(|v| {
                    enabled(&v.tree)
                        && matches!(&v.tree.items[v.index],Item::Element(addon) if enabled(addon))
                });
                let setting = if files::exists(&root.join(STARTUP)) {
                    runtime::value(&root.join(STARTUP))?
                } else {
                    Value::Null
                };
                value["state"] = json!("available");
                value["prepared"] = json!(prepared);
                value["package_installed"] = json!(package);
                value["startup_found"] = json!(entry.is_some());
                value["configured"] = json!(
                    prepared
                        && package
                        && active
                        && setting["format"].as_u64() == Some(1)
                        && setting["enabled"] == true
                );
                value["message"] = json!(
                    "Installation und Lizenz prüft FSDT. Der GSX-Flugbetrieb unter Linux ist noch nicht bestätigt."
                );
            }
            let idle = wine_processes::idle(&prefix).is_ok();
            let processes = if prefix.exists() {
                Some(wine_processes::Processes::new(&prefix)?)
            } else {
                None
            };
            let running = processes.as_ref().is_some_and(|p| p.any(Some(MANAGERS)));
            let game = processes
                .as_ref()
                .is_some_and(|p| p.any(Some(wine_processes::GAMES)));
            let interactive =
                job["state"] == "running" && job["operation"] == "open" && job["stopping"] != true;
            value["idle"] = json!(idle);
            value["manager_running"] = json!(running);
            value["can_change"] = json!(idle && !busy && !playing && !closing);
            value["can_stop"] =
                json!(running && !playing && !game && !closing && (interactive || !busy));
            Ok(())
        })();
        if let Err(error) = result {
            value["message"] = json!(error.to_string());
        }
    }
    value
}
fn open(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    require(
        complete(root) && snapshot(&ctx.launcher)["prepared"] == true,
        "Bereite zuerst den FSDT-Installer vor.",
    )?;
    let prefix = root.join("local/msfs-prefix");
    let directory =
        manager(&prefix)?.ok_or(Error::Invalid("Bereite zuerst den FSDT-Installer vor."))?;
    let mut wine = Wine::new(
        &prefix,
        &root.join("runner"),
        &root.join("private/gsx-manager.log"),
        &ctx.cancel,
    )?;
    wine.without_fenix();
    let mut command = wine.command()?;
    command
        .arg(directory.join("Couatl_Updater.exe"))
        .args(["/SILENT", "/INSTALLMODE=TRUE"])
        .current_dir(&directory);
    let mut child = process::spawn(&mut command, None)?;
    ctx.progress(
        "Installiere und aktiviere GSX im FSDT-Installer. Schließe ihn danach vollständig.",
    );
    let mut quiet = None;
    let result = (|| {
        loop {
            if ctx.pause.load(Ordering::Relaxed) || ctx.cancel.load(Ordering::Relaxed) {
                wine_processes::stop(root, MANAGERS)?;
                process::terminate(&mut child)?;
                return ctx.interrupted();
            }
            let code = child.try_wait()?;
            let running = wine_processes::Processes::new(&prefix)?.any(Some(MANAGERS));
            if let Some(code) = code
                && !running
            {
                let since = quiet.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(1500) {
                    wine_processes::stop(root, MANAGERS)?;
                    return require(
                        code.success(),
                        "Der FSDT-Installer wurde unerwartet beendet. Details stehen im GSX-Protokoll.",
                    );
                }
            } else {
                quiet = None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    })();
    if result.is_err() {
        let _ = wine_processes::stop(root, MANAGERS);
        let _ = process::terminate(&mut child);
    }
    result
}
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    require(
        ["prepare", "open", "configure", "disable", "recover", "stop"].contains(&operation),
        "Unbekannter GSX-Vorgang.",
    )?;
    let root = app.root()?;
    require(
        root == Path::new(string(data, "runtime_path")?),
        "Die ausgewählte Runtime hat sich geändert. GSX-Status neu laden.",
    )?;
    if operation != "stop" {
        proton::check(&root)?;
    } else {
        require(
            snapshot(app)["can_stop"] == true,
            "Beende MSFS und laufende Installationen, bevor du FSDT schließt.",
        )?;
        let mut s = app.lock();
        Launcher::open(&s)?;
        if s.active.as_ref().is_some_and(|v| v.kind == "gsx")
            && s.jobs
                .get("gsx")
                .is_some_and(|v| v["state"] == "running" && v["operation"] == "open")
        {
            if let Some(active) = &s.active {
                active.pause.store(true, Ordering::Relaxed);
            }
            let job = s
                .jobs
                .get_mut("gsx")
                .ok_or(Error::Invalid("FSDT läuft nicht."))?;
            job["stopping"] = json!(true);
            return Ok(json!({"ok":true,"job_id":job["id"]}));
        }
    }
    let operation = operation.to_string();
    app.start_job("gsx",&operation.clone(),true,move|ctx|{
        let selected=ctx.root()?;require(selected==root,"Die ausgewählte Runtime hat sich geändert. GSX-Status neu laden.")?;let root=selected;fenix::validate(root,operation=="recover"||operation=="stop")?;if operation!="stop"{wine_processes::idle(&root.join("local/msfs-prefix"))?;}
        match operation.as_str(){"prepare"=>prepare(ctx)?,"recover"=>recover(ctx)?,"open"=>open(ctx)?,"stop"=>wine_processes::stop(root,MANAGERS)?,_=>configure(root,operation=="configure")?}
        let message=match operation.as_str(){"prepare"=>"FSDT ist vorbereitet. Öffne den Installer, um GSX zu installieren und zu aktivieren.","configure"=>"GSX startet mit MSFS. Prüfe Menü und Bodendienste im Simulator; Linux-Kompatibilität ist noch unbestätigt.","disable"=>"Der automatische GSX-Start ist ausgeschaltet.","recover"=>"Das Windows-Profil vor der unterbrochenen GSX-Einrichtung ist wieder aktiv.",_=>"FSDT wurde geschlossen. Der GSX-Status wird neu geprüft."};Ok(json!({"state":"complete","message":message,"stopping":false}))
    })
}
