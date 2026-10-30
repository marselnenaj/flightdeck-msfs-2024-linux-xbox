// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{bootstrap, files};
use std::{io::Cursor, sync::atomic::AtomicBool};

#[test]
#[ignore = "requires FLIGHTDECK_RUNNER_ARCHIVE pointing to the pinned XZ and the system xz command"]
fn pinned_runner_xz_matches_independent_decoder_byte_for_byte() {
    use sha2::{Digest, Sha256};
    use std::{
        fs::File,
        io::{BufReader, Read},
        process::{Command, Stdio},
    };
    fn stream_hash(mut reader: impl Read) -> (u64, String) {
        let mut hash = Sha256::new();
        let mut length = 0_u64;
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            let count = reader.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            length += count as u64;
            hash.update(&buffer[..count]);
        }
        (length, hex::encode(hash.finalize()))
    }
    let path = std::path::PathBuf::from(std::env::var_os("FLIGHTDECK_RUNNER_ARCHIVE").unwrap());
    let lock = flightdeck::resources::json("compat/bootstrap.lock.json").unwrap();
    assert_eq!(
        files::digest(File::open(&path).unwrap()).unwrap(),
        lock["runner"]["archive_sha256"]
    );
    let mut reference = Command::new("xz")
        .arg("-dc")
        .arg(&path)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let expected = stream_hash(reference.stdout.take().unwrap());
    assert!(reference.wait().unwrap().success());
    let actual = stream_hash(lzma_rust2::XzReader::new(
        BufReader::new(File::open(&path).unwrap()),
        false,
    ));
    assert_eq!(actual, expected);
    println!(
        "Pinned runner decoded: {} bytes, SHA-256 {}",
        actual.0, actual.1
    );
}
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
fn xz_component_archives_interoperate_and_retain_extraction_guards() {
    // Independent GNU tar/XZ fixtures, hex-encoded for the source allowlist.
    // They contain folder/file = "hello", or a link escaping the destination.
    let safe = hex::decode(include_str!("fixtures/bootstrap-tar-xz.txt").trim()).unwrap();
    let unsafe_link =
        hex::decode(include_str!("fixtures/bootstrap-unsafe-tar-xz.txt").trim()).unwrap();
    for (bytes, valid) in [
        (safe.clone(), true),
        (unsafe_link, false),
        (safe[..safe.len() / 2].to_vec(), false),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("components.tar.xz");
        let target = temp.path().join("extracted");
        std::fs::write(&path, bytes).unwrap();
        files::private_dir(&target).unwrap();
        let result = bootstrap::extract(&path, &target, &AtomicBool::new(false));
        assert_eq!(result.is_ok(), valid);
        if valid {
            assert_eq!(std::fs::read(target.join("folder/file")).unwrap(), b"hello");
        }
        assert!(!temp.path().join("outside").exists());
    }
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
