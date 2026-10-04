// SPDX-License-Identifier: MIT
//! Verified public releases; downloads never accept a caller-provided URL.
use crate::{
    Error, Result,
    backend::{Context, Launcher, State as LauncherState, string},
    error::require,
    files, installer, process, transaction as tx,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub const PROJECT: &str = "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox";
pub const API: &str =
    "https://api.github.com/repos/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases?per_page=50";
pub const ASSET: &str = "Flightdeck-Linux-x86_64.tar.gz";
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_EXPANDED: u64 = 384 * 1024 * 1024;
const MAX_MEMBER: u64 = 128 * 1024 * 1024;
const INVALID: &str = "Das GitHub-Updatepaket hat keine gültigen Prüfdaten.";
const NETWORK: &str =
    "GitHub ist nicht erreichbar. Prüfe deine Internetverbindung und versuche es erneut.";
const UNAVAILABLE: &str = "Dieser Launcher-Updatevorgang ist gerade nicht verfügbar.";
pub struct State {
    pub context: Option<(PathBuf, String)>,
    pub context_error: String,
    pub release: Value,
    pub check_id: Option<String>,
    pub checked_at: Option<String>,
    pub job: Value,
    pub worker: Option<Arc<AtomicBool>>,
    pub attempted: Option<Instant>,
    pub restart: Option<Child>,
}
impl Default for State {
    fn default() -> Self {
        let (context, error) = match installer::context() {
            Ok(context) => (context, String::new()),
            Err(e) => (None, e.to_string()),
        };
        Self {
            context,
            context_error: error,
            release: Value::Null,
            check_id: None,
            checked_at: None,
            job: Value::Null,
            worker: None,
            attempted: None,
            restart: None,
        }
    }
}
pub fn version(text: &str) -> Result<[u32; 3]> {
    let captures = crate::log_reader::regex(
        r"^v?(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})$",
    )
    .captures(text)
    .ok_or(Error::Invalid(
        "GitHub meldet eine ungültige Flightdeck-Version.",
    ))?;
    Ok([1, 2, 3].map(|i| captures[i].parse().expect("bounded digits")))
}
pub fn safe_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && matches!(
                u.host_str(),
                Some(
                    "api.github.com"
                        | "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            )
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none()
            && u.fragment().is_none()
    })
}
fn newer_than_running(release: [u32; 3]) -> bool {
    let base = version(crate::VERSION.split('-').next().unwrap_or(crate::VERSION))
        .expect("compiled semantic version");
    release > base || release == base && crate::VERSION.contains('-')
}
pub fn release_metadata(raw: &Value) -> Result<Value> {
    require(
        raw["draft"] == false && raw["prerelease"] == false,
        "GitHub meldet keine veröffentlichte Flightdeck-Version.",
    )?;
    let tag = string(raw, "tag_name")?;
    version(tag)?;
    let matches: Vec<_> = raw["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["name"] == ASSET)
        .collect();
    require(
        matches.len() == 1,
        "Das vollständige Linux-Updatepaket fehlt in diesem Release.",
    )?;
    let asset = matches[0];
    let checksum = string(asset, "digest")?
        .strip_prefix("sha256:")
        .ok_or(Error::Invalid(INVALID))?;
    let url = format!("{PROJECT}/releases/download/{tag}/{ASSET}");
    require(
        asset["browser_download_url"] == url
            && asset["state"] == "uploaded"
            && asset["size"]
                .as_u64()
                .is_some_and(|v| v > 0 && v <= MAX_ARCHIVE)
            && files::hex_digest(checksum),
        INVALID,
    )?;
    Ok(
        json!({"version":tag.trim_start_matches('v'),"tag":tag,"url":url,"sha256":checksum,"size":asset["size"],"release_url":format!("{PROJECT}/releases/tag/{tag}"),"notes":raw["body"].as_str().unwrap_or("").chars().take(12000).collect::<String>()}),
    )
}
fn open(url: &str) -> Result<reqwest::blocking::Response> {
    require(safe_url(url), "Ungültige GitHub-Downloadadresse.")?;
    let client = crate::http_client::builder(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !safe_url(attempt.url().as_str()) {
                attempt.error("invalid update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| Error::Invalid(NETWORK))?;
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .map_err(|_| Error::Invalid(NETWORK))?;
    require(
        response.status().is_success(),
        "GitHub ist gerade nicht verfügbar oder das Abfragelimit ist erreicht. Bitte später erneut versuchen.",
    )?;
    Ok(response)
}
/// The legacy /latest endpoint stays on the Python transition release. Prepared
/// launchers choose the highest stable full release from the bounded feed.
pub fn stable_release(raw: &Value) -> Result<Value> {
    let releases = raw.as_array().ok_or(Error::Invalid(INVALID))?;
    require(releases.len() <= 50, INVALID)?;
    let candidates: Vec<_> = releases
        .iter()
        .filter(|v| v["draft"] == false && v["prerelease"] == false)
        .filter(|v| {
            v["assets"]
                .as_array()
                .is_some_and(|assets| assets.iter().any(|a| a["name"] == ASSET))
        })
        .filter_map(|v| {
            v["tag_name"]
                .as_str()
                .and_then(|tag| version(tag).ok())
                .map(|number| (number, v))
        })
        .collect();
    let newest = candidates
        .iter()
        .map(|(number, _)| *number)
        .max()
        .ok_or(Error::Invalid(
            "GitHub meldet keine veröffentlichte Flightdeck-Version.",
        ))?;
    let matches: Vec<_> = candidates
        .into_iter()
        .filter(|(number, _)| *number == newest)
        .collect();
    require(matches.len() == 1, INVALID)?;
    release_metadata(matches[0].1)
}

pub fn latest_release() -> Result<Value> {
    let mut data = Vec::new();
    open(API)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| Error::Invalid(NETWORK))?;
    release_feed(&data)
}
pub fn release_feed(data: &[u8]) -> Result<Value> {
    require(
        data.len() <= 2 * 1024 * 1024,
        "Die GitHub-Antwort ist zu groß.",
    )?;
    stable_release(&crate::strict_json::decode(data).map_err(|_| Error::Invalid(INVALID))?)
}
pub fn download(
    release: &Value,
    target: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    let size = release["size"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= MAX_ARCHIVE)
        .ok_or(Error::Invalid(INVALID))?;
    let hash = string(release, "sha256")?;
    require(files::hex_digest(hash), INVALID)?;
    let mut response = open(string(release, "url")?)?;
    require(response.content_length().is_none_or(|v| v == size), INVALID)?;
    installer::no_links(target)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?;
    let mut received = 0u64;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        tx::interrupted(cancel)?;
        let count = response
            .read(&mut buffer)
            .map_err(|_| Error::Invalid(NETWORK))?;
        if count == 0 {
            break;
        }
        received += count as u64;
        require(
            received <= size,
            "Die Größe des Updatepakets stimmt nicht mit GitHub überein.",
        )?;
        digest.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
        progress(received, size);
    }
    require(
        received == size && hex::encode(digest.finalize()) == hash,
        "Die Prüfsumme des Updates stimmt nicht. Bitte erneut herunterladen.",
    )?;
    output.sync_all()?;
    Ok(())
}
pub fn extract(
    archive: &Path,
    destination: &Path,
    expected: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    version(expected)?;
    require(
        !files::exists(destination),
        "Der Updateordner ist nicht leer.",
    )?;
    files::private_dir(destination)?;
    let input = files::open_at(rustix::fs::CWD, archive, false, false)?;
    require(input.metadata()?.len() <= MAX_ARCHIVE, INVALID)?;
    let reader = flate2::read::GzDecoder::new(input).take(MAX_EXPANDED + 4 * 1024 * 1024);
    let mut archive = tar::Archive::new(reader);
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for item in archive.entries()? {
        tx::interrupted(cancel)?;
        let mut item = item?;
        let raw = item.path_bytes();
        let name = std::str::from_utf8(&raw).map_err(|_| Error::Invalid(INVALID))?;
        let kind = item.header().entry_type();
        let name = if kind.is_dir() {
            name.trim_end_matches('/')
        } else {
            name
        };
        require(
            name.len() <= 1024
                && name.split('/').next() == Some("flightdeck-linux")
                && name.split('/').all(|s| !matches!(s, "" | "." | ".."))
                && !name.contains(['\\', ':'])
                && !name.chars().any(char::is_control)
                && seen.len() < 4096
                && seen.insert(name.to_owned())
                && (matches!(kind.as_byte(), 0 | b'0') || kind.is_dir())
                && item.size() <= MAX_MEMBER,
            "Das Updatepaket enthält ungültige Dateien.",
        )?;
        total = total
            .checked_add(item.size())
            .ok_or(Error::Invalid(INVALID))?;
        require(
            total <= MAX_EXPANDED,
            "Das entpackte Updatepaket ist zu groß.",
        )?;
        let target = destination.join(name);
        if kind.is_dir() {
            require(item.size() == 0, INVALID)?;
            files::private_dir(&target)?;
            continue;
        }
        files::private_dir(target.parent().ok_or(Error::Invalid(INVALID))?)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&target)?;
        let mut remaining = item.size();
        let mut buffer = vec![0; 256 * 1024];
        while remaining > 0 {
            tx::interrupted(cancel)?;
            let count = item.read(&mut buffer[..(remaining as usize).min(256 * 1024)])?;
            require(count > 0, "Das Updatepaket ist unvollständig.")?;
            output.write_all(&buffer[..count])?;
            remaining -= count as u64;
        }
        output.sync_all()?;
    }
    let source = destination.join("flightdeck-linux");
    let package: Value = crate::cloud::json(&files::read(
        &source.join(installer::PACKAGE),
        2 * 1024 * 1024,
    )?)?;
    require(
        package["version"] == expected,
        "Die Version im Updatepaket stimmt nicht mit GitHub überein.",
    )?;
    let executable = installer::package_executable(&source)?;
    fs::set_permissions(executable, fs::Permissions::from_mode(0o700))?;
    Ok(source)
}
fn cloud_idle(s: &LauncherState) -> Result<()> {
    require(
        crate::cloud_sync::automatic(s)["state"] != "attention",
        "Bitte zuerst den ausstehenden Cloud-Abgleich abschließen.",
    )
}
pub fn snapshot_locked(s: &mut LauncherState) -> Value {
    Launcher::poll(s);
    let idle = s.active.is_none() && s.process.is_none() && !s.closing && !Launcher::external(s);
    let u = &mut s.launcher_updates;
    let installed = u
        .context
        .as_ref()
        .and_then(|(root, _)| match installer::load(root) {
            Ok(v) => v,
            Err(e) => {
                u.context_error = e.to_string();
                None
            }
        });
    let pending = installed.as_ref().is_some_and(|v| {
        u.context
            .as_ref()
            .is_some_and(|(_, running)| v["current"] != *running)
    });
    if u.restart
        .as_mut()
        .is_some_and(|child| child.try_wait().is_ok_and(|v| v.is_some()))
    {
        u.restart = None;
        if pending {
            u.job = json!({"id":uuid::Uuid::new_v4().simple().to_string(),"operation":"restart","state":"failed","phase":"restart","can_cancel":false,"message":"","error":"Flightdeck konnte nicht neu geöffnet werden. Bitte beende Spiel und Einrichtung und versuche es erneut."});
        }
    }
    let active = u.worker.is_some();
    let newer = u.release["version"]
        .as_str()
        .and_then(|v| version(v).ok())
        .is_some_and(newer_than_running);
    json!({"installed_version":crate::VERSION,"latest_version":u.release["version"],"update_available":if u.release.is_null(){Value::Null}else{json!(newer)},"checked_at":u.checked_at,"check_id":u.check_id,"release_url":u.release["release_url"].as_str().map(str::to_owned).unwrap_or(format!("{PROJECT}/releases")),"notes":u.release["notes"].as_str().unwrap_or(""),"download_size":u.release["size"],"managed":installed.is_some(),"pending_restart":pending,"can_check":!active&&!pending&&!s.closing,"can_install":installed.is_some()&&newer&&idle&&!active&&!pending,"can_rollback":installed.as_ref().is_some_and(|v|v["previous"].is_string())&&idle&&!active&&!pending,"can_restart":pending&&idle&&!active&&u.restart.is_none(),"busy":!idle,"unavailable_reason":if !u.context_error.is_empty(){&u.context_error}else if installed.is_none(){"Für Updates im Launcher Flightdeck einmal mit dem offiziellen Installer installieren."}else{""},"job":u.job})
}
pub fn snapshot(app: &Launcher) -> Value {
    snapshot_locked(&mut app.lock())
}
fn change(app: &Launcher, id: &str, patch: Value) {
    let mut s = app.lock();
    if s.launcher_updates.job["id"] == id
        && let (Some(job), Some(patch)) =
            (s.launcher_updates.job.as_object_mut(), patch.as_object())
    {
        job.extend(patch.clone());
    }
}
fn commit(app: &Launcher, id: &str, cancel: &AtomicBool) -> Result<()> {
    let mut s = app.lock();
    tx::interrupted(cancel)?;
    require(s.launcher_updates.job["id"] == id, UNAVAILABLE)?;
    s.launcher_updates.job["can_cancel"] = json!(false);
    s.launcher_updates.job["phase"] = json!("installing");
    Ok(())
}
fn work(
    app: &Arc<Launcher>,
    id: &str,
    operation: &str,
    release: &Value,
    context: Option<(PathBuf, String)>,
    cancel: &AtomicBool,
) -> Result<&'static str> {
    if operation == "check" {
        let found = latest_release()?;
        tx::interrupted(cancel)?;
        let newer = newer_than_running(version(string(&found, "version")?)?);
        let mut s = app.lock();
        require(s.launcher_updates.job["id"] == id, UNAVAILABLE)?;
        s.launcher_updates.release = found;
        s.launcher_updates.check_id = Some(uuid::Uuid::new_v4().simple().to_string());
        s.launcher_updates.checked_at = Some(files::now());
        return Ok(if newer {
            "Eine neue Flightdeck-Version ist verfügbar."
        } else {
            "Flightdeck ist auf dem neuesten Stand."
        });
    }
    let (root, running) = context.ok_or(Error::Invalid(UNAVAILABLE))?;
    let state = installer::load(&root)?.ok_or(Error::Invalid(UNAVAILABLE))?;
    require(
        state["current"] == running,
        "Flightdeck wurde inzwischen geändert. Bitte den Launcher neu öffnen.",
    )?;
    let language = state["language"].as_str().unwrap_or("en");
    if operation == "rollback" {
        commit(app, id, cancel)?;
        installer::rollback(&root, language, Some(&running))?;
        return Ok("Vorherige Version bereit. Öffne Flightdeck jetzt neu.");
    }
    let staging = tx::new_directory(&app.state_dir, ".launcher-update-")?;
    let identity = crate::owned_tree::identity(&staging)?;
    let result = (|| -> Result<()> {
        let archive = staging.join(ASSET);
        change(
            app,
            id,
            json!({"phase":"downloading","message":"Flightdeck wird von GitHub heruntergeladen …"}),
        );
        download(release, &archive, cancel, |received, total| {
            change(
                app,
                id,
                json!({"received":received,"total":total,"progress":received*100/total}),
            )
        })?;
        change(
            app,
            id,
            json!({"phase":"verifying","message":"Updatepaket wird geprüft …","progress":null}),
        );
        let source = extract(
            &archive,
            &staging.join("unpacked"),
            string(release, "version")?,
            cancel,
        )?;
        commit(app, id, cancel)?;
        change(app, id, json!({"message":"Flightdeck wird aktualisiert …"}));
        let executable = installer::package_executable(&source)?;
        let status = process::run(
            Command::new(executable)
                .arg("install")
                .arg("--source")
                .arg(&source)
                .arg("--data-dir")
                .arg(&root)
                .args([
                    "--no-desktop",
                    "--no-launch",
                    "--language",
                    language,
                    "--expected-current",
                    &running,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            Duration::from_secs(180),
            &AtomicBool::new(false),
        )?;
        require(
            status.success(),
            "Das Launcher-Update konnte nicht installiert werden. Die bisherige Version bleibt verfügbar.",
        )?;
        Ok(())
    })();
    let cleanup = crate::owned_tree::remove(&staging, identity);
    result?;
    cleanup?;
    Ok("Update installiert. Öffne Flightdeck jetzt neu.")
}
fn begin(
    app: &Arc<Launcher>,
    operation: &str,
    check_id: Option<&str>,
    startup: bool,
) -> Result<Value> {
    let mut s = app.lock();
    Launcher::open(&s)?;
    if startup
        && s.launcher_updates
            .attempted
            .is_some_and(|v| v.elapsed() < Duration::from_secs(1800))
    {
        return Ok(json!({"ok":true}));
    }
    let allowed = snapshot_locked(&mut s);
    let permission = match operation {
        "check" => "can_check",
        "install" => "can_install",
        "rollback" => "can_rollback",
        _ => return Err(Error::Invalid(UNAVAILABLE)),
    };
    if startup && allowed[permission] != true {
        return Ok(json!({"ok":true}));
    }
    require(allowed[permission] == true, UNAVAILABLE)?;
    if operation == "install" {
        require(
            check_id.is_some() && check_id == s.launcher_updates.check_id.as_deref(),
            "Bitte die Flightdeck-Updates zuerst erneut prüfen.",
        )?;
    }
    let ctx: Option<Context> = if operation == "check" {
        None
    } else {
        cloud_idle(&s)?;
        Some(app.reserve_locked(&mut s, "launcher-update", operation, false)?)
    };
    let cancel = ctx
        .as_ref()
        .map(|v| Arc::clone(&v.cancel))
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let id = uuid::Uuid::new_v4().simple().to_string();
    let u = &mut s.launcher_updates;
    if operation == "check" {
        u.release = Value::Null;
        u.check_id = None;
        u.attempted = Some(Instant::now());
    }
    u.job = json!({"id":id,"operation":operation,"state":"running","phase":if operation=="check"{"checking"}else{"preparing"},"message":if operation=="check"{"GitHub wird nach Flightdeck-Updates gefragt …"}else{"Launcher-Update wird vorbereitet …"},"progress":null,"received":0,"total":null,"error":"","can_cancel":operation!="rollback"});
    u.worker = Some(Arc::clone(&cancel));
    let release = u.release.clone();
    let context = u.context.clone();
    let reply = json!({"ok":true,"job":u.job});
    drop(s);
    let app = Arc::clone(app);
    let operation = operation.to_owned();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            work(&app, &id, &operation, &release, context, &cancel)
        }))
        .unwrap_or(Err(Error::Invalid(
            "Das Launcher-Update ist fehlgeschlagen. Die bisherige Version bleibt verfügbar.",
        )));
        match &result {
            Ok(message) => change(
                &app,
                &id,
                json!({"state":"complete","phase":"complete","message":message,"progress":100,"can_cancel":false}),
            ),
            Err(error) => change(
                &app,
                &id,
                json!({"state":if matches!(error,Error::Cancelled){"cancelled"}else{"failed"},"message":error.to_string(),"error":error.to_string(),"can_cancel":false}),
            ),
        };
        if let Some(ctx) = ctx {
            app.finish(&ctx, result.map(|_| json!({})), false);
        }
        app.lock().launcher_updates.worker = None;
    });
    Ok(reply)
}
pub fn start(app: &Arc<Launcher>, operation: &str, check_id: Option<&str>) -> Result<Value> {
    begin(app, operation, check_id, false)
}
pub fn check_on_startup(app: &Arc<Launcher>) -> Result<Value> {
    begin(app, "check", None, true)
}
pub fn cancel(app: &Launcher, id: &str) -> Result<Value> {
    let mut s = app.lock();
    let u = &mut s.launcher_updates;
    require(
        u.job["id"] == id && u.job["state"] == "running" && u.job["can_cancel"] == true,
        "Dieser Launcher-Updatevorgang kann nicht mehr abgebrochen werden.",
    )?;
    u.worker
        .as_ref()
        .ok_or(Error::Invalid(UNAVAILABLE))?
        .store(true, Ordering::Relaxed);
    u.job["can_cancel"] = json!(false);
    u.job["message"] = json!("Launcher-Update wird abgebrochen …");
    Ok(json!({"ok":true}))
}
pub fn restart(app: &Launcher, port: u16) -> Result<Value> {
    let mut s = app.lock();
    require(
        snapshot_locked(&mut s)["can_restart"] == true,
        "Bitte beende Spiel und Einrichtung vor dem Launcher-Neustart.",
    )?;
    cloud_idle(&s)?;
    let (root, _) = s
        .launcher_updates
        .context
        .as_ref()
        .ok_or(Error::Invalid(UNAVAILABLE))?;
    let state = installer::load(root)?.ok_or(Error::Invalid(UNAVAILABLE))?;
    let entry = &state["entries"]["launcher"];
    let path = Path::new(string(entry, "path")?);
    installer::no_links(path)?;
    require(
        files::sha256(&files::read(path, 1024 * 1024)?) == string(entry, "sha256")?,
        "Der installierte Launcher wurde verändert. Bitte den Installer erneut ausführen.",
    )?;
    let source = installer::verify_release(root, string(&state, "current")?)?;
    let mut command = if source.join("bin/flightdeck").is_file() {
        let mut cmd = Command::new(source.join("bin/flightdeck"));
        cmd.arg("desktop-handoff")
            .arg("--state-dir")
            .arg(&app.state_dir)
            .arg("--port")
            .arg(port.to_string());
        cmd
    } else {
        // Explicit rollback to an older Python release retains its own service.
        let mut cmd = Command::new("python3");
        cmd.args(["-B","-c","import sys;from pathlib import Path;sys.path.insert(0,sys.argv.pop(1));from flightdeck.desktop import ensure_service;_,r=ensure_service(Path(sys.argv[1]),port=int(sys.argv[2]));sys.exit(1 if r.get('update_pending') else 0)"]).arg(&source).arg(&app.state_dir).arg(port.to_string()).env_remove("PYTHONHOME").env_remove("PYTHONPATH");
        cmd
    };
    let child = process::spawn(
        command
            .current_dir(&source)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0),
        None,
    )?;
    let u = &mut s.launcher_updates;
    u.restart = Some(child);
    u.job = json!({"id":uuid::Uuid::new_v4().simple().to_string(),"operation":"restart","state":"running","phase":"restart","can_cancel":false,"message":"Flightdeck wird neu geöffnet …","error":""});
    Ok(json!({"ok":true,"job":u.job}))
}
