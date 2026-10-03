// SPDX-License-Identifier: MIT
//! Read-only setup plans, then revalidated independent profile publication.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    bootstrap, components,
    error::require,
    files, game_package,
    games::Game,
    process, resources, runtime, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
pub fn path_input(value: &str, exists: bool) -> Result<PathBuf> {
    require(
        !value.trim().is_empty()
            && value.len() <= 4096
            && !value.chars().any(|c| c.is_control() || ":;|".contains(c)),
        "Bitte einen gültigen absoluten Ordnerpfad angeben.",
    )?;
    let path = files::expand(value);
    require(path.is_absolute(), "Der Pfad muss absolut sein.")?;
    if exists {
        let path = path.canonicalize()?;
        files::directory(&path, false)?;
        Ok(path)
    } else {
        tx::resolve_new(&path)
    }
}
pub fn market(value: &str) -> Result<&str> {
    require(
        value.len() == 2 && value.bytes().all(|v| v.is_ascii_uppercase()),
        "Bitte einen Ländercode mit zwei Großbuchstaben wählen, zum Beispiel AT.",
    )?;
    Ok(value)
}
fn chosen(data: &Value) -> Result<Game> {
    Game::select(if data.get("game_id").is_none() {
        "msfs2024"
    } else {
        string(data, "game_id")?
    })
}
fn locals(data: &Value, default: bool) -> Result<bool> {
    if data.get("local_saves").is_none() {
        return Ok(default);
    }
    data["local_saves"].as_bool().ok_or(Error::Invalid(
        "Die Auswahl für lokale Spielstände muss Ja oder Nein sein.",
    ))
}
pub fn default_path(game: Game) -> PathBuf {
    files::xdg("XDG_DATA_HOME", ".local/share")
        .join("flightdeck/runtimes")
        .join(game.id())
}
pub fn preflight(data: &Value, ctx: &Context) -> Result<Value> {
    ctx.interrupted()?;
    let game = chosen(data)?;
    let region = market(string(data, "market")?)?;
    let local_saves = locals(data, true)?;
    let mut sources = BTreeMap::new();
    for name in ["artifacts", "game", "runner", "prefix"] {
        sources.insert(
            name,
            path_input(string(data, &format!("{name}_path"))?, true)?,
        );
    }
    let dest = path_input(string(data, "destination_path")?, false)?;
    require(
        !sources.values().any(|v| dest.starts_with(v)),
        "Der Zielordner darf nicht innerhalb eines Eingabeordners liegen.",
    )?;
    ctx.progress("Spielpaket, Proton und Build-Prüfsummen werden geprüft …");
    game_package::validate_download(&sources["game"], game)?;
    let runner = &sources["runner"];
    require(
        runtime::executable(&runtime::wine(runner))
            && runtime::executable(&runner.join("files/bin/wineserver")),
        "Der Proton-Ordner enthält keine vollständige Wine-Umgebung.",
    )?;
    let upstream = resources::json("compat/upstreams.lock.json")?;
    let original = runner.join("files/lib/wine/x86_64-windows/xgameruntime.dll");
    require(
        tx::digest(&original)? == upstream["runner"]["original_runtime_sha256"],
        "Der Proton-Runner passt nicht zur geprüften Runtime.",
    )?;
    let prefix = &sources["prefix"];
    tx::prefix_system32(prefix)?;
    for name in ["system.reg", "user.reg"] {
        files::read(&prefix.join(name), 64 * 1024 * 1024)?;
    }
    let manifest: Value =
        files::json(&sources["artifacts"].join("manifest.json"), 2 * 1024 * 1024)?;
    bootstrap::verify_native(&sources["artifacts"], &manifest)?;
    let plugins = if data["media_plugins_path"]
        .as_str()
        .is_some_and(|v| !v.is_empty())
    {
        Some(path_input(string(data, "media_plugins_path")?, true)?)
    } else {
        None
    };
    let size = tx::prefix_size(prefix, &ctx.cancel)?;
    let mut need = size + 16 * 1024 * 1024 + original.metadata()?.len();
    for name in bootstrap::artifact_names(&manifest)? {
        need = need
            .checked_add(sources["artifacts"].join(name).metadata()?.len() * 2)
            .ok_or(Error::Invalid("Die Einrichtung ist zu groß."))?;
    }
    require(
        tx::free_bytes(&dest)? >= need,
        "Am Ziel fehlt Speicherplatz für eine vollständige Profilkopie.",
    )?;
    ctx.interrupted()?;
    Ok(
        json!({"mode":"prepare","game_id":game.id(),"market":region,"local_saves":local_saves,"destination_path":dest,"artifacts_path":sources["artifacts"],"game_path":sources["game"],"runner_path":runner,"prefix_path":prefix,"media_plugins_path":plugins.map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),"prefix_bytes":size,"manifest":manifest,"original_hash":upstream["runner"]["original_runtime_sha256"]}),
    )
}
pub fn prepare(plan: &Value, ctx: &Context, xdg: Option<&Path>) -> Result<PathBuf> {
    let dest = PathBuf::from(string(plan, "destination_path")?);
    require(
        !files::exists(&dest),
        "Der Zielordner existiert bereits und wird nicht überschrieben.",
    )?;
    let stage = tx::new_directory(
        dest.parent()
            .ok_or(Error::Invalid("Ungültiger Zielpfad."))?,
        ".flightdeck-setup-",
    )?;
    let result = (|| {
        for name in ["private", "local", "games", "bin"] {
            files::private_dir(&stage.join(name))?;
        }
        if let Some(xdg) = xdg {
            require(xdg.is_absolute(), "Ungültiger privater Downloadordner.")?;
            files::directory(xdg, true)?;
            std::os::unix::fs::symlink(xdg, stage.join("private/xdg"))?;
        }
        resources::install_scripts(&stage.join("tools"))?;
        let game = chosen(plan)?;
        let runner = Path::new(string(plan, "runner_path")?);
        let prefix = Path::new(string(plan, "prefix_path")?);
        let artifacts = Path::new(string(plan, "artifacts_path")?);
        std::os::unix::fs::symlink(
            string(plan, "game_path")?,
            stage.join("games").join(game.directory()),
        )?;
        std::os::unix::fs::symlink(runner, stage.join("runner"))?;
        ctx.progress("Wine-Umgebung wird unabhängig kopiert. Das kann einige Minuten dauern …");
        let fresh = stage.join("local/msfs-prefix");
        process::copy_tree(prefix, &fresh, &ctx.cancel)?;
        tx::relocate_prefix_links(prefix, &fresh, &ctx.cancel)?;
        let system = tx::prefix_system32(&fresh)?;
        ctx.progress("Geprüfte Kompatibilitätsdateien werden eingesetzt …");
        for name in bootstrap::artifact_names(&plan["manifest"])? {
            let target = if let Some(name) = name.strip_prefix("builtin/") {
                stage.join("local/store-runtime").join(name)
            } else if name.starts_with("bin/") {
                stage.join(name)
            } else {
                system.join("xgameruntime.dll")
            };
            tx::copy_path(
                &artifacts.join(name),
                &target,
                plan["manifest"]["files"][name].as_str(),
                &ctx.cancel,
            )?;
        }
        tx::copy_path(
            &runner.join("files/lib/wine/x86_64-windows/xgameruntime.dll"),
            &system.join("xgameruntime_original.dll"),
            Some(string(plan, "original_hash")?),
            &ctx.cancel,
        )?;
        tx::copy_path(
            &stage.join("local/store-runtime/x86_64-windows/xodus_store_test.dll"),
            &system.join("xodus_store_test.dll"),
            None,
            &ctx.cancel,
        )?;
        if let Some(plugins) = plan["media_plugins_path"]
            .as_str()
            .filter(|v| !v.is_empty())
        {
            std::os::unix::fs::symlink(plugins, stage.join("local/media-plugins"))?;
        }
        let mut scripts = json!({});
        for name in components::SCRIPTS {
            scripts[name] = json!(tx::digest(&stage.join("tools").join(name))?);
        }
        let local = locals(plan, false)?;
        let mut inputs = json!({});
        for name in ["artifacts", "game", "runner", "prefix"] {
            inputs[name] = plan[format!("{name}_path")].clone();
        }
        files::atomic_json(
            &stage.join("private/runtime.json"),
            &json!({"format":1,"game_id":game.id(),"market":plan["market"],"local_saves":local}),
        )?;
        files::atomic_json(
            &stage.join("private/import-manifest.json"),
            &json!({"format":1,"artifacts":plan["manifest"],"original_runtime_sha256":plan["original_hash"],"runtime_files":scripts,"local_saves":local,"inputs":inputs}),
        )?;
        if local {
            files::private_dir(&stage.join("private/local-saves"))?;
            files::atomic(&stage.join("private/local-saves.enabled"), b"local-only\n")?;
        }
        ctx.interrupted()?;
        ctx.progress("Vollständige Runtime wird übernommen …");
        tx::publish(&stage, &dest)?;
        Ok(dest)
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
pub fn preflight_install(data: &Value, ctx: &Context) -> Result<Value> {
    require(
        bootstrap::availability()["available"] == true,
        "Die automatische Installation unterstützt derzeit Linux auf x86-64.",
    )?;
    let game = chosen(data)?;
    let destination = match data["destination_path"].as_str().filter(|v| !v.is_empty()) {
        Some(path) => path_input(path, false)?,
        None => tx::resolve_new(&default_path(game))?,
    };
    let market = market(data["market"].as_str().unwrap_or("US"))?;
    let local = locals(data, false)?;
    require(
        tx::free_bytes(&destination)? >= 100 * 1024 * 1024 * 1024,
        "Für die Erstinstallation werden mindestens 100 GiB freier Speicherplatz benötigt. Bitte einen anderen Speicherort wählen.",
    )?;
    require(
        (std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some())
            && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        "Flightdeck bitte aus der grafischen Linux-Sitzung starten, damit die Microsoft-Anmeldung und der Schlüsselbund verfügbar sind.",
    )?;
    let lock = resources::json("compat/bootstrap.lock.json")?;
    let libc = process::output(
        Command::new("getconf").arg("GNU_LIBC_VERSION"),
        Duration::from_secs(5),
        256,
        &ctx.cancel,
    )?;
    let libc = String::from_utf8(libc)
        .map_err(|_| Error::Invalid("Die glibc-Version konnte nicht geprüft werden."))?;
    let observed = libc
        .trim()
        .strip_prefix("glibc ")
        .ok_or(Error::Invalid("Dieses Laufzeitpaket benötigt glibc."))?;
    let parts = |v: &str| -> Result<Vec<u32>> {
        v.split('.')
            .map(|p| {
                p.parse()
                    .map_err(|_| Error::Invalid("Die glibc-Version konnte nicht geprüft werden."))
            })
            .collect()
    };
    require(
        parts(observed)? >= parts(string(&lock["native"], "minimum_glibc")?)?,
        "Dieses Laufzeitpaket benötigt eine neuere glibc-Version. Bitte ein passendes Linux-System verwenden.",
    )?;
    let ldconfig = process::which("ldconfig").unwrap_or_else(|| PathBuf::from("/sbin/ldconfig"));
    let libs = process::output(
        Command::new(ldconfig).arg("-p"),
        Duration::from_secs(5),
        1024 * 1024,
        &ctx.cancel,
    )?;
    let libs = String::from_utf8_lossy(&libs);
    require(
        [
            "libwebkit2gtk-4.1.so",
            "libgtk-3.so",
            "libssl.so",
            "libvulkan.so",
        ]
        .iter()
        .all(|name| libs.contains(name)),
        "Für die Anmeldung oder Grafik fehlen Linux-Bibliotheken. Bitte WebKitGTK 4.1, GTK3, OpenSSL und Vulkan über die Softwareverwaltung installieren.",
    )?;
    let media=process::which("gst-inspect-1.0").ok_or(Error::Invalid("Für die Videoprüfung fehlt gst-inspect-1.0. Bitte die GStreamer-Werkzeuge über die Softwareverwaltung installieren."))?;
    let work = tx::new_directory(&ctx.launcher.state_dir, ".media-check-")?;
    let result: Result<()> = (|| {
        for plugin in ["qtdemux", "h264parse", "avdec_h264"] {
            let mut c = Command::new(&media);
            for (key, _) in std::env::vars() {
                if key.starts_with("GST_PLUGIN_") || key.starts_with("GST_REGISTRY") {
                    c.env_remove(key);
                }
            }
            c.arg(plugin)
                .env("GST_PLUGIN_PATH", "")
                .env("GST_PLUGIN_PATH_1_0", "")
                .env("GST_REGISTRY_1_0", work.join("registry.bin"))
                .env("GST_DEBUG", "0")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            require(
                process::run(&mut c, Duration::from_secs(5), &ctx.cancel)?.success(),
                "Für die Videowiedergabe fehlen GStreamer-Module. Bitte GStreamer Good, Bad und Libav über die Softwareverwaltung installieren.",
            )?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&work);
    result?;
    if let Some(native) = bootstrap::native_path(&lock)? {
        require(
            process::run(
                Command::new(native.join("bin/xodus-cli"))
                    .arg("--help")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null()),
                Duration::from_secs(15),
                &ctx.cancel,
            )?
            .success(),
            "Xodus kann auf diesem Linux nicht starten. Bitte die GTK-, WebKitGTK- und OpenSSL-Laufzeitbibliotheken prüfen.",
        )?;
    }
    ctx.interrupted()?;
    Ok(
        json!({"mode":"install","game_id":game.id(),"market":market,"local_saves":local,"destination_path":destination}),
    )
}
pub fn suggested_market() -> String {
    let zone = std::env::var("TZ")
        .ok()
        .filter(|v| !v.starts_with(':') && v.contains('/'))
        .or_else(|| {
            fs::read_link("/etc/localtime").ok().and_then(|p| {
                p.to_str()
                    .and_then(|p| p.split_once("/zoneinfo/").map(|v| v.1.to_string()))
            })
        });
    if let Some(zone) = zone
        && let Ok(bytes) =
            files::read_public(Path::new("/usr/share/zoneinfo/zone1970.tab"), 256 * 1024)
    {
        for line in String::from_utf8_lossy(&bytes)
            .lines()
            .filter(|v| !v.starts_with('#'))
        {
            let v: Vec<_> = line.split('\t').collect();
            if v.len() >= 3 && v[2] == zone && market(v[0]).is_ok() {
                return v[0].into();
            }
        }
    }
    let re = regex::Regex::new(
        r"^[A-Za-z]{2,3}(?:_[A-Za-z]{4})?_([A-Z]{2})(?:\.[A-Za-z0-9_-]+)?(?:@[A-Za-z0-9_-]+)?$",
    )
    .expect("constant locale regex");
    for key in ["LC_ADDRESS", "LC_MONETARY", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(key)
            && v.len() <= 256
            && let Some(v) = re.captures(&v)
        {
            return v[1].into();
        }
    }
    String::new()
}
pub fn picker() -> Option<(&'static str, PathBuf)> {
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return None;
    }
    ["zenity", "kdialog"]
        .iter()
        .find_map(|n| process::which(n).map(|p| (*n, p)))
}
pub fn snapshot(app: &Launcher) -> Value {
    let s = app.lock();
    let root = s.runtime.as_deref();
    let game = root
        .and_then(|r| Game::for_runtime(r).ok())
        .unwrap_or(Game::Msfs2024);
    let capability = bootstrap::availability();
    let artifacts = resources::source_root()
        .map(|p| p.join("build/compat/artifacts"))
        .filter(|p| p.join("manifest.json").is_file());
    let job = s.jobs.get("setup").cloned().unwrap_or(Value::Null);
    json!({"available":true,"prepare_available":true,"directory_picker":picker().is_some(),"install_available":capability["available"],"install_unavailable_reason":capability["reason"],"prepare_unavailable_reason":"","state":job["state"].as_str().unwrap_or("idle"),"job":job,"defaults":{"mode":if root.is_some(){"existing"}else{"install"},"runtime_path":root.map(|v|v.to_string_lossy().into_owned()).unwrap_or_default(),"artifacts_path":artifacts.map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),"game_path":root.and_then(|r|game.path(r).canonicalize().ok()).map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),"runner_path":root.and_then(|r|r.join("runner").canonicalize().ok()).map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),"prefix_path":root.map(|r|r.join("local/msfs-prefix").to_string_lossy().into_owned()).unwrap_or_default(),"destination_path":default_path(game),"game_id":game.id(),"market":suggested_market(),"local_saves":true,"media_plugins_path":""}})
}
pub fn discover(app: &Launcher) -> Value {
    let s = app.lock();
    let selected = s.runtime.clone();
    let mut candidates: Vec<_> = selected
        .iter()
        .cloned()
        .chain(s.known.values().cloned())
        .collect();
    drop(s);
    if let Some(source) = resources::source_root()
        && let Some(parent) = source.parent()
    {
        candidates.push(parent.join("msfs-linux"));
        candidates.push(parent.join("msfs-linux/runtime"));
    }
    let mut limited = false;
    if let Ok(entries) =
        fs::read_dir(files::xdg("XDG_DATA_HOME", ".local/share").join("flightdeck/runtimes"))
    {
        for (i, entry) in entries.take(33).enumerate() {
            if i == 32 {
                limited = true;
                break;
            }
            if let Ok(entry) = entry
                && entry.path().is_dir()
            {
                candidates.push(entry.path());
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    let mut checked = 0;
    for candidate in candidates {
        if let Ok(path) = candidate.canonicalize() {
            if !seen.insert(path.clone()) {
                continue;
            }
            checked += 1;
            if runtime::validate(&path.to_string_lossy()).is_err() {
                continue;
            }
            let checks = runtime::checks(Some(&path));
            let game = Game::for_runtime(&path).ok();
            found.push(json!({"name":game.map(Game::name).unwrap_or("Unbekannte MSFS-Version"),"game_id":game.map(Game::id).unwrap_or(""),"path":path,"ready":checks.iter().all(|v|v["ok"]==true),"configured":selected.as_ref()==Some(&path),"checks":checks}));
        }
    }
    found.sort_by_key(|v| {
        (
            v["configured"] != true,
            v["ready"] != true,
            v["path"].as_str().unwrap_or("").to_string(),
        )
    });
    json!({"ok":true,"runtimes":found,"checked_count":checked,"limited":limited})
}
pub fn check(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let mode = string(data, "mode")?;
    require(
        ["existing", "prepare", "install", "update"].contains(&mode),
        "Bitte vorhandene Runtime oder neue Einrichtung wählen.",
    )?;
    let operation = data["operation"].as_str().unwrap_or("update");
    require(
        mode != "update" || ["update", "repair", "verify"].contains(&operation),
        "Ungültige Updateanfrage.",
    )?;
    let ctx = app.reserve("setup", "check", mode == "update")?;
    app.lock().plans.remove("setup");
    ctx.update(json!({"state":"checking","mode":mode,"operation":operation,"phase":"paths","checks":[],"progress":null,"can_pause":false,"can_resume":false,"transfer":null,"market":data["market"],"game_id":data["game_id"].as_str().unwrap_or("msfs2024")}));
    let data = data.clone();
    let job = app.job("setup");
    let retain = mode != "update";
    std::thread::spawn(move || {
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<Value>{
            if data["mode"]=="update"&&data["operation"]=="verify"{
                let root=ctx.root()?;let game=Game::for_runtime(root)?;ctx.update(json!({"phase":"integrity_check"}));
                let report=crate::integrity::verify(&game.path(root),&ctx.cancel,|r|ctx.update(json!({"progress":(r.checked*100).checked_div(r.total).unwrap_or(0)})))?;
                return Ok(json!({"state":"complete","phase":"complete","integrity_result":report,"progress":100,"message":if report.healthy{"Dateiprüfung abgeschlossen. Alle geprüften Dateien stimmen überein."}else{"Dateiprüfung abgeschlossen. Fehlende, veränderte oder unlesbare Dateien wurden gefunden."}}));
            }
            let plan=match data["mode"].as_str(){Some("existing")=>{let path=runtime::validate(string(&data,"runtime_path")?)?;let checks=runtime::checks(Some(&path));ctx.update(json!({"checks":checks}));require(checks.iter().all(|v|v["ok"]==true),"Die ausgewählte Runtime ist noch nicht startbereit.")?;json!({"mode":"existing","runtime_path":path})},Some("prepare")=>preflight(&data,&ctx)?,Some("update")=>crate::game_update::check(&ctx,&data)?,_=>preflight_install(&data,&ctx)?};
            ctx.interrupted()?;ctx.launcher.lock().plans.insert("setup".into(),plan.clone());
            if plan["mode"]=="update"{
                let newer=game_package::version(string(&plan["latest"],"version")?)?>game_package::version(string(&plan["current"],"version")?)?;let repair=data["operation"]=="repair";let ready=newer||repair;
                return Ok(json!({"state":if ready{"ready"}else{"complete"},"phase":if ready{"ready"}else{"complete"},"installed_version":plan["current"]["version"],"latest_version":plan["latest"]["version"],"update_available":newer,"auth_required":false,"progress":100,"message":if repair{"Die vollständige Reparatur ist vorbereitet. Das Store-Basispaket wird nach Bestätigung neu heruntergeladen."}else if newer{"Eine neue Spielversion ist verfügbar. Das Update kann gestartet werden."}else{"Die installierte Spielversion ist aktuell."}}));
            }
            Ok(json!({"state":"ready","phase":"ready","message":"Prüfung abgeschlossen. Die Einrichtung kann gestartet werden.","progress":100,"runtime_path":if plan["mode"]=="existing"{plan["runtime_path"].clone()}else{plan["destination_path"].clone()},"market":plan["market"],"prefix_bytes":plan["prefix_bytes"]}))
        })).unwrap_or(Err(Error::Invalid("Die Einrichtung konnte nicht geprüft werden.")));
        ctx.launcher.finish(&ctx, result, retain);
    });
    Ok(json!({"ok":true,"job":job}))
}
pub fn start(app: &Arc<Launcher>, id: &str) -> Result<Value> {
    let ctx = app.continuation("setup", id)?;
    let plan = app
        .lock()
        .plans
        .get("setup")
        .cloned()
        .ok_or(Error::Invalid(
            "Bitte die ausgewählten Dateien zuerst erneut prüfen.",
        ))?;
    let job = app.job("setup");
    std::thread::spawn(move || {
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<Value>{
            if plan["mode"]=="update" {
                crate::game_update::install(&ctx,&plan)?;
                return Ok(json!({"state":"complete","phase":"complete","message":if plan["operation"]=="repair"{"Reparatur abgeschlossen. Das neu heruntergeladene Basispaket ist aktiv; der vorherige Stand bleibt erhalten."}else{"Spielupdate abgeschlossen. Die vorherige Version bleibt für eine Rückkehr erhalten."},"progress":100,"can_pause":false,"can_resume":false,"update_available":false,"transfer":null}));
            }
            let path=match plan["mode"].as_str(){
                Some("existing")=>{let p=runtime::validate(string(&plan,"runtime_path")?)?;require(runtime::ready(&p),"Die ausgewählte Runtime ist noch nicht startbereit.")?;p},
                Some("prepare")=>{let plan=preflight(&plan,&ctx)?;prepare(&plan,&ctx,None)?},
                Some("install")=>{let plan=preflight_install(&plan,&ctx)?;ctx.update(json!({"phase":"bootstrap"}));let built=bootstrap::obtain(&ctx,Path::new(string(&plan,"destination_path")?))?;
                    let work=PathBuf::from(string(&built,"workspace")?);let game=chosen(&plan)?;let game_path=work.join("game");let xdg=work.join("xdg");
                    crate::game_install::download(crate::game_install::Request{cli:Path::new(string(&built,"cli")?),expected:string(&built,"cli_sha256")?,destination:&game_path,market:string(&plan,"market")?,xdg:&xdg,features:&built["cli_features"],sign_in:true,expected_package:None,game},&ctx)?;
                    ctx.update(json!({"phase":"provision"}));let mut inputs=plan.clone();for key in ["artifacts_path","runner_path","prefix_path"]{inputs[key]=built[key].clone();}inputs["game_path"]=json!(game_path);inputs["mode"]=json!("prepare");let prepared=preflight(&inputs,&ctx)?;prepare(&prepared,&ctx,Some(&xdg))?
                },_=>return Err(Error::Invalid("Ungültiger Einrichtungsplan."))};
            ctx.launcher.activate(&ctx,&path)?;Ok(json!({"state":"complete","phase":"complete","message":"Runtime eingerichtet und im Launcher ausgewählt.","progress":100,"runtime_path":path,"can_pause":false,"can_resume":false,"transfer":null}))
        })).unwrap_or(Err(Error::Invalid("Die Einrichtung wurde unerwartet beendet.")));
        ctx.launcher.finish(&ctx, result, false);
    });
    Ok(json!({"ok":true,"job":job}))
}
pub fn download_action(app: &Launcher, id: &str, operation: &str) -> Result<Value> {
    use std::sync::atomic::Ordering;
    let mut s = app.lock();
    Launcher::open(&s)?;
    let active = s
        .active
        .as_ref()
        .filter(|v| v.kind == "setup" && v.id == id && !v.cancel.load(Ordering::Relaxed))
        .ok_or(Error::Invalid(
            "Es gibt keine passende laufende Einrichtung.",
        ))?;
    let job = s.jobs.get("setup").ok_or(Error::Invalid(
        "Es gibt keine passende laufende Einrichtung.",
    ))?;
    require(
        job["state"] == "installing"
            && match operation {
                "pause" => job["can_pause"] == true,
                "resume" => job["can_resume"] == true,
                _ => false,
            },
        "Dieser Download kann gerade nicht pausiert oder fortgesetzt werden.",
    )?;
    active.pause.store(operation == "pause", Ordering::Relaxed);
    let job = s.jobs.get_mut("setup").ok_or(Error::Invalid(
        "Es gibt keine passende laufende Einrichtung.",
    ))?;
    job["can_pause"] = json!(false);
    job["can_resume"] = json!(false);
    job["phase"] = json!(if operation == "pause" {
        "pausing"
    } else {
        "download"
    });
    job["message"] = json!(if operation == "pause" {
        "Download wird pausiert. Der laufende Teil wird sicher beendet …"
    } else {
        "Der Download wird fortgesetzt …"
    });
    Ok(json!({"ok":true,"job":job}))
}
pub static PICKER: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn pick(app: &Launcher, data: &Value, locale: &str) -> Result<Value> {
    let field = string(data, "field")?;
    require(
        [
            "runtime_path",
            "artifacts_path",
            "game_path",
            "runner_path",
            "prefix_path",
            "destination_path",
            "media_plugins_path",
        ]
        .contains(&field),
        "Dieses Verzeichnisfeld wird nicht unterstützt.",
    )?;
    {
        let s = app.lock();
        Launcher::open(&s)?;
        require(
            s.active.is_none(),
            "Bitte die laufende Einrichtung zuerst abschließen oder abbrechen.",
        )?;
    }
    let _guard = PICKER
        .try_lock()
        .map_err(|_| Error::Invalid("Ein Verzeichnisdialog ist bereits geöffnet."))?;
    let (name, program) = picker().ok_or(Error::Invalid(
        "Kein Verzeichnisdialog verfügbar. Bitte den Pfad direkt eingeben.",
    ))?;
    let defaults = snapshot(app);
    let initial = data["initial"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            defaults["defaults"][field]
                .as_str()
                .filter(|v| !v.is_empty())
        });
    let home = files::home();
    let raw = initial.unwrap_or_else(|| home.to_str().unwrap_or("/"));
    require(
        raw.len() <= 4096 && !raw.chars().any(char::is_control),
        "Der Ausgangspfad ist ungültig.",
    )?;
    let mut folder = files::expand(raw);
    require(folder.is_absolute(), "Der Ausgangspfad muss absolut sein.")?;
    while !folder.is_dir() {
        if !folder.pop() {
            return Err(Error::Invalid("Der Ausgangspfad ist ungültig."));
        }
    }
    let title = if locale == "en" {
        "Flightdeck – Select folder"
    } else {
        "Flightdeck – Ordner auswählen"
    };
    let mut command = Command::new(program);
    if name == "zenity" {
        command
            .args(["--file-selection", "--directory"])
            .arg(format!("--title={title}"))
            .arg(format!("--filename={}/", folder.display()));
    } else {
        command
            .arg("--getexistingdirectory")
            .arg(folder)
            .args(["--title", title]);
    }
    let result = process::output(
        &mut command,
        Duration::from_secs(120),
        4098,
        &AtomicBool::new(false),
    );
    let path = result
        .ok()
        .and_then(|v| String::from_utf8(v).ok())
        .map(|v| v.trim_end_matches(['\r', '\n']).to_string())
        .filter(|v| {
            !v.is_empty() && v.len() <= 4096 && Path::new(v).is_absolute() && Path::new(v).is_dir()
        });
    Ok(if let Some(path) = path {
        json!({"ok":true,"cancelled":false,"field":field,"path":path})
    } else {
        json!({"ok":true,"cancelled":true,"field":field})
    })
}
