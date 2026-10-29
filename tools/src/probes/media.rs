// SPDX-License-Identifier: MIT
//! Read a local video with a synthetic SourceReader in a new Wine prefix.
use super::{
    Site,
    host::{self, Environment},
    lab,
};
use crate::util::{Result, require};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(clap::Args)]
pub struct Arguments {
    #[arg(long)]
    runner: PathBuf,
    /// A local video to read, never modified or copied into a package.
    #[arg(long)]
    video: PathBuf,
    #[arg(long, value_parser = clap::value_parser!(u32).range(2..=16384))]
    width: u32,
    #[arg(long, value_parser = clap::value_parser!(u32).range(2..=16384))]
    height: u32,
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u32).range(2..=30000))]
    frames: u32,
    /// Use the selected-Proton media path instead of the default runner's host codecs.
    #[arg(long)]
    runner_media: bool,
    /// Exercise the NVIDIA software-decoder preference, even without an NVIDIA GPU.
    #[arg(long)]
    prefer_software: bool,
    /// New directory for the isolated prefix, logs and result.
    #[arg(long)]
    output: PathBuf,
}

pub fn run(site: &Site, args: &Arguments) -> Result<u8> {
    let runner = host::resolve(&args.runner)?;
    let video = host::resolve(&args.video)?;
    require(
        host::proton_style(&runner),
        "The runner has no Wine and wineserver",
    )?;
    require(video.is_file(), "The local video is missing")?;
    let output = host::resolve(&args.output)?;
    host::new_dirs(&output, host::PRIVATE)?;
    let root = output.join("runtime");
    host::new_dirs(&root.join("private"), host::PRIVATE)?;
    host::new_dirs(&root.join("local"), host::PRIVATE)?;
    std::os::unix::fs::symlink(&runner, root.join("runner"))?;
    lab::mark(&root)?;
    let prepared = site.lab.call_with_stderr(
        "media-env",
        json!({"root":root,
        "runner_media":args.runner_media,"prefer_software":args.prefer_software}),
        &output.join("media-selection.log"),
    )?;
    let mut env: Environment = serde_json::from_value(prepared["environment"].clone())?;
    for category in ["CONFIG", "DATA", "CACHE", "STATE"] {
        let path = root.join("private/xdg").join(category.to_lowercase());
        host::dirs(&path, host::PRIVATE)?;
        env.insert(format!("XDG_{category}_HOME"), host::text(&path));
    }
    env.insert(
        "WINEDLLOVERRIDES".into(),
        "winemenubuilder.exe,mscoree,mshtml=d".into(),
    );
    env.insert("GST_DEBUG".into(), "2,GST_ELEMENT_FACTORY:4".into());
    env.remove("DISPLAY");
    env.remove("WAYLAND_DISPLAY");
    let binary = output.join("media-decode.exe");
    host::checked(
        Command::new(&site.compiler)
            .args(["-O2", "-municode"])
            .arg(site.root.join("tests/graphics/media-decode.c"))
            .arg("-o")
            .arg(&binary)
            .args(["-lmfreadwrite", "-lmfplat", "-lmfuuid", "-lole32", "-luuid"]),
    )?;
    // Use the launcher's 64-bit loader choice, including older Proton layouts
    // whose `wine` is a separate 32-bit executable.
    let wine =
        PathBuf::from(env.get("WINE").ok_or_else(|| {
            crate::util::Failure("The media driver selected no Wine loader".into())
        })?);
    let server = runner.join("files/bin/wineserver");
    let ran = (|| -> Result<i32> {
        let boot = host::logged(
            Command::new(&wine)
                .args(["wineboot", "-u"])
                .env_clear()
                .envs(&env)
                .current_dir(&output),
            &output.join("wineboot.log"),
            90,
        )?;
        require(boot == 0, "The isolated media prefix could not be prepared")?;
        host::logged(
            Command::new(&wine)
                .arg(&binary)
                .arg(format!("Z:{}", host::text(&video).replace('/', "\\")))
                .args([
                    args.width.to_string(),
                    args.height.to_string(),
                    args.frames.to_string(),
                ])
                .env_clear()
                .envs(&env)
                .current_dir(&output),
            &output.join("decode.log"),
            180,
        )
    })();
    let stopped = host::stop_wine(&server, &env, |command| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    });
    // Retain the decoded evidence even if a runner hangs in MFShutdown.
    // Such a run still fails; frames alone never override a timeout.
    let finished = host::finally(ran, stopped);
    let code = finished.as_ref().ok().copied();
    let log = fs::read_to_string(output.join("decode.log")).unwrap_or_default();
    let decoded = verdict(&log, args.width, args.height, args.frames);
    let passed = code == Some(0) && decoded;
    let report = json!({"scope":"Media Foundation NV12 decode and rewind; no game/account or GPU presentation",
        "source":prepared["source"],"h264":prepared["h264"],"overlay":prepared["overlay"],"width":args.width,"height":args.height,
        "frames_per_pass":args.frames,"passes":2,"exit_code":code,"decoding_passed":decoded,
        "end_of_stream":log.contains("flightdeck-media-probe: reached end of stream"),
        "shutdown_completed":log.contains("flightdeck-media-probe: closed"),"passed":passed});
    host::write(&output.join("result.json"), &host::pretty(&report)?)?;
    println!("{}", host::pretty(&report)?);
    if let Err(error) = finished {
        eprintln!("{error}");
    }
    Ok(if passed { 0 } else { 1 })
}

fn verdict(log: &str, width: u32, height: u32, frames: u32) -> bool {
    let bytes = u64::from(width) * u64::from(height) * 3 / 2 * u64::from(frames);
    (1..=2).all(|pass| log.lines().any(|line| line.trim_end() == format!(
        "flightdeck-media-probe: pass={pass} hr=00000000 width={width} height={height} frames={frames} bytes={bytes}"
    ))) && !log.contains("placeholder-video-used")
}

#[cfg(test)]
mod tests {
    use super::verdict;
    #[test]
    fn only_two_real_video_passes_can_succeed() {
        let row = |pass, width, height, bytes| {
            format!(
                "flightdeck-media-probe: pass={pass} hr=00000000 width={width} height={height} frames=120 bytes={bytes}\n"
            )
        };
        let real = row(1, 3840, 2160, 1492992000_u64) + &row(2, 3840, 2160, 1492992000_u64);
        assert!(verdict(&real, 3840, 2160, 120));
        assert!(!verdict(
            &row(1, 3840, 2160, 1492992000_u64),
            3840,
            2160,
            120
        ));
        let blank = row(1, 320, 240, 13824000_u64) + &row(2, 320, 240, 13824000_u64);
        assert!(!verdict(&blank, 3840, 2160, 120));
        assert!(!verdict(
            &(real + "placeholder-video-used"),
            3840,
            2160,
            120
        ));
    }
}
