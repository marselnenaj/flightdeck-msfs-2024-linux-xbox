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
fn retry_due(record: &Record, now: Instant) -> bool {
    let delay = Duration::from_secs(if record.value["state"] == "failed" {
        300
    } else {
        1800
    });
    now.saturating_duration_since(record.attempt) >= delay
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
        .is_some_and(|record| !retry_due(record, Instant::now()))
        || s.jobs.get("setup").is_some_and(|job| {
            job["mode"] == "update"
                && job["runtime_path"].as_str() == root.to_str()
                && !game_update::terminal_job(job)
        })
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
    let started_at = files::now();
    s.startup_updates.worker = Some(Arc::clone(&cancel));
    s.startup_updates.records.insert(
        root.clone(),
        Record {
            attempt: Instant::now(),
            value: json!({"state":"checking","started_at":started_at}),
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
        let mut value = match result {
            Ok(value) => value,
            Err(Error::AuthRequired(message)) => {
                json!({"state":"failed","auth_required":true,"error":message,"checked_at":files::now()})
            }
            Err(_) => {
                json!({"state":"failed","error":"Die Updateprüfung konnte nicht abgeschlossen werden. Verbindung prüfen und erneut versuchen.","checked_at":files::now()})
            }
        };
        value["started_at"] = json!(started_at);
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

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn failed_game_discovery_retries_sooner_without_expiring_a_successful_check() {
        let attempt = Instant::now();
        let mut record = Record {
            attempt,
            value: json!({"state":"failed"}),
        };
        assert!(!retry_due(&record, attempt + Duration::from_secs(299)));
        assert!(retry_due(&record, attempt + Duration::from_secs(300)));
        for state in ["complete", "checking"] {
            record.value = json!({"state":state});
            assert!(!retry_due(&record, attempt + Duration::from_secs(1799)));
            assert!(retry_due(&record, attempt + Duration::from_secs(1800)));
        }
    }
}
