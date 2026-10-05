// SPDX-License-Identifier: MIT
//! Explicit, account-free execution of the official application's install hook.
#![allow(clippy::unwrap_used)]
use flightdeck::{bootstrap, fenix_installer, files, process, wine::Wine};
use serde_json::json;
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Duration};

#[test]
#[ignore = "requires cached official FenixApp and .NET Desktop files; uses a new synthetic prefix"]
fn official_fenix_hook_with_persisted_wine_environment() {
    let input = |key| PathBuf::from(std::env::var_os(key).expect("explicit fixture input"));
    let runner = input("FLIGHTDECK_TEST_RUNNER").canonicalize().unwrap();
    let application = input("FLIGHTDECK_TEST_FENIX_APP").canonicalize().unwrap();
    let dotnet = input("FLIGHTDECK_TEST_DOTNET").canonicalize().unwrap();
    let output = input("FLIGHTDECK_TEST_OUTPUT");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    assert!(
        output.is_absolute() && output.starts_with(repo.join("build")) && !files::exists(&output)
    );
    let metadata =
        flightdeck::xml::parse(&files::read(&application.join("sq.version"), 65536).unwrap())
            .unwrap();
    let metadata = metadata.child("metadata").expect("package metadata");
    assert_eq!(metadata.child("id").unwrap().text(), "FenixApp");
    let version = metadata.child("version").unwrap().text();
    assert!(
        !version.is_empty()
            && version.len() < 64
            && version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
    );
    files::private_dir(&output).unwrap();
    let cancel = AtomicBool::new(false);
    let prefix = output.join("prefix");
    bootstrap::prepare_prefix(&runner, &prefix, &cancel).unwrap();
    let current = prefix.join("drive_c/flightdeck-hook-test/current");
    files::private_dir(current.parent().unwrap()).unwrap();
    process::copy_tree(&application, &current, &cancel).unwrap();
    process::copy_tree(
        &dotnet,
        &prefix.join("drive_c/Program Files/dotnet"),
        &cancel,
    )
    .unwrap();
    let log = output.join("hook.log");
    let wine = Wine::new(&prefix, &runner, &log, &cancel).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // No main application launch, activation, login, or account data: only
        // the Velopack hook which exits before Fenix starts its normal UI.
        fenix_installer::prepare(&wine).unwrap();
        wine.stop_staged().unwrap();
        let offset = wine.log.metadata().unwrap().len();
        let mut command = wine.command().unwrap();
        command
            .arg(current.join("FenixApp.exe"))
            .args(["--veloapp-install", &version])
            .current_dir(&current)
            .env_remove("DOTNET_SYSTEM_GLOBALIZATION_USENLS")
            .env_remove("DOTNET_ReadyToRun");
        let code = process::run(&mut command, Duration::from_secs(60), &cancel).unwrap();
        assert!(code.success(), "install hook exit {code:?}");
        fenix_installer::check_log(&log, &wine.log, offset).unwrap();
        let registry =
            String::from_utf8(files::read(&prefix.join("user.reg"), 64 * 1024 * 1024).unwrap())
                .unwrap();
        assert!(registry.contains("\"DOTNET_SYSTEM_GLOBALIZATION_USENLS\"=\"1\""));
        files::atomic_json(&output.join("result.json"),&json!({"passed":true,"fenix_version":version,"application_sha256":flightdeck::transaction::digest(&current.join("FenixApp.exe")).unwrap(),"hook":"--veloapp-install","exit_code":code.code(),"unix_dotnet_overrides_removed":true,"persisted_prefix_environment":true,"account_calls":false,"existing_prefixes_modified":false})).unwrap();
    }));
    wine.stop_staged().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
