// SPDX-License-Identifier: MIT
//! Explicit, cancellable Store diagnostics and same-account sign-in recovery.
use crate::{
    Error, Result,
    backend::{Context, Launcher},
    cloud, files, game_install, process, resources, store_diagnostics, xml,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
const STAGES: [&str; 6] = [
    "runtime", "account", "catalog", "license", "library", "window",
];
const ONLINE: [&str; 4] = ["account", "catalog", "license", "library"];
const CODES: [&str; 19] = [
    "checking",
    "available",
    "verified",
    "local_session",
    "visible",
    "sign_in_required",
    "expired",
    "not_licensed",
    "unsupported",
    "invalid_config",
    "connection",
    "timeout",
    "account_changed",
    "keyring",
    "error",
    "cancelled",
    "runtime_update",
    "incomplete",
    "signing_in",
];
pub fn config(runtime: &Path) -> Result<Value> {
    let info = crate::cloud_runtime::config(runtime)?;
    let root = xml::parse(info["config"].as_str().unwrap_or("").as_bytes())?;
    fn apps(root: &xml::Element, out: &mut Vec<String>) {
        if root.start.local_name().as_ref() == "MSAAppId" {
            out.push(root.text());
        }
        for item in &root.items {
            if let xml::Item::Element(child) = item {
                apps(child, out);
            }
        }
    }
    let mut found = Vec::new();
    apps(&root, &mut found);
    let settings: Value = files::json(&runtime.join("private/runtime.json"), 65536)?;
    let market = settings["market"].as_str().unwrap_or("");
    crate::error::require(
        found.len() == 1
            && (cloud::guid(&found[0])
                || found[0].len() == 16 && found[0].bytes().all(|b| b.is_ascii_hexdigit()))
            && market.len() == 2
            && market.bytes().all(|b| b.is_ascii_uppercase()),
        "Ungültige öffentliche Spielkonfiguration.",
    )?;
    Ok(
        json!({"store_id":crate::games::Game::for_runtime(runtime)?.store_id(),"title_id":info["title_id"],"msa_app_id":found[0],"package_family_name":info["pfn"],"market":market}),
    )
}
pub fn verify_runtime(root: &Path) -> Result<Value> {
    let lock = resources::json("compat/bootstrap.lock.json")?;
    for (name, paths) in crate::components::FILES
        .iter()
        .zip(crate::components::targets(root))
    {
        for path in paths {
            let file = crate::transaction::owned_file(root, &path)?;
            crate::error::require(
                files::digest(file)? == lock["native"]["files"][*name],
                "Runtime-Komponenten benötigen ein Update.",
            )?;
        }
    }
    store_diagnostics::launch_record(root).ok_or(Error::Invalid(
        "Die Runtime-Komponenten konnten nicht geprüft werden.",
    ))
}
pub fn snapshot(app: &Arc<Launcher>) -> Value {
    let s = app.lock();
    json!({"job":s.jobs.get("store-check").filter(|j|j["runtime_path"].as_str()==s.runtime.as_deref().and_then(|p|p.to_str()))})
}
pub fn report(s: &crate::backend::State) -> Value {
    let Some(job) = s
        .jobs
        .get("store-check")
        .filter(|j| j["runtime_path"].as_str() == s.runtime.as_deref().and_then(|p| p.to_str()))
    else {
        return Value::Null;
    };
    ["state", "started_at", "finished_at", "steps", "components"]
        .into_iter()
        .map(|k| (k.into(), job[k].clone()))
        .collect::<serde_json::Map<_, _>>()
        .into()
}
fn step(ctx: &Context, stage: &str, state: &str, code: &str) {
    let mut s = ctx.launcher.lock();
    if let Some(rows) = s
        .jobs
        .get_mut("store-check")
        .and_then(|j| j["steps"].as_array_mut())
        && let Some(row) = rows.iter_mut().find(|r| r["stage"] == stage)
    {
        row["state"] = json!(state);
        row["code"] = json!(code);
    }
}
pub fn finish_pending(rows: &mut [Value], stages: &[&str], code: &str) {
    for row in rows {
        if row["stage"].as_str().is_some_and(|s| stages.contains(&s))
            && matches!(row["state"].as_str(), Some("pending" | "running"))
        {
            row["state"] = json!(if code == "cancelled" {
                "cancelled"
            } else {
                "failed"
            });
            row["code"] = json!(code);
        }
    }
}
pub fn event(raw: &[u8], rows: &mut [Value], allowed: &[&str]) -> bool {
    let Ok(value) = cloud::json(raw) else {
        return false;
    };
    if value == json!({"stage":"check","state":"failed","code":"timeout"})
        || value == json!({"stage":"check","state":"failed","code":"error"})
    {
        finish_pending(rows, allowed, value["code"].as_str().unwrap_or("error"));
        return true;
    }
    if value.as_object().is_none_or(|m| m.len() != 3) {
        return false;
    }
    let (Some(stage), Some(state), Some(code)) = (
        value["stage"].as_str(),
        value["state"].as_str(),
        value["code"].as_str(),
    ) else {
        return false;
    };
    if !allowed.contains(&stage)
        || !["running", "passed", "failed", "skipped", "cancelled"].contains(&state)
        || !CODES.contains(&code)
    {
        return false;
    }
    let expected = match stage {
        "account" => "local_session",
        "catalog" | "library" => "available",
        "license" => "verified",
        "window" => "visible",
        _ => "",
    };
    if state == "passed" && code != expected || state == "running" && code != "checking" {
        return false;
    }
    let Some(row) = rows.iter_mut().find(|r| r["stage"] == stage) else {
        return false;
    };
    if !matches!(row["state"].as_str(), Some("pending" | "running")) {
        return false;
    }
    *row = value;
    true
}
fn pending(ctx: &Context, allowed: &[&str], code: &str) {
    let mut s = ctx.launcher.lock();
    if let Some(rows) = s
        .jobs
        .get_mut("store-check")
        .and_then(|j| j["steps"].as_array_mut())
    {
        finish_pending(rows, allowed, code);
    }
}
pub fn check_process(
    ctx: &Context,
    command: &mut Command,
    payload: &[u8],
    allowed: &[&str],
    timeout: Duration,
) -> Result<&'static str> {
    crate::error::require(payload.len() <= 4096, "Ungültige Prüfanfrage.")?;
    for category in ["CONFIG", "DATA", "CACHE", "STATE"] {
        let path = ctx
            .root()?
            .join("private/xdg")
            .join(category.to_ascii_lowercase());
        files::private_dir(&path)?;
        command.env(format!("XDG_{category}_HOME"), path);
    }
    command
        .env("XODUS_LOG", "off")
        .env("RUST_LOG", "off")
        .env("RUST_BACKTRACE", "0")
        .env_remove("FLIGHTDECK_STORE_LAUNCH")
        .current_dir(ctx.root()?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = process::spawn(command, None)?;
    let result = (|| -> Result<&'static str> {
        let mut input = child
            .stdin
            .take()
            .ok_or(Error::Invalid("Die Store-Prüfung hat keinen Eingabekanal."))?;
        input.write_all(payload)?;
        drop(input);
        let mut output = child
            .stdout
            .take()
            .ok_or(Error::Invalid("Die Store-Prüfung hat keinen Ausgabekanal."))?;
        rustix::fs::fcntl_setfl(
            &output,
            rustix::fs::fcntl_getfl(&output)? | rustix::fs::OFlags::NONBLOCK,
        )?;
        let deadline = Instant::now() + timeout;
        let mut pending_bytes = Vec::new();
        let mut total = 0;
        let mut ended = false;
        let mut status = None;
        loop {
            if ctx.cancel.load(Ordering::Acquire) {
                return Ok("cancelled");
            }
            if Instant::now() >= deadline {
                return Ok("timeout");
            }
            let mut buffer = [0; 4096];
            loop {
                match output.read(&mut buffer) {
                    Ok(0) => {
                        ended = true;
                        break;
                    }
                    Ok(n) => {
                        total += n;
                        if total > 65536 {
                            return Ok("error");
                        }
                        pending_bytes.extend_from_slice(&buffer[..n]);
                        while let Some(end) = pending_bytes.iter().position(|b| *b == b'\n') {
                            let valid = if end > 1024 {
                                false
                            } else {
                                let mut s = ctx.launcher.lock();
                                s.jobs
                                    .get_mut("store-check")
                                    .and_then(|j| j["steps"].as_array_mut())
                                    .is_some_and(|rows| event(&pending_bytes[..end], rows, allowed))
                            };
                            if !valid {
                                return Ok("error");
                            }
                            pending_bytes.drain(..=end);
                        }
                        if pending_bytes.len() > 1024 {
                            return Ok("error");
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            if ended && let Some(status) = status {
                return Ok(if status && pending_bytes.is_empty() {
                    "available"
                } else {
                    "error"
                });
            }
            status = child.try_wait()?.map(|code| code.success());
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    let cleanup = process::terminate_group(&mut child);
    let result = result?;
    cleanup?;
    Ok(result)
}
pub fn start(
    app: &Arc<Launcher>,
    language: &str,
    recover: bool,
    data: &Value,
    cloud_request: bool,
) -> Result<Value> {
    let request_id = if cloud_request {
        data["request_id"].as_str().map(str::to_owned)
    } else {
        None
    };
    let mut s = app.lock();
    if !data["job_id"].is_null() {
        crate::error::require(
            data["job_id"].is_string()
                && s.jobs.get("store-check").is_some_and(|job| {
                    job["id"] == data["job_id"]
                        && job["runtime_path"].as_str()
                            == s.runtime.as_deref().and_then(|p| p.to_str())
                }),
            "Diese Store-Prüfung ist nicht mehr aktuell. Bitte den Status neu laden.",
        )?;
    }
    if cloud_request {
        let cloud = crate::cloud_sync::automatic(&s);
        crate::error::require(
            recover
                && cloud["can_sign_in"] == true
                && request_id
                    .as_ref()
                    .is_some_and(|id| cloud["request_id"] == *id),
            "Dieser Cloud-Vorgang ist nicht mehr aktuell. Bitte den Status neu laden.",
        )?;
    }
    let ctx = app.reserve_locked(
        &mut s,
        "store-check",
        if recover { "recover" } else { "check" },
        true,
    )?;
    drop(s);
    let stages = if recover {
        vec![
            "runtime", "sign_in", "account", "catalog", "license", "library",
        ]
    } else {
        STAGES.to_vec()
    };
    ctx.update(json!({"state":"running","finished_at":null,"steps":stages.iter().map(|s|json!({"stage":s,"state":"pending","code":"checking"})).collect::<Vec<_>>(),"components":null}));
    let language = if language == "de" { "de" } else { "en" };
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(&ctx, language, recover)
        }))
        .unwrap_or(Err(Error::Invalid(
            "Die Store-Prüfung wurde unerwartet beendet.",
        )));
        if outcome.is_err() {
            pending(
                &ctx,
                &stages,
                if ctx.cancel.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "error"
                },
            );
        }
        let mut job = ctx.launcher.job("store-check");
        if let Some(rows) = job["steps"].as_array_mut() {
            for row in rows.iter_mut() {
                if matches!(row["state"].as_str(), Some("running" | "pending")) {
                    row["state"] = json!(if ctx.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "skipped"
                    });
                    row["code"] = json!(if ctx.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "incomplete"
                    });
                }
            }
            let states: BTreeSet<_> = rows.iter().filter_map(|r| r["state"].as_str()).collect();
            let state = if states == ["passed"].into() {
                "passed"
            } else if ctx.cancel.load(Ordering::Acquire) {
                "cancelled"
            } else if states.contains("failed") {
                "failed"
            } else {
                "incomplete"
            };
            job["state"] = json!(state);
        }
        job["finished_at"] = json!(files::now());
        let success = job["state"] == "passed" && !ctx.cancel.load(Ordering::Acquire);
        ctx.launcher.finish(&ctx, Ok(job), false);
        let same_runtime = ctx.launcher.lock().runtime == ctx.runtime;
        if success
            && let Some(id) = request_id
            && same_runtime
        {
            let _ = crate::cloud_auto::action(&ctx.launcher, "retry", &json!({"request_id":id}));
        }
    });
    Ok(json!({"ok":true,"job":app.job("store-check")}))
}
fn run(ctx: &Context, language: &str, recover: bool) -> Result<()> {
    let root = ctx.root()?;
    step(ctx, "runtime", "running", "checking");
    let components = match verify_runtime(root) {
        Ok(v) => v,
        Err(_) => {
            step(ctx, "runtime", "failed", "runtime_update");
            return Ok(());
        }
    };
    ctx.update(json!({"components":components}));
    step(ctx, "runtime", "passed", "verified");
    if recover {
        step(ctx, "sign_in", "running", "signing_in");
        let result = game_install::run_cli(
            &root.join("bin/xodus-cli"),
            &["login".into(), "--same-account".into()],
            &root.join("private"),
            Some(&root.join("private/xdg")),
            ctx,
            game_install::Options {
                timeout: Some(Duration::from_secs(900)),
                ..Default::default()
            },
        )?;
        let code = match result {
            game_install::Exit::Code(c) => c,
            game_install::Exit::Paused => -1,
        };
        if let Err(error) = game_install::login_result(code) {
            step(ctx, "sign_in", "failed", "sign_in_required");
            let mut s = ctx.launcher.lock();
            if let Some(row) = s
                .jobs
                .get_mut("store-check")
                .and_then(|j| j["steps"].as_array_mut())
                .and_then(|rows| rows.iter_mut().find(|r| r["stage"] == "sign_in"))
            {
                row["exit_code"] = json!(code);
                row["message"] = json!(error.to_string());
            }
            return Ok(());
        }
        step(ctx, "sign_in", "passed", "verified");
    }
    match config(root) {
        Err(_) => pending(ctx, &ONLINE, "invalid_config"),
        Ok(config) => {
            let code = check_process(
                ctx,
                Command::new(root.join("bin/xodus-service")).arg("--store-check"),
                &serde_json::to_vec(&config)?,
                &ONLINE,
                Duration::from_secs(125),
            )?;
            pending(
                ctx,
                &ONLINE,
                if code == "available" {
                    "incomplete"
                } else {
                    code
                },
            );
        }
    }
    if !recover && !ctx.cancel.load(Ordering::Acquire) {
        step(ctx, "window", "running", "checking");
        let code = check_process(
            ctx,
            Command::new(root.join("bin/xodus-cli")).args([
                "store-window-check",
                "--language",
                language,
            ]),
            &[],
            &["window"],
            Duration::from_secs(95),
        )?;
        pending(
            ctx,
            &["window"],
            if code == "available" {
                "incomplete"
            } else {
                code
            },
        );
    }
    Ok(())
}
