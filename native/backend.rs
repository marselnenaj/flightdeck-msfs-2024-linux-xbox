// SPDX-License-Identifier: MIT
//! One coordinator owns the selected runtime, jobs, processes and their leases.
use crate::{Error, Result, error::require, files, games::Game, runtime};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    process::Child,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
pub struct Active {
    pub id: String,
    pub kind: String,
    pub runtime: Option<PathBuf>,
    pub lease: Option<files::Lease>,
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}
pub struct State {
    pub installations: crate::installations::Registry,
    pub steam: Option<crate::installations::Installation>,
    pub steam_session: Option<crate::steam_session::Session>,
    pub(crate) steam_helpers: Option<crate::steam_companions::Session>,
    pub steam_notice: String,
    pub runtime: Option<PathBuf>,
    pub known: BTreeMap<String, PathBuf>,
    pub process: Option<Child>,
    pub started: Option<Instant>,
    pub started_at: Option<String>,
    pub exit_code: Option<i32>,
    pub stopping: bool,
    pub closing: bool,
    pub active: Option<Active>,
    pub jobs: BTreeMap<String, Value>,
    pub plans: BTreeMap<String, Value>,
    pub cloud: Value,
    pub cloud_data: crate::cloud_sync::State,
    pub graphics_report: Value,
    pub installation_report: Value,
    pub maintenance: Option<crate::maintenance::Plan>,
    pub mod_removal: Option<crate::mods::Removal>,
    pub launcher_updates: crate::launcher_update::State,
    pub startup_updates: crate::startup_updates::State,
    pub fenix_updates: crate::fenix_update::State,
}
pub struct Launcher {
    pub state_dir: PathBuf,
    pub state: Mutex<State>,
}
pub struct Context {
    pub launcher: Arc<Launcher>,
    pub id: String,
    pub kind: String,
    pub runtime: Option<PathBuf>,
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}
impl Context {
    pub fn progress(&self, message: &str) {
        let mut s = self.launcher.lock();
        if self.kind == "cloud-auto" && s.cloud["request_id"] == self.id {
            s.cloud["message"] = json!(message.chars().take(1500).collect::<String>());
        }
        if let Some(job) = s.jobs.get_mut(&self.kind)
            && job["id"] == self.id
        {
            job["message"] = json!(message.chars().take(1500).collect::<String>());
        }
    }
    pub fn update(&self, patch: Value) {
        let mut s = self.launcher.lock();
        if let Some(job) = s.jobs.get_mut(&self.kind)
            && job["id"] == self.id
            && let (Some(target), Some(patch)) = (job.as_object_mut(), patch.as_object())
        {
            target.extend(patch.clone());
        }
    }
    pub fn begin_commit(&self, phase: &str, message: &str) -> Result<()> {
        let mut s = self.launcher.lock();
        self.interrupted()?;
        require(
            s.active.as_ref().is_some_and(|a| a.id == self.id),
            "Dieser Vorgang ist nicht mehr aktuell.",
        )?;
        let job = s
            .jobs
            .get_mut(&self.kind)
            .ok_or(Error::Invalid("Dieser Vorgang ist nicht mehr aktuell."))?;
        job["phase"] = json!(phase);
        job["message"] = json!(message);
        job["can_pause"] = json!(false);
        job["can_resume"] = json!(false);
        job["can_cancel"] = json!(false);
        Ok(())
    }
    pub fn interrupted(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    pub fn root(&self) -> Result<&Path> {
        self.runtime
            .as_deref()
            .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))
    }
    pub fn lease(&self) -> Result<File> {
        let s = self.launcher.lock();
        let active = s
            .active
            .as_ref()
            .filter(|a| a.id == self.id)
            .ok_or(Error::Invalid("Dieser Vorgang ist nicht mehr aktuell."))?;
        Ok(active
            .lease
            .as_ref()
            .ok_or(Error::Invalid("Die Runtime ist nicht reserviert."))?
            .0
            .try_clone()?)
    }
}
impl Launcher {
    pub fn new(state_dir: PathBuf, selected: Option<&str>) -> Result<Arc<Self>> {
        files::private_dir(&state_dir)?;
        let state_dir = state_dir.canonicalize()?;
        // Undo our temporary desktop change even if a later profile read fails.
        // Keep the launcher available for diagnostics when live reload is pending;
        // the launch preflight retries restoration before any Wine/auth helper.
        if let Err(error) =
            crate::display_preflight::restore_pending(&state_dir, &AtomicBool::new(false))
        {
            eprintln!("Display recovery is pending: {error}");
        }
        crate::installations::recover(&state_dir)?;
        let mut runtime = None;
        let mut known = BTreeMap::new();
        if let Ok(value) = runtime::value(&state_dir.join("config.json")) {
            runtime = value["runtime_path"]
                .as_str()
                .and_then(|v| runtime::validate(v).ok());
            if let Some(map) = value["runtimes"].as_object() {
                for (id, path) in map {
                    if let (Ok(game), Some(path)) = (Game::select(id), path.as_str())
                        && let Ok(path) = runtime::validate(path)
                        && Game::for_runtime(&path).ok() == Some(game)
                    {
                        known.insert(id.clone(), path);
                    }
                }
            }
        }
        if let Some(root) = &runtime
            && let Ok(game) = Game::for_runtime(root)
        {
            known.insert(game.id().into(), root.clone());
        }
        let installation_report = crate::setup_diagnostics::load(&state_dir);
        let mut installations = crate::installations::load(&state_dir)?;
        let mut migrated = false;
        for path in known.values().chain(runtime.iter()) {
            if let Ok(game) = Game::for_runtime(path)
                && !installations.entries.iter().any(
                    |i| matches!(&i.source,crate::installations::Source::Xbox{path:p} if p==path),
                )
            {
                installations
                    .entries
                    .push(crate::installations::Installation::new(
                        game,
                        crate::installations::Source::Xbox { path: path.clone() },
                    )?);
                migrated = true;
            }
        }
        if installations.selected.is_none()
            && let Some(path) = &runtime
        {
            installations.selected = installations
                .entries
                .iter()
                .find(|i| matches!(&i.source,crate::installations::Source::Xbox{path:p} if p==path))
                .map(|i| i.id.clone());
            migrated = true;
        }
        if migrated {
            crate::installations::save(&state_dir, &installations)?;
        }
        let steam = installations
            .entries
            .iter()
            .find(|i| Some(&i.id) == installations.selected.as_ref() && i.steam())
            .cloned();
        let app = Arc::new(Self {
            state_dir,
            state: Mutex::new(State {
                installations,
                steam,
                steam_session: None,
                steam_helpers: None,
                steam_notice: String::new(),
                runtime,
                known,
                process: None,
                started: None,
                started_at: None,
                exit_code: None,
                stopping: false,
                closing: false,
                active: None,
                jobs: BTreeMap::new(),
                plans: BTreeMap::new(),
                cloud: json!({"enabled":true,"state":"idle","phase":null,"message":"","error_code":null,"error_details":{},"can_retry":false,"can_play_local":false,"can_cancel":false,"request_id":null,"last_synced_at":null,"conflict":false,"summary":null}),
                cloud_data: crate::cloud_sync::State::default(),
                graphics_report: Value::Null,
                installation_report,
                maintenance: None,
                mod_removal: None,
                launcher_updates: crate::launcher_update::State::default(),
                startup_updates: crate::startup_updates::State::default(),
                fenix_updates: crate::fenix_update::State::default(),
            }),
        });
        if let Some(path) = selected {
            app.configure(path)?;
        }
        Ok(app)
    }
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn root(&self) -> Result<PathBuf> {
        let s = self.lock();
        require(
            s.steam.is_none(),
            "Diese Aktion gehört zur Xbox-Installation. Für Steam die Steam-Verwaltung verwenden.",
        )?;
        s.runtime
            .clone()
            .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))
    }
    pub fn poll(s: &mut State) {
        if let Some(child) = s.process.as_mut()
            && let Ok(Some(code)) = child.try_wait()
        {
            s.exit_code = code.code().or(Some(-1));
            s.stopping = false;
            if !s.active.as_ref().is_some_and(|a| a.kind == "cloud-auto") {
                s.process = None;
            }
        }
    }
    pub fn context(s: &State) -> String {
        s.steam.as_ref().map(|i| i.context()).unwrap_or_else(|| {
            s.runtime
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    }
    pub fn external(s: &State) -> bool {
        if let Some(item) = &s.steam {
            return s
                .steam_session
                .as_ref()
                .is_some_and(|v| !v.seen && v.requested.elapsed().as_secs() < 180)
                || s.steam_helpers.as_ref().is_some_and(|h| !h.finished())
                || crate::steam_session::running(item).unwrap_or(true);
        }
        let Some(root) = &s.runtime else {
            return false;
        };
        if s.process.is_some()
            || s.active
                .as_ref()
                .is_some_and(|a| a.runtime.as_ref() == Some(root) && a.lease.is_some())
        {
            return false;
        }
        match files::Lease::acquire(&root.join("private/play.lock"), false) {
            Ok(_) => false,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => true,
        }
    }
    pub fn open(s: &State) -> Result<()> {
        require(
            !s.closing,
            "Der Launcher wird für ein Update neu gestartet. Bitte gleich erneut versuchen.",
        )
    }
    pub fn idle(s: &mut State) -> Result<()> {
        Self::open(s)?;
        Self::poll(s);
        require(
            s.active.is_none() && s.process.is_none() && !Self::external(s),
            "Die Einrichtung benötigt ein beendetes Spiel und darf nur einmal laufen.",
        )
    }
    pub fn configure(&self, path: &str) -> Result<Value> {
        let selected = runtime::validate(path)?;
        let mut s = self.lock();
        Self::idle(&mut s)?;
        let mut known = s.known.clone();
        if let Ok(game) = Game::for_runtime(&selected) {
            known.insert(game.id().into(), selected.clone());
        }

        let item = crate::installations::Installation::new(
            Game::for_runtime(&selected)?,
            crate::installations::Source::Xbox {
                path: selected.clone(),
            },
        )?;
        let mut registry = s.installations.clone();
        registry.entries.retain(|i| i.id != item.id);
        registry.selected = Some(item.id.clone());
        registry.entries.push(item);
        crate::installations::commit_selection(
            &self.state_dir,
            &registry,
            &json!({"schema":1,"runtime_path":selected,"runtimes":known}),
        )?;
        s.installations = registry;
        s.steam = None;
        s.steam_session = None;
        s.steam_helpers = None;
        s.runtime = Some(selected);
        s.known = known;
        s.exit_code = None;
        s.graphics_report = Value::Null;
        Ok(json!({"ok":true}))
    }
    pub fn register(&self, path: &str) -> Result<Value> {
        let selected = runtime::validate(path)?;
        let game = Game::for_runtime(&selected)?;
        require(
            runtime::ready(&selected),
            "Diese MSFS-Installation ist noch nicht startbereit.",
        )?;
        let mut s = self.lock();
        Self::open(&s)?;
        if s.runtime.is_none() {
            drop(s);
            return self.configure(path);
        }
        let mut known = s.known.clone();
        known.insert(game.id().into(), selected.clone());
        let item = crate::installations::Installation::new(
            game,
            crate::installations::Source::Xbox { path: selected },
        )?;
        let mut registry = s.installations.clone();
        registry.entries.retain(|i| i.id != item.id);
        registry.entries.push(item);
        crate::installations::commit_selection(
            &self.state_dir,
            &registry,
            &json!({"schema":1,"runtime_path":s.runtime,"runtimes":known}),
        )?;
        s.known = known;
        s.installations = registry;
        Ok(json!({"ok":true}))
    }
    pub fn select_game(&self, id: &str) -> Result<Value> {
        let game = Game::select(id)?;
        let s = self.lock();
        let versions = runtime::versions(s.runtime.as_deref(), &s.known);
        let item = &versions[game.id()];
        require(
            item["installed"] == true,
            "Diese MSFS-Version ist noch nicht installiert. Bitte zuerst einrichten.",
        )?;
        let path = string(item, "path")?.to_string();
        drop(s);
        self.configure(&path)?;
        // Readiness is a launch guard, not an installation-discovery guard.
        // An inactive edition can still need the bundled component migration
        // after a launcher update. Select it before applying that migration;
        // other failed checks must remain visible and repairable in the UI.
        if matches!(
            crate::components::state(Some(Path::new(&path))),
            "pending" | "interrupted"
        ) {
            crate::components::refresh(self)?;
        }
        Ok(json!({"ok":true}))
    }
    pub fn status(&self) -> Value {
        if self.lock().steam.is_some() {
            return crate::steam::status(self);
        }
        let mut s = self.lock();
        Self::poll(&mut s);
        let checks = runtime::checks(s.runtime.as_deref());
        let ready = checks.iter().all(|v| v["ok"] == true);
        let game = s
            .runtime
            .as_deref()
            .map(Game::for_runtime)
            .transpose()
            .ok()
            .flatten()
            .unwrap_or(Game::Msfs2024);
        let state = if s.process.is_some() {
            if s.stopping {
                "stopping"
            } else if s.started.is_some_and(|v| v.elapsed().as_secs() < 15) {
                "starting"
            } else {
                "running"
            }
        } else if Self::external(&s) {
            "external"
        } else {
            "stopped"
        };
        let idle = state == "stopped" && s.active.is_none() && !s.closing;
        let cloud = crate::cloud_sync::automatic(&s);
        let automatic = cloud["enabled"] == true;
        json!({"app":{"name":"Flightdeck","version":crate::VERSION},"runtime":{"configured":s.runtime.is_some(),"path":s.runtime.as_ref().map(|v|v.to_string_lossy().into_owned()).unwrap_or_default(),"ready":ready,"checks":checks,"game_id":game.id(),"game_name":game.name()},"versions":runtime::versions(s.runtime.as_deref(),&s.known),"graphics":crate::graphics::snapshot(s.runtime.as_deref()),"vr":crate::vr::launcher_snapshot(&s),"game":{"state":state,"managed":s.process.is_some(),"can_start":ready&&idle,"can_stop":s.process.is_some()&&!s.stopping,"started_at":s.started_at,"exit_code":s.exit_code},"saves":runtime::saves(s.runtime.as_deref(),idle),"setup":{"busy":s.active.is_some()},"cloud":cloud,"support":{"level":"experimental","cloud_saves":automatic,"cloud_sync":if automatic{"automatic"}else{"local"},"automatic_cloud_sync":automatic}})
    }
    pub fn reserve(
        self: &Arc<Self>,
        kind: &str,
        operation: &str,
        needs_runtime: bool,
    ) -> Result<Context> {
        let mut s = self.lock();
        self.reserve_locked(&mut s, kind, operation, needs_runtime)
    }
    pub(crate) fn reserve_locked(
        self: &Arc<Self>,
        s: &mut State,
        kind: &str,
        operation: &str,
        needs_runtime: bool,
    ) -> Result<Context> {
        Self::idle(s)?;
        let root = if s.steam.is_some() {
            None
        } else {
            s.runtime.clone()
        };
        require(
            !needs_runtime || root.is_some(),
            "Zuerst eine Runtime auswählen.",
        )?;
        let lease = if needs_runtime {
            Some(files::Lease::acquire(
                &root
                    .as_ref()
                    .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?
                    .join("private/play.lock"),
                true,
            )?)
        } else {
            None
        };
        let id = uuid::Uuid::new_v4().simple().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let job_context = if let Some(item) = &s.steam {
            json!(item.context())
        } else {
            json!(root)
        };
        let installation_id = s.steam.as_ref().map(|i| i.id.clone());
        s.jobs.insert(kind.into(),json!({"id":id,"operation":operation,"state":"running","runtime_path":job_context,"installation_id":installation_id,"message":"Vorgang wird vorbereitet …","started_at":files::now()}));
        s.active = Some(Active {
            id: id.clone(),
            kind: kind.into(),
            runtime: root.clone(),
            lease,
            cancel: Arc::clone(&cancel),
            pause: Arc::clone(&pause),
        });
        Ok(Context {
            launcher: Arc::clone(self),
            id,
            kind: kind.into(),
            runtime: root,
            cancel,
            pause,
        })
    }
    pub fn finish(&self, ctx: &Context, outcome: Result<Value>, retain: bool) {
        let mut s = self.lock();
        if !s.active.as_ref().is_some_and(|a| a.id == ctx.id) {
            return;
        }
        if let Some(job) = s.jobs.get_mut(&ctx.kind) {
            match outcome {
                Ok(extra) => {
                    if let (Some(job), Some(extra)) = (job.as_object_mut(), extra.as_object()) {
                        job.extend(extra.clone());
                    }
                    if job["state"] == "running" {
                        job["state"] = json!("complete");
                    }
                    if !retain {
                        job["completed_at"] = json!(files::now());
                    }
                }
                Err(error) => {
                    job["state"] = json!(if matches!(error, Error::Cancelled) {
                        "cancelled"
                    } else {
                        "failed"
                    });
                    job["message"] = json!(error.to_string());
                    job["error"] = json!(error.to_string());
                    if job["phase"] != "download_failed" {
                        job["failure_phase"] = job["phase"].clone();
                    }
                    job["phase"] = job["state"].clone();
                    job["can_pause"] = json!(false);
                    job["can_resume"] = json!(false);
                    job["can_retry"] = json!(false);
                    job["transfer"] = Value::Null;
                    job["auth_required"] = json!(matches!(error, Error::AuthRequired(_)));
                    if let Error::Cloud(error) = &error {
                        job["state"] =
                            json!(if error.code == "cancelled" && !error.recovery_required {
                                "cancelled"
                            } else {
                                "failed"
                            });
                        job["error_code"] = json!(error.code);
                        job["recovery_required"] = json!(error.recovery_required);
                        job["committed_containers"] = json!(error.committed_containers);
                        job["result"] = Value::Null;
                        job["finished_at"] = json!(files::now());
                        job["error_details"] = json!({"http_status":error.http_status,"native_hresult":error.native_hresult});
                        if let (Some(target), Some(extra)) = (
                            job["error_details"].as_object_mut(),
                            error.detail.as_ref().and_then(Value::as_object),
                        ) {
                            target.extend(extra.clone());
                        }
                        job["auth_required"] = json!(error.code == "authentication");
                    }
                }
            }
        }
        if ctx.kind == "setup"
            && let Some(job) = s.jobs.get("setup")
        {
            s.installation_report = crate::setup_diagnostics::persist(&self.state_dir, job);
        }
        if !retain
            || s.jobs
                .get(&ctx.kind)
                .is_some_and(|v| v["state"] == "failed" || v["state"] == "cancelled")
        {
            s.active = None;
        }
    }
    pub fn start_job<F>(
        self: &Arc<Self>,
        kind: &str,
        operation: &str,
        needs_runtime: bool,
        work: F,
    ) -> Result<Value>
    where
        F: FnOnce(&Context) -> Result<Value> + Send + 'static,
    {
        let ctx = self.reserve(kind, operation, needs_runtime)?;
        let id = ctx.id.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&ctx)))
                .unwrap_or(Err(Error::Invalid("Der Vorgang wurde unerwartet beendet.")));
            ctx.launcher.finish(&ctx, result, false);
        });
        Ok(json!({"ok":true,"job_id":id}))
    }
    pub fn continuation(self: &Arc<Self>, kind: &str, id: &str) -> Result<Context> {
        let mut s = self.lock();
        Self::open(&s)?;
        require(
            s.jobs
                .get(kind)
                .is_some_and(|v| v["id"] == id && v["state"] == "ready"),
            "Bitte die ausgewählten Dateien zuerst erneut prüfen.",
        )?;
        if s.active.is_none() {
            require(
                s.jobs.get(kind).is_some_and(|v| {
                    v["mode"] == "update"
                        && v["runtime_path"].as_str() == s.runtime.as_ref().and_then(|p| p.to_str())
                }),
                "Das installierte Spiel hat sich geändert. Bitte Updates erneut prüfen.",
            )?;
            Self::idle(&mut s)?;
            let root = s
                .runtime
                .clone()
                .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?;
            let lease = files::Lease::acquire(&root.join("private/play.lock"), true)?;
            s.active = Some(Active {
                id: id.into(),
                kind: kind.into(),
                runtime: Some(root),
                lease: Some(lease),
                cancel: Arc::new(AtomicBool::new(false)),
                pause: Arc::new(AtomicBool::new(false)),
            });
        }
        let active = s
            .active
            .as_ref()
            .filter(|a| a.kind == kind && a.id == id && !a.cancel.load(Ordering::Relaxed))
            .ok_or(Error::Invalid("Dieser Vorgang ist nicht mehr aktuell."))?;
        let ctx = Context {
            launcher: Arc::clone(self),
            id: id.into(),
            kind: kind.into(),
            runtime: active.runtime.clone(),
            cancel: Arc::clone(&active.cancel),
            pause: Arc::clone(&active.pause),
        };
        if let Some(job) = s.jobs.get_mut(kind) {
            job["state"] = json!("installing");
            job["phase"] = json!("recheck");
            job["message"] = json!("Eingaben werden vor der Übernahme erneut geprüft …");
        }
        Ok(ctx)
    }
    pub fn activate(&self, ctx: &Context, path: &Path) -> Result<()> {
        let root = runtime::validate(
            path.to_str()
                .ok_or(Error::Invalid("Ungültiger Runtimepfad."))?,
        )?;
        let mut s = self.lock();
        ctx.interrupted()?;
        require(
            s.active.as_ref().is_some_and(|a| a.id == ctx.id),
            "Dieser Vorgang ist nicht mehr aktuell.",
        )?;
        let game = Game::for_runtime(&root)?;
        let mut known = s.known.clone();
        known.insert(game.id().into(), root.clone());
        let item = crate::installations::Installation::new(
            game,
            crate::installations::Source::Xbox { path: root.clone() },
        )?;
        let mut registry = s.installations.clone();
        registry.entries.retain(|i| i.id != item.id);
        registry.selected = Some(item.id.clone());
        registry.entries.push(item);
        crate::installations::commit_selection(
            &self.state_dir,
            &registry,
            &json!({"schema":1,"runtime_path":root,"runtimes":known}),
        )?;
        s.installations = registry;
        s.runtime = Some(root);
        s.steam = None;
        s.steam_session = None;
        s.steam_helpers = None;
        s.known = known;
        s.exit_code = None;
        Ok(())
    }
    pub fn cancel(&self, kind: &str, id: &str) -> Result<Value> {
        let mut s = self.lock();
        require(
            !s.jobs.get(kind).is_some_and(|v| {
                v["id"] == id
                    && (v["phase"] == "switch_update"
                        || v["phase"] == "publish_runtime"
                        || v["phase"] == "removing")
            }),
            "Der atomare Spielwechsel wird gerade abgeschlossen. Bitte kurz warten.",
        )?;
        if s.jobs
            .get(kind)
            .is_some_and(|v| v["id"] == id && v["mode"] == "update" && v["state"] == "ready")
            && !s.active.as_ref().is_some_and(|v| v.id == id)
        {
            if let Some(job) = s.jobs.get_mut(kind) {
                job["state"] = json!("cancelled");
                job["message"] = json!("Einrichtung abgebrochen.");
            }
            s.plans.remove(kind);
            return Ok(json!({"ok":true}));
        }
        let active = s
            .active
            .as_ref()
            .filter(|a| a.kind == kind && a.id == id)
            .ok_or(Error::Invalid("Dieser Vorgang ist nicht mehr aktuell."))?;
        active.cancel.store(true, Ordering::Relaxed);
        let ready = s.jobs.get(kind).is_some_and(|v| v["state"] == "ready");
        if let Some(job) = s.jobs.get_mut(kind) {
            job["can_cancel"] = json!(false);
            job["message"] = json!("Vorgang wird abgebrochen …");
            if ready {
                job["state"] = json!("cancelled");
            }
        }
        if ready {
            s.active = None;
            s.plans.remove(kind);
        }
        Ok(json!({"ok":true}))
    }
    pub fn job(&self, kind: &str) -> Value {
        self.lock().jobs.get(kind).cloned().unwrap_or(Value::Null)
    }
    pub fn save_backup(&self) -> Result<Value> {
        let mut s = self.lock();
        Self::idle(&mut s)?;
        let root = s
            .runtime
            .as_deref()
            .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?;
        require(
            runtime::saves(Some(root), true)["can_backup"] == true,
            "Ein Backup benötigt vorhandene lokale Spielstände und ein beendetes Spiel.",
        )?;
        let _lease = files::Lease::acquire(&root.join("private/play.lock"), true)?;
        runtime::backup(root, false)
    }
    pub fn configure_setting(
        &self,
        data: &Value,
        file: &str,
        key: &str,
        choices: &[&str],
    ) -> Result<Value> {
        let selected = string(data, "runtime_path")?;
        let value = string(data, key)?;
        require(choices.contains(&value), "Ungültige Einstellung.")?;
        let mut s = self.lock();
        Self::idle(&mut s)?;
        let root = s
            .runtime
            .as_deref()
            .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?;
        require(
            root == Path::new(selected),
            "Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.",
        )?;
        let _lease = files::Lease::acquire(&root.join("private/play.lock"), true)?;
        let mut config = json!({"schema":1});
        config[key] = json!(value);
        files::atomic_json(&root.join("private").join(file), &config)?;
        Ok(json!({"ok":true}))
    }
    pub fn stop(&self) -> Result<Value> {
        let mut s = self.lock();
        Self::poll(&mut s);
        let child = s.process.as_mut().ok_or(Error::Invalid(
            "Es läuft kein von Flightdeck gestartetes Spiel.",
        ))?;
        if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            rustix::process::kill_process(pid, rustix::process::Signal::TERM)?;
        }
        s.stopping = true;
        Ok(json!({"ok":true}))
    }
    pub fn refresh(&self) -> Value {
        let mut s = self.lock();
        let idle = Self::idle(&mut s).is_ok();
        if idle {
            s.closing = true;
        }
        json!({"ok":true,"refresh":if idle{"restarting"}else{"busy"}})
    }
    pub fn close(&self) {
        let mut s = self.lock();
        s.closing = true;
        if let Some(cancel) = &s.startup_updates.worker {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(cancel) = &s.fenix_updates.worker {
            cancel.store(true, Ordering::Relaxed);
        }
        if s.launcher_updates.job["can_cancel"] == true
            && let Some(cancel) = &s.launcher_updates.worker
        {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(active) = s.active.as_ref()
            && s.jobs
                .get(&active.kind)
                .is_some_and(|j| j["state"] == "ready")
        {
            let kind = active.kind.clone();
            active.cancel.store(true, Ordering::Relaxed);
            if let Some(job) = s.jobs.get_mut(&kind) {
                job["state"] = json!("cancelled");
            }
            s.plans.remove(&kind);
            s.active = None;
        }
        if let Some(active) = &s.active
            && (active.kind != "cloud-auto"
                || s.cloud["phase"] == "before_start" && s.process.is_none())
        {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }
}
pub fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Invalid("Ungültige Anfrageparameter."))
}
