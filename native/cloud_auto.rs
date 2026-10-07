// SPDX-License-Identifier: MIT
//! One reservation spans comparison, game lifetime, backup and final upload.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    cloud::{self, Failure},
    cloud_flow::{self as flow, Attention},
    cloud_import::Choice,
    cloud_process_guard as guard,
    cloud_runtime::Client,
    cloud_storage::Transport,
    cloud_sync::{self, Review},
    files, runtime,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Instant};
const BEFORE: &str =
    "Cloud-Spielstände werden geladen. Der Simulator startet anschließend automatisch.";
const AFTER: &str = "Deine Spielstände werden in der Xbox-Cloud gespeichert. Eine lokale Sicherung bleibt erhalten.";
const LOCAL: &str = "Diese Sitzung verwendet lokale Spielstände. Der Cloud-Abgleich wird beim nächsten Start erneut versucht.";
fn set(ctx: &Context, state: &str, message: &str, patch: Value) {
    let mut s = ctx.launcher.lock();
    if s.active.as_ref().is_some_and(|a| a.id == ctx.id) {
        s.cloud["state"] = json!(state);
        s.cloud["message"] = json!(message);
        if let (Some(target), Some(extra)) = (s.cloud.as_object_mut(), patch.as_object()) {
            target.extend(extra.clone());
        }
    }
}
fn elapsed(ctx: &Context, key: &str, start: Instant) {
    let mut s = ctx.launcher.lock();
    if s.cloud["request_id"] == ctx.id {
        s.cloud["timings"][key] = json!((start.elapsed().as_secs_f64() * 1000.0).round() / 1000.0);
    }
}
pub fn launch(app: &Arc<Launcher>) -> Result<Value> {
    start(app, None)
}
pub fn action(app: &Arc<Launcher>, action: &str, data: &Value) -> Result<Value> {
    if action == "retry" && cloud_sync::automatic(&app.lock())["error_code"] == "unsafe_session" {
        return recover(app, data);
    }
    if action == "cancel-auto" {
        let id = string(data, "request_id")?;
        let s = app.lock();
        let status = cloud_sync::automatic(&s);
        crate::error::require(
            status["request_id"] == id && status["can_cancel"] == true,
            "Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.",
        )?;
        let active = s.active.as_ref().ok_or(Error::Invalid(
            "Dieser Cloud-Vorgang ist nicht mehr aktuell.",
        ))?;
        active
            .cancel
            .store(true, std::sync::atomic::Ordering::Release);
        return Ok(json!({"ok":true}));
    }
    crate::error::require(
        matches!(action, "retry" | "play-local" | "resolve"),
        "Diese Cloud-Aktion ist nicht verfügbar.",
    )?;
    start(app, Some((action, data)))
}
fn recover(app: &Arc<Launcher>, data: &Value) -> Result<Value> {
    let mut state = app.lock();
    Launcher::idle(&mut state)?;
    let status = cloud_sync::automatic(&state);
    crate::error::require(
        status["state"] == "attention"
            && status["error_code"] == "unsafe_session"
            && status["can_retry"] == true
            && data["request_id"]
                .as_str()
                .is_some_and(|id| status["request_id"] == id),
        "Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.",
    )?;
    let ctx = app.reserve_locked(&mut state, "cloud-auto", "recover", true)?;
    state.cloud_data.auto_runtime = state.runtime.clone();
    state.cloud_data.review = None;
    state.cloud_data.binding = None;
    state.cloud = status;
    state.cloud["state"] = json!("syncing");
    state.cloud["phase"] = json!("recovery");
    state.cloud["request_id"] = json!(ctx.id);
    state.cloud["message"] =
        json!("Vorherige Spielsitzung wird geprüft und lokale Spielstände werden gesichert …");
    drop(state);
    let id = ctx.id.clone();
    std::thread::spawn(move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> cloud::Result<Value> {
                guard::recover(ctx.root()?, &ctx.lease()?)
            }))
            .unwrap_or_else(|_| Err(Failure::new("unsafe_session")));
        match result {
            Ok(recovery) => set(
                &ctx,
                "idle",
                "Die vorherige Sitzung ist beendet und lokale Spielstände sind gesichert. Der Simulator kann wieder gestartet werden.",
                json!({"phase":null,"request_id":null,"error_code":null,"error_details":{},"conflict":false,"summary":null,"recovery":recovery}),
            ),
            Err(error) => attention(&ctx, "before_start", Attention { error, plan: None }),
        }
        ctx.launcher
            .finish(&ctx, Ok(json!({"state":"complete"})), false);
    });
    Ok(json!({"ok":true,"recovery_only":true,"request_id":id}))
}
fn start(app: &Arc<Launcher>, action: Option<(&str, &Value)>) -> Result<Value> {
    // Validation and reservation form one critical section; another request
    // must not replace a review or runtime between those steps.
    let mut s = app.lock();
    let (root, mut phase, local_only, review, choice, expected) = {
        Launcher::idle(&mut s)?;
        let root = s
            .runtime
            .clone()
            .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?;
        crate::error::require(
            runtime::ready(&root),
            "Die Runtime ist nicht startbereit oder das Spiel läuft bereits.",
        )?;
        if let Some((action, data)) = action {
            let status = cloud_sync::automatic(&s);
            crate::error::require(
                data["request_id"]
                    .as_str()
                    .is_some_and(|id| status["request_id"] == id)
                    && status["can_retry"] == true,
                "Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.",
            )?;
            let phase = status["phase"]
                .as_str()
                .unwrap_or("before_start")
                .to_owned();
            let (review, choice) = if action == "resolve" {
                let choice = match data["choice"].as_str() {
                    Some("cloud") => Choice::Cloud,
                    Some("local") => Choice::Local,
                    _ => {
                        return Err(Error::Invalid(
                            "Der Spielstandvergleich ist abgelaufen. Bitte erneut versuchen.",
                        ));
                    }
                };
                let review = s
                    .cloud_data
                    .review
                    .as_ref()
                    .filter(|r| r.current(Some(&root)))
                    .ok_or(Error::Invalid(
                        "Der Spielstandvergleich ist abgelaufen. Bitte erneut versuchen.",
                    ))?;
                (Some(review.clone()), choice)
            } else {
                (None, Choice::Automatic)
            };
            if action == "play-local" {
                crate::error::require(
                    status["can_play_local"] == true,
                    "Diese lokale Sitzung kann nicht gestartet werden.",
                )?;
            }
            let expected = (phase == "after_exit")
                .then(|| s.cloud_data.binding.clone())
                .flatten();
            (
                root,
                phase,
                action == "play-local",
                review,
                choice,
                expected,
            )
        } else {
            (
                root,
                "before_start".into(),
                !crate::cloud_runtime::available(
                    s.runtime
                        .as_deref()
                        .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?,
                    true,
                ),
                None,
                Choice::Automatic,
                None,
            )
        }
    };
    if local_only {
        phase = "before_start".into();
    }
    let ctx = app.reserve_locked(&mut s, "cloud-auto", "session", true)?;
    drop(s);
    {
        let mut s = app.lock();
        s.cloud_data.auto_runtime = Some(root.clone());
        s.cloud_data.review = None;
        if phase == "before_start" {
            s.cloud_data.binding = None;
        }
        let previous = s.cloud["last_synced_at"].clone();
        s.cloud = json!({"state":"syncing","phase":phase,"message":if phase=="before_start"{BEFORE}else{AFTER},"error_code":null,"error_details":{},"request_id":ctx.id,"last_synced_at":previous,"timings":{}});
    }
    let id = ctx.id.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(
                &ctx,
                &phase,
                local_only,
                review.as_ref(),
                &choice,
                expected.as_deref(),
            )
        }));
        if result.is_err() {
            attention(
                &ctx,
                &phase,
                Attention {
                    error: Failure::new("failed"),
                    plan: None,
                },
            );
        }
        ctx.launcher
            .finish(&ctx, Ok(json!({"state":"succeeded"})), false);
    });
    Ok(json!({"ok":true,"cloud_sync":!local_only,"request_id":id}))
}
fn attention(ctx: &Context, phase: &str, error: Attention) {
    let code = error.error.code;
    let message = if code == "unsafe_session" {
        "Die vorherige Spielsitzung ist noch nicht freigegeben. Erneut versuchen prüft, ob alle zugehörigen Prozesse beendet sind, und sichert die lokalen Spielstände."
    } else if error.plan.is_some() {
        "Auf diesem Rechner und in der Cloud gibt es unterschiedliche Änderungen. Welchen Stand möchtest du verwenden?"
    } else if matches!(code, "authentication" | "auth_required" | "unauthorized") {
        "Die Xbox-Anmeldung muss erneuert werden. Melde dich mit demselben Microsoft-Konto an; danach wird der Cloud-Abgleich erneut versucht."
    } else if matches!(code, "transport" | "deadline") {
        "Die Xbox-Cloud ist gerade nicht erreichbar. Versuche es erneut oder spiele mit lokalen Spielständen weiter."
    } else if code == "lease_lost" {
        "Die Cloud-Sperre ist nicht verfügbar. Beende eine laufende Sitzung auf anderen Geräten und versuche es erneut oder spiele lokal weiter."
    } else if code == "quota" {
        "Der Cloud-Speicher reicht für diese Spielstände nicht aus. Deine lokalen Spielstände bleiben erhalten."
    } else if code == "cancelled" && phase == "before_start" {
        "Der Start wurde abgebrochen. Gesicherte Spielstände bleiben erhalten."
    } else if phase == "before_start" {
        "Der Cloud-Abgleich konnte nicht abgeschlossen werden. Versuche es erneut oder spiele mit dem gesicherten lokalen Stand."
    } else {
        "Deine Spielstände sind lokal gesichert. Versuche den Cloud-Upload erneut oder spiele mit lokalen Spielständen weiter. Der ausstehende Abgleich bleibt erhalten."
    };
    let mut details = json!({});
    if let Some(status) = error.error.http_status {
        details["http_status"] = json!(status);
    }
    if let Some(hr) = error.error.native_hresult {
        details["native_hresult"] = json!(hr);
    }
    set(
        ctx,
        if code == "cancelled" && phase == "before_start" {
            "idle"
        } else {
            "attention"
        },
        message,
        json!({"phase":phase,"error_code":code,"error_details":details}),
    );
    if let Some(plan) = error.plan
        && let Some(root) = &ctx.runtime
    {
        ctx.launcher.lock().cloud_data.review = Some(Review::new(root.clone(), *plan));
    }
}
fn connect(ctx: &Context, lease: &std::fs::File, phase: &str) -> cloud::Result<Client> {
    let start = Instant::now();
    let client = Client::open(ctx.root()?, lease, Arc::clone(&ctx.cancel));
    elapsed(ctx, &format!("{phase}_connect_seconds"), start);
    client
}
fn cleanup(ctx: &Context, client: &mut Client, phase: &str) -> cloud::Result<()> {
    let start = Instant::now();
    let result = client.close();
    elapsed(ctx, &format!("{phase}_cleanup_seconds"), start);
    result
}
fn run(
    ctx: &Context,
    initial_phase: &str,
    local_only: bool,
    review: Option<&Review>,
    choice: &Choice,
    expected: Option<&str>,
) {
    let mut phase = initial_phase.to_owned();
    let mut start = Instant::now();
    let mut transferred = false;
    let result = (|| -> flow::Outcome<()> {
        let root = ctx.root()?;
        let lease = ctx.lease()?;
        guard::check(root, &lease)?;
        flow::enable_local(root, &lease)?;
        let mut binding = expected.map(str::to_owned);
        if phase == "before_start" {
            let backup_start = Instant::now();
            runtime::backup(root, true)?;
            elapsed(ctx, "before_backup_seconds", backup_start);
            if !local_only {
                let mut client = connect(ctx, &lease, "before")?;
                let compare_start = Instant::now();
                let request = flow::Request {
                    runtime: root,
                    lease: &lease,
                    cancel: &ctx.cancel,
                    review: review.map(|r| (&r.plan, choice)),
                    expected_binding: None,
                };
                let result = flow::before(&request, &mut client);
                elapsed(ctx, "before_compare_seconds", compare_start);
                let closed = cleanup(ctx, &mut client, "before");
                result?;
                closed?;
                binding = Some(client.scope().binding());
                ctx.launcher.lock().cloud_data.binding = binding.clone();
            }
            ctx.interrupted()?;
            flow::offline(root, &lease, true)?;
            set(
                ctx,
                "syncing",
                "Windows-Umgebung wird vor dem Spielstart geprüft …",
                json!({}),
            );
            guard::mark(root, &lease)?;
            if let Err(error) = crate::supervisor::spawn_reserved(ctx) {
                if ctx.launcher.lock().process.is_none() {
                    guard::clear(root, &lease)?;
                }
                let message = error.to_string();
                let code = match error {
                    Error::Graphics(_) => "graphics",
                    Error::Framework(_) => "framework",
                    Error::Cancelled => "cancelled",
                    _ => "launch",
                };
                set(
                    ctx,
                    "attention",
                    &message,
                    json!({"error_code":code,"phase":"before_start"}),
                );
                return Err(Attention {
                    error: Failure::new(code),
                    plan: None,
                });
            }
            set(
                ctx,
                "playing",
                if local_only {
                    LOCAL
                } else {
                    "Cloud-Spielstände geladen. Änderungen werden nach dem Beenden synchronisiert."
                },
                json!({}),
            );
            elapsed(ctx, "before_total_seconds", start);
            // Keep the owned Child in State so the Stop button and status poll
            // refer to the exact same supervisor, while retaining the lease.
            let code = loop {
                let mut s = ctx.launcher.lock();
                cloud::require(s.active.as_ref().is_some_and(|a| a.id == ctx.id), "changed")?;
                let child = s.process.as_mut().ok_or_else(|| Failure::new("changed"))?;
                if let Some(code) = child
                    .try_wait()
                    .map_err(|_| Failure::new("unsafe_session"))?
                {
                    s.exit_code = code.code().or(Some(-1));
                    s.process = None;
                    s.stopping = false;
                    break code;
                }
                drop(s);
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            start = Instant::now();
            phase = "after_exit".into();
            cloud::require(code.code().is_some(), "unsafe_session")?;
            guard::clear(root, &lease)?;
            set(ctx, "syncing", AFTER, json!({"phase":phase}));
            let backup_start = Instant::now();
            runtime::backup(root, true)?;
            elapsed(ctx, "after_backup_seconds", backup_start);
            if local_only {
                set(ctx, "local", LOCAL, json!({"phase":null}));
                return Ok(());
            }
        }
        ctx.interrupted()?;
        let mut client = connect(ctx, &lease, "after")?;
        let transfer_start = Instant::now();
        let request = flow::Request {
            runtime: root,
            lease: &lease,
            cancel: &ctx.cancel,
            review: if initial_phase == "after_exit" {
                review.map(|r| (&r.plan, choice))
            } else {
                None
            },
            expected_binding: binding.as_deref(),
        };
        let result = flow::after(&request, &mut client);
        elapsed(ctx, "after_transfer_seconds", transfer_start);
        let closed = cleanup(ctx, &mut client, "after");
        result?;
        transferred = true;
        closed?;
        set(
            ctx,
            "synced",
            "Deine Spielstände sind mit der Xbox-Cloud synchronisiert.",
            json!({"phase":null,"last_synced_at":files::now()}),
        );
        Ok(())
    })();
    if let Err(error) = result {
        if transferred {
            set(
                ctx,
                "synced",
                "Die Spielstände wurden synchronisiert. Die Verbindung konnte nicht vollständig geschlossen werden.",
                json!({"phase":null,"last_synced_at":files::now()}),
            );
        } else if !matches!(error.error.code, "graphics" | "framework" | "launch") {
            attention(ctx, &phase, error);
        }
    }
    elapsed(
        ctx,
        if phase == "before_start" {
            "before_total_seconds"
        } else {
            "after_total_seconds"
        },
        start,
    );
}
