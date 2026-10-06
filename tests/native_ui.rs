// SPDX-License-Identifier: MIT
//! End-to-end native GUI client contract against a real isolated Rust service.
use flightdeck_ui::{Client, Request};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Service(Child);
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn native_client_uses_authenticated_context_bound_service_and_persists_language() {
    let temp = tempfile::tempdir().expect("temp");
    let state = temp.path().join("state");
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
}
