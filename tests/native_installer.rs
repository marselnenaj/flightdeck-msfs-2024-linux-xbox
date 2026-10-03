// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{
    files,
    installer::{self, Options},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::Command,
};
fn write(path: &Path, data: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, data).unwrap();
}
fn source(path: &Path, revision: &str) {
    let mut elf = vec![0; 64];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[18] = 62;
    let inputs: BTreeMap<&str, Vec<u8>> = [
        ("bin/flightdeck", elf),
        ("ui/mark.svg", b"<svg/>".to_vec()),
        ("LICENSE", format!("MIT {revision}").into_bytes()),
        ("THIRD-PARTY-NOTICES.txt", b"synthetic notices".to_vec()),
        (
            "RUST-STANDARD-LIBRARY-NOTICES.html",
            b"synthetic standard library notices".to_vec(),
        ),
    ]
    .into();
    let mut hashes = BTreeMap::new();
    for (name, data) in inputs {
        write(&path.join(name), &data);
        hashes.insert(name, files::sha256(&data));
    }
    files::atomic_json(
        &path.join(installer::PACKAGE),
        &json!({"schema":1,"kind":"rust-launcher","version":flightdeck::VERSION,"files":hashes}),
    )
    .unwrap();
}
struct F {
    _temp: tempfile::TempDir,
    source: PathBuf,
    root: PathBuf,
    bin: PathBuf,
    apps: PathBuf,
}
impl F {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        let result = Self {
            source: t.path().join("source"),
            root: t.path().join("data with ' quote $ dollar"),
            bin: t.path().join("bin"),
            apps: t.path().join("applications"),
            _temp: t,
        };
        source(&result.source, "one");
        result
    }
    fn install(&self) -> Value {
        installer::install(Options {
            source: &self.source,
            root: &self.root,
            bin_dir: &self.bin,
            applications_dir: &self.apps,
            desktop: true,
            language: "de",
            expected_current: None,
        })
        .unwrap()
    }
}
#[test]
fn native_release_install_update_and_rollback_keep_atomic_selection() {
    let f = F::new();
    let first = f.install();
    assert_eq!(first["language"], "de");
    assert!(first["previous"].is_null());
    let first_id = first["current"].as_str().unwrap();
    installer::verify_release(&f.root, first_id).unwrap();
    let launcher = fs::read_to_string(f.bin.join("flightdeck")).unwrap();
    assert!(launcher.starts_with("#!/bin/sh\n"));
    assert!(launcher.contains("--managed-root"));
    assert!(!launcher.contains("python"));
    assert!(launcher.contains("'\\''"));
    let unchanged = f.install();
    assert_eq!(unchanged["current"], first_id);
    assert!(unchanged["previous"].is_null());
    source(&f.source, "two");
    let second = f.install();
    assert_ne!(second["current"], first["current"]);
    assert_eq!(second["previous"], first_id);
    let current = second["current"].as_str().unwrap();
    assert!(installer::rollback(&f.root, "en", Some(first_id)).is_err());
    let reverted = installer::rollback(&f.root, "en", Some(current)).unwrap();
    assert_eq!(reverted["current"], first_id);
    assert_eq!(reverted["manager"], current);
    assert_eq!(reverted["previous"], current);
    installer::verify_release(&f.root, current).unwrap();
    assert!(
        fs::read_to_string(f.apps.join("flightdeck.desktop"))
            .unwrap()
            .contains(first_id)
    );
}
#[test]
fn modified_entries_abort_before_overwriting_and_keep_selected_release() {
    let f = F::new();
    let first = f.install();
    write(&f.bin.join("flightdeck"), b"user customized wrapper");
    source(&f.source, "two");
    let outcome = installer::install(Options {
        source: &f.source,
        root: &f.root,
        bin_dir: &f.bin,
        applications_dir: &f.apps,
        desktop: true,
        language: "de",
        expected_current: None,
    });
    assert!(outcome.is_err());
    assert_eq!(
        fs::read(f.bin.join("flightdeck")).unwrap(),
        b"user customized wrapper"
    );
    assert_eq!(installer::load(&f.root).unwrap().unwrap(), first);
    assert_eq!(fs::read_dir(f.root.join("releases")).unwrap().count(), 1);
}
#[test]
fn untracked_destinations_links_and_modified_source_are_rejected() {
    let f = F::new();
    write(&f.bin.join("flightdeck"), b"foreign launcher");
    assert!(
        installer::install(Options {
            source: &f.source,
            root: &f.root,
            bin_dir: &f.bin,
            applications_dir: &f.apps,
            desktop: true,
            language: "en",
            expected_current: None
        })
        .is_err()
    );
    assert_eq!(
        fs::read(f.bin.join("flightdeck")).unwrap(),
        b"foreign launcher"
    );
    assert!(!f.apps.join("flightdeck.desktop").exists());
    let original = fs::read(f.source.join("LICENSE")).unwrap();
    write(&f.source.join("LICENSE"), b"tampered");
    assert!(installer::snapshot(&f.source).is_err());
    write(&f.source.join("LICENSE"), &original);
    let linked = f._temp.path().join("linked");
    symlink(&f.source, &linked).unwrap();
    assert!(installer::snapshot(&linked).is_err());
}
#[test]
fn uninstall_keeps_modified_and_unknown_files_and_never_touches_runtime() {
    let f = F::new();
    let state = f.install();
    let release = f
        .root
        .join("releases")
        .join(state["current"].as_str().unwrap());
    write(&release.join("LICENSE"), b"modified license");
    write(&release.join("user-notes"), b"keep unknown");
    let runtime = f._temp.path().join("runtime");
    write(&runtime.join("private/local-saves/save"), b"progress");
    let retained = installer::uninstall(&f.root).unwrap();
    assert!(retained.contains(&release));
    assert_eq!(
        fs::read(release.join("LICENSE")).unwrap(),
        b"modified license"
    );
    assert_eq!(
        fs::read(release.join("user-notes")).unwrap(),
        b"keep unknown"
    );
    assert!(!f.bin.join("flightdeck").exists());
    assert!(!f.apps.join("flightdeck.desktop").exists());
    assert!(!f.root.join("installation.json").exists());
    assert!(f.root.join(".install.lock").is_file());
    assert_eq!(
        fs::read(runtime.join("private/local-saves/save")).unwrap(),
        b"progress"
    );
}
#[test]
fn legacy_installation_can_upgrade_then_select_its_preserved_python_release() {
    let f = F::new();
    let script = "import json,hashlib,sys,os\nfrom pathlib import Path\nroot,bin,apps=map(Path,sys.argv[1:])\ndef encoded(v):return (json.dumps(v,indent=2,sort_keys=True,ensure_ascii=False)+'\\n').encode()\nfiles={'flightdeck/__init__.py':b'__version__ = \"0.1.21\"\\n','flightdeck/__main__.py':b'pass\\n','ui/mark.svg':b'<svg/>'}\nhashes={k:hashlib.sha256(v).hexdigest() for k,v in files.items()};identity=hashlib.sha256(encoded(hashes)).hexdigest();folder=root/'releases'/identity\nfor name,data in files.items():\n p=folder/name;p.parent.mkdir(parents=True,exist_ok=True,mode=0o700);p.write_bytes(data)\nroot.chmod(0o700)\n(folder/'release.json').write_bytes(encoded({'app':'flightdeck-source-launcher','format':1,'release':identity,'files':hashes}))\nbin.mkdir(mode=0o700);entry=bin/'flightdeck';entry.write_bytes(b'legacy wrapper');entry.chmod(0o700)\n(root/'installation.json').write_bytes(encoded({'app':'flightdeck-source-launcher','format':1,'data_dir':str(root),'current':identity,'previous':None,'language':'en','entries':{'launcher':{'path':str(entry),'sha256':hashlib.sha256(entry.read_bytes()).hexdigest()}}}))\n";
    assert!(
        Command::new("python3")
            .args(["-c", script])
            .args([&f.root, &f.bin, &f.apps])
            .status()
            .unwrap()
            .success()
    );
    let legacy = installer::load(&f.root).unwrap().unwrap();
    let installed = f.install();
    assert_eq!(installed["previous"], legacy["current"]);
    let restored =
        installer::rollback(&f.root, "en", Some(installed["current"].as_str().unwrap())).unwrap();
    assert_eq!(restored["current"], legacy["current"]);
    assert_eq!(restored["manager"], installed["current"]);
    installer::verify_release(&f.root, legacy["current"].as_str().unwrap()).unwrap();
}
#[test]
fn manifest_paths_and_incomplete_optional_components_are_rejected() {
    let f = F::new();
    let mut manifest: Value = files::json(&f.source.join(installer::PACKAGE), 65536).unwrap();
    manifest["files"]["../outside"] = json!("a".repeat(64));
    files::atomic_json(&f.source.join(installer::PACKAGE), &manifest).unwrap();
    assert!(installer::snapshot(&f.source).is_err());
    source(&f.source, "one");
    let mut manifest: Value = files::json(&f.source.join(installer::PACKAGE), 65536).unwrap();
    write(
        &f.source.join("resources/native/bin/xodus-cli"),
        b"unexpected",
    );
    manifest["files"]["resources/native/bin/xodus-cli"] = json!(files::sha256(b"unexpected"));
    files::atomic_json(&f.source.join(installer::PACKAGE), &manifest).unwrap();
    assert!(installer::snapshot(&f.source).is_err());
}
#[test]
fn installed_native_launcher_and_management_work_without_python_on_path() {
    let f = F::new();
    let executable = env!("CARGO_BIN_EXE_flightdeck-rust");
    let bytes = fs::read(executable).unwrap();
    write(&f.source.join("bin/flightdeck"), &bytes);
    assert!(
        Command::new("strip")
            .arg("--strip-debug")
            .arg(f.source.join("bin/flightdeck"))
            .status()
            .unwrap()
            .success()
    );
    let bytes = fs::read(f.source.join("bin/flightdeck")).unwrap();
    let mut manifest: Value = files::json(&f.source.join(installer::PACKAGE), 65536).unwrap();
    manifest["files"]["bin/flightdeck"] = json!(files::sha256(&bytes));
    files::atomic_json(&f.source.join(installer::PACKAGE), &manifest).unwrap();
    let result = Command::new(executable)
        .arg("install")
        .arg("--source")
        .arg(&f.source)
        .arg("--data-dir")
        .arg(&f.root)
        .arg("--bin-dir")
        .arg(&f.bin)
        .arg("--applications-dir")
        .arg(&f.apps)
        .args(["--no-launch", "--language", "en"])
        .env("PATH", "/no-programs")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let wrapper = f.bin.join("flightdeck");
    let result = Command::new(&wrapper)
        .arg("--version")
        .env("PATH", "/no-programs")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains(flightdeck::VERSION));
    let result = Command::new(&wrapper)
        .args(["--uninstall", "--language=en"])
        .env("PATH", "/no-programs")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!wrapper.exists());
    assert!(!f.root.join("installation.json").exists());
}
