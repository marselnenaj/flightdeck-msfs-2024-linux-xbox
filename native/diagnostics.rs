// SPDX-License-Identifier: MIT
use crate::{
    backend::{Launcher, State},
    cloud_sync, fenix_diagnostics, files,
    games::Game,
    graphics, graphics_diagnostics,
    log_reader::regex,
    proton, run_diagnostics, runtime, store_check, store_diagnostics, vr,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

/// Capture service-owned state while holding the coordinator briefly. Log scans,
/// library hashing and child probes must run after releasing its lock.
pub(crate) struct Captured {
    root: Option<PathBuf>,
    store_check: Value,
    installation: Value,
    maintenance: Value,
    cloud: Value,
    vr: Value,
    graphics_report: Value,
}
pub(crate) fn capture(s: &State) -> Captured {
    Captured {
        root: s.runtime.clone(),
        store_check: store_check::report(s),
        installation: s.installation_report.clone(),
        maintenance: crate::maintenance::diagnostics(s),
        cloud: cloud_sync::automatic(s),
        vr: vr::launcher_snapshot(s),
        graphics_report: s.graphics_report.clone(),
    }
}
pub fn snapshot(app: &Arc<Launcher>) -> Value {
    snapshot_with(app, collect)
}
fn snapshot_with(app: &Launcher, gather: impl FnOnce(Captured) -> Value) -> Value {
    let captured = capture(&app.lock());
    gather(captured)
}
pub(crate) fn collect(captured: Captured) -> Value {
    let root = captured.root.as_deref();
    let mut summary = json!({"run_found":false,"auth_http":[],"local_save_init":[],"store_calls":[],"store_catalog":[],"exit":null,"context":{"diagnostics_schema":crate::run_diagnostics::SCHEMA,"launcher_version":crate::VERSION,"game_id":root.and_then(|r|Game::for_runtime(r).ok()).map(|g|g.id()),"cloud_sync_scope":"current_service","run_log_modified_at":null}});
    let mut graphics_log = Value::Null;
    if let Some(root) = root
        && let Ok(entries) = std::fs::read_dir(root.join("private"))
    {
        let run = entries
            .take(10000)
            .filter_map(|v| v.ok())
            .filter(|v| {
                regex(r"^run-\d{8}-\d{6}-[A-Za-z0-9]+$").is_match(&v.file_name().to_string_lossy())
                    && v.file_type().is_ok_and(|t| t.is_dir())
            })
            .max_by_key(|v| v.file_name());
        if let Some(run) = run {
            summary["store_session"] = store_diagnostics::session(&run.path());
            if let Ok((mut evidence, info)) = run_diagnostics::read(&run.path().join("game.log")) {
                graphics_log = evidence["graphics_log"].take();
                if let Some(map) = evidence.as_object_mut() {
                    map.remove("graphics_log");
                    if let Some(summary) = summary.as_object_mut() {
                        summary.extend(map.clone());
                    }
                }
                summary["run_found"] = json!(true);
                if let Ok(time) = info.modified() {
                    let time: chrono::DateTime<chrono::Utc> = time.into();
                    summary["context"]["run_log_modified_at"] = json!(time.to_rfc3339());
                }
            }
        }
    }
    summary["store_check"] = captured.store_check;
    summary["installation"] = captured.installation;
    summary["maintenance"] = captured.maintenance;
    summary["fenix"] = root.map(fenix_diagnostics::load).unwrap_or(Value::Null);
    let cloud = captured.cloud;
    summary["cloud_sync"] = ["state", "phase", "error_code", "error_details"]
        .into_iter()
        .map(|k| (k.into(), cloud[k].clone()))
        .collect::<serde_json::Map<_, _>>()
        .into();
    summary["graphics"] = graphics::probe(false);
    summary["display"] = root
        .map(crate::display_preflight::snapshot)
        .unwrap_or(Value::Null);
    summary["proton"] = proton::diagnostic(root);
    let vr = captured.vr;
    summary["vr"] = json!({"mode":vr["mode"],"state":vr["state"]});
    if vr["check"].is_object() {
        summary["vr"]["check"] =
            json!({"state":vr["check"]["state"],"checked_at":vr["check"]["checked_at"]});
    }
    summary["graphics"]["log"] = graphics_log;
    if let Some(root) = root {
        summary["graphics"]["prefix"] = graphics_diagnostics::prefix_summary(root);
        summary["graphics"]["last_start_attempt"] = graphics_diagnostics::load_launch(root);
    }
    if !captured.graphics_report.is_null() {
        summary["graphics"]["last_launch"] = captured.graphics_report;
    }
    json!({"generated_at":files::now(),"summary":summary,"checks":runtime::checks(root)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expensive_collection_releases_the_coordinator_and_retains_captured_state() {
        let temp = tempfile::tempdir().expect("state");
        let app = Launcher::new(temp.path().join("state"), None).expect("launcher");
        app.lock().installation_report = json!({"state":"captured"});
        let result = snapshot_with(&app, |captured| {
            // Collection may be slow. Actions and status reads must still be
            // able to acquire the coordinator while its probes run.
            let mut state = app
                .state
                .try_lock()
                .expect("diagnostics must not retain lock");
            state.installation_report = json!({"state":"newer"});
            captured.installation
        });
        assert_eq!(result["state"], "captured");
        assert_eq!(app.lock().installation_report["state"], "newer");
    }
}
