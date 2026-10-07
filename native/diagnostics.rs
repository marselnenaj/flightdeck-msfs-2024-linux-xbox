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
use std::sync::Arc;
pub fn snapshot(app: &Arc<Launcher>) -> Value {
    from_state(&app.lock())
}
pub fn from_state(s: &State) -> Value {
    let root = s.runtime.as_deref();
    let mut summary = json!({"run_found":false,"auth_http":[],"local_save_init":[],"store_calls":[],"store_catalog":[],"exit":null,"context":{"diagnostics_schema":6,"launcher_version":crate::VERSION,"game_id":root.and_then(|r|Game::for_runtime(r).ok()).map(|g|g.id()),"cloud_sync_scope":"current_service","run_log_modified_at":null}});
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
    summary["store_check"] = store_check::report(s);
    summary["fenix"] = root.map(fenix_diagnostics::load).unwrap_or(Value::Null);
    let cloud = cloud_sync::automatic(s);
    summary["cloud_sync"] = ["state", "phase", "error_code", "error_details"]
        .into_iter()
        .map(|k| (k.into(), cloud[k].clone()))
        .collect::<serde_json::Map<_, _>>()
        .into();
    summary["graphics"] = graphics::probe(false);
    summary["proton"] = proton::diagnostic(root);
    let vr = vr::launcher_snapshot(s);
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
    if !s.graphics_report.is_null() {
        summary["graphics"]["last_launch"] = s.graphics_report.clone();
    }
    json!({"generated_at":files::now(),"summary":summary,"checks":runtime::checks(root)})
}
