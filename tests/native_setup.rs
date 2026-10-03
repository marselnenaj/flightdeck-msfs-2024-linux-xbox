// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{backend::Launcher, bootstrap, files, game_package, games::Game, setup};
use serde_json::json;
use std::{fs, os::unix::fs::symlink, path::Path};
fn write(path: &Path, data: &[u8]) {
    files::private_dir(path.parent().unwrap()).unwrap();
    files::atomic(path, data).unwrap();
}
fn config(store: &str) -> String {
    format!(
        r#"<Game><Identity Name="Flightdeck.Synthetic" Publisher="CN=Test &amp; Test" Version="1.2.3.4"/><Executable Name="FlightSimulator2024.exe"/><StoreId>{store}</StoreId></Game>"#
    )
}
#[test]
fn utf16_and_namespaced_packages_have_the_same_identity() {
    let xml = config(Game::Msfs2024.store_id());
    let first = game_package::parse(xml.as_bytes()).unwrap();
    let utf16: Vec<u8> = [0xff, 0xfe]
        .into_iter()
        .chain(xml.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    assert_eq!(
        first.identity,
        game_package::parse(&utf16).unwrap().identity
    );
    assert_eq!(first.identity.unwrap().publisher, "CN=Test & Test");
    let namespaced = xml
        .replace("<Game>", "<g:Game xmlns:g=\"urn:test\">")
        .replace("</Game>", "</g:Game>");
    assert_eq!(
        game_package::parse(namespaced.as_bytes()).unwrap().store_id,
        Game::Msfs2024.store_id()
    );
}
#[test]
fn ambiguous_and_entity_declared_packages_fail() {
    let xml = config(Game::Msfs2024.store_id());
    for bad in [
        format!("<!DOCTYPE Game [<!ENTITY e 'x'>]>{xml}"),
        xml.replace("</Game>", "<StoreId>different</StoreId></Game>"),
        xml.replace("</Game>", "<Identity Name=\"duplicate\"/></Game>"),
        xml.replace("</Game>", "</Other>"),
    ] {
        assert!(game_package::parse(bad.as_bytes()).is_err(), "{bad}");
    }
    assert!(game_package::version("1.2.65536.0").is_err());
    assert!(game_package::version("1.2.3").is_err());
}
#[test]
fn incomplete_or_wrong_game_download_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("MicrosoftGame.Config"),
        config(Game::Msfs2024.store_id()).as_bytes(),
    );
    write(
        &temp.path().join("FlightSimulator2024.exe"),
        b"encrypted bytes",
    );
    write(&temp.path().join(".xodus-streaming.msixvc"), b"complete");
    game_package::validate_download(temp.path(), Game::Msfs2024).unwrap();
    assert!(game_package::validate_download(temp.path(), Game::Msfs2020).is_err());
    write(&temp.path().join(".xodus-streaming-tmp.msixvc"), b"partial");
    assert!(game_package::validate_download(temp.path(), Game::Msfs2024).is_err());
}
#[test]
fn prepare_copies_profile_and_relocates_internal_links_without_touching_source() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    let prefix = base.join("input-prefix");
    let runner = base.join("runner");
    let artifacts = base.join("artifacts");
    let game = base.join("game");
    files::private_dir(&game).unwrap();
    write(
        &prefix.join("drive_c/windows/system32/keep.dll"),
        b"original",
    );
    write(&prefix.join("system.reg"), b"registry");
    write(&prefix.join("user.reg"), b"user registry");
    files::private_dir(&prefix.join("dosdevices")).unwrap();
    symlink(prefix.join("drive_c"), prefix.join("dosdevices/c:")).unwrap();
    write(
        &runner.join("files/lib/wine/x86_64-windows/xgameruntime.dll"),
        b"original runtime",
    );
    let mut hashes = json!({});
    let spec = json!({"features":["connected-storage-read-v1"],"files":flightdeck::resources::json("compat/bootstrap.lock.json").unwrap()["native"]["files"]});
    for name in bootstrap::artifact_names(&spec).unwrap() {
        let data = format!("synthetic {name}");
        write(&artifacts.join(name), data.as_bytes());
        hashes[name] = json!(files::sha256(data.as_bytes()));
    }
    let destination = base.join("new-runtime");
    let app = Launcher::new(base.join("state"), None).unwrap();
    let ctx = app.reserve("setup", "prepare", false).unwrap();
    let plan = json!({"mode":"prepare","game_id":"msfs2024","market":"AT","local_saves":true,"destination_path":destination,"prefix_path":prefix,"runner_path":runner,"game_path":game,"artifacts_path":artifacts,"original_hash":files::sha256(b"original runtime"),"manifest":{"format":1,"features":["connected-storage-read-v1"],"files":hashes}});
    setup::prepare(&plan, &ctx, None).unwrap();
    assert_eq!(
        destination
            .join("local/msfs-prefix/dosdevices/c:")
            .canonicalize()
            .unwrap(),
        destination.join("local/msfs-prefix/drive_c")
    );
    write(
        &destination.join("local/msfs-prefix/dosdevices/c:/windows/system32/keep.dll"),
        b"trial only",
    );
    assert_eq!(
        fs::read(prefix.join("drive_c/windows/system32/keep.dll")).unwrap(),
        b"original"
    );
    assert!(destination.join("private/local-saves.enabled").exists());
    assert!(setup::prepare(&plan, &ctx, None).is_err());
}
#[test]
fn ready_cancellation_and_shutdown_release_the_setup_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let app = Launcher::new(temp.path().join("state"), None).unwrap();
    let ctx = app.reserve("setup", "check", false).unwrap();
    app.finish(&ctx, Ok(json!({"state":"ready"})), true);
    app.cancel("setup", &ctx.id).unwrap();
    assert!(app.lock().active.is_none());
    let ctx = app.reserve("setup", "check", false).unwrap();
    app.finish(&ctx, Ok(json!({"state":"ready"})), true);
    app.close();
    assert!(app.lock().active.is_none());
}
