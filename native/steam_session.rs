// SPDX-License-Identifier: MIT
//! Steam owns the launch. Observe the simulator, not the client request.
use crate::{
    Error, Result,
    backend::Launcher,
    error::require,
    files,
    installations::{Client, Installation, Source},
    process, wine_processes,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::MetadataExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
fn native_client(root: &Path) -> Result<Command> {
    // A second native client directory must not silently send its AppID to
    // an already running Steam instance from a different directory.
    let executables = [
        root.join("ubuntu12_32/steam"),
        root.join("ubuntu12_64/steam"),
    ];
    for (count, entry) in std::fs::read_dir("/proc")?.enumerate() {
        require(count < 131072, "Zu viele Prozesse für die Steam-Prüfung.")?;
        let proc = entry?.path();
        if !proc.metadata().is_ok_and(|m| m.uid() == files::uid())
            || !std::fs::read(proc.join("comm")).is_ok_and(|v| v == b"steam\n")
            || proc.join("root/.flatpak-info").is_file()
        {
            continue;
        }
        let executable = std::fs::metadata(proc.join("exe")).ok();
        let matches = executables.iter().any(|p| {
            p.metadata()
                .ok()
                .zip(executable.as_ref())
                .is_some_and(|(a, b)| [a.dev(), a.ino()] == [b.dev(), b.ino()])
        });
        require(
            matches,
            "Ein anderer nativer Steam-Client läuft. Beende ihn vollständig, bevor du diese Steam-Installation startest.",
        )?;
    }
    let script = root.join("steam.sh");
    if script
        .metadata()
        .is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
    {
        let mut command = Command::new(script);
        command.current_dir(root);
        return Ok(command);
    }
    let home = files::home();
    require(
        [".local/share/Steam", ".steam/root", ".steam/steam"]
            .iter()
            .any(|p| home.join(p).canonicalize().is_ok_and(|p| p == root)),
        "In diesem Steam-Ordner fehlt steam.sh. Bitte den Steam-Clientordner auswählen.",
    )?;
    Ok(Command::new("steam"))
}
pub struct Session {
    pub installation: String,
    pub requested: Instant,
    pub seen: bool,
    pub client: Option<Child>,
    pub stopping: bool,
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.client.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
pub fn running(item: &Installation) -> Result<bool> {
    let prefix = item.prefix();
    if !prefix.exists() {
        return Ok(false);
    }
    let name = item.game.executable().to_lowercase();
    Ok(wine_processes::Processes::new(&prefix)?.any(Some(&[&name])))
}
pub fn command(item: &Installation, open_only: bool) -> Result<Command> {
    let Source::Steam {
        client, ref root, ..
    } = item.source
    else {
        return Err(Error::Invalid("Keine Steam-Installation ausgewählt."));
    };
    let mut c = match client {
        Client::Native => native_client(root)?,
        Client::Flatpak => {
            let mut c = Command::new("flatpak");
            c.args(["run", "com.valvesoftware.Steam"]);
            c
        }
        Client::Snap => {
            let mut c = Command::new("snap");
            c.args(["run", "steam"]);
            c
        }
    };
    if open_only {
        c.arg(format!("steam://nav/games/details/{}", item.appid()));
    } else {
        c.args(["-applaunch", item.appid()]);
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(c)
}
pub fn launch(app: &Arc<Launcher>) -> Result<Value> {
    let mut s = app.lock();
    Launcher::idle(&mut s)?;
    let item = s
        .steam
        .clone()
        .ok_or(Error::Invalid("Keine Steam-Installation ausgewählt."))?;
    item.validate()?;
    require(!running(&item)?, "Dieser Simulator läuft bereits in Steam.")?;
    let child = process::spawn(&mut command(&item, false)?, None)?;
    s.steam_notice.clear();
    s.started = Some(Instant::now());
    s.started_at = Some(files::now());
    s.exit_code = None;
    s.steam_session = Some(Session {
        installation: item.id,
        requested: Instant::now(),
        seen: false,
        client: Some(child),
        stopping: false,
    });
    Ok(json!({"ok":true,"state":"starting","via":"steam"}))
}
pub fn open(app: &Arc<Launcher>) -> Result<Value> {
    let s = app.lock();
    Launcher::open(&s)?;
    let item = s
        .steam
        .as_ref()
        .ok_or(Error::Invalid("Keine Steam-Installation ausgewählt."))?;
    item.validate()?;
    let mut child = process::spawn(&mut command(item, true)?, None)?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(json!({"ok":true}))
}
pub fn status(s: &mut crate::backend::State, state_dir: &std::path::Path) -> Value {
    let Some(item) = s.steam.clone() else {
        return Value::Null;
    };
    if s.steam_helpers.as_ref().is_some_and(|h| h.finished()) {
        s.steam_helpers = None;
    }
    let mut state = "stopped";
    let mut message = "";
    match running(&item) {
        Ok(true) => {
            if s.steam_helpers.is_none() {
                s.steam_helpers = crate::steam_companions::Session::start(state_dir, &item).ok();
            }
            s.steam_notice.clear();
            if let Some(session) = s.steam_session.as_mut() {
                session.seen = true;
                state = if session.stopping {
                    "stopping"
                } else {
                    "running"
                };
            } else {
                state = "external";
            }
        }
        Err(_) => {
            state = "unknown";
            message = "Der Steam-Spielprozess konnte nicht eindeutig geprüft werden.";
        }
        Ok(false) => {
            // The worker observes the same exit before cleanup; dropping it
            // here would cancel that final companion cleanup.
            let mut finished = false;
            if let Some(session) = s.steam_session.as_mut() {
                if let Some(child) = session.client.as_mut()
                    && let Ok(Some(code)) = child.try_wait()
                {
                    session.client = None;
                    if !code.success() {
                        finished = true;
                        s.exit_code = code.code();
                        message = "Steam konnte den Start nicht ausführen.";
                    }
                }
                if session.seen {
                    finished = true;
                } else if session.requested.elapsed() < Duration::from_secs(180) && !finished {
                    state = "starting";
                } else if !finished {
                    finished = true;
                    message = "Steam hat den Spielstart noch nicht bestätigt. Prüfe den Simulator in Steam.";
                }
            }
            if finished {
                s.steam_session = None;
                s.started = None;
            }
            if s.steam_helpers.as_ref().is_some_and(|h| !h.finished()) {
                state = "stopping";
            }
        }
    }
    if !message.is_empty() {
        s.steam_notice = message.to_string();
    }
    let message = s.steam_notice.clone();
    let idle = state == "stopped" && s.active.is_none() && !s.closing;
    json!({"state":state,"managed":s.steam_session.is_some(),"can_start":idle&&item.validate().is_ok(),"can_stop":(["running","external"].contains(&state)),"started_at":s.started_at,"exit_code":s.exit_code,"message":message,"via":"steam"})
}
pub fn stop(app: &Arc<Launcher>) -> Result<Value> {
    let mut s = app.lock();
    Launcher::open(&s)?;
    require(
        s.active.is_none(),
        "Beende zuerst den laufenden Einrichtungsvorgang.",
    )?;
    let item = s
        .steam
        .clone()
        .ok_or(Error::Invalid("Keine Steam-Installation ausgewählt."))?;
    require(running(&item)?, "Dieser Simulator läuft nicht.")?;
    if let Some(session) = s.steam_session.as_mut() {
        session.stopping = true;
    }
    drop(s);
    wine_processes::terminate_named(
        &item.prefix(),
        &[&item.game.executable().to_lowercase()],
        Duration::from_secs(10),
    )?;
    Ok(json!({"ok":true}))
}
