// SPDX-License-Identifier: MIT
//! Native adapter for the same pinned Steam engine used by the standalone installer.
use crate::{
    Error, Result,
    backend::{Context, Launcher},
    error::require,
    fenix_bundle, files, framework_repair,
    installations::Installation,
    wine_processes,
};
use fenix_patch::{
    core::{RunOptions, Wine},
    json::Value as SharedValue,
    steam,
    system::{Progress, System},
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub fn map_error(error: fenix_patch::Error) -> Error {
    match error {
        fenix_patch::Error::Interrupted => Error::Cancelled,
        _ => Error::Invalid(
            "Die Steam-Fenix-Aktion ist fehlgeschlagen. Details stehen im Fenix-Status und im privaten Einrichtungsprotokoll.",
        ),
    }
}
pub struct Engine {
    pub bundle: PathBuf,
    pub manifest: Value,
    pub cancel: Arc<AtomicBool>,
    invocation: Mutex<Option<(File, u64)>>,
}
impl Engine {
    pub fn new(bundle: PathBuf, manifest: Value, cancel: Arc<AtomicBool>) -> Self {
        Self {
            bundle,
            manifest,
            cancel,
            invocation: Mutex::new(None),
        }
    }
}
impl System for Engine {
    fn environ(&self) -> fenix_patch::process::Env {
        fenix_patch::process::Env(
            crate::wine::inherited_environment(std::env::vars())
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
    fn spawn_app(
        &self,
        cmd: &fenix_patch::process::Cmd<'_>,
    ) -> fenix_patch::Result<std::process::Child> {
        if let fenix_patch::process::Output::File(file) = &cmd.output {
            let copy = file.try_clone().map_err(|e| fenix_patch::Error::io(&e))?;
            let offset = copy
                .metadata()
                .map_err(|e| fenix_patch::Error::io(&e))?
                .len();
            *self.invocation.lock().unwrap_or_else(|e| e.into_inner()) = Some((copy, offset));
        }
        fenix_patch::process::spawn(cmd)
    }
    fn root(&self) -> &Path {
        &self.bundle
    }
    fn interrupted(&self) -> fenix_patch::Result<()> {
        if self.cancel.swap(false, Ordering::SeqCst) {
            Err(fenix_patch::Error::Interrupted)
        } else {
            Ok(())
        }
    }
    fn manifest(&self, variant: Option<&str>) -> fenix_patch::Result<SharedValue> {
        let manifest = fenix_bundle::select_variant(self.manifest.clone(), variant)
            .map_err(|_| fenix_patch::Error::patch("Invalid patch variant"))?;
        fenix_patch::json::loads(&manifest.to_string())
    }
    fn ensure_idle(&self, prefix: &Path) -> fenix_patch::Result<()> {
        wine_processes::idle(prefix).map_err(|_| {
            fenix_patch::Error::patch(
                "Close MSFS, Fenix and all installers for this profile first.",
            )
        })
    }
    fn prepare_framework(
        &self,
        wine: &dyn Wine,
        cache: &Path,
        progress: Progress<'_>,
        first_version: &str,
    ) -> fenix_patch::Result<()> {
        struct Adapter<'a> {
            wine: &'a dyn Wine,
            cancel: &'a AtomicBool,
        }
        impl framework_repair::SetupWine for Adapter<'_> {
            fn prefix(&self) -> &Path {
                self.wine.prefix()
            }
            fn run(&mut self, args: &[String], installer: bool, timeout: Duration) -> Result<()> {
                self.wine
                    .run(
                        &args.iter().map(OsString::from).collect::<Vec<_>>(),
                        &RunOptions {
                            timeout: timeout.as_secs(),
                            accepted: if installer {
                                framework_repair::INSTALLER_EXIT_CODES
                            } else {
                                &[0]
                            },
                            env: if installer {
                                &[("WINEDLLOVERRIDES", framework_repair::INSTALLER_OVERRIDES)]
                            } else {
                                &[]
                            },
                        },
                    )
                    .map_err(|error| {
                        // Keep command failure details in the private service
                        // log; HTTP responses still contain only static text.
                        eprintln!(
                            "Steam Fenix prerequisite command failed: {}",
                            error.to_string().chars().take(8192).collect::<String>()
                        );
                        map_error(error)
                    })
            }
            fn reg(&mut self, key: &str, name: &str, value: &str, kind: &str) -> Result<()> {
                self.wine.reg(key, name, value, kind).map_err(|error| {
                    eprintln!(
                        "Steam Fenix registry preparation failed: {}",
                        error.to_string().chars().take(8192).collect::<String>()
                    );
                    map_error(error)
                })
            }
            fn stop(&mut self) -> Result<()> {
                self.wine.stop().map_err(|error| {
                    eprintln!(
                        "Steam Fenix staging server shutdown failed: {}",
                        error.to_string().chars().take(8192).collect::<String>()
                    );
                    map_error(error)
                })
            }
            fn list(&mut self) -> Result<String> {
                let mut c = fenix_patch::process::Cmd::new(vec![
                    fenix_patch::core::wine_binary(self.wine.runner()).into_os_string(),
                    "uninstaller".into(),
                    "--list".into(),
                ]);
                c.env = Some(self.wine.env());
                c.output = fenix_patch::process::Output::Capture;
                c.timeout = Some(120);
                c.stdin = fenix_patch::process::Input::Null;
                let output = fenix_patch::process::run_checked(&c, &|| {
                    if self.cancel.load(Ordering::Relaxed) {
                        Err(fenix_patch::Error::Interrupted)
                    } else {
                        Ok(())
                    }
                })
                .map_err(map_error)?;
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            }
        }
        let result = framework_repair::prepare_with_version(
            &mut Adapter {
                wine,
                cancel: &self.cancel,
            },
            |name, url, hash| {
                self.download(url, &cache.join(name), hash, 512 * 1024 * 1024)
                    .map_err(map_error)
            },
            |message| {
                if progress(message).is_err() {
                    self.cancel.store(true, Ordering::Relaxed);
                }
            },
            first_version,
        );
        result.map_err(|e| {
            if matches!(e, Error::Cancelled) {
                fenix_patch::Error::Interrupted
            } else {
                fenix_patch::Error::patch(e.to_string())
            }
        })
    }
}
pub(crate) fn recorded(state_dir: &Path, item: &Installation) -> Result<Value> {
    let path = state_dir
        .join("steam")
        .join(&item.id)
        .join("private/fenix-patch-release.json");
    if files::exists(&path) {
        let value: Value = files::json(&path, 2 * 1024 * 1024)?;
        crate::fenix_update::installed_manifest(&value, None)?;
        return Ok(value);
    }
    let manifest = fenix_bundle::manifest(None)?;
    Ok(
        json!({"version":manifest["version"],"bundle_manifest":manifest,"bundle_release":crate::resources::json("compat/fenix/release.json")?}),
    )
}
fn engine(state_dir: &Path, item: &Installation) -> Result<Engine> {
    let manifest = recorded(state_dir, item)?["bundle_manifest"].clone();
    Ok(Engine {
        bundle: state_dir
            .join("fenix-bundles")
            .join(manifest["version"].as_str().unwrap_or("unavailable")),
        manifest,
        cancel: Arc::new(AtomicBool::new(false)),
        invocation: Mutex::new(None),
    })
}
pub(crate) fn companion_profile(
    state_dir: &Path,
    item: &Installation,
) -> Result<crate::display_refresh::Profile> {
    let system = engine(state_dir, item)?;
    let target = item.target()?;
    let state = steam::installed_state(&system, &target).map_err(map_error)?;
    use fenix_patch::json::Dict;
    let tool = state.need_str("tool").map_err(map_error)?;
    require(
        steam::selected_tool(&target).as_deref() == Some(tool),
        "Der Fenix-Proton ist für dieses Spiel nicht ausgewählt.",
    )?;
    let runner = fenix_patch::fsx::contained(&target.tools(), tool).map_err(map_error)?;
    require(
        wine_processes::path_in_use(&runner)?,
        "Der ausgewählte Fenix-Proton läuft noch nicht.",
    )?;
    let library = ["SimConnect_internal.dll", "SimConnect.dll"]
        .into_iter()
        .map(|name| item.game_path().join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| item.game_path().join("SimConnect_internal.dll"));
    let private = evidence_root_at(state_dir, item).join("private");
    files::private_dir(&private)?;
    Ok(crate::display_refresh::Profile {
        prefix: item.prefix(),
        runner,
        library,
        private,
    })
}
pub fn snapshot(app: &Launcher) -> Value {
    let (item, busy, job, patch_update, candidate) = {
        let s = app.lock();
        let Some(item) = s.steam.clone() else {
            return json!({"state":"unavailable"});
        };
        (
            item,
            s.active.is_some() || s.closing,
            s.jobs.get("fenix").cloned().unwrap_or(Value::Null),
            s.fenix_updates.status.clone(),
            s.fenix_updates.candidate.clone(),
        )
    };
    let target = match item.target() {
        Ok(v) => v,
        Err(_) => return json!({"state":"unavailable"}),
    };
    let mut engine = match engine(&app.state_dir, &item) {
        Ok(v) => v,
        Err(_) => {
            return json!({"state":"unavailable","runtime_path":item.context(),"message":"Die gespeicherte Fenix-Releasebeschreibung konnte nicht geprüft werden."});
        }
    };
    if crate::fenix_update::version(candidate["bundle"]["version"].as_str().unwrap_or(""))
        .ok()
        .zip(crate::fenix_update::version(engine.manifest["version"].as_str().unwrap_or("")).ok())
        .is_some_and(|(latest, current)| latest > current)
    {
        engine.manifest = candidate["bundle"].clone();
    }
    let mut value = match steam::snapshot(&engine, &target) {
        Ok(v) => {
            serde_json::from_str::<Value>(&fenix_patch::json::dumps(&v, None)).unwrap_or_default()
        }
        Err(_) => {
            json!({"state":"unavailable","message":"Der Steam-Fenix-Status konnte nicht gelesen werden."})
        }
    };
    // The standalone CLI includes detailed filesystem errors in its status.
    // Keep HTTP status text static, as for native Xbox operations; paths and
    // arbitrary metadata from underlying errors belong in local diagnostics.
    value["message"] = json!(if value["state"] == "unavailable" {
        "Der Steam-Fenix-Status konnte nicht gelesen werden. Prüfe das lokale Einrichtungsprotokoll."
    } else if value["state"] == "installed" && value["tool_selected"] != true {
        "Steam verwendet den Fenix-Proton für diese Installation noch nicht."
    } else if value["state"] != "installed" && value["can_restore"] == true {
        "Eine frühere Einrichtung wurde nicht abgeschlossen. Stelle das ursprüngliche Profil wieder her und starte die Einrichtung erneut."
    } else {
        ""
    });
    let same_job = job["installation_id"] == item.id
        && (!crate::game_update::terminal_job(&job)
            || job["runtime_path"] == item.context()
            || ["install", "restore"].contains(&job["operation"].as_str().unwrap_or("")));
    value["job"] = if same_job { job } else { Value::Null };
    value["runtime_path"] = json!(item.context());
    value["installation_id"] = json!(item.id);
    value["source"] = json!("steam");
    value["busy"] = json!(busy);
    value["can_retry"] = json!(false);
    value["can_change"] = json!(!busy && value["idle"] == true && item.validate().is_ok());
    value["can_check_updates"] = json!(!busy);
    value["patch_update"] = patch_update;
    value["last_attempt"] = latest_attempt(&app.state_dir, &item);
    value["can_cancel"] =
        json!(busy && !crate::game_update::terminal_job(&value["job"]) && !value["job"].is_null());
    value["can_repair_installer"] =
        json!(crate::fenix_installer::installed_version(&item.prefix()).is_ok());
    value["settings_ready"] = json!(
        value["fenix_installed"] == true
            && item
                .prefix()
                .join(crate::fenix_setup::CONFIG)
                .join("fenixConfig.xml")
                .is_file()
    );
    value["fenix_running"] = json!(wine_processes::status(&item.prefix()).0);
    value["can_stop"] = json!(
        !wine_processes::status(&item.prefix()).1
            && ((!busy && value["fenix_running"] == true)
                || (value["can_cancel"] == true
                    && ["installer", "manager", "open", "repair"]
                        .contains(&value["job"]["operation"].as_str().unwrap_or(""))))
    );
    value["steam_running"] = json!(steam::steam_running());
    value["needs_first_launch"] = json!(!item.prefix().join("system.reg").is_file());
    if value["state"] != "installed" {
        value["installed_version"] = Value::Null;
    }
    value
}
pub fn perform(
    system: &dyn System,
    item: &Installation,
    operation: &str,
    payload: &Path,
    installer: Option<&str>,
    progress: Progress<'_>,
) -> fenix_patch::Result<()> {
    let item = item
        .target()
        .map_err(|_| fenix_patch::Error::patch("Invalid Steam target"))?;
    match operation {
        "install" => steam::install(system, &item, payload.as_os_str(), progress, None),
        "configure" => steam::configure(system, &item, progress),
        "restore" => steam::restore(system, &item, progress),
        "select" => steam::select_tool(system, &item, progress),
        "installer" => steam::windows_app(
            system,
            &item,
            installer.map(std::ffi::OsStr::new),
            progress,
            false,
            None,
        ),
        "manager" => steam::windows_app(system, &item, None, progress, true, None),
        "open" => steam::windows_app(system, &item, None, progress, false, None),
        _ => Err(fenix_patch::Error::patch("Unknown Steam patch operation")),
    }
}
fn evidence_root(app: &Launcher, item: &Installation) -> PathBuf {
    evidence_root_at(&app.state_dir, item)
}
fn evidence_root_at(state_dir: &Path, item: &Installation) -> PathBuf {
    state_dir
        .join("steam")
        .join(&item.id)
        .join("profiles")
        .join(item.prefix_generation())
}
pub fn latest_attempt(state_dir: &Path, item: &Installation) -> Value {
    crate::fenix_diagnostics::load(&evidence_root_at(state_dir, item))
}
fn windows_app(
    ctx: &Context,
    item: &Installation,
    system: &Engine,
    operation: &str,
    installer: Option<&str>,
) -> Result<()> {
    let target = item.target()?;
    let evidence = evidence_root(&ctx.launcher, item);
    files::private_dir(&evidence.join("private"))?;
    let mut attempt = if ["installer", "manager", "repair"].contains(&operation) {
        Some(crate::fenix_diagnostics::begin(
            &evidence, operation, "proton",
        )?)
    } else {
        None
    };
    let log_path = target.state().join("fenix-app.log");
    let result = (|| {
        let _guard = steam::locked(system, &target, true, false).map_err(map_error)?;
        let state = steam::installed_state(system, &target).map_err(map_error)?;
        steam::verify_installed(system, &target, &state).map_err(map_error)?;
        use fenix_patch::json::Dict;
        let tool = fenix_patch::fsx::contained(
            &target.tools(),
            state.need_str("tool").map_err(map_error)?,
        )
        .map_err(map_error)?;
        let wine = crate::wine::Wine::new(&item.prefix(), &tool, &log_path, &ctx.cancel)?;
        crate::fenix_installer::prepare(&wine)?;
        let result = (|| {
            if operation == "repair" {
                crate::fenix_desktop::ensure(&wine, &target.state().join("fenix-downloads"), ctx)?;
                if let Some(attempt) = &mut attempt {
                    attempt.package_version =
                        Some(crate::fenix_installer::installed_version(&item.prefix())?);
                    attempt.hook = Some("install".into());
                }
                let offset = wine.log.metadata()?.len();
                ctx.progress(
                "Der offizielle Fenix-Einrichtungsschritt wird im Steam-Profil erneut ausgeführt …",
            );
                let outcome = crate::fenix_installer::repair(&wine);
                if let Some(attempt) = &mut attempt
                    && let Ok(code) = &outcome
                {
                    attempt.process_exit_code = code.code();
                    attempt.hook_exit_code = code.code();
                    if !code.success() {
                        attempt.failure = Some("hook_nonzero".into());
                    }
                }
                *system.invocation.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((wine.log.try_clone()?, offset));
                let code = outcome?;
                require(
                    code.success(),
                    "Der Fenix-Einrichtungsschritt ist weiterhin fehlgeschlagen. Bitte den neuen Diagnosebericht teilen.",
                )?;
                return Ok(());
            }
            let mut handoff = crate::fenix_installer::Handoff::default();
            let mut waiter = |child: &mut std::process::Child| -> fenix_patch::Result<i32> {
                let mut code = None;
                loop {
                    system.interrupted()?;
                    if code.is_none() {
                        code = child.try_wait().map_err(|e| fenix_patch::Error::io(&e))?;
                    }
                    if let Some(code) = code.filter(|code| !code.success()) {
                        let exit = code.code().unwrap_or(1);
                        if let Some(attempt) = &mut attempt {
                            attempt.process_exit_code = Some(exit);
                        }
                        return Ok(exit);
                    }
                    let applications = wine_processes::Processes::new(&item.prefix())
                        .map_err(|_| {
                            fenix_patch::Error::patch("The Steam profile could not be inspected")
                        })?
                        .applications_running();
                    if handoff.complete(std::time::Instant::now(), code.is_some(), applications) {
                        let exit = code.and_then(|s| s.code()).unwrap_or(1);
                        if let Some(attempt) = &mut attempt {
                            attempt.process_exit_code = Some(exit);
                        }
                        return Ok(exit);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            };
            fenix_patch::core::run_app(
                system,
                &item.prefix(),
                &tool,
                &log_path,
                installer.map(std::ffi::OsStr::new),
                &|m| {
                    ctx.progress(m);
                    Ok(())
                },
                operation == "manager",
                Some(&mut waiter),
            )
            .map_err(map_error)
        })();
        // Retain the standalone installer's profile lock through cleanup. Its
        // Steam client and other profiles are never part of this stop set.
        let cleaned = wine_processes::stop_prefix(&item.prefix(), &tool, wine_processes::FENIX);
        result.and(cleaned)
    })();
    let hooks = system
        .invocation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|(file, offset)| {
            crate::fenix_diagnostics::invocation_log(file, &log_path, *offset, &mut attempt)
        });
    let result = result.and_then(|()| match hooks {
        Some(v) => v?.check(),
        None => Ok(()),
    });
    if let Some(mut attempt) = attempt {
        attempt.completed_at = Some(files::now());
        attempt.status = match &result {
            Ok(()) if attempt.hook_exit_code == Some(0) && attempt.failure.is_none() => "succeeded",
            Ok(()) => "unknown",
            Err(Error::Cancelled) => "cancelled",
            Err(_) => "failed",
        }
        .into();
        if result.is_err() && !matches!(result, Err(Error::Cancelled)) && attempt.failure.is_none()
        {
            attempt.failure = Some(
                if attempt.process_exit_code.is_some_and(|c| c != 0)
                    || attempt.hook_exit_code.is_some_and(|c| c != 0)
                {
                    "hook_nonzero"
                } else {
                    "preparation_failed"
                }
                .into(),
            );
        }
        let recorded = crate::fenix_diagnostics::save(&evidence, &attempt).is_ok();
        ctx.update(json!({"diagnostics_recorded":recorded}));
        if !recorded {
            eprintln!("Steam Fenix diagnostic result could not be saved.");
        }
    }
    result
}
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    if operation == "cancel" || operation == "stop" {
        let mut s = app.lock();
        Launcher::open(&s)?;
        if let Some(active) = s.active.as_ref().filter(|active| active.kind == "fenix") {
            let job = s
                .jobs
                .get("fenix")
                .ok_or(Error::Invalid("Dieser Vorgang ist nicht mehr aktuell."))?;
            require(
                s.steam
                    .as_ref()
                    .is_some_and(|item| job["installation_id"] == item.id),
                "Die Installation hat sich geändert.",
            )?;
            require(
                operation == "stop" || data["job_id"] == active.id,
                "Dieser Vorgang ist nicht mehr aktuell.",
            )?;
            active.cancel.store(true, Ordering::Relaxed);
            if let Some(job) = s.jobs.get_mut("fenix") {
                job["can_cancel"] = json!(false);
                job["message"] = json!("Vorgang wird abgebrochen …");
            }
            return Ok(json!({"ok":true}));
        }
        require(
            operation != "cancel",
            "Dieser Vorgang ist nicht mehr aktuell.",
        )?;
    }
    require(
        [
            "install",
            "configure",
            "restore",
            "select",
            "installer",
            "manager",
            "open",
            "stop",
            "repair",
        ]
        .contains(&operation),
        "Diese Fenix-Aktion ist für Steam nicht verfügbar.",
    )?;
    let item = app
        .lock()
        .steam
        .clone()
        .ok_or(Error::Invalid("Keine Steam-Installation ausgewählt."))?;
    item.validate()?;
    if [
        "install",
        "restore",
        "select",
        "configure",
        "installer",
        "repair",
    ]
    .contains(&operation)
    {
        require(
            !steam::steam_running(),
            "Steam vollständig beenden, bevor der Fenix-Patch oder seine Proton-Zuordnung geändert wird.",
        )?;
    }
    let operation = operation.to_string();
    let body = data.clone();
    app.start_job("fenix",&operation.clone(),false,move|ctx|{
        let current=ctx.launcher.lock().steam.clone().ok_or(Error::Invalid("Die Installation hat sich geändert."))?;
        require(current==item,"Die Installation hat sich geändert.")?;item.validate()?;
        ctx.update(json!({"runtime_path":item.context(),"installation_id":item.id,"phase":"prepare_steam"}));
        if operation=="stop" {
            require(!crate::steam_session::running(&item)?,"Beende zuerst den Simulator.")?;
            let system=engine(&ctx.launcher.state_dir,&item)?;let target=item.target()?;
            let _held=steam::locked(&system,&target,false,false).map_err(map_error)?;
            let state=steam::installed_state(&system,&target).map_err(map_error)?;
            use fenix_patch::json::Dict;
            let runner=fenix_patch::fsx::contained(&target.tools(),state.need_str("tool").map_err(map_error)?).map_err(map_error)?;
            wine_processes::stop_prefix(&item.prefix(),&runner,wine_processes::FENIX)?;
        }else {
            let (payload,manifest)=if operation=="install"{
                let selected=crate::fenix_update::selected(ctx)?;
                let payload=fenix_bundle::obtain(&ctx.launcher.state_dir.join("fenix-bundles"),body["bundle_path"].as_str(),ctx)?;
                let manifest=fenix_bundle::payload_manifest(&payload,None)?;
                let evidence=ctx.launcher.state_dir.join("steam").join(&item.id).join("private");files::private_dir(&evidence)?;
                files::atomic_json(&evidence.join("fenix-patch-release.json"),&json!({"version":manifest["version"],"bundle_manifest":manifest,"bundle_release":selected["release"]}))?;
                (payload,manifest)
            }else{let system=engine(&ctx.launcher.state_dir,&item)?;(system.bundle,system.manifest)};
            ctx.update(json!({"phase":"setup","transfer":null,"progress":null}));
            let system=Engine::new(payload.clone(),manifest,Arc::clone(&ctx.cancel));
            if ["installer","manager","open","repair"].contains(&operation.as_str()) {
                windows_app(ctx,&item,&system,&operation,body["installer_path"].as_str())?;
            }else{
                let result=perform(&system,&item,&operation,&payload,None,&|message|{ctx.progress(message);Ok(())});
                if let Err(error)=&result{eprintln!("Steam Fenix operation failed: {}",error.to_string().chars().take(8192).collect::<String>());}
                result.map_err(map_error)?;
            }
        }
        Ok(json!({"state":"complete","phase":"complete","message":"Steam-Fenix-Vorgang abgeschlossen.","detail":null}))
    })
}
