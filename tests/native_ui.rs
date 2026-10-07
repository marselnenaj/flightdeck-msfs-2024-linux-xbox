// SPDX-License-Identifier: MIT
//! End-to-end native GUI client contract against a real isolated Rust service.
use flightdeck_ui::{Client, Request};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
struct Service(Child);
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct ReplacementService(PathBuf);
impl Drop for ReplacementService {
    fn drop(&mut self) {
        if let Ok(Some(record)) = flightdeck::desktop::verified(&self.0) {
            let _ = flightdeck::desktop::request(&record, "/api/desktop/refresh", Some(&json!({})));
        }
    }
}
#[test]
fn native_client_uses_authenticated_context_bound_service_and_persists_language() {
    let temp = tempfile::tempdir().expect("temp");
    let state = temp.path().join("state");
    let _replacement = ReplacementService(state.clone());
    let mut service = Service(
        Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"))
            .args(["--desktop-service", "--state-dir"])
            .arg(&state)
            .env("HOME", temp.path())
            .env("XDG_STATE_HOME", temp.path())
            .env("XDG_CONFIG_HOME", temp.path())
            .env("XDG_DATA_HOME", temp.path())
            .env("XDG_CACHE_HOME", temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("service"),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let record = loop {
        if let Ok(Some(record)) = flightdeck::desktop::verified(&state) {
            break record;
        }
        assert!(
            service.0.try_wait().expect("child").is_none(),
            "service exited"
        );
        assert!(Instant::now() < deadline, "service readiness deadline");
        std::thread::sleep(Duration::from_millis(50));
    };
    // A coordinator must validate the still-selected, intact target before it
    // asks the authenticated service to stop, even when invoked directly.
    let source = temp.path().join("source");
    let installation = temp.path().join("installed");
    let mut elf = vec![0; 64];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[18] = 62;
    let mut hashes = serde_json::Map::new();
    for (name, data) in [
        ("bin/flightdeck", elf.as_slice()),
        ("ui/mark.svg", b"<svg/>".as_slice()),
        ("LICENSE", b"MIT".as_slice()),
        ("THIRD-PARTY-NOTICES.txt", b"synthetic".as_slice()),
        (
            "RUST-STANDARD-LIBRARY-NOTICES.html",
            b"synthetic".as_slice(),
        ),
    ] {
        let path = source.join(name);
        flightdeck::files::private_dir(path.parent().expect("parent")).expect("directory");
        flightdeck::files::atomic(&path, data).expect("source file");
        hashes.insert(name.into(), json!(flightdeck::files::sha256(data)));
    }
    flightdeck::files::atomic_json(
        &source.join(flightdeck::installer::PACKAGE),
        &json!({"schema":1,"kind":"rust-launcher","version":flightdeck::VERSION,"files":hashes}),
    )
    .expect("package");
    let installed = flightdeck::installer::install(flightdeck::installer::Options {
        source: &source,
        root: &installation,
        bin_dir: &temp.path().join("bin"),
        applications_dir: &temp.path().join("applications"),
        desktop: false,
        language: "en",
        expected_current: None,
    })
    .expect("install synthetic target");
    let selected = installed["current"].as_str().expect("selected");
    let port = record["port"].as_u64().expect("port") as u16;
    assert!(
        flightdeck::desktop::installed_handoff(&state, port, &installation, &"a".repeat(64))
            .is_err()
    );
    assert_eq!(
        flightdeck::desktop::verified(&state).expect("live service"),
        Some(record.clone())
    );
    flightdeck::files::atomic(
        &installation.join("releases").join(selected).join("LICENSE"),
        b"tampered",
    )
    .expect("tamper fixture");
    assert!(flightdeck::desktop::installed_handoff(&state, port, &installation, selected).is_err());
    assert_eq!(
        flightdeck::desktop::verified(&state).expect("live service"),
        Some(record.clone())
    );
    // A loopback connection is not an OS-user credential. Every API read and
    // mutation must fail closed without exactly one valid private token.
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .expect("HTTP");
    let origin = format!("http://127.0.0.1:{}", record["port"]);
    let token = record["token"].as_str().expect("token");
    for path in ["status", "diagnostics", "problem-reports", "missing"] {
        for supplied in [vec![], vec!["invalid"], vec![token, token]] {
            let mut request = http.get(format!("{origin}/api/{path}"));
            for value in supplied {
                request = request.header("X-Flightdeck-Token", value);
            }
            let reply = request.send().expect("response");
            assert_eq!(reply.status(), 403, "{path}");
            assert!(!reply.text().expect("body").contains(token));
        }
    }
    for supplied in [vec![], vec!["invalid"], vec![token, token]] {
        let mut request = http
            .post(format!("{origin}/api/preferences"))
            .json(&json!({"language":"de"}));
        for value in supplied {
            request = request.header("X-Flightdeck-Token", value);
        }
        assert_eq!(request.send().expect("response").status(), 403);
    }
    let client = Client::new(
        record["port"].as_u64().expect("port") as u16,
        record["token"].as_str().expect("token").to_string(),
    )
    .expect("client");
    assert!(!format!("{client:?}").contains(record["token"].as_str().expect("token")));
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let discovery = client
            .discover("setup/discover", "en")
            .await
            .expect("discovery");
        assert!(discovery.is_object());
        let snapshot = client.snapshot("en", true).await.expect("native snapshot");
        assert_eq!(snapshot["status"]["runtime"]["configured"], false);
        for key in [
            "setup",
            "fenix",
            "gsx",
            "launcher-update",
            "cloud-saves",
            "diagnostics",
        ] {
            assert!(snapshot.contains_key(key));
        }
        let request = Request {
            path: "preferences",
            body: json!({"language":"en"}),
            runtime: String::new(),
            confirmation: None,
        };
        client.post(&request, "en").await.expect("preferences");
        let prefs: Value = flightdeck::files::json(&state.join("ui-preferences.json"), 4096)
            .expect("private preferences");
        assert_eq!(prefs["language"], "en");
        assert_eq!(flightdeck::desktop::language(&state, None), "en");
        assert_eq!(flightdeck::desktop::language(&state, Some("de")), "de");
        let stale = Request {
            runtime: "/synthetic/stale-installation".into(),
            body: json!({"language":"de"}),
            ..request
        };
        assert!(client.post(&stale, "en").await.is_err());
        let prefs: Value = flightdeck::files::json(&state.join("ui-preferences.json"), 4096)
            .expect("preferences unchanged");
        assert_eq!(prefs["language"], "en");
        let impostor = Client::new(
            record["port"].as_u64().expect("port") as u16,
            "wrong-session-token-0000000000000000".into(),
        )
        .expect("test identity");
        assert!(impostor.snapshot("en", false).await.is_err());
        assert!(impostor.discover("setup/discover", "en").await.is_err());
    });
    // Exercise a real selected release, whose manifest ID must not be confused
    // with the executable digest returned by its service. Stripping debug info
    // gives this target a distinct identity and fits the installer size limit.
    flightdeck::files::atomic(
        &installation.join("releases").join(selected).join("LICENSE"),
        b"MIT",
    )
    .expect("restore tampered fixture");
    let target_binary = source.join("bin/flightdeck");
    std::fs::copy(env!("CARGO_BIN_EXE_flightdeck-rust"), &target_binary).expect("copy binary");
    assert!(
        Command::new("strip")
            .arg("--strip-debug")
            .arg(&target_binary)
            .status()
            .expect("strip test binary")
            .success()
    );
    let binary_identity =
        flightdeck::files::digest(std::fs::File::open(&target_binary).expect("binary"))
            .expect("binary digest");
    let mut package: Value =
        flightdeck::files::json(&source.join(flightdeck::installer::PACKAGE), 65536)
            .expect("package");
    package["files"]["bin/flightdeck"] = json!(binary_identity);
    flightdeck::files::atomic_json(&source.join(flightdeck::installer::PACKAGE), &package)
        .expect("updated package");
    let installed = flightdeck::installer::install(flightdeck::installer::Options {
        source: &source,
        root: &installation,
        bin_dir: &temp.path().join("bin"),
        applications_dir: &temp.path().join("applications"),
        desktop: false,
        language: "en",
        expected_current: None,
    })
    .expect("install real target");
    let selected = installed["current"].as_str().expect("selected");
    assert_ne!(selected, binary_identity);
    assert_ne!(record["release"], binary_identity);
    let status = flightdeck::process::run(
        Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"))
            .args(["desktop-handoff", "--state-dir"])
            .arg(&state)
            .arg("--port")
            .arg(port.to_string())
            .arg("--installation-root")
            .arg(&installation)
            .arg("--expected-release")
            .arg(selected)
            .env("HOME", temp.path())
            .env("XDG_STATE_HOME", temp.path())
            .env("XDG_CONFIG_HOME", temp.path())
            .env("XDG_DATA_HOME", temp.path())
            .env("XDG_CACHE_HOME", temp.path())
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", ""),
        Duration::from_secs(25),
        &AtomicBool::new(false),
    )
    .expect("handoff deadline");
    assert!(status.success());
    let replacement = flightdeck::desktop::verified(&state)
        .expect("verify replacement")
        .expect("live replacement");
    assert_eq!(replacement["release"], binary_identity);
    assert_eq!(replacement["port"], record["port"]);
    assert_ne!(replacement["pid"], record["pid"]);
    assert_ne!(replacement["token"], record["token"]);
}
