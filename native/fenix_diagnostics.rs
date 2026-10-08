// SPDX-License-Identifier: MIT
//! Latest Fenix invocation evidence; never export paths or third-party log text.
use crate::{Error, Result, error::require, files, log_reader::regex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const FILE: &str = "fenix-installer-result.json";
const MAXIMUM: usize = 4096;
pub(crate) const MAX_MANAGED_EXCEPTION_TYPES: usize = 8;
// Exact exception identifiers only. Messages, application-specific type names,
// stack frames and file paths are never persisted or exported.
pub(crate) const MANAGED_EXCEPTION_TYPES: &[&str] = &[
    "System.Exception",
    "System.ArgumentException",
    "System.ArgumentNullException",
    "System.ArgumentOutOfRangeException",
    "System.BadImageFormatException",
    "System.DllNotFoundException",
    "System.EntryPointNotFoundException",
    "System.InvalidOperationException",
    "System.IO.DirectoryNotFoundException",
    "System.IO.FileLoadException",
    "System.IO.FileNotFoundException",
    "System.IO.IOException",
    "System.MissingMethodException",
    "System.NotSupportedException",
    "System.NullReferenceException",
    "System.PlatformNotSupportedException",
    "System.Reflection.ReflectionTypeLoadException",
    "System.Runtime.InteropServices.COMException",
    "System.Security.SecurityException",
    "System.TypeInitializationException",
    "System.TypeLoadException",
    "System.UnauthorizedAccessException",
];

pub(crate) fn managed_exception_type(value: &str) -> Option<&'static str> {
    MANAGED_EXCEPTION_TYPES
        .iter()
        .copied()
        .find(|known| *known == value)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub schema: u8,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub operation: String,
    pub status: String,
    pub failure: Option<String>,
    pub package_version: Option<String>,
    pub hook: Option<String>,
    pub hook_exit_code: Option<i32>,
    pub process_exit_code: Option<i32>,
    pub evidence_complete: bool,
    pub runner: String,
    #[serde(default)]
    pub managed_exception_types: Vec<String>,
    #[serde(default)]
    pub clr_exception_code: Option<String>,
}

pub fn valid_version(version: &str) -> bool {
    regex(r"^[0-9]{1,5}\.[0-9]{1,5}\.[0-9]{1,5}(?:\.[0-9]{1,5})?$").is_match(version)
}

fn timestamp(value: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    if !regex(
        r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?(?:Z|\+00:00)$",
    )
    .is_match(value)
    {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

impl Attempt {
    fn valid(&self) -> bool {
        let Some(started) = timestamp(&self.started_at) else {
            return false;
        };
        let completion_valid = match self.completed_at.as_deref() {
            Some(at) => self.status != "running" && timestamp(at).is_some_and(|at| at >= started),
            None => self.status == "running",
        };
        self.schema == 1
            && completion_valid
            && ["installer", "manager", "repair"].contains(&self.operation.as_str())
            && ["running", "succeeded", "failed", "cancelled", "unknown"]
                .contains(&self.status.as_str())
            && [
                "flightdeck",
                "experimental",
                "cachyos",
                "ge_proton",
                "proton",
                "custom",
                "unknown",
            ]
            .contains(&self.runner.as_str())
            && self.package_version.as_deref().is_none_or(valid_version)
            && self.managed_exception_types.len() <= MAX_MANAGED_EXCEPTION_TYPES
            && self
                .managed_exception_types
                .iter()
                .enumerate()
                .all(|(index, value)| {
                    managed_exception_type(value).is_some()
                        && !self.managed_exception_types[..index].contains(value)
                })
            && self
                .clr_exception_code
                .as_deref()
                .is_none_or(|code| code == "e0434352")
            && (self.failure.as_deref() != Some("managed_exception")
                || !self.managed_exception_types.is_empty()
                || self.clr_exception_code.is_some())
            && self
                .hook
                .as_deref()
                .is_none_or(|hook| ["install", "updated"].contains(&hook))
            && self.failure.as_deref().is_none_or(|failure| {
                [
                    "icu_symbol_missing",
                    "managed_exception",
                    "hook_nonzero",
                    "hook_timeout",
                    "hook_incomplete",
                    "hook_failed",
                    "process_nonzero",
                    "spawn_failed",
                    "preparation_failed",
                    "invalid_metadata",
                    "log_unavailable",
                    "log_changed",
                    "log_too_large",
                    "unknown",
                ]
                .contains(&failure)
            })
            && (!matches!(self.status.as_str(), "running" | "succeeded") || self.failure.is_none())
    }
}

fn write(root: &Path, attempt: &Attempt) -> Result<()> {
    require(attempt.valid(), "Invalid Fenix diagnostic evidence.")?;
    let folder = files::directory(&root.join("private"), true)?;
    files::atomic_at(&folder, FILE, &serde_json::to_vec(attempt)?)
}

/// Replace the previous result before this invocation starts, including when a
/// later spawn fails. Runtime/job ownership remains with the existing caller.
pub fn begin(root: &Path, operation: &str, runner: &str) -> Result<Attempt> {
    let attempt = Attempt {
        schema: 1,
        started_at: files::now(),
        completed_at: None,
        operation: operation.into(),
        status: "running".into(),
        failure: None,
        package_version: None,
        hook: None,
        hook_exit_code: None,
        process_exit_code: None,
        evidence_complete: false,
        runner: runner.into(),
        managed_exception_types: Vec::new(),
        clr_exception_code: None,
    };
    write(root, &attempt)?;
    Ok(attempt)
}

fn read(root: &Path) -> Result<Attempt> {
    let folder = files::directory(&root.join("private"), true)?;
    let file = files::open_at(&folder, FILE, false, true)?;
    let raw = files::read_file(file, MAXIMUM)?;
    // Retain the existing duplicate-key rejection for diagnostic records too.
    let value = crate::cloud::json(&raw)?;
    let attempt: Attempt = serde_json::from_value(value)?;
    require(attempt.valid(), "Invalid Fenix diagnostic evidence.")?;
    Ok(attempt)
}

/// A late completion must not replace a more recent invocation's evidence.
pub fn save(root: &Path, attempt: &Attempt) -> Result<()> {
    let current = read(root)?;
    require(
        current.started_at == attempt.started_at
            && current.operation == attempt.operation
            && current.runner == attempt.runner,
        "The Fenix diagnostic attempt has changed.",
    )?;
    write(root, attempt)
}

/// Reconstruct only typed, validated evidence. Missing or untrusted records do
/// not imply either a successful installation or a known failure.
pub fn load(root: &Path) -> Value {
    read(root)
        .and_then(|attempt| serde_json::to_value(attempt).map_err(Error::from))
        .map(|mut value| {
            value["scope"] = json!("latest_recorded_attempt");
            value
        })
        .unwrap_or(Value::Null)
}
