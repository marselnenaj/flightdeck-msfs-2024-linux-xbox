// SPDX-License-Identifier: MIT
use flightdeck::{
    cloud_pipe::{Pipe, helper_error},
    process,
};
use serde_json::json;
use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
// An account-free protocol fixture. Python is only the deliberately adversarial
// test peer; the launcher and production helper transport are Rust.
fn pipe(mode: &str, cancel: Arc<AtomicBool>) -> Pipe {
    let script = r#"
import sys,struct,json,time
mode=sys.argv[1]
for i in range(3):
    header=sys.stdin.buffer.read(4)
    if not header: break
    n=struct.unpack('<I',header)[0]
    request=json.loads(sys.stdin.buffer.read(n))
    if mode=='timeout': time.sleep(.3); break
    if mode=='oversize': sys.stdout.buffer.write(struct.pack('<I',300000)); sys.stdout.buffer.flush(); break
    raw=b'{"ok":true,"ok":true}' if mode=='duplicate' else json.dumps({'ok':True,'body_bytes':3}).encode()
    data=struct.pack('<I',len(raw))+raw+b'abc'
    for b in data:
        sys.stdout.buffer.write(bytes([b]));sys.stdout.buffer.flush()
        if mode=='fragmented': time.sleep(.001)
"#;
    let child = process::spawn(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0),
        None,
    )
    .expect("fixture");
    Pipe::new(child, cancel).expect("pipe")
}
#[test]
fn fragmented_frames_and_cleanup_after_cancellation() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut pipe = pipe("fragmented", Arc::clone(&cancel));
    let (answer, body) = pipe
        .exchange(
            &json!({"op":"index"}),
            &[],
            3,
            Duration::from_secs(2),
            false,
        )
        .expect("fragments");
    assert_eq!(answer["ok"], true);
    assert_eq!(body, b"abc");
    cancel.store(true, Ordering::Release);
    assert_eq!(
        pipe.exchange(
            &json!({"op":"index"}),
            &[],
            3,
            Duration::from_secs(2),
            false
        )
        .expect_err("cancel")
        .code,
        "cancelled"
    );
    pipe.exchange(
        &json!({"op":"lease_release"}),
        &[],
        3,
        Duration::from_secs(2),
        true,
    )
    .expect("bounded cleanup ignores cancel");
}
#[test]
fn malformed_frames_poison_session_and_timeout_is_bounded() {
    for mode in ["duplicate", "oversize"] {
        let mut pipe = pipe(mode, Arc::new(AtomicBool::new(false)));
        assert!(
            pipe.exchange(
                &json!({"op":"index"}),
                &[],
                3,
                Duration::from_secs(1),
                false
            )
            .is_err()
        );
        assert_eq!(
            pipe.exchange(
                &json!({"op":"index"}),
                &[],
                3,
                Duration::from_secs(1),
                false
            )
            .expect_err("failed session")
            .code,
            "transport"
        );
    }
    let mut pipe = pipe("timeout", Arc::new(AtomicBool::new(false)));
    let start = Instant::now();
    assert_eq!(
        pipe.exchange(
            &json!({"op":"index"}),
            &[],
            3,
            Duration::from_millis(40),
            false
        )
        .expect_err("timeout")
        .code,
        "deadline"
    );
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[test]
fn numeric_network_failures_are_not_misreported_as_bad_login() {
    let e =
        helper_error(&json!({"error":"authentication","hresult":0x80072EE2u32,"token":"private"}));
    assert_eq!(e.code, "deadline");
    assert_eq!(e.native_hresult, Some(0x80072EE2));
    assert!(!format!("{e:?}").contains("private"));
    assert_eq!(
        helper_error(&json!({"error":"authentication","http_status":503})).code,
        "transport"
    );
    assert_eq!(
        helper_error(&json!({"error":"authentication","http_status":401,"hresult":0x80072EE2u32}))
            .code,
        "authentication"
    );
}
