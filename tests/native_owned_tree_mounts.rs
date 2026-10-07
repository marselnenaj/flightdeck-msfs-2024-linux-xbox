// SPDX-License-Identifier: MIT
//! Mount fixtures run only after entering an independent user/mount namespace.
#![allow(clippy::unwrap_used)]
use flightdeck::owned_tree;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

const CHILD: &str = "FLIGHTDECK_TEST_MOUNT_PARENT_NAMESPACE";

struct Bind(PathBuf);
impl Bind {
    fn new(source: &Path, target: &Path) -> Self {
        assert!(
            Command::new("mount")
                .arg("--bind")
                .arg(source)
                .arg(target)
                .status()
                .unwrap()
                .success()
        );
        Self(target.into())
    }
}
impl Drop for Bind {
    fn drop(&mut self) {
        assert!(
            Command::new("umount")
                .arg(&self.0)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
#[ignore = "requires unprivileged user/mount namespaces and mount/umount utilities"]
fn nested_same_filesystem_bind_mount_preserves_external_files() {
    let namespace = fs::read_link("/proc/self/ns/mnt").unwrap();
    let Some(parent) = std::env::var_os(CHILD) else {
        let status = Command::new("unshare")
            .args(["--user", "--map-root-user", "--mount", "--fork", "--"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "nested_same_filesystem_bind_mount_preserves_external_files",
                "--ignored",
                "--nocapture",
            ])
            .env(CHILD, namespace)
            .status()
            .unwrap();
        assert!(status.success(), "isolated mount fixture failed: {status}");
        return;
    };
    // Never run a mount command in the invoking process's namespace, even if
    // someone accidentally supplies this environment variable directly.
    assert_ne!(namespace, PathBuf::from(parent));
    assert!(
        Command::new("mount")
            .args(["--make-rprivate", "/"])
            .status()
            .unwrap()
            .success()
    );
    let temp = tempfile::tempdir().unwrap();
    let tree = temp.path().join("tree");
    let external = temp.path().join("external");
    let nested = tree.join("nested");
    let selected_mount = temp.path().join("selected-mount");
    for folder in [&nested, &external, &selected_mount] {
        fs::create_dir_all(folder).unwrap();
    }
    fs::write(external.join("sentinel"), b"keep original bytes").unwrap();
    let root_id = owned_tree::identity(&tree).unwrap();
    let nested_bind = Bind::new(&external, &nested);
    assert_eq!(
        tree.metadata().unwrap().dev(),
        nested.metadata().unwrap().dev(),
        "fixture must defeat a device-only check"
    );
    assert!(owned_tree::inventory(&tree, &AtomicBool::new(false)).is_err());
    // Also exercise removal directly, covering a mount added after inventory.
    assert!(owned_tree::remove(&tree, root_id).is_err());
    assert_eq!(
        fs::read(external.join("sentinel")).unwrap(),
        b"keep original bytes"
    );
    let root_bind = Bind::new(&external, &selected_mount);
    assert!(owned_tree::inventory(&selected_mount, &AtomicBool::new(false)).is_ok());
    drop(root_bind);
    drop(nested_bind);
    // Ordinary directories and symlink behavior remain covered by maintenance.
    owned_tree::remove(&tree, root_id).unwrap();
    assert_eq!(
        fs::read(external.join("sentinel")).unwrap(),
        b"keep original bytes"
    );
}
