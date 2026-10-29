// SPDX-License-Identifier: MIT
//! Per-device Winebus access, gated by an independently verified official runner DLL.
use crate::{
    Error, Result, addon_apps::Profile, backend::Context, error::require, files, game_package,
    transaction as tx,
};
use serde_json::{Value, json};
use std::{fs, time::Duration};

const HASH: &str = "bb8e46a8b9b182f4aca1436fb620f68e52743826a674aafa39810f28f738e8f8";
const KEY: &str = r"HKLM\System\CurrentControlSet\Services\winebus";
const IDS: &[&str] = &["4098:BB3E", "4098:BC1E", "4098:B920", "4098:BC28"];
const INVALID: &str =
    "Die vorhandenen Winebus-Einstellungen sind unbekannt oder zu groß. Sie bleiben unverändert.";
#[derive(Default, Debug)]
struct Settings {
    ids: Vec<String>,
    disabled: bool,
}
fn valid_id(id: &str) -> bool {
    id.len() == 9
        && id.as_bytes()[4] == b':'
        && id
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || b.is_ascii_hexdigit())
}
fn decode(value: &str) -> Result<Vec<String>> {
    let text = if let Some(value) = value.strip_prefix("hex(7):") {
        let bytes = value
            .split(',')
            .map(|b| u8::from_str_radix(b.trim(), 16).map_err(|_| Error::Invalid(INVALID)))
            .collect::<Result<Vec<_>>>()?;
        require(
            bytes.len() <= 1024 && bytes.len().is_multiple_of(2) && bytes.ends_with(&[0, 0, 0, 0]),
            INVALID,
        )?;
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| Error::Invalid(INVALID))?
    } else if let Some(value) = value
        .strip_prefix("str(7):\"")
        .and_then(|v| v.strip_suffix('"'))
    {
        // Wine's native registry format omits one final NUL from the quoted string.
        require(
            value.len() <= 1024 && (value.is_empty() || value.ends_with(r"\0")),
            INVALID,
        )?;
        value.replace(r"\0", "\0")
    } else {
        return Err(Error::Invalid(INVALID));
    };
    let ids = text
        .trim_end_matches('\0')
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    require(
        ids.len() <= 48 && ids.iter().all(|id| valid_id(id)),
        INVALID,
    )?;
    Ok(ids)
}
fn settings(raw: &[u8]) -> Result<Settings> {
    require(raw.len() <= 64 * 1024 * 1024, INVALID)?;
    let text = game_package::text(raw)?;
    let mut logical = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(line) = line.strip_suffix('\\') {
            pending.push_str(line);
            continue;
        }
        pending.push_str(line);
        logical.push(std::mem::take(&mut pending));
    }
    require(pending.is_empty(), INVALID)?;
    let mut found = None;
    let mut section = String::new();
    let mut result = Settings::default();
    for line in logical {
        if let Some(value) = line.strip_prefix('[').and_then(|s| s.split(']').next()) {
            section = value.to_ascii_lowercase().replace("\\\\", "\\");
            section = section
                .strip_prefix("hkey_local_machine\\")
                .unwrap_or(&section)
                .to_owned();
        } else if let Some((name, value)) = line.split_once('=') {
            let name = name.to_ascii_lowercase();
            let main = [
                r"system\currentcontrolset\services\winebus",
                r"system\controlset001\services\winebus",
            ]
            .contains(&section.as_str());
            if main && name == "\"enablehidraw\"" {
                let ids = decode(value)?;
                require(
                    found.as_ref().is_none_or(|previous| previous == &ids),
                    INVALID,
                )?;
                found = Some(ids);
            } else if main && name == "\"disablehidraw\"" {
                let value = value
                    .strip_prefix("dword:")
                    .ok_or(Error::Invalid(INVALID))?;
                result.disabled |=
                    u32::from_str_radix(value, 16).map_err(|_| Error::Invalid(INVALID))? != 0;
            } else if section.contains(r"\services\winebus\devices\4098") && name == "\"hidraw\"" {
                // Respect vendor-wide or device-specific user overrides, even if
                // they affect a WinWing product outside our four tested IDs.
                result.disabled |= value.eq_ignore_ascii_case("dword:00000000");
            }
        }
    }
    result.ids = found.unwrap_or_default();
    Ok(result)
}
fn environment_disabled() -> bool {
    std::env::vars().any(|(key, value)| {
        key.eq_ignore_ascii_case("PROTON_DISABLE_HIDRAW")
            && (value == "1"
                || IDS.iter().any(|id| {
                    value.to_ascii_lowercase().contains(&format!(
                        "0x{}/0x{}",
                        id[..4].to_ascii_lowercase(),
                        id[5..].to_ascii_lowercase()
                    ))
                }))
    })
}
pub(super) fn supported(profile: &Profile) -> bool {
    profile.runner().is_ok_and(|r| {
        tx::digest(&r.join("files/lib/wine/x86_64-windows/winebus.sys")).is_ok_and(|h| h == HASH)
    })
}
pub(super) fn snapshot(profile: &Profile) -> Value {
    let supported = supported(profile);
    let state = files::read(&profile.prefix.join("system.reg"), 64 * 1024 * 1024)
        .and_then(|b| settings(&b));
    let configured = state.as_ref().is_ok_and(|s| {
        IDS.iter()
            .all(|id| s.ids.iter().any(|i| i.eq_ignore_ascii_case(id)))
    });
    let disabled = environment_disabled() || state.as_ref().is_ok_and(|s| s.disabled);
    json!({"state":if !supported {"unsupported_runner"} else if state.is_err() {"unavailable"} else if disabled {"disabled_by_user"} else if configured {"ready"} else {"required"},"supported":supported,"configured":configured,"disabled_by_user":disabled,"devices":IDS})
}
/// Merge the Windows export format while preserving every unrelated allowed device.
/// DisableHidraw and explicit device disable preferences are never removed.
pub fn merged_allowlist(raw: &[u8]) -> Result<Option<String>> {
    let settings = settings(raw)?;
    require(
        !settings.disabled,
        "HID-Zugriff ist in diesem Profil ausdrücklich deaktiviert. Flightdeck überschreibt diese Geräte-Einstellung nicht.",
    )?;
    let mut ids = settings.ids;
    let mut changed = false;
    for id in IDS {
        if !ids.iter().any(|i| i.eq_ignore_ascii_case(id)) {
            ids.push((*id).into());
            changed = true;
        }
    }
    require(ids.len() <= 48, INVALID)?;
    Ok(changed.then(|| ids.join(r"\0")))
}
pub(super) fn configure(ctx: &Context, profile: &Profile) -> Result<()> {
    require(
        supported(profile),
        "Der ausgewählte Runner unterstützt den geprüften WinWing-HID-Zugriff noch nicht. Runner und Geräte-Einstellungen bleiben unverändert.",
    )?;
    require(
        !environment_disabled(),
        "PROTON_DISABLE_HIDRAW deaktiviert WinWing ausdrücklich. Diese Auswahl bleibt erhalten.",
    )?;
    let wine = profile.wine(ctx, "mobiflight-hid-wine.log")?;
    let name = format!(
        ".flightdeck-mobiflight-winebus-{}.reg",
        uuid::Uuid::new_v4().simple()
    );
    let path = profile.prefix.join("drive_c").join(&name);
    let windows = format!("C:\\{name}");
    files::atomic(&path, b"")?;
    let result = (|| {
        wine.run(
            &["reg", "export", KEY, &windows, "/y"],
            None,
            &[0],
            Duration::from_secs(30),
        )?;
        let original = files::read(&path, 2 * 1024 * 1024)?;
        if let Some(merged) = merged_allowlist(&original)? {
            files::atomic(
                &profile
                    .root
                    .join("private")
                    .join(format!("mobiflight-winebus-{}.reg", ctx.id)),
                &original,
            )?;
            wine.reg(KEY, "EnableHidraw", &merged, "REG_MULTI_SZ")?;
            wine.run(
                &["reg", "export", KEY, &windows, "/y"],
                None,
                &[0],
                Duration::from_secs(30),
            )?;
            require(
                merged_allowlist(&files::read(&path, 2 * 1024 * 1024)?)?.is_none(),
                "Die WinWing-Geräteliste konnte nicht vollständig gespeichert werden. Die Registry-Sicherung bleibt erhalten.",
            )?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(path);
    // No interactive application is running: the caller holds the selected
    // profile's lease and idle check. Restart driver services so Winebus reads
    // the per-device list on the next application start.
    let stopped = wine.stop_staged();
    result.and(stopped)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn export(ids: &[&str]) -> Vec<u8> {
        let text = ids.join("\0") + "\0\0";
        let hex = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("Windows Registry Editor Version 5.00\r\n[HKEY_LOCAL_MACHINE\\System\\CurrentControlSet\\Services\\winebus]\r\n\"EnableHidraw\"=hex(7):{hex}\r\n\"Unrelated\"=dword:00000001\r\n").into_bytes()
    }
    #[test]
    fn merge_keeps_other_devices_and_is_idempotent() {
        let raw = export(&["1234:ABCD", "4098:bb3e"]);
        let merged = merged_allowlist(&raw).expect("merge").expect("changed");
        assert!(merged.starts_with(r"1234:ABCD\04098:bb3e\0"));
        assert!(
            merged_allowlist(&export(&[
                "1234:ABCD",
                "4098:BB3E",
                "4098:BC1E",
                "4098:B920",
                "4098:BC28"
            ]))
            .expect("unchanged")
            .is_none()
        );
        let mut disabled = export(&[]);
        disabled.extend_from_slice(b"\"DisableHidraw\"=dword:00000001\r\n");
        assert!(merged_allowlist(&disabled).is_err());
    }
    #[test]
    fn native_wine_format_and_explicit_device_overrides_are_respected() {
        let raw = br#"WINE REGISTRY Version 2
[System\\ControlSet001\\Services\\winebus]
"EnableHidraw"=str(7):"1234:ABCD\0"
[System\\ControlSet001\\Services\\winebus\\Devices\\4098/BB3E]
"Hidraw"=dword:00000000
"#;
        assert!(merged_allowlist(raw).is_err());
        assert!(merged_allowlist(b"[System\\\\ControlSet001\\\\Services\\\\winebus]\n\"EnableHidraw\"=dword:00000001\n").is_err());
    }
}
