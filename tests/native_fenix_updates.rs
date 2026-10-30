// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{
    backend::{Context, Launcher},
    fenix_bundle as bundle, fenix_update as update, files,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

#[test]
fn embedded_bundle_satisfies_the_same_contract_as_downloaded_updates() {
    update::compatible_manifest(&bundle::manifest(None).unwrap()).unwrap();
}

fn future() -> Value {
    let mut lock = bundle::manifest(None).unwrap();
    let version = lock["version"].as_str().unwrap().to_string();
    let variants = lock["variants"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for variant in std::iter::once(None).chain(variants.iter().map(|v| Some(v.as_str()))) {
        let old = bundle::manifest(variant).unwrap();
        let target = match variant {
            None => &mut lock,
            Some(v) => &mut lock["variants"][v],
        };
        target["previous_releases"][&version] = json!({"files":old["files"],"integration":old["integration"],"prefix_files":old["prefix_files"]});
    }
    let (base, patch) = version.split('-').next().unwrap().rsplit_once('.').unwrap();
    let v = format!("{base}.{}", patch.parse::<u64>().unwrap() + 1);
    lock["version"] = json!(v);
    json!({"format":1,"installer_contract":2,"minimum_flightdeck_version":"0.2.22","bundle":lock,
        "release":{"version":v,"url":format!("{}/releases/download/v{v}/fenix-a320-linux-patch-{v}-linux-x86_64.zip", update::PROJECT),"archive_root":format!("fenix-a320-linux-patch-{v}"),"sha256":"a".repeat(64),"size":100}})
}
fn metadata(value: &Value) -> (Vec<u8>, Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    let meta = json!({"size":bytes.len(),"sha256":files::sha256(&bytes),"version":value["release"]["version"]});
    (bytes, meta)
}
fn release(v: &str) -> Value {
    json!({"draft":false,"prerelease":v.contains("preview"),"tag_name":format!("v{v}"),"assets":[{"name":update::ASSET,"browser_download_url":format!("{}/releases/download/v{v}/{}",update::PROJECT, update::ASSET),"digest":format!("sha256:{}","a".repeat(64)),"size":1024,"state":"uploaded"}]})
}
fn context(base: &Path, app: Arc<Launcher>) -> Context {
    Context {
        launcher: app,
        id: "synthetic-fenix-update".into(),
        kind: "fenix".into(),
        runtime: Some(base.join("runtime")),
        cancel: Arc::new(AtomicBool::new(false)),
        pause: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn discovers_highest_preview_then_stable_without_downgrading_or_reentering_previews() {
    let mut draft = release("9.0.0");
    draft["draft"] = json!(true);
    let raw = json!([
        release("0.1.0-preview.9"),
        release("0.1.0-preview.12"),
        release("0.1.0-preview.11"),
        draft
    ]);
    assert_eq!(
        update::release_metadata(&raw, "0.1.0-preview.11")
            .unwrap()
            .unwrap()["version"],
        "0.1.0-preview.12"
    );
    let feed = json!([
        release("0.1.0-preview.12"),
        release("0.1.0"),
        release("0.2.0-preview.1")
    ]);
    assert_eq!(update::release_metadata(&feed, "0.1.0").unwrap(), None);
    assert_eq!(
        update::release_metadata(&feed, "0.1.0-preview.11")
            .unwrap()
            .unwrap()["version"],
        "0.1.0"
    );
    assert_eq!(
        update::release_metadata(
            &json!([release("0.1.0"), release("0.1.0-preview.12")]),
            "0.1.0-preview.11"
        )
        .unwrap()
        .unwrap()["version"],
        "0.1.0"
    );
    for bad in [
        "0.1.0-preview.0",
        "0.1.0-preview.01",
        "0.1.0-preview.a",
        "0.1.0-rc.1",
        "0.01.0",
        "../0.1.0",
    ] {
        assert!(update::version(bad).is_err(), "{bad}");
    }
    let mut foreign = release("0.1.0-preview.12");
    foreign["assets"][0]["browser_download_url"] = json!("https://example.invalid/payload");
    assert!(update::release_metadata(&json!([foreign]), "0.1.0-preview.11").is_err());
    assert!(
        update::release_metadata(
            &json!([release("0.1.0-preview.12"), release("0.1.0-preview.12")]),
            "0.1.0-preview.11"
        )
        .is_err()
    );
}

#[test]
fn binds_descriptor_bytes_version_runner_and_installer_contract() {
    let value = future();
    let (bytes, meta) = metadata(&value);
    assert_eq!(update::descriptor(&bytes, &meta).unwrap(), value);
    let mut corrupt = bytes.clone();
    corrupt[10] ^= 1;
    assert!(update::descriptor(&corrupt, &meta).is_err());
    for field in ["runner_version", "protocol", "display_renderer"] {
        let mut v = value.clone();
        v["bundle"][field] = json!("unsupported");
        let (data, m) = metadata(&v);
        assert!(update::descriptor(&data, &m).is_err(), "{field}");
    }
    for pointer in [
        "/installer_contract",
        "/format",
        "/minimum_flightdeck_version",
        "/release/version",
        "/release/url",
        "/release/archive_root",
        "/release/sha256",
        "/bundle/files/..~1outside",
    ] {
        let mut v = value.clone();
        match pointer {
            "/minimum_flightdeck_version" => v["minimum_flightdeck_version"] = json!("99.0.0"),
            "/bundle/files/..~1outside" => {
                v["bundle"]["files"]["../outside"] = json!("a".repeat(64))
            }
            _ => *v.pointer_mut(pointer).unwrap() = json!("invalid"),
        }
        let (data, m) = metadata(&v);
        assert!(update::descriptor(&data, &m).is_err(), "{pointer}");
    }
}

#[test]
fn independent_payload_survives_service_restart_and_keeps_versions_when_offline() {
    let t = tempfile::tempdir().unwrap();
    let app = Launcher::new(t.path().join("state"), None).unwrap();
    let ctx = context(t.path(), Arc::clone(&app));
    let mut v = future();
    // An isolated future payload with all existing destinations and new bytes.
    let bytes = b"synthetic independently released native payload";
    let hash = files::sha256(bytes);
    for value in v["bundle"]["files"].as_object_mut().unwrap().values_mut() {
        *value = json!(hash);
    }
    for variant in v["bundle"]["variants"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        for value in variant["files"].as_object_mut().unwrap().values_mut() {
            *value = json!(hash);
        }
    }
    for value in v["bundle"]["integration"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *value = json!(hash);
    }
    for helper in [
        "FenixWindowGuard.exe",
        "FenixMCDURefresh.exe",
        "fenix-runtime",
    ] {
        v["bundle"]["prefix_files"][format!("drive_c/windows/system32/{helper}")] = json!(hash);
    }
    let cache = t.path().join("cache");
    files::private_dir(&cache).unwrap();
    let archive = cache.join(format!("{}.zip", v["release"]["version"].as_str().unwrap()));
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    let root = v["release"]["archive_root"].as_str().unwrap();
    zip.start_file(format!("{root}/bundle.json"), options)
        .unwrap();
    zip.write_all(&serde_json::to_vec(&v["bundle"]).unwrap())
        .unwrap();
    let mut names = Vec::new();
    for name in v["bundle"]["files"].as_object().unwrap().keys() {
        names.push(format!("payload/{name}"));
    }
    for (variant, entry) in v["bundle"]["variants"].as_object().unwrap() {
        for name in entry["files"].as_object().unwrap().keys() {
            names.push(format!("payload/variants/{variant}/{name}"));
        }
    }
    for name in v["bundle"]["integration"].as_object().unwrap().keys() {
        if name != bundle::LEGACY_DISPLAY_HELPER {
            names.push(format!("integration/{name}"));
        }
    }
    for name in names {
        zip.start_file(format!("{root}/{name}"), options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    v["release"]["sha256"] = json!(flightdeck::transaction::digest(&archive).unwrap());
    v["release"]["size"] = json!(archive.metadata().unwrap().len());
    let (data, meta) = metadata(&v);
    let v = update::descriptor(&data, &meta).unwrap();
    app.lock().fenix_updates.candidate = v.clone();
    let extracted = bundle::obtain(&cache, None, &ctx).unwrap();
    assert_eq!(
        bundle::payload_manifest(&extracted, None).unwrap(),
        v["bundle"]
    );
    assert!(
        bundle::verify(&extracted).is_err(),
        "future version is not the embedded release"
    );
    files::private_dir(&ctx.root().unwrap().join("private")).unwrap();
    files::atomic_json(&ctx.root().unwrap().join(flightdeck::fenix::MARKER), &json!({"state":"installed","version":v["release"]["version"],"bundle_manifest":v["bundle"],"bundle_release":v["release"]})).unwrap();
    let restarted = Launcher::new(t.path().join("state"), None).unwrap();
    let restarted = context(t.path(), restarted);
    assert_eq!(update::selected(&restarted).unwrap()["bundle"], v["bundle"]);
    assert_eq!(bundle::obtain(&cache, None, &restarted).unwrap(), extracted);
    assert!(
        !update::ensure(&restarted).unwrap(),
        "offline startup must not downgrade or reinstall the retained version"
    );
    assert_eq!(
        update::installed_manifest(
            &json!({"version":v["release"]["version"],"bundle_manifest":v["bundle"]}),
            Some("cachyos-11")
        )
        .unwrap()["version"],
        v["release"]["version"]
    );
    fs::write(
        extracted.join("payload/files/lib/wine/x86_64-windows/ntdll.dll"),
        b"tampered",
    )
    .unwrap();
    assert!(bundle::obtain(&cache, None, &restarted).is_err());
    restarted
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(update::download(&v["release"], &archive, &restarted).is_err());
    assert!(matches!(
        update::ensure(&restarted),
        Err(flightdeck::Error::Cancelled)
    ));
}

#[test]
#[ignore = "explicit public GitHub metadata check; no Wine or existing profile is used"]
fn public_stable_descriptor_matches_the_embedded_release() {
    let descriptor = update::discover("0.1.0-preview.10", &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(descriptor["bundle"], bundle::manifest(None).unwrap());
    let embedded = flightdeck::resources::json("compat/fenix/release.json").unwrap();
    for field in ["version", "url", "sha256", "archive_root"] {
        assert_eq!(descriptor["release"][field], embedded[field]);
    }
}
