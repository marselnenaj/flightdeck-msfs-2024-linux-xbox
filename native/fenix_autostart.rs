// SPDX-License-Identifier: MIT
//! Whether MSFS will start Fenix: its exe.xml entry, the bootstrapper that entry
//! names, and which Fenix processes a session actually started.
//!
//! MSFS starts FenixBootstrapper.exe from exe.xml; the bootstrapper starts the
//! Fenix companion when the aircraft loads. Setup records "configured" once,
//! so a later edit of exe.xml, a moved bootstrapper or an entry written before
//! the aircraft existed left the companion silently unstarted. Evidence holds
//! fixed categories, booleans and times only: no paths or log text.
use crate::{
    Error, Result,
    error::require,
    fenix_setup, files, wine_processes,
    xml::{self, Element, Item},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const FOLDER_2024: &str = "Microsoft Flight Simulator 2024";
pub const DEFAULT_PATH: &str = r"C:\Program Files\FenixSim A320\deps\FenixBootstrapper.exe";
const BOOTSTRAPPER: &str = "fenixbootstrapper.exe";
const FILE: &str = "fenix-autostart.json";
const STATES: &[&str] = &[
    "ready",
    "no_settings_folder",
    "unreadable",
    "missing",
    "disabled",
    "wrong_path",
    "bootstrapper_missing",
];

/// The one simulator settings folder below the profile's users.
pub fn settings_folder(prefix: &Path, folder: &str) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    for user in fenix_setup::user_folders(prefix)? {
        let path = user.join("AppData/Roaming").join(folder);
        if path.is_dir() {
            require(
                path.canonicalize()?.starts_with(prefix.canonicalize()?),
                "The simulator settings folder escapes the profile.",
            )?;
            candidates.push(path);
        }
    }
    require(
        candidates.len() == 1,
        "Could not identify one MSFS 2024 settings folder. Run the simulator once first.",
    )?;
    Ok(candidates.remove(0))
}

/// A real, non-symlinked entry of `dir` whose name matches case-insensitively,
/// as Windows programs inside Wine see it.
fn entry(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if fs::symlink_metadata(&exact).is_ok_and(|m| !m.is_symlink()) {
        return Some(exact);
    }
    fs::read_dir(dir)
        .ok()?
        .take(4096)
        .filter_map(|v| v.ok())
        .find(|v| {
            v.file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
                && v.file_type().is_ok_and(|t| !t.is_symlink())
        })
        .map(|v| v.path())
}

/// The installed bootstrapper: its usual place first, otherwise anywhere in
/// the Fenix program folder, as an aircraft update may move it.
pub fn bootstrapper(prefix: &Path) -> Option<PathBuf> {
    let program = Path::new(fenix_setup::PROGRAM)
        .components()
        .try_fold(prefix.to_path_buf(), |dir, part| {
            entry(&dir, part.as_os_str().to_str()?)
        })?;
    if let Some(found) = entry(&program, "deps").and_then(|deps| entry(&deps, BOOTSTRAPPER))
        && found.is_file()
    {
        return Some(found);
    }
    let mut queue = vec![(program, 0)];
    let mut seen = 0;
    while let Some((dir, depth)) = queue.pop() {
        let Ok(items) = fs::read_dir(&dir) else {
            continue;
        };
        for item in items.filter_map(|v| v.ok()) {
            seen += 1;
            if seen > 20000 {
                return None;
            }
            let Ok(kind) = item.file_type() else { continue };
            let name = item.file_name();
            if kind.is_file()
                && name
                    .to_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(BOOTSTRAPPER))
            {
                return Some(item.path());
            }
            if kind.is_dir() && depth < 3 {
                queue.push((item.path(), depth + 1));
            }
        }
    }
    None
}

/// `C:\…` for a file below the profile's drive_c.
pub fn windows_path(prefix: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(prefix.join("drive_c")).ok()?;
    let parts = relative
        .components()
        .map(|c| match c {
            std::path::Component::Normal(v) => v.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!(r"C:\{}", parts.join(r"\")))
}

/// The file a `C:\…` launch path names inside the profile, if it exists.
pub fn resolve(prefix: &Path, windows: &str) -> Option<PathBuf> {
    let text = windows.trim().trim_matches('"');
    let rest = text
        .get(..3)
        .filter(|v| v.eq_ignore_ascii_case(r"C:\") || v.eq_ignore_ascii_case("C:/"))
        .map(|_| &text[3..])?;
    let mut path = prefix.join("drive_c");
    for part in rest.split(['\\', '/']).filter(|v| !v.is_empty()) {
        require(part != "." && part != "..", "").ok()?;
        path = entry(&path, part)?;
    }
    path.is_file().then_some(path)
}

fn is_fenix(v: &Element) -> bool {
    v.named("Launch.Addon")
        && ["Name", "Path"]
            .iter()
            .filter_map(|n| v.child(n))
            .any(|v| v.text().to_lowercase().contains("fenix"))
}

fn yes(element: &Element, name: &str) -> bool {
    element
        .child(name)
        .is_some_and(|v| v.text().trim().eq_ignore_ascii_case("true"))
}

/// One of `STATES`, never an error.
pub fn status(prefix: &Path, folder: &str) -> &'static str {
    let Ok(settings) = settings_folder(prefix, folder) else {
        return "no_settings_folder";
    };
    let path = settings.join("exe.xml");
    if !files::exists(&path) {
        return "missing";
    }
    let Ok(root) = files::read(&path, 2 * 1024 * 1024).and_then(|v| xml::parse(&v)) else {
        return "unreadable";
    };
    let Some(entry) = root.items.iter().find_map(|v| match v {
        Item::Element(v) if is_fenix(v) => Some(v),
        _ => None,
    }) else {
        return "missing";
    };
    if yes(&root, "Disabled") || yes(entry, "Disabled") || yes(entry, "ManualLoad") {
        return "disabled";
    }
    let named = entry
        .child("Path")
        .and_then(|v| resolve(prefix, &v.text()))
        .is_some_and(|v| {
            v.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(BOOTSTRAPPER))
        });
    if named {
        "ready"
    } else if bootstrapper(prefix).is_some() {
        "wrong_path"
    } else {
        "bootstrapper_missing"
    }
}

/// Register the installed bootstrapper, or its usual path before the aircraft
/// is installed. Other launch entries and their order are kept.
pub fn configure(prefix: &Path, folder: &str) -> Result<()> {
    let path = settings_folder(prefix, folder)?.join("exe.xml");
    let mut root = if files::exists(&path) {
        xml::parse(&files::read(&path, 2 * 1024 * 1024)?)?
    } else {
        let mut root = Element::new("SimBase.Document");
        root.start.push_attribute(("Type", "Launch"));
        root.start.push_attribute(("version", "1,0"));
        for (key, value) in [
            ("Descr", "Launch"),
            ("Filename", "exe.xml"),
            ("Disabled", "False"),
        ] {
            root.set(key, value);
        }
        root
    };
    // A disabled document starts no add-on at all.
    if yes(&root, "Disabled") {
        root.set("Disabled", "False");
    }
    let mut found = false;
    root.items.retain(|v| {
        if let Item::Element(v) = v
            && is_fenix(v)
        {
            if found {
                return false;
            }
            found = true;
        }
        true
    });
    if !found {
        let mut node = Element::new("Launch.Addon");
        node.set("Name", "FenixA320");
        root.items.push(Item::Element(node));
    }
    let target = bootstrapper(prefix)
        .and_then(|v| windows_path(prefix, &v))
        .unwrap_or_else(|| DEFAULT_PATH.to_string());
    let entry = root
        .items
        .iter_mut()
        .find_map(|v| match v {
            Item::Element(v) if is_fenix(v) => Some(v),
            _ => None,
        })
        .ok_or(Error::Invalid("Invalid Fenix autostart entry."))?;
    for (key, value) in [
        ("Name", "FenixA320"),
        ("Disabled", "False"),
        ("ManualLoad", "False"),
        ("Path", target.as_str()),
    ] {
        entry.set(key, value);
    }
    files::atomic(&path, &root.bytes()?)?;
    Ok(())
}

fn record(private: &Path) -> Value {
    files::read(&private.join(FILE), 64 * 1024)
        .ok()
        .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
        .filter(|v| v["schema"] == 1)
        .unwrap_or_else(|| json!({"schema":1}))
}

fn save(private: &Path, value: &Value) {
    if files::private_dir(private).is_ok() {
        let _ = files::atomic_json(&private.join(FILE), value);
    }
}

/// Check the entry before a start and repair what Flightdeck itself wrote.
/// A missing bootstrapper or settings folder is reported, not invented.
pub fn ensure(prefix: &Path, folder: &str, private: &Path) -> Value {
    let before = status(prefix, folder);
    let repairable = ["missing", "disabled", "wrong_path"].contains(&before);
    let mut failed = false;
    if repairable && let Err(error) = configure(prefix, folder) {
        eprintln!("Fenix autostart repair failed: {error}");
        failed = true;
    }
    let after = if repairable {
        status(prefix, folder)
    } else {
        before
    };
    let mut value = record(private);
    value["checked_at"] = json!(files::now());
    value["before"] = json!(before);
    value["after"] = json!(after);
    value["repaired"] = json!(repairable && !failed && after == "ready");
    value["session"] = Value::Null;
    save(private, &value);
    value
}

/// The last check and session, restricted to the fixed vocabulary above.
pub fn load(private: &Path) -> Value {
    let value = record(private);
    if value.get("checked_at").is_none() {
        return Value::Null;
    }
    let state = |v: &Value| {
        v.as_str()
            .filter(|v| STATES.contains(v))
            .map_or(Value::Null, |v| json!(v))
    };
    let time = |v: &Value| {
        v.as_str()
            .filter(|v| chrono::DateTime::parse_from_rfc3339(v).is_ok())
            .map_or(Value::Null, |v| json!(v))
    };
    let session = &value["session"];
    json!({
        "checked_at": time(&value["checked_at"]),
        "before": state(&value["before"]),
        "after": state(&value["after"]),
        "repaired": value["repaired"] == true,
        "session": if session.is_object() {
            json!({
                "started_at": time(&session["started_at"]),
                "ended_at": time(&session["ended_at"]),
                "bootstrapper_seen": session["bootstrapper_seen"] == true,
                "bootstrapper_after_s": session["bootstrapper_after_s"].as_u64(),
                "fenix_seen": session["fenix_seen"] == true,
                "fenix_after_s": session["fenix_after_s"].as_u64(),
            })
        } else {
            Value::Null
        },
    })
}

/// Before an Xbox runtime session with a configured Fenix: check and repair
/// the entry, then watch which Fenix processes the session starts.
pub fn runtime_session(root: &Path) -> Option<Watch> {
    let state = crate::runtime::value(&root.join(crate::fenix::MARKER)).ok()?;
    if state["state"] != "installed" || state["configured"] != true {
        return None;
    }
    let prefix = root.join("local/msfs-prefix");
    let private = root.join("private");
    ensure(&prefix, FOLDER_2024, &private);
    Some(Watch::new(&prefix, &private))
}

/// Samples the profile while the simulator runs. The bootstrapper waits for
/// the aircraft, so a short-lived one is the rejection case worth seeing.
pub struct Watch {
    prefix: PathBuf,
    private: PathBuf,
    started: Instant,
    last: Option<Instant>,
    bootstrapper: Option<u64>,
    fenix: Option<u64>,
}
impl Watch {
    pub fn new(prefix: &Path, private: &Path) -> Self {
        let watch = Self {
            prefix: prefix.to_path_buf(),
            private: private.to_path_buf(),
            started: Instant::now(),
            last: None,
            bootstrapper: None,
            fenix: None,
        };
        let mut value = record(private);
        value["session"] =
            json!({"started_at":files::now(),"bootstrapper_seen":false,"fenix_seen":false});
        save(private, &value);
        watch
    }
    pub fn poll(&mut self) {
        if self
            .last
            .is_some_and(|v| v.elapsed() < Duration::from_secs(2))
            || self.bootstrapper.is_some() && self.fenix.is_some()
        {
            return;
        }
        self.last = Some(Instant::now());
        let Ok(processes) = wine_processes::Processes::new(&self.prefix) else {
            return;
        };
        let after = self.started.elapsed().as_secs();
        let mut changed = false;
        if self.bootstrapper.is_none() && processes.any(Some(&[BOOTSTRAPPER])) {
            self.bootstrapper = Some(after);
            changed = true;
        }
        if self.fenix.is_none() && processes.any(Some(&["fenixsystem.exe", "fenix.exe"])) {
            self.fenix = Some(after);
            changed = true;
        }
        if changed {
            self.write(false);
        }
    }
    fn write(&self, ended: bool) {
        let mut value = record(&self.private);
        if !value["session"].is_object() {
            value["session"] = json!({});
        }
        let session = &mut value["session"];
        session["bootstrapper_seen"] = json!(self.bootstrapper.is_some());
        session["bootstrapper_after_s"] = json!(self.bootstrapper);
        session["fenix_seen"] = json!(self.fenix.is_some());
        session["fenix_after_s"] = json!(self.fenix);
        if ended {
            session["ended_at"] = json!(files::now());
        }
        save(&self.private, &value);
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.write(true);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn put(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn profile() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix");
        let settings = prefix
            .join("drive_c/users/pilot/AppData/Roaming")
            .join(FOLDER_2024);
        fs::create_dir_all(&settings).unwrap();
        (temp, prefix, settings.join("exe.xml"))
    }

    #[test]
    fn reports_each_reason_the_companion_would_not_start() {
        let (_temp, prefix, exe) = profile();
        assert_eq!(status(&prefix, FOLDER_2024), "missing");
        put(&exe, b"<SimBase.Document><Launch.Addon><Name>Other</Name><Path>C:\\x.exe</Path></Launch.Addon></SimBase.Document>");
        assert_eq!(status(&prefix, FOLDER_2024), "missing");
        put(&exe, b"<SimBase.Document><Launch.Addon><Name>FenixA320</Name><Path>C:\\Program Files\\FenixSim A320\\deps\\FenixBootstrapper.exe</Path></Launch.Addon></SimBase.Document>");
        assert_eq!(status(&prefix, FOLDER_2024), "bootstrapper_missing");
        put(
            &prefix.join("drive_c/Program Files/FenixSim A320/bin/FenixBootstrapper.exe"),
            b"MZ",
        );
        assert_eq!(status(&prefix, FOLDER_2024), "wrong_path");
        put(&exe, b"<SimBase.Document><Launch.Addon><Name>FenixA320</Name><Disabled>True</Disabled><Path>C:\\Program Files\\FenixSim A320\\bin\\FenixBootstrapper.exe</Path></Launch.Addon></SimBase.Document>");
        assert_eq!(status(&prefix, FOLDER_2024), "disabled");
        put(&exe, b"<SimBase.Document><Disabled>True</Disabled><Launch.Addon><Name>FenixA320</Name><Path>C:\\Program Files\\FenixSim A320\\bin\\FenixBootstrapper.exe</Path></Launch.Addon></SimBase.Document>");
        assert_eq!(status(&prefix, FOLDER_2024), "disabled");
        put(&exe, b"<SimBase.Document><Launch.Addon><Name>FenixA320</Name><Path>c:\\program files\\fenixsim a320\\BIN\\fenixbootstrapper.EXE</Path></Launch.Addon></SimBase.Document>");
        assert_eq!(status(&prefix, FOLDER_2024), "ready");
        put(&exe, b"<SimBase.Document><Launch.Addon>");
        assert_eq!(status(&prefix, FOLDER_2024), "unreadable");
        fs::remove_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(status(&prefix, FOLDER_2024), "no_settings_folder");
    }

    #[test]
    fn repair_points_at_the_installed_bootstrapper_and_keeps_other_addons() {
        let (temp, prefix, exe) = profile();
        let private = temp.path().join("private");
        put(&exe, b"<SimBase.Document><Disabled>True</Disabled><Launch.Addon><Name>Keep Me</Name><Path>C:\\Other.exe</Path></Launch.Addon><Launch.Addon><Name>FenixA320</Name><ManualLoad>True</ManualLoad><Path>C:\\Program Files\\FenixSim A320\\deps\\FenixBootstrapper.exe</Path></Launch.Addon></SimBase.Document>");
        put(
            &prefix.join("drive_c/Program Files/FenixSim A320/app/FenixBootstrapper.exe"),
            b"MZ",
        );
        let checked = ensure(&prefix, FOLDER_2024, &private);
        assert_eq!(checked["before"], "disabled");
        assert_eq!(checked["after"], "ready");
        assert_eq!(checked["repaired"], true);
        let text = fs::read_to_string(&exe).unwrap();
        assert!(text.contains("Keep Me"));
        assert!(text.contains(r"C:\Program Files\FenixSim A320\app\FenixBootstrapper.exe"));
        assert!(!text.contains("True"));
        // The usual location wins once it exists.
        put(
            &prefix.join("drive_c/Program Files/FenixSim A320/deps/FenixBootstrapper.exe"),
            b"MZ",
        );
        configure(&prefix, FOLDER_2024).unwrap();
        assert!(fs::read_to_string(&exe).unwrap().contains(DEFAULT_PATH));
        let loaded = load(&private);
        assert_eq!(loaded["before"], "disabled");
        assert_eq!(loaded["repaired"], true);
        assert!(loaded["session"].is_null());
    }

    #[test]
    fn missing_aircraft_is_reported_and_left_alone() {
        let (temp, prefix, exe) = profile();
        let private = temp.path().join("private");
        put(&exe, b"<SimBase.Document><Launch.Addon><Name>FenixA320</Name><Path>C:\\Program Files\\FenixSim A320\\deps\\FenixBootstrapper.exe</Path></Launch.Addon></SimBase.Document>");
        let before = fs::read(&exe).unwrap();
        let checked = ensure(&prefix, FOLDER_2024, &private);
        assert_eq!(checked["after"], "bootstrapper_missing");
        assert_eq!(checked["repaired"], false);
        assert_eq!(fs::read(&exe).unwrap(), before);
    }

    #[test]
    fn session_evidence_is_restricted_to_fixed_values() {
        let temp = tempfile::tempdir().unwrap();
        let private = temp.path().join("private");
        assert!(load(&private).is_null());
        put(
            &private.join(FILE),
            br#"{"schema":1,"checked_at":"2026-11-15T10:00:00+00:00","before":"C:\\secret","after":"ready","repaired":true,"session":{"started_at":"nope","bootstrapper_seen":true,"bootstrapper_after_s":12,"fenix_seen":false,"extra":"x"}}"#,
        );
        let loaded = load(&private);
        assert!(loaded["before"].is_null());
        assert_eq!(loaded["after"], "ready");
        assert!(loaded["session"]["started_at"].is_null());
        assert_eq!(loaded["session"]["bootstrapper_after_s"], 12);
        assert!(loaded["session"].get("extra").is_none());
        let prefix = temp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        drop(Watch::new(&prefix, &private));
        let loaded = load(&private);
        assert_eq!(loaded["session"]["bootstrapper_seen"], false);
        assert!(loaded["session"]["ended_at"].is_string());
    }

    #[test]
    fn windows_paths_round_trip() {
        let prefix = Path::new("/p");
        assert_eq!(
            windows_path(prefix, Path::new("/p/drive_c/Program Files/A/b.exe")).as_deref(),
            Some(r"C:\Program Files\A\b.exe")
        );
        assert!(windows_path(prefix, Path::new("/elsewhere/b.exe")).is_none());
        assert!(resolve(prefix, r"D:\x.exe").is_none());
        assert!(resolve(prefix, r"C:\..\..\etc\passwd").is_none());
    }
}
