// SPDX-License-Identifier: MIT
//! Real file transitions and frozen migration contracts; no game or account.
#![allow(clippy::unwrap_used)]
use flightdeck::{files, graphics_settings as gs};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
const CONFIG: &str = "Version 66\r\n{Video\r\n\tAdapter \"NVIDIA GeForce RTX 4080\"\r\n\tAntiAliasing DLSS\r\n\tReflex ONBOOST\r\n\tFrameGeneration DLSSG\r\n\tAntiAliasingVR DLSS\r\n\tReflexVR ON\r\n\tFrameGenerationVR DLSSG\r\n\tResolution 3840 2160\r\n}\r\n{Graphics\r\n\t{Texture\r\n\t\tQuality 3\r\n\t}\r\n}\r\nInstalledPackagesPath \"C:\\Flüge\\Packages\"\r\n";
fn fixture(root: &Path) -> PathBuf {
    let config = root.join("local/msfs-prefix/drive_c/users/steamuser/AppData/Roaming/Microsoft Flight Simulator 2024/UserCfg.opt");
    files::private_dir(config.parent().unwrap()).unwrap();
    files::private_dir(&root.join("private")).unwrap();
    files::atomic(&config, CONFIG.as_bytes()).unwrap();
    config
}
#[test]
fn legacy_undo_records_survive_restarts_and_migration() {
    let legacy: Value = serde_json::from_str(include_str!(
        "fixtures/legacy-python/graphics-settings.json"
    ))
    .unwrap();
    assert_eq!(legacy["original"], CONFIG);
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("native");
    let config = fixture(&root);
    let reference = t.path().join("legacy");
    let pyconfig = fixture(&reference);
    files::atomic(&pyconfig, legacy["prepared"].as_str().unwrap().as_bytes()).unwrap();
    files::atomic_json(&pyconfig.with_file_name(gs::MARKER), &legacy["marker"]).unwrap();
    let result = gs::prepare(&root, true).unwrap();
    assert_eq!(result, legacy["prepare_result"]);
    assert_eq!(fs::read(&config).unwrap(), fs::read(&pyconfig).unwrap());
    let marker: Value = files::json(&config.with_file_name(gs::MARKER), 65536).unwrap();
    assert_eq!(marker, legacy["marker"]);
    let stamp = config.metadata().unwrap().modified().unwrap();
    assert_eq!(gs::prepare(&root, true).unwrap()["changed_files"], 0);
    assert_eq!(config.metadata().unwrap().modified().unwrap(), stamp);
    assert!(
        !fs::read_to_string(config.with_file_name(gs::MARKER))
            .unwrap()
            .contains("Packages")
    );
    // Restore a recorded Python prefix and a newly prepared native prefix.
    assert_eq!(
        gs::prepare(&reference, false).unwrap(),
        legacy["restore_result"]
    );
    assert_eq!(gs::prepare(&root, false).unwrap(), legacy["restore_result"]);
    assert_eq!(fs::read(&config).unwrap(), CONFIG.as_bytes());
    assert_eq!(fs::read(&pyconfig).unwrap(), CONFIG.as_bytes());
    assert!(!config.with_file_name(gs::MARKER).exists());
    assert!(!pyconfig.with_file_name(gs::MARKER).exists());
}
#[test]
fn restore_keeps_new_user_settings_and_other_frame_generators() {
    let t = tempfile::tempdir().unwrap();
    let config = fixture(t.path());
    gs::prepare(t.path(), true).unwrap();
    let edited = fs::read_to_string(&config)
        .unwrap()
        .replace("AntiAliasing TAA", "AntiAliasing FSR")
        .replace("FrameGeneration NONE", "FrameGeneration FSRFG")
        .replace("3840 2160", "1920 1080");
    files::atomic(&config, edited.as_bytes()).unwrap();
    gs::prepare(t.path(), false).unwrap();
    let restored = fs::read_to_string(config).unwrap();
    assert!(restored.contains("AntiAliasing FSR\r\n"));
    assert!(restored.contains("FrameGeneration FSRFG\r\n"));
    assert!(restored.contains("Reflex ONBOOST\r\n"));
    assert!(restored.contains("Resolution 1920 1080"));
}
#[test]
fn all_encodings_and_line_endings_round_trip_exactly() {
    for text in [
        CONFIG.to_owned(),
        CONFIG
            .replace("\r\n", "\n")
            .trim_end_matches('\n')
            .to_owned(),
    ] {
        let encodings = [
            text.as_bytes().to_vec(),
            [0xef, 0xbb, 0xbf].into_iter().chain(text.bytes()).collect(),
            [0xff, 0xfe]
                .into_iter()
                .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
                .collect(),
            [0xfe, 0xff]
                .into_iter()
                .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
                .collect(),
        ];
        for data in encodings {
            let (edited, original, changed) = gs::transform(&data, &gs::Undo::new(), true).unwrap();
            assert_eq!(changed.len(), 6);
            assert_eq!(gs::transform(&edited, &original, false).unwrap().0, data);
        }
    }
}
#[test]
fn ambiguous_files_and_linked_paths_are_preserved() {
    let t = tempfile::tempdir().unwrap();
    let config = fixture(t.path());
    for text in [
        format!("{CONFIG}{{Video\n}}\n"),
        CONFIG.replace("Reflex ONBOOST", "Reflex OFF\n\tReflex ONBOOST"),
        "{Video\n".into(),
    ] {
        files::atomic(&config, text.as_bytes()).unwrap();
        assert_eq!(gs::prepare(t.path(), true).unwrap()["skipped_files"], 1);
        assert_eq!(fs::read(&config).unwrap(), text.as_bytes());
    }
    files::atomic(&config, CONFIG.as_bytes()).unwrap();
    let original = config.with_file_name("original");
    fs::rename(&config, &original).unwrap();
    symlink(&original, &config).unwrap();
    assert_eq!(gs::prepare(t.path(), true).unwrap()["skipped_files"], 1);
    fs::remove_file(&config).unwrap();
    fs::hard_link(&original, &config).unwrap();
    assert_eq!(gs::prepare(t.path(), true).unwrap()["skipped_files"], 1);
    fs::remove_file(&config).unwrap();
    fs::rename(&original, &config).unwrap();
    let parent = config.parent().unwrap();
    let external = parent.with_file_name("external");
    fs::rename(parent, &external).unwrap();
    symlink(&external, parent).unwrap();
    assert_eq!(gs::prepare(t.path(), true).unwrap()["skipped_files"], 1);
    assert_eq!(
        fs::read(external.join("UserCfg.opt")).unwrap(),
        CONFIG.as_bytes()
    );
}
#[test]
fn interrupted_apply_keeps_undo_and_different_editions_are_not_touched() {
    let t = tempfile::tempdir().unwrap();
    let config = fixture(t.path());
    let (_, original, _) = gs::transform(CONFIG.as_bytes(), &gs::Undo::new(), true).unwrap();
    files::atomic_json(
        &config.with_file_name(gs::MARKER),
        &json!({"schema":1,"original":original}),
    )
    .unwrap();
    gs::prepare(t.path(), true).unwrap();
    gs::prepare(t.path(), false).unwrap();
    assert_eq!(fs::read(&config).unwrap(), CONFIG.as_bytes());
    files::atomic_json(
        &t.path().join("private/runtime.json"),
        &json!({"game_id":"msfs2020"}),
    )
    .unwrap();
    assert_eq!(gs::prepare(t.path(), true).unwrap()["changed_files"], 0);
    assert_eq!(fs::read(&config).unwrap(), CONFIG.as_bytes());
}

#[test]
fn invalid_optional_package_identity_keeps_roaming_settings_available() {
    let t = tempfile::tempdir().unwrap();
    let config = fixture(t.path());
    let game = flightdeck::games::Game::Msfs2024.path(t.path());
    files::private_dir(&game).unwrap();
    files::atomic(&game.join("MicrosoftGame.Config"), b"<broken").unwrap();
    assert_eq!(gs::prepare(t.path(), true).unwrap()["changed_files"], 1);
    assert_eq!(gs::prepare(t.path(), false).unwrap()["changed_files"], 1);
    assert_eq!(fs::read(config).unwrap(), CONFIG.as_bytes());
}
