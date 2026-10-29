// SPDX-License-Identifier: MIT
//! The native service's HTTP contract, exercised with synthetic profiles over
//! real loopback HTTP. No game, account or network is involved.
use crate::{
    ensure,
    http::{self, Response},
    util::{Failure, Result, require},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

enum Body<'a> {
    None,
    Json(Value),
    Raw(&'a [u8]),
}

struct Client {
    port: u16,
    token: String,
}
impl Client {
    /// A request without body, headers of its own or an explicit timeout is
    /// the only one that carries the session token implicitly.
    fn send(
        &self,
        path: &str,
        body: Body<'_>,
        headers: Option<&[(&str, &str)]>,
        seconds: u64,
    ) -> Result<Response> {
        let encoded;
        let payload = match &body {
            Body::None => None,
            Body::Json(value) => {
                encoded = value.to_string().into_bytes();
                Some(&encoded[..])
            }
            Body::Raw(raw) => Some(*raw),
        };
        let mut supplied = vec![("Accept-Language", "en")];
        match headers {
            None if payload.is_none() => supplied.push(("X-Flightdeck-Token", &self.token)),
            None => {}
            Some(headers) => supplied.extend_from_slice(headers),
        }
        if payload.is_some()
            && !supplied
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
        {
            supplied.push(("Content-Type", "application/json"));
        }
        let method = if payload.is_some() { "POST" } else { "GET" };
        http::request(
            self.port,
            method,
            path,
            &supplied,
            payload,
            Duration::from_secs(seconds),
        )
    }
    fn get(&self, path: &str) -> Result<Response> {
        self.send(path, Body::None, None, 5)
    }
    fn status(&self, path: &str, body: Body<'_>, headers: Option<&[(&str, &str)]>) -> Result<u16> {
        Ok(self.send(path, body, headers, 5)?.status)
    }
}

fn directory(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    Ok(())
}

fn executable(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Polls a job until it leaves `while_state`, for at most five seconds.
fn settled(client: &Client, path: &str, while_state: &str) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let job = client.get(path)?.json()["job"].clone();
        if job["state"] != while_state || Instant::now() >= deadline {
            return Ok(job);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn checks(client: &Client, work: &Path) -> Result<usize> {
    let token = client.token.as_str();
    let safe: &[(&str, &str)] = &[("X-Flightdeck-Token", token)];
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut checks = 0;
    // The first status hashes the executable for the service identity. An
    // unoptimized debug binary exceeds 400 MiB and needs around 16 seconds on
    // CI. Only startup gets this budget; the steady-state contract keeps five.
    let first = client.send("/api/status", Body::None, None, 30)?;
    let initial = first.json();
    ensure!(first.status == 200 && initial["app"]["name"] == "Flightdeck");
    ensure!(initial["runtime"]["configured"] == false && initial["game"]["can_start"] == false);
    ensure!(first.headers.get("cache-control").map(String::as_str) == Some("no-store"));
    ensure!(initial["csrf_token"] == token);
    checks += 4;
    for path in [
        "/api/status",
        "/api/diagnostics",
        "/api/problem-reports",
        "/api/unknown",
    ] {
        for supplied in [&[][..], &[("X-Flightdeck-Token", "invalid")][..]] {
            let denied = client.send(path, Body::None, Some(supplied), 5)?;
            ensure!(
                denied.status == 403 && denied.json().get("csrf_token").is_none(),
                "{path}"
            );
            checks += 1;
        }
    }
    // Duplicate credentials must fail even when both values are correct.
    for (method, path) in [("GET", "/api/status"), ("POST", "/api/preferences")] {
        let duplicate = [
            ("X-Flightdeck-Token", token),
            ("X-Flightdeck-Token", token),
            ("Content-Type", "application/json"),
        ];
        let response = http::request(
            client.port,
            method,
            path,
            &duplicate,
            Some(b"{}"),
            Duration::from_secs(5),
        )?;
        ensure!(response.status == 403, "{method} {path}");
        checks += 1;
    }
    for path in [
        "/api/cloud-saves",
        "/api/launcher-update",
        "/api/store-check",
        "/api/problem-reports",
        "/api/maintenance",
        "/api/proton",
        "/api/fenix",
        "/api/gsx",
        "/api/navigraph",
        "/api/orbx",
        "/api/mobiflight",
        "/api/mods",
        "/api/game-update",
        "/api/diagnostics",
    ] {
        let response = client.get(path)?;
        ensure!(
            response.status == 200 && response.json().is_object(),
            "{path}"
        );
        checks += 1;
    }
    let post = |path: &str, value: Value| client.status(path, Body::Json(value), Some(safe));
    ensure!(post("/api/launcher-update/install", json!({"check_id": "stale"}))? == 409);
    ensure!(post("/api/launcher-update/rollback", json!({}))? == 409);
    ensure!(post("/api/launcher-update/restart", json!({}))? == 409);
    ensure!(post("/api/store-check/cancel", json!({"job_id": "stale"}))? == 409);
    ensure!(
        post(
            "/api/cloud-saves/cancel-auto",
            json!({"request_id": "stale"})
        )? == 409
    );
    ensure!(
        post(
            "/api/problem-reports/discard",
            json!({"report_id": "stale"})
        )? == 409
    );
    let duplicate = br#"{"runtime_path":"a","runtime_path":"b"}"#;
    ensure!(client.status("/api/config", Body::Raw(duplicate), Some(safe))? == 400);
    checks += 7;
    for header in [
        ("Host", "example.invalid"),
        ("Origin", "https://example.invalid"),
        ("Sec-Fetch-Site", "cross-site"),
    ] {
        ensure!(
            client.status("/api/status", Body::None, Some(&[header]))? == 403,
            "{}",
            header.0
        );
        checks += 1;
    }
    ensure!(
        client.status(
            "/api/config",
            Body::Json(json!({"runtime_path": text(work)})),
            None
        )? == 403
    );
    let invalid = [("X-Flightdeck-Token", "invalid")];
    ensure!(client.status("/api/config", Body::Json(json!({})), Some(&invalid))? == 403);
    ensure!(post("/api/config", json!([]))? == 400);
    ensure!(client.status("/api/config", Body::Raw(b"{"), Some(safe))? == 400);
    let plain = [
        ("X-Flightdeck-Token", token),
        ("Content-Type", "text/plain"),
    ];
    ensure!(client.status("/api/config", Body::Json(json!({})), Some(&plain))? == 415);
    ensure!(client.status("/api/config", Body::Raw(&[b' '; 16385]), Some(safe))? == 413);
    ensure!(client.get("/private/config.json")?.status == 404);
    ensure!(client.get("/../Cargo.toml")?.status == 404);
    checks += 8;
    // The production service exposes API data, never executable web UI.
    for name in ["index.html", "app.js", "styles.css", "i18n.js"] {
        ensure!(client.get(&format!("/{name}"))?.status == 404, "{name}");
        checks += 1;
    }
    let runtime = work.join("runtime with spaces");
    for relative in [
        "private",
        "tools",
        "games/MSFS2024",
        "local/msfs-prefix/drive_c/windows/system32",
    ] {
        directory(&runtime.join(relative))?;
    }
    for relative in [
        "tools/play-msfs.sh",
        "games/MSFS2024/FlightSimulator2024.exe",
        "local/msfs-prefix/system.reg",
        "local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll",
    ] {
        fs::write(runtime.join(relative), "synthetic fixture\n")?;
    }
    fs::set_permissions(
        runtime.join("tools/play-msfs.sh"),
        fs::Permissions::from_mode(0o700),
    )?;
    ensure!(post("/api/config", json!({"runtime_path": text(&runtime)}))? == 200);
    let configured = client.get("/api/status")?.json();
    ensure!(configured["runtime"]["configured"] == true && configured["runtime"]["ready"] == true);
    ensure!(configured["versions"]["msfs2024"]["path"] == text(&runtime));
    // An installed edition with failed readiness checks must remain selectable
    // for repair and never be treated as a new installation.
    let other = work.join("msfs2020");
    for name in ["tools", "private"] {
        directory(&other.join(name))?;
    }
    executable(&other.join("tools/play-msfs.sh"), "#!/bin/sh\nexit 99\n")?;
    fs::write(
        other.join("private/runtime.json"),
        json!({"game_id": "msfs2020"}).to_string(),
    )?;
    ensure!(post("/api/config", json!({"runtime_path": text(&other)}))? == 200);
    ensure!(post("/api/game/select", json!({"game_id": "msfs2024"}))? == 200);
    ensure!(post("/api/game/select", json!({"game_id": "msfs2020"}))? == 200);
    let selected = client.get("/api/status")?.json();
    ensure!(selected["runtime"]["path"] == text(&other));
    ensure!(selected["versions"]["msfs2020"]["installed"] == true);
    ensure!(selected["runtime"]["ready"] == false);
    ensure!(selected["game"]["can_start"] == false);
    ensure!(post("/api/launch", json!({}))? == 409);
    let config: Value = serde_json::from_slice(&fs::read(work.join("state/config.json"))?)?;
    ensure!(config["runtime_path"] == text(&other));
    ensure!(post("/api/game/select", json!({"game_id": "msfs2024"}))? == 200);
    checks += 10;
    // The same edition can be selected independently from Steam. Verify both
    // storefront admission and stale physical-profile bindings over real HTTP.
    let steam = work.join("Steam client");
    let steam_game = steam.join("steamapps/common/Synthetic MSFS");
    let steam_prefix = steam.join("steamapps/compatdata/2537590/pfx");
    for path in [&steam_game, &steam_prefix, &steam.join("config")] {
        directory(path)?;
    }
    fs::write(
        steam.join("config/config.vdf"),
        "\"InstallConfigStore\" {}\n",
    )?;
    fs::write(
        steam.join("steamapps/appmanifest_2537590.acf"),
        "\"AppState\" {\"appid\" \"2537590\" \"installdir\" \"Synthetic MSFS\"}",
    )?;
    fs::write(steam_prefix.join("system.reg"), "synthetic Steam registry")?;
    ensure!(
        post(
            "/api/installations/add-steam-root",
            json!({"steam_root":steam,"client":"native"})
        )? == 200
    );
    let catalog = client.get("/api/installations")?.json();
    let entries = catalog["installations"]
        .as_array()
        .ok_or_else(|| Failure("No installation inventory".into()))?;
    let steam_item = entries
        .iter()
        .find(|i| i["source"] == "steam" && i["path"] == text(&steam_game))
        .ok_or_else(|| Failure("Steam MSFS was not detected".into()))?
        .clone();
    let xbox_item = entries
        .iter()
        .find(|i| i["source"] == "xbox" && i["context"] == text(&runtime))
        .ok_or_else(|| Failure("Xbox MSFS was lost".into()))?
        .clone();
    ensure!(
        post(
            "/api/installations/select",
            json!({"installation_id":steam_item["id"],"binding":steam_item["context"]})
        )? == 200
    );
    let selected = client.get("/api/status")?.json();
    ensure!(selected["runtime"]["source"] == "steam" && selected["cloud"]["enabled"] == false);
    let old_context = json!(text(&runtime)).to_string();
    let old_headers = [
        ("X-Flightdeck-Token", token),
        ("X-Flightdeck-Context", old_context.as_str()),
    ];
    ensure!(
        client.status(
            "/api/fenix/select",
            Body::Json(json!({})),
            Some(&old_headers)
        )? == 409
    );
    for path in [
        "/api/setup/check",
        "/api/cloud-saves/import",
        "/api/game-update/start",
        "/api/graphics",
        "/api/mods/preview-remove",
    ] {
        ensure!(
            post(path, json!({}))? == 409,
            "Steam admitted an Xbox action: {path}"
        );
        checks += 1;
    }
    let steam_context = selected["runtime"]["path"].to_string();
    fs::rename(
        &steam_prefix,
        steam_prefix.with_file_name("before-recreated-pfx"),
    )?;
    directory(&steam_prefix)?;
    fs::write(steam_prefix.join("system.reg"), "recreated Steam registry")?;
    let stale_headers = [
        ("X-Flightdeck-Token", token),
        ("X-Flightdeck-Context", steam_context.as_str()),
    ];
    ensure!(
        client.status(
            "/api/fenix/select",
            Body::Json(json!({})),
            Some(&stale_headers)
        )? == 409
    );
    ensure!(client.get("/api/status")?.json()["setup"]["busy"] == false);
    let legacy: Value = serde_json::from_slice(&fs::read(work.join("state/config.json"))?)?;
    ensure!(legacy["runtime_path"] == text(&runtime));
    ensure!(
        post(
            "/api/installations/select",
            json!({"installation_id":xbox_item["id"],"binding":xbox_item["context"]})
        )? == 200
    );
    ensure!(client.get("/api/status")?.json()["runtime"]["path"] == text(&runtime));
    checks += 8;
    ensure!(
        post(
            "/api/graphics",
            json!({"runtime_path": text(work), "nvidia_mode": "auto"})
        )? == 409
    );
    ensure!(
        post(
            "/api/vr/configure",
            json!({"runtime_path": text(&runtime), "mode": "off"})
        )? == 200
    );
    let setup = client.get("/api/setup")?.json();
    ensure!(
        setup["prepare_available"] == true && setup["defaults"]["runtime_path"] == text(&runtime)
    );
    let request = json!({"mode": "existing", "runtime_path": text(&runtime)});
    let checked = client.send("/api/setup/check", Body::Json(request), Some(safe), 5)?;
    ensure!(checked.status == 200);
    let check_id = checked.json()["job"]["id"].clone();
    let job = settled(client, "/api/setup", "checking")?;
    ensure!(job["state"] == "ready", "{job}");
    ensure!(post("/api/config", json!({"runtime_path": text(&runtime)}))? == 409);
    ensure!(post("/api/setup/start", json!({"check_id": "stale"}))? == 409);
    ensure!(post("/api/setup/start", json!({"check_id": check_id}))? == 200);
    let job = settled(client, "/api/setup", "installing")?;
    ensure!(job["state"] == "complete", "{job}");
    ensure!(client.get("/api/status")?.json()["setup"]["busy"] == false);
    checks += 8;
    let community = work.join("Community");
    let addon = community.join("synthetic-addon");
    directory(&addon)?;
    fs::write(
        addon.join("manifest.json"),
        json!({"title": "Synthetic add-on", "package_version": "1.0"}).to_string(),
    )?;
    fs::write(
        runtime.join("private/runtime.json"),
        json!({"game_id": "msfs2024", "community_path": text(&community)}).to_string(),
    )?;
    for operation in ["preview-remove", "remove", "discard-remove"] {
        let path = format!("/api/mods/{operation}");
        ensure!(
            client.status(&path, Body::Json(json!({})), None)? == 403,
            "{operation}"
        );
        let stale =
            json!({"runtime_path": text(&runtime), "job_id": "stale", "addon_id": "../outside"});
        ensure!(post(&path, stale)? == 409, "{operation}");
        checks += 2;
    }
    let preview = json!({"runtime_path": text(&runtime), "addon_id": "synthetic-addon"});
    ensure!(post("/api/mods/preview-remove", preview)? == 200);
    let wait_mod = |state: &str| -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let job = client.get("/api/mods")?.json()["job"].clone();
            if job["state"] == state {
                return Ok(job);
            }
            ensure!(
                job["state"] != "failed" && Instant::now() < deadline,
                "{job}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let reviewed = wait_mod("ready")?;
    ensure!(addon.exists() && reviewed["entry_path"] == text(&addon));
    ensure!(post("/api/mods/remove", json!({"job_id": "different-review"}))? == 409);
    ensure!(post("/api/mods/remove", json!({"job_id": reviewed["id"]}))? == 200);
    wait_mod("complete")?;
    ensure!(!addon.exists() && community.is_dir());
    checks += 5;
    let saves = runtime.join("private/local-saves/title");
    directory(&saves)?;
    fs::write(runtime.join("private/local-saves.enabled"), "")?;
    fs::write(saves.join("state.bin"), "synthetic local save")?;
    std::os::unix::fs::symlink(work.join("state/config.json"), saves.join("external"))?;
    let result = client.send("/api/saves/backup", Body::Json(json!({})), Some(safe), 5)?;
    let value = result.json();
    ensure!(result.status == 200 && value["files"] == 1);
    let backup = runtime
        .join("private/save-backups")
        .join(value["backup"]["name"].as_str().unwrap_or_default());
    ensure!(fs::read(backup.join("data/title/state.bin"))? == b"synthetic local save");
    ensure!(fs::symlink_metadata(backup.join("data/title/external")).is_err());
    checks += 8;
    Ok(checks)
}

/// Stops the service like a desktop session would and reports how it exited.
fn stop(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::INT);
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                child.kill()?;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    require(
        child.wait()?.code() == Some(0),
        "Native service did not shut down cleanly",
    )
}

pub fn check(binary: &Path) -> Result<()> {
    let checks = run(binary)?;
    println!("Rust HTTP: {checks} service, origin, asset, configuration and backup checks passed.");
    Ok(())
}

/// Starts the service in a private home and returns the number of passed checks.
pub fn run(binary: &Path) -> Result<usize> {
    let binary = fs::canonicalize(binary)?;
    let temporary = tempfile::Builder::new()
        .prefix("flightdeck-native-http-")
        .tempdir()?;
    let work = temporary.path();
    let mut command = Command::new(&binary);
    command
        .arg("--state-dir")
        .arg(work.join("state"))
        .arg("--no-browser")
        .env("HOME", work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(work.join("service.log"))?);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("FLIGHTDECK_") {
            command.env_remove(name);
        }
    }
    for name in ["CONFIG", "DATA", "CACHE", "STATE"] {
        command.env(format!("XDG_{name}_HOME"), work.join(name.to_lowercase()));
    }
    let mut child = command
        .spawn()
        .map_err(|error| Failure(format!("{}: {error}", binary.display())))?;
    let result = (|| {
        let stdout = child.stdout.take().expect("piped output");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = sender.send(line);
        });
        // An unoptimized debug binary announces after about 5 seconds on a
        // fast machine and has exceeded 15 seconds on a CI runner.
        let line = receiver
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| Failure("Native service did not announce its loopback listener".into()))?;
        let port = line
            .strip_prefix("Flightdeck: http://127.0.0.1:")
            .and_then(|rest| rest.strip_suffix('\n'))
            .filter(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|port| port.parse().ok())
            .ok_or_else(|| Failure("Unexpected service startup output".into()))?;
        let record: Value =
            serde_json::from_slice(&fs::read(work.join("state/desktop-service.json"))?)?;
        let client = Client {
            port,
            token: record["token"].as_str().unwrap_or_default().to_string(),
        };
        checks(&client, work)
    })();
    let stopped = stop(&mut child);
    let checks = result.inspect_err(|_| {
        if let Ok(log) = fs::read_to_string(work.join("service.log")) {
            let tail: Vec<&str> = log.lines().rev().take(20).collect();
            eprintln!("{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
    })?;
    stopped?;
    Ok(checks)
}
