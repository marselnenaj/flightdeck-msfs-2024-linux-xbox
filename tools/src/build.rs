// SPDX-License-Identifier: MIT
//! The release build with remapped paths, and the workspace tests behind a
//! private process namespace.
use crate::util::{Failure, Result, fail, require};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn status(command: &mut Command, label: &str) -> Result<()> {
    let status = command
        .status()
        .map_err(|error| Failure(format!("{label}: {error}")))?;
    require(status.success(), format!("{label} failed"))
}

/// The compiler flags that keep checkout, Cargo and toolchain paths out of
/// the executable, appended to whatever the environment already requests.
pub fn remapped(existing: Vec<String>, root: &Path, cargo_home: &Path, sysroot: &Path) -> String {
    let mut flags = existing;
    for (source, replacement) in [
        (root, "/usr/src/flightdeck"),
        (cargo_home, "/usr/src/cargo"),
        (sysroot, "/usr/src/rust"),
    ] {
        let source = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
        flags.push(format!(
            "--remap-path-prefix={}={replacement}",
            source.display()
        ));
    }
    flags.join("\x1f")
}

pub fn native(
    root: &Path,
    offline: bool,
    toolchain: Option<&str>,
    bundled_media: bool,
    bundled_mobiflight: bool,
) -> Result<()> {
    if bundled_media {
        crate::media::verify_bundle(root, &root.join("build/media"))?;
    }
    if bundled_mobiflight {
        crate::mobiflight::verify_bundle(root, &root.join("build/mobiflight"))?;
    }
    let selector: Vec<String> = toolchain
        .map(|name| format!("+{name}"))
        .into_iter()
        .collect();
    let sysroot = Command::new("rustc")
        .args(&selector)
        .args(["--print", "sysroot"])
        .output()
        .map_err(|error| Failure(format!("rustc: {error}")))?;
    require(
        sysroot.status.success(),
        "rustc could not report its sysroot",
    )?;
    let sysroot = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim());
    let existing = match std::env::var("CARGO_ENCODED_RUSTFLAGS") {
        Ok(encoded) if !encoded.is_empty() => encoded.split('\x1f').map(str::to_string).collect(),
        _ => std::env::var("RUSTFLAGS")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect(),
    };
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .ok_or_else(|| Failure("Neither CARGO_HOME nor HOME is set".into()))?;
    let mut build = Command::new("cargo");
    build
        .args(&selector)
        .args(["build", "--locked", "--release"])
        .args(bundled_media.then_some("--features=bundled-media"))
        .args(bundled_mobiflight.then_some("--features=bundled-mobiflight"))
        .args(offline.then_some("--offline"))
        .current_dir(root)
        .env_remove("RUSTFLAGS")
        .env(
            "CARGO_ENCODED_RUSTFLAGS",
            remapped(existing, root, &cargo_home, &sysroot),
        );
    status(&mut build, "cargo build")
}

/// The complete workspace suite with private process visibility, as the
/// invoking user. GitHub's runner has a persistent, unreadable sudo process
/// with the runner's real UID, which the cloud guard correctly refuses as an
/// unknown writer. Only the namespace setup is privileged; Cargo and every
/// test run without capabilities, and no credential reaches them.
pub fn isolated_tests(
    variable: impl Fn(&str) -> Option<String>,
    uid: u32,
    gid: u32,
    cargo: Option<&str>,
) -> Result<Vec<String>> {
    let Some(cargo) = cargo.filter(|_| uid != 0) else {
        return fail("Run the tests as an unprivileged user with Cargo available");
    };
    let set = |name: &str| variable(name).filter(|value| !value.is_empty());
    let home = variable("HOME").ok_or_else(|| Failure("HOME is not set".into()))?;
    let mut values = vec![
        ("HOME", home.clone()),
        ("PATH", variable("PATH").unwrap_or_default()),
        (
            "CARGO_HOME",
            set("CARGO_HOME").unwrap_or(format!("{home}/.cargo")),
        ),
        (
            "RUSTUP_HOME",
            set("RUSTUP_HOME").unwrap_or(format!("{home}/.rustup")),
        ),
        (
            "RUSTUP_TOOLCHAIN",
            set("RUSTUP_TOOLCHAIN").unwrap_or("1.98.0".into()),
        ),
        ("LANG", "C.UTF-8".into()),
        ("LC_ALL", "C.UTF-8".into()),
    ];
    for name in ["ICED_TEST_BACKEND", "CARGO_TARGET_DIR"] {
        if let Some(value) = set(name) {
            values.push((name, value));
        }
    }
    let fixed = [
        "/usr/bin/sudo",
        "-n",
        "/usr/bin/unshare",
        "--mount",
        "--propagation",
        "private",
        "--pid",
        "--fork",
        "--kill-child=SIGKILL",
        "--mount-proc",
        "/usr/bin/setpriv",
    ];
    let identity = [
        format!("--reuid={uid}"),
        format!("--regid={gid}"),
        "--init-groups".into(),
        "--bounding-set=-all".into(),
        "--inh-caps=-all".into(),
        "--ambient-caps=-all".into(),
        "--no-new-privs".into(),
        "/usr/bin/env".into(),
        "-i".into(),
    ];
    Ok(fixed
        .iter()
        .map(|part| part.to_string())
        .chain(identity)
        .chain(values.iter().map(|(name, value)| format!("{name}={value}")))
        .chain([cargo, "test", "--locked", "--workspace"].map(str::to_string))
        .collect())
}

pub fn native_tests(root: &Path) -> Result<()> {
    let cargo = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|folder| folder.join("cargo"))
            .find(|candidate| candidate.is_file())
    });
    let command = isolated_tests(
        |name| std::env::var(name).ok(),
        rustix::process::getuid().as_raw(),
        rustix::process::getgid().as_raw(),
        cargo.as_deref().and_then(Path::to_str),
    )?;
    let mut tests = Command::new(&command[0]);
    tests
        .args(&command[1..])
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8");
    status(&mut tests, "The isolated test suite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_user_private_proc_no_capabilities_or_credentials_and_full_suite() {
        let environment = |name: &str| {
            [
                ("HOME", "/fixture/home"),
                ("PATH", "/tools:/usr/bin"),
                ("ICED_TEST_BACKEND", "tiny-skia"),
                ("GH_TOKEN", "must-not-leak"),
                ("GITHUB_TOKEN", "must-not-leak"),
                ("CARGO_HOME", ""),
                ("RUSTUP_HOME", ""),
            ]
            .into_iter()
            .find_map(|(key, value)| (key == name).then(|| value.to_string()))
        };
        let command =
            isolated_tests(environment, 1001, 1002, Some("/tools/cargo")).expect("command");
        for argument in [
            "--pid",
            "--mount-proc",
            "--propagation",
            "private",
            "--reuid=1001",
            "--regid=1002",
            "--bounding-set=-all",
            "--inh-caps=-all",
            "--ambient-caps=-all",
            "--no-new-privs",
            "--kill-child=SIGKILL",
        ] {
            assert!(command.iter().any(|part| part == argument), "{argument}");
        }
        assert!(!command.iter().any(|part| part == "--user"));
        assert_eq!(
            command[command.len() - 4..],
            ["/tools/cargo", "test", "--locked", "--workspace"]
        );
        for value in [
            "CARGO_HOME=/fixture/home/.cargo",
            "RUSTUP_HOME=/fixture/home/.rustup",
            "ICED_TEST_BACKEND=tiny-skia",
            "RUSTUP_TOOLCHAIN=1.98.0",
        ] {
            assert!(command.iter().any(|part| part == value), "{value}");
        }
        assert!(!command.join(" ").contains("must-not-leak"));
        let position = |word: &str| command.iter().position(|part| part == word).expect(word);
        assert!(position("/usr/bin/setpriv") < position("/tools/cargo"));
        assert_eq!(command[position("/usr/bin/env") + 1], "-i");
    }

    #[test]
    fn root_or_a_missing_toolchain_is_rejected_before_namespace_setup() {
        let home = |name: &str| (name == "HOME").then(|| "/fixture/home".to_string());
        assert!(isolated_tests(home, 0, 0, Some("/tools/cargo")).is_err());
        assert!(isolated_tests(home, 1001, 1001, None).is_err());
        assert!(isolated_tests(|_| None, 1001, 1001, Some("/tools/cargo")).is_err());
    }

    #[test]
    fn checkout_cargo_and_toolchain_paths_are_remapped_after_existing_flags() {
        let flags = remapped(
            vec!["-C".into(), "target-cpu=x86-64".into()],
            Path::new("/checkout"),
            Path::new("/cargo home"),
            Path::new("/toolchain"),
        );
        assert_eq!(
            flags.split('\x1f').collect::<Vec<_>>(),
            [
                "-C",
                "target-cpu=x86-64",
                "--remap-path-prefix=/checkout=/usr/src/flightdeck",
                "--remap-path-prefix=/cargo home=/usr/src/cargo",
                "--remap-path-prefix=/toolchain=/usr/src/rust"
            ]
        );
    }
}
