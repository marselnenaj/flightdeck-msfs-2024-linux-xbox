// SPDX-License-Identifier: MIT
//! Native installation and read-only support commands.
use crate::{Error, Result, error::require, files, installer, process};
use clap::Args;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::Duration,
};
#[derive(Args)]
pub struct Install {
    #[arg(long)]
    pub source: Option<PathBuf>,
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    #[arg(long)]
    pub bin_dir: Option<PathBuf>,
    #[arg(long)]
    pub applications_dir: Option<PathBuf>,
    #[arg(long)]
    pub no_desktop: bool,
    #[arg(long)]
    pub no_launch: bool,
    #[arg(long)]
    pub gui: bool,
    #[arg(long, conflicts_with = "rollback")]
    pub uninstall: bool,
    #[arg(long, conflicts_with = "uninstall")]
    pub rollback: bool,
    #[arg(long, hide = true)]
    pub expected_current: Option<String>,
}
pub fn locale(selected: Option<&str>) -> String {
    selected.map(str::to_owned).unwrap_or_else(|| {
        let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        if raw.to_lowercase().starts_with("de") {
            "de".into()
        } else {
            "en".into()
        }
    })
}
fn confirm(message: &str, language: &str) -> Result<bool> {
    let mut command = if let Some(tool) = process::which("zenity") {
        let mut cmd = Command::new(tool);
        cmd.args([
            "--question",
            "--title=Flightdeck",
            "--no-markup",
            "--width=520",
            "--text",
            message,
        ]);
        cmd
    } else if let Some(tool) = process::which("kdialog") {
        let mut cmd = Command::new(tool);
        cmd.args(["--title", "Flightdeck", "--yesno", message]);
        cmd
    } else {
        return Err(Error::Invalid(if language == "de" {
            "Für den grafischen Installer bitte Zenity oder KDialog installieren, oder den Installer ohne --gui starten."
        } else {
            "Install Zenity or KDialog for the graphical installer, or run the installer without --gui."
        }));
    };
    let status = process::run(
        command.stdin(Stdio::null()).stdout(Stdio::null()),
        Duration::from_secs(3600),
        &AtomicBool::new(false),
    )?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Error::Invalid(
            "Der Installationsdialog konnte nicht geöffnet werden.",
        )),
    }
}
pub fn install(options: &Install, language: &str) -> Result<()> {
    let data = files::xdg("XDG_DATA_HOME", ".local/share");
    let root = installer::absolute(
        options
            .data_dir
            .as_deref()
            .unwrap_or(&data.join("flightdeck-launcher")),
    )?;
    if options.uninstall || options.rollback {
        let state = installer::load(&root)?.ok_or(Error::Invalid(
            "Hier ist kein Flightdeck-Launcher installiert.",
        ))?;
        if options.gui {
            let action = if options.uninstall {
                if language == "de" {
                    "entfernen"
                } else {
                    "uninstall"
                }
            } else if language == "de" {
                "zurücksetzen"
            } else {
                "roll back"
            };
            if !confirm(
                &format!("Flightdeck: {action}\n\n{}", root.display()),
                language,
            )? {
                return Ok(());
            }
        }
        if options.uninstall {
            let retained = installer::uninstall(&root)?;
            println!(
                "{}",
                if language == "de" {
                    "Launcher entfernt. Einstellungen, Runtimes und Spielstände bleiben erhalten."
                } else {
                    "Launcher removed. Settings, runtimes and saves are preserved."
                }
            );
            for path in retained {
                println!(
                    "{}: {}",
                    if language == "de" {
                        "Behalten"
                    } else {
                        "Retained"
                    },
                    path.display()
                );
            }
        } else {
            installer::rollback(
                &root,
                language,
                Some(crate::backend::string(&state, "current")?),
            )?;
            println!(
                "{}",
                if language == "de" {
                    "Vorherige Version bereit. Öffne Flightdeck jetzt neu."
                } else {
                    "Previous version ready. Reopen Flightdeck now."
                }
            );
        }
        return Ok(());
    }
    let bin = installer::absolute(
        options
            .bin_dir
            .as_deref()
            .unwrap_or(&files::home().join(".local/bin")),
    )?;
    let applications = installer::absolute(
        options
            .applications_dir
            .as_deref()
            .unwrap_or(&data.join("applications")),
    )?;
    let source = match &options.source {
        Some(p) => installer::absolute(p)?,
        None => std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .ok_or(Error::Invalid("Das Flightdeck-Paket fehlt."))?
            .to_path_buf(),
    };
    // Validate the concrete payload before presenting the installation dialog.
    installer::snapshot(&source)?;
    if options.gui {
        let message = if language == "de" {
            format!(
                "Flightdeck {} installieren?\n\nProgramm: {}\nStartbefehl: {}\n\nMSFS wird anschließend separat eingerichtet.",
                crate::VERSION,
                root.display(),
                bin.join("flightdeck").display()
            )
        } else {
            format!(
                "Install Flightdeck {}?\n\nApplication: {}\nLauncher: {}\n\nSet up MSFS separately after installation.",
                crate::VERSION,
                root.display(),
                bin.join("flightdeck").display()
            )
        };
        if !confirm(&message, language)? {
            return Ok(());
        }
    }
    let state = installer::install(installer::Options {
        source: &source,
        root: &root,
        bin_dir: &bin,
        applications_dir: &applications,
        desktop: !options.no_desktop,
        language,
        expected_current: options.expected_current.as_deref(),
    })?;
    println!(
        "{}: {}",
        if language == "de" {
            "Flightdeck installiert"
        } else {
            "Flightdeck installed"
        },
        crate::backend::string(&state["entries"]["launcher"], "path")?
    );
    if !options.no_launch {
        let mut child = process::spawn(
            Command::new(crate::backend::string(
                &state["entries"]["launcher"],
                "path",
            )?)
            .arg("--desktop")
            .args(["--language", language])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
            None,
        )?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    Ok(())
}
pub fn error_dialog(error: &Error, language: &str) {
    let message = crate::i18n::text(&error.to_string(), language);
    let command = if let Some(path) = process::which("zenity") {
        let mut c = Command::new(path);
        c.args([
            "--error",
            "--no-markup",
            "--title=Flightdeck",
            "--text",
            &message,
        ]);
        Some(c)
    } else if let Some(path) = process::which("kdialog") {
        let mut c = Command::new(path);
        c.args(["--title", "Flightdeck", "--error", &message]);
        Some(c)
    } else {
        None
    };
    if let Some(mut command) = command {
        let _ = process::run(
            command.stdin(Stdio::null()).stdout(Stdio::null()),
            Duration::from_secs(3600),
            &AtomicBool::new(false),
        );
    }
}
pub fn diagnose(run: Option<&Path>, runtime: Option<&Path>, state_dir: &Path) -> Result<Value> {
    let run = if let Some(run) = run {
        installer::absolute(run)?
    } else {
        let root = if let Some(root) = runtime {
            installer::absolute(root)?
        } else {
            let config: Value = files::json(&state_dir.join("config.json"), 65536)?;
            installer::absolute(Path::new(crate::backend::string(&config, "runtime_path")?))?
        };
        files::directory(&root.join("private"), false)?;
        let mut entries = fs::read_dir(root.join("private"))?;
        let mut latest = None;
        for entry in entries.by_ref().take(10000) {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && crate::log_reader::regex(r"^run-\d{8}-\d{6}-[A-Za-z0-9]+$")
                    .is_match(&entry.file_name().to_string_lossy())
                && latest
                    .as_ref()
                    .is_none_or(|old: &PathBuf| entry.path() > *old)
            {
                latest = Some(entry.path());
            }
        }
        require(
            entries.next().is_none(),
            "Der Protokollordner enthält zu viele Einträge.",
        )?;
        latest.ok_or(Error::Invalid(
            "Kein Spielprotokoll gefunden. Bitte --run oder --runtime angeben.",
        ))?
    };
    files::directory(&run, false)?;
    let (mut summary, info) = crate::run_diagnostics::read(&run.join("game.log"))?;
    let graphics = summary
        .as_object_mut()
        .ok_or(Error::Invalid("Ungültiger Diagnosebericht."))?
        .remove("graphics_log")
        .unwrap_or(Value::Null);
    summary["graphics"] = json!({"log":graphics});
    summary["store_session"] = crate::store_diagnostics::session(&run);
    let modified: chrono::DateTime<chrono::Utc> = info.modified()?.into();
    summary["context"] = json!({"diagnostics_schema":5,"tool":"flightdeck-run-log-diagnostics-v1","run_log_modified_at":modified.to_rfc3339(),"cloud_sync_scope":"not_collected"});
    Ok(json!({"summary":summary,"generated_at":files::now()}))
}
pub fn output(value: &Value, path: Option<&Path>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if let Some(path) = path {
        let path = installer::absolute(path)?;
        installer::no_links(&path)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        files::directory(
            path.parent()
                .ok_or(Error::Invalid("Ungültiger Ausgabepfad."))?,
            false,
        )?
        .sync_all()?;
    } else {
        std::io::stdout().lock().write_all(&bytes)?;
    }
    Ok(())
}
pub fn play(root: &Path, language: &str) -> Result<u8> {
    let root = root.canonicalize()?;
    let app = crate::backend::Launcher::new(
        root.join("private/launcher-state"),
        Some(
            root.to_str()
                .ok_or(Error::Invalid("Ungültiger Runtime-Pfad."))?,
        ),
    )?;
    if matches!(
        crate::components::state(Some(&root)),
        "pending" | "interrupted" | "current"
    ) {
        crate::components::refresh(&app)?;
    }
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?.block_on(async {
        let mut interrupt=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut terminate=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        crate::cloud_auto::launch(&app)?;let mut previous=String::new();
        loop {
            {let s=app.lock();let message=s.cloud["message"].as_str().unwrap_or("");if message!=previous&&!message.is_empty(){println!("{}",crate::i18n::text(message,language));previous=message.into();}
                if s.active.is_none(){require(s.cloud["state"]!="attention","Der Start oder Cloud-Abgleich benötigt eine Entscheidung. Bitte Flightdeck öffnen.")?;return Ok(s.exit_code.unwrap_or(0).clamp(0,255) as u8);}}
            let stop=tokio::select!{_=interrupt.recv()=>true,_=terminate.recv()=>true,_=tokio::time::sleep(Duration::from_millis(100))=>false};
            if stop {let _=app.stop();app.close();}
        }
    })
}
