// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{components, files, resources, transaction as tx};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    native: PathBuf,
    old: Value,
    new: Value,
    lock: Value,
}
fn write(path: &Path, data: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, data).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("runtime");
        let native = temp.path().join("native");
        let mut old = json!({});
        let mut new = json!({});
        for (name, targets) in components::FILES.iter().zip(components::targets(&root)) {
            let before = format!("old {name}");
            let after = format!("new {name}");
            old[name] = json!(files::sha256(before.as_bytes()));
            new[name] = json!(files::sha256(after.as_bytes()));
            for target in targets {
                write(&target, before.as_bytes());
            }
            write(&native.join(name), after.as_bytes());
        }
        files::private_dir(&root.join("private")).unwrap();
        files::atomic_json(
            &root.join("private/import-manifest.json"),
            &json!({"format":1,"artifacts":{"files":old},"keep":"profile settings"}),
        )
        .unwrap();
        write(&root.join("private/save.bin"), b"synthetic save");
        let lock =
            json!({"native":{"files":new,"upgrade_from":[old],"features":[],"cli_features":[]}});
        Self {
            _temp: temp,
            root,
            native,
            old,
            new,
            lock,
        }
    }
    fn assert_hashes(&self, expected: &Value) {
        for (name, targets) in components::FILES
            .iter()
            .zip(components::targets(&self.root))
        {
            for path in targets {
                assert_eq!(tx::digest(&path).unwrap(), expected[name]);
            }
        }
        assert_eq!(
            files::read(&self.root.join("private/save.bin"), 100).unwrap(),
            b"synthetic save"
        );
    }
    fn journal(&self) -> PathBuf {
        let backup = self
            .root
            .join("private/.component-0123456789abcdef0123456789abcdef");
        files::private_dir(&backup).unwrap();
        for (i, targets) in components::targets(&self.root).iter().enumerate() {
            for (j, path) in targets.iter().enumerate() {
                write(
                    &backup.join(format!("{i}-{j}")),
                    &files::read(path, 4096).unwrap(),
                );
            }
        }
        let manifest = self.root.join("private/import-manifest.json");
        write(
            &backup.join("import-manifest.json"),
            &files::read(&manifest, 4096).unwrap(),
        );
        files::atomic_json(&self.root.join("private/component-update.json"),&json!({"format":1,"backup":backup.file_name().unwrap().to_str().unwrap(),"before":self.old,"after":self.new,"manifest_sha256":tx::digest(&manifest).unwrap()})).unwrap();
        backup
    }
}
#[test]
fn updates_all_copies_preserving_profile_and_is_idempotent() {
    let f = Fixture::new();
    assert!(components::refresh_with(&f.root, &f.native, &f.lock).unwrap());
    f.assert_hashes(&f.new);
    assert!(!components::refresh_with(&f.root, &f.native, &f.lock).unwrap());
    assert!(!f.root.join("private/component-update.json").exists());
}
#[test]
fn interrupted_partial_copy_restores_the_old_set() {
    let f = Fixture::new();
    let backup = f.journal();
    write(&f.root.join("bin/xodus-cli"), b"partial replacement");
    components::recover(&f.root).unwrap();
    f.assert_hashes(&f.old);
    assert!(!backup.exists());
}
#[test]
fn changed_backup_cannot_partially_restore() {
    let f = Fixture::new();
    let backup = f.journal();
    write(&f.root.join("bin/xodus-cli"), b"partial replacement");
    write(&backup.join("5-0"), b"tampered backup");
    assert!(components::recover(&f.root).is_err());
    assert_eq!(
        files::read(&f.root.join("bin/xodus-cli"), 100).unwrap(),
        b"partial replacement"
    );
    assert!(backup.exists());
}
#[test]
fn refuses_parent_symlink_before_mutating_any_target() {
    let f = Fixture::new();
    let moved = f.root.join("saved-bin");
    fs::rename(f.root.join("bin"), &moved).unwrap();
    symlink(&moved, f.root.join("bin")).unwrap();
    assert!(components::refresh_with(&f.root, &f.native, &f.lock).is_err());
    f.assert_hashes(&f.old);
}
#[test]
fn changed_native_payload_stops_before_update() {
    let f = Fixture::new();
    write(&f.native.join(components::FILES[5]), b"changed");
    assert!(components::refresh_with(&f.root, &f.native, &f.lock).is_err());
    f.assert_hashes(&f.old);
}
#[test]
fn script_update_uses_the_embedded_pinned_release() {
    let mut f = Fixture::new();
    let lock = resources::json("compat/bootstrap.lock.json").unwrap();
    let mut old = json!({});
    for name in components::SCRIPTS {
        let data = format!("#!/bin/sh\n# old {name}\n");
        old[name] = json!(files::sha256(data.as_bytes()));
        write(&f.root.join("tools").join(name), data.as_bytes());
    }
    f.lock["runtime_scripts"] =
        json!({"files":lock["runtime_scripts"]["files"],"upgrade_from":[old]});
    assert!(components::refresh_with(&f.root, &f.native, &f.lock).unwrap());
    for name in components::SCRIPTS {
        assert_eq!(
            tx::digest(&f.root.join("tools").join(name)).unwrap(),
            lock["runtime_scripts"]["files"][name]
        );
    }
}
#[test]
fn custom_launch_scripts_survive_native_update() {
    let mut f = Fixture::new();
    let lock = resources::json("compat/bootstrap.lock.json").unwrap();
    f.lock["runtime_scripts"] = lock["runtime_scripts"].clone();
    for name in components::SCRIPTS {
        write(&f.root.join("tools").join(name), b"#!/bin/sh\n# custom\n");
    }
    assert!(components::refresh_with(&f.root, &f.native, &f.lock).unwrap());
    f.assert_hashes(&f.new);
    for name in components::SCRIPTS {
        assert_eq!(
            files::read(&f.root.join("tools").join(name), 100).unwrap(),
            b"#!/bin/sh\n# custom\n"
        );
    }
}
