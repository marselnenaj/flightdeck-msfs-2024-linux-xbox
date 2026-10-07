// SPDX-License-Identifier: MIT
//! Native launcher, installer and runtime helpers.
use clap::{Parser, Subcommand};
use flightdeck::{Result, files, framework, games::Game, integrity, save_state};
use serde_json::json;
use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool};

#[derive(Parser)]
#[command(
    name = "flightdeck",
    version,
    about = "Flightdeck — MSFS Xbox PC on Linux"
)]
struct Arguments {
    #[arg(long)]
    runtime: Option<String>,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long)]
    no_browser: bool,
    #[arg(long)]
    desktop: bool,
    #[arg(long, hide = true)]
    desktop_service: bool,
    #[arg(long, global=true, value_parser=["de","en"])]
    language: Option<String>,
    #[arg(long)]
    refresh_components: bool,
    #[arg(long, hide=true, value_parser=["graphics","vr","nvidia-directory","hip"])]
    native_probe: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Start MSFS with automatic repair, backups and cloud synchronization.
    Play {
        #[arg(long)]
        runtime: PathBuf,
    },
    /// Install or update Flightdeck for the current user.
    Install(flightdeck::cli::Install),
    /// Configure the experimental Linux AMD neural-rendering bridge.
    NeuralRendering(flightdeck::neural_rendering::Options),
    /// Convert your local NR model to an experimental HIP cache (CPU only).
    NeuralWeights(flightdeck::neural_weights::Options),
    /// Export a private-data-free summary of an existing game log.
    DiagnoseRun {
        #[arg(long, conflicts_with = "runtime")]
        run: Option<PathBuf>,
        #[arg(long)]
        runtime: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    #[command(hide = true)]
    DesktopHandoff {
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long)]
        port: u16,
        #[arg(long, requires = "expected_release")]
        installation_root: Option<PathBuf>,
        #[arg(long, requires = "installation_root")]
        expected_release: Option<String>,
    },
    #[command(hide = true)]
    RunGame {
        #[arg(long)]
        runtime: PathBuf,
        #[arg(long)]
        lock_fd: Option<i32>,
    },
    #[command(hide = true)]
    DisplayRefresh {
        #[arg(long)]
        runtime: PathBuf,
    },
    #[command(hide = true)]
    WineLaunch {
        #[arg(long)]
        runtime: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        arguments: Vec<std::ffi::OsString>,
    },
    /// Inspect both .NET registry views and CLR files without starting Wine.
    FrameworkCheck {
        #[arg(long)]
        prefix: PathBuf,
    },
    /// Verify game files against their original completed download index.
    Verify {
        #[arg(long)]
        runtime: PathBuf,
    },
    /// Validate a native XDLOCAL1 save file, returning aggregate counts only.
    SaveCheck { file: PathBuf },
    /// Inspect the game edition and required runtime files without starting it.
    RuntimeCheck {
        #[arg(long)]
        runtime: PathBuf,
    },
}
fn run(command: Command) -> Result<(serde_json::Value, bool)> {
    match command {
        Command::NeuralRendering(options) => {
            Ok((flightdeck::neural_rendering::cli(options)?, true))
        }
        Command::NeuralWeights(options) => Ok((flightdeck::neural_weights::cli(options)?, true)),
        Command::Install(_)
        | Command::DiagnoseRun { .. }
        | Command::DesktopHandoff { .. }
        | Command::Play { .. } => Err(flightdeck::Error::Invalid(
            "This command requires its own process.",
        )),
        Command::RunGame { .. } => Err(flightdeck::Error::Invalid(
            "The game supervisor requires its own process.",
        )),
        Command::DisplayRefresh { .. } => Err(flightdeck::Error::Invalid(
            "The display helper requires its own process.",
        )),
        Command::WineLaunch { .. } => Err(flightdeck::Error::Invalid(
            "The runtime helper requires its own process.",
        )),
        Command::FrameworkCheck { prefix } => {
            let status = framework::status(&prefix);
            let ok = status.ready;
            Ok((serde_json::to_value(status)?, ok))
        }
        Command::Verify { runtime } => {
            let runtime = runtime.canonicalize()?;
            let _lease = files::Lease::acquire(&runtime.join("private/play.lock"), true)?;
            let game = Game::for_runtime(&runtime)?;
            let report = integrity::verify(&game.path(&runtime), &AtomicBool::new(false), |_| {})?;
            let ok = report.healthy;
            Ok((serde_json::to_value(report)?, ok))
        }
        Command::SaveCheck { file } => {
            let raw = files::read(&file, save_state::QUOTA + save_state::METADATA_LIMIT + 32)?;
            let state = save_state::decode(&raw)?;
            let blobs: usize = state.containers.values().map(|v| v.blobs.len()).sum();
            let bytes: usize = state
                .containers
                .values()
                .flat_map(|v| v.blobs.values())
                .map(Vec::len)
                .sum();
            Ok((
                json!({"valid":true,"containers":state.containers.len(),"blobs":blobs,"bytes":bytes,
                       "canonical_sha256":files::sha256(&save_state::encode(&state)?),"content_sha256":save_state::content_digest(&state)?}),
                true,
            ))
        }
        Command::RuntimeCheck { runtime } => {
            let runtime = runtime.canonicalize()?;
            let game = Game::for_runtime(&runtime)?;
            let checks = [
                ("launcher", runtime.join("tools/play-msfs.sh")),
                ("game", game.path(&runtime).join(game.executable())),
                ("prefix", runtime.join("local/msfs-prefix/system.reg")),
                (
                    "bridge",
                    runtime.join("local/msfs-prefix/drive_c/windows/system32/xgameruntime.dll"),
                ),
            ]
            .map(|(id, path)| json!({"id":id,"ok":path.is_file()}));
            let ok = checks.iter().all(|v| v["ok"] == true);
            Ok((
                json!({"game_id":game.id(),"game_name":game.name(),"checks":checks,"files_present":ok,
                       "integrity_available":integrity::available(&game.path(&runtime)),"scope":"file_inventory_only"}),
                ok,
            ))
        }
    }
}
fn main() -> ExitCode {
    // Establish private defaults once, before threads or child processes exist.
    rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));
    let raw: Vec<_> = std::env::args_os().skip(1).collect();
    if raw.first().is_some_and(|s| s == "--managed-root") {
        let result = raw
            .get(1)
            .ok_or(flightdeck::Error::Invalid("Missing installation root."))
            .and_then(|root| flightdeck::installer::managed(std::path::Path::new(root), &raw[2..]));
        return finish(result, None);
    }
    if std::env::args_os().next().is_some_and(|v| {
        std::path::Path::new(&v)
            .file_name()
            .is_some_and(|v| v == "xodus-wine-launch")
    }) {
        let result = (|| {
            let root = std::env::var_os("FLIGHTDECK_HELPER_RUNTIME").ok_or(
                flightdeck::Error::Invalid("Missing Flightdeck helper runtime."),
            )?;
            flightdeck::loader::run(
                std::path::Path::new(&root),
                &std::env::args_os().skip(1).collect::<Vec<_>>(),
            )
        })();
        return match result {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("xodus-wine-launch: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let arguments = Arguments::parse();
    let language = flightdeck::cli::locale(arguments.language.as_deref());
    let state_dir = arguments
        .state_dir
        .clone()
        .unwrap_or_else(|| files::xdg("XDG_STATE_HOME", ".local/state").join("flightdeck"));
    match &arguments.command {
        Some(Command::Play { runtime }) => {
            return match flightdeck::cli::play(runtime, &language) {
                Ok(code) => ExitCode::from(code),
                Err(error) => finish(Err(error), Some(&language)),
            };
        }
        Some(Command::Install(options)) => {
            let result = flightdeck::cli::install(options, &language);
            if options.gui
                && let Err(error) = &result
            {
                flightdeck::cli::error_dialog(error, &language);
            }
            return finish(result, Some(&language));
        }
        Some(Command::DiagnoseRun {
            run,
            runtime,
            output,
        }) => {
            return finish(
                flightdeck::cli::diagnose(run.as_deref(), runtime.as_deref(), &state_dir)
                    .and_then(|value| flightdeck::cli::output(&value, output.as_deref())),
                Some(&language),
            );
        }
        Some(Command::DesktopHandoff {
            state_dir,
            port,
            installation_root,
            expected_release,
        }) => {
            return finish(
                match (installation_root, expected_release) {
                    (Some(root), Some(expected)) => {
                        flightdeck::desktop::installed_handoff(state_dir, *port, root, expected)
                    }
                    _ => flightdeck::desktop::handoff(state_dir, *port),
                },
                Some(&language),
            );
        }
        _ => {}
    }
    if let Some(Command::RunGame { runtime, lock_fd }) = &arguments.command {
        return match flightdeck::supervisor::run(runtime, *lock_fd) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("Flightdeck game supervisor: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if let Some(Command::DisplayRefresh { runtime }) = &arguments.command {
        return match flightdeck::display_refresh::run(runtime) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Flightdeck display refresh: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if let Some(Command::WineLaunch { runtime, arguments }) = &arguments.command {
        return match flightdeck::loader::run(runtime, arguments) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("xodus-wine-launch: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if let Some(probe) = arguments.native_probe.as_deref() {
        let result = match probe {
            "graphics" => flightdeck::native_probe::graphics(),
            "vr" => Ok(flightdeck::native_probe::vr()),
            "hip" => std::env::var_os("FLIGHTDECK_HIP_LIBRARY")
                .ok_or(flightdeck::Error::Invalid("Missing HIP library path."))
                .and_then(|p| flightdeck::hip_probe::probe(&PathBuf::from(p))),
            _ => flightdeck::native_probe::nvidia_directory(),
        };
        return match result {
            Ok(value) => {
                println!("{value}");
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::FAILURE,
        };
    }
    if arguments.command.is_none() {
        let result = (|| -> Result<()> {
            let state_dir = arguments
                .state_dir
                .clone()
                .unwrap_or_else(|| files::xdg("XDG_STATE_HOME", ".local/state").join("flightdeck"));
            if !arguments.no_browser && !arguments.desktop_service && !arguments.refresh_components
            {
                return flightdeck::desktop::start(
                    &state_dir,
                    arguments.runtime.as_deref(),
                    arguments.port,
                    arguments.language.as_deref(),
                );
            }
            let state_dir = flightdeck::desktop::state_directory(&state_dir)?;
            let _lease = if arguments.desktop_service {
                Some(files::Lease::acquire(
                    &state_dir.join("desktop-service.lock"),
                    true,
                )?)
            } else {
                None
            };
            let launcher =
                flightdeck::backend::Launcher::new(state_dir, arguments.runtime.as_deref())?;
            if arguments.refresh_components {
                let changed = flightdeck::components::refresh(&launcher)?;
                println!(
                    "{}",
                    flightdeck::i18n::text(
                        if changed {
                            "Runtime-Komponenten aktualisiert."
                        } else {
                            "Runtime-Komponenten bereits aktuell."
                        },
                        &language
                    )
                );
                return Ok(());
            }
            if matches!(
                flightdeck::components::state(launcher.lock().runtime.as_deref()),
                "pending" | "interrupted" | "current"
            ) && let Err(error) = flightdeck::components::refresh(&launcher)
            {
                eprintln!(
                    "Flightdeck: {}",
                    flightdeck::i18n::text(&error.to_string(), &language)
                );
            }
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?
                .block_on(flightdeck::server::serve(
                    launcher,
                    arguments.port,
                    arguments.no_browser || arguments.desktop_service,
                    arguments.language.as_deref(),
                    arguments.desktop_service,
                ))
        })();
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Flightdeck: {error}");
                ExitCode::FAILURE
            }
        };
    }
    match run(arguments.command.expect("diagnostic command selected")) {
        Ok((value, ok)) => {
            match serde_json::to_string_pretty(&value) {
                Ok(output) => println!("{output}"),
                Err(_) => return ExitCode::FAILURE,
            }
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("Flightdeck: {error}");
            ExitCode::FAILURE
        }
    }
}
fn finish(result: Result<()>, language: Option<&str>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "Flightdeck: {}",
                flightdeck::i18n::text(&error.to_string(), language.unwrap_or("en"))
            );
            ExitCode::FAILURE
        }
    }
}
