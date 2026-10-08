// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{native_probe::HOST_LIBRARIES, process};
use serde_json::Value;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool, time::Duration};

fn compile(directory: &Path, name: &str, source: &str) {
    let input = directory.join("fixture.c");
    fs::write(&input, source).unwrap();
    let output = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"])
        .arg(&input)
        .arg("-o")
        .arg(directory.join(name))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn command(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_flightdeck-rust"));
    command
        .args(["--native-probe", "host"])
        .env("LD_LIBRARY_PATH", directory)
        .env("PATH", "/nonexistent-flightdeck-test-tools");
    command
}
fn probe(directory: &Path) -> Value {
    let output = process::output(
        &mut command(directory),
        Duration::from_secs(5),
        4096,
        &AtomicBool::new(false),
    )
    .unwrap();
    serde_json::from_slice(&output).unwrap()
}
fn fixture(directory: &Path) {
    compile(
        directory,
        "libfixture.so",
        "int fixture(void) { return 0; }\n",
    );
    for (_, soname) in HOST_LIBRARIES {
        fs::copy(directory.join("libfixture.so"), directory.join(soname)).unwrap();
    }
}
#[test]
fn host_probe_uses_actual_loader_paths_and_exact_openssl_abi_without_ldconfig() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    let ready = probe(temp.path());
    assert_eq!(ready["schema"], 1);
    assert_eq!(
        ready["libraries"].as_object().unwrap().len(),
        HOST_LIBRARIES.len()
    );
    for (id, _) in HOST_LIBRARIES {
        assert_eq!(ready["libraries"][id], true);
    }
    // A loadable older OpenSSL must not mask an unusable exact ABI. The host's
    // real libssl cannot satisfy this case because the first matching file is bad.
    fs::copy(
        temp.path().join("libfixture.so"),
        temp.path().join("libssl.so.1.1"),
    )
    .unwrap();
    fs::write(
        temp.path().join("libssl.so.3"),
        b"wrong ELF architecture or corrupt library",
    )
    .unwrap();
    let missing = probe(temp.path());
    assert_eq!(missing["libraries"]["openssl3"], false);
    assert_eq!(missing["libraries"]["crypto3"], true);
    assert!(!missing.to_string().contains(temp.path().to_str().unwrap()));
    // RTLD_NOW also rejects libraries whose required symbols cannot resolve.
    compile(
        temp.path(),
        "libssl.so.3",
        "extern int flightdeck_missing_dependency(void); int fixture(void) { return flightdeck_missing_dependency(); }\n",
    );
    assert_eq!(probe(temp.path())["libraries"]["openssl3"], false);
}
#[test]
fn host_probe_constructor_failure_and_hang_stay_in_bounded_child() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    compile(
        temp.path(),
        "libgtk-3.so.0",
        "#include <stdlib.h>\n__attribute__((constructor)) static void init(void) { _Exit(19); }\n",
    );
    let (status, _) = process::output_status(
        &mut command(temp.path()),
        Duration::from_secs(2),
        4096,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(status.code(), Some(19));
    compile(
        temp.path(),
        "libgtk-3.so.0",
        "#include <unistd.h>\n#include <stdio.h>\n#include <stdlib.h>\n__attribute__((constructor)) static void init(void) { FILE *file = fopen(getenv(\"FLIGHTDECK_TEST_MARKER\"), \"w\"); if (file) fclose(file); for (;;) pause(); }\n",
    );
    let marker = temp.path().join("constructor-entered");
    let mut hanging = command(temp.path());
    hanging.env("FLIGHTDECK_TEST_MARKER", &marker);
    let started = std::time::Instant::now();
    assert!(
        process::output(
            &mut hanging,
            Duration::from_millis(500),
            4096,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        marker.is_file(),
        "timeout exercised the library constructor"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}
