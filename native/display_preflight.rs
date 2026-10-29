// SPDX-License-Identifier: MIT
//! A fresh, bounded X11 handshake before Wine, with narrowly scoped recovery.
//!
//! The native helper does the potentially blocking XOpenDisplay call in a child.
//! No display addresses, authentication material or configuration contents are
//! returned by this module. Recovery is deliberately limited to the exact
//! Hyprland build for which config-only XWayland recreation was verified.
use crate::{Error, Result, files, graphics, wine};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[path = "display_repair.rs"]
mod repair;

pub use repair::{Compositor, Desktop};

const UNAVAILABLE: &str = "X11 ist nicht erreichbar. Die Grafiksitzung bitte prüfen und den Start erneut versuchen; die Anmeldung wurde nicht gestartet.";
const MISSING_LIBRARY: &str = "Die X11-Bibliothek libX11.so.6 fehlt oder kann nicht geladen werden. Bitte die X11-Laufzeit installieren und erneut starten.";
const UNSUPPORTED: &str = "X11 ist nicht erreichbar. Eine automatische Reparatur ist nur für die geprüfte Hyprland-0.56.2-Sitzung mit Omarchy-Lua-Konfiguration möglich. Bitte die Grafiksitzung reparieren und erneut starten.";
const BUSY: &str = "X11 ist nicht erreichbar, aber X11-Fenster oder ein XWayland-Prozess sind noch vorhanden. Flightdeck verändert diese Sitzung nicht. Bitte die betroffenen Programme schließen oder die Sitzung prüfen und erneut starten.";
const UNSAFE_CONFIG: &str = "Die Hyprland-Konfiguration wurde geändert oder ist nicht sicher zugänglich. Flightdeck hat die automatische X11-Reparatur angehalten; bitte die Konfiguration prüfen und erneut starten.";
const CONFIG_ERRORS: &str = "Hyprland meldet Konfigurationsfehler. Bitte diese beheben und den Start erneut versuchen; Flightdeck setzt die Desktop-Konfiguration nicht zurück.";
const RESTORE_PENDING: &str = "Die vorübergehende X11-Reparatur muss noch wiederhergestellt werden. Bitte Flightdeck in derselben Sitzung erneut öffnen. Eigene Konfigurationsänderungen und die private Sicherung bleiben erhalten.";
const PROBE_FAILED: &str = "Die X11-Grafikvorprüfung hat nicht rechtzeitig oder nicht gültig geantwortet. Bitte die Grafiksitzung prüfen und erneut starten.";

#[derive(Clone, Copy, Debug)]
pub(crate) enum Failure {
    Unavailable,
    MissingLibrary,
    Unsupported,
    Busy,
    UnsafeConfig,
    ConfigErrors,
    RestorePending,
    ProbeFailed,
}
pub(crate) fn failure(kind: Failure) -> Error {
    Error::Invalid(match kind {
        Failure::Unavailable => UNAVAILABLE,
        Failure::MissingLibrary => MISSING_LIBRARY,
        Failure::Unsupported => UNSUPPORTED,
        Failure::Busy => BUSY,
        Failure::UnsafeConfig => UNSAFE_CONFIG,
        Failure::ConfigErrors => CONFIG_ERRORS,
        Failure::RestorePending => RESTORE_PENDING,
        Failure::ProbeFailed => PROBE_FAILED,
    })
}
fn graphics_error(error: Error) -> Error {
    match error {
        Error::Cancelled | Error::Graphics(_) => error,
        _ => Error::Graphics(Box::new(error)),
    }
}

/// Stable safe identifiers for diagnostics and recoverable launch failures.
pub fn failure_code(error: &Error) -> Option<&'static str> {
    match error {
        Error::Graphics(inner) => failure_code(inner),
        Error::Invalid(message) => Some(match *message {
            UNAVAILABLE => "display_unavailable",
            MISSING_LIBRARY => "display_missing_library",
            UNSUPPORTED => "display_recovery_unsupported",
            BUSY => "display_recovery_busy",
            UNSAFE_CONFIG => "display_config_changed",
            CONFIG_ERRORS => "display_config_errors",
            RESTORE_PENDING => "display_restore_pending",
            PROBE_FAILED => "display_probe_failed",
            _ => return None,
        }),
        _ => None,
    }
}

/// Injection boundary for hardware-free recovery tests. Production always uses
/// the fixed system hyprctl and the current user's compositor/process table.
#[doc(hidden)]
pub trait Adapter {
    fn probe(&mut self, env: &graphics::Environment, cancel: &AtomicBool) -> Result<Value>;
    fn desktop(&self) -> Result<Desktop>;
    fn trusted_hyprctl(&self) -> bool;
    fn hyprctl(&mut self, args: &[&str], cancel: &AtomicBool) -> Result<Vec<u8>>;
    fn compositor(&self, pid: u32) -> Result<Compositor>;
    fn xwayland_present(&self) -> Result<bool>;
}

fn truthy(value: &str) -> bool {
    !value.is_empty() && !["0", "false", "off", "no"].contains(&value.to_ascii_lowercase().as_str())
}
fn wayland_environment(env: &BTreeMap<String, String>) -> bool {
    [
        "PROTON_ENABLE_WAYLAND",
        "PROTON_USE_WAYLAND",
        "WINE_WAYLAND_DRIVER",
    ]
    .iter()
    .any(|key| env.get(*key).is_some_and(|v| truthy(v)))
        || ["WINE_DRIVER", "WINE_GRAPHICS_DRIVER"].iter().any(|key| {
            env.get(*key)
                .is_some_and(|v| v.to_ascii_lowercase().contains("wayland"))
        })
        || env.get("WINEDLLOVERRIDES").is_some_and(|value| {
            value.split(';').any(|entry| {
                let Some((names, mode)) = entry.split_once('=') else {
                    return false;
                };
                let mode = mode.trim().to_ascii_lowercase();
                names.split(',').any(|name| {
                    let name = name.trim().trim_start_matches('*').to_ascii_lowercase();
                    (name == "winewayland.drv" && !mode.is_empty())
                        || (name == "winex11.drv" && ["", "d", "disabled"].contains(&mode.as_str()))
                })
            })
        })
}

fn registry_wayland(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut drivers = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            let section = line.split(']').next().unwrap_or("").to_ascii_lowercase();
            // Wine registry exports escape backslashes. Cover both export and
            // plain spelling, including application-specific driver choices.
            let section = section.replace("\\\\", "\\");
            drivers = (section == "[software\\wine\\drivers"
                || (section.starts_with("[software\\wine\\appdefaults\\")
                    && section.ends_with("\\drivers")))
                || section.ends_with("\\software\\wine\\drivers");
            continue;
        }
        if drivers
            && let Some((name, value)) = line.split_once('=')
            && name.trim().eq_ignore_ascii_case("\"Graphics\"")
            && value.to_ascii_lowercase().contains("wayland")
        {
            return true;
        }
    }
    false
}

/// Only an installed X11 Wine driver requires this preflight. This intentionally
/// leaves minimal test runners, native Wayland Wine and explicit AppDefaults
/// driver selections alone.
pub fn required(prefix: &Path, runner: &Path, env: &graphics::Environment) -> Result<bool> {
    let x11 = [
        "files/lib/wine/x86_64-unix/winex11.so",
        "files/lib64/wine/x86_64-unix/winex11.so",
        "files/lib/wine/x86_64-unix/winex11.drv.so",
        "files/lib64/wine/x86_64-unix/winex11.drv.so",
        "lib/wine/x86_64-unix/winex11.so",
        "lib64/wine/x86_64-unix/winex11.so",
    ]
    .iter()
    .any(|name| runner.join(name).is_file());
    if !x11 || wayland_environment(env) {
        return Ok(false);
    }
    for name in ["user.reg", "system.reg", "userdef.reg"] {
        let path = prefix.join(name);
        if files::exists(&path) && registry_wayland(&files::read(&path, 64 * 1024 * 1024)?) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn summary(state: &str, repaired: bool) -> Value {
    json!({"schema":1,"state":state,"repaired":repaired})
}
fn local_display(env: &graphics::Environment) -> Option<u32> {
    let address = env.get("DISPLAY")?;
    let local = address
        .strip_prefix(':')
        .or_else(|| address.strip_prefix("unix:"))
        .or_else(|| address.strip_prefix("unix/:"))?;
    let (number, screen) = local
        .split_once('.')
        .map_or((local, None), |(n, s)| (n, Some(s)));
    if number.is_empty()
        || !number.bytes().all(|b| b.is_ascii_digit())
        || screen.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    number.parse().ok().filter(|n| *n <= 65535)
}

/// Run this before authentication helpers as well as before the game itself.
/// `state_dir` must be the same private, user-level directory for all profiles.
pub fn ensure(root: &Path, state_dir: &Path, cancel: &AtomicBool) -> Result<Value> {
    let prefix = root.join("local/msfs-prefix");
    let runner = root.join("runner");
    ensure_profile(
        &prefix,
        &runner,
        &wine::environment(&prefix, &runner),
        state_dir,
        cancel,
    )
}

/// Explicit selected prefix/runner/environment binding for Steam and profiles.
pub fn ensure_profile(
    prefix: &Path,
    runner: &Path,
    env: &graphics::Environment,
    state_dir: &Path,
    cancel: &AtomicBool,
) -> Result<Value> {
    ensure_with(
        prefix,
        runner,
        env,
        state_dir,
        cancel,
        &mut repair::NativeAdapter,
    )
}

#[doc(hidden)]
pub fn ensure_with(
    prefix: &Path,
    runner: &Path,
    env: &graphics::Environment,
    state_dir: &Path,
    cancel: &AtomicBool,
    adapter: &mut impl Adapter,
) -> Result<Value> {
    (|| {
        // Replaying an interrupted repair precedes even a native-Wayland skip.
        let pending = repair::pending(state_dir);
        if !pending && !required(prefix, runner, env)? {
            return Ok(summary("not_required", false));
        }
        let _lock = repair::lock(state_dir)?;
        let restored = repair::restore(adapter, state_dir)?;
        let needs_x11 = required(prefix, runner, env)?;
        if !needs_x11 {
            return Ok(summary("not_required", restored));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let probe = match adapter.probe(env, cancel) {
            Ok(value) => value,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            // Only a child handshake timeout proves unavailable X11. A spawn
            // or protocol failure must never trigger a desktop mutation.
            Err(Error::Invalid("Der Vorgang hat nicht rechtzeitig geantwortet.")) => {
                json!({"schema":1,"state":"unavailable"})
            }
            Err(_) => return Err(failure(Failure::ProbeFailed)),
        };
        if probe["schema"] != 1 {
            return Err(failure(Failure::ProbeFailed));
        }
        match probe["state"].as_str() {
            Some("ready") => return Ok(summary("ready", restored)),
            Some("missing_library") => return Err(failure(Failure::MissingLibrary)),
            Some("not_required") => return Ok(summary("not_required", restored)),
            Some("unavailable") => {}
            _ => return Err(failure(Failure::ProbeFailed)),
        }
        let display = local_display(env).ok_or_else(|| failure(Failure::Unavailable))?;
        repair::repair(adapter, state_dir, cancel, display)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let verified = adapter.probe(env, cancel).map_err(|error| match error {
            Error::Cancelled => error,
            _ => failure(Failure::Unavailable),
        })?;
        if verified["schema"] != 1 || verified["state"] != "ready" {
            return Err(failure(Failure::Unavailable));
        }
        Ok(summary("ready", true))
    })()
    .map_err(graphics_error)
}

/// Startup replay is harmless when no repair is pending. Restoration is bounded
/// and deliberately finishes even if cancellation was requested after mutation.
pub fn restore_pending(state_dir: &Path, _cancel: &AtomicBool) -> Result<bool> {
    restore_pending_with(state_dir, &mut repair::NativeAdapter)
}

#[doc(hidden)]
pub fn restore_pending_with(state_dir: &Path, adapter: &mut impl Adapter) -> Result<bool> {
    if !repair::pending(state_dir) {
        return Ok(false);
    }
    (|| {
        let _lock = repair::lock(state_dir)?;
        repair::restore(adapter, state_dir)
    })()
    .map_err(graphics_error)
}

/// Read-only, deliberately non-probing diagnostic summary.
pub fn snapshot(root: &Path) -> Value {
    let prefix = root.join("local/msfs-prefix");
    let runner = root.join("runner");
    match required(&prefix, &runner, &wine::environment(&prefix, &runner)) {
        Ok(true) => summary("unchecked", false),
        Ok(false) => summary("not_required", false),
        Err(_) => summary("unavailable", false),
    }
}
