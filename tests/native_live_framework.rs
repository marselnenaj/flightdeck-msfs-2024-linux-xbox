// SPDX-License-Identifier: MIT
//! Explicit opt-in test: new synthetic profiles, cached Microsoft installers, no account.
#![allow(clippy::unwrap_used)]
use flightdeck::{
    backend::Launcher, bootstrap, fenix_bundle, files, framework, framework_repair, process,
    transaction as tx, wine::StagedWine,
};
use serde_json::json;
use std::{fs, path::PathBuf};
#[test]
#[ignore = "requires the pinned Wine runner and cached Microsoft installers; creates a fresh build directory"]
fn native_framework_install_repair_and_restart() {
    let input = |name| PathBuf::from(std::env::var_os(name).expect("explicit live-test input"));
    let runner = input("FLIGHTDECK_TEST_RUNNER").canonicalize().unwrap();
    let cache = input("FLIGHTDECK_TEST_CACHE").canonicalize().unwrap();
    let output = input("FLIGHTDECK_TEST_OUTPUT");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    assert!(
        output.is_absolute() && output.starts_with(repo.join("build")) && !files::exists(&output)
    );
    fenix_bundle::runner_variant(&runner, false).unwrap();
    for (name, _, hash) in framework_repair::PACKAGES {
        assert_eq!(tx::digest(&cache.join(name)).unwrap(), hash);
    }
    files::private_dir(&output).unwrap();
    let app = Launcher::new(output.join("state"), None).unwrap();
    let ctx = app.reserve("framework-live", "test", false).unwrap();
    let active = output.join("local/msfs-prefix");
    files::private_dir(active.parent().unwrap()).unwrap();
    bootstrap::prepare_prefix(&runner, &active, &ctx.cancel).unwrap();
    let before = tx::digest(&active.join("system.reg")).unwrap();
    let stage = output.join("local/staged-prefix");
    process::copy_tree(&active, &stage, &ctx.cancel).unwrap();
    tx::relocate_prefix_links(&active, &stage, &ctx.cancel).unwrap();
    let mut wine = StagedWine::new(
        &output,
        &stage,
        &runner,
        &output.join("framework.log"),
        &ctx,
    )
    .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let obtain = |name: &str, _url: &str, hash: &str| {
            let path = cache.join(name);
            assert_eq!(tx::digest(&path).unwrap(), hash);
            Ok(path)
        };
        println!("Rust: fresh .NET install in an unpublished synthetic profile");
        framework_repair::prepare(&mut wine, obtain, |s| println!("{s}")).unwrap();
        wine.stop().unwrap();
        assert!(framework::status(&stage).ready);
        let clr = framework::framework_path(&stage, "Framework", "clr.dll").unwrap();
        let expected = tx::digest(&clr).unwrap();
        fs::rename(&clr, clr.with_extension("dll.retained-for-test")).unwrap();
        assert!(!framework::status(&stage).ready);
        println!("Rust: repair a deliberately missing x86 CLR");
        framework_repair::prepare(&mut wine, obtain, |s| println!("{s}")).unwrap();
        wine.stop().unwrap();
        assert!(framework::status(&stage).ready);
        assert_eq!(tx::digest(&clr).unwrap(), expected);
        let mut messages = Vec::new();
        framework_repair::prepare(
            &mut wine,
            |_, _, _| panic!("healthy restart must not download"),
            |s| messages.push(s.to_string()),
        )
        .unwrap();
        wine.stop().unwrap();
        assert_eq!(
            messages,
            ["Microsoft .NET Framework 4.8 is already installed."]
        );
        assert_eq!(tx::digest(&active.join("system.reg")).unwrap(), before);
        assert!(!framework::status(&active).ready);
        files::atomic_json(&output.join("result.json"),&json!({"passed":true,"implementation":"rust","account_calls":false,"fresh_install":true,"missing_x86_clr_repaired":true,"same_clr_hash":true,"idempotent_restart":true,"active_profile_unchanged":true,"status":framework::status(&stage)})).unwrap();
    }));
    wine.stop().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
