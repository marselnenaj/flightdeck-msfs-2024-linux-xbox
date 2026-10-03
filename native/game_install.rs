// SPDX-License-Identifier: MIT
//! Licensed Xodus login/download with a private progress pipe and owned cancellation.
use crate::{
    Error, Result, backend::Context, error::require, files, game_package, games::Game, process,
    progress, setup, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::Read,
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
pub fn verify_cli(cli: &Path, expected: &str) -> Result<PathBuf> {
    require(
        cli.is_absolute()
            && crate::runtime::executable(cli)
            && files::hex_digest(expected)
            && tx::digest(cli)? == expected,
        "Die Prüfsumme des Xodus-Installers stimmt nicht. Bitte die Laufzeitkomponenten erneut vorbereiten.",
    )?;
    Ok(cli.into())
}
pub fn environment(command: &mut Command, xdg: Option<&Path>) {
    command.env("XODUS_LOG", "off").env("RUST_BACKTRACE", "0");
    if let Some(xdg) = xdg {
        for category in ["config", "data", "cache", "state"] {
            command.env(
                format!("XDG_{}_HOME", category.to_uppercase()),
                xdg.join(category),
            );
        }
    }
}
pub fn login_result(code: i32) -> Result<()> {
    require(
        code == 0,
        match code {
            70 => {
                "Das Microsoft-Anmeldefenster oder der Anmeldeablauf ist fehlgeschlagen. Flightdeck aus der grafischen Sitzung starten und erneut versuchen."
            }
            71 => {
                "Die Microsoft-Anmeldung konnte nicht vorbereitet werden. Schlüsselbund und Netzwerk prüfen und erneut versuchen."
            }
            72 => {
                "Die Microsoft-Anmeldedaten konnten nicht gespeichert werden. Den Linux-Schlüsselbund prüfen und erneut versuchen."
            }
            73 => {
                "Die Verbindung zu Microsoft ist während der Anmeldung fehlgeschlagen (Anmeldecode 73). Bitte die Verbindung prüfen und erneut versuchen."
            }
            74 => {
                "Die Microsoft-Anmeldeantwort konnte nicht verarbeitet werden (Anmeldecode 74). Bitte diesen Code beim Fehlerbericht angeben."
            }
            75 => {
                "Microsoft hat die Anmeldung ohne einen unterstützten Verifizierungsschritt abgelehnt (Anmeldecode 75). Bitte diesen Code beim Fehlerbericht angeben."
            }
            76 => "Bitte mit demselben Microsoft-Konto wie zuvor anmelden (Anmeldecode 76).",
            79 => {
                "Microsoft hat keinen unterstützten Verifizierungsschritt zurückgegeben (Anmeldecode 79). Bitte diesen Code beim Fehlerbericht angeben."
            }
            80 => "Die Microsoft-Tokenanfrage konnte nicht erstellt werden (Anmeldecode 80).",
            81 => "Die Microsoft-Anmeldeantwort überschreitet das Größenlimit (Anmeldecode 81).",
            82 => "Die Textkodierung der Microsoft-Anmeldeantwort ist ungültig (Anmeldecode 82).",
            83 => {
                "Die XML-Struktur der Microsoft-Anmeldeantwort konnte nicht gelesen werden (Anmeldecode 83)."
            }
            84 => {
                "Die Signatur der Microsoft-Anmeldeantwort konnte nicht geprüft werden (Anmeldecode 84)."
            }
            85 => {
                "Der Microsoft-Verifizierungsschritt konnte nicht entschlüsselt werden (Anmeldecode 85)."
            }
            86 => {
                "Der entschlüsselte Microsoft-Verifizierungsschritt konnte nicht gelesen werden (Anmeldecode 86)."
            }
            87 => "Die Microsoft-Tokenantwort konnte nicht entschlüsselt werden (Anmeldecode 87).",
            88 => {
                "Die entschlüsselte Microsoft-Tokenantwort konnte nicht gelesen werden (Anmeldecode 88)."
            }
            89 => "Die Microsoft-Tokenantwort ist unvollständig (Anmeldecode 89).",
            90 => {
                "Der Microsoft-Anmeldedienst hat einen HTTP-Fehler zurückgegeben (Anmeldecode 90)."
            }
            91 => "Microsoft hat kein gültiges Haupt-Anmeldetoken zurückgegeben (Anmeldecode 91).",
            92 => "Die Microsoft-Tokenanfrage hat das Zeitlimit überschritten (Anmeldecode 92).",
            93 => {
                "Microsoft hat zu viele Anmeldeversuche gemeldet. Bitte vor dem nächsten Versuch warten (Anmeldecode 93)."
            }
            101 => {
                "Die Microsoft-Anmeldung wurde unerwartet beendet. Grafische Sitzung, GTK/WebKitGTK und Schlüsselbund prüfen und erneut versuchen."
            }
            _ => "Die Microsoft-Anmeldung wurde nicht abgeschlossen. Bitte erneut versuchen.",
        },
    )
}
pub enum Exit {
    Code(i32),
    Paused,
}
#[derive(Default)]
pub struct Options {
    pub timeout: Option<Duration>,
    pub transfers: bool,
    pub pausable: bool,
}
pub fn run_cli(
    cli: &Path,
    args: &[String],
    cwd: &Path,
    xdg: Option<&Path>,
    ctx: &Context,
    options: Options,
) -> Result<Exit> {
    ctx.interrupted()?;
    let mut command = Command::new(cli);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    environment(&mut command, xdg);
    let mut pipe = if options.transfers {
        let (read, write) = rustix::pipe::pipe_with(
            rustix::pipe::PipeFlags::CLOEXEC | rustix::pipe::PipeFlags::NONBLOCK,
        )?;
        Some((File::from(read), File::from(write)))
    } else {
        None
    };
    if let Some((_, write)) = &pipe {
        command.args(["--progress-fd", &write.as_raw_fd().to_string()]);
    }
    let mut child = process::spawn(&mut command, pipe.as_ref().map(|v| &v.1))?;
    let mut reader = pipe.take().map(|(read, write)| {
        drop(write);
        read
    });
    let mut decoder = progress::Decoder::default();
    let started = Instant::now();
    let result = (|| {
        let mut exited = None;
        loop {
            ctx.interrupted()?;
            if let Some(reader) = reader.as_mut() {
                let mut buffer = [0_u8; 4096];
                for _ in 0..16 {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => decoder.consume(&buffer[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e.into()),
                    }
                }
                if let Some(value) = decoder.take() {
                    ctx.update(json!({"transfer":value}));
                }
            }
            if let Some(code) = exited {
                return Ok(Exit::Code(code));
            }
            exited = child.try_wait()?.map(|code| code.code().unwrap_or(-1));
            if exited.is_some() {
                continue;
            } // Drain the last frame written before exit.
            if options.pausable && ctx.pause.load(Ordering::Relaxed) {
                process::terminate_group(&mut child)?;
                return Ok(Exit::Paused);
            }
            require(
                options
                    .timeout
                    .is_none_or(|limit| started.elapsed() < limit),
                "Die Microsoft-Anmeldung hat zu lange gedauert. Bitte erneut versuchen.",
            )?;
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    if result.is_err() {
        let _ = process::terminate_group(&mut child);
    }
    result
}
pub struct Request<'a> {
    pub cli: &'a Path,
    pub expected: &'a str,
    pub destination: &'a Path,
    pub market: &'a str,
    pub xdg: &'a Path,
    pub features: &'a Value,
    pub sign_in: bool,
    pub expected_package: Option<&'a str>,
    pub game: Game,
}
pub fn download(request: Request<'_>, ctx: &Context) -> Result<PathBuf> {
    setup::market(request.market)?;
    verify_cli(request.cli, request.expected)?;
    require(
        request.destination.is_absolute()
            && request.destination.parent().is_some_and(Path::is_dir)
            && !files::exists(request.destination),
        "Der private Downloadordner ist noch nicht vorbereitet oder existiert bereits.",
    )?;
    files::private_dir(request.destination)?;
    files::private_dir(request.xdg)?;
    for category in ["config", "data", "cache", "state"] {
        files::private_dir(&request.xdg.join(category))?;
    }
    let cwd = request
        .destination
        .parent()
        .ok_or(Error::Invalid("Ungültiger Downloadpfad."))?;
    let login = || -> Result<()> {
        ctx.update(
            json!({"phase":"authentication","transfer":null,"can_pause":false,"can_resume":false}),
        );
        ctx.progress("Bitte im Microsoft-Fenster mit dem Konto anmelden, das MSFS besitzt.");
        verify_cli(request.cli, request.expected)?;
        match run_cli(
            request.cli,
            &["login".into()],
            cwd,
            Some(request.xdg),
            ctx,
            Options {
                timeout: Some(Duration::from_secs(900)),
                ..Options::default()
            },
        )? {
            Exit::Code(code) => login_result(code),
            Exit::Paused => Err(Error::Invalid("Die Anmeldung kann nicht pausiert werden.")),
        }
    };
    if request.sign_in {
        login()?;
    }
    let feature = |name: &str| {
        request
            .features
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == name))
    };
    let resume = feature("streaming-resume-files-v1");
    let transfers = resume && feature("streaming-progress-v1");
    let mut args = vec![
        "streaming".into(),
        request.game.store_id().into(),
        request.destination.to_string_lossy().into_owned(),
        "--market".into(),
        request.market.into(),
        "--parallel".into(),
        "4".into(),
    ];
    if resume {
        args.push("--resume-files".into());
    }
    if let Some(package) = request.expected_package {
        require(
            resume && feature("package-info-json-v1") && files::hex_digest(package),
            "Diese Flightdeck-Komponenten unterstützen noch keine sicheren Spielupdates. Bitte Flightdeck aktualisieren.",
        )?;
        args.extend(["--expect-package".into(), package.into()]);
    }
    let mut authenticated = false;
    let result = (|| {
        loop {
            ctx.interrupted()?;
            verify_cli(request.cli, request.expected)?;
            ctx.update(json!({"phase":"download","can_pause":resume,"can_resume":false}));
            ctx.progress("MSFS wird über Xodus angefordert, die Lizenz geprüft und das Spiel heruntergeladen. Das kann längere Zeit dauern …");
            match run_cli(
                request.cli,
                &args,
                cwd,
                Some(request.xdg),
                ctx,
                Options {
                    timeout: None,
                    transfers,
                    pausable: resume,
                },
            )? {
                Exit::Paused => {
                    ctx.update(json!({"phase":"paused","can_pause":false,"can_resume":true,"message":"Download pausiert. Flightdeck geöffnet lassen. Vollständige Dateien bleiben erhalten; die unvollständige Datei beginnt beim Fortsetzen erneut."}));
                    while ctx.pause.load(Ordering::Relaxed) {
                        ctx.interrupted()?;
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    authenticated = false;
                }
                Exit::Code(77) if resume && !authenticated => {
                    login()?;
                    authenticated = true;
                }
                Exit::Code(78) if request.expected_package.is_some() => {
                    return Err(Error::Invalid(
                        "Im Store ist inzwischen eine andere Paketrevision verfügbar. Bitte das Update erneut prüfen; die bisherige Installation bleibt erhalten.",
                    ));
                }
                Exit::Code(code) => {
                    require(
                        code == 0,
                        "Der Spieldownload wurde nicht abgeschlossen. Bitte Kaufberechtigung, Speicherplatz und Verbindung prüfen.",
                    )?;
                    break;
                }
            }
        }
        ctx.interrupted()?;
        game_package::validate_download(request.destination, request.game)?;
        if feature("streaming-integrity-index-v1") {
            crate::integrity::record_installation(request.destination, request.game)?;
        }
        Ok(request.destination.to_path_buf())
    })();
    ctx.pause.store(false, Ordering::Relaxed);
    ctx.update(json!({"can_pause":false,"can_resume":false,"transfer":null}));
    result
}
