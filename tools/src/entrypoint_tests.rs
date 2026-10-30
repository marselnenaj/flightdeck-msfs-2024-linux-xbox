// SPDX-License-Identifier: MIT
//! The shell entry point of a package, before any Rust code runs: platform and
//! loader errors, bounded probes and literal arguments. No installed profile
//! is touched; the "launcher" is a script that records how it was called.
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Entrypoint {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    package: PathBuf,
    commands: PathBuf,
}

const LAUNCHER: &str = r#"#!/bin/bash
printf '%s\0' "$@" >> "$TEST_CALLS"
printf '\0' >> "$TEST_CALLS"
if [[ "$1" == --version ]]; then
  case "${TEST_FAILURE:-}" in
    hang) exec /bin/sleep 30 ;;
    huge) for ((i=0; i<10000; i++)); do printf 'bounded diagnostic text '; done; exit 1 ;;
    *) if [[ -n ${TEST_FAILURE:-} ]]; then printf '%s\n' "$TEST_FAILURE" >&2; exit 127; fi ;;
  esac
  printf '%s\n' 'flightdeck 0.2.7'
fi
"#;

fn which(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|folder| folder.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{name} is required for this test"))
}

impl Entrypoint {
    fn new() -> Self {
        let temporary = tempfile::Builder::new()
            .prefix("flightdeck-entrypoint-")
            .tempdir()
            .expect("temp");
        let root = fs::canonicalize(temporary.path()).expect("root");
        let package = root.join("package with spaces $(touch unintended)");
        let commands = root.join("commands");
        fs::create_dir_all(package.join("bin")).expect("package");
        fs::create_dir(&commands).expect("commands");
        fs::copy(
            crate::util::root().join("install.sh"),
            package.join("install.sh"),
        )
        .expect("install.sh");
        for command in ["dirname", "timeout"] {
            std::os::unix::fs::symlink(which(command), commands.join(command)).expect("command");
        }
        let fixture = Self {
            _temporary: temporary,
            root,
            package,
            commands,
        };
        fixture.command(
            "uname",
            "case \"$1\" in\n  -s) printf '%s\\n' \"${TEST_SYSTEM-Linux}\" ;;\n  -m) printf '%s\\n' \"${TEST_ARCH-x86_64}\" ;;\nesac\n",
        );
        fixture.script(&fixture.binary(), LAUNCHER);
        fixture
    }
    fn binary(&self) -> PathBuf {
        self.package.join("bin/flightdeck")
    }
    fn script(&self, path: &Path, content: &str) {
        fs::write(path, content).expect("script");
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("mode");
    }
    fn command(&self, name: &str, body: &str) {
        self.script(&self.commands.join(name), &format!("#!/bin/bash\n{body}"));
    }
    fn run(&self, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
        let mut variables: BTreeMap<&str, String> = [
            ("PATH", self.commands.to_string_lossy().into_owned()),
            ("HOME", self.root.to_string_lossy().into_owned()),
            ("LANG", "C".to_string()),
            ("LC_ALL", "C".to_string()),
            (
                "TEST_CALLS",
                self.root.join("calls").to_string_lossy().into_owned(),
            ),
            (
                "TEST_DIALOG",
                self.root.join("dialog").to_string_lossy().into_owned(),
            ),
        ]
        .into();
        for (name, value) in environment {
            variables.insert(name, value.to_string());
        }
        let mut child = Command::new("/bin/bash")
            .arg(self.package.join("install.sh"))
            .args(arguments)
            .env_clear()
            .envs(&variables)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("install.sh");
        let deadline = Instant::now() + Duration::from_secs(12);
        while child.try_wait().expect("wait").is_none() {
            assert!(Instant::now() < deadline, "install.sh did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().expect("output")
    }
    /// The argument lists the launcher was started with, in order.
    fn calls(&self) -> Vec<Vec<String>> {
        let Ok(raw) = fs::read(self.root.join("calls")) else {
            return Vec::new();
        };
        let text = String::from_utf8(raw).expect("calls");
        text.split("\0\0")
            .filter(|part| !part.is_empty())
            .map(|part| part.split('\0').map(str::to_string).collect())
            .collect()
    }
    fn assert_not_installed(&self) {
        assert!(
            self.calls().iter().all(|call| call == &["--version"]),
            "{:?}",
            self.calls()
        );
    }
    fn dialog(&self) -> Option<Vec<String>> {
        let raw = fs::read(self.root.join("dialog")).ok()?;
        let text = String::from_utf8(raw).expect("dialog");
        Some(
            text.trim_end_matches('\0')
                .split('\0')
                .map(str::to_string)
                .collect(),
        )
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn probe_then_install_preserves_literal_arguments_without_ldd_or_getconf() {
    let fixture = Entrypoint::new();
    let arguments = [
        "--language",
        "de",
        "--language=en",
        "--gui",
        "--data-home",
        "literal $(touch unexpected); *",
    ];
    let result = fixture.run(&arguments, &[]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    let package = fixture.package.to_string_lossy();
    let install = [&["install", "--source", &package][..], &arguments[..]].concat();
    assert_eq!(
        fixture.calls(),
        [strings(&["--version"]), strings(&install)]
    );
    assert!(!fixture.root.join("unexpected").exists());
    assert!(!fixture.root.join("unintended").exists());
    assert!(result.stdout.is_empty());
}

#[test]
fn missing_optional_probe_and_platform_tools_do_not_block_installation() {
    let fixture = Entrypoint::new();
    fs::remove_file(fixture.commands.join("timeout")).expect("timeout");
    fs::remove_file(fixture.commands.join("uname")).expect("uname");
    let result = fixture.run(&["--language=de"], &[]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    let package = fixture.package.to_string_lossy();
    assert_eq!(
        fixture.calls(),
        [strings(&["install", "--source", &package, "--language=de"])]
    );
}

#[test]
fn unknown_platform_information_does_not_block_a_working_binary() {
    let fixture = Entrypoint::new();
    let result = fixture.run(&[], &[("TEST_SYSTEM", ""), ("TEST_ARCH", "")]);
    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(fixture.calls().len(), 2);
}

#[test]
fn clear_unsupported_platform_is_reported_before_running_the_binary() {
    for environment in [("TEST_ARCH", "aarch64"), ("TEST_SYSTEM", "Darwin")] {
        let fixture = Entrypoint::new();
        let result = fixture.run(&[], &[environment]);
        assert_eq!(result.status.code(), Some(1));
        assert!(stderr(&result).contains("requires Linux on x86-64"));
        assert!(fixture.calls().is_empty());
    }
}

#[test]
fn loader_failures_are_classified_and_never_install() {
    for (detail, expected) in [
        (
            "/lib/libc.so.6: version `GLIBC_2.39' not found",
            "needs glibc 2.39 or newer",
        ),
        (
            "error while loading shared libraries: libgcc_s.so.1: cannot open shared object file: No such file or directory",
            "runtime library",
        ),
        (
            "timeout: failed to run command: No such file or directory",
            "program loader",
        ),
        ("cannot execute: required file not found", "program loader"),
        ("Permission denied", "cannot start here"),
    ] {
        let fixture = Entrypoint::new();
        let result = fixture.run(&[], &[("TEST_FAILURE", detail)]);
        assert_eq!(result.status.code(), Some(1), "{detail}");
        let message = stderr(&result);
        assert!(
            message.contains(expected) && message.contains(detail),
            "{message}"
        );
        fixture.assert_not_installed();
    }
}

#[test]
fn explicit_language_and_german_locale_select_translated_errors() {
    let fixture = Entrypoint::new();
    let result = fixture.run(
        &["--language=de"],
        &[("TEST_FAILURE", "GLIBC_2.39 not found")],
    );
    assert!(stderr(&result).contains("glibc-Version dieses Systems"));
    let german = [
        ("TEST_ARCH", "aarch64"),
        ("LC_ALL", ""),
        ("LANG", "de_AT.UTF-8"),
    ];
    let result = fixture.run(&[], &german);
    assert!(stderr(&result).contains("benötigt Linux auf x86-64"));
    let result = fixture.run(&["--language", "en"], &german);
    assert!(stderr(&result).contains("requires Linux on x86-64"));
    fixture.assert_not_installed();
}

#[test]
fn invalid_language_does_not_run_the_binary() {
    let fixture = Entrypoint::new();
    for arguments in [&["--language", "xx"][..], &["--language"][..]] {
        let result = fixture.run(arguments, &[]);
        assert_eq!(result.status.code(), Some(2));
        assert!(stderr(&result).contains("Choose --language"));
        assert!(fixture.calls().is_empty());
    }
}

#[test]
fn missing_or_nonexecutable_package_is_reported() {
    let fixture = Entrypoint::new();
    fs::set_permissions(fixture.binary(), fs::Permissions::from_mode(0o600)).expect("mode");
    let result = fixture.run(&[], &[]);
    assert_eq!(result.status.code(), Some(1));
    assert!(stderr(&result).contains("not executable"));
    assert!(fixture.calls().is_empty());
    fs::remove_file(fixture.binary()).expect("remove");
    let result = fixture.run(&["--language=de"], &[]);
    assert!(stderr(&result).contains("fehlt oder ist nicht ausführbar"));
}

#[test]
fn a_real_hung_probe_is_bounded() {
    let fixture = Entrypoint::new();
    let started = Instant::now();
    let result = fixture.run(&[], &[("TEST_FAILURE", "hang")]);
    let elapsed = started.elapsed();
    assert_eq!(result.status.code(), Some(1));
    assert!(stderr(&result).contains("did not finish in time"));
    assert!(
        elapsed >= Duration::from_millis(4500) && elapsed < Duration::from_secs(9),
        "{elapsed:?}"
    );
    fixture.assert_not_installed();
}

#[test]
fn diagnostic_output_is_bounded() {
    let fixture = Entrypoint::new();
    let result = fixture.run(&[], &[("TEST_FAILURE", "huge")]);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stderr.len() < 4600);
    assert!(stderr(&result).contains("bounded diagnostic text"));
    fixture.assert_not_installed();
}

#[test]
fn gui_uses_literal_bounded_zenity_arguments_only_when_requested() {
    let fixture = Entrypoint::new();
    fixture.command("zenity", "printf '%s\\0' \"$@\" > \"$TEST_DIALOG\"\n");
    fixture.command("kdialog", "exit 99\n");
    let detail = "Permission denied <b>& $(touch unexpected)";
    let result = fixture.run(
        &["--gui", "--language=de"],
        &[("TEST_FAILURE", detail), ("DISPLAY", ":fixture")],
    );
    assert_eq!(result.status.code(), Some(1));
    let arguments = fixture.dialog().expect("dialog");
    assert_eq!(
        arguments[..5],
        ["--error", "--no-markup", "--title", "Flightdeck", "--text"]
    );
    assert_eq!(arguments.len(), 6);
    assert!(arguments[5].contains("kann hier nicht starten") && arguments[5].contains(detail));
    assert!(!fixture.root.join("unexpected").exists());
    fs::remove_file(fixture.root.join("dialog")).expect("remove");
    fixture.run(&[], &[("TEST_FAILURE", detail), ("DISPLAY", ":fixture")]);
    assert!(fixture.dialog().is_none(), "no dialog without --gui");
    fixture.run(&["--gui"], &[("TEST_FAILURE", detail)]);
    assert!(fixture.dialog().is_none(), "no dialog without a display");
}

#[test]
fn kdialog_fallback_escapes_diagnostics_markup() {
    let fixture = Entrypoint::new();
    fixture.command("kdialog", "printf '%s\\0' \"$@\" > \"$TEST_DIALOG\"\n");
    let result = fixture.run(
        &["--gui"],
        &[
            ("TEST_FAILURE", "Permission denied <b>&"),
            ("WAYLAND_DISPLAY", "fixture"),
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    let arguments = fixture.dialog().expect("dialog");
    assert_eq!(arguments[..3], ["--title", "Flightdeck", "--error"]);
    assert_eq!(arguments.len(), 4);
    assert!(arguments[3].contains("&lt;b&gt;&amp;") && !arguments[3].contains("<b>"));
}
