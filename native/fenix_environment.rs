// SPDX-License-Identifier: MIT
//! Repair the legacy native-only CLR override and demonstrably missing startup hooks.
//! Existing hooks and unresolved/relative/expanded values are preserved.
use crate::{Result, error::require, files, framework_repair, process, wine::Wine};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn missing(prefix: &Path, hook: &str) -> bool {
    let bytes = hook.as_bytes();
    if !hook.is_ascii()
        || bytes.len() < 3
        || !bytes[0].eq_ignore_ascii_case(&b'c')
        || bytes[1] != b':'
        || !b"\\/".contains(&bytes[2])
        || hook.contains(['%', '\0', '\n', '\r'])
    {
        return false;
    }
    let mut path = prefix.join("drive_c");
    for part in hook[3..].split(['\\', '/']) {
        if part.is_empty() || part == "." {
            continue;
        }
        // Ambiguous paths must not cause a destructive registry change.
        if part == ".."
            || part.ends_with(['.', ' '])
            || part.contains(['*', '?', '<', '>', '|', ':', '"'])
        {
            return false;
        }
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) => return false,
        };
        let mut next = None;
        for (count, entry) in entries.take(100_001).enumerate() {
            if count >= 100_000 {
                return false;
            }
            let Ok(entry) = entry else {
                return false;
            };
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(part)
            {
                next = Some(entry.path());
                break;
            }
        }
        let Some(next) = next else {
            return true;
        };
        path = next;
    }
    matches!(fs::metadata(path),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
}

fn filtered(prefix: &Path, value: &str) -> Option<String> {
    let hooks: Vec<_> = value.split(';').collect();
    let kept: Vec<_> = hooks
        .iter()
        .copied()
        .filter(|hook| !missing(prefix, hook.trim()))
        .collect();
    (kept.len() != hooks.len()).then(|| kept.join(";"))
}

fn backup(prefix: &Path, change: Value) -> Result<()> {
    let backup = prefix.join(".flightdeck-fenix-environment.json");
    let mut record: Value = if files::exists(&backup) {
        files::json(&backup, 65536)?
    } else {
        json!({"schema":1,"changes":[]})
    };
    require(
        record["schema"] == 1 && record["changes"].as_array().is_some_and(|v| v.len() < 32),
        "Die Sicherung der Fenix-Umgebung konnte nicht geprüft werden.",
    )?;
    record["changes"]
        .as_array_mut()
        .expect("checked array")
        .push(change);
    files::atomic_json(&backup, &record)
}

fn native_only(output: &str) -> Option<&str> {
    let values: Vec<_> = output
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.trim_start().split_once(char::is_whitespace)?;
            if !name.eq_ignore_ascii_case("mscoree") {
                return None;
            }
            let (kind, value) = rest.trim_start().split_once(char::is_whitespace)?;
            Some((kind, value.trim()))
        })
        .collect();
    match values.as_slice() {
        [("REG_SZ", value)]
            if value.eq_ignore_ascii_case("native") || value.eq_ignore_ascii_case("n") =>
        {
            Some(*value)
        }
        _ => None,
    }
}

fn repair_loader(wine: &Wine<'_>) -> Result<()> {
    const KEY: &str = r"HKCU\Software\Wine\DllOverrides";
    let mut command = wine.command()?;
    let (status, output) = process::output_status(
        command.args(["reg", "query", KEY, "/v", "mscoree"]),
        Duration::from_secs(30),
        16384,
        wine.cancel,
    )?;
    let output = String::from_utf8_lossy(&output);
    if status.success()
        && let Some(before) = native_only(&output)
    {
        // Only migrate native-only values, including the old setup policy.
        // Explicit disabled/builtin preferences and per-app settings survive.
        // Native DLLs retain precedence; no DLL file is removed or replaced.
        backup(
            &wine.prefix,
            json!({"key":KEY,"name":"mscoree","kind":"REG_SZ",
            "before":before,"after":framework_repair::MSCOREE_OVERRIDE,"at":files::now()}),
        )?;
        wine.reg(KEY, "mscoree", framework_repair::MSCOREE_OVERRIDE, "REG_SZ")?;
    }
    Ok(())
}

pub(crate) fn prepare(wine: &Wine<'_>) -> Result<()> {
    repair_loader(wine)?;
    for key in [
        r"HKCU\Environment",
        r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    ] {
        let mut command = wine.command()?;
        let (status, output) = process::output_status(
            command.args(["reg", "query", key, "/v", "DOTNET_STARTUP_HOOKS"]),
            Duration::from_secs(30),
            16384,
            wine.cancel,
        )?;
        if !status.success() {
            continue;
        }
        let output = String::from_utf8_lossy(&output);
        let entry = output.lines().find_map(|line| {
            let line = line.trim_start();
            let (name, rest) = line.split_once(char::is_whitespace)?;
            if !name.eq_ignore_ascii_case("DOTNET_STARTUP_HOOKS") {
                return None;
            }
            let (kind, value) = rest.trim_start().split_once(char::is_whitespace)?;
            ["REG_SZ", "REG_EXPAND_SZ"]
                .contains(&kind)
                .then(|| (kind, value.trim_start()))
        });
        let Some((kind, value)) = entry else {
            continue;
        };
        let Some(replacement) = filtered(&wine.prefix, value) else {
            continue;
        };
        // Keep the original value locally, never in safe diagnostics or HTTP.
        backup(
            &wine.prefix,
            json!({"key":key,"name":"DOTNET_STARTUP_HOOKS","kind":kind,"before":value,"after":replacement,"at":files::now()}),
        )?;
        if replacement.is_empty() {
            wine.run(
                &["reg", "delete", key, "/v", "DOTNET_STARTUP_HOOKS", "/f"],
                None,
                &[0],
                Duration::from_secs(30),
            )?;
        } else {
            wine.reg(key, "DOTNET_STARTUP_HOOKS", &replacement, kind)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};
    #[test]
    fn loader_migration_is_backed_up_idempotent_and_preserves_explicit_preferences() {
        for value in ["n", "native", "builtin", "", "native,builtin"] {
            let temp = tempfile::tempdir().expect("runtime");
            let prefix = temp.path().join("prefix");
            let runner = temp.path().join("runner");
            files::private_dir(&prefix.join("drive_c/windows/system32")).expect("prefix");
            files::private_dir(&runner.join("files/bin")).expect("runner");
            fs::write(prefix.join("override"), value).expect("value");
            let binary = runner.join("files/bin/wine");
            fs::write(&binary, b"#!/bin/sh\nif [ \"$2\" = query ]; then\n read -r value < \"$WINEPREFIX/override\" || true\n printf '    mscoree    REG_SZ    %s\\n' \"$value\"\nelif [ \"$2\" = add ]; then\n printf '%s' \"$9\" > \"$WINEPREFIX/override\"\nfi\n").expect("Wine fixture");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).expect("mode");
            let cancel = AtomicBool::new(false);
            let wine =
                Wine::new(&prefix, &runner, &temp.path().join("wine.log"), &cancel).expect("Wine");
            for _ in 0..2 {
                repair_loader(&wine).expect("repair");
            }
            if ["n", "native"].contains(&value) {
                assert_eq!(
                    fs::read_to_string(prefix.join("override")).expect("value"),
                    "native,builtin"
                );
                let record: Value =
                    files::json(&prefix.join(".flightdeck-fenix-environment.json"), 65536)
                        .expect("backup");
                assert_eq!(record["changes"].as_array().expect("changes").len(), 1);
                assert_eq!(record["changes"][0]["before"], value);
                // An unreadable backup must prevent a registry mutation.
                fs::write(prefix.join("override"), "native").expect("reset");
                fs::write(
                    prefix.join(".flightdeck-fenix-environment.json"),
                    b"invalid backup",
                )
                .expect("invalid");
                assert!(repair_loader(&wine).is_err());
                assert_eq!(
                    fs::read_to_string(prefix.join("override")).expect("value"),
                    "native"
                );
            } else {
                assert_eq!(
                    fs::read_to_string(prefix.join("override")).expect("value"),
                    value
                );
                assert!(!prefix.join(".flightdeck-fenix-environment.json").exists());
            }
        }
    }
    #[test]
    fn only_the_unambiguous_native_only_override_is_migrated() {
        for value in ["native", "NATIVE", "n", "n   "] {
            assert!(native_only(&format!("HKEY_CURRENT_USER\\Software\\Wine\\DllOverrides\n    mscoree    REG_SZ    {value}\n")).is_some());
        }
        for value in [
            "",
            "builtin",
            "b",
            "native,builtin",
            "n,b",
            "disabled",
            "native,disabled",
        ] {
            assert!(native_only(&format!("mscoree REG_SZ {value}\n")).is_none());
        }
        assert!(native_only("other REG_SZ native\n").is_none());
        assert!(native_only("mscoree REG_EXPAND_SZ native\n").is_none());
        assert!(native_only("mscoree REG_SZ native\nmscoree REG_SZ native\n").is_none());
    }
    #[test]
    fn only_missing_absolute_paths_are_removed_and_existing_case_insensitive_hooks_survive() {
        let temp = tempfile::tempdir().expect("prefix");
        files::private_dir(&temp.path().join("drive_c/Hooks")).expect("hooks");
        fs::write(
            temp.path().join("drive_c/Hooks/Valid.dll"),
            b"retained user hook",
        )
        .expect("hook");
        let value = r"C:\Missing.dll;C:\hooks\valid.DLL;CustomAssembly;%USERPROFILE%\hook.dll;Z:\outside.dll";
        assert_eq!(
            filtered(temp.path(), value).as_deref(),
            Some(r"C:\hooks\valid.DLL;CustomAssembly;%USERPROFILE%\hook.dll;Z:\outside.dll")
        );
        assert_eq!(
            filtered(temp.path(), r"C:\missing.dll"),
            Some(String::new())
        );
        assert_eq!(filtered(temp.path(), r"C:\..\ambiguous.dll"), None);
        assert_eq!(filtered(temp.path(), r"C:\Hooks\Valid.dll"), None);
        assert_eq!(filtered(temp.path(), r"C:\Hooks\Valid.dll."), None);
        assert_eq!(filtered(temp.path(), r"C:\Hooks\Ünicode.dll"), None);
        assert_eq!(filtered(temp.path(), r"C:\Hooks\Valid.dll:stream"), None);
    }
}
