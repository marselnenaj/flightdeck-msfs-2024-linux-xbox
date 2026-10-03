// SPDX-License-Identifier: MIT
//! Reviewed edition-scoped removal and reversible Wine environment resets.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    bootstrap, cloud_process_guard,
    error::require,
    files,
    games::Game,
    log_reader::regex,
    mods,
    owned_tree::{self, Inventory},
    proton, transaction as tx, wine_processes,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
const STALE: &str = "Die Vorschau ist nicht mehr gültig. Prüfe die Wartung erneut.";
const CHANGED: &str =
    "Die Installation wurde seit der Vorschau geändert. Prüfe die Wartung erneut.";
const RESTORE: &str = "Die gesicherte Spielumgebung passt nicht mehr zur aktiven Installation.";
#[derive(Clone)]
pub struct Plan {
    runtime: PathBuf,
    packages: Vec<PathBuf>,
    inventory: BTreeMap<PathBuf, Inventory>,
    created: Instant,
    id: String,
    operation: String,
    keep_data: bool,
    delete_packages: bool,
}
fn idle(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    cloud_process_guard::check(root, &ctx.lease()?)?;
    wine_processes::idle(&root.join("local/msfs-prefix"))
}
fn game_entries(root: &Path) -> Result<Vec<PathBuf>> {
    let game = Game::for_runtime(root)?;
    files::directory(&root.join("games"), false)?;
    let pattern = regex::Regex::new(&format!(
        r"^\.{}-before-[0-9a-f]{{32}}$",
        regex::escape(game.directory())
    ))
    .expect("known game directory");
    let mut result = vec![game.path(root)];
    let entries = fs::read_dir(root.join("games"))?
        .take(10001)
        .collect::<std::io::Result<Vec<_>>>()?;
    require(entries.len() <= 10000, CHANGED)?;
    let mut backups: Vec<_> = entries
        .into_iter()
        .filter(|e| pattern.is_match(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    backups.sort();
    result.extend(backups);
    Ok(result)
}
pub fn packages(root: &Path, known: &BTreeMap<String, PathBuf>) -> Result<Vec<PathBuf>> {
    let game = Game::for_runtime(root)?;
    let (locations, limited) = mods::locations(root)?;
    require(
        !limited,
        "Die zusätzlichen Paketordner konnten nicht vollständig geprüft werden.",
    )?;
    let mut result = Vec::new();
    for entry in game_entries(root)? {
        let target = entry.canonicalize()?;
        require(
            target != files::home()
                && target != Path::new("/")
                && !root.starts_with(&target)
                && !target.starts_with(root.join("local"))
                && !root.join("private/local-saves").starts_with(&target),
            "Dieser Spielordner kann nicht sicher entfernt werden.",
        )?;
        crate::integrity::installed_identity(&target, game)?;
        require(
            !locations.iter().any(|(p, _)| p.starts_with(&target)),
            "Im Basisspiel liegen zusätzliche Pakete oder Add-ons. Verschiebe sie zuerst oder behalte die Spieldateien.",
        )?;
        for other in known.values().filter(|p| p.as_path() != root && p.exists()) {
            for path in game_entries(other)? {
                require(
                    path.canonicalize()? != target,
                    "Eine andere Installation verwendet diese Spieldateien. Wähle Spieldateien behalten.",
                )?;
            }
        }
        if !result.contains(&target) {
            result.push(target);
        }
    }
    require(
        !result
            .iter()
            .any(|a| result.iter().any(|b| a != b && a.starts_with(b))),
        "Dieser Spielordner kann nicht sicher entfernt werden.",
    )?;
    Ok(result)
}
fn reset_sources(root: &Path) -> Result<BTreeMap<&'static str, (PathBuf, String)>> {
    require(
        !files::exists(&root.join(crate::fenix::MARKER)),
        "Stelle unter Mods zuerst den ursprünglichen Fenix-Zustand wieder her. Danach kannst du die Spielumgebung zurücksetzen.",
    )?;
    files::directory(&root.join("local"), false)?;
    let system = tx::prefix_system32(&root.join("local/msfs-prefix"))?;
    let record: Value = files::json(&root.join("private/import-manifest.json"), 2 * 1024 * 1024)?;
    let sources = [
        (
            "xgameruntime.dll",
            system.join("xgameruntime.dll"),
            record["artifacts"]["files"]["runtime/xgameruntime.dll"].clone(),
        ),
        (
            "xodus_store_test.dll",
            root.join("local/store-runtime/x86_64-windows/xodus_store_test.dll"),
            record["artifacts"]["files"]["builtin/x86_64-windows/xodus_store_test.dll"].clone(),
        ),
        (
            "xgameruntime_original.dll",
            root.join("runner/files/lib/wine/x86_64-windows/xgameruntime.dll"),
            record["original_runtime_sha256"].clone(),
        ),
    ];
    let mut result = BTreeMap::new();
    for (name, path, hash) in sources {
        let hash = hash
            .as_str()
            .ok_or(Error::Invalid("Ungültige Kompatibilitätsdateien."))?;
        require(
            files::hex_digest(hash) && tx::digest(&path)? == hash,
            "Die Kompatibilitätsdateien passen nicht zur Installation. Aktualisiere Flightdeck vor dem Zurücksetzen.",
        )?;
        result.insert(name, (path, hash.to_owned()));
    }
    require(
        root.join("runner/files/bin/wine").is_file(),
        "Der eingerichtete Runner fehlt.",
    )?;
    Ok(result)
}
pub fn restore_path(root: &Path) -> Result<PathBuf> {
    files::directory(&root.join("local"), false)?;
    let active = owned_tree::identity(&root.join("local/msfs-prefix"))?;
    for name in ["environment-reset-pending.json", "environment-reset.json"] {
        let read = || -> Result<PathBuf> {
            let record: Value = files::json(&root.join("private").join(name), 2 * 1024 * 1024)?;
            let name = string(&record, "backup")?;
            require(
                record["schema"] == 1
                    && regex(r"^local/environment-backup-[a-f0-9]{32}/prefix$").is_match(name),
                RESTORE,
            )?;
            let backup = root.join(name);
            proton::managed(root, &backup)?;
            require(
                json!(owned_tree::identity(&backup)?) == record["original_id"]
                    && json!(active) == record["fresh_id"],
                RESTORE,
            )?;
            Ok(backup)
        };
        if let Ok(path) = read() {
            return Ok(path);
        }
    }
    Err(Error::Invalid(RESTORE))
}
pub fn reset(ctx: &Context) -> Result<PathBuf> {
    let root = ctx.root()?;
    let sources = reset_sources(root)?;
    let prefix = root.join("local/msfs-prefix");
    let stage = tx::new_directory(&root.join("local"), "environment-backup-")?;
    let fresh = stage.join("prefix");
    bootstrap::prepare_prefix(&root.join("runner"), &fresh, &ctx.cancel)?;
    let system = tx::prefix_system32(&fresh)?;
    for (name, (path, hash)) in sources {
        tx::copy_path(&path, &system.join(name), Some(&hash), &ctx.cancel)?;
    }
    let (locations, limited) = mods::locations(root)?;
    require(
        !limited,
        "Die zusätzlichen Paketordner konnten nicht vollständig geprüft werden.",
    )?;
    let mut packages: Vec<_> = locations
        .into_iter()
        .filter_map(|(p, _)| p.parent().map(Path::to_path_buf))
        .collect();
    packages.sort_by_key(|p| p.components().count());
    packages.dedup();
    let mut linked: Vec<PathBuf> = Vec::new();
    for packages in packages {
        if let Ok(relative) = packages.strip_prefix(&prefix)
            && packages.is_dir()
            && !linked.iter().any(|p| packages.starts_with(p))
        {
            require(
                files::relative(relative.to_str().unwrap_or("")),
                "Ungültiger Paketordner.",
            )?;
            let target = fresh.join(relative);
            require(
                !files::exists(&target),
                "Ein Paketordner ist in der neuen Spielumgebung bereits vorhanden.",
            )?;
            let mut parent = fresh.clone();
            for component in relative.parent().into_iter().flat_map(Path::components) {
                parent.push(component);
                if !files::exists(&parent) {
                    fs::create_dir(&parent)?;
                }
                files::directory(&parent, false)?;
            }
            std::os::unix::fs::symlink(&target, &target)?;
            linked.push(packages);
        }
    }
    let record = json!({"schema":1,"backup":fresh.strip_prefix(root).map_err(|_|Error::Invalid(RESTORE))?,"original_id":owned_tree::identity(&prefix)?,"fresh_id":owned_tree::identity(&fresh)?});
    ctx.begin_commit("activating", "Die neue Spielumgebung wird aktiviert …")?;
    let pending = root.join("private/environment-reset-pending.json");
    files::atomic_json(&pending, &record)?;
    tx::exchange(&prefix, &fresh)?;
    fs::rename(pending, root.join("private/environment-reset.json"))?;
    files::directory(&root.join("private"), true)?.sync_all()?;
    Ok(fresh)
}
pub fn snapshot(app: &Arc<Launcher>) -> Value {
    let s = app.lock();
    json!({"job":s.jobs.get("maintenance"),"can_restore":s.runtime.as_deref().is_some_and(|r|restore_path(r).is_ok())})
}
pub fn preview(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let operation = string(data, "operation")?.to_owned();
    let keep_data = data
        .get("keep_data")
        .unwrap_or(&Value::Bool(true))
        .as_bool()
        .ok_or(Error::Invalid("Ungültige Wartungsoptionen."))?;
    let delete_packages = data
        .get("delete_packages")
        .unwrap_or(&Value::Bool(true))
        .as_bool()
        .ok_or(Error::Invalid("Ungültige Wartungsoptionen."))?;
    require(
        ["reset", "restore", "uninstall"].contains(&operation.as_str()),
        "Ungültige Wartungsoptionen.",
    )?;
    require(
        delete_packages || keep_data,
        "Wenn du Spieldateien behältst, muss auch die bisherige Installation erhalten bleiben.",
    )?;
    let mut s = app.lock();
    let root = s
        .runtime
        .as_ref()
        .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?;
    require(
        proton::selection(root)?.is_none() && proton::check(root).is_ok(),
        "Kehre vor der Wartung unter Proton-Version zur Flightdeck-Umgebung zurück.",
    )?;
    let game = Game::for_runtime(root)?;
    let known = s.known.clone();
    let ctx = app.reserve_locked(&mut s, "maintenance", &operation, true)?;
    s.maintenance = None;
    drop(s);
    ctx.update(json!({"state":"checking","game_name":game.name(),"keep_data":keep_data,"delete_packages":delete_packages,"message":"Die ausgewählte Installation wird geprüft …","error":null}));
    std::thread::spawn(move || {
        let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<Value>{idle(&ctx)?;let root=ctx.root()?;let paths=if operation=="uninstall"&&delete_packages{packages(root,&known)?}else{vec![]};if operation=="reset"{reset_sources(root)?;}else if operation=="restore"{restore_path(root)?;}let mut inventory=BTreeMap::new();for p in std::iter::once(root).chain(paths.iter().map(PathBuf::as_path)){inventory.insert(p.into(),owned_tree::inventory(p,&ctx.cancel)?);}let bytes:u64=paths.iter().filter_map(|p|inventory.get(p)).map(|v|v.bytes).sum();let plan=Plan{runtime:root.into(),packages:paths.clone(),inventory,created:Instant::now(),id:ctx.id.clone(),operation,keep_data,delete_packages};ctx.launcher.lock().maintenance=Some(plan);Ok(json!({"state":"ready","message":"Prüfe die Ordner und bestätige anschließend die Aktion.","packages":paths,"package_bytes":bytes}))})).unwrap_or(Err(Error::Invalid("Die Wartung konnte nicht abgeschlossen werden. Vorhandene Sicherungen bleiben erhalten.")));
        ctx.launcher.finish(&ctx, outcome, false);
    });
    Ok(json!({"ok":true,"job":app.job("maintenance")}))
}
pub fn discard(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let mut s = app.lock();
    require(
        s.jobs
            .get("maintenance")
            .is_some_and(|j| j["id"] == data["job_id"] && j["state"] == "ready"),
        STALE,
    )?;
    s.maintenance = None;
    let job = s.jobs.get_mut("maintenance").ok_or(Error::Invalid(STALE))?;
    job["state"] = json!("cancelled");
    job["message"] = json!("Wartung abgebrochen.");
    let job = job.clone();
    drop(s);
    Ok(json!({"ok":true,"job":job,"can_restore":snapshot(app)["can_restore"]}))
}
pub fn start(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let mut s = app.lock();
    let plan = s.maintenance.clone().ok_or(Error::Invalid(STALE))?;
    require(
        data["confirmed"] == true
            && data["job_id"] == plan.id
            && s.runtime.as_ref() == Some(&plan.runtime)
            && plan.created.elapsed() <= Duration::from_secs(600)
            && s.jobs
                .get("maintenance")
                .is_some_and(|j| j["id"] == plan.id && j["state"] == "ready"),
        STALE,
    )?;
    let mut job = s.jobs["maintenance"].clone();
    let ctx = app.reserve_locked(&mut s, "maintenance", &plan.operation, true)?;
    job["id"] = json!(ctx.id);
    job["state"] = json!("running");
    job["message"] = json!("Die bestätigte Wartung wird ausgeführt …");
    s.jobs.insert("maintenance".into(), job);
    s.maintenance = None;
    drop(s);
    std::thread::spawn(move || {
        let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||execute(&ctx,&plan))).unwrap_or(Err(Error::Invalid("Die Wartung konnte nicht abgeschlossen werden. Vorhandene Sicherungen bleiben erhalten.")));
        ctx.launcher.finish(&ctx, outcome, false);
    });
    Ok(json!({"ok":true,"job":app.job("maintenance")}))
}
fn execute(ctx: &Context, plan: &Plan) -> Result<Value> {
    idle(ctx)?;
    for (path, inventory) in &plan.inventory {
        require(
            owned_tree::inventory(path, &ctx.cancel)? == *inventory,
            CHANGED,
        )?;
    }
    let (backup, message) = match plan.operation.as_str() {
        "reset" => (
            Some(reset(ctx)?),
            "Die Spielumgebung wurde zurückgesetzt. Basisspiel und lokale Spielstände bleiben erhalten. Zusatzprogramme müssen neu eingerichtet werden.",
        ),
        "restore" => {
            let backup = restore_path(&plan.runtime)?;
            ctx.begin_commit("activating", "Die vorherige Spielumgebung wird aktiviert …")?;
            tx::exchange(&plan.runtime.join("local/msfs-prefix"), &backup)?;
            (
                Some(backup),
                "Die vorherige Spielumgebung ist wieder aktiv.",
            )
        }
        "uninstall" => (
            uninstall(ctx, plan)?,
            "Das Spiel wurde aus Flightdeck entfernt. Du kannst es unter Installation erneut einrichten.",
        ),
        _ => return Err(Error::Invalid(STALE)),
    };
    ctx.launcher.lock().graphics_report = Value::Null;
    Ok(json!({"state":"complete","message":message,"backup_path":backup}))
}
fn uninstall(ctx: &Context, plan: &Plan) -> Result<Option<PathBuf>> {
    let root = &plan.runtime;
    let archive = root.with_file_name(format!(
        "{}.uninstalled-{}",
        root.file_name()
            .and_then(|s| s.to_str())
            .ok_or(Error::Invalid(STALE))?,
        uuid::Uuid::new_v4().simple()
    ));
    let moves: Vec<_> = plan
        .packages
        .iter()
        .map(|p| {
            (
                p.clone(),
                p.with_file_name(format!(
                    ".flightdeck-remove-{}",
                    uuid::Uuid::new_v4().simple()
                )),
            )
        })
        .collect();
    let mut record = json!({"schema":1,"original_runtime":root,"archive_runtime":archive,"moves":moves.iter().map(|(p,q)|json!({"source":p,"quarantine":q})).collect::<Vec<_>>(),"keep_data":plan.keep_data,"state":"prepared"});
    ctx.begin_commit("removing", "Die bestätigte Installation wird entfernt …")?;
    let mut s = ctx.launcher.lock();
    if plan.delete_packages {
        require(packages(root, &s.known)? == plan.packages, CHANGED)?;
    }
    let config = ctx.launcher.state_dir.join("config.json");
    let read_config = || -> Result<Option<Vec<u8>>> {
        match files::read(&config, 65536) {
            Ok(data) => Ok(Some(data)),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    };
    let previous_config = read_config()?;
    files::atomic_json(&root.join("private/uninstalled.json"), &record)?;
    let mut moved = Vec::new();
    let mut archived = false;
    let known: BTreeMap<_, _> = s
        .known
        .iter()
        .filter(|(_, p)| *p != root)
        .map(|(k, p)| (k.clone(), p.clone()))
        .collect();
    let selected = known.values().next().cloned();
    let publish = (|| -> Result<()> {
        for (p, q) in &moves {
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                p,
                rustix::fs::CWD,
                q,
                rustix::fs::RenameFlags::NOREPLACE,
            )?;
            moved.push((p, q));
            files::directory(q.parent().ok_or(Error::Invalid(STALE))?, false)?.sync_all()?;
        }
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            root,
            rustix::fs::CWD,
            &archive,
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        archived = true;
        files::directory(archive.parent().ok_or(Error::Invalid(STALE))?, false)?.sync_all()?;
        files::atomic_json(
            &ctx.launcher.state_dir.join("config.json"),
            &json!({"schema":1,"runtime_path":selected,"runtimes":known}),
        )
    })();
    if let Err(error) = publish {
        // A failed directory flush can occur after config publication. Preserve
        // every quarantined file if publication cannot be ruled out; never
        // delete or silently roll back data against a different configuration.
        if archived && !read_config().is_ok_and(|now| now == previous_config) {
            s.runtime = selected;
            s.known = known;
            if let Some(job) = s.jobs.get_mut("maintenance") {
                job["backup_path"] = json!(archive);
            }
            drop(s);
            record["state"] = json!("publication_uncertain");
            let _ = files::atomic_json(&archive.join("private/uninstalled.json"), &record);
            return Err(error);
        }
        if archived {
            tx::publish(&archive, root)?;
        }
        for (p, q) in moved.iter().rev() {
            tx::publish(q, p)?;
        }
        fs::remove_file(root.join("private/uninstalled.json"))?;
        files::directory(&root.join("private"), true)?.sync_all()?;
        return Err(error);
    }
    s.runtime = selected;
    s.known = known;
    drop(s);
    ctx.update(json!({"backup_path":archive}));
    let relocated = |p: &Path| {
        p.strip_prefix(root)
            .map(|r| archive.join(r))
            .unwrap_or_else(|_| p.into())
    };
    record["state"] = json!("detached");
    record["cleanup"] = json!(moves.iter().map(|(_, q)| relocated(q)).collect::<Vec<_>>());
    files::atomic_json(&archive.join("private/uninstalled.json"), &record)?;
    for (p, q) in moves {
        owned_tree::remove(
            &relocated(&q),
            plan.inventory.get(&p).ok_or(Error::Invalid(CHANGED))?.id,
        )?;
    }
    if !plan.keep_data {
        owned_tree::remove(
            &archive,
            plan.inventory.get(root).ok_or(Error::Invalid(CHANGED))?.id,
        )?;
        Ok(None)
    } else {
        Ok(Some(archive))
    }
}
