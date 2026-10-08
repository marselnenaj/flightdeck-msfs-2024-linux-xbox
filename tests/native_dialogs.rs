//! Real bounded child-process results, with no desktop, account or game access.
use flightdeck::dialog;
use flightdeck_ui::platform::{DIALOG_FAILED, DIALOG_INVALID, DIALOG_TIMEOUT};
use std::{
    process::Command,
    time::{Duration, Instant},
};

fn shell(script: &str, timeout: Duration) -> flightdeck::Result<Option<String>> {
    dialog::select(Command::new("/bin/sh").args(["-c", script]), timeout)
}

#[test]
fn selection_and_user_cancellation_remain_distinct_from_failed_dialogs() {
    let timeout = Duration::from_secs(2);
    assert_eq!(
        shell("printf '/tmp/report with spaces .json\\n'", timeout).expect("selected"),
        Some("/tmp/report with spaces .json".into())
    );
    assert_eq!(shell("exit 1", timeout).expect("cancelled"), None);
    for script in [
        "echo 'private failure detail' >&2; exit 127",
        "exit 255",
        "kill -TERM $$",
    ] {
        assert_eq!(
            shell(script, timeout)
                .expect_err("failed dialog")
                .to_string(),
            DIALOG_FAILED
        );
    }
    assert_eq!(
        shell("printf 'relative-path\\n'", timeout)
            .expect_err("invalid path")
            .to_string(),
        DIALOG_INVALID
    );
    let missing = dialog::select(
        &mut Command::new("/nonexistent/flightdeck-test-dialog"),
        timeout,
    );
    assert_eq!(
        missing.expect_err("missing program").to_string(),
        DIALOG_FAILED
    );
}

#[test]
fn timed_out_dialog_is_stopped_and_reported_as_an_error() {
    let started = Instant::now();
    let result = shell("exec sleep 30", Duration::from_millis(30));
    assert_eq!(result.expect_err("timeout").to_string(), DIALOG_TIMEOUT);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn unsupported_socket_entrypoint_stops_before_creating_a_background_service() {
    let temp = tempfile::tempdir().expect("isolated state");
    let state = temp.path().join("never-created");
    let output = Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"))
        .args(["--desktop", "--language", "en", "--state-dir"])
        .arg(&state)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("WAYLAND_SOCKET", "99999")
        .env("PATH", "") // No graphical error dialog may run in this test.
        .output()
        .expect("launcher process");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("WAYLAND_SOCKET"));
    assert!(
        !state.exists(),
        "no service/state may be created for an unusable session"
    );
}
