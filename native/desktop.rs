// SPDX-License-Identifier: MIT
//! Reuse a background service only after lock, process-birth and token checks.
use crate::{Error, Result, error::require, files, installer, process, resources, server::Service};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
const RECORD: &str = "desktop-service.json";
const UNVERIFIED: &str =
    "Ein vorhandener Hintergrunddienst konnte nicht verifiziert werden. Er wurde nicht verändert.";
pub fn state_directory(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    for component in path.ancestors() {
        require(
            !component.is_symlink(),
            "Der Einstellungsordner muss ein echter Ordner deines Benutzerkontos sein.",
        )?;
    }
    files::private_dir(&path)?;
    Ok(path.canonicalize()?)
}
pub fn process_start(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let path = PathBuf::from(format!("/proc/{pid}"));
    if path.metadata().ok()?.uid() != files::uid() {
        return None;
    }
    let data = fs::read_to_string(path.join("stat")).ok()?;
    let (_, tail) = data.rsplit_once(')')?;
    let fields = tail.split_whitespace().collect::<Vec<_>>();
    if ["Z", "X"].contains(fields.first()?) {
        return None;
    }
    fields.get(19)?.parse().ok()
}
pub fn read_record(root: &Path) -> Result<Option<Value>> {
    let path = root.join(RECORD);
    if !files::exists(&path) {
        return Ok(None);
    }
    let file = files::open_at(rustix::fs::CWD, &path, false, true)?;
    let value: Value = serde_json::from_slice(&files::read_file(file, 4096)?)?;
    require(
        value["app"] == "flightdeck-desktop-service"
            && value["schema"] == 1
            && value["uid"] == files::uid()
            && value["pid"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= u32::MAX as u64)
            && value["start"].as_u64().is_some()
            && value["port"]
                .as_u64()
                .is_some_and(|n| (1..=65535).contains(&n))
            && value["token"].as_str().is_some_and(|s| {
                (32..=128).contains(&s.len())
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            && (value.get("release").is_none()
                || value["release"].as_str().is_some_and(files::hex_digest)),
        "Der gespeicherte Hintergrunddienst-Eintrag ist ungültig. Bitte den Einstellungsordner prüfen.",
    )?;
    Ok(Some(value))
}
pub fn write_record(root: &Path, service: &Service) -> Result<()> {
    read_record(root)?;
    let start = process_start(std::process::id()).ok_or(Error::Invalid(UNVERIFIED))?;
    files::atomic_json(
        &root.join(RECORD),
        &json!({"app":"flightdeck-desktop-service","schema":1,"uid":files::uid(),"pid":std::process::id(),"start":start,"port":service.port,"token":service.token,"release":resources::release_identity()}),
    )
}
pub fn request(record: &Value, path: &str, data: Option<&Value>) -> Option<Value> {
    let port = record["port"].as_u64()?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let url = format!("http://127.0.0.1:{port}{path}");
    let response = if let Some(data) = data {
        client
            .post(url)
            .header("Origin", format!("http://127.0.0.1:{port}"))
            .header("X-Flightdeck-Token", record["token"].as_str()?)
            .json(data)
            .send()
            .ok()?
    } else {
        client
            .get(url)
            .header("X-Flightdeck-Token", record["token"].as_str()?)
            .send()
            .ok()?
    };
    if response.status() != 200 {
        return None;
    }
    use std::io::Read;
    let mut data = Vec::new();
    response.take(65537).read_to_end(&mut data).ok()?;
    if data.len() > 65536 {
        return None;
    }
    serde_json::from_slice(&data).ok()
}
fn service_locked(root: &Path) -> bool {
    files::Lease::acquire(&root.join("desktop-service.lock"), true).is_err()
}
pub fn verified(root: &Path) -> Result<Option<Value>> {
    let Some(record) = read_record(root)? else {
        return Ok(None);
    };
    let pid = record["pid"].as_u64().unwrap_or(0) as u32;
    let start = record["start"].as_u64();
    if process_start(pid) != start || !service_locked(root) {
        return Ok(None);
    }
    let Some(reply) = request(&record, "/api/status", None) else {
        return Ok(None);
    };
    if reply["app"]["name"] != "Flightdeck"
        || !reply["csrf_token"].as_str().is_some_and(|s| {
            s.as_bytes()
                .ct_eq(record["token"].as_str().unwrap_or("").as_bytes())
                .unwrap_u8()
                == 1
        })
        || (record.get("release").is_some() && reply["service"]["release"] != record["release"])
        || process_start(pid) != start
    {
        return Ok(None);
    }
    Ok(Some(record))
}
pub fn ensure_service(root: &Path, runtime: Option<&str>, port: u16) -> Result<Value> {
    let executable = std::env::current_exe()?;
    ensure_service_with(root, runtime, port, resources::release_identity(), || {
        Command::new(&executable)
    })
}
fn ensure_service_with(
    root: &Path,
    runtime: Option<&str>,
    port: u16,
    identity: &str,
    mut command: impl FnMut() -> Command,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let _start = loop {
        match files::Lease::acquire(&root.join("desktop-start.lock"), true) {
            Ok(lease) => break lease,
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    };
    let old = read_record(root)?;
    let mut running = verified(root)?;
    if let Some(record) = &running
        && record["release"] != identity
    {
        let result = request(record, "/api/desktop/refresh", Some(&json!({})));
        if result
            .as_ref()
            .is_some_and(|r| r["ok"] == true && r["refresh"] == "restarting")
        {
            while service_locked(root) {
                require(
                    Instant::now() < deadline,
                    "Der bisherige Launcher wird noch beendet. Bitte Flightdeck gleich erneut öffnen.",
                )?;
                thread::sleep(Duration::from_millis(50));
            }
            running = None;
        } else if let Some(record) = running.as_mut() {
            record["update_pending"] = json!(true);
        }
    }
    if let Some(record) = running {
        if let Some(runtime) = runtime {
            require(
                request(
                    &record,
                    "/api/config",
                    Some(&json!({"runtime_path":runtime})),
                )
                .is_some_and(|r| r["ok"] == true),
                "Der laufende Launcher konnte die Runtime nicht wechseln. Bitte Spiel und laufende Einrichtung zuerst beenden.",
            )?;
        }
        return Ok(record);
    }
    require(!service_locked(root), UNVERIFIED)?;
    let preferred = if port != 0 {
        port
    } else {
        old.as_ref().and_then(|r| r["port"].as_u64()).unwrap_or(0) as u16
    };
    let choices = if preferred != 0 && port == 0 {
        vec![preferred, 0]
    } else {
        vec![preferred]
    };
    for port in choices {
        let mut command = command();
        command
            .args(["--desktop-service", "--state-dir"])
            .arg(root)
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        if let Some(runtime) = runtime {
            command.args(["--runtime", runtime]);
        }
        let mut child = process::spawn(&mut command, None)?;
        let until = Instant::now() + Duration::from_secs(60);
        while Instant::now() < until && child.try_wait()?.is_none() {
            if let Some(record) = verified(root)?
                && record["release"] == identity
            {
                thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(record);
            }
            thread::sleep(Duration::from_millis(75));
        }
        process::terminate(&mut child)?;
        require(!service_locked(root), UNVERIFIED)?;
    }
    Err(Error::Invalid(
        "Der Launcher konnte nicht im Hintergrund starten. Bitte den Einstellungsordner prüfen; flightdeck --no-browser zeigt Details.",
    ))
}
pub fn language(root: &Path, explicit: Option<&str>) -> String {
    let saved = files::json::<Value>(&root.join("ui-preferences.json"), 4096).ok();
    crate::cli::locale(explicit.or_else(|| saved.as_ref().and_then(|v| v["language"].as_str())))
}
pub fn open_interface(root: &Path, record: &Value, language: Option<&str>) -> Result<String> {
    require(
        ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty())),
        "Das native Flightdeck-Fenster konnte nicht geöffnet werden. Starte Flightdeck in einer X11- oder Wayland-Sitzung.",
    )?;
    let client = flightdeck_ui::Client::new(
        record["port"]
            .as_u64()
            .ok_or(Error::Invalid("Invalid local service port."))? as u16,
        record["token"]
            .as_str()
            .ok_or(Error::Invalid("Invalid local service identity."))?
            .to_string(),
    )
    .map_err(|_| Error::Invalid("Der lokale Flightdeck-Dienst konnte nicht verbunden werden."))?;
    let language = self::language(root, language);
    let state = root.to_path_buf();
    let connector: flightdeck_ui::Connector = Arc::new(move || {
        let record = verified(&state)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "Der lokale Dienst ist nicht erreichbar. Bitte Flightdeck erneut öffnen."
                    .to_string()
            })?;
        flightdeck_ui::Client::new(
            record["port"].as_u64().unwrap_or(0) as u16,
            record["token"].as_str().unwrap_or("").to_string(),
        )
    });
    flightdeck_ui::run(client, if language == "en" {flightdeck_ui::Language::En} else {flightdeck_ui::Language::De}, Some(connector))
        .map_err(|_| Error::Invalid("Das native Flightdeck-Fenster konnte nicht geöffnet werden. Starte Flightdeck in einer X11- oder Wayland-Sitzung."))?;
    Ok("native".into())
}
pub fn start(root: &Path, runtime: Option<&str>, port: u16, language: Option<&str>) -> Result<()> {
    let root = state_directory(root)?;
    let record = ensure_service(&root, runtime, port)?;
    open_interface(&root, &record, language)?;
    Ok(())
}
/// The updater waits for this coordinator, not for the lifetime of a GUI window.
/// Start a new frontend only after the new service has been verified.
pub fn handoff(root: &Path, port: u16) -> Result<()> {
    let record = ensure_service(root, None, port)?;
    require(
        record["update_pending"] != true,
        "Bitte beende Spiel und Einrichtung vor dem Launcher-Neustart.",
    )?;
    let mut child = process::spawn(
        Command::new(std::env::current_exe()?)
            .arg("--desktop")
            .arg("--state-dir")
            .arg(root)
            .arg("--port")
            .arg(record["port"].as_u64().unwrap_or(0).to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0),
        None,
    )?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
/// The running release owns the authenticated handoff, including a rollback to
/// a release whose client predates authentication of service status reads.
pub fn installed_handoff(
    root: &Path,
    port: u16,
    installation: &Path,
    expected: &str,
) -> Result<()> {
    let root = state_directory(root)?;
    let installation = installer::absolute(installation)?;
    installer::no_links(&installation)?;
    files::directory(&installation, false)?;
    // Keep the selected, verified release stable until its service is ready.
    let _install = files::Lease::acquire(&installation.join(".install.lock"), true)?;
    let state = installer::load(&installation)?.ok_or(Error::Invalid(UNVERIFIED))?;
    require(
        files::hex_digest(expected) && state["current"] == expected,
        "Flightdeck wurde inzwischen geändert. Bitte den Launcher neu öffnen.",
    )?;
    let source = installer::verify_release(&installation, expected)?;
    let native = source.join("bin/flightdeck").is_file();
    // Installation IDs hash the complete package manifest. Service IDs instead
    // identify the native executable or the legacy release's own code set.
    let identity = if native {
        files::digest(fs::File::open(source.join("bin/flightdeck"))?)?
    } else {
        let output = process::output(
            Command::new("python3")
                .args([
                    "-B",
                    "-c",
                    "from flightdeck.desktop import release_identity; print(release_identity())",
                ])
                .current_dir(&source)
                .env_remove("PYTHONHOME")
                .env_remove("PYTHONPATH"),
            Duration::from_secs(20),
            128,
            &AtomicBool::new(false),
        )?;
        let identity = std::str::from_utf8(&output)
            .map_err(|_| Error::Invalid(UNVERIFIED))?
            .trim()
            .to_string();
        require(files::hex_digest(&identity), UNVERIFIED)?;
        identity
    };
    let selected_command = || {
        let mut command = if native {
            Command::new(source.join("bin/flightdeck"))
        } else {
            let mut command = Command::new("python3");
            command
                .args(["-B", "-m", "flightdeck"])
                .env_remove("PYTHONHOME")
                .env_remove("PYTHONPATH");
            command
        };
        command.current_dir(&source);
        command
    };
    let record = ensure_service_with(&root, None, port, &identity, selected_command)?;
    require(
        record["update_pending"] != true,
        "Bitte beende Spiel und Einrichtung vor dem Launcher-Neustart.",
    )?;
    if native {
        let mut child = process::spawn(
            selected_command()
                .arg("--desktop")
                .arg("--state-dir")
                .arg(&root)
                .arg("--port")
                .arg(record["port"].as_u64().unwrap_or(0).to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0),
            None,
        )?;
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
    Ok(())
}
