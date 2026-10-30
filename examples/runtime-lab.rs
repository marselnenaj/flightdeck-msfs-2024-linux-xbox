// SPDX-License-Identifier: MIT
//! Development-only driver for synthetic Wine probes. Never shipped or used by
//! the launcher. The probes of `tools/` compile test programs and collect their logs;
//! prefix, graphics, VR, Fenix and save logic comes from the Rust library.
use flightdeck::{
    Error, Result,
    backend::Launcher,
    bootstrap,
    error::require,
    fenix, fenix_bundle, fenix_setup, files, graphics, native_probe, process, proton, runtime,
    save_state, transaction as tx, vr,
    wine::{self, StagedWine},
};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str> {
    input[key]
        .as_str()
        .ok_or(Error::Invalid("Missing probe input."))
}
fn path(input: &Value, key: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(text(input, key)?))
}
fn synthetic(input: &Value) -> Result<PathBuf> {
    let root = path(input, "root")?.canonicalize()?;
    require(
        files::read(&root.join("private/synthetic-probe"), 64)? == b"Flightdeck isolated test\n",
        "This operation requires a new isolated probe runtime.",
    )?;
    Ok(root)
}
fn fenix_fixture(root: &Path, payload: &Path) -> Result<Value> {
    let payload = fenix_bundle::verify(payload)?;
    let app = Launcher::new(root.join("private/probe-state"), None)?;
    let ctx = app.reserve("probe", "fenix", false)?;
    let work = root.join("local/fenix-patch-20261003T000000-00000000");
    require(
        !files::exists(&work),
        "Probe Fenix directory already exists.",
    )?;
    files::private_dir(&work)?;
    let runner = work.join("runner");
    process::copy_tree(&root.join("runner").canonicalize()?, &runner, &ctx.cancel)?;
    let active = root.join("local/msfs-prefix");
    let stage = work.join("prefix");
    process::copy_tree(&active, &stage, &ctx.cancel)?;
    tx::relocate_prefix_links(&active, &stage, &ctx.cancel)?;
    let wine = StagedWine::new(root, &stage, &runner, &work.join("geometry.log"), &ctx)?;
    let cache = work.join("downloads");
    files::private_dir(&cache)?;
    // Reuse optional maintainer inputs without letting setup write into them.
    for name in ["Windows6.1-KB2670838-x64.msu", "msdelta.dll"] {
        let source = payload.join("build/downloads").join(name);
        if source.is_file() {
            tx::copy(
                std::fs::File::open(source)?,
                &cache.join(name),
                None,
                &ctx.cancel,
            )?;
        }
    }
    let prepared = fenix_setup::geometry(&wine, &cache, &payload, &ctx);
    let stopped = wine.stop();
    prepared?;
    stopped?;
    fenix_bundle::overlay(&stage, &runner, &payload, None, &ctx)?;
    tx::relocate_prefix_links(&stage, &stage, &ctx.cancel)?;
    let backup = work.join("original-prefix");
    std::fs::rename(&active, &backup)?;
    std::fs::rename(&stage, &active)?;
    fenix::replace_link(&root.join("runner"), &runner)?;
    let version = fenix_bundle::manifest(None)?["version"].clone();
    files::atomic_json(
        &root.join(fenix::MARKER),
        &json!({"format":1,"state":"installed","version":version,
        "work":"local/fenix-patch-20261003T000000-00000000","backup":"private/fenix-fixture","configured":false}),
    )?;
    Ok(json!({"bundle":payload,"version":version}))
}
fn run(input: Value) -> Result<Value> {
    let cancel = AtomicBool::new(false);
    match text(&input, "operation")? {
        "probe" => Ok(graphics::probe(true)),
        "prefix" => {
            let root = synthetic(&input)?;
            let prefix = root.join("local/msfs-prefix");
            require(!files::exists(&prefix), "Probe prefix already exists.")?;
            bootstrap::prepare_prefix(&root.join("runner"), &prefix, &cancel)?;
            Ok(json!(true))
        }
        "graphics" => {
            let root = synthetic(&input)?;
            let (env, report) =
                graphics::prepare(&root, serde_json::from_value(input["environment"].clone())?)?;
            Ok(json!([env, report]))
        }
        "mode" => {
            let mut env = serde_json::from_value(input["environment"].clone())?;
            graphics::apply_mode(&mut env, text(&input, "mode")?);
            Ok(json!(env))
        }
        "renderer" => {
            let root = synthetic(&input)?;
            let bundle = input["bundle"].as_str().map(Path::new);
            Ok(json!(graphics::renderer_from(&root, bundle)?))
        }
        "vr" => Ok(json!(vr::prepare(
            &synthetic(&input)?,
            serde_json::from_value(input["environment"].clone())?,
            &cancel
        )?)),
        "proton-info" => {
            let root = path(&input, "root")?;
            let selected = proton::selection(&root)?;
            let base = selected
                .as_ref()
                .and_then(|v| v["base_runner"].as_str())
                .map(PathBuf::from)
                .unwrap_or(root.join("runner"))
                .canonicalize()?;
            Ok(
                json!({"selection":selected,"base_runner":base,"version":fenix_bundle::version(&root.join("runner"))?,
                "wine":runtime::wine(&root.join("runner")),"bridge":proton::BRIDGE}),
            )
        }
        "wine-env" => {
            let root = synthetic(&input)?;
            Ok(json!(wine::environment(
                &root.join("local/msfs-prefix"),
                &root.join("runner")
            )))
        }
        "media-env" => {
            let root = synthetic(&input)?;
            let mut env = wine::environment(&root.join("local/msfs-prefix"), &root.join("runner"))
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect();
            let (source, decoder) = flightdeck::media::configure_verified(
                &root,
                &root.join("runner"),
                input["runner_media"] == true,
                input["prefer_software"] == true,
                &mut env,
            )?;
            let wine = root.join("runner/files/bin/wine").canonicalize()?;
            let (media_wine, overlay) =
                flightdeck::media_overlay::prepare(&root, &root.join("runner"), &wine, &mut env)?;
            env.insert("WINE".into(), media_wine.into_os_string());
            let env: std::collections::BTreeMap<String, String> = env
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            Ok(json!({"environment":env,"source":source,"h264":decoder,"overlay":overlay}))
        }
        "fenix-environment" => {
            let root = synthetic(&input)?;
            let wine = wine::Wine::new(
                &root.join("local/msfs-prefix"),
                &root.join("runner"),
                &root.join("private/fenix-environment.log"),
                &cancel,
            )?;
            flightdeck::fenix_installer::prepare(&wine)?;
            Ok(json!(true))
        }
        "maintenance-reset" | "maintenance-preview" => {
            let root = synthetic(&input)?;
            let state = root
                .parent()
                .ok_or(Error::Invalid("Missing probe parent."))?
                .join(format!(
                    "maintenance-state-{}",
                    uuid::Uuid::new_v4().simple()
                ));
            let app = Launcher::new(state, root.to_str())?;
            flightdeck::maintenance::preview(&app, &json!({"operation":"reset"}))?;
            let wait = || -> Result<Value> {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
                while app.lock().active.is_some() {
                    require(
                        std::time::Instant::now() < deadline,
                        "Probe maintenance timed out.",
                    )?;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Ok(app.job("maintenance"))
            };
            let preview = wait()?;
            if input["operation"] == "maintenance-preview" || preview["state"] != "ready" {
                return Ok(preview);
            }
            require(
                input["confirmed"] == true,
                "Synthetic reset requires explicit confirmation.",
            )?;
            flightdeck::maintenance::start(
                &app,
                &json!({"job_id":preview["id"],"confirmed":true}),
            )?;
            wait()
        }
        "fenix" => fenix_fixture(&synthetic(&input)?, &path(&input, "bundle")?),
        "fenix-check" => {
            let root = synthetic(&input)?;
            fenix::verify_installed(&root, &runtime::value(&root.join(fenix::MARKER))?)?;
            Ok(json!(true))
        }
        "save-write" => {
            let state = save_state::State {
                generation: 7,
                containers: [(
                    "profile".into(),
                    save_state::Container {
                        display_name: "Pilot".into(),
                        modified: 123,
                        blobs: [("data".into(), b"\0\xffB".to_vec())].into(),
                    },
                )]
                .into(),
            };
            files::atomic(&path(&input, "file")?, &save_state::encode(&state)?)?;
            Ok(json!(true))
        }
        "save-reply" => {
            let raw = files::read(&path(&input, "file")?, 65536)?;
            let state = save_state::decode(&raw)?;
            Ok(json!(
                state.generation == 8
                    && state
                        .containers
                        .get("profile")
                        .is_some_and(|c| c.display_name == "Native reply"
                            && c.blobs
                                == [
                                    ("data".into(), b"\0\xffB".to_vec()),
                                    ("native_result".into(), b"PE\0\xff".to_vec())
                                ]
                                .into())
            ))
        }
        _ => Err(Error::Invalid("Unknown probe operation.")),
    }
}
fn main() -> std::process::ExitCode {
    rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));
    let result = (|| {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.first().is_some_and(|v| v == "--native-probe") {
            return match args.get(1).map(String::as_str) {
                Some("graphics") => native_probe::graphics(),
                Some("media") => Ok(native_probe::media()),
                Some("vr") => Ok(native_probe::vr()),
                Some("nvidia-directory") => native_probe::nvidia_directory(),
                _ => Err(Error::Invalid("Unknown native probe.")),
            };
        }
        let mut data = Vec::new();
        io::stdin()
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut data)?;
        require(data.len() <= 2 * 1024 * 1024, "Probe request too large.")?;
        run(serde_json::from_slice(&data)?)
    })();
    match result {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
