// SPDX-License-Identifier: MIT
//! Recoverable Fenix transactions; the active prefix is published only after setup.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    bootstrap,
    error::require,
    fenix_bundle as bundle, fenix_setup as setup, files, framework_repair,
    games::Game,
    process, runtime, transaction as tx,
    wine::{StagedWine, Wine},
    wine_processes,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub const MARKER: &str = "private/fenix-linux-patch.json";
pub fn validate(root: &Path, recovery: bool) -> Result<()> {
    require(
        Game::for_runtime(root)? == Game::Msfs2024,
        "This preview supports MSFS 2024 only.",
    )?;
    for name in ["private", "tools", "local"] {
        files::directory(&bundle::contained(root, name)?, false)?;
    }
    require(
        root.join("runner").symlink_metadata()?.is_symlink(),
        "Expected Flightdeck's runner symlink; the original runner will be retained.",
    )?;
    if !recovery {
        tx::prefix_system32(&root.join("local/msfs-prefix"))?;
        files::open_at(
            rustix::fs::CWD,
            root.join("local/msfs-prefix/system.reg"),
            false,
            false,
        )?;
    }
    require(
        !files::exists(&root.join("private/proton-switch.json")),
        "Recover the interrupted Proton switch before changing Fenix.",
    )
}
pub fn host_check() -> Result<()> {
    require(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "The binary preview requires x86_64 Linux.",
    )?;
    let output = process::output(
        Command::new("getconf").arg("GNU_LIBC_VERSION"),
        Duration::from_secs(5),
        4096,
        &AtomicBool::new(false),
    )?;
    let text = String::from_utf8_lossy(&output);
    let mut numbers = text
        .trim()
        .strip_prefix("glibc ")
        .unwrap_or("")
        .split('.')
        .filter_map(|v| v.parse::<u32>().ok());
    require(
        (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0)) >= (2, 38),
        "This binary build requires glibc 2.38 or newer. Build from source on older distributions.",
    )
}
pub fn identity(path: &Path) -> Result<Value> {
    let m = files::directory(path, false)?.metadata()?;
    Ok(json!([m.dev(), m.ino()]))
}
pub fn replace_link(path: &Path, target: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or(Error::Invalid("Invalid runner link."))?;
    let temp = parent.join(format!(".fenix-link-{}", uuid::Uuid::new_v4().simple()));
    symlink(target, &temp)?;
    let result = fs::rename(&temp, path);
    let _ = fs::remove_file(temp);
    result?;
    files::directory(parent, false)?.sync_all()?;
    Ok(())
}
fn matches(pattern: &str, value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|v| regex::Regex::new(pattern).is_ok_and(|r| r.is_match(v)))
}
pub fn retryable(root: &Path, state: &Value) -> bool {
    (|| -> Result<bool> {
        if state["state"] != "preparing"
            || state.get("upgrade_backup").is_some()
            || state["migrated"] == true
        {
            return Ok(false);
        }
        for (key, pattern) in [
            ("work", r"^local/fenix-patch-[0-9]{8}T[0-9]{6}-[0-9a-f]{8}$"),
            (
                "backup",
                r"^private/fenix-patch-backup-[0-9]{8}T[0-9]{6}-[0-9a-f]{8}$",
            ),
            (
                "previous_prefix",
                r"^local/msfs-prefix.before-fenix-[0-9]{8}T[0-9]{6}-[0-9a-f]{8}$",
            ),
        ] {
            if !matches(pattern, &state[key]) {
                return Ok(false);
            }
        }
        let prefix = root.join("local/msfs-prefix");
        if identity(&prefix)? != state["original_prefix_id"]
            || fs::read_link(root.join("runner"))? != Path::new(string(state, "previous_runner")?)
            || files::exists(&root.join(string(state, "previous_prefix")?))
        {
            return Ok(false);
        }
        let work = bundle::contained(root, string(state, "work")?)?;
        let backup = bundle::contained(root, string(state, "backup")?)?;
        for path in [&work, &backup] {
            files::directory(path, false)?;
        }
        for name in bundle::LAUNCH_FILES {
            if tx::digest(&root.join("tools").join(name))? != tx::digest(&backup.join(name))? {
                return Ok(false);
            }
        }
        let staged = work.join("prefix");
        if files::exists(&staged) && identity(&staged)? == identity(&prefix)? {
            return Ok(false);
        }
        Ok(true)
    })()
    .unwrap_or(false)
}
fn retry(root: &Path, state: &Value, ctx: &Context) -> Result<()> {
    require(
        retryable(root, state),
        "A previous patch transaction exists. Restore it before reinstalling.",
    )?;
    ctx.progress(
        "Retrying interrupted Fenix preparation automatically; the original profile is retained …",
    );
    let work = bundle::contained(root, string(state, "work")?)?;
    let backup = bundle::contained(root, string(state, "backup")?)?;
    if work.join("prefix").exists() {
        let staged = StagedWine::new(
            root,
            &work.join("prefix"),
            &root.join("runner"),
            &backup.join("retry.log"),
            ctx,
        )?;
        staged.stop()?;
        wine_processes::idle(&work.join("prefix"))?;
    }
    require(
        retryable(root, state),
        "The original profile changed; automatic retry was stopped.",
    )?;
    files::atomic_json(&backup.join("interrupted.json"), state)?;
    fs::remove_file(root.join(MARKER))?;
    files::directory(&root.join("private"), false)?.sync_all()?;
    Ok(())
}
pub fn verify_installed(root: &Path, state: &Value) -> Result<()> {
    let current = bundle::manifest(state["variant"].as_str())?;
    let lock = if state["version"] == current["version"] {
        &current
    } else {
        current["previous_releases"]
            .get(string(state, "version")?)
            .ok_or(Error::Invalid(
                "This installed patch version is not supported by the current installer.",
            ))?
    };
    let runner = root.join("runner").canonicalize()?;
    let expected = bundle::contained(root, string(state, "work")?)?.join("runner");
    require(
        runner == expected,
        "The active runner changed since patch installation.",
    )?;
    let mut hashes = current["runner_files"]
        .as_object()
        .ok_or(Error::Invalid("Invalid Fenix runner manifest."))?
        .clone();
    hashes.extend(
        lock["files"]
            .as_object()
            .ok_or(Error::Invalid("Invalid Fenix payload manifest."))?
            .clone(),
    );
    for (name, hash) in hashes {
        require(
            tx::digest(&bundle::contained(&runner, &name)?)? == hash,
            "Installed Fenix patch file changed.",
        )?;
    }
    for (name, hash) in lock["prefix_files"].as_object().into_iter().flatten() {
        require(
            tx::digest(&bundle::contained(&root.join("local/msfs-prefix"), name)?)? == *hash,
            "Installed Fenix dependency changed.",
        )?;
    }
    Ok(())
}
fn optional(path: &Path) -> Result<Value> {
    if files::exists(path) {
        runtime::value(path)
    } else {
        Ok(Value::Null)
    }
}
pub fn install(ctx: &Context, payload: &Path) -> Result<()> {
    let root = ctx.root()?;
    host_check()?;
    validate(root, false)?;
    wine_processes::idle(&root.join("local/msfs-prefix"))?;
    bundle::verify(payload)?;
    let current = bundle::manifest(None)?;
    let marker = root.join(MARKER);
    let mut upgrade = None;
    if files::exists(&marker) {
        let state = runtime::value(&marker)?;
        if retryable(root, &state) {
            retry(root, &state, ctx)?;
        } else if state["state"] == "installed" && state["version"] == current["version"] {
            verify_installed(root, &state)?;
            crate::framework_maintenance::ensure(ctx)?;
            ctx.progress("This patch version is already installed.");
            return Ok(());
        } else if state["state"] == "installed"
            && state["version"]
                .as_str()
                .is_some_and(|v| current["previous_releases"].get(v).is_some())
        {
            verify_installed(root, &state)?;
            upgrade = Some(state);
        } else {
            return Err(Error::Invalid(
                "A previous patch transaction exists. Restore it before reinstalling.",
            ));
        }
    }
    let original_runner = root.join("runner").canonicalize()?;
    let variant = if let Some(upgrade) = &upgrade {
        upgrade["variant"].as_str().map(str::to_string)
    } else {
        bundle::runner_variant(&original_runner, false)?
    };
    let lock = bundle::manifest(variant.as_deref())?;
    for name in bundle::LAUNCH_FILES {
        require(
            bundle::accepted_script(
                name,
                &tx::digest(&root.join("tools").join(name))?,
                &lock,
                upgrade.as_ref().and_then(|v| v["version"].as_str()),
            )?,
            "Custom Flightdeck launch script detected; it will not be overwritten.",
        )?;
        bundle::script(name)?;
    }
    let prefix = root.join("local/msfs-prefix");
    let size = tx::prefix_size(&prefix, &ctx.cancel)?
        .saturating_add(tx::prefix_size(&original_runner, &ctx.cancel)?);
    require(
        tx::free_bytes(root)? > size.saturating_add(1024 * 1024 * 1024),
        "Not enough free space for the independent Wine profile and runner copies.",
    )?;
    let stamp = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let backup = root
        .join("private")
        .join(format!("fenix-patch-backup-{stamp}"));
    let work = root.join("local").join(format!("fenix-patch-{stamp}"));
    files::private_dir(&backup)?;
    files::private_dir(&work)?;
    let mut state = json!({"format":1,"version":lock["version"],"state":"preparing","work":work.strip_prefix(root).map_err(|_|Error::Invalid("Invalid staging path."))?,"backup":backup.strip_prefix(root).map_err(|_|Error::Invalid("Invalid backup path."))?,"previous_runner":fs::read_link(root.join("runner"))?,"previous_prefix":format!("local/msfs-prefix.before-fenix-{stamp}"),"configured":false,"original_prefix_id":identity(&prefix)?,"variant":variant,"proton_before":optional(&root.join("private/proton-selection.json"))?});
    if let Some(previous) = &upgrade {
        files::atomic_json(&backup.join("previous-patch.json"), previous)?;
        for key in [
            "backup",
            "previous_runner",
            "previous_prefix",
            "original_prefix_id",
            "configured",
            "proton_before",
        ] {
            state[key] = previous[key].clone();
        }
        state["upgrade_backup"] = json!(
            backup
                .strip_prefix(root)
                .map_err(|_| Error::Invalid("Invalid backup path."))?
        );
        state["upgrade_previous_prefix"] =
            json!(format!("local/msfs-prefix.before-fenix-update-{stamp}"));
    }
    for name in bundle::LAUNCH_FILES {
        tx::copy_path(
            &root.join("tools").join(name),
            &backup.join(name),
            None,
            &ctx.cancel,
        )?;
    }
    if root.join("private/import-manifest.json").is_file() {
        tx::copy_path(
            &root.join("private/import-manifest.json"),
            &backup.join("import-manifest.json"),
            None,
            &ctx.cancel,
        )?;
    }
    files::atomic_json(&marker, &state)?;
    let staged = work.join("prefix");
    let runner = work.join("runner");
    ctx.progress(if upgrade.is_some() {
        "Updating the Fenix patch; installed aircraft and settings are retained …"
    } else {
        "Copying Wine profile and runner; the original profile remains available …"
    });
    process::copy_tree(&prefix, &staged, &ctx.cancel)?;
    tx::relocate_prefix_links(&prefix, &staged, &ctx.cancel)?;
    let drive = bundle::contained(&staged, "dosdevices/c:").or_else(|_| {
        let path = staged.join("dosdevices/c:");
        files::directory(&staged.join("dosdevices"), false)?;
        Ok::<_, Error>(path)
    })?;
    require(
        !files::exists(&drive) || drive.symlink_metadata()?.is_symlink(),
        "The staged C: drive is not a Wine link.",
    )?;
    if files::exists(&drive) {
        fs::remove_file(&drive)?;
    }
    symlink("../drive_c", &drive)?;
    process::copy_tree(&original_runner, &runner, &ctx.cancel)?;
    let mut wine = StagedWine::new(root, &staged, &runner, &backup.join("setup.log"), ctx)?;
    let setup_result = (|| {
        framework_repair::prepare(
            &mut wine,
            |name, url, hash| {
                bootstrap::download(
                    url,
                    hash,
                    &root.join("private/fenix-downloads").join(name),
                    &ctx.cancel,
                    |_| {},
                )
            },
            |message| ctx.progress(message),
        )?;
        if upgrade.is_none() {
            ctx.progress("Preparing fonts, graphics dependencies and Fenix settings …");
            setup::graphics_fonts(&wine)?;
            state["configured"] = json!(setup::configure_prefix(&staged)?);
        }
        setup::geometry(&wine, &root.join("private/fenix-downloads"), payload, ctx)
    })();
    let stopped = wine.stop();
    setup_result.and(stopped)?;
    bundle::overlay(&staged, &runner, payload, variant.as_deref(), ctx)?;
    wine_processes::idle(&prefix)?;
    ctx.begin_commit(
        "publish_runtime",
        "Die geprüfte Fenix-Umgebung wird aktiviert …",
    )?;
    state["state"] = json!("committing");
    files::atomic_json(&marker, &state)?;
    let previous = state["upgrade_previous_prefix"]
        .as_str()
        .unwrap_or(string(&state, "previous_prefix")?);
    tx::publish(&prefix, &bundle::contained(root, previous)?)?;
    tx::publish(&staged, &prefix)?;
    replace_link(&root.join("runner"), &runner)?;
    for name in bundle::LAUNCH_FILES {
        crate::resources::ensure_helper(root)?;
        bundle::write(&root.join("tools").join(name), bundle::script(name)?, 0o700)?;
    }
    update_script_manifest(root)?;
    let mut selected = optional(&root.join("private/proton-selection.json"))?;
    if !selected.is_null() {
        selected["schema"] = json!(2);
        selected["runner"] = json!(
            runner
                .strip_prefix(root)
                .map_err(|_| Error::Invalid("Invalid runner path."))?
        );
        files::atomic_json(&root.join("private/proton-selection.json"), &selected)?;
    }
    state["state"] = json!("installed");
    files::atomic_json(&marker, &state)?;
    Ok(())
}
pub fn update_script_manifest(root: &Path) -> Result<()> {
    let imported = root.join("private/import-manifest.json");
    if files::exists(&imported) {
        let mut data = runtime::value(&imported)?;
        if data["runtime_files"].is_object() {
            for name in bundle::LAUNCH_FILES {
                data["runtime_files"][name] = json!(tx::digest(&root.join("tools").join(name))?);
            }
        }
        files::atomic_json(&imported, &data)?;
    }
    Ok(())
}
pub fn restore(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    validate(root, true)?;
    let prefix = root.join("local/msfs-prefix");
    wine_processes::idle(&prefix)?;
    let marker = root.join(MARKER);
    let mut state = runtime::value(&marker)?;
    require(
        state["migrated"] != true,
        "This migrated profile has no managed pre-patch restore point.",
    )?;
    require(
        state["state"] != "restored",
        "This patch is already restored.",
    )?;
    let backup = bundle::contained(root, string(&state, "backup")?)?;
    let previous = bundle::contained(root, string(&state, "previous_prefix")?)?;
    let work = bundle::contained(root, string(&state, "work")?)?;
    files::directory(&backup, false)?;
    wine_processes::idle(&work.join("prefix"))?;
    let scripts: Vec<_> = bundle::LAUNCH_FILES
        .iter()
        .map(|name| files::read(&backup.join(name), 1024 * 1024).map(|bytes| (*name, bytes)))
        .collect::<Result<_>>()?;
    let imported = if backup.join("import-manifest.json").exists() {
        Some(files::read(
            &backup.join("import-manifest.json"),
            1024 * 1024,
        )?)
    } else {
        None
    };
    require(
        identity(if previous.exists() {
            &previous
        } else {
            &prefix
        })? == state["original_prefix_id"],
        "The original profile backup is missing; nothing was removed.",
    )?;
    let runner = PathBuf::from(string(&state, "previous_runner")?);
    let selected = state["proton_before"].clone();
    ctx.begin_commit(
        "publish_runtime",
        "Das Profil vor dem Fenix-Patch wird wiederhergestellt …",
    )?;
    state["state"] = json!("restoring");
    files::atomic_json(&marker, &state)?;
    if previous.exists() {
        if prefix.exists() {
            let retained = root.join("local").join(format!(
                "msfs-prefix.fenix-retained-{}",
                uuid::Uuid::new_v4().simple()
            ));
            tx::publish(&prefix, &retained)?;
            state["retained_prefix"] = json!(
                retained
                    .strip_prefix(root)
                    .map_err(|_| Error::Invalid("Invalid retained profile path."))?
            );
            files::atomic_json(&marker, &state)?;
        }
        tx::publish(&previous, &prefix)?;
    }
    replace_link(&root.join("runner"), &runner)?;
    for (name, bytes) in scripts {
        bundle::write(&root.join("tools").join(name), &bytes, 0o700)?;
    }
    if let Some(bytes) = imported {
        files::atomic(&root.join("private/import-manifest.json"), &bytes)?;
    }
    let proton = root.join("private/proton-selection.json");
    if selected.is_null() {
        if files::exists(&proton) {
            fs::remove_file(&proton)?;
        }
    } else {
        files::atomic_json(&proton, &selected)?;
    }
    state["state"] = json!("restored");
    files::atomic_json(&backup.join("restored.json"), &state)?;
    fs::remove_file(marker)?;
    files::directory(&root.join("private"), false)?.sync_all()?;
    Ok(())
}
pub fn configure(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    validate(root, false)?;
    let prefix = root.join("local/msfs-prefix");
    wine_processes::idle(&prefix)?;
    let mut state = runtime::value(&root.join(MARKER))?;
    require(
        state["state"] == "installed",
        "Finish patch installation first.",
    )?;
    verify_installed(root, &state)?;
    let backup = tx::new_directory(
        &bundle::contained(root, string(&state, "backup")?)?,
        "settings-",
    )?;
    for name in ["fenixConfig.xml", "persistancy.xml"] {
        let path = bundle::contained(&prefix, &format!("{}/{name}", setup::CONFIG))?;
        if files::exists(&path) {
            tx::copy_path(&path, &backup.join(name), None, &ctx.cancel)?;
        }
    }
    state["configured"] = json!(setup::configure_prefix(&prefix)?);
    files::atomic_json(&root.join(MARKER), &state)?;
    require(
        state["configured"] == true,
        "Start Fenix once and sign in, then close it and apply settings again. CPU rendering and Legacy readouts need its initial settings files.",
    )
}
fn windows_app(ctx: &Context, operation: &str, data: &Value) -> Result<()> {
    let root = ctx.root()?;
    validate(root, false)?;
    let prefix = root.join("local/msfs-prefix");
    wine_processes::idle(&prefix)?;
    let state = optional(&root.join(MARKER))?;
    if !state.is_null() {
        require(
            state["state"] == "installed",
            "Install the compatibility patch first.",
        )?;
        verify_installed(root, &state)?;
    } else {
        require(
            operation != "installer" && root.join("private/fenix-compat.json").is_file(),
            "Install the compatibility patch first.",
        )?;
    }
    let app = match operation {
        "installer" => {
            let raw = string(data, "installer_path")?;
            let path = files::expand(raw);
            require(
                path.is_absolute() && raw.len() <= 4096,
                "Bitte die offizielle Fenix-Installer-EXE auswählen.",
            )?;
            let path = path.canonicalize()?;
            let mut file = files::open_at(rustix::fs::CWD, &path, false, false)?;
            use std::io::Read;
            let mut magic = [0; 2];
            file.read_exact(&mut magic)?;
            require(
                file.metadata()?.len() <= 1024 * 1024 * 1024
                    && path
                        .extension()
                        .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
                    && magic == *b"MZ",
                "Select the official Fenix Installer .exe from your Fenix account.",
            )?;
            path
        }
        "manager" => setup::manager(&prefix)?,
        _ => prefix.join(setup::PROGRAM).join("Fenix.exe"),
    };
    files::open_at(rustix::fs::CWD, &app, false, false)?;
    crate::framework_maintenance::ensure(ctx)?;
    let wine = Wine::new(
        &prefix,
        &root.join("runner"),
        &root.join("private/fenix-app.log"),
        &ctx.cancel,
    )?;
    setup::ui_fonts(&wine)?;
    wine.reg(
        r"HKCU\Software\Wine\Explorer",
        "ShowSystray",
        "0",
        "REG_DWORD",
    )?;
    ctx.progress(
        "Fenix is open. Complete its setup or sign-in, then close the application to continue.",
    );
    let mut c = wine.command()?;
    c.arg(&app).current_dir(
        app.parent()
            .ok_or(Error::Invalid("Invalid Fenix executable path."))?,
    );
    let mut child = process::spawn(&mut c, None)?;
    let result = (|| {
        loop {
            if ctx.pause.load(Ordering::Relaxed) || ctx.cancel.load(Ordering::Relaxed) {
                ctx.update(json!({"stopping":true,"message":"Fenix wird beendet …"}));
                wine_processes::stop(root, wine_processes::FENIX)?;
                process::terminate(&mut child)?;
                return ctx.interrupted();
            }
            if let Some(code) = child.try_wait()? {
                ctx.update(json!({"app_exited":true}));
                wine_processes::stop(root, wine_processes::FENIX)?;
                return require(
                    code.success(),
                    "Fenix wurde unerwartet beendet. Details stehen im lokalen Fenix-Protokoll.",
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })();
    if result.is_err() {
        let _ = wine_processes::stop(root, wine_processes::FENIX);
        let _ = process::terminate(&mut child);
    }
    result
}
pub fn snapshot(app: &Launcher) -> Value {
    let (root, busy, owned, closing, job) = {
        let mut s = app.lock();
        Launcher::poll(&mut s);
        (
            s.runtime.clone(),
            s.active.is_some() || Launcher::external(&s),
            s.process.is_some(),
            s.closing,
            s.jobs.get("fenix").cloned().unwrap_or(Value::Null),
        )
    };
    let mut value = json!({"state":"unavailable","version":bundle::manifest(None).ok().map(|v|v["version"].clone()),"configured":false,"installed":false,"fenix_installed":false,"can_restore":false,"can_retry":false,"manager_installed":false,"idle":false,"message":"Zuerst MSFS 2024 in Flightdeck einrichten.","job":job,"runtime_path":root,"busy":busy||owned,"fenix_running":false,"can_stop":false,"can_change":false,"project":"https://github.com/marselnenaj/fenix-a320-linux-patch"});
    if let Some(root) = root {
        let result = (|| -> Result<()> {
            validate(&root, true)?;
            let prefix = root.join("local/msfs-prefix");
            value["fenix_installed"] =
                json!(prefix.join(setup::PROGRAM).join("Fenix.exe").is_file());
            value["manager_installed"] = json!(setup::manager(&prefix).is_ok());
            value["settings_ready"] = json!(
                value["fenix_installed"] == true
                    && ["fenixConfig.xml", "persistancy.xml"]
                        .iter()
                        .all(|name| prefix.join(setup::CONFIG).join(name).is_file())
            );
            if files::exists(&root.join(MARKER)) {
                let state = runtime::value(&root.join(MARKER))?;
                value["state"] = state["state"].clone();
                value["installed"] = json!(state["state"] == "installed");
                value["configured"] = json!(state["configured"] == true);
                value["can_restore"] = json!(state["migrated"] != true);
                value["can_retry"] = json!(retryable(&root, &state));
                value["installed_version"] = state["version"].clone();
                value["update_available"] = json!(
                    state["state"] == "installed"
                        && state["version"]
                            .as_str()
                            .is_some_and(|v| bundle::manifest(None)
                                .is_ok_and(|l| l["previous_releases"].get(v).is_some()))
                );
            } else if root.join("private/fenix-compat.json").exists() {
                value["state"] = json!("legacy");
                value["message"] = json!(
                    "An earlier local Fenix patch is active. Keep using it; automatic replacement is disabled."
                );
            } else {
                host_check()?;
                bundle::runner_variant(&root.join("runner"), false)?;
                value["state"] = json!("available");
            }
            let idle = wine_processes::idle(&prefix).is_ok();
            let (fenix_running, game_running) = wine_processes::status(&prefix);
            let interactive = job["state"] == "running"
                && job["app_exited"] != true
                && ["open", "manager"].contains(&job["operation"].as_str().unwrap_or(""));
            value["idle"] = json!(idle);
            value["fenix_running"] = json!(fenix_running);
            value["can_stop"] = json!(
                fenix_running
                    && !game_running
                    && !owned
                    && !closing
                    && job["stopping"] != true
                    && (interactive || !busy)
            );
            value["can_change"] = json!(!busy && !owned && !closing && idle);
            if value["state"] != "legacy" {
                value["message"] = json!("");
            }
            Ok(())
        })();
        if let Err(error) = result {
            value["message"] = json!(error.to_string());
        }
    }
    value
}
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    require(
        [
            "install",
            "configure",
            "restore",
            "installer",
            "open",
            "manager",
            "stop",
        ]
        .contains(&operation),
        "Unbekannter Fenix-Vorgang.",
    )?;
    if operation == "stop" {
        let mut s = app.lock();
        Launcher::open(&s)?;
        if s.active.as_ref().is_some_and(|a| a.kind == "fenix")
            && s.jobs.get("fenix").is_some_and(|j| {
                j["state"] == "running"
                    && ["open", "manager"].contains(&j["operation"].as_str().unwrap_or(""))
            })
        {
            if let Some(active) = &s.active {
                active.pause.store(true, Ordering::Relaxed);
            }
            let job = s
                .jobs
                .get_mut("fenix")
                .ok_or(Error::Invalid("Fenix läuft nicht."))?;
            job["stopping"] = json!(true);
            return Ok(json!({"ok":true,"job_id":job["id"]}));
        }
    }
    let data = data.clone();
    let operation = operation.to_string();
    app.start_job("fenix",&operation.clone(),true,move|ctx|{
        if ["configure","installer","open","manager"].contains(&operation.as_str())&&snapshot(&ctx.launcher)["update_available"]==true{let payload=bundle::obtain(&ctx.launcher.state_dir.join("fenix-bundles"),None,ctx)?;install(ctx,&payload)?;}
        match operation.as_str(){"install"=>{let payload=bundle::obtain(&ctx.launcher.state_dir.join("fenix-bundles"),data["bundle_path"].as_str(),ctx)?;install(ctx,&payload)?;},"restore"=>restore(ctx)?,"configure"=>configure(ctx)?,"stop"=>{validate(ctx.root()?,true)?;wine_processes::stop(ctx.root()?,wine_processes::FENIX)?;},_=>windows_app(ctx,&operation,&data)?,}
        Ok(json!({"state":"complete","stopping":false,"message":match operation.as_str(){"install"=>"Fenix-Patch eingerichtet. Die nächsten Schritte stehen oben.","configure"=>"Anzeigen und automatischer Fenix-Start sind eingerichtet.","restore"=>"Das Profil vor dem Patch wurde wiederhergestellt.","installer"=>"Installer beendet. Prüfe die nächsten Schritte oben; die Fenix-Einrichtung ist noch nicht automatisch abgeschlossen.",_=>"Fenix wurde beendet."}}))
    })
}

pub fn pick(app: &Launcher, data: &Value) -> Result<Value> {
    let kind = string(data, "kind")?;
    require(
        ["installer", "bundle"].contains(&kind),
        "Ungültige Fenix-Dateiauswahl.",
    )?;
    {
        let s = app.lock();
        Launcher::open(&s)?;
        require(
            s.active.is_none(),
            "Bitte die laufende Einrichtung zuerst abschließen oder abbrechen.",
        )?;
    }
    let _guard = crate::setup::PICKER
        .try_lock()
        .map_err(|_| Error::Invalid("Ein Dateidialog ist bereits geöffnet."))?;
    let (name, program) = crate::setup::picker().ok_or(Error::Invalid(
        "Kein Dateidialog verfügbar. Bitte den Pfad direkt eingeben.",
    ))?;
    let mut command = Command::new(program);
    if name == "zenity" {
        command.arg("--file-selection").arg(if kind == "installer" {
            "--title=Fenix Installer"
        } else {
            "--title=Fenix Patch"
        });
        command.arg(if kind == "installer" {
            "--file-filter=Windows Installer | *.exe"
        } else {
            "--directory"
        });
    } else {
        command
            .arg(if kind == "installer" {
                "--getopenfilename"
            } else {
                "--getexistingdirectory"
            })
            .arg(files::home());
    }
    let path = process::output(
        &mut command,
        Duration::from_secs(120),
        4098,
        &AtomicBool::new(false),
    )
    .ok()
    .and_then(|v| String::from_utf8(v).ok())
    .map(|v| v.trim_end_matches(['\r', '\n']).to_string())
    .filter(|v| !v.is_empty() && v.len() <= 4096 && Path::new(v).is_absolute());
    Ok(if let Some(path) = path {
        json!({"ok":true,"path":path})
    } else {
        json!({"ok":true,"cancelled":true})
    })
}
