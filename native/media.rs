// SPDX-License-Identifier: MIT
//! Media settings for the game's Wine child, never the Store or broker.
use crate::{Error, Result, files};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub type Environment = BTreeMap<OsString, OsString>;

/// Both layouts are used by supported Proton releases. Do not mix a runner's
/// GStreamer plugins with host libraries: their GLib/FFmpeg ABIs can differ.
fn libraries(runner: &Path) -> Vec<PathBuf> {
    [
        "files/lib/x86_64-linux-gnu",
        "files/lib64",
        "files/lib/i386-linux-gnu",
        "files/lib",
    ]
    .into_iter()
    .map(|name| runner.join(name))
    .filter(|path| path.is_dir())
    .collect()
}

pub fn runner_libraries(runner: &Path, env: &mut Environment) -> Result<()> {
    if !bundles_codecs(runner) {
        return Ok(());
    }
    let mut paths = libraries(runner);
    if paths.is_empty() {
        return Ok(());
    }
    let original = env
        .get(OsStr::new("LD_LIBRARY_PATH"))
        .cloned()
        .unwrap_or_default();
    env.entry("ORIG_LD_LIBRARY_PATH".into()).or_insert(original);
    if let Some(old) = env.get(OsStr::new("LD_LIBRARY_PATH")) {
        for path in std::env::split_paths(old) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    env.insert(
        "LD_LIBRARY_PATH".into(),
        std::env::join_paths(paths).map_err(|_| Error::Invalid("Invalid Proton library path."))?,
    );
    Ok(())
}

fn bundles_codecs(runner: &Path) -> bool {
    libraries(runner).iter().any(|directory| {
        std::fs::read_dir(directory).is_ok_and(|entries| {
            entries.take(4096).flatten().any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                (name == "libavcodec.so" || name.starts_with("libavcodec.so."))
                    && entry.path().is_file()
            })
        })
    })
}

/// `runner_media` is true for a selected Proton. The default Xodus build's
/// bundled media converter substitutes a small test video for H.264; outside
/// Steam it has no transcoded-video cache. Keep its working host-codec path.
pub fn configure(
    root: &Path,
    runner: &Path,
    runner_media: bool,
    nvidia: bool,
    env: &mut Environment,
) -> Result<(&'static str, &'static str)> {
    // Stock Steam Proton also contains GStreamer plugins but relies on Steam
    // for restricted codecs/transcoded media. Only use a bundled stack when
    // the selected runner actually supplies its FFmpeg codec library.
    let runner_media = runner_media && bundles_codecs(runner);
    for (key, name) in [
        ("MEDIACONV_BLANK_VIDEO_FILE", "blank.mkv"),
        ("MEDIACONV_BLANK_AUDIO_FILE", "blank.ptna"),
    ] {
        env.entry(key.into())
            .or_insert_with(|| runner.join("files/share/media").join(name).into_os_string());
    }
    if runner_media {
        runner_libraries(runner, env)?;
        let plugins: Vec<_> = libraries(runner)
            .into_iter()
            // MSFS and this probe are 64-bit. Scanning 32-bit plugins only
            // creates ELFCLASS32 failures in the 64-bit registry.
            .filter(|path| path.file_name().is_none_or(|n| n != "i386-linux-gnu"))
            .map(|path| path.join("gstreamer-1.0"))
            .filter(|path| path.is_dir())
            .collect();
        if !plugins.is_empty() {
            env.entry("GST_PLUGIN_SYSTEM_PATH_1_0".into()).or_insert(
                std::env::join_paths(plugins)
                    .map_err(|_| Error::Invalid("Invalid Proton media path."))?,
            );
        }
    } else if root.join("local/media-plugins").is_dir() {
        let mut plugins = vec![root.join("local/media-plugins")];
        if let Some(old) = env.get(OsStr::new("GST_PLUGIN_PATH_1_0")) {
            plugins.extend(std::env::split_paths(old));
        }
        env.insert(
            "GST_PLUGIN_PATH_1_0".into(),
            std::env::join_paths(plugins)
                .map_err(|_| Error::Invalid("Invalid media plugin path."))?,
        );
    }
    let source = if runner_media { "runner" } else { "host" };
    let identity = if runner_media {
        let runner = runner.canonicalize()?;
        format!(
            "runner-{}",
            &files::sha256(runner.as_os_str().as_encoded_bytes())[..16]
        )
    } else {
        "host".to_string()
    };
    let cache = root.join("private/gstreamer");
    files::private_dir(&cache)?;
    let cache = cache.join(identity);
    files::private_dir(&cache)?;
    env.insert("WINE_GST_REGISTRY_DIR".into(), cache.as_os_str().into());
    env.insert(
        "GST_REGISTRY_1_0".into(),
        cache.join("registry.bin").into_os_string(),
    );
    // Per-frame colorimetry warnings can flood a looping loading video.
    // The maintainer probe opts into decoder detail separately.
    env.entry("GST_DEBUG".into()).or_insert("1".into());
    env.entry("GST_DEBUG_NO_COLOR".into()).or_insert("1".into());

    // Prefer CPU H.264 decoding when it exists. Do not disable a working
    // hardware decoder if the host has no avdec_h264, or change explicit ranks.
    let rank = env.get(OsStr::new("GST_PLUGIN_FEATURE_RANK"));
    let explicit = rank.is_some_and(|value| {
        value.to_string_lossy().split(',').any(|entry| {
            entry
                .split_once(':')
                .is_some_and(|(name, _)| name.trim().contains("h264"))
        })
    });
    let decoder = if explicit {
        "explicit"
    } else if nvidia {
        let mut value = rank.cloned().unwrap_or_default();
        if !value.is_empty() {
            value.push(",");
        }
        value.push("avdec_h264:512");
        env.insert("GST_PLUGIN_FEATURE_RANK".into(), value);
        "software_preferred"
    } else {
        "default"
    };
    Ok((source, decoder))
}

/// Prefer the runner's complete stack. If its H.264 plugin cannot load, use a
/// separately verified host stack, including the host library search path.
/// Explicit media choices are left to the user.
pub fn configure_verified(
    root: &Path,
    runner: &Path,
    selected: bool,
    nvidia: bool,
    env: &mut Environment,
) -> Result<(&'static str, &'static str)> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    configure_checked(
        root,
        runner,
        selected,
        nvidia,
        env,
        |env, source| {
            let result = (|| -> Result<bool> {
                let mut command = std::process::Command::new(std::env::current_exe()?);
                command
                    .args(["--native-probe", "media"])
                    .env_clear()
                    .envs(env);
                let bytes = crate::process::output(
                    &mut command,
                    std::time::Duration::from_secs(5),
                    4096,
                    &cancel,
                )?;
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                let valid = value["schema"] == 1
                    && [
                        "ready",
                        "incomplete",
                        "initialization_failed",
                        "unavailable",
                    ]
                    .contains(&value["state"].as_str().unwrap_or(""));
                if !valid {
                    return Err(Error::Invalid("Invalid media probe."));
                }
                eprintln!(
                    "flightdeck-media-probe: source={source} state={}",
                    value["state"].as_str().unwrap_or("unavailable")
                );
                for name in crate::native_probe::MEDIA_ELEMENTS {
                    let ready = value["elements"][name] == true;
                    eprintln!(
                        "flightdeck-media-element: source={source} name={name} ready={}",
                        u8::from(ready)
                    );
                }
                Ok(value["state"] == "ready" && value["h264"] == true)
            })();
            if let Err(error) = &result {
                let state = if matches!(
                    error,
                    Error::Invalid("Der Vorgang hat nicht rechtzeitig geantwortet.")
                ) {
                    "timeout"
                } else {
                    "probe_failed"
                };
                eprintln!("flightdeck-media-probe: source={source} state={state}");
            }
            result.unwrap_or(false)
        },
        |env| {
            let mut command = std::process::Command::new(crate::runtime::wine(runner));
            command.arg("--version").env_clear().envs(env);
            crate::process::output(
                &mut command,
                std::time::Duration::from_secs(5),
                4096,
                &cancel,
            )
            .is_ok()
        },
    )
}

fn configure_checked(
    root: &Path,
    runner: &Path,
    selected: bool,
    nvidia: bool,
    env: &mut Environment,
    h264: impl Fn(&Environment, &'static str) -> bool,
    wine: impl Fn(&Environment) -> bool,
) -> Result<(&'static str, &'static str)> {
    let original = env.clone();
    let explicit = [
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_PATH_1_0",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_FEATURE_RANK",
    ]
    .iter()
    .any(|key| original.contains_key(OsStr::new(key)));
    let configured = configure(root, runner, selected, nvidia, env)?;
    if explicit {
        eprintln!("flightdeck-media-selection: reason=explicit");
        return Ok(configured);
    }
    let ready = h264(env, configured.0);
    if configured.0 != "runner" || ready {
        eprintln!(
            "flightdeck-media-selection: reason={}",
            if configured.0 == "host" {
                "initial_host"
            } else {
                "runner"
            }
        );
        return Ok(configured);
    }
    let mut host = original;
    let host_configured = configure(root, runner, false, nvidia, &mut host)?;
    if h264(&host, "host") && wine(&host) {
        *env = host;
        eprintln!("flightdeck-media-selection: reason=verified_host_fallback");
        eprintln!(
            "flightdeck-media: bundled H.264 decoder unavailable; verified host codec stack selected"
        );
        Ok(host_configured)
    } else {
        eprintln!("flightdeck-media-selection: reason=runner_unverified");
        Ok(configured)
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    #[test]
    fn initial_host_selection_is_probed_but_does_not_block_hardware_fallback() {
        let temp = tempfile::tempdir().expect("runtime");
        let count = std::cell::Cell::new(0);
        let mut env = Environment::new();
        let configured = configure_checked(
            temp.path(),
            &temp.path().join("runner"),
            false,
            false,
            &mut env,
            |_, source| {
                assert_eq!(source, "host");
                count.set(count.get() + 1);
                false
            },
            |_| panic!("host selection does not replace the runner"),
        )
        .expect("missing software plugins must not block a hardware decoder");
        assert_eq!(configured.0, "host");
        assert_eq!(count.get(), 1);
    }
    #[test]
    fn broken_bundled_decoder_falls_back_only_to_a_verified_complete_host_environment() {
        let temp = tempfile::tempdir().expect("runtime");
        let root = temp.path();
        let runner = root.join("runner");
        let libs = runner.join("files/lib/x86_64-linux-gnu");
        files::private_dir(&libs.join("gstreamer-1.0")).expect("plugins");
        files::atomic(&libs.join("libavcodec.so.62"), b"codec").expect("codec");
        files::private_dir(&runner.join("files/lib/i386-linux-gnu/gstreamer-1.0"))
            .expect("32-bit plugins");
        for host_ready in [true, false] {
            let mut env = Environment::from([("LD_LIBRARY_PATH".into(), "/host/libraries".into())]);
            let result = configure_checked(
                root,
                &runner,
                true,
                false,
                &mut env,
                |env, _| host_ready && !env.contains_key(OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")),
                |_| true,
            )
            .expect("media");
            assert_eq!(result.0, if host_ready { "host" } else { "runner" });
            if host_ready {
                assert_eq!(env[OsStr::new("LD_LIBRARY_PATH")], "/host/libraries");
            } else {
                assert!(
                    !env[OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")]
                        .to_string_lossy()
                        .contains("i386")
                );
            }
        }
        let mut explicit =
            Environment::from([("GST_PLUGIN_SYSTEM_PATH_1_0".into(), "/user/plugins".into())]);
        configure_checked(
            root,
            &runner,
            true,
            false,
            &mut explicit,
            |_, _| panic!("explicit choices must not be probed"),
            |_| false,
        )
        .expect("explicit");
        assert_eq!(
            explicit[OsStr::new("GST_PLUGIN_SYSTEM_PATH_1_0")],
            "/user/plugins"
        );
    }
}
