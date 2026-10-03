// SPDX-License-Identifier: MIT
//! Proton selection preserving the current aircraft/profile and durable recovery.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    bootstrap,
    error::require,
    fenix, fenix_bundle as bundle, fenix_setup, files, framework, process, runtime,
    transaction as tx,
    wine::StagedWine,
    wine_processes,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};
const SETTINGS: &str = "private/proton-selection.json";
const JOURNAL: &str = "private/proton-switch.json";
const BACKUP: &str = r"^local/proton-tests/[0-9a-f]{32}/previous-prefix$";
const RUNNER: &str =
    r"^local/(?:proton-tests/[0-9a-f]{32}|fenix-patch-[0-9T]+-[0-9a-f]{8})/runner$";
pub const BRIDGE: [&str; 3] = [
    "xgameruntime.dll",
    "xgameruntime_original.dll",
    "xodus_store_test.dll",
];
fn matches(pattern: &str, value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|v| regex::Regex::new(pattern).is_ok_and(|r| r.is_match(v)))
}
pub fn managed(root: &Path, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("Invalid managed Proton directory."))?;
    let mut parent = files::directory(root, false)?;
    for part in relative.components() {
        require(
            matches!(part, std::path::Component::Normal(_)),
            "Invalid managed Proton directory.",
        )?;
        parent = files::open_at(&parent, part.as_os_str(), true, false)?;
    }
    Ok(())
}
pub fn validate_selection(value: &Value) -> Result<()> {
    require(
        value.is_object()
            && [Some(1), Some(2)].contains(&value["schema"].as_u64())
            && matches(RUNNER, &value["runner"])
            && matches(BACKUP, &value["base_prefix"])
            && value["base_runner"]
                .as_str()
                .is_some_and(|v| Path::new(v).is_absolute())
            && value["version"].as_str().is_some_and(|v| v.len() <= 256)
            && value["base_hashes"].as_object().is_some_and(|v| {
                v.len() == 3
                    && BRIDGE.iter().all(|name| {
                        v.get(*name)
                            .and_then(Value::as_str)
                            .is_some_and(files::hex_digest)
                    })
            }),
        "Invalid Proton selection.",
    )
}
pub fn selection(root: &Path) -> Result<Option<Value>> {
    if !files::exists(&root.join(SETTINGS)) {
        return Ok(None);
    }
    let value = runtime::value(&root.join(SETTINGS))?;
    validate_selection(&value)?;
    Ok(Some(value))
}
pub fn check(root: &Path) -> Result<()> {
    require(
        !files::exists(&root.join(JOURNAL)),
        "Proton-Wechsel unterbrochen. Unter Proton-Version zu Flightdeck zurückkehren.",
    )?;
    if let Some(selected) = selection(root)? {
        require(
            root.join("runner").canonicalize()? == root.join(string(&selected, "runner")?)
                && root
                    .join(string(&selected, "runner")?)
                    .join("files/bin/wine")
                    .is_file(),
            "Die Proton-Auswahl ist ungültig. Stelle unter Proton-Version die Flightdeck-Umgebung wieder her.",
        )?;
    }
    Ok(())
}
pub fn diagnostic(root: Option<&Path>) -> Value {
    let result = (|| -> Result<Value> {
        let Some(root) = root else {
            return Ok(json!({"mode":"flightdeck","version":"unknown","loader":"native"}));
        };
        let selected = selection(root)?;
        let version = selected
            .as_ref()
            .and_then(|v| v["version"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| version(&root.join("runner")));
        let version = if regex::Regex::new(
            r"^(?:experimental|GE-Proton|cachyos|xodus|Proton|proton|\d)[A-Za-z0-9_. +()-]{0,127}$",
        )
        .is_ok_and(|r| r.is_match(&version))
        {
            version
        } else {
            "custom".into()
        };
        Ok(
            json!({"mode":if selected.is_some(){"proton"}else{"flightdeck"},"version":version,"loader":if selected.is_some(){"portable"}else{"native"}}),
        )
    })();
    result.unwrap_or(json!({"mode":"unavailable","version":"unknown","loader":"unknown"}))
}
fn version(root: &Path) -> String {
    bundle::version(root).unwrap_or_else(|_| {
        root.file_name()
            .map(|v| v.to_string_lossy().chars().take(256).collect())
            .unwrap_or_default()
    })
}
pub fn inspect(raw: &str) -> Result<Value> {
    require(
        !raw.trim().is_empty() && raw.len() <= 4096,
        "Wähle einen installierten Proton-Ordner.",
    )?;
    let root = files::expand(raw).canonicalize()?;
    let mut folder = root.join("files");
    if !folder.is_dir() {
        folder = root.join("dist");
    }
    let mut names = vec!["bin/wine".into(), "bin/wineserver".into()];
    if folder.join("bin/wine64").is_file() {
        names.push("bin/wine64".into());
    }
    for arch in ["x86_64-windows", "i386-windows"] {
        for (library, dlls) in [
            ("dxvk", &["dxgi", "d3d11", "d3d10core"][..]),
            ("vkd3d-proton", &["d3d12", "d3d12core"][..]),
        ] {
            for name in dlls {
                names.push(format!("lib/wine/{library}/{arch}/{name}.dll"));
            }
        }
    }
    let mut hashes = BTreeMap::new();
    for name in names {
        let target = folder.join(&name).canonicalize()?;
        let info = target.metadata()?;
        require(
            target.starts_with(&root) && info.is_file() && info.len() <= 256 * 1024 * 1024,
            "Der Proton-Ordner enthält keine vollständige Wine- und Grafikumgebung.",
        )?;
        require(
            !name.starts_with("bin/") || info.mode() & 0o111 != 0,
            "Wine ist in diesem Proton-Ordner nicht ausführbar.",
        )?;
        hashes.insert(name, tx::digest(&target)?);
    }
    Ok(json!({"path":root,"files":folder,"version":version(&root),"hashes":hashes}))
}
pub fn discover(home: &Path) -> Vec<Value> {
    let mut roots: BTreeSet<_> = [
        ".local/share/Steam",
        ".steam/root",
        ".steam/steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .map(|v| home.join(v))
    .into_iter()
    .collect();
    if let Ok(pattern) = regex::Regex::new(r#""path"\s+"((?:[^"\\]|\\.)*)"#) {
        for root in roots.clone() {
            for name in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
                if let Ok(data) = files::read_public(&root.join(name), 1024 * 1024) {
                    for row in pattern
                        .captures_iter(&String::from_utf8_lossy(&data))
                        .take(256)
                    {
                        let path = PathBuf::from(row[1].replace(r"\\", r"\").replace("\\\"", "\""));
                        if path.is_absolute() {
                            roots.insert(path);
                        }
                    }
                }
            }
        }
    }
    let mut directories: BTreeSet<_> = roots
        .into_iter()
        .flat_map(|r| [r.join("steamapps/common"), r.join("compatibilitytools.d")])
        .collect();
    directories.insert("/usr/share/steam/compatibilitytools.d".into());
    let mut choices = BTreeMap::new();
    for dir in directories {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.take(256).flatten() {
            let Ok(root) = entry.path().canonicalize() else {
                continue;
            };
            let folder = if root.join("files").is_dir() {
                root.join("files")
            } else {
                root.join("dist")
            };
            if [
                "bin/wine",
                "bin/wineserver",
                "lib/wine/vkd3d-proton/x86_64-windows/d3d12.dll",
            ]
            .iter()
            .all(|p| folder.join(p).is_file())
            {
                choices.insert(root.clone(),json!({"path":root,"label":entry.file_name(),"version":version(&root),"fenix":bundle::runner_variant(&root,false).is_ok()}));
            }
        }
    }
    let mut choices: Vec<_> = choices.into_values().collect();
    choices.sort_by_key(|v| v["label"].as_str().unwrap_or("").to_lowercase());
    choices
}
pub fn bridge_hashes(root: &Path) -> Result<Value> {
    let record = runtime::value(&root.join("private/import-manifest.json"))?;
    let result = json!({"xgameruntime.dll":record["artifacts"]["files"]["runtime/xgameruntime.dll"],"xodus_store_test.dll":record["artifacts"]["files"]["builtin/x86_64-windows/xodus_store_test.dll"],"xgameruntime_original.dll":record["original_runtime_sha256"]});
    require(
        BRIDGE
            .iter()
            .all(|n| result[n].as_str().is_some_and(files::hex_digest)),
        "Die Store-Komponenten passen nicht zur Installation. Aktualisiere Flightdeck zuerst.",
    )?;
    Ok(result)
}
fn bridge(root: &Path) -> Result<(PathBuf, Value)> {
    let hashes = bridge_hashes(root)?;
    let system = tx::prefix_system32(&root.join("local/msfs-prefix"))?;
    for name in BRIDGE {
        require(
            tx::digest(&system.join(name))? == hashes[name],
            "Die Store-Komponenten passen nicht zur Installation. Aktualisiere Flightdeck zuerst.",
        )?;
    }
    Ok((system, hashes))
}
pub fn prepare(
    ctx: &Context,
    candidate: &Value,
    payload: Option<&Path>,
    restoring: bool,
) -> Result<(PathBuf, Value)> {
    let root = ctx.root()?;
    let previous = selection(root)?;
    let prefix = root.join("local/msfs-prefix");
    managed(root, &prefix)?;
    wine_processes::idle(&prefix)?;
    let original_runner = previous
        .as_ref()
        .and_then(|v| v["base_runner"].as_str())
        .map(PathBuf::from)
        .unwrap_or(root.join("runner").canonicalize()?);
    let (source, hashes) = match bridge(root) {
        Ok(value) => value,
        Err(error) => {
            if !restoring {
                return Err(error);
            }
            let previous = previous.as_ref().ok_or(error)?;
            let backup = root.join(string(previous, "base_prefix")?);
            managed(root, &backup)?;
            let system = tx::prefix_system32(&backup)?;
            let hashes = bridge_hashes(root)?;
            for name in BRIDGE {
                require(
                    tx::digest(&system.join(name))? == hashes[name],
                    "Die gesicherte Flightdeck-Umgebung wurde verändert. Sie wurde nicht überschrieben.",
                )?;
            }
            (system, hashes)
        }
    };
    let mut fenix_state = Value::Null;
    let mut variant = None;
    let marker = root.join(fenix::MARKER);
    let legacy = root.join("private/fenix-compat.json");
    if files::exists(&marker) || files::exists(&legacy) {
        require(
            payload.is_some(),
            "Das passende Fenix-Paket muss vor dem Proton-Wechsel verfügbar sein.",
        )?;
        if files::exists(&marker) {
            fenix_state = runtime::value(&marker)?;
            require(
                fenix_state["state"] == "installed",
                "Beende oder repariere zuerst die Fenix-Einrichtung.",
            )?;
            fenix::verify_installed(root, &fenix_state)?;
        } else {
            require(
                framework::status(&prefix).ready,
                "Die bestehende Fenix-Umgebung enthält kein vollständiges .NET Framework.",
            )?;
            fenix_state = json!({"format":1,"state":"installed","migrated":true,"configured":true,"proton_before":null});
        }
        let runner = Path::new(string(candidate, "path")?);
        variant = bundle::runner_variant(runner, false).or_else(|e| {
            if restoring {
                bundle::runner_variant(runner, true)
            } else {
                Err(e)
            }
        })?;
    }
    let tests = root.join("local/proton-tests");
    if !files::exists(&tests) {
        files::private_dir(&tests)?;
    }
    managed(root, &tests)?;
    let work = tx::new_directory(&tests, "")?;
    let runner = work.join("runner");
    files::private_dir(&runner)?;
    let fresh = work.join("previous-prefix");
    let mut scripts = json!({});
    let result = (|| {
        if !fenix_state.is_null() {
            let lock = bundle::manifest(None)?;
            let legacy_state = if files::exists(&legacy) {
                runtime::value(&legacy)?
            } else {
                Value::Null
            };
            for name in bundle::LAUNCH_FILES {
                let before = tx::digest(&root.join("tools").join(name))?;
                let known = bundle::accepted_script(name, &before, &lock, None)?
                    || lock["integration"][name] == before
                    || lock["previous_releases"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .any(|(_, v)| v["integration"][name] == before)
                    || legacy_state["deployed_scripts"][name] == before;
                require(
                    known,
                    "Ein Fenix-Startskript wurde angepasst. Die bestehende Installation wurde nicht verändert.",
                )?;
                tx::copy_path(
                    &root.join("tools").join(name),
                    &work.join(format!("before-{name}")),
                    Some(&before),
                    &ctx.cancel,
                )?;
                let bytes = bundle::script(name)?;
                crate::resources::ensure_helper(root)?;
                bundle::write(&work.join(name), bytes, 0o700)?;
                scripts[name] = json!({"before":before,"after":files::sha256(bytes),"source":work.join(name).strip_prefix(root).map_err(|_|Error::Invalid("Invalid Proton script path."))?});
            }
        }
        ctx.progress("Proton und Windows-Umgebung werden unabhängig kopiert …");
        process::copy_tree(
            Path::new(string(candidate, "files")?),
            &runner.join("files"),
            &ctx.cancel,
        )?;
        bundle::write(
            &runner.join("version"),
            format!("{}\n", string(candidate, "version")?).as_bytes(),
            0o600,
        )?;
        require(
            inspect(
                runner
                    .to_str()
                    .ok_or(Error::Invalid("Invalid Proton path."))?,
            )?["hashes"]
                == candidate["hashes"]
                && inspect(string(candidate, "path")?)?["hashes"] == candidate["hashes"],
            "Proton wurde während der Vorbereitung aktualisiert. Bitte erneut auswählen.",
        )?;
        process::copy_tree(&prefix, &fresh, &ctx.cancel)?;
        tx::relocate_prefix_links(&prefix, &fresh, &ctx.cancel)?;
        ctx.progress("Die neue Proton-Umgebung wird eingerichtet …");
        let mut command = bootstrap::wine_command(&runner, &fresh);
        command.env("WINEDLLOVERRIDES", "winemenubuilder.exe,mscoree,mshtml=d");
        for category in ["CONFIG", "DATA", "CACHE", "STATE"] {
            let path = work.join("xdg").join(category.to_lowercase());
            files::private_dir(&path)?;
            command.env(format!("XDG_{category}_HOME"), path);
        }
        bootstrap::command(command.args(["wineboot", "-u"]), &ctx.cancel)?;
        bootstrap::wait_wine(&runner, &fresh, &ctx.cancel)?;
        bootstrap::install_graphics(&runner, &fresh, &ctx.cancel)?;
        let system = tx::prefix_system32(&fresh)?;
        for name in BRIDGE {
            tx::copy_path(
                &source.join(name),
                &system.join(name),
                Some(string(&hashes, name)?),
                &ctx.cancel,
            )?;
        }
        bootstrap::wait_wine(&runner, &fresh, &ctx.cancel)?;
        bootstrap::stop_wine(&runner, &fresh)?;
        if !fenix_state.is_null() {
            ctx.progress("Fenix wird an die gewählte Proton-Version angepasst …");
            let wine = StagedWine::new(root, &fresh, &runner, &work.join("fenix-setup.log"), ctx)?;
            let prepared = fenix_setup::geometry(
                &wine,
                &root.join("private/fenix-downloads"),
                payload.ok_or(Error::Invalid("Missing Fenix bundle."))?,
                ctx,
            );
            let stopped = wine.stop();
            prepared.and(stopped)?;
            bundle::overlay(
                &fresh,
                &runner,
                payload.ok_or(Error::Invalid("Missing Fenix bundle."))?,
                variant.as_deref(),
                ctx,
            )?;
            fenix_state["variant"] = json!(variant);
            fenix_state["version"] = bundle::manifest(None)?["version"].clone();
            fenix_state["work"] = json!(
                work.strip_prefix(root)
                    .map_err(|_| Error::Invalid("Invalid Fenix staging path."))?
            );
            if fenix_state["migrated"] == true {
                fenix_state["backup"] = fenix_state["work"].clone();
            }
        }
        let selected = json!({"schema":if fenix_state.is_null(){1}else{2},"version":candidate["version"],"source":candidate["path"],"runner":runner.strip_prefix(root).map_err(|_|Error::Invalid("Invalid Proton runner path."))?,"base_runner":original_runner,"base_prefix":previous.as_ref().map(|v|v["base_prefix"].clone()).unwrap_or(json!(fresh.strip_prefix(root).map_err(|_|Error::Invalid("Invalid Proton backup path."))?)),"base_hashes":previous.as_ref().map(|v|v["base_hashes"].clone()).unwrap_or(hashes),"fenix_state":fenix_state,"scripts":scripts});
        files::atomic_json(
            &work.join("provenance.json"),
            &json!({"source":candidate["path"],"version":candidate["version"],"files":candidate["hashes"],"previous_runner":root.join("runner").canonicalize()?}),
        )?;
        ctx.interrupted()?;
        Ok((fresh.clone(), selected))
    })();
    // A failed profile stays available for local diagnosis. Never delete a tree
    // that might still be referenced by Wine after an unsuccessful shutdown.
    if result.is_err() && fresh.is_dir() {
        let _ = bootstrap::stop_wine(&runner, &fresh);
    }
    result
}
pub fn recover(root: &Path) -> Result<()> {
    let journal = root.join(JOURNAL);
    if !files::exists(&journal) {
        return Ok(());
    }
    let data = runtime::value(&journal)?;
    let runner = PathBuf::from(string(&data, "runner")?);
    require(
        data["schema"].as_u64() == Some(1)
            && matches(BACKUP, &data["backup"])
            && runner.is_absolute(),
        "Invalid Proton recovery record.",
    )?;
    if !data["selection"].is_null() {
        validate_selection(&data["selection"])?;
        require(
            runner == root.join(string(&data["selection"], "runner")?),
            "Inconsistent Proton recovery runner.",
        )?;
    }
    let fenix_state = &data["fenix_state"];
    if !fenix_state.is_null() {
        require(
            fenix_state["state"] == "installed"
                && matches(
                    RUNNER,
                    &json!(format!("{}/runner", string(fenix_state, "work")?)),
                )
                && root.join(string(fenix_state, "work")?).join("runner") == runner,
            "Invalid Fenix recovery record.",
        )?;
        bundle::manifest(fenix_state["variant"].as_str())?;
    }
    let relative = runner
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("Invalid prepared Proton runner."))?;
    require(
        matches(RUNNER, &json!(relative)),
        "Invalid prepared Proton runner.",
    )?;
    managed(root, &runner)?;
    let empty = serde_json::Map::new();
    let scripts = if data["scripts"].is_null() {
        &empty
    } else {
        data["scripts"]
            .as_object()
            .ok_or(Error::Invalid("Invalid Proton scripts."))?
    };
    require(
        scripts
            .keys()
            .all(|v| bundle::LAUNCH_FILES.contains(&v.as_str())),
        "Invalid Proton scripts.",
    )?;
    let mut prepared = Vec::new();
    for (name, item) in scripts {
        let source = string(item, "source")?;
        require(
            matches(
                &format!(
                    r"^local/proton-tests/[0-9a-f]{{32}}/{}$",
                    regex::escape(name)
                ),
                &json!(source),
            ),
            "Invalid Proton script path.",
        )?;
        managed(
            root,
            root.join(source)
                .parent()
                .ok_or(Error::Invalid("Invalid script path."))?,
        )?;
        let bytes = files::read(&root.join(source), 1024 * 1024)?;
        require(
            files::sha256(&bytes) == item["after"]
                && [item["before"].as_str(), item["after"].as_str()]
                    .contains(&Some(tx::digest(&root.join("tools").join(name))?.as_str())),
            "Prepared or active Proton script changed.",
        )?;
        prepared.push((name, bytes));
    }
    for key in ["before", "after"] {
        require(
            data[key]
                .as_array()
                .is_some_and(|v| v.len() == 2 && v.iter().all(|v| v.as_u64().is_some())),
            "Invalid Proton prefix identity.",
        )?;
    }
    let prefix = root.join("local/msfs-prefix");
    let backup = root.join(string(&data, "backup")?);
    for folder in [&prefix, &backup] {
        managed(root, folder)?;
        wine_processes::idle(folder)?;
    }
    require(
        root.join("runner").symlink_metadata()?.is_symlink()
            && [data["previous_runner"].as_str(), data["runner"].as_str()]
                .contains(&root.join("runner").canonicalize()?.to_str()),
        "Proton runner link changed.",
    )?;
    let mut imported = if scripts.is_empty() {
        Value::Null
    } else {
        runtime::value(&root.join("private/import-manifest.json"))?
    };
    if fenix::identity(&prefix)? == data["before"] && fenix::identity(&backup)? == data["after"] {
        tx::exchange(&prefix, &backup)?;
    } else {
        require(
            fenix::identity(&prefix)? == data["after"]
                && fenix::identity(&backup)? == data["before"],
            "Proton prefix identity changed.",
        )?;
    }
    fenix::replace_link(&root.join("runner"), &runner)?;
    if data["selection"].is_null() {
        if files::exists(&root.join(SETTINGS)) {
            fs::remove_file(root.join(SETTINGS))?;
        }
    } else {
        files::atomic_json(&root.join(SETTINGS), &data["selection"])?;
    }
    if !fenix_state.is_null() {
        files::atomic_json(&root.join(fenix::MARKER), fenix_state)?;
    }
    for (name, bytes) in prepared {
        bundle::write(&root.join("tools").join(name), &bytes, 0o700)?;
        if imported["runtime_files"].is_object() {
            imported["runtime_files"][name] = scripts[name]["after"].clone();
        }
    }
    if !scripts.is_empty() {
        files::atomic_json(&root.join("private/import-manifest.json"), &imported)?;
    }
    files::directory(&root.join("private"), false)?.sync_all()?;
    fs::remove_file(journal)?;
    files::directory(&root.join("private"), false)?.sync_all()?;
    Ok(())
}
pub fn switch(ctx: &Context, backup: &Path, prepared: &Value, restore: bool) -> Result<()> {
    let root = ctx.root()?;
    ctx.begin_commit("publish_runtime", "Proton-Umgebung wird aktiviert …")?;
    files::atomic_json(
        &root.join(JOURNAL),
        &json!({"schema":1,"backup":backup.strip_prefix(root).map_err(|_|Error::Invalid("Invalid Proton backup."))?,"runner":root.join(string(prepared,"runner")?),"previous_runner":root.join("runner").canonicalize()?,"before":fenix::identity(&root.join("local/msfs-prefix"))?,"after":fenix::identity(backup)?,"selection":if restore{Value::Null}else{prepared.clone()},"fenix_state":prepared["fenix_state"],"scripts":prepared["scripts"]}),
    )?;
    recover(root)
}
pub fn snapshot(app: &Launcher) -> Value {
    let (root, job) = {
        let s = app.lock();
        (
            s.runtime.clone(),
            s.jobs.get("proton").cloned().unwrap_or(Value::Null),
        )
    };
    let mut result = json!({"runtime_path":root,"selected":"Flightdeck (Xodus)","experimental":false,"can_restore":false,"error":"","fenix":false,"job":job});
    if let Some(root) = root {
        match selection(&root) {
            Ok(Some(selected)) => {
                result["selected"] = selected["version"].clone();
                result["experimental"] = json!(true);
                result["can_restore"] = json!(true);
            }
            Err(e) => {
                result["error"] = json!(e.to_string());
                result["can_restore"] = json!(true);
            }
            _ => {}
        }
        if let Err(e) = check(&root) {
            result["error"] = json!(e.to_string());
            result["can_restore"] = json!(true);
        }
        result["fenix"] = json!(
            [fenix::MARKER, "private/fenix-compat.json"]
                .iter()
                .any(|v| files::exists(&root.join(v)))
        );
    }
    result
}
pub fn start(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let root = app.root()?;
    require(
        crate::gsx::complete(&root),
        "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.",
    )?;
    require(
        Path::new(string(data, "runtime_path")?) == root,
        "Die ausgewählte Installation hat sich geändert. Bitte erneut auswählen.",
    )?;
    let mode = string(data, "mode")?;
    require(
        ["default", "proton"].contains(&mode),
        "Ungültige Proton-Auswahl.",
    )?;
    let mode = mode.to_string();
    let data = data.clone();
    let response=app.start_job("proton","select",true,move|ctx|{
        let root=ctx.root()?;require(root==Path::new(string(&data,"runtime_path")?),"Die ausgewählte Installation hat sich geändert. Bitte erneut auswählen.")?;ctx.update(json!({"state":"preparing","error":""}));wine_processes::idle(&root.join("local/msfs-prefix"))?;recover(root)?;
        if mode=="proton"&&files::exists(&root.join(fenix::MARKER)){require(runtime::value(&root.join(fenix::MARKER))?["state"]=="installed","Beende oder repariere zuerst die Fenix-Einrichtung.")?;}
        let mut candidate=if mode=="proton"{Some(inspect(string(&data,"path")?)?)}else{None};let has_fenix=[fenix::MARKER,"private/fenix-compat.json"].iter().any(|v|files::exists(&root.join(v)));let payload=if has_fenix{if let Some(c)=&candidate{bundle::runner_variant(Path::new(string(c,"path")?),false)?;}Some(bundle::obtain(&ctx.launcher.state_dir.join("fenix-bundles"),None,ctx)?)}else{None};
        if mode=="default"{if let Some(selected)=selection(root)?{let backup=root.join(string(&selected,"base_prefix")?);managed(root,&backup)?;let hashes=bridge_hashes(root)?;let system=tx::prefix_system32(&backup)?;
            for name in BRIDGE{let actual=tx::digest(&system.join(name))?;require(selected["base_hashes"][name]==actual||hashes[name]==actual,"Die gesicherte Flightdeck-Umgebung wurde verändert. Sie wurde nicht überschrieben.")?;}
            for name in BRIDGE{if tx::digest(&system.join(name))?!=hashes[name]{let(source,_)=bridge(root)?;tx::copy_path(&source.join(name),&system.join(name),Some(string(&hashes,name)?),&ctx.cancel)?;}}
            candidate=Some(inspect(string(&selected,"base_runner")?)?);
        }}else{require(root.join("runner").symlink_metadata()?.is_symlink(),"Diese Installation unterstützt keinen Proton-Wechsel.")?;require(has_fenix||String::from_utf8_lossy(&files::read(&root.join("tools/launch-msfs.sh"),65536)?).contains("FLIGHTDECK_PROTON_LOADER"),"Aktualisiere zuerst die Flightdeck-Runtime-Komponenten und öffne Flightdeck erneut.")?;}
        if let Some(candidate)=candidate{let(fresh,prepared)=prepare(ctx,&candidate,payload.as_deref(),mode=="default")?;switch(ctx,&fresh,&prepared,mode=="default")?;}
        ctx.launcher.lock().graphics_report=Value::Null;Ok(json!({"state":"complete","message":"Proton-Auswahl bereit. Du kannst den Simulator starten."}))
    })?;
    Ok(json!({"ok":true,"job":response["job"],"runtime_path":root}))
}
