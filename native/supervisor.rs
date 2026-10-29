// SPDX-License-Identifier: MIT
//! Own one game/service session and its lease, including bounded companion cleanup.
use crate::{
    Error, Result, backend::Context, error::require, fenix, files, game_update, games::Game,
    graphics, graphics_diagnostics, gsx, inherited_fd, process, proton, runtime, transaction as tx,
    vr, wine_processes,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
pub type Environment = BTreeMap<String, String>;
pub fn environment(root: &Path) -> Result<(Environment, PathBuf)> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or(Error::Invalid("Start from your graphical Linux session."))?;
    files::directory(&runtime_dir, true)?;
    let hash = files::sha256(root.as_os_str().as_encoded_bytes());
    let socket_name = format!("flightdeck-{}", &hash[..16]);
    let socket_dir = runtime_dir.join(&socket_name);
    files::private_dir(&socket_dir)?;
    files::private_dir(&root.join("private"))?;
    let mut env: BTreeMap<_, _> = std::env::vars().collect();
    env.insert(
        "MSFS_LINUX_ROOT".into(),
        root.to_string_lossy().into_owned(),
    );
    env.insert(
        "FLIGHTDECK_SOCKET_DIR".into(),
        socket_dir.to_string_lossy().into_owned(),
    );
    env.insert(
        "XODUS_USER_SOCKET_SUFFIX".into(),
        format!("{socket_name}/xodus.sock"),
    );
    for category in ["CONFIG", "DATA", "CACHE", "STATE"] {
        let path = root.join("private/xdg").join(category.to_lowercase());
        files::private_dir(&path)?;
        env.insert(
            format!("XDG_{category}_HOME"),
            path.to_string_lossy().into_owned(),
        );
    }
    env.insert("RUST_LOG".into(), "warn".into());
    env.insert("XODUS_LOG".into(), "warn".into());
    // The Xodus service opens the in-game Store purchase window from this session.
    if let Some((key, value)) = crate::game_install::webkit() {
        env.insert(key.into(), value.into());
    }
    Ok((env, socket_dir))
}
pub fn game_environment(root: &Path, mut env: Environment) -> Result<Environment> {
    proton::check(root)?;
    proton::prepare_game_runtime(root)?;
    require(
        gsx::complete(root),
        "GSX setup is incomplete. Recover it under Mods in Flightdeck before starting MSFS.",
    )?;
    let prefix = root.join("local/msfs-prefix");
    for (key,value) in [("WINEPREFIX",prefix.to_string_lossy().into_owned()),("WINEARCH","win64".into()),("WINEESYNC","0".into()),("WINEFSYNC","0".into()),("WINEDEBUG","-all,err+all,warn+gdkc,fixme+gdkc,warn+mmdevapi,warn+pulse,warn+alsa,warn+xaudio2,warn+dsound,warn+winegstreamer".into()),("XODUS_USER_RUNTIME","1".into())]{env.insert(key.into(),value);}
    // At this level the renderer names its version, the device it created
    // and each swapchain: a few lines per session. Without them the log of a
    // Direct3D 11 game cannot show whether anything was ever drawn.
    env.entry("DXVK_LOG_LEVEL".into()).or_insert("info".into());
    env.entry("VKD3D_DEBUG".into()).or_insert("warn".into());
    let old = env
        .get("WINEDLLOVERRIDES")
        .filter(|v| !v.is_empty())
        .map(|v| format!("{v};"))
        .unwrap_or_default();
    env.insert(
        "WINEDLLOVERRIDES".into(),
        format!("{old}xgameruntime=n;xgameruntime_original=n,b;xodus_store_test=b"),
    );
    let old = env
        .get("WINEDLLPATH")
        .filter(|v| !v.is_empty())
        .map(|v| format!(":{v}"))
        .unwrap_or_default();
    env.insert(
        "WINEDLLPATH".into(),
        format!("{}{old}", root.join("local/store-runtime").display()),
    );
    if files::exists(&root.join(fenix::MARKER)) {
        let state = runtime::value(&root.join(fenix::MARKER))?;
        require(
            state["state"] == "installed",
            "Fenix setup is incomplete. Restore or finish it in Flightdeck before starting MSFS.",
        )?;
        for key in [
            "WINE_TRACK_WRITECOPY",
            "WINE_D2D1_DISPLAY_EFFECTS",
            "WINE_D2D1_GEOMETRY_PROVIDER",
            "WINE_DWRITE_UNHINTED_OUTLINES",
            "DOTNET_SYSTEM_GLOBALIZATION_USENLS",
            "DOTNET_ReadyToRun",
            "PROTON_NO_NTSYNC",
            "WINE_DISABLE_FAST_SYNC",
            "WINE_FENIX_HELPER_WINDOWS",
        ] {
            if let Some(value) = crate::wine::environment(&prefix, &root.join("runner")).remove(key)
            {
                env.insert(key.into(), value);
            }
        }
        env.insert("WINE_FENIX_WINDOW_GUARD".into(), "1".into());
        env.insert("WINE_FENIX_DISPLAY_REFRESH".into(), "1".into());
    }
    let portable = proton::selection(root)?.is_some();
    let runner = root.join("runner");
    let wine = if portable {
        runtime::wine(&runner)
    } else {
        runner.join("files/bin/wine")
    };
    env.insert(
        "XODUS_WINE_RUNNER".into(),
        wine.to_string_lossy().into_owned(),
    );
    env.insert(
        "FLIGHTDECK_PROTON_LOADER".into(),
        if portable { "portable" } else { "native" }.into(),
    );
    if portable {
        env.insert("WINE_DISABLE_FAST_SYNC".into(), "1".into());
        env.insert("WINELOADER".into(), wine.to_string_lossy().into_owned());
        env.insert(
            "WINESERVER".into(),
            runner
                .join("files/bin/wineserver")
                .to_string_lossy()
                .into_owned(),
        );
    }
    if root.join("private/local-saves.enabled").is_file()
        && root.join("private/local-saves").is_dir()
    {
        env.insert("XODUS_LOCAL_GAMESAVE".into(), "1".into());
        env.insert(
            "XODUS_LOCAL_GAMESAVE_ROOT".into(),
            format!(
                "Z:{}\\private\\local-saves",
                root.to_string_lossy().replace('/', "\\")
            ),
        );
    } else {
        env.remove("XODUS_LOCAL_GAMESAVE");
        env.remove("XODUS_LOCAL_GAMESAVE_ROOT");
    }
    env.insert(
        "XODUS_STORE_MARKET".into(),
        game_update::configured_market(root)?,
    );
    Ok(env)
}

pub fn socket_ready(path: &Path) -> bool {
    (|| -> std::io::Result<bool> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        let payload = b"MSFS launcher probe";
        let mut request = Vec::from(0x58445358_u32.to_le_bytes());
        request.extend(1_u16.to_le_bytes());
        request.extend((payload.len() as u16).to_le_bytes());
        request.extend(payload);
        stream.write_all(&request)?;
        request[4] = 2;
        let mut response = vec![0; request.len()];
        stream.read_exact(&mut response)?;
        Ok(response == request)
    })()
    .unwrap_or(false)
}
/// Reclaim only a disconnected socket in this runtime's private socket folder.
/// A protocol timeout or unexpected response is not evidence of a stale listener.
fn prepare_service_socket(root: &Path, folder: &Path, lease: &File) -> Result<bool> {
    let private = files::directory(&root.join("private"), true)?;
    crate::cloud_fs::lease(&private, lease)?;
    let directory = files::directory(folder, true)?;
    let path = folder.join("xodus.sock");
    let before = match path.symlink_metadata() {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    require(
        before.file_type().is_socket() && before.uid() == files::uid() && before.nlink() == 1,
        "The existing Xodus socket is not owned by this launcher session.",
    )?;
    if socket_ready(&path) {
        return Ok(true);
    }
    require(
        UnixStream::connect(&path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionRefused),
        "An Xodus socket exists but is not accepting connections.",
    )?;
    // connect() also refuses a live socket between bind() and listen(). Only a
    // filesystem remnant with no bound kernel socket is safe to reclaim.
    let sockets = files::read_public(Path::new("/proc/net/unix"), 16 * 1024 * 1024)?;
    let bound = sockets
        .split(|byte| *byte == b'\n')
        .skip(1)
        .any(|mut line| {
            for _ in 0..7 {
                line = line.trim_ascii_start();
                let Some(end) = line.iter().position(u8::is_ascii_whitespace) else {
                    return false;
                };
                line = &line[end..];
            }
            line.trim_ascii_start() == path.as_os_str().as_encoded_bytes()
        });
    require(
        !bound,
        "An Xodus socket is still owned by a running service.",
    )?;
    let after = path.symlink_metadata()?;
    require(
        after.file_type().is_socket()
            && after.uid() == files::uid()
            && (
                before.dev(),
                before.ino(),
                before.ctime(),
                before.ctime_nsec(),
            ) == (after.dev(), after.ino(), after.ctime(), after.ctime_nsec()),
        "The Xodus socket changed while checking the previous session.",
    )?;
    crate::cloud_fs::same(folder, &directory, true)?;
    crate::cloud_fs::lease(&private, lease)?;
    rustix::fs::unlinkat(&directory, "xodus.sock", rustix::fs::AtFlags::empty())?;
    directory.sync_all()?;
    Ok(false)
}
pub fn inherited_lease(root: &Path, number: i32) -> Result<File> {
    let file = inherited_fd::duplicate(number)?;
    files::directory(&root.join("private"), true)?;
    let info = file.metadata()?;
    let expected = root.join("private/play.lock").symlink_metadata()?;
    require(
        info.is_file()
            && expected.is_file()
            && info.uid() == files::uid()
            && info.nlink() == 1
            && info.mode() & 0o077 == 0
            && (info.dev(), info.ino()) == (expected.dev(), expected.ino()),
        "The runtime does not support this managed launch.",
    )?;
    let probe = files::open_at(rustix::fs::CWD, root.join("private/play.lock"), false, true)?;
    require(
        matches!(
            rustix::fs::flock(&probe, rustix::fs::FlockOperation::NonBlockingLockExclusive),
            Err(rustix::io::Errno::WOULDBLOCK)
        ),
        "The runtime descriptor does not own its lease.",
    )?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
    Ok(file)
}
pub fn stop_group(child: &mut Child) -> Result<()> {
    let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) else {
        return Ok(());
    };
    for (signal, seconds) in [
        (rustix::process::Signal::INT, 4),
        (rustix::process::Signal::TERM, 1),
    ] {
        let _ = rustix::process::kill_process_group(pid, signal);
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            if child.try_wait()?.is_some()
                && matches!(
                    rustix::process::test_kill_process_group(pid),
                    Err(rustix::io::Errno::SRCH)
                )
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    child.wait()?;
    Ok(())
}
struct Session<'a> {
    root: &'a Path,
    service: Option<Child>,
    game: Option<Child>,
}
impl Drop for Session<'_> {
    fn drop(&mut self) {
        if let Some(game) = &mut self.game {
            let _ = stop_group(game);
            // Fenix belongs to the simulator session: end all of it, including
            // the app and its WebView, rather than fencing the next launch.
            let mut companions = wine_processes::FENIX.to_vec();
            if runtime::value(&self.root.join(gsx::STARTUP))
                .is_ok_and(|v| v["format"].as_u64() == Some(1) && v["enabled"] == true)
            {
                companions.extend(gsx::COMPANIONS);
            }
            if crate::navigraph::startup_enabled(self.root) {
                companions.extend(crate::navigraph::SIMLINK);
            }
            if crate::mobiflight::startup_enabled(self.root) {
                companions.extend(crate::mobiflight::PROCESSES);
            }
            if let Err(error) = wine_processes::stop(self.root, &companions) {
                eprintln!("Windows process cleanup did not finish: {error}");
            }
        }
        if let Some(service) = &mut self.service {
            let _ = stop_group(service);
        }
    }
}
async fn pending_stop(
    term: &mut tokio::signal::unix::Signal,
    interrupt: &mut tokio::signal::unix::Signal,
) -> Option<u8> {
    // Synchronous preparation can leave a signal in Tokio's OS pipe. Give the
    // reactor a turn before allowing a spawn; an immediately ready fallback or
    // just polling recv() would miss that signal. Prefer Stop if both are ready.
    tokio::select! {
        biased;
        _ = term.recv() => Some(143),
        _ = interrupt.recv() => Some(130),
        _ = tokio::time::sleep(Duration::from_millis(1)) => None,
    }
}
pub fn run(root: &Path, lease: Option<i32>) -> Result<u8> {
    run_with_state(
        root,
        lease,
        &files::xdg("XDG_STATE_HOME", ".local/state").join("flightdeck"),
    )
}
pub fn run_with_state(root: &Path, lease: Option<i32>, state_dir: &Path) -> Result<u8> {
    // Install termination handlers before synchronous prefix/environment work.
    // Pending Stop requests are consumed before either child can start.
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (mut term, mut interrupt) = {
        let _entered = executor.enter();
        (
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
        )
    };
    crate::display_preflight::restore_pending(state_dir, &AtomicBool::new(false))?;
    let root = root.canonicalize()?;
    let _lease = if let Some(number) = lease {
        inherited_lease(&root, number)?
    } else {
        files::Lease::acquire(&root.join("private/play.lock"), true)?.0
    };
    crate::display_preflight::ensure(&root, state_dir, &AtomicBool::new(false))?;
    let game = Game::for_runtime(&root)?;
    let (env, socket_dir) = environment(&root)?;
    let env = game_environment(&root, env)?;
    let game_dir = game.path(&root);
    require(
        game_dir.join(game.executable()).is_file()
            && game_dir.join(".xodus-streaming.msixvc").is_file(),
        "The configured game package is incomplete.",
    )?;
    // Older sessions make room first. A log that cannot be removed safely is
    // no reason to refuse the start.
    let _ = crate::run_logs::prune(&root.join("private"), crate::run_logs::KEPT);
    let run = tx::new_directory(
        &root.join("private"),
        &format!("run-{}-", chrono::Utc::now().format("%Y%m%d-%H%M%S")),
    )?;
    let mut session = Session {
        root: &root,
        service: None,
        game: None,
    };
    let mut logs = Vec::new();
    executor.block_on(async{
        let service_ready=prepare_service_socket(&root,&socket_dir,&_lease)?;
        if let Some(code)=pending_stop(&mut term,&mut interrupt).await{return Ok(code);}
        if !service_ready{let log=process::log(&run.join("service.log"))?;let mut command=Command::new(root.join("bin/xodus-service"));command.env_clear().envs(&env).env("XDG_RUNTIME_DIR",&socket_dir).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log.try_clone()?).process_group(0);session.service=Some(process::spawn(&mut command,None)?);logs.push(crate::run_logs::Bounded::new(&log,crate::run_logs::LIMIT)?);
            let end=Instant::now()+Duration::from_secs(60);loop{if socket_ready(&socket_dir.join("xodus.sock")){break;}require(Instant::now()<end&&session.service.as_mut().is_some_and(|v|v.try_wait().is_ok_and(|v|v.is_none())),"Xodus service did not start. See the private service log.")?;tokio::select!{_ = term.recv()=>return Ok(143),_ = interrupt.recv()=>return Ok(130),_ = tokio::time::sleep(Duration::from_millis(200))=>{}}}
        }
        if let Some(code)=pending_stop(&mut term,&mut interrupt).await{return Ok(code);}
        // The self-contained native launcher acts as the Xodus exec target. Its
        // executable-name dispatch supplies the runtime without a shell script.
        let helper=run.join("xodus-wine-launch");std::os::unix::fs::symlink(std::env::current_exe()?,&helper)?;
        let log=process::log(&run.join("game.log"))?;let mut command=Command::new(root.join("bin/xodus-cli"));command.arg("run").arg(&game_dir).arg(&helper).args(["--exe",game.executable(),"--market",env.get("XODUS_STORE_MARKET").ok_or(Error::Invalid("Invalid Store market."))?]).env_clear().envs(&env).env("FLIGHTDECK_HELPER_RUNTIME",&root).current_dir(&game_dir).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log.try_clone()?).process_group(0);session.game=Some(process::spawn(&mut command,None)?);logs.push(crate::run_logs::Bounded::new(&log,crate::run_logs::LIMIT)?);let mut checked=Instant::now();
        println!("Starting MSFS. Private logs: {}",run.display());let status=loop{let game=session.game.as_mut().ok_or(Error::Invalid("Missing game process."))?;if let Some(status)=game.try_wait()?{break status;}
            // The game and the service keep their logs open. What they write
            // beyond the kept start and end is given back every few seconds.
            if checked.elapsed()>=Duration::from_secs(10){checked=Instant::now();for log in &mut logs{log.enforce();}}
            let stop=tokio::select!{_ = term.recv()=>true,_ = interrupt.recv()=>true,_ = tokio::time::sleep(Duration::from_millis(50))=>false};if stop{stop_group(game)?;break game.wait()?;}};
        let code=status.code().unwrap_or_else(||128+status.signal().unwrap_or(1));println!("MSFS process ended with status {code}. Log: {}/game.log",run.display());Ok(code.clamp(0,255) as u8)
    })
}
pub fn spawn_reserved(ctx: &Context) -> Result<Value> {
    let root = ctx.root()?;
    require(
        runtime::ready(root),
        "Die Runtime ist nicht startbereit oder das Spiel läuft bereits.",
    )?;
    wine_processes::idle(&root.join("local/msfs-prefix"))?;
    crate::display_preflight::ensure(root, &ctx.launcher.state_dir, &ctx.cancel)?;
    crate::fenix_update::ensure(ctx)?;
    crate::framework_maintenance::ensure(ctx)?;
    ctx.launcher.lock().graphics_report = Value::Null;
    let at = files::now();
    let initial = std::env::vars().collect();
    let mut record =
        graphics_diagnostics::launch_record(root, &Value::Null, &initial, "preparing", &at);
    graphics_diagnostics::save_launch(root, &record);
    let prepared = (|| -> Result<_> {
        let (mut environment, report) = graphics::prepare(root, initial)?;
        if proton::selection(root)?.is_some() {
            environment.insert("WINE_DISABLE_FAST_SYNC".into(), "1".into());
            environment.insert(
                "WINELOADER".into(),
                runtime::wine(&root.join("runner"))
                    .to_string_lossy()
                    .into_owned(),
            );
            environment.insert(
                "WINESERVER".into(),
                root.join("runner/files/bin/wineserver")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        Ok((vr::prepare(root, environment, &ctx.cancel)?, report))
    })();
    let (mut environment, report) = match prepared {
        Ok(value) => value,
        Err(error) => {
            record["state"] = json!("preparation_failed");
            graphics_diagnostics::save_launch(root, &record);
            return Err(Error::Graphics(Box::new(error)));
        }
    };
    record = graphics_diagnostics::launch_record(root, &report, &environment, "prepared", &at);
    graphics_diagnostics::save_launch(root, &record);
    environment.remove("FLIGHTDECK_STORE_LAUNCH");
    if let Some(build) = crate::store_diagnostics::launch_record(root) {
        environment.insert(
            "FLIGHTDECK_STORE_LAUNCH".into(),
            serde_json::to_string(&build)?,
        );
    }
    ctx.interrupted()?;
    let lease = ctx.lease()?;
    let log = process::log(&ctx.launcher.state_dir.join("launcher.log"))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--state-dir")
        .arg(&ctx.launcher.state_dir)
        .args(["run-game", "--runtime"])
        .arg(root)
        .arg("--lock-fd")
        .arg(lease.as_raw_fd().to_string())
        .env_clear()
        .envs(environment)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0);
    let mut s = ctx.launcher.lock();
    require(
        s.active.as_ref().is_some_and(|v| v.id == ctx.id) && s.process.is_none() && !s.closing,
        "Die Runtime ist nicht für diesen Spielstart reserviert.",
    )?;
    let child = match process::spawn(&mut command, Some(&lease)) {
        Ok(child) => child,
        Err(error) => {
            record["state"] = json!("spawn_failed");
            graphics_diagnostics::save_launch(root, &record);
            return Err(error);
        }
    };
    s.process = Some(child);
    s.started = Some(Instant::now());
    s.started_at = Some(files::now());
    s.stopping = false;
    s.exit_code = None;
    s.graphics_report = report;
    record["state"] = json!("spawned");
    graphics_diagnostics::save_launch(root, &record);
    Ok(json!({"ok":true}))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod fenix_environment_tests {
    use super::*;

    #[test]
    fn fenix_game_and_helpers_select_the_same_server_synchronization() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        files::private_dir(&root.join("private")).unwrap();
        files::atomic_json(
            &root.join("private/runtime.json"),
            &serde_json::json!({"market": "AT"}),
        )
        .unwrap();
        let input = Environment::from([("DXVK_HUD".into(), "fps".into())]);
        let plain = game_environment(root, input.clone()).unwrap();
        assert!(!plain.contains_key("PROTON_NO_NTSYNC"));
        files::atomic_json(
            &root.join(fenix::MARKER),
            &serde_json::json!({"state": "installed"}),
        )
        .unwrap();
        let game = game_environment(root, input).unwrap();
        let helper =
            crate::wine::environment(&root.join("local/msfs-prefix"), &root.join("runner"));
        for key in ["PROTON_NO_NTSYNC", "WINE_DISABLE_FAST_SYNC"] {
            assert_eq!(game[key], "1", "{key}");
            assert_eq!(game[key], helper[key], "{key}");
        }
        assert_eq!(game["DXVK_HUD"], "fps");
    }
}
