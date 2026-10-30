// SPDX-License-Identifier: MIT
//! Explicit release-payload checks using cached public archives and read-only runners.
use flightdeck::{backend::Launcher, fenix_bundle, files, resources, transaction};
use serde_json::Value;
use std::{collections::BTreeMap, fs, os::unix::fs::symlink, path::PathBuf};

#[test]
#[ignore = "requires the public patch ZIP, preceding ZIP and original runner paths; never starts Wine"]
fn published_payload_updates_every_runner_and_prefix_without_changing_originals() {
    let input = |key| PathBuf::from(std::env::var_os(key).expect("explicit release input"));
    let archive = input("FLIGHTDECK_TEST_FENIX_ARCHIVE");
    let previous = input("FLIGHTDECK_TEST_FENIX_PREVIOUS_ARCHIVE");
    let runners: BTreeMap<String, PathBuf> = serde_json::from_slice(
        &fs::read(input("FLIGHTDECK_TEST_FENIX_RUNNERS")).expect("runner paths"),
    )
    .expect("runner map");
    let manifest = fenix_bundle::manifest(None).expect("manifest");
    let expected: std::collections::BTreeSet<_> = manifest["variants"]
        .as_object()
        .expect("variants")
        .keys()
        .map(String::as_str)
        .chain(["xodus"])
        .collect();
    assert_eq!(
        runners
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    let temp = tempfile::tempdir().expect("isolated output");
    let app = Launcher::new(temp.path().join("state"), None).expect("launcher");
    let ctx = app
        .reserve("probe", "fenix-release", false)
        .expect("context");
    let cache = temp.path().join("bundles");
    files::private_dir(&cache).expect("cache");
    let release = resources::json("compat/fenix/release.json").expect("release");
    assert_eq!(
        transaction::digest(&archive).expect("ZIP hash"),
        release["sha256"]
    );
    fs::copy(
        &archive,
        cache.join(format!(
            "{}.zip",
            release["version"].as_str().expect("version")
        )),
    )
    .expect("cached public ZIP");
    let bundle = fenix_bundle::obtain(&cache, None, &ctx).expect("production archive extraction");
    assert_eq!(
        fenix_bundle::verify(&bundle).expect("payload hashes"),
        bundle
    );

    let mut old_zip = zip::ZipArchive::new(fs::File::open(previous).expect("preceding ZIP"))
        .expect("preceding archive");
    let old_manifest_name = (0..old_zip.len())
        .find_map(|index| {
            let entry = old_zip.by_index(index).expect("ZIP entry");
            let name = entry.name().expect("member name");
            name.ends_with("/bundle.json").then(|| name.into_owned())
        })
        .expect("preceding manifest");
    let old_root = old_manifest_name
        .strip_suffix("bundle.json")
        .expect("archive root")
        .to_owned();
    let old: Value =
        serde_json::from_reader(old_zip.by_name(&old_manifest_name).expect("manifest"))
            .expect("preceding manifest JSON");

    for (name, original) in runners {
        let variant = (name != "xodus").then_some(name.as_str());
        let lock = fenix_bundle::manifest(variant).expect("current variant");
        let old_lock =
            fenix_bundle::select_variant(old.clone(), variant).expect("preceding variant");
        assert_eq!(
            lock["previous_releases"][old["version"].as_str().expect("old version")]["files"],
            old_lock["files"]
        );
        let work = temp.path().join(&name);
        let runner = work.join("runner");
        let prefix = work.join("prefix");
        files::private_dir(&runner).expect("runner");
        files::private_dir(&prefix.join("drive_c/windows/system32")).expect("prefix");
        fs::copy(original.join("version"), runner.join("version")).expect("runner version");
        let originals = lock["runner_files"]
            .as_object()
            .expect("original checksums");
        for (relative, hash) in originals {
            let source = original.join(relative);
            assert_eq!(transaction::digest(&source).expect("original hash"), *hash);
            let target = runner.join(relative);
            files::private_dir(target.parent().expect("parent")).expect("runner directory");
            fs::copy(&source, &target).expect("isolated original module");
            if relative.starts_with("files/lib/wine/x86_64-windows/") {
                symlink(
                    &source,
                    prefix
                        .join("drive_c/windows/system32")
                        .join(target.file_name().expect("DLL name")),
                )
                .expect("original prefix DLL link");
            }
        }
        let payload = variant.map_or_else(
            || "payload/".to_owned(),
            |v| format!("payload/variants/{v}/"),
        );
        for (relative, hash) in old_lock["files"].as_object().expect("preceding files") {
            let mut entry = old_zip
                .by_name(&format!("{old_root}{payload}{relative}"))
                .expect("preceding module");
            let target = runner.join(relative);
            files::private_dir(target.parent().expect("parent")).expect("module directory");
            if target.exists() {
                fs::remove_file(&target).expect("replace isolated original module");
            }
            std::io::copy(
                &mut entry,
                &mut fs::File::create(&target).expect("preceding copy"),
            )
            .expect("preceding module bytes");
            assert_eq!(transaction::digest(&target).expect("preceding hash"), *hash);
        }
        let aircraft = prefix.join("installed-aircraft-sentinel");
        assert_eq!(
            fenix_bundle::runner_variant_with(&runner, true, &manifest)
                .expect("recognize preserved preceding patch for Proton restore"),
            variant.map(str::to_string)
        );
        assert!(
            fenix_bundle::runner_variant_with(&runner, false, &manifest).is_err(),
            "a patched backup is not an unmodified runner"
        );
        fs::write(&aircraft, b"retain aircraft and preferences").expect("sentinel");
        fenix_bundle::overlay(&prefix, &runner, &bundle, variant, &ctx)
            .expect("production overlay");
        fenix_bundle::verify_runner(&runner, &lock, true).expect("current runner");
        let runtime = prefix.join("drive_c/windows/system32/fenix-runtime");
        assert_eq!(
            transaction::digest(&runtime).expect("native runtime hash"),
            lock["integration"]["fenix-runtime"]
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            runtime
                .metadata()
                .expect("native runtime metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        for (relative, hash) in lock["files"].as_object().expect("current files") {
            if relative.starts_with("files/lib/wine/x86_64-windows/")
                || relative == flightdeck::fenix_setup::DXGI_FILE
            {
                let target = prefix.join("drive_c/windows/system32").join(
                    std::path::Path::new(relative)
                        .file_name()
                        .expect("DLL name"),
                );
                assert!(!target.is_symlink());
                assert_eq!(transaction::digest(&target).expect("prefix module"), *hash);
            }
        }
        assert_eq!(
            fs::read(aircraft).expect("retained sentinel"),
            b"retain aircraft and preferences"
        );
        for (relative, hash) in originals {
            assert_eq!(
                transaction::digest(&original.join(relative)).expect("unchanged original"),
                *hash
            );
        }
        println!(
            "{name}: preceding payload upgraded, prefix DLL links replaced, originals retained"
        );
    }
}
