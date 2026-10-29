// SPDX-License-Identifier: MIT
//! Retain only fixed setup categories and numeric results, including before a runtime exists.
use crate::{backend::Context, files};
use serde_json::{Value, json};
use std::path::Path;

const FILE: &str = "setup-attempt.json";

pub fn summary(job: &Value) -> Value {
    let Some(id) = job["id"]
        .as_str()
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return Value::Null;
    };
    let mut report = json!({"schema":1,"id":id});
    for (key, allowed) in [
        ("mode", &["install", "prepare", "existing", "update"][..]),
        ("game_id", &["msfs2020", "msfs2024"][..]),
        (
            "state",
            &[
                "checking",
                "ready",
                "installing",
                "failed",
                "cancelled",
                "complete",
            ][..],
        ),
    ] {
        if let Some(value) = job[key].as_str().filter(|s| allowed.contains(s)) {
            report[key] = json!(value);
        }
    }
    const PHASES: &[&str] = &[
        "paths",
        "ready",
        "recheck",
        "bootstrap",
        "authentication",
        "download",
        "pausing",
        "paused",
        "download_failed",
        "provision",
        "verify_update",
        "integrity_check",
        "switch_update",
        "publish_runtime",
        "complete",
        "failed",
        "cancelled",
    ];
    for key in ["phase", "failure_phase"] {
        // Retried jobs keep their earlier failure fields while progressing.
        // A completed installation must not look like a current auth failure.
        if key == "failure_phase"
            && job["state"] != "failed"
            && !(job["state"] == "cancelled"
                && ["download_exit_code", "login_exit_code"]
                    .iter()
                    .any(|code| job[code].as_i64().is_some_and(|value| value != 0)))
        {
            continue;
        }
        if let Some(value) = job[key].as_str().filter(|s| PHASES.contains(s)) {
            report[key] = json!(value);
        }
    }
    for key in ["started_at", "recorded_at"] {
        if let Some(value) = job[key]
            .as_str()
            .filter(|s| s.len() <= 40 && chrono::DateTime::parse_from_rfc3339(s).is_ok())
        {
            report[key] = json!(value);
        }
    }
    for key in ["download_exit_code", "login_exit_code"] {
        if let Some(value) = job[key].as_i64().filter(|n| (-1..=255).contains(n)) {
            report[key] = json!(value);
        }
    }
    if let Some(value) = job["download_attempt"].as_u64().filter(|n| *n <= 1_000_000) {
        report["download_attempt"] = json!(value);
    }
    if let Ok(transfer) =
        serde_json::from_value::<crate::progress::Transfer>(job["transfer"].clone())
        && transfer.valid()
    {
        report["transfer"] = json!(transfer);
    }
    report
}

pub fn load(state: &Path) -> Value {
    files::json::<Value>(&state.join(FILE), 16 * 1024)
        .ok()
        .filter(|v| v["schema"] == 1)
        .map(|v| summary(&v))
        .unwrap_or(Value::Null)
}

pub fn persist(state: &Path, job: &Value) -> Value {
    let mut report = summary(job);
    if report.is_object() {
        report["recorded_at"] = json!(files::now());
        // Diagnostic storage failure must not abort an otherwise usable installation.
        let _ = files::atomic_json(&state.join(FILE), &report);
    }
    report
}

pub fn record(ctx: &Context) {
    if ctx.kind != "setup" {
        return;
    }
    let mut state = ctx.launcher.lock();
    if let Some(job) = state.jobs.get("setup").filter(|v| v["id"] == ctx.id) {
        state.installation_report = persist(&ctx.launcher.state_dir, job);
    }
}
