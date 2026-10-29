// SPDX-License-Identifier: MIT
//! No device access, Wine execution, downloads or hidden maintainer payload is needed.
#![allow(clippy::unwrap_used)]
use flightdeck::{files, hidsharp_patch, mobiflight};
use std::{fs, path::Path, sync::atomic::AtomicBool};

fn put(path: &Path, bytes: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, bytes).unwrap();
}
#[test]
fn existing_installs_use_the_actual_windows_user_and_ambiguity_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let prefix = temp.path();
    let original =
        prefix.join("drive_c/users/actual-user/AppData/Local/MobiFlight/MobiFlight Connector");
    put(&original.join("MFConnector.exe"), b"installed application");
    put(&original.join("settings/user.mfproj"), b"keep my projects");
    assert_eq!(
        mobiflight::manager(prefix).unwrap(),
        Some(original.canonicalize().unwrap())
    );
    put(
        &prefix.join("drive_c/Program Files/MobiFlight/MFConnector.exe"),
        b"second application",
    );
    assert!(mobiflight::manager(prefix).is_err());
    assert_eq!(
        fs::read(original.join("settings/user.mfproj")).unwrap(),
        b"keep my projects"
    );
}
#[test]
fn escaped_or_linked_applications_are_never_adopted() {
    let temp = tempfile::tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let external = temp.path().join("external");
    put(&external.join("MFConnector.exe"), b"outside");
    files::private_dir(&prefix.join("drive_c/Program Files")).unwrap();
    std::os::unix::fs::symlink(&external, prefix.join("drive_c/Program Files/MobiFlight")).unwrap();
    assert!(mobiflight::manager(&prefix).is_err());
}
#[test]
fn unknown_dll_and_missing_payload_never_damage_an_existing_app() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("app/HidSharp.dll");
    put(&path, b"future vendor release");
    let backup = temp.path().join("backup");
    assert_eq!(hidsharp_patch::state(&path), "unsupported");
    assert!(
        hidsharp_patch::apply(path.parent().unwrap(), &backup, &AtomicBool::new(false)).is_err()
    );
    assert_eq!(fs::read(path).unwrap(), b"future vendor release");
    assert!(!backup.exists());
    assert!(!hidsharp_patch::verified_payload(b"MZ untrusted patch"));
}
#[test]
fn startup_edits_escape_paths_preserve_other_addons_and_disable_without_rewriting_them() {
    let path = r"C:\Flightdeck & André\MFConnector.exe";
    let raw = br#"<?xml version="1.0"?><SimBase.Document Type="Launch" version="1,0"><Disabled>False</Disabled><Launch.Addon><Name>Keep me</Name><Path>C:\Other\app.exe</Path><CommandLine>--user-choice</CommandLine></Launch.Addon><Launch.Addon><Name>Previous</Name><Path>C:\old\MFConnector.exe</Path><CommandLine>custom.mfproj</CommandLine></Launch.Addon></SimBase.Document>"#;
    let enabled = mobiflight::startup_document(Some(raw), path, true).unwrap();
    let text = String::from_utf8(enabled.clone()).unwrap();
    assert!(text.contains("Flightdeck &amp; André"));
    assert!(
        text.contains("custom.mfproj")
            && text.contains("--user-choice")
            && text.contains("Keep me")
    );
    assert_eq!(
        mobiflight::startup_document(Some(&enabled), path, true).unwrap(),
        enabled
    );
    let disabled =
        mobiflight::startup_document(Some(&enabled), r"C:\different\MFConnector.exe", false)
            .unwrap();
    let text = String::from_utf8(disabled).unwrap();
    assert!(text.contains("Flightdeck &amp; André") && !text.contains("different"));
    assert!(text.contains("<Disabled>True</Disabled>"));
    assert!(
        mobiflight::startup_document(
            Some(br#"<SimBase.Document><Disabled>True</Disabled></SimBase.Document>"#),
            path,
            true
        )
        .is_err()
    );
    assert!(mobiflight::startup_document(None, "C:\\app.exe\nMFConnector.exe", true).is_err());
}
