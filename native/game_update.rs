// SPDX-License-Identifier: MIT
//! Store revision-bound downloads and atomic activation with retained rollback.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    bootstrap,
    error::require,
    files, game_install,
    game_package::{self, Identity},
    games::Game,
    integrity, mods, process, resources, setup, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
const INVALID: &str = "Die Paketantwort enthält keine gültige, eindeutige Updateversion.";
pub fn tools(root: &Path, verify: bool) -> Result<(PathBuf, String, Value)> {
    let lock = resources::json("compat/bootstrap.lock.json")?;
    let spec = &lock["native"];
    let features = &spec["cli_features"];
    require(
        ["package-info-json-v1", "streaming-resume-files-v1"]
            .iter()
            .all(|name| {
                features
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == name))
            }),
        "Diese Flightdeck-Komponenten unterstützen noch keine sicheren Spielupdates. Bitte Flightdeck aktualisieren.",
    )?;
    let expected = string(&spec["files"], "bin/xodus-cli")?;
    let mut choices = vec![root.join("bin/xodus-cli")];
    if let Some(packaged) = bootstrap::native_path(&lock)? {
        choices.insert(0, packaged.join("bin/xodus-cli"));
    }
    for path in choices {
        if (!verify && path.is_file()) || game_install::verify_cli(&path, expected).is_ok() {
            return Ok((path, expected.into(), features.clone()));
        }
    }
    Err(Error::Invalid(
        "Der geprüfte Update-Downloader fehlt. Bitte das vollständige aktuelle Flightdeck-Paket installieren.",
    ))
}
pub fn configured_market(root: &Path) -> Result<String> {
    let path = root.join("private/runtime.json");
    if files::exists(&path) {
        let config: Value = files::json(&path, 65536)?;
        return Ok(setup::market(string(&config, "market")?)?.into());
    }
    let bytes = files::read(&root.join("tools/launch-msfs.sh"), 128 * 1024)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Invalid("Die konfigurierte Store-Region ist ungültig."))?;
    let re = regex::Regex::new(r"(?m)^[ \t]*(export[ \t]+)?XODUS_STORE_MARKET=(.*)$")
        .expect("constant market regex");
    let found: Vec<_> = re.captures_iter(text).collect();
    require(
        found.len() == 1 && found[0].get(1).is_some(),
        "Für diese ältere Runtime fehlt eine eindeutige Store-Region. Bitte die Runtimekonfiguration prüfen.",
    )?;
    let re = regex::Regex::new(r#"^(?:([A-Z]{2})|"([A-Z]{2})"|'([A-Z]{2})')(?:[ \t]+#.*|[ \t]*)$"#)
        .expect("constant market literal regex");
    let value = re.captures(&found[0][2]).ok_or(Error::Invalid(
        "Die konfigurierte Store-Region ist ungültig.",
    ))?;
    Ok(value
        .iter()
        .skip(1)
        .flatten()
        .next()
        .ok_or(Error::Invalid(INVALID))?
        .as_str()
        .into())
}
pub fn validate_info(value: &Value, game: Game) -> Result<()> {
    let keys = [
        "schema",
        "store_id",
        "version",
        "version_id",
        "content_id",
        "package_identity",
        "size_bytes",
    ];
    require(
        value
            .as_object()
            .is_some_and(|v| v.len() == keys.len() && keys.iter().all(|k| v.contains_key(*k)))
            && value["schema"].as_u64() == Some(1)
            && value["store_id"] == game.store_id(),
        INVALID,
    )?;
    let version = game_package::version(string(value, "version")?)?;
    let guid = |s: &str| {
        uuid::Uuid::parse_str(s).is_ok_and(|id| id.hyphenated().to_string() == s.to_lowercase())
    };
    require(guid(string(value, "content_id")?), INVALID)?;
    let revision = string(value, "version_id")?;
    require(revision.len() <= 128, INVALID)?;
    if !guid(revision) {
        let (prefix, suffix) = revision.rsplit_once('.').ok_or(Error::Invalid(INVALID))?;
        require(
            game_package::version(prefix)? == version && guid(suffix),
            INVALID,
        )?;
    }
    require(
        files::hex_digest(string(value, "package_identity")?)
            && value["size_bytes"]
                .as_u64()
                .is_some_and(|v| v > 0 && v < 8 * 1024_u64.pow(4)),
        INVALID,
    )
}
fn private(path: &Path) -> Result<()> {
    let dir = files::directory(path, false)?;
    require(
        dir.metadata()?.mode() & 0o022 == 0,
        "Der Updateordner muss ein eigener, nicht gemeinsam beschreibbarer Ordner sein.",
    )
}
fn no_user_packages(root: &Path, target: &Path) -> Result<()> {
    let (locations, limited) = mods::locations(root)?;
    require(
        !limited && !locations.iter().any(|(p, _)| p.starts_with(target)),
        "Community- oder Benutzerpakete liegen im Spielpaket. Diese bitte zuerst im Spiel in einen separaten Paketordner verschieben.",
    )
}
pub fn check(ctx: &Context, data: &Value) -> Result<Value> {
    let root = ctx.root()?;
    private(&root.join("private"))?;
    private(&root.join("games"))?;
    let (cli, hash, features) = tools(root, true)?;
    if data["operation"] == "repair" {
        require(
            features
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v == "streaming-integrity-index-v1")),
            "Für eine vollständige Reparatur mit Prüfnachweis bitte das aktuelle Flightdeck-Paket installieren.",
        )?;
    }
    let game = Game::for_runtime(root)?;
    let current = integrity::installed_identity(&game.path(root), game)?;
    let target = game.path(root).canonicalize()?;
    no_user_packages(root, &target)?;
    let market = configured_market(root)?;
    let sign_in = data
        .get("sign_in")
        .map(Value::as_bool)
        .unwrap_or(Some(false))
        .ok_or(Error::Invalid("Ungültige Updateanfrage."))?;
    let xdg = root.join("private/xdg");
    if sign_in {
        ctx.update(json!({"phase":"authentication"}));
        ctx.progress("Bitte im Microsoft-Fenster mit dem Konto anmelden, das MSFS besitzt.");
        match game_install::run_cli(
            &cli,
            &["login".into()],
            &root.join("private"),
            Some(&xdg),
            ctx,
            game_install::Options {
                timeout: Some(Duration::from_secs(900)),
                ..game_install::Options::default()
            },
        )? {
            game_install::Exit::Code(code) => {
                game_install::login_result(code).map_err(|_|Error::AuthRequired("Die Microsoft-Anmeldung wurde nicht abgeschlossen. Bitte erneut versuchen."))?;
            }
            _ => return Err(Error::Invalid("Ungültiger Anmeldestatus.")),
        }
    }
    ctx.update(json!({"phase":"package_check"}));
    ctx.progress("Installierte Spielversion und aktuelle Store-Paketversion werden geprüft …");
    let latest = package_info(&cli, &hash, root, game, &market, &ctx.cancel)?;
    require(
        game_package::version(string(&latest, "version")?)?
            >= game_package::version(&current.version)?,
        "Der Store liefert eine ältere Spielversion. Es wird kein Downgrade durchgeführt.",
    )?;
    Ok(
        json!({"mode":"update","runtime_path":root,"current":current,"game_target":target,"latest":latest,"cli":cli,"cli_hash":hash,"features":features,"market":market,"operation":data["operation"].as_str().unwrap_or("update")}),
    )
}
pub fn package_info(
    cli: &Path,
    hash: &str,
    root: &Path,
    game: Game,
    market: &str,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    private(&root.join("private"))?;
    let xdg = root.join("private/xdg");
    game_install::verify_cli(cli, hash)?;
    let mut command = Command::new(cli);
    command
        .args(["package-info", game.store_id(), "--market", market])
        .current_dir(root.join("private"));
    game_install::environment(&mut command, Some(&xdg));
    let (status, bytes) =
        process::output_status(&mut command, Duration::from_secs(90), 8192, cancel)?;
    if status.code() == Some(77) {
        return Err(Error::AuthRequired(
            "Für die Updateprüfung ist eine erneute Microsoft-Anmeldung erforderlich.",
        ));
    }
    require(
        status.success(),
        "Die Updateprüfung konnte nicht abgeschlossen werden. Verbindung prüfen und erneut versuchen.",
    )?;
    let latest = crate::cloud::json(&bytes)?;
    validate_info(&latest, game)?;
    Ok(latest)
}
fn history_record(root: &Path, path: &Path) -> Result<(Value, PathBuf)> {
    let game = Game::for_runtime(root)?;
    let value: Value = files::json(path, 16384)?;
    let name = string(&value, "previous_entry")?;
    let prefix = format!(".{}-before-", game.directory());
    require(
        value["format"] == 1
            && name.strip_prefix(&prefix).is_some_and(|s| {
                s.len() == 32
                    && s.bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            }),
        "Ungültiger Rückfallstand.",
    )?;
    let previous = root.join("games").join(name);
    let target = PathBuf::from(string(&value, "new_target")?);
    require(
        target.is_absolute()
            && target.starts_with(root.join("private/game-updates"))
            && target.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            }),
        "Ungültiger Rückfallstand.",
    )?;
    let active = game.path(root);
    require(
        fs::symlink_metadata(&active)?.is_symlink()
            && active.canonicalize()? == target
            && previous.exists(),
        "Der Rückfallstand passt nicht zur aktiven Spielversion.",
    )?;
    require(
        serde_json::to_value(integrity::installed_identity(&active, game)?)?
            == value["new_identity"]
            && serde_json::to_value(integrity::installed_identity(&previous, game)?)?
                == value["old_identity"],
        "Der Rückfallstand passt nicht zur aktiven Spielversion.",
    )?;
    Ok((value, previous))
}
fn history(root: &Path) -> Result<(Value, PathBuf)> {
    for name in ["game-update-pending.json", "game-update.json"] {
        if let Ok(value) = history_record(root, &root.join("private").join(name)) {
            return Ok(value);
        }
    }
    Err(Error::Invalid(
        "Die vorherige Spielversion ist nicht eindeutig verfügbar. Es wurde nichts gelöscht.",
    ))
}
fn still_current(ctx: &Context, plan: &Value) -> Result<()> {
    let root = ctx.root()?;
    let game = Game::for_runtime(root)?;
    require(
        ctx.launcher.lock().runtime.as_deref() == Some(root)
            && game.path(root).canonicalize()? == Path::new(string(plan, "game_target")?)
            && serde_json::to_value(integrity::installed_identity(&game.path(root), game)?)?
                == plan["current"],
        "Das installierte Spiel hat sich geändert. Bitte Updates erneut prüfen.",
    )?;
    no_user_packages(root, &game.path(root).canonicalize()?)
}
pub fn install(ctx: &Context, plan: &Value) -> Result<PathBuf> {
    let root = ctx.root()?;
    let game = Game::for_runtime(root)?;
    private(&root.join("private"))?;
    private(&root.join("games"))?;
    still_current(ctx, plan)?;
    validate_info(&plan["latest"], game)?;
    let need = plan["latest"]["size_bytes"]
        .as_u64()
        .ok_or(Error::Invalid(INVALID))?
        * 2
        + 1024 * 1024 * 1024;
    require(
        tx::free_bytes(&root.join("private"))? >= need,
        "Für die neue Spielversion und die erhaltene Rückfallversion fehlt freier Speicherplatz.",
    )?;
    let updates = root.join("private/game-updates");
    files::private_dir(&updates)?;
    let work = tx::new_directory(&updates, "update-")?;
    ctx.update(json!({"workspace_path":work}));
    let mut xdg = root.join("private/xdg");
    if fs::symlink_metadata(&xdg).is_ok_and(|m| m.is_symlink()) {
        xdg = xdg.canonicalize()?;
    }
    files::private_dir(&xdg)?;
    let downloaded = game_install::download(
        game_install::Request {
            cli: Path::new(string(plan, "cli")?),
            expected: string(plan, "cli_hash")?,
            destination: &work.join("game"),
            market: string(plan, "market")?,
            xdg: &xdg,
            features: &plan["features"],
            sign_in: false,
            expected_package: Some(string(&plan["latest"], "package_identity")?),
            game,
        },
        ctx,
    )?;
    ctx.update(json!({"phase":"verify_update"}));
    ctx.progress("Die heruntergeladene Spielversion wird vor dem Wechsel geprüft …");
    let identity = game_package::installed(&downloaded, game)?;
    let mut expected: Identity = serde_json::from_value(plan["current"].clone())?;
    expected.version = string(&plan["latest"], "version")?.into();
    require(
        identity == expected,
        "Das heruntergeladene Paket passt nicht zur geprüften Spielidentität und Version. Die bisherige Version bleibt aktiv.",
    )?;
    ctx.interrupted()?;
    still_current(ctx, plan)?;
    let previous = root.join("games").join(format!(
        ".{}-before-{}",
        game.directory(),
        uuid::Uuid::new_v4().simple()
    ));
    std::os::unix::fs::symlink(&downloaded, &previous)?;
    let record = root.join("private/game-update.json");
    let pending = root.join("private/game-update-pending.json");
    let mut old_record = if files::exists(&record) {
        Some(files::json::<Value>(&record, 16384)?)
    } else {
        None
    };
    if let Ok((recovered, _)) = history(root)
        && old_record.as_ref() != Some(&recovered)
    {
        files::atomic_json(&record, &recovered)?;
        old_record = Some(recovered);
    }
    ctx.interrupted()?;
    // From this point the journal and exchange finish as one noncancellable commit.
    ctx.begin_commit(
        "switch_update",
        "Die geprüfte Spielversion wird atomar aktiviert …",
    )?;
    let history = json!({"format":1,"previous_entry":previous.file_name().and_then(|v|v.to_str()),"new_target":downloaded,"old_identity":plan["current"],"new_identity":identity,"package":plan["latest"]});
    let mut swapped = false;
    let result = (|| -> Result<()> {
        files::atomic_json(&pending, &history)?;
        tx::exchange(&previous, &game.path(root))?;
        swapped = true;
        files::atomic_json(&record, &history)?;
        fs::remove_file(&pending)?;
        files::directory(&root.join("private"), true)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        if swapped {
            tx::exchange(&previous, &game.path(root))?;
        }
        if let Some(value) = old_record {
            files::atomic_json(&record, &value)?;
        } else if files::exists(&record) {
            fs::remove_file(&record)?;
        }
        if files::exists(&pending) {
            fs::remove_file(&pending)?;
        }
        files::directory(&root.join("private"), true)?.sync_all()?;
    }
    result?;
    Ok(root.into())
}
pub fn rollback(app: &Arc<Launcher>) -> Result<Value> {
    let ctx = app.reserve("game-rollback", "rollback", true)?;
    let result = (|| {
        let root = ctx.root()?;
        let (_, previous) = history(root)?;
        tx::exchange(&previous, &Game::for_runtime(root)?.path(root))?;
        let mut s = app.lock();
        s.plans.remove("setup");
        if s.jobs.get("setup").is_some_and(|v| v["mode"] == "update") {
            s.jobs.remove("setup");
        }
        Ok(
            json!({"ok":true,"message":"Die vorherige Spielversion wurde wieder aktiviert. Spielstände und Add-ons wurden nicht verändert."}),
        )
    })();
    match result {
        Ok(value) => {
            app.finish(&ctx, Ok(json!({"state":"complete"})), false);
            Ok(value)
        }
        Err(error) => {
            app.finish(&ctx,Err(Error::Invalid("Die vorherige Spielversion ist nicht eindeutig verfügbar. Es wurde nichts gelöscht.")),false);
            Err(error)
        }
    }
}
pub fn snapshot(app: &Launcher) -> Value {
    let mut s = app.lock();
    Launcher::poll(&mut s);
    let root = s.runtime.clone();
    let busy = s.active.is_some() || s.process.is_some() || s.closing || Launcher::external(&s);
    let job = s.jobs.get("setup").cloned().unwrap_or(Value::Null);
    let discovered = root
        .as_ref()
        .and_then(|root| s.startup_updates.records.get(root))
        .map(|record| record.value.clone())
        .unwrap_or(Value::Null);
    drop(s);
    let mut value = json!({"available":false,"unavailable_reason":"Zuerst eine Runtime auswählen.","installed_version":null,"latest_version":null,"update_available":null,"can_check":false,"can_start":false,"can_rollback":false,"auth_required":false,"job":null,"can_repair":false,"integrity":{"available":false,"unavailable_reason":"Zuerst eine Runtime auswählen.","can_check":false,"result":null}});
    let Some(root) = root else {
        return value;
    };
    let result = (|| -> Result<()> {
        let game = Game::for_runtime(&root)?;
        let usable = integrity::available(&game.path(&root));
        value["integrity"] = json!({"available":usable,"unavailable_reason":if usable{""}else{"Für diese Installation fehlt ein vollständiger Download-Prüfnachweis. Eine vollständige Reparatur erstellt ihn; vorhandene Dateien werden nicht als fehlerfreie Vorlage übernommen."},"can_check":usable&&!busy,"result":null});
        value["installed_version"] =
            json!(integrity::installed_identity(&game.path(&root), game)?.version);
        let (_, _, features) = tools(&root, false)?;
        value["available"] = json!(true);
        value["unavailable_reason"] = json!("");
        value["can_check"] = json!(!busy);
        value["can_repair"] = json!(
            !busy
                && features
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == "streaming-integrity-index-v1"))
        );
        Ok(())
    })();
    if let Err(error) = result {
        value["unavailable_reason"] = json!(error.to_string());
    }
    if job["mode"] == "update" && job["runtime_path"].as_str() == root.to_str() {
        value["job"] = job.clone();
        value["integrity"]["result"] = job["integrity_result"].clone();
        for key in ["latest_version", "update_available", "auth_required"] {
            if let Some(v) = job.get(key) {
                value[key] = v.clone();
            }
        }
        value["can_start"] = json!(
            value["available"] == true
                && !busy
                && job["state"] == "ready"
                && (job["update_available"] == true || job["operation"] == "repair")
        );
    }
    value["can_rollback"] = json!(!busy && history(&root).is_ok());
    if value["job"].is_null() && value["available"] == true {
        value["background_checking"] = json!(discovered["state"] == "checking");
        value["startup_error"] = json!(discovered["error"].as_str().unwrap_or(""));
        value["auth_required"] = json!(discovered["auth_required"] == true);
        if discovered["installed_version"] == value["installed_version"] {
            for key in ["latest_version", "update_available"] {
                if let Some(v) = discovered.get(key) {
                    value[key] = v.clone();
                }
            }
        }
    }
    value
}
