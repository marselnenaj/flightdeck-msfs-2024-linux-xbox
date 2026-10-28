// SPDX-License-Identifier: MIT
//! Managed desktop add-ons share the selected simulator's existing Wine profile.
use crate::{
    Result, backend::Context, error::require, fenix, files, gsx, process, proton, runtime,
    wine::Wine, wine_processes,
};
use std::{
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub(crate) fn validate(root: &Path) -> Result<()> {
    proton::check(root)?;
    require(
        gsx::complete(root),
        "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.",
    )?;
    if files::exists(&root.join(fenix::MARKER)) {
        require(
            runtime::value(&root.join(fenix::MARKER))?["state"] == "installed",
            "Fenix setup is incomplete. Restore or finish it in Flightdeck before starting MSFS.",
        )?;
    }
    let prefix = root.join("local/msfs-prefix");
    files::directory(&prefix.join("drive_c"), false)?;
    require(
        files::exists(&prefix.join("system.reg")),
        "Starte MSFS einmal, damit das Windows-Profil angelegt wird.",
    )
}

pub(crate) fn wine<'a>(ctx: &'a Context, root: &Path, log: &str) -> Result<Wine<'a>> {
    let mut wine = Wine::new(
        &root.join("local/msfs-prefix"),
        &root.join("runner"),
        &root.join("private").join(log),
        &ctx.cancel,
    )?;
    wine.without_fenix();
    Ok(wine)
}

pub(crate) fn interactive(
    ctx: &Context,
    root: &Path,
    wine: &Wine,
    arguments: &[&Path],
    names: &[&str],
    failure: &'static str,
) -> Result<()> {
    let prefix = root.join("local/msfs-prefix");
    let mut command = wine.command()?;
    command.args(arguments);
    if let Some(directory) = arguments.first().and_then(|v| v.parent()) {
        command.current_dir(directory);
    }
    let mut child = process::spawn(&mut command, None)?;
    let mut quiet = None;
    let result = (|| {
        loop {
            if ctx.pause.load(Ordering::Relaxed) || ctx.cancel.load(Ordering::Relaxed) {
                wine_processes::stop(root, names)?;
                process::terminate(&mut child)?;
                return ctx.interrupted();
            }
            let code = child.try_wait()?;
            let running = wine_processes::Processes::new(&prefix)?.applications_running();
            if let Some(code) = code
                && !running
            {
                let since = quiet.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(1500) {
                    return require(code.success(), failure);
                }
            } else {
                quiet = None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    })();
    if result.is_err() {
        let _ = wine_processes::stop(root, names);
        let _ = process::terminate(&mut child);
    }
    result
}
