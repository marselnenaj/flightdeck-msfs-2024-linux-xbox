// SPDX-License-Identifier: MIT
//! Storefront dispatch: Steam installations never enter Xbox setup or cloud code.
use crate::{
    Error, Result,
    backend::{Launcher, string},
    installations::Source,
    steam_fenix, steam_session,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub fn selected(app: &Launcher) -> bool {
    app.lock().steam.is_some()
}
pub fn status(app: &Launcher) -> Value {
    let mut s = app.lock();
    let Some(item) = s.steam.clone() else {
        return json!({});
    };
    let game = steam_session::status(&mut s, &app.state_dir);
    let available = item.validate().is_ok();
    let checks = json!([{"id":"steam","label":"Steam-Installation","ok":available,"detail":if available{"Vorhanden"}else{"Bibliothek getrennt oder Installation verschoben. Erneut suchen."}}]);
    json!({"app":{"name":"Flightdeck","version":crate::VERSION},"runtime":{"configured":true,"path":item.context(),"ready":available,"checks":checks,"game_id":item.game.id(),"game_name":item.game.name(),"source":"steam","installation_id":item.id,"installation_name":item.label(),"game_path":item.game_path(),"prefix":item.prefix(),"needs_first_launch":!item.prefix().join("system.reg").is_file(),"steam_client":match item.source{Source::Steam{client,..}=>Some(client),_=>None}},"versions":{},"capabilities":{"launch":true,"steam":true,"fenix":true,"xbox_setup":false,"xbox_cloud":false,"game_updates":"steam","graphics":"steam","vr":"steam"},"graphics":{"available":false},"vr":{"available":false,"mode":"off"},"game":game,"saves":{"available":false,"can_backup":false,"managed_by":"steam"},"setup":{"busy":s.active.is_some()},"cloud":{"enabled":false,"state":"idle","message":"Synchronisierung wird vom Spiel und Steam verwaltet."},"support":{"level":"beta","cloud_saves":false,"cloud_sync":"platform","automatic_cloud_sync":false}})
}
pub fn get(app: &Arc<Launcher>, path: &str) -> Option<Result<Value>> {
    let context = {
        let s = app.lock();
        s.steam.as_ref()?.context()
    };
    Some(Ok(match path {
        "/api/status" => status(app),
        "/api/fenix" => steam_fenix::snapshot(app),
        "/api/mods" => crate::mods::steam_snapshot(app),
        "/api/setup" => json!({"job":null,"managed_by":"steam"}),
        "/api/game-update" => json!({"runtime_path":context,"managed_by":"steam","job":null}),
        "/api/cloud-saves" => {
            json!({"available":false,"enabled":false,"managed_by":"steam","runtime_path":context})
        }
        "/api/gsx" | "/api/proton" | "/api/maintenance" | "/api/store-check" => {
            json!({"state":"unavailable","runtime_path":context,"available":false,"managed_by":"steam","job":null})
        }
        "/api/diagnostics" => {
            let s = app.lock();
            let item = s.steam.as_ref()?;
            json!({"source":"steam","installation":item.public(true),"summary":{"context":{"source":"steam","game_id":item.game.id(),"launcher_version":crate::VERSION},"fenix":steam_fenix::latest_attempt(&app.state_dir,item)},"beta":true})
        }
        "/api/problem-reports" => json!({"available":false,"source":"steam"}),
        _ => return None,
    }))
}
pub fn post(app: &Arc<Launcher>, path: &str, data: &Value) -> Option<Result<Value>> {
    if !selected(app) {
        return None;
    }
    Some(match path {
        "/api/launch" => steam_session::launch(app),
        "/api/stop" => steam_session::stop(app),
        "/api/steam/open" => steam_session::open(app),
        "/api/mods/open-folder" => crate::mods::steam_open_folder(app, data),
        "/api/updates/check-startup" => {
            let a = crate::launcher_update::check_on_startup(app);
            if a.is_ok() {
                let _ = crate::fenix_update::check(app, true);
            }
            a
        }
        "/api/fenix/pick" => crate::fenix::pick(app, data),
        "/api/fenix/check-update" => crate::fenix_update::check(app, true),
        p if p.starts_with("/api/fenix/") => {
            steam_fenix::start(app, p.rsplit('/').next().unwrap_or(""), data)
        }
        "/api/game/select" => string(data, "game_id").and_then(|id| app.select_game(id)),
        p if p.starts_with("/api/launcher-update/")
            || [
                "/api/preferences",
                "/api/installations/select",
                "/api/installations/add-steam-root",
                "/api/config",
                "/api/game/register",
            ]
            .contains(&p) =>
        {
            return None;
        }
        _ => Err(Error::Invalid(
            "Diese Funktion gehört zur Xbox-Version. Für Steam Installation, Spielupdates und Startoptionen in Steam verwalten.",
        )),
    })
}
