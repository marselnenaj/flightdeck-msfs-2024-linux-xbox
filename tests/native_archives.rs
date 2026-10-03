// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{bootstrap, files};
use std::{io::Cursor, sync::atomic::AtomicBool};
fn archive(link: Option<(&str, &str)>, special: bool) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o755);
    if let Some((name, target)) = link {
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        builder.append_link(&mut header, name, target).unwrap();
    } else if special {
        header.set_entry_type(tar::EntryType::Fifo);
        header.set_size(0);
        header.set_cksum();
        builder
            .append_data(&mut header, "pipe", Cursor::new([]))
            .unwrap();
    } else {
        header.set_size(5);
        header.set_cksum();
        builder
            .append_data(&mut header, "folder/file", Cursor::new(b"hello"))
            .unwrap();
    }
    builder.into_inner().unwrap()
}
#[test]
fn extract_preserves_bounded_contents() {
    let temp = tempfile::tempdir().unwrap();
    files::private_dir(temp.path()).unwrap();
    files::private_dir(temp.path()).unwrap();
    bootstrap::extract_reader(
        Cursor::new(archive(None, false)),
        temp.path(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("folder/file")).unwrap(),
        b"hello"
    );
}
#[test]
fn rejects_absolute_and_escaping_links_and_devices() {
    for bytes in [
        archive(Some(("bad", "/tmp/outside")), false),
        archive(Some(("folder/bad", "../../outside")), false),
        archive(None, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        files::private_dir(temp.path()).unwrap();
        assert!(
            bootstrap::extract_reader(Cursor::new(bytes), temp.path(), &AtomicBool::new(false))
                .is_err()
        );
    }
}
#[test]
fn allows_relative_internal_runner_links() {
    let temp = tempfile::tempdir().unwrap();
    files::private_dir(temp.path()).unwrap();
    bootstrap::extract_reader(
        Cursor::new(archive(Some(("folder/link", "../file")), false)),
        temp.path(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_link(temp.path().join("folder/link")).unwrap(),
        std::path::Path::new("../file")
    );
}
#[test]
fn cancellation_prevents_extraction() {
    let temp = tempfile::tempdir().unwrap();
    files::private_dir(temp.path()).unwrap();
    assert!(
        bootstrap::extract_reader(
            Cursor::new(archive(None, false)),
            temp.path(),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert!(!temp.path().join("folder").exists());
}
#[test]
fn embedded_script_hashes_match_the_release_contract() {
    let lock = flightdeck::resources::json("compat/bootstrap.lock.json").unwrap();
    for name in flightdeck::components::SCRIPTS {
        assert_eq!(
            files::sha256(
                flightdeck::resources::asset(&format!("scripts/runtime/{name}")).unwrap()
            ),
            lock["runtime_scripts"]["files"][name]
        );
    }
}
