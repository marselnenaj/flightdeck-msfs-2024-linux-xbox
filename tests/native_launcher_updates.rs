// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, files, installer, launcher_update as update, startup_updates};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
fn metadata() -> Value {
    json!({"draft":false,"prerelease":false,"tag_name":"v0.3.0","body":"Release notes","assets":[{"name":update::ASSET,"digest":format!("sha256:{}","a".repeat(64)),"size":1234,"state":"uploaded","browser_download_url":format!("{}/releases/download/v0.3.0/{}",update::PROJECT,update::ASSET)}]})
}
#[test]
fn prepared_clients_choose_the_newest_stable_release_independently_of_legacy_latest() {
    let native = metadata();
    let mut old = metadata();
    old["tag_name"] = json!("v0.1.22");
    let mut preview = metadata();
    preview["prerelease"] = json!(true);
    preview["tag_name"] = json!("v0.4.0");
    let mut draft = metadata();
    draft["draft"] = json!(true);
    for values in [json!([old, native, preview, draft]), json!([native, old])] {
        assert_eq!(update::stable_release(&values).unwrap()["version"], "0.3.0");
    }
    let mut bad = native.clone();
    bad["assets"][0]["digest"] = json!("sha256:bad");
    for values in [
        Value::Null,
        json!({}),
        json!([]),
        json!([preview, draft]),
        json!([native, native]),
        json!(vec![native; 51]),
        json!([old, bad]),
    ] {
        assert!(update::stable_release(&values).is_err());
    }
}
#[test]
fn release_identity_is_exact_and_public_urls_cannot_be_redirected_to_other_hosts() {
    let raw = metadata();
    let release = update::release_metadata(&raw).unwrap();
    assert_eq!(release["version"], "0.3.0");
    for bad in ["v01.2.3", "1.2", "0.3.0-dev", "0.3.0\n", "9999999.0.0"] {
        assert!(update::version(bad).is_err());
    }
    for key in ["draft", "prerelease"] {
        let mut bad = raw.clone();
        bad[key] = json!(true);
        assert!(update::release_metadata(&bad).is_err());
    }
    for (key, value) in [
        ("size", json!(1234.0)),
        ("digest", json!(format!("sha256:{}", "A".repeat(64)))),
        (
            "browser_download_url",
            json!("https://github.com/another/release"),
        ),
        ("state", json!("new")),
    ] {
        let mut bad = raw.clone();
        bad["assets"][0][key] = value;
        assert!(update::release_metadata(&bad).is_err());
    }
    let mut duplicate = raw.clone();
    duplicate["assets"]
        .as_array_mut()
        .unwrap()
        .push(raw["assets"][0].clone());
    assert!(update::release_metadata(&duplicate).is_err());
    for url in [
        "http://github.com/test",
        "https://github.com.evil.invalid/test",
        "https://github.com@evil.invalid/test",
        "https://user@github.com/test",
        "https://github.com:444/test",
        "https://objects.githubusercontent.com/test#fragment",
    ] {
        assert!(!update::safe_url(url));
    }
    for url in [
        update::API,
        "https://release-assets.githubusercontent.com/test",
        "https://objects.githubusercontent.com:443/test",
    ] {
        assert!(update::safe_url(url));
    }
}
fn tar(path: &Path, entries: &[(&str, Vec<u8>, u8)]) {
    let file = fs::File::create(path).unwrap();
    let zipped = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut out = tar::Builder::new(zipped);
    for (name, data, kind) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::new(*kind));
        let raw = h.as_mut_bytes();
        raw[..name.len()].copy_from_slice(name.as_bytes());
        h.set_cksum();
        out.append(&h, data.as_slice()).unwrap();
    }
    out.into_inner()
        .unwrap()
        .finish()
        .unwrap()
        .sync_all()
        .unwrap();
}
fn entries() -> Vec<(&'static str, Vec<u8>, u8)> {
    let binary = b"\x7fELFfixture".to_vec();
    let package = json!({"schema":1,"kind":"rust-launcher","version":"0.3.0","files":{"bin/flightdeck":files::sha256(&binary)}});
    vec![
        ("flightdeck-linux/bin/flightdeck", binary, b'0'),
        (
            "flightdeck-linux/FLIGHTDECK-PACKAGE.json",
            serde_json::to_vec(&package).unwrap(),
            b'0',
        ),
    ]
}
#[test]
fn archives_require_matching_version_binary_hash_and_unique_canonical_files() {
    let t = tempfile::tempdir().unwrap();
    let archive = t.path().join("update.gz");
    tar(&archive, &entries());
    let root = update::extract(
        &archive,
        &t.path().join("good"),
        "0.3.0",
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(installer::package_executable(&root).is_ok());
    assert!(
        update::extract(
            &archive,
            &t.path().join("wrong-version"),
            "0.4.0",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let mut bad = entries();
    bad[0].1.push(b'X');
    tar(&archive, &bad);
    assert!(
        update::extract(
            &archive,
            &t.path().join("wrong-hash"),
            "0.3.0",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    for (index, name, kind) in [
        (0, "flightdeck-linux/../escaped", b'0'),
        (1, "flightdeck-linux/link", b'2'),
        (2, "/absolute", b'0'),
        (3, "flightdeck-linux/a\\b", b'0'),
        (4, "flightdeck-linux/./file", b'0'),
        (5, "flightdeck-linux/sparse", b'S'),
    ] {
        let mut bad = entries();
        bad.push((name, vec![], kind));
        tar(&archive, &bad);
        assert!(
            update::extract(
                &archive,
                &t.path().join(format!("bad-{index}")),
                "0.3.0",
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    let mut bad = entries();
    bad.push(bad[0].clone());
    tar(&archive, &bad);
    assert!(
        update::extract(
            &archive,
            &t.path().join("duplicate"),
            "0.3.0",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(!t.path().join("escaped").exists());
    tar(&archive, &entries());
    assert!(
        update::extract(
            &archive,
            &t.path().join("cancelled"),
            "0.3.0",
            &AtomicBool::new(true)
        )
        .is_err()
    );
}
#[test]
fn oversized_headers_and_broken_gzip_are_rejected_before_unbounded_output() {
    let t = tempfile::tempdir().unwrap();
    let archive = t.path().join("update.gz");
    let mut raw = tar::Header::new_gnu();
    raw.set_path("flightdeck-linux/huge").unwrap();
    raw.set_mode(0o644);
    raw.set_size(128 * 1024 * 1024 + 1);
    raw.set_cksum();
    let mut out = flate2::write::GzEncoder::new(
        fs::File::create(&archive).unwrap(),
        flate2::Compression::fast(),
    );
    out.write_all(raw.as_bytes()).unwrap();
    out.finish().unwrap();
    assert!(
        update::extract(
            &archive,
            &t.path().join("huge"),
            "0.3.0",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(!t.path().join("huge/flightdeck-linux/huge").exists());
    fs::write(&archive, b"bad gzip").unwrap();
    assert!(
        update::extract(
            &archive,
            &t.path().join("bad-gzip"),
            "0.3.0",
            &AtomicBool::new(false)
        )
        .is_err()
    );
}
#[test]
fn discovery_never_reserves_a_game_and_cancellation_is_bound_to_its_job() {
    let t = tempfile::tempdir().unwrap();
    let app = Launcher::new(t.path().join("state"), None).unwrap();
    {
        let mut s = app.lock();
        s.launcher_updates.attempted = Some(Instant::now());
        s.launcher_updates.release = update::release_metadata(&metadata()).unwrap();
        s.launcher_updates.check_id = Some("fresh-check".into());
    }
    assert_eq!(startup_updates::check(&app).unwrap(), json!({"ok":true}));
    assert!(app.lock().active.is_none());
    let view = update::snapshot(&app);
    assert_eq!(view["managed"], false);
    assert_eq!(view["can_install"], false);
    assert_eq!(view["update_available"], true);
    assert!(update::start(&app, "install", Some("fresh-check")).is_err());
    assert!(update::restart(&app, 0).is_err());
    let owned = Arc::new(AtomicBool::new(false));
    {
        let mut s = app.lock();
        s.launcher_updates.worker = Some(Arc::clone(&owned));
        s.launcher_updates.job = json!({"id":"new","state":"running","can_cancel":true});
    }
    assert!(update::cancel(&app, "old").is_err());
    assert!(!owned.load(Ordering::Relaxed));
    update::cancel(&app, "new").unwrap();
    assert!(owned.load(Ordering::Relaxed));
    {
        let mut s = app.lock();
        s.launcher_updates.job["can_cancel"] = json!(false);
    }
    assert!(update::cancel(&app, "new").is_err());
}
