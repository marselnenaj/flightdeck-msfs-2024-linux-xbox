// SPDX-License-Identifier: MIT
//! Manual transfers and private review capabilities owned by the launcher coordinator.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    cloud::{self, Failure},
    cloud_fs as fs, cloud_import as ci,
    cloud_runtime::Client,
    cloud_storage::{self as cs, Transport},
    cloud_write as cw, files, runtime,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub(crate) struct Review {
    pub runtime: PathBuf,
    pub id: String,
    pub deadline: Instant,
    pub plan: ci::Plan,
}
impl Review {
    pub fn new(runtime: PathBuf, plan: ci::Plan) -> Self {
        Self {
            runtime,
            id: uuid::Uuid::new_v4().simple().to_string(),
            deadline: Instant::now() + Duration::from_secs(900),
            plan,
        }
    }
    pub fn current(&self, runtime: Option<&std::path::Path>) -> bool {
        Some(self.runtime.as_path()) == runtime && Instant::now() < self.deadline
    }
    fn summary(&self) -> Value {
        let mut value = self.plan.summary();
        value["id"] = json!(self.id);
        value
    }
}
#[derive(Default)]
pub struct State {
    pub(crate) plan: Option<Review>,
    pub(crate) restore: Option<(PathBuf, String)>,
    pub(crate) auto_runtime: Option<PathBuf>,
    pub(crate) review: Option<Review>,
    pub(crate) binding: Option<String>,
}
pub fn automatic(s: &crate::backend::State) -> Value {
    let enabled = s
        .runtime
        .as_deref()
        .is_some_and(|root| crate::cloud_runtime::available(root, true));
    automatic_enabled(s, enabled)
}
fn automatic_enabled(s: &crate::backend::State, enabled: bool) -> Value {
    let current = s.cloud_data.auto_runtime == s.runtime;
    let mut value = if current {
        s.cloud.clone()
    } else {
        json!({"state":"idle","phase":null,"message":"Cloud-Spielstände werden vor dem Start und nach dem Beenden automatisch abgeglichen.","error_code":null,"error_details":{},"request_id":null,"last_synced_at":null,"timings":{}})
    };
    let active = current && s.active.as_ref().is_some_and(|a| a.kind == "cloud-auto");
    // A durable fence outlives the service's in-memory cloud state. Surface it
    // only when idle; every legitimate running session also has this fence.
    let pending = if enabled && s.active.is_none() && s.process.is_none() && !s.closing {
        s.runtime.as_deref().and_then(|root| {
            crate::cloud_process_guard::recovery_id(root)
                .unwrap_or_else(|_| Some("interrupted-invalid".into()))
        })
    } else {
        None
    };
    if let Some(id) = &pending {
        if !current || value["error_code"] != "unsafe_session" {
            value["request_id"] = json!(id);
        }
        value["state"] = json!("attention");
        value["phase"] = json!("before_start");
        value["error_code"] = json!("unsafe_session");
        value["message"] = json!(
            "Die vorherige Spielsitzung ist noch nicht freigegeben. Erneut versuchen prüft, ob alle zugehörigen Prozesse beendet sind, und sichert die lokalen Spielstände."
        );
    }
    let attention = enabled
        && (current || pending.is_some())
        && value["state"] == "attention"
        && !active
        && !s.closing;
    let review = current && pending.is_none() && s.cloud_data.review.is_some();
    let code = value["error_code"].as_str().unwrap_or("");
    // An interrupted session gets a recovery-only retry: verify no owned
    // writers remain and preserve a local backup before clearing its fence.
    let retry = attention;
    let sign_in = attention && matches!(code, "authentication" | "auth_required" | "unauthorized");
    let local = attention
        && !review
        && (value["phase"] == "before_start"
            && !matches!(code, "unsafe_session" | "graphics" | "framework" | "launch")
            || value["phase"] == "after_exit"
                && matches!(
                    code,
                    "authentication"
                        | "auth_required"
                        | "unauthorized"
                        | "forbidden"
                        | "lease_lost"
                        | "transport"
                        | "deadline"
                        | "readback"
                        | "quota"
                        | "failed"
                        | "cancelled"
                ));
    value["enabled"] = json!(enabled);
    value["can_retry"] = json!(retry);
    value["can_sign_in"] = json!(sign_in);
    value["can_play_local"] = json!(local);
    value["can_cancel"] =
        json!(active && value["state"] == "syncing" && value["phase"] == "before_start");
    value["conflict"] = json!(attention && review);
    value["summary"] = if attention && review {
        s.cloud_data
            .review
            .as_ref()
            .map(|r| r.plan.summary())
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    value
}

pub fn snapshot(app: &Arc<Launcher>) -> Value {
    let mut s = app.lock();
    Launcher::poll(&mut s);
    let root = s.runtime.as_deref();
    let available = root.is_some_and(|r| crate::cloud_runtime::available(r, false));
    let writable = root.is_some_and(|r| crate::cloud_runtime::available(r, true));
    let busy = s.active.is_some() || s.process.is_some() || s.closing || Launcher::external(&s);
    let local = runtime::saves(root, false)["available"] == true;
    let plan = s.cloud_data.plan.as_ref().filter(|p| p.current(root));
    let restore = s
        .cloud_data
        .restore
        .as_ref()
        .filter(|(r, _)| Some(r.as_path()) == root)
        .map(|(_, id)| id);
    let job = s
        .jobs
        .get("cloud")
        .filter(|j| j["runtime_path"].as_str() == root.and_then(|p| p.to_str()))
        .cloned()
        .unwrap_or(Value::Null);
    json!({"available":available,"mode":if writable{"automatic_sync"}else{"download_and_import"},"sync_supported":writable,"automatic_sync":writable,"automatic":automatic(&s),
        "can_upload":writable&&local&&plan.is_some()&&!busy,"can_restore":available&&local&&restore.is_some()&&!busy,"restore_id":restore,"plan":plan.map(Review::summary),
        "can_prepare_import":available&&local&&!busy,"can_import":available&&local&&plan.is_some()&&!busy,"can_check":available&&!busy,"can_download":available&&!busy,"can_cancel":job["state"]=="running","job":job})
}
pub fn discard(app: &Arc<Launcher>, id: &str) -> Result<Value> {
    let mut s = app.lock();
    crate::error::require(
        s.cloud_data
            .plan
            .as_ref()
            .is_some_and(|p| p.id == id && p.current(s.runtime.as_deref())),
        "Dieser Spielstandvergleich ist nicht mehr gültig. Bitte erneut vergleichen.",
    )?;
    s.cloud_data.plan = None;
    Ok(json!({"ok":true}))
}
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    crate::error::require(
        matches!(
            operation,
            "check" | "download" | "prepare-import" | "import" | "upload" | "restore"
        ),
        "Diese Cloud-Aktion ist nicht verfügbar.",
    )?;
    let ctx = app.reserve("cloud", operation, true)?;
    let selection = (|| -> Result<(Option<ci::Plan>, Option<String>)> {
        let root = ctx.root()?;
        crate::error::require(
            crate::cloud_runtime::available(root, operation == "upload"),
            "Die Cloud-Komponente fehlt. Bitte das aktuelle vollständige Flightdeck-Paket installieren.",
        )?;
        if !matches!(operation, "check" | "download") {
            crate::error::require(
                runtime::saves(Some(root), false)["available"] == true,
                "Für die Übernahme müssen lokale Spielstände in dieser Runtime aktiviert sein.",
            )?;
        }
        let mut s = app.lock();
        let mut selected = None;
        let mut backup = None;
        if matches!(operation, "import" | "upload") {
            let review = s
                .cloud_data
                .plan
                .as_ref()
                .filter(|p| {
                    p.current(Some(root))
                        && data["plan_id"] == p.id
                        && data["choice"]
                            == if operation == "import" {
                                "cloud"
                            } else {
                                "local"
                            }
                })
                .ok_or(Error::Invalid(
                    "Dieser Spielstandvergleich ist nicht mehr gültig. Bitte erneut vergleichen.",
                ))?;
            selected = Some(review.plan.clone());
        }
        if operation == "restore" {
            backup = Some(
                s.cloud_data
                    .restore
                    .as_ref()
                    .filter(|(r, id)| r == root && data["backup_id"] == *id)
                    .ok_or(Error::Invalid(
                        "Diese Spielstandsicherung ist nicht mehr für eine Rücknahme verfügbar.",
                    ))?
                    .1
                    .clone(),
            );
        }
        if !matches!(operation, "check" | "download") {
            s.cloud_data.plan = None;
        }
        Ok((selected, backup))
    })();
    let (plan, backup) = match selection {
        Ok(v) => v,
        Err(e) => {
            app.finish(
                &ctx,
                Ok(json!({"state":"failed","message":e.to_string()})),
                false,
            );
            return Err(e);
        }
    };
    ctx.update(json!({"finished_at":null,"result":null,"error_code":null,"message":"Cloud-Spielstände werden abgefragt …"}));
    let id = ctx.id.clone();
    let operation = operation.to_owned();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            manual(&ctx, &operation, plan.as_ref(), backup.as_deref())
        }))
        .unwrap_or(Err(Error::Cloud(Failure::new("failed"))));
        ctx.launcher.finish(&ctx, result, false);
    });
    Ok(json!({"ok":true,"job_id":id}))
}
fn upload(
    ctx: &Context,
    client: &mut Client,
    plan: &ci::Plan,
    lease: &std::fs::File,
) -> cloud::Result<Value> {
    let scope = client.scope().clone();
    let export = ci::export_local(ctx.root()?, &scope, lease, &ctx.cancel)?;
    cloud::require(export.binding == plan.binding, "invalid_scope")?;
    cloud::require(export.raw_digest == plan.local_digest, "changed")?;
    // Immutable pre-upload backup even if the first remote commit later fails.
    ci::backup(
        &export.local.private,
        &export.binding,
        &export.namespace,
        (export.raw_digest != "missing").then_some(export.encoded.as_slice()),
        &export.encoded,
        None,
    )?;
    let receipt = cw::upload(
        &scope,
        client,
        &export.state()?,
        &plan.remote_digest,
        || export.assert_unchanged(),
        &ctx.cancel,
        &cw::Limits::default(),
    )?;
    let mut result = receipt.summary();
    result["uploaded"] = json!(true);
    result["downloaded"] = json!(false);
    result["rechecked"] = json!(true);
    result["baseline_saved"] = json!(ci::record_common(&export, &receipt).is_ok());
    Ok(result)
}
fn manual(
    ctx: &Context,
    operation: &str,
    selected: Option<&ci::Plan>,
    backup: Option<&str>,
) -> Result<Value> {
    let root = ctx.root()?;
    let lease = ctx.lease()?;
    let mut client = Client::open(root, &lease, Arc::clone(&ctx.cancel))?;
    let scope = client.scope().clone();
    let mut prepared = None;
    let mut result = match operation {
        "upload" => upload(
            ctx,
            &mut client,
            selected.ok_or(Error::Cloud(Failure::new("invalid_plan")))?,
            &lease,
        )?,
        "restore" => {
            let restored = ci::restore(
                root,
                &scope,
                backup.ok_or(Error::Cloud(Failure::new("invalid_plan")))?,
                &lease,
                &ctx.cancel,
            )?;
            let mut result = restored.summary();
            result["restored"] = json!(true);
            result["downloaded"] = json!(false);
            result["rechecked"] = json!(false);
            result
        }
        "check" => {
            let rows = cs::inventory(&mut client, &cs::Limits::default(), &ctx.cancel)?;
            json!({"container_count":rows.len(),"blob_count":null,"total_bytes":rows.iter().map(|r|r.size).sum::<usize>(),"downloaded":false,"rechecked":false})
        }
        _ => {
            let private = fs::open(&root.join("private"), true)?;
            fs::child(&private, "cloud-saves", true)?;
            let snap = cs::download(
                &mut client,
                &root.join("private/cloud-saves"),
                &cs::Limits::default(),
                &ctx.cancel,
            )?;
            let mut result = json!({"container_count":snap.container_count,"blob_count":snap.blob_count,"total_bytes":snap.total_bytes,"downloaded":true,"rechecked":true,"snapshot_id":snap.path.file_name().and_then(|s|s.to_str()),"consistency":snap.consistency});
            if matches!(operation, "prepare-import" | "import") {
                let baseline = ci::load_baseline(root, &scope)?;
                let plan = ci::prepare(root, &scope, &snap, baseline.as_ref())?;
                if operation == "prepare-import" {
                    prepared = Some(plan);
                    result["prepared_for_import"] = json!(true);
                } else {
                    let selected = selected.ok_or(Error::Cloud(Failure::new("invalid_plan")))?;
                    cloud::require(selected.remote_digest == plan.remote_digest, "changed")?;
                    let imported = ci::apply(
                        root,
                        &scope,
                        selected,
                        &ci::Choice::Cloud,
                        &lease,
                        &ctx.cancel,
                    )?;
                    if let (Some(target), Some(extra)) =
                        (result.as_object_mut(), imported.summary().as_object())
                    {
                        target.extend(extra.clone());
                    }
                }
            }
            result
        }
    };
    if matches!(operation, "check" | "prepare-import") {
        ctx.interrupted()?;
    }
    let cleanup = client.close();
    let completed = result["uploaded"] == true
        || result["restored"] == true
        || result["imported"] == true
        || operation == "download" && result["downloaded"] == true;
    if cleanup.is_err() && !completed {
        cleanup.as_ref().map_err(Clone::clone)?;
    }
    let mut message = if result["restored"] == true {
        "Lokale Spielstände wiederhergestellt."
    } else if result["uploaded"] == true {
        "Lokale Spielstände hochgeladen und in der Xbox-Cloud geprüft."
    } else if result["imported"] == true {
        "Cloud-Spielstände übernommen. Der vorherige lokale Stand wurde gesichert."
    } else if operation == "import" {
        "Die lokalen Spielstände stimmen bereits mit der Cloud überein."
    } else if operation == "prepare-import" {
        "Spielstände verglichen. Wähle den Stand aus, den MSFS verwenden soll."
    } else if operation == "download" {
        "Cloud-Kopie heruntergeladen. Lokale Spielstände wurden nicht ersetzt."
    } else if result["container_count"] == 0 {
        "Für dieses Spielprofil sind keine Cloud-Spielstände vorhanden."
    } else {
        "Cloud-Spielstände wurden gefunden."
    };
    let mut warning = Value::Null;
    if result["imported"] == true && result["durability_confirmed"] == false {
        warning = json!("durability_unknown");
        message = "Die Cloud-Spielstände wurden übernommen. Die endgültige Speicherung konnte nicht bestätigt werden; die Sicherung bleibt erhalten.";
    }
    if result["uploaded"] == true
        && (result["lease_released"] != true || result["baseline_saved"] != true)
    {
        warning = json!("sync_cleanup");
        message = "Der Upload wurde durch erneutes Lesen bestätigt. Bitte vor weiteren Änderungen erneut vergleichen.";
    }
    if cleanup.is_err() {
        warning = json!("connection_cleanup");
        message = "Der Vorgang ist abgeschlossen. Die Verbindung konnte nicht vollständig geschlossen werden.";
        result["local_cleanup_failed"] = json!(true);
    }
    let mut s = ctx.launcher.lock();
    if let Some(plan) = prepared {
        s.cloud_data.plan = Some(Review::new(root.into(), plan));
    }
    if operation == "import" && result["imported"] == true {
        s.cloud_data.restore = result["backup_id"]
            .as_str()
            .map(|id| (root.into(), id.into()));
    } else if operation == "restore" {
        s.cloud_data.restore = None;
    }
    Ok(
        json!({"state":"succeeded","result":result,"message":message,"warning":warning,"finished_at":files::now()}),
    )
}
pub fn post(app: &Arc<Launcher>, action: &str, data: &Value) -> Result<Value> {
    match action {
        "retry" | "play-local" | "cancel-auto" | "resolve" => {
            crate::cloud_auto::action(app, action, data)
        }
        "discard-plan" => discard(app, string(data, "plan_id")?),
        "cancel" => app.cancel("cloud", string(data, "job_id")?),
        _ => start(app, action, data),
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn persisted_fence_is_recoverable_after_service_restart_but_not_during_owned_work() {
        let temp = tempfile::tempdir().expect("state");
        let root = temp.path().join("runtime");
        files::private_dir(&root.join("private")).expect("private");
        let lease = files::Lease::acquire(&root.join("private/play.lock"), true).expect("lease");
        crate::cloud_process_guard::mark(&root, &lease.0).expect("fence");
        // This fixture models a fully exited owner. A concurrent test's fork can
        // retain a CLOEXEC descriptor until exec, so closing alone is not enough.
        rustix::fs::flock(&lease.0, rustix::fs::FlockOperation::Unlock)
            .expect("release synthetic owner");
        drop(lease);
        let app = Launcher::new(temp.path().join("launcher"), None).expect("launcher");
        let mut state = app.lock();
        state.runtime = Some(root.clone());
        let status = automatic_enabled(&state, true);
        assert_eq!(status["state"], "attention");
        assert_eq!(status["error_code"], "unsafe_session");
        assert_eq!(status["can_retry"], true);
        assert_eq!(status["can_play_local"], false);
        assert!(
            status["request_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("interrupted-"))
        );
        let ctx = app
            .reserve_locked(&mut state, "cloud-auto", "recover", true)
            .expect("reserve");
        assert_ne!(automatic_enabled(&state, true)["state"], "attention");
        drop(state);
        app.finish(&ctx, Ok(json!({})), false);
        let mut state = app.lock();
        assert_eq!(automatic_enabled(&state, true)["can_retry"], true);
        state.runtime = None;
        assert_eq!(automatic_enabled(&state, false)["state"], "idle");
    }
}
