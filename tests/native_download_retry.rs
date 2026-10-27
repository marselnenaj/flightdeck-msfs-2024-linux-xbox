// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{
    backend::Launcher, files, game_install, games::Game, problem_reports, setup, setup_diagnostics,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

struct Download {
    temp: tempfile::TempDir,
    app: Arc<Launcher>,
    id: String,
    worker: Option<thread::JoinHandle<bool>>,
}

impl Download {
    fn start(body: &str, features: Value, sign_in: bool, expected_package: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cli = temp.path().join("xodus-cli");
        let success = format!(
            r#"
printf '%s' '<Game><Executable Name="FlightSimulator2024.exe"/><StoreId>{}</StoreId></Game>' > "$3/MicrosoftGame.Config"
printf 'encrypted fixture' > "$3/FlightSimulator2024.exe"
printf 'complete fixture' > "$3/.xodus-streaming.msixvc"
exit 0
"#,
            Game::Msfs2024.store_id()
        );
        let script = format!(
            "#!/bin/sh\nset -eu\n{}\n",
            body.replace("@SUCCESS@", &success)
        );
        fs::write(&cli, &script).unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        let digest = files::sha256(script.as_bytes());
        let app = Launcher::new(temp.path().join("state"), None).unwrap();
        let ctx = app.reserve("setup", "install", false).unwrap();
        ctx.update(json!({"mode":"install","game_id":"msfs2024","state":"installing"}));
        let id = ctx.id.clone();
        let destination = temp.path().join("game");
        let xdg = temp.path().join("xdg");
        let package = "a".repeat(64);
        let worker = thread::spawn(move || {
            let result = game_install::download(
                game_install::Request {
                    cli: &cli,
                    expected: &digest,
                    destination: &destination,
                    market: "AT",
                    xdg: &xdg,
                    features: &features,
                    sign_in,
                    expected_package: expected_package.then_some(package.as_str()),
                    game: Game::Msfs2024,
                },
                &ctx,
            );
            let success = result.is_ok();
            ctx.launcher.finish(
                &ctx,
                result.map(|_| json!({"state":"complete","phase":"complete"})),
                false,
            );
            success
        });
        Self {
            temp,
            app,
            id,
            worker: Some(worker),
        }
    }
    fn wait(&self, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let job = self.app.job("setup");
            if job["state"] == state {
                return job;
            }
            assert!(Instant::now() < deadline, "timed out: {job}");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn join(&mut self) -> bool {
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.app.cancel("setup", &self.id);
            let _ = worker.join();
        }
    }
}
fn resume() -> Value {
    json!(["streaming-resume-files-v1"])
}

#[test]
fn xbox_exchange_failure_is_reported_without_repeating_successful_browser_sign_in() {
    let mut run = Download::start(
        "case \"$1\" in login) printf 'login\\n' >> login-calls; exit 0;; esac\nexit 94",
        resume(),
        true,
        false,
    );
    let failed = run.wait("failed");
    assert_eq!(failed["login_exit_code"], 0);
    assert_eq!(failed["download_exit_code"], 94);
    assert_eq!(failed["download_attempt"], 1);
    assert_eq!(failed["can_retry"], true);
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("Xbox-Authentifizierung")
    );
    assert_eq!(
        fs::read_to_string(run.temp.path().join("login-calls")).unwrap(),
        "login\n"
    );
    assert_eq!(run.app.lock().installation_report["download_exit_code"], 94);
    run.app.cancel("setup", &run.id).unwrap();
    assert!(!run.join());
}

#[test]
fn failed_download_retries_in_the_same_directory_and_exports_evidence_before_installation() {
    let mut run = Download::start(
        r#"
case "$*" in *'--resume-files'*) ;; *) exit 99;; esac
if [ ! -f "$3/completed.bin" ]; then
  printf 'verified content' > "$3/completed.bin"
  exit 42
fi
test "$(cat "$3/completed.bin")" = 'verified content'
@SUCCESS@
"#,
        resume(),
        false,
        false,
    );
    let failed = run.wait("failed");
    assert_eq!(failed["download_exit_code"], 42);
    assert_eq!(failed["can_retry"], true);
    assert!(run.app.lock().active.is_some());
    assert!(setup::download_action(&run.app, "stale", "retry").is_err());
    assert!(setup::download_action(&run.app, &run.id, "resume").is_err());
    let draft = problem_reports::prepare(&run.app, &json!({
        "category":"installation", "description":"The initial download failed after sign-in.", "runtime_path":null
    })).unwrap();
    let summary = &draft["draft"]["report"]["diagnostics"]["summary"];
    assert_eq!(summary["context"]["game_id"], Value::Null);
    assert_eq!(summary["installation"]["game_id"], "msfs2024");
    assert_eq!(summary["installation"]["failure_phase"], "download");
    assert_eq!(summary["installation"]["download_exit_code"], 42);
    assert_eq!(summary["installation"]["download_attempt"], 1);
    assert!(
        !summary
            .to_string()
            .contains(run.temp.path().to_str().unwrap())
    );
    setup::download_action(&run.app, &run.id, "retry").unwrap();
    assert!(run.join());
    assert_eq!(run.app.job("setup")["download_attempt"], 2);
    assert_eq!(run.app.job("setup")["download_exit_code"], 0);
    assert!(run.app.lock().active.is_none());
    assert_eq!(
        fs::read(run.temp.path().join("game/completed.bin")).unwrap(),
        b"verified content"
    );
}

#[test]
fn cancelling_a_failed_download_releases_its_reservation_and_retains_numeric_failure_after_restart()
{
    let mut run = Download::start(
        "printf 'keep' > \"$3/complete.bin\"\nexit 42",
        resume(),
        false,
        false,
    );
    run.wait("failed");
    run.app.cancel("setup", &run.id).unwrap();
    assert!(!run.join());
    assert!(run.app.lock().active.is_none());
    assert_eq!(run.app.job("setup")["can_retry"], false);
    assert!(setup::download_action(&run.app, &run.id, "retry").is_err());
    let restored = Launcher::new(run.temp.path().join("state"), None).unwrap();
    let report = &restored.lock().installation_report;
    assert_eq!(report["download_exit_code"], 42);
    assert_eq!(report["failure_phase"], "download");
    assert_eq!(
        fs::read(run.temp.path().join("game/complete.bin")).unwrap(),
        b"keep"
    );
}

#[test]
fn an_expired_session_does_not_reopen_sign_in_indefinitely() {
    let mut run = Download::start(
        r#"
if [ "$1" = login ]; then
  printf 'login\n' >> "$XDG_DATA_HOME/logins"
  exit 0
fi
if [ ! -f "$3/first" ]; then touch "$3/first"; exit 77; fi
if [ ! -f "$3/second" ]; then touch "$3/second"; exit 77; fi
@SUCCESS@
"#,
        resume(),
        false,
        false,
    );
    let job = run.wait("failed");
    assert_eq!(job["download_exit_code"], 77);
    assert_eq!(
        fs::read_to_string(run.temp.path().join("xdg/data/logins")).unwrap(),
        "login\n"
    );
    setup::download_action(&run.app, &run.id, "retry").unwrap();
    assert!(run.join());
    assert_eq!(run.app.job("setup")["download_attempt"], 3);
}

#[test]
fn failed_initial_sign_in_can_be_retried_without_replacing_the_download_folder() {
    let mut run = Download::start(
        r#"
if [ "$1" = login ]; then
  if [ ! -f "$XDG_DATA_HOME/first-login" ]; then touch "$XDG_DATA_HOME/first-login"; exit 71; fi
  exit 0
fi
@SUCCESS@
"#,
        resume(),
        true,
        false,
    );
    let job = run.wait("failed");
    assert_eq!(job["failure_phase"], "authentication");
    assert_eq!(job["login_exit_code"], 71);
    assert_eq!(job["download_attempt"], 0);
    setup::download_action(&run.app, &run.id, "retry").unwrap();
    assert!(run.join());
    assert_eq!(run.app.job("setup")["login_exit_code"], 0);
}

#[test]
fn package_revision_mismatch_requires_a_fresh_check_and_does_not_offer_retry() {
    let mut run = Download::start(
        "exit 78",
        json!(["streaming-resume-files-v1", "package-info-json-v1"]),
        false,
        true,
    );
    assert!(!run.join());
    let job = run.app.job("setup");
    assert_eq!(job["state"], "failed");
    assert_eq!(job["download_exit_code"], 78);
    assert_eq!(job["can_retry"], false);
    assert!(run.app.lock().active.is_none());
}

#[test]
fn old_components_do_not_advertise_file_reuse_and_still_report_the_download_code() {
    let mut run = Download::start("exit 42", json!([]), false, false);
    assert!(!run.join());
    let job = run.app.job("setup");
    assert_eq!(job["state"], "failed");
    assert_eq!(job["can_retry"], false);
    assert_eq!(run.app.lock().installation_report["download_exit_code"], 42);
}

#[test]
fn retry_rechecks_the_downloader_hash_before_executing_it_again() {
    let mut run = Download::start("exit 42", resume(), false, false);
    run.wait("failed");
    fs::write(run.temp.path().join("xodus-cli"), b"#!/bin/sh\nexit 0\n").unwrap();
    setup::download_action(&run.app, &run.id, "retry").unwrap();
    assert!(!run.join());
    assert_eq!(run.app.job("setup")["can_retry"], false);
    assert_eq!(run.app.job("setup")["download_attempt"], 1);
    assert!(
        !run.temp
            .path()
            .join("game/FlightSimulator2024.exe")
            .exists()
    );
}

#[test]
fn a_zero_exit_after_retry_does_not_replace_package_validation() {
    let mut run = Download::start(
        "if [ ! -f \"$3/first\" ]; then touch \"$3/first\"; exit 42; fi\nexit 0",
        resume(),
        false,
        false,
    );
    run.wait("failed");
    setup::download_action(&run.app, &run.id, "retry").unwrap();
    assert!(!run.join());
    let job = run.app.job("setup");
    assert_eq!(job["state"], "failed");
    assert_eq!(job["download_exit_code"], 0);
    assert_eq!(job["can_retry"], false);
    assert!(run.app.lock().active.is_none());
}

#[test]
fn setup_evidence_is_a_closed_projection_and_rejects_private_or_malformed_values() {
    let private = "/private/account/token";
    let job = json!({"id":"a".repeat(32),"mode":"install","game_id":"msfs2024","state":"failed","phase":"download_failed","failure_phase":"download", "download_exit_code":42, "login_exit_code":0, "download_attempt":1, "message":private,"error":private,"workspace_path":private,"url":private,"token":private,"recorded_at":private});
    let projected = setup_diagnostics::summary(&job);
    assert_eq!(projected["download_exit_code"], 42);
    assert!(!projected.to_string().contains(private));
    for (key, invalid) in [
        ("mode", json!(private)),
        ("phase", json!(private)),
        ("download_exit_code", json!(256)),
        ("download_attempt", json!(-1)),
    ] {
        let mut bad = job.clone();
        bad[key] = invalid;
        assert!(setup_diagnostics::summary(&bad).get(key).is_none());
    }
    assert!(setup_diagnostics::summary(&json!({"id":private})).is_null());
    let temp = tempfile::tempdir().unwrap();
    setup_diagnostics::persist(temp.path(), &job);
    assert_eq!(
        setup_diagnostics::load(temp.path())["failure_phase"],
        "download"
    );
    let mode = fs::metadata(temp.path().join("setup-attempt.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0);
}

#[test]
fn preflight_failure_is_retained_even_when_the_check_requested_a_ready_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let app = Launcher::new(temp.path().join("state"), None).unwrap();
    let ctx = app.reserve("setup", "check", false).unwrap();
    ctx.update(json!({"mode":"install","game_id":"msfs2020","phase":"paths"}));
    app.finish(
        &ctx,
        Err(flightdeck::Error::Invalid("Synthetic preflight failure.")),
        true,
    );
    let report = &app.lock().installation_report;
    assert_eq!(report["game_id"], "msfs2020");
    assert_eq!(report["state"], "failed");
    assert_eq!(report["failure_phase"], "paths");
    assert!(report.get("download_exit_code").is_none());
}
