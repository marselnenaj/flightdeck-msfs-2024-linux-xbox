// SPDX-License-Identifier: MIT
#![allow(clippy::unwrap_used)]
use flightdeck::{files, media};
use std::{ffi::OsStr, path::Path};

fn configured(
    root: &Path,
    runner: &Path,
    selected: bool,
    nvidia: bool,
    env: &mut media::Environment,
) {
    media::configure(root, runner, selected, nvidia, env).unwrap();
}

#[test]
fn default_runner_uses_host_codecs_and_nvidia_prefers_available_software_decoder() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let runner = root.join("runner");
    files::private_dir(&runner.join("files/lib/x86_64-linux-gnu/gstreamer-1.0")).unwrap();
    files::private_dir(&root.join("local/media-plugins")).unwrap();
    let mut env = media::Environment::from([
        ("LD_LIBRARY_PATH".into(), "/host/libraries".into()),
        ("GST_PLUGIN_FEATURE_RANK".into(), "vp9dec:128".into()),
    ]);
    configured(root, &runner, false, true, &mut env);
    assert_eq!(
        env[OsStr::new("GST_REGISTRY_1_0")],
        Path::new(&env[OsStr::new("WINE_GST_REGISTRY_DIR")]).join("registry.x86_64.bin")
    );
    assert_eq!(env[OsStr::new("LD_LIBRARY_PATH")], "/host/libraries");
    assert!(!env.contains_key(OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")));
    assert_eq!(
        env[OsStr::new("GST_PLUGIN_PATH_1_0")],
        root.join("local/media-plugins")
    );
    assert_eq!(
        env[OsStr::new("GST_PLUGIN_FEATURE_RANK")],
        "vp9dec:128,avdec_h264:512"
    );
    // A repeated configure is idempotent and never disables hardware fallback.
    configured(root, &runner, false, true, &mut env);
    assert_eq!(
        env[OsStr::new("GST_PLUGIN_FEATURE_RANK")],
        "vp9dec:128,avdec_h264:512"
    );
}

#[test]
fn selected_proton_keeps_its_ffmpeg_and_plugins_together_with_a_separate_registry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let runner = root.join("runner with spaces");
    let libs = runner.join("files/lib/x86_64-linux-gnu");
    files::private_dir(&libs.join("gstreamer-1.0")).unwrap();
    files::atomic(&libs.join("libavcodec.so.62"), b"synthetic codec library").unwrap();
    files::private_dir(&root.join("local/media-plugins")).unwrap();
    let mut original =
        media::Environment::from([("LD_LIBRARY_PATH".into(), "/host/libraries".into())]);
    configured(root, &runner, false, false, &mut original);
    let host_registry = original[OsStr::new("GST_REGISTRY_1_0")].clone();
    let mut env = media::Environment::from([("LD_LIBRARY_PATH".into(), "/host/libraries".into())]);
    configured(root, &runner, true, false, &mut env);
    assert_eq!(env[OsStr::new("ORIG_LD_LIBRARY_PATH")], "/host/libraries");
    assert_eq!(
        std::env::split_paths(&env[OsStr::new("LD_LIBRARY_PATH")])
            .next()
            .unwrap(),
        libs
    );
    assert_eq!(
        env[OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")],
        libs.join("gstreamer-1.0")
    );
    assert!(!env.contains_key(OsStr::new("GST_PLUGIN_PATH_1_0")));
    assert_ne!(env[OsStr::new("GST_REGISTRY_1_0")], host_registry);
    assert!(!env.contains_key(OsStr::new("GST_PLUGIN_FEATURE_RANK")));
}

#[test]
fn selected_steam_runner_without_codec_libraries_keeps_the_working_host_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let runner = root.join("runner");
    files::private_dir(&runner.join("files/lib/x86_64-linux-gnu/gstreamer-1.0")).unwrap();
    let mut env = media::Environment::new();
    assert_eq!(
        media::configure(root, &runner, true, true, &mut env)
            .unwrap()
            .0,
        "host"
    );
    assert!(!env.contains_key(OsStr::new("LD_LIBRARY_PATH")));
    assert!(!env.contains_key(OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")));
}

#[test]
fn explicit_media_settings_are_retained_including_hardware_decoder_choice() {
    let temp = tempfile::tempdir().unwrap();
    for ranks in ["avdec_h264:0", "nvh264dec:512", "vah264dec:512"] {
        let mut env = media::Environment::from([
            ("GST_PLUGIN_FEATURE_RANK".into(), ranks.into()),
            ("GST_DEBUG".into(), "0".into()),
            (
                "GST_PLUGIN_SYSTEM_PATH_1_0".into(),
                "/explicit/plugins".into(),
            ),
        ]);
        configured(temp.path(), temp.path(), true, true, &mut env);
        assert_eq!(env[OsStr::new("GST_PLUGIN_FEATURE_RANK")], ranks);
        assert_eq!(env[OsStr::new("GST_DEBUG")], "0");
        assert_eq!(
            env[OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")],
            "/explicit/plugins"
        );
    }
}
