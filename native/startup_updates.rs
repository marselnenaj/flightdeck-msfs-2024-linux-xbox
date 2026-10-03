// SPDX-License-Identifier: MIT
//! Throttled version discovery, without reserving the game or creating a plan.
use crate::{
    Error, Result,
    backend::{Launcher, string},
    files, game_package, game_update,
    games::Game,
    integrity,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub struct Record {
    pub attempt: Instant,
    pub value: Value,
}
#[derive(Default)]
pub struct State {
    pub records: BTreeMap<PathBuf, Record>,
    pub worker: Option<Arc<AtomicBool>>,
}
pub fn discover(root: &Path, cancel: &AtomicBool) -> Result<Value> {
    let game = Game::for_runtime(root)?;
    let current = integrity::installed_identity(&game.path(root), game)?;
    let (cli, checksum, _) = game_update::tools(root, true)?;
    let market = game_update::configured_market(root)?;
    let latest = game_update::package_info(&cli, &checksum, root, game, &market, cancel)?;
    let old = game_package::version(&current.version)?;
    let new = game_package::version(string(&latest, "version")?)?;
    crate::error::require(
        new >= old,
        "Der Store liefert eine ältere Spielversion. Es wird kein Downgrade durchgeführt.",
    )?;
    Ok(
        json!({"state":"complete","installed_version":current.version,"latest_version":latest["version"],"update_available":new>old,"checked_at":files::now()}),
    )
}
pub fn check(app: &Arc<Launcher>) -> Result<Value> {
    crate::launcher_update::check_on_startup(app)?;
    let mut s = app.lock();
    Launcher::open(&s)?;
    let Some(root) = s.runtime.clone() else {
        return Ok(json!({"ok":true}));
    };
    if Launcher::idle(&mut s).is_err() || s.startup_updates.worker.is_some() {
        return Ok(json!({"ok":true,"deferred":true}));
    }
    if s.startup_updates
        .records
        .get(&root)
        .is_some_and(|v| v.attempt.elapsed() < Duration::from_secs(1800))
        || s.jobs
            .get("setup")
            .is_some_and(|v| v["mode"] == "update" && v["runtime_path"].as_str() == root.to_str())
    {
        return Ok(json!({"ok":true}));
    }
    if s.startup_updates.records.len() >= 128
        && let Some(old) = s
            .startup_updates
            .records
            .iter()
            .min_by_key(|(_, record)| record.attempt)
            .map(|(root, _)| root.clone())
    {
        s.startup_updates.records.remove(&old);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    s.startup_updates.worker = Some(Arc::clone(&cancel));
    s.startup_updates.records.insert(
        root.clone(),
        Record {
            attempt: Instant::now(),
            value: json!({"state":"checking"}),
        },
    );
    drop(s);
    let app = Arc::clone(app);
    std::thread::spawn(move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| discover(&root, &cancel)))
                .unwrap_or(Err(Error::Invalid(
                    "Die Updateprüfung konnte nicht abgeschlossen werden.",
                )));
        let value = match result {
            Ok(value) => value,
            Err(Error::AuthRequired(message)) => {
                json!({"state":"failed","auth_required":true,"error":message})
            }
            Err(_) => {
                json!({"state":"failed","error":"Die Updateprüfung konnte nicht abgeschlossen werden. Verbindung prüfen und erneut versuchen."})
            }
        };
        let mut s = app.lock();
        if !cancel.load(Ordering::Relaxed)
            && !s.closing
            && let Some(record) = s.startup_updates.records.get_mut(&root)
        {
            record.value = value;
        }
        s.startup_updates.worker = None;
    });
    Ok(json!({"ok":true}))
}
