// SPDX-License-Identifier: MIT
use crate::{
    Result,
    backend::{Launcher, string},
    graphics, vr,
};
use serde_json::{Value, json};
use std::sync::Arc;
pub fn get(app: &Arc<Launcher>, path: &str) -> Option<Result<Value>> {
    Some(match path {
        "/api/status" => Ok(app.status()),
        "/api/diagnostics" => Ok(crate::diagnostics::snapshot(app)),
        "/api/maintenance" => Ok(crate::maintenance::snapshot(app)),
        "/api/problem-reports" => Ok(crate::problem_reports::snapshot(app)),
        "/api/cloud-saves" => Ok(crate::cloud_sync::snapshot(app)),
        "/api/store-check" => Ok(crate::store_check::snapshot(app)),
        "/api/fenix" => Ok(crate::fenix::snapshot(app)),
        "/api/gsx" => Ok(crate::gsx::snapshot(app)),
        "/api/proton" => Ok(crate::proton::snapshot(app)),
        "/api/proton/discover" => {
            Ok(json!({"ok":true,"choices":crate::proton::discover(&crate::files::home())}))
        }
        "/api/mods" => Ok(crate::mods::snapshot(app)),
        "/api/game-update" => Ok(crate::game_update::snapshot(app)),
        "/api/launcher-update" => Ok(crate::launcher_update::snapshot(app)),
        "/api/setup" => Ok(crate::setup::snapshot(app)),
        "/api/setup/discover" => Ok(crate::setup::discover(app)),
        _ => return None,
    })
}
pub fn post(app: &Arc<Launcher>, path: &str, data: &Value, locale: &str) -> Option<Result<Value>> {
    Some(match path {
        "/api/preferences" => {
            let language = string(data, "language");
            language.and_then(|language| {
                crate::error::require(["de", "en"].contains(&language), "Ungültige Sprache.")?;
                crate::files::atomic_json(
                    &app.state_dir.join("ui-preferences.json"),
                    &json!({"language":language}),
                )?;
                Ok(json!({"ok":true}))
            })
        }
        "/api/launch" => crate::cloud_auto::launch(app),
        "/api/updates/check-startup" => crate::startup_updates::check(app),
        "/api/launcher-update/check" => crate::launcher_update::start(app, "check", None),
        "/api/launcher-update/install" => string(data, "check_id")
            .and_then(|id| crate::launcher_update::start(app, "install", Some(id))),
        "/api/launcher-update/rollback" => crate::launcher_update::start(app, "rollback", None),
        "/api/launcher-update/cancel" => {
            string(data, "job_id").and_then(|id| crate::launcher_update::cancel(app, id))
        }
        "/api/maintenance/preview" => crate::maintenance::preview(app, data),
        "/api/maintenance/start" => crate::maintenance::start(app, data),
        "/api/maintenance/discard" => crate::maintenance::discard(app, data),
        "/api/problem-reports/prepare" => crate::problem_reports::prepare(app, data),
        "/api/problem-reports/discard" => {
            string(data, "report_id").and_then(|id| crate::problem_reports::discard(app, id))
        }
        "/api/store-check/start" => crate::store_check::start(app, locale, false, data, false),
        "/api/store-check/sign-in" => crate::store_check::start(app, locale, true, data, false),
        "/api/cloud-saves/sign-in" => crate::store_check::start(app, locale, true, data, true),
        "/api/store-check/cancel" => string(data, "job_id")
            .and_then(|id| app.cancel("store-check", id))
            .map(|_| json!({"ok":true,"job":app.job("store-check")})),
        path if path.starts_with("/api/cloud-saves/") => {
            crate::cloud_sync::post(app, path.rsplit('/').next().unwrap_or(""), data)
        }
        "/api/fenix/pick" => crate::fenix::pick(app, data),
        path if path.starts_with("/api/gsx/") => {
            crate::gsx::start(app, path.rsplit('/').next().unwrap_or(""), data)
        }
        path if path.starts_with("/api/fenix/") => {
            crate::fenix::start(app, path.rsplit('/').next().unwrap_or(""), data)
        }
        "/api/proton/select" => crate::proton::start(app, data),
        "/api/proton/cancel" => string(data, "job_id").and_then(|id| app.cancel("proton", id)),
        "/api/mods/open-folder" => crate::mods::open_folder(app, data),
        "/api/game-update/check" | "/api/game-update/repair/check" | "/api/game-update/verify" => {
            crate::setup::check(
                app,
                &json!({"mode":"update","operation":if path.ends_with("/verify"){"verify"}else if path.contains("/repair/"){"repair"}else{"update"},"sign_in":data.get("sign_in").cloned().unwrap_or(json!(false))}),
            )
        }
        "/api/game-update/start" => {
            let job = app.job("setup");
            crate::error::require(
                job["mode"] == "update",
                "Bitte Updates zuerst erneut prüfen.",
            )
            .and_then(|()| string(data, "check_id").and_then(|id| crate::setup::start(app, id)))
        }
        "/api/game-update/rollback" => crate::game_update::rollback(app),
        "/api/setup/check" => crate::setup::check(app, data),
        "/api/setup/start" => string(data, "check_id").and_then(|id| crate::setup::start(app, id)),
        "/api/setup/cancel" => string(data, "job_id").and_then(|id| app.cancel("setup", id)),
        "/api/setup/pause" | "/api/setup/resume" => string(data, "job_id").and_then(|id| {
            crate::setup::download_action(app, id, path.rsplit('/').next().unwrap_or(""))
        }),
        "/api/setup/pick" => crate::setup::pick(app, data, locale),
        "/api/config" => string(data, "runtime_path").and_then(|p| app.configure(p)),
        "/api/game/register" => string(data, "runtime_path").and_then(|p| app.register(p)),
        "/api/game/select" => string(data, "game_id").and_then(|p| app.select_game(p)),
        "/api/saves/backup" => app.save_backup(),
        "/api/stop" => app.stop(),
        "/api/graphics" => app.configure_setting(
            data,
            "graphics-settings.json",
            "nvidia_mode",
            &graphics::MODES,
        ),
        "/api/vr/configure" => app.configure_setting(data, "vr-settings.json", "mode", &vr::MODES),
        "/api/vr/check" => {
            let requested = string(data, "runtime_path");
            requested.and_then(|requested| {
                let root = app.root()?;
                crate::error::require(
                    root == std::path::Path::new(requested),
                    "Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.",
                )?;
                let mode = vr::settings(Some(&root))?;
                crate::error::require(
                    mode != "off",
                    "Aktiviere zuerst VR und speichere den Modus.",
                )?;
                app.start_job("vr", "check", true, move |ctx| {
                    ctx.update(json!({"mode":mode}));
                    crate::error::require(ctx.root()?==root,"Die ausgewählte Installation hat sich geändert. Bitte den Status neu laden.")?;
                    let env = std::env::vars().collect();
                    let selected = vr::choose(&mode, &env);
                    let result = if !vr::bridge(ctx.runtime.as_deref()) {
                        json!({"state":"bridge_missing"})
                    } else if let Some(selected) = selected {
                        if selected["valid"] == true {
                            vr::probe(&selected, env, &ctx.cancel)
                        } else {
                            json!({"state":"invalid"})
                        }
                    } else {
                        json!({"state":"missing"})
                    };
                    let result = vr::public(&result);
                    Ok(json!({"state":"complete","check":result,"mode":mode}))
                })
            })
        }
        _ => return None,
    })
}
