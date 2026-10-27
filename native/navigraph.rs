// SPDX-License-Identifier: MIT
//! Official Navigraph Hub and Simlink inside the selected simulator profile.
//! Sign-in, subscriptions and navdata stay with Navigraph's own applications.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    error::require,
    fenix, fenix_setup, files,
    games::Game,
    gsx, installer, mods, process, proton, runtime,
    wine::Wine,
    wine_processes,
    xml::{self, Element, Item},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub const STARTUP: &str = "private/navigraph-startup.json";
const HUB_DIR: &str = "AppData/Local/Programs/navigraph-hub";
const HUB_EXE: &str = "Navigraph Hub.exe";
// Hub 1.2.x wraps 7-Zip in Process Governor, whose job-object freeze call
// fails in Wine (Win32Exception 87). Without the file, Hub runs 7za directly.
const GOVERNOR: &str = "resources/assets/bin/procgov.exe";
const GOVERNOR_DISABLED: &str = "resources/assets/bin/procgov.exe.flightdeck-disabled";
const SIMLINK_DIRS: &[&str] = &[
    "drive_c/Program Files/Navigraph/Simlink",
    "drive_c/Program Files (x86)/Navigraph/Simlink",
];
const SIMLINK_EXE: &str = "NavigraphSimlink.exe";
/// Simlink's QSettings file, shared with the Charts desktop app.
const SIMLINK_SETTINGS: &str = "AppData/Roaming/Navigraph/Charts/.settings";
pub const HUB: &[&str] = &["navigraph hub.exe"];
pub const SIMLINK: &[&str] = &["navigraphsimlink.exe"];
/// Browser sign-in returns through these private URI schemes.
pub const SCHEMES: &[(&str, App)] = &[
    ("navigraph-desktop", App::Hub),
    ("navigraph-traffic-desktop", App::Simlink),
];
const HANDLER: &str = "flightdeck-navigraph.desktop";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Hub,
    Simlink,
}
impl App {
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Hub => HUB,
            Self::Simlink => SIMLINK,
        }
    }
    fn executable(self, prefix: &Path) -> Result<Option<PathBuf>> {
        match self {
            Self::Hub => hub(prefix).map(|v| v.map(|d| d.join(HUB_EXE))),
            Self::Simlink => simlink(prefix),
        }
    }
}
fn contained(path: &Path, prefix: &Path) -> Result<()> {
    require(
        path.canonicalize()?.starts_with(prefix.canonicalize()?),
        "Der Navigraph-Ordner muss innerhalb des gewählten Windows-Profils liegen.",
    )
}
fn executable_file(path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    require(
        files::open_at(rustix::fs::CWD, path, false, false)?
            .metadata()?
            .len()
            <= 512 * 1024 * 1024,
        "Ungültige Navigraph-Programmdatei.",
    )?;
    Ok(true)
}
/// The per-user Hub installation folder, if exactly one Windows user has it.
pub fn hub(prefix: &Path) -> Result<Option<PathBuf>> {
    if !prefix.join("drive_c/users").is_dir() {
        return Ok(None);
    }
    let mut found = Vec::new();
    for user in fenix_setup::user_folders(prefix)? {
        let directory = user.join(HUB_DIR);
        if executable_file(&directory.join(HUB_EXE))? {
            contained(&directory, prefix)?;
            found.push(directory);
        }
    }
    require(
        found.len() <= 1,
        "Navigraph Hub ist für mehrere Windows-Benutzer installiert.",
    )?;
    Ok(found.pop())
}
pub fn simlink(prefix: &Path) -> Result<Option<PathBuf>> {
    for directory in SIMLINK_DIRS {
        let path = prefix.join(directory).join(SIMLINK_EXE);
        if executable_file(&path)? {
            contained(&path, prefix)?;
            return Ok(Some(path));
        }
    }
    Ok(None)
}
fn simlink_settings(prefix: &Path) -> Result<Vec<PathBuf>> {
    if !prefix.join("drive_c/users").is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for user in fenix_setup::user_folders(prefix)? {
        let path = user.join(SIMLINK_SETTINGS);
        if path.is_file() {
            contained(&path, prefix)?;
            found.push(path);
        }
    }
    Ok(found)
}
fn ini_value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == format!("[{section}]");
        } else if inside
            && let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim());
        }
    }
    None
}
/// Whether Simlink holds a Navigraph sign-in. Only the presence of a refresh
/// token is checked; no token leaves the file.
pub fn simlink_signed_in(prefix: &Path) -> Result<bool> {
    for path in simlink_settings(prefix)? {
        let raw = files::read(&path, 1024 * 1024)?;
        if ini_value(&String::from_utf8_lossy(&raw), "user", "refresh_token")
            .is_some_and(|v| !v.is_empty())
        {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Simlink's settings with its own Windows login start turned off, if it was on.
fn without_login_start(text: &str) -> Option<String> {
    let mut inside = false;
    let mut changed = false;
    let mut lines = Vec::new();
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[simlink]";
        } else if inside
            && let Some((name, value)) = trimmed.split_once('=')
            && name.trim() == "autostart_app"
            && value.trim() != "0"
        {
            let ending = &line[line.trim_end().len()..];
            lines.push(format!("autostart_app=0{ending}"));
            changed = true;
            continue;
        }
        lines.push(line.to_string());
    }
    changed.then(|| lines.concat())
}
/// Simlink writes itself back into Windows login startup on every start while
/// its own setting is on; MSFS starts it through exe.xml instead.
pub fn simlink_without_login_start(prefix: &Path) -> Result<()> {
    for path in simlink_settings(prefix)? {
        let raw = files::read(&path, 1024 * 1024)?;
        if let Some(text) = without_login_start(&String::from_utf8_lossy(&raw)) {
            files::atomic(&path, text.as_bytes())?;
        }
    }
    Ok(())
}
/// Move Process Governor aside. Hub updates restore it, so this runs before
/// every start; a previously disabled copy is replaced by the newer file.
pub fn disable_governor(directory: &Path) -> Result<bool> {
    let active = directory.join(GOVERNOR);
    let disabled = directory.join(GOVERNOR_DISABLED);
    if files::exists(&active) {
        require(
            !active.symlink_metadata()?.is_symlink(),
            "Ungültige Navigraph-Hilfsdatei.",
        )?;
        fs::rename(&active, &disabled)?;
        files::directory(
            active
                .parent()
                .ok_or(Error::Invalid("Ungültige Navigraph-Hilfsdatei."))?,
            false,
        )?
        .sync_all()?;
        return Ok(true);
    }
    Ok(false)
}
/// Make closing Hub's window quit it and keep it out of Windows login startup.
/// Hub otherwise keeps running in the background, which Flightdeck's setup
/// step and the simulator launch would wait for.
pub fn prefer_foreground(directory: &Path) -> Result<()> {
    let user = directory
        .ancestors()
        .nth(4)
        .ok_or(Error::Invalid("Ungültiger Navigraph-Programmpfad."))?;
    let folder = user.join("AppData/Roaming/navigraph-hub");
    let path = folder.join("config.json");
    let mut config = if files::exists(&path) {
        files::json::<Value>(&path, 4 * 1024 * 1024)?
    } else {
        json!({})
    };
    let settings = config
        .as_object_mut()
        .ok_or(Error::Invalid("Ungültige Navigraph-Hub-Einstellungen."))?;
    if settings.get("runInBackground") == Some(&json!(false))
        && settings.get("autoLaunch") == Some(&json!(false))
    {
        return Ok(());
    }
    settings.insert("runInBackground".into(), json!(false));
    settings.insert("autoLaunch".into(), json!(false));
    fs::create_dir_all(&folder)?;
    contained(&folder, user)?;
    files::atomic_json(&path, &config)
}
fn governor_active(directory: &Path) -> bool {
    files::exists(&directory.join(GOVERNOR))
}
fn windows_path(path: &Path, prefix: &Path) -> Result<String> {
    let relative = path
        .canonicalize()?
        .strip_prefix(prefix.join("drive_c").canonicalize()?)
        .map_err(|_| Error::Invalid("Ungültiger Navigraph-Programmpfad."))?
        .to_string_lossy()
        .replace('/', "\\");
    Ok(format!("C:\\{relative}"))
}
/// The edition's settings folder that holds the active UserCfg.opt. Hub uses
/// the same file to detect MSFS 2020 and 2024 and their package location.
pub fn settings_folder(root: &Path) -> Result<PathBuf> {
    let prefix = root.join("local/msfs-prefix");
    let game = Game::for_runtime(root)?;
    let family = mods::family(root)?;
    let mut found = Vec::new();
    for user in fenix_setup::user_folders(&prefix)? {
        let mut folders = vec![user.join("AppData/Roaming").join(game.user_config())];
        if let Some(family) = &family {
            folders.push(
                user.join("AppData/Local/Packages")
                    .join(family)
                    .join("LocalCache"),
            );
        }
        for folder in folders {
            if files::exists(&folder.join("UserCfg.opt")) {
                contained(&folder, &prefix)?;
                found.push(folder);
            }
        }
    }
    require(
        found.len() == 1,
        "Starte MSFS einmal, damit die Simulator-Einstellungen angelegt werden.",
    )?;
    Ok(found.remove(0))
}
fn is_simlink(element: &Element) -> bool {
    element.named("Launch.Addon")
        && element.child("Path").is_some_and(|v| {
            v.text()
                .trim()
                .trim_matches('"')
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .is_some_and(|name| SIMLINK.contains(&name.to_lowercase().as_str()))
        })
}
fn enabled(element: &Element) -> bool {
    element
        .child("Disabled")
        .is_none_or(|v| v.text().trim().to_lowercase() != "true")
}
/// Whether exe.xml starts Simlink with the simulator.
fn startup_entry(folder: &Path) -> Result<bool> {
    let path = folder.join("exe.xml");
    if !files::exists(&path) {
        return Ok(false);
    }
    let tree = xml::parse(&files::read(&path, 2 * 1024 * 1024)?)?;
    Ok(enabled(&tree)
        && tree
            .items
            .iter()
            .any(|v| matches!(v, Item::Element(v) if is_simlink(v) && enabled(v))))
}
/// Add or update the single Simlink entry; other add-ons stay unchanged.
pub fn startup_document(raw: Option<&[u8]>, executable: &str, enable: bool) -> Result<Vec<u8>> {
    let mut tree = if let Some(raw) = raw {
        xml::parse(raw)?
    } else {
        let mut tree = Element::new("SimBase.Document");
        tree.start.push_attribute(("Type", "Launch"));
        tree.start.push_attribute(("version", "1,0"));
        for (key, value) in [
            ("Descr", "Launch"),
            ("Filename", "exe.xml"),
            ("Disabled", "False"),
        ] {
            tree.set(key, value);
        }
        tree
    };
    require(
        !enable || enabled(&tree),
        "Der automatische Add-on-Start ist in exe.xml ausgeschaltet.",
    )?;
    let mut found = false;
    tree.items.retain(|v| {
        if let Item::Element(v) = v
            && is_simlink(v)
        {
            if found {
                return false;
            }
            found = true;
        }
        true
    });
    if !found {
        if !enable {
            return tree.bytes();
        }
        let mut node = Element::new("Launch.Addon");
        node.set("Path", executable);
        tree.items.push(Item::Element(node));
    }
    let entry = tree
        .items
        .iter_mut()
        .find_map(|v| match v {
            Item::Element(v) if is_simlink(v) => Some(v),
            _ => None,
        })
        .ok_or(Error::Invalid("Ungültiger Simlink-Starteintrag."))?;
    for (key, value) in [
        ("Name", "Navigraph Simlink"),
        ("Disabled", if enable { "False" } else { "True" }),
        ("ManualLoad", "False"),
        ("Path", executable),
    ] {
        entry.set(key, value);
    }
    tree.bytes()
}
pub fn configure(root: &Path, enable: bool) -> Result<()> {
    let prefix = root.join("local/msfs-prefix");
    let executable = simlink(&prefix)?.ok_or(Error::Invalid(
        "Installiere zuerst Navigraph Simlink in diesem Profil.",
    ))?;
    let folder = settings_folder(root)?;
    let path = folder.join("exe.xml");
    let raw = if files::exists(&path) {
        let raw = files::read(&path, 2 * 1024 * 1024)?;
        files::atomic(
            &root.join("private").join(format!(
                "navigraph-exe-{}.xml",
                uuid::Uuid::new_v4().simple()
            )),
            &raw,
        )?;
        Some(raw)
    } else {
        None
    };
    let bytes = startup_document(raw.as_deref(), &windows_path(&executable, &prefix)?, enable)?;
    files::atomic(&path, &bytes)?;
    files::atomic_json(&root.join(STARTUP), &json!({"format":1,"enabled":enable}))
}
pub fn startup_enabled(root: &Path) -> bool {
    runtime::value(&root.join(STARTUP))
        .is_ok_and(|v| v["format"].as_u64() == Some(1) && v["enabled"] == true)
}
/// Navigraph packages in the one configured Community folder.
pub fn packages(root: &Path) -> Result<Vec<String>> {
    let (locations, limited) = mods::locations(root)?;
    if limited {
        return Ok(Vec::new());
    }
    let paths: BTreeSet<_> = locations
        .into_iter()
        .map(|(p, _)| p.canonicalize())
        .filter_map(std::io::Result::ok)
        .collect();
    let Some(community) = paths.iter().next().filter(|_| paths.len() == 1) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for (n, entry) in fs::read_dir(community)?.enumerate() {
        if n >= 8192 {
            break;
        }
        let name = entry?.file_name();
        if let Some(name) = name.to_str()
            && name.to_lowercase().starts_with("navigraph-")
        {
            found.push(name.to_string());
        }
    }
    found.sort();
    found.truncate(32);
    Ok(found)
}
fn applications_dir() -> PathBuf {
    files::xdg("XDG_DATA_HOME", ".local/share").join("applications")
}
fn launcher_path() -> Result<PathBuf> {
    let installed = files::home().join(".local/bin/flightdeck");
    let current = std::env::current_exe()?;
    let wrapper = fs::read_to_string(&installed).ok();
    Ok(if managed_release(&current, wrapper.as_deref()) {
        installed
    } else {
        current
    })
}
/// A managed release moves on every update, so links go through the stable
/// wrapper that selects it. Any other build, such as one under test, handles
/// its own links.
fn managed_release(current: &Path, wrapper: Option<&str>) -> bool {
    let mut ancestors = current.ancestors();
    let release = ancestors.nth(3).is_some_and(|p| p.ends_with("releases"));
    release
        && ancestors.next().zip(wrapper).is_some_and(|(root, script)| {
            script.contains(&format!("--managed-root '{}'", root.display()))
        })
}
pub fn handler_bytes(launcher: &Path, state_dir: &Path) -> Result<Vec<u8>> {
    let mime: String = SCHEMES
        .iter()
        .map(|(scheme, _)| format!("x-scheme-handler/{scheme};"))
        .collect();
    Ok(format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Flightdeck Navigraph sign-in\nExec=\"{}\" --state-dir \"{}\" navigraph-url %u\nTerminal=false\nNoDisplay=true\nMimeType={mime}\nStartupNotify=false\n",
        installer::exec_argument(launcher)?,
        installer::exec_argument(state_dir)?,
    )
    .into_bytes())
}
/// Default handlers recorded in mimeapps.list, as written by xdg-mime.
pub fn handler_registered(state_dir: &Path) -> bool {
    let desktop = applications_dir().join(HANDLER);
    let expected = launcher_path().and_then(|l| handler_bytes(&l, state_dir));
    if !expected.is_ok_and(|v| files::read(&desktop, 64 * 1024).is_ok_and(|raw| raw == v)) {
        return false;
    }
    let list = files::xdg("XDG_CONFIG_HOME", ".config").join("mimeapps.list");
    let Ok(raw) = files::read(&list, 1024 * 1024) else {
        return false;
    };
    let text = String::from_utf8_lossy(&raw);
    let mut defaults = false;
    let mut found = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            defaults = line == "[Default Applications]";
        } else if defaults
            && let Some((key, value)) = line.split_once('=')
            && value.split(';').next().map(str::trim) == Some(HANDLER)
        {
            found.insert(key.trim().to_string());
        }
    }
    SCHEMES
        .iter()
        .all(|(scheme, _)| found.contains(&format!("x-scheme-handler/{scheme}")))
}
/// Return browser sign-ins to the Windows applications in Flightdeck's profiles.
pub fn register_handler(state_dir: &Path) -> Result<()> {
    let program = process::which("xdg-mime").ok_or(Error::Invalid(
        "xdg-mime fehlt. Installiere xdg-utils, damit die Navigraph-Anmeldung zurückkehren kann.",
    ))?;
    let directory = applications_dir();
    fs::create_dir_all(&directory)?;
    files::atomic(
        &directory.join(HANDLER),
        &handler_bytes(&launcher_path()?, state_dir)?,
    )?;
    let mut command = Command::new(program);
    command.args(["default", HANDLER]);
    for (scheme, _) in SCHEMES {
        command.arg(format!("x-scheme-handler/{scheme}"));
    }
    require(
        process::run(
            &mut command,
            Duration::from_secs(30),
            &AtomicBool::new(false),
        )?
        .success(),
        "Die Navigraph-Anmeldung konnte nicht als Link-Handler eingetragen werden.",
    )?;
    if let Some(update) = process::which("update-desktop-database") {
        let _ = process::run(
            Command::new(update).arg(&directory),
            Duration::from_secs(30),
            &AtomicBool::new(false),
        );
    }
    require(
        handler_registered(state_dir),
        "Die Navigraph-Anmeldung konnte nicht als Link-Handler eingetragen werden.",
    )
}
pub fn callback_app(url: &str) -> Result<App> {
    require(
        url.len() <= 16 * 1024 && !url.chars().any(char::is_control),
        "Ungültiger Navigraph-Link.",
    )?;
    let (scheme, rest) = url
        .split_once("://")
        .ok_or(Error::Invalid("Ungültiger Navigraph-Link."))?;
    require(!rest.is_empty(), "Ungültiger Navigraph-Link.")?;
    SCHEMES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(scheme))
        .map(|(_, app)| *app)
        .ok_or(Error::Invalid("Ungültiger Navigraph-Link."))
}
fn known_runtimes(state_dir: &Path) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    if let Ok(value) = runtime::value(&state_dir.join("config.json")) {
        let paths = value["runtimes"]
            .as_object()
            .into_iter()
            .flat_map(|v| v.values())
            .chain(std::iter::once(&value["runtime_path"]));
        for path in paths.filter_map(Value::as_str) {
            if let Ok(path) = runtime::validate(path) {
                found.insert(path);
            }
        }
    }
    found.into_iter().collect()
}
/// Hand a sign-in link to the waiting application. Electron and Simlink pass
/// the second instance's argument to the running one in the same profile.
pub fn forward(state_dir: &Path, url: &str) -> Result<()> {
    let app = callback_app(url)?;
    let mut running = Vec::new();
    let mut installed = Vec::new();
    for root in known_runtimes(state_dir) {
        let prefix = root.join("local/msfs-prefix");
        let Ok(Some(executable)) = app.executable(&prefix) else {
            continue;
        };
        if wine_processes::Processes::new(&prefix).is_ok_and(|p| p.any(Some(app.names()))) {
            running.push((root.clone(), executable.clone()));
        }
        installed.push((root, executable));
    }
    let mut candidates = if running.is_empty() {
        installed
    } else {
        running
    };
    require(
        candidates.len() == 1,
        if candidates.is_empty() {
            "Die Navigraph-Anwendung wurde in keiner Flightdeck-Installation gefunden."
        } else {
            "Die Navigraph-Anwendung läuft in mehreren Installationen. Schließe die nicht benötigte."
        },
    )?;
    let (root, executable) = candidates.remove(0);
    proton::check(&root)?;
    let cancel = AtomicBool::new(false);
    let mut wine = Wine::new(
        &root.join("local/msfs-prefix"),
        &root.join("runner"),
        &root.join("private/navigraph-sign-in.log"),
        &cancel,
    )?;
    wine.without_fenix();
    let mut command = wine.command()?;
    command
        .arg(&executable)
        .arg(url)
        .current_dir(executable.parent().unwrap_or(&root));
    let mut child = process::spawn(&mut command, None)?;
    // A forwarding instance exits quickly; a freshly started one keeps running.
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if child.try_wait()?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}
fn validate(root: &Path) -> Result<()> {
    proton::check(root)?;
    require(
        gsx::complete(root),
        "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.",
    )?;
    if files::exists(&root.join(fenix::MARKER)) {
        require(
            runtime::value(&root.join(fenix::MARKER))?["state"] == "installed",
            "Fenix setup is incomplete. Restore or finish it in Flightdeck before starting MSFS.",
        )?;
    }
    let prefix = root.join("local/msfs-prefix");
    files::directory(&prefix.join("drive_c"), false)?;
    require(
        files::exists(&prefix.join("system.reg")),
        "Starte MSFS einmal, damit das Windows-Profil angelegt wird.",
    )
}
pub fn snapshot(app: &Launcher) -> Value {
    let (root, busy, playing, closing, job) = {
        let mut s = app.lock();
        Launcher::poll(&mut s);
        (
            s.runtime.clone(),
            s.active.is_some() || Launcher::external(&s),
            s.process.is_some(),
            s.closing,
            s.jobs
                .get("navigraph")
                .cloned()
                .filter(|j| {
                    j["runtime_path"].as_str() == s.runtime.as_ref().and_then(|p| p.to_str())
                })
                .unwrap_or(Value::Null),
        )
    };
    let mut value = json!({"state":"unavailable","message":"","hub_installed":false,"governor_disabled":false,"simlink_installed":false,"simlink_signed_in":false,"settings_found":false,"startup":false,"handler":false,"packages":[],"hub_running":false,"simlink_running":false,"idle":false,"runtime_path":root,"job":job,"busy":busy||playing,"can_change":false,"can_stop":false});
    if let Some(root) = root {
        let result = (|| -> Result<()> {
            validate(&root)?;
            let prefix = root.join("local/msfs-prefix");
            let directory = hub(&prefix)?;
            value["state"] = json!("available");
            value["game"] = json!(Game::for_runtime(&root)?.id());
            value["hub_installed"] = json!(directory.is_some());
            value["governor_disabled"] =
                json!(directory.as_deref().is_some_and(|d| !governor_active(d)));
            value["simlink_installed"] = json!(simlink(&prefix)?.is_some());
            value["simlink_signed_in"] = json!(simlink_signed_in(&prefix)?);
            let folder = settings_folder(&root);
            value["settings_found"] = json!(folder.is_ok());
            value["startup"] = json!(
                startup_enabled(&root) && folder.is_ok_and(|f| startup_entry(&f).unwrap_or(false))
            );
            value["handler"] = json!(handler_registered(&app.state_dir));
            value["packages"] = json!(packages(&root).unwrap_or_default());
            value["message"] = json!(
                "Anmeldung, Abonnement und Navdata prüft Navigraph. Navigraph unterstützt Linux offiziell nicht."
            );
            let processes = wine_processes::Processes::new(&prefix)?;
            let hub_running = processes.any(Some(HUB));
            let simlink_running = processes.any(Some(SIMLINK));
            let game = processes.any(Some(wine_processes::GAMES));
            let idle = wine_processes::idle(&prefix).is_ok();
            value["hub_running"] = json!(hub_running);
            value["simlink_running"] = json!(simlink_running);
            value["idle"] = json!(idle);
            value["can_change"] = json!(idle && !busy && !playing && !closing);
            let interactive = job["state"] == "running"
                && ["open-hub", "open-simlink"].contains(&job["operation"].as_str().unwrap_or(""))
                && job["stopping"] != true;
            value["can_stop"] = json!(
                (hub_running || simlink_running)
                    && !playing
                    && !game
                    && !closing
                    && (interactive || !busy)
            );
            Ok(())
        })();
        if let Err(error) = result {
            value["message"] = json!(error.to_string());
        }
    }
    value
}
/// Delete Hub's Windows login autostart: in Wine it would start Hub with the
/// simulator's profile and keep package extraction running beside MSFS.
fn remove_run_entries(wine: &Wine, prefix: &Path) -> Result<()> {
    let path = prefix.join("user.reg");
    if !files::exists(&path) {
        return Ok(());
    }
    let raw = files::read(&path, 32 * 1024 * 1024)?;
    let text = String::from_utf8_lossy(&raw);
    let mut inside = false;
    let mut names = Vec::new();
    for line in text.lines() {
        if line.starts_with('[') {
            inside = line
                .to_ascii_lowercase()
                .starts_with(r"[software\\microsoft\\windows\\currentversion\\run]");
        } else if inside
            // Hub and Simlink start with Windows by default; under Wine that is
            // every program start in the profile, and MSFS starts Simlink itself.
            && ["navigraph-hub", "navigraphsimlink.exe"]
                .iter()
                .any(|v| line.to_ascii_lowercase().contains(v))
            && let Some(name) = line
                .strip_prefix('"')
                .and_then(|v| v.split_once("\"="))
                .map(|(name, _)| name.replace("\\\\", "\\"))
        {
            names.push(name);
        }
    }
    for name in names {
        wine.run(
            &[
                "reg",
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                &name,
                "/f",
            ],
            None,
            &[0, 1],
            Duration::from_secs(180),
        )?;
    }
    Ok(())
}
fn wine<'a>(ctx: &'a Context, root: &Path, log: &str) -> Result<Wine<'a>> {
    let mut wine = Wine::new(
        &root.join("local/msfs-prefix"),
        &root.join("runner"),
        &root.join("private").join(log),
        &ctx.cancel,
    )?;
    wine.without_fenix();
    Ok(wine)
}
/// Run an interactive Windows program until it and everything it started in
/// the profile have exited, or until Stop ends the named applications.
fn interactive(
    ctx: &Context,
    root: &Path,
    wine: &Wine,
    arguments: &[&Path],
    names: &[&str],
) -> Result<()> {
    let prefix = root.join("local/msfs-prefix");
    let mut command = wine.command()?;
    command.args(arguments);
    if let Some(directory) = arguments.first().and_then(|v| v.parent()) {
        command.current_dir(directory);
    }
    let mut child = process::spawn(&mut command, None)?;
    let mut quiet = None;
    let result = (|| {
        loop {
            if ctx.pause.load(Ordering::Relaxed) || ctx.cancel.load(Ordering::Relaxed) {
                wine_processes::stop(root, names)?;
                process::terminate(&mut child)?;
                return ctx.interrupted();
            }
            let code = child.try_wait()?;
            let running = wine_processes::Processes::new(&prefix)?.applications_running();
            if let Some(code) = code
                && !running
            {
                let since = quiet.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(1500) {
                    return require(
                        code.success(),
                        "Die Navigraph-Anwendung wurde unerwartet beendet. Details stehen im Navigraph-Protokoll.",
                    );
                }
            } else {
                quiet = None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    })();
    if result.is_err() {
        let _ = wine_processes::stop(root, names);
        let _ = process::terminate(&mut child);
    }
    result
}
fn install_hub(ctx: &Context, installer: &Path) -> Result<()> {
    let root = ctx.root()?;
    let prefix = root.join("local/msfs-prefix");
    ctx.progress("Navigraph Hub wird im Windows-Profil installiert …");
    let wine = wine(ctx, root, "navigraph-hub-setup.log")?;
    // electron-builder's NSIS installer: per-user, silent and without autostart.
    wine.run(
        &[installer.to_string_lossy().into_owned(), "/S".into()],
        None,
        &[0],
        Duration::from_secs(1800),
    )?;
    wine.stop_staged()?;
    let directory = hub(&prefix)?.ok_or(Error::Invalid(
        "Navigraph Hub wurde nicht vollständig installiert. Details stehen im Navigraph-Protokoll.",
    ))?;
    disable_governor(&directory)?;
    prefer_foreground(&directory)?;
    remove_run_entries(&wine, &prefix)?;
    wine.stop_staged()
}
fn open_hub(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    let prefix = root.join("local/msfs-prefix");
    let directory = hub(&prefix)?.ok_or(Error::Invalid("Installiere zuerst Navigraph Hub."))?;
    disable_governor(&directory)?;
    prefer_foreground(&directory)?;
    let wine = wine(ctx, root, "navigraph-hub.log")?;
    ctx.progress(
        "Melde dich in Navigraph Hub an und installiere die Navdata. Schließe Hub danach vollständig.",
    );
    let result = interactive(ctx, root, &wine, &[&directory.join(HUB_EXE)], HUB);
    let cleanup = remove_run_entries(&wine, &prefix);
    result.and(cleanup)
}
fn install_simlink(ctx: &Context, installer: &Path) -> Result<()> {
    let root = ctx.root()?;
    let wine = wine(ctx, root, "navigraph-simlink-setup.log")?;
    ctx.progress("Navigraph Simlink wird im Windows-Profil installiert …");
    // Inno Setup's wizard leaves its hidden application window visible under
    // Wine, so the installer runs unattended into its default folder.
    wine.run(
        &[
            installer.to_string_lossy().into_owned(),
            "/VERYSILENT".into(),
            "/SUPPRESSMSGBOXES".into(),
            "/NORESTART".into(),
            "/SP-".into(),
        ],
        None,
        &[0],
        Duration::from_secs(1800),
    )?;
    let _ = wine_processes::stop(root, SIMLINK);
    simlink_without_login_start(&root.join("local/msfs-prefix"))?;
    remove_run_entries(&wine, &root.join("local/msfs-prefix"))?;
    wine.stop_staged()?;
    require(
        simlink(&root.join("local/msfs-prefix"))?.is_some(),
        "Simlink wurde nicht vollständig installiert. Details stehen im Navigraph-Protokoll.",
    )
}
fn open_simlink(ctx: &Context) -> Result<()> {
    let root = ctx.root()?;
    let executable = simlink(&root.join("local/msfs-prefix"))?
        .ok_or(Error::Invalid("Installiere zuerst Navigraph Simlink."))?;
    let wine = wine(ctx, root, "navigraph-simlink.log")?;
    let prefix = root.join("local/msfs-prefix");
    simlink_without_login_start(&prefix)?;
    ctx.progress(
        "Melde dich in Simlink an. Schließe Simlink danach mit „Navigraph schließen“, denn sein Fenster blendet es nur aus.",
    );
    let result = interactive(ctx, root, &wine, &[&executable], SIMLINK);
    // The first start creates the settings file with login start turned on.
    let settings = simlink_without_login_start(&prefix);
    let cleanup = remove_run_entries(&wine, &prefix);
    result.and(settings).and(cleanup)
}
fn installer_path(data: &Value, app: App) -> Result<PathBuf> {
    let path = PathBuf::from(string(data, "installer_path")?.trim());
    let name = path
        .file_name()
        .map(|v| v.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    require(
        path.is_absolute()
            && name.ends_with(".exe")
            // Both install silently, so only Navigraph's own installer is accepted.
            && name.contains("navigraph")
            && name.contains(match app {
                App::Hub => "hub",
                App::Simlink => "simlink",
            })
            && executable_file(&path)?,
        match app {
            App::Hub => "Wähle die heruntergeladene Navigraph-Hub-Installer-Datei (.exe).",
            App::Simlink => "Wähle die heruntergeladene Simlink-Installer-Datei (.exe).",
        },
    )?;
    Ok(path.canonicalize()?)
}
pub fn pick(app: &Launcher, data: &Value) -> Result<Value> {
    let title = match data["app"].as_str() {
        Some("hub") => "--title=Navigraph Hub",
        Some("simlink") => "--title=Navigraph Simlink",
        _ => return Err(Error::Invalid("Unbekannter Navigraph-Installer.")),
    };
    {
        let s = app.lock();
        Launcher::open(&s)?;
        require(
            s.active.is_none(),
            "Bitte die laufende Einrichtung zuerst abschließen oder abbrechen.",
        )?;
    }
    let _guard = crate::setup::PICKER
        .try_lock()
        .map_err(|_| Error::Invalid("Ein Dateidialog ist bereits geöffnet."))?;
    let (name, program) = crate::setup::picker().ok_or(Error::Invalid(
        "Kein Dateidialog verfügbar. Bitte den Pfad direkt eingeben.",
    ))?;
    let mut command = Command::new(program);
    if name == "zenity" {
        command.args([
            "--file-selection",
            title,
            "--file-filter=Windows Installer | *.exe",
        ]);
    } else {
        command.arg("--getopenfilename").arg(files::home());
    }
    let path = crate::dialog::select(&mut command, Duration::from_secs(120))?;
    Ok(if let Some(path) = path {
        json!({"ok":true,"path":path})
    } else {
        json!({"ok":true,"cancelled":true})
    })
}
const OPERATIONS: &[&str] = &[
    "install-hub",
    "open-hub",
    "install-simlink",
    "open-simlink",
    "configure",
    "disable",
    "handler",
    "stop",
];
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    require(
        OPERATIONS.contains(&operation),
        "Unbekannter Navigraph-Vorgang.",
    )?;
    let root = app.root()?;
    require(
        root == Path::new(string(data, "runtime_path")?),
        "Die ausgewählte Runtime hat sich geändert. Navigraph-Status neu laden.",
    )?;
    if operation == "handler" {
        register_handler(&app.state_dir)?;
        return Ok(json!({"ok":true}));
    }
    if operation == "stop" {
        require(
            snapshot(app)["can_stop"] == true,
            "Beende MSFS und laufende Installationen, bevor du Navigraph schließt.",
        )?;
        let mut s = app.lock();
        Launcher::open(&s)?;
        if s.active.as_ref().is_some_and(|v| v.kind == "navigraph")
            && s.jobs.get("navigraph").is_some_and(|v| {
                v["state"] == "running"
                    && !["install-hub", "install-simlink"]
                        .contains(&v["operation"].as_str().unwrap_or(""))
            })
        {
            if let Some(active) = &s.active {
                active.pause.store(true, Ordering::Relaxed);
            }
            let job = s
                .jobs
                .get_mut("navigraph")
                .ok_or(Error::Invalid("Navigraph läuft nicht."))?;
            job["stopping"] = json!(true);
            return Ok(json!({"ok":true,"job_id":job["id"]}));
        }
    } else {
        validate(&root)?;
    }
    let installer = match operation {
        "install-hub" => Some(installer_path(data, App::Hub)?),
        "install-simlink" => Some(installer_path(data, App::Simlink)?),
        _ => None,
    };
    let operation = operation.to_string();
    app.start_job("navigraph", &operation.clone(), true, move |ctx| {
        let selected = ctx.root()?;
        require(
            selected == root,
            "Die ausgewählte Runtime hat sich geändert. Navigraph-Status neu laden.",
        )?;
        let root = selected;
        if operation != "stop" {
            validate(root)?;
            wine_processes::idle(&root.join("local/msfs-prefix"))?;
        }
        match operation.as_str() {
            "install-hub" => install_hub(
                ctx,
                installer
                    .as_deref()
                    .ok_or(Error::Invalid("Fehlende Navigraph-Hub-Installer-Datei."))?,
            )?,
            "open-hub" => open_hub(ctx)?,
            "install-simlink" => install_simlink(
                ctx,
                installer
                    .as_deref()
                    .ok_or(Error::Invalid("Fehlende Simlink-Installer-Datei."))?,
            )?,
            "open-simlink" => open_simlink(ctx)?,
            "stop" => {
                let mut names = HUB.to_vec();
                names.extend(SIMLINK);
                wine_processes::stop(root, &names)?
            }
            _ => {
                if operation == "configure" {
                    simlink_without_login_start(&root.join("local/msfs-prefix"))?;
                }
                configure(root, operation == "configure")?
            }
        }
        let message = match operation.as_str() {
            "install-hub" => "Navigraph Hub ist installiert. Öffne Hub, melde dich an und installiere die Navdata.",
            "open-hub" => "Navigraph Hub wurde geschlossen. Die Navdata-Pakete werden neu geprüft.",
            "install-simlink" => "Simlink ist installiert. Richte den automatischen Start mit MSFS ein.",
            "open-simlink" => "Simlink wurde geschlossen.",
            "configure" => "Simlink startet jetzt mit MSFS. Die Verbindung im Simulator ist noch unbestätigt.",
            "disable" => "Der automatische Simlink-Start ist ausgeschaltet.",
            _ => "Navigraph wurde geschlossen.",
        };
        Ok(json!({"state":"complete","message":message,"stopping":false}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_links_select_their_application_only() {
        assert_eq!(
            callback_app("navigraph-desktop://callback?code=1").ok(),
            Some(App::Hub)
        );
        assert_eq!(
            callback_app("Navigraph-Traffic-Desktop://signin#x").ok(),
            Some(App::Simlink)
        );
        for url in [
            "https://navigraph.com",
            "navigraph-desktop:",
            "navigraph-desktop://",
            "navigraph-desktop://a\nb",
            "file:///etc/passwd",
        ] {
            assert!(callback_app(url).is_err(), "{url}");
        }
    }

    #[test]
    fn startup_entry_is_added_once_and_keeps_other_addons() {
        let raw = br#"<?xml version="1.0" encoding="Windows-1252"?>
<SimBase.Document Type="Launch" version="1,0">
  <Descr>Launch</Descr>
  <Filename>exe.xml</Filename>
  <Disabled>False</Disabled>
  <Launch.Addon><Name>FenixA320</Name><Disabled>False</Disabled><Path>C:\Program Files\FenixSim A320\deps\FenixBootstrapper.exe</Path></Launch.Addon>
  <Launch.Addon><Name>Old</Name><Path>"C:\Program Files\Navigraph\Simlink\NavigraphSimlink.exe"</Path></Launch.Addon>
  <Launch.Addon><Name>Duplicate</Name><Path>C:\x\navigraphsimlink.exe</Path></Launch.Addon>
</SimBase.Document>"#;
        let path = r"C:\Program Files\Navigraph\Simlink\NavigraphSimlink.exe";
        let bytes = startup_document(Some(raw), path, true).expect("updated");
        let tree = xml::parse(&bytes).expect("valid");
        let entries: Vec<_> = tree
            .items
            .iter()
            .filter_map(|v| match v {
                Item::Element(v) if v.named("Launch.Addon") => Some(v),
                _ => None,
            })
            .collect();
        assert_eq!(entries.len(), 2);
        assert!(
            entries[0]
                .child("Name")
                .is_some_and(|v| v.text() == "FenixA320")
        );
        assert!(is_simlink(entries[1]) && enabled(entries[1]));
        assert_eq!(
            entries[1].child("Path").map(Element::text).as_deref(),
            Some(path)
        );
        let disabled = startup_document(Some(&bytes), path, false).expect("disabled");
        let tree = xml::parse(&disabled).expect("valid");
        assert!(
            tree.items
                .iter()
                .any(|v| matches!(v, Item::Element(v) if is_simlink(v) && !enabled(v)))
        );
        let fresh = startup_document(None, path, true).expect("created");
        assert!(
            xml::parse(&fresh)
                .expect("valid")
                .items
                .iter()
                .any(|v| matches!(v, Item::Element(v) if is_simlink(v)))
        );
        assert!(startup_document(None, path, false).is_ok());
    }

    #[test]
    fn disabled_startup_document_is_not_reenabled() {
        let raw = br#"<SimBase.Document Type="Launch" version="1,0"><Disabled>True</Disabled></SimBase.Document>"#;
        assert!(startup_document(Some(raw), r"C:\a\NavigraphSimlink.exe", true).is_err());
    }

    #[test]
    fn governor_is_moved_aside_and_replaced_after_updates() {
        let root = tempfile::tempdir().expect("fixture");
        let bin = root.path().join("resources/assets/bin");
        fs::create_dir_all(&bin).expect("bin");
        fs::write(bin.join("procgov.exe"), b"old").expect("governor");
        assert!(disable_governor(root.path()).expect("disabled"));
        assert!(!governor_active(root.path()));
        assert!(!disable_governor(root.path()).expect("unchanged"));
        fs::write(bin.join("procgov.exe"), b"new").expect("updated governor");
        assert!(disable_governor(root.path()).expect("disabled again"));
        assert_eq!(
            fs::read(bin.join("procgov.exe.flightdeck-disabled")).expect("kept"),
            b"new"
        );
    }

    #[test]
    fn hub_quits_with_its_window_and_keeps_other_settings() {
        let root = tempfile::tempdir().expect("fixture");
        let user = root.path().join("users/steamuser");
        let directory = user.join(HUB_DIR);
        fs::create_dir_all(&directory).expect("hub");
        prefer_foreground(&directory).expect("created");
        let path = user.join("AppData/Roaming/navigraph-hub/config.json");
        let value: Value = files::json(&path, 4096).expect("config");
        assert_eq!(value, json!({"runInBackground":false,"autoLaunch":false}));
        fs::write(&path, br#"{"runInBackground":true,"language":"de"}"#).expect("changed");
        prefer_foreground(&directory).expect("updated");
        let value: Value = files::json(&path, 4096).expect("config");
        assert_eq!(value["language"], "de");
        assert_eq!(value["runInBackground"], false);
        fs::write(&path, b"[]").expect("invalid");
        assert!(prefer_foreground(&directory).is_err());
    }

    #[test]
    fn simlink_sign_in_and_login_start_come_from_its_settings() {
        let settings = "[simlink]\r\nautocheck_update=1\r\nautostart_app=1\r\n\r\n[user]\r\nrefresh_token=abc\r\nautostart_app=1\r\n";
        assert_eq!(ini_value(settings, "user", "refresh_token"), Some("abc"));
        assert_eq!(ini_value(settings, "simlink", "refresh_token"), None);
        let changed = without_login_start(settings).expect("changed");
        assert_eq!(
            changed,
            "[simlink]\r\nautocheck_update=1\r\nautostart_app=0\r\n\r\n[user]\r\nrefresh_token=abc\r\nautostart_app=1\r\n"
        );
        assert_eq!(without_login_start(&changed), None);
        let root = tempfile::tempdir().expect("fixture");
        let prefix = root.path();
        let path = prefix
            .join("drive_c/users/steamuser")
            .join(SIMLINK_SETTINGS);
        fs::create_dir_all(path.parent().expect("folder")).expect("folder");
        fs::write(&path, "[user]\nrefresh_token=\n").expect("settings");
        assert!(!simlink_signed_in(prefix).expect("empty token"));
        fs::write(&path, settings).expect("settings");
        assert!(simlink_signed_in(prefix).expect("token"));
        simlink_without_login_start(prefix).expect("login start");
        assert_eq!(fs::read_to_string(&path).expect("settings"), changed);
    }
    #[test]
    fn hub_installs_only_from_its_own_installer() {
        let root = tempfile::tempdir().expect("fixture");
        let select = |name: &str, app| {
            let path = root.path().join(name);
            fs::write(&path, b"MZ").expect("installer");
            installer_path(&json!({ "installer_path": path }), app).is_ok()
        };
        assert!(select("Navigraph Hub 1.2.25.exe", App::Hub));
        assert!(select("navigraph-hub-setup.EXE", App::Hub));
        assert!(!select("Navigraph+Simlink.exe", App::Hub));
        assert!(!select("setup.exe", App::Hub));
        assert!(!select("Navigraph Hub.msi", App::Hub));
        assert!(select("Navigraph+Simlink.exe", App::Simlink));
        assert!(!select("Navigraph Hub 1.2.25.exe", App::Simlink));
        assert!(!select("setup.exe", App::Simlink));
        assert!(
            !installer_path(
                &json!({"installer_path":"relative/Navigraph Hub.exe"}),
                App::Hub
            )
            .is_ok()
        );
    }
    #[test]
    fn only_a_managed_release_hands_links_to_its_wrapper() {
        let wrapper = "exec '/data/releases/abc/bin/flightdeck' --managed-root '/data' \"$@\"\n";
        let release = Path::new("/data/releases/abc/bin/flightdeck");
        assert!(managed_release(release, Some(wrapper)));
        assert!(!managed_release(release, None));
        assert!(!managed_release(
            Path::new("/other/releases/abc/bin/flightdeck"),
            Some(wrapper)
        ));
        assert!(!managed_release(
            Path::new("/work/target/debug/flightdeck-rust"),
            Some(wrapper)
        ));
    }
    #[test]
    fn handler_entry_quotes_paths_and_lists_both_schemes() {
        let bytes =
            handler_bytes(Path::new("/opt/flight deck/$bin"), Path::new("/state")).expect("entry");
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(
            text.contains(
                r#"Exec="/opt/flight deck/\\$bin" --state-dir "/state" navigraph-url %u"#
            )
        );
        assert!(text.contains(
            "MimeType=x-scheme-handler/navigraph-desktop;x-scheme-handler/navigraph-traffic-desktop;"
        ));
    }
}
