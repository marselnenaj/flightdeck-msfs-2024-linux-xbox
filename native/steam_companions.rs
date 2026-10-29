// SPDX-License-Identifier: MIT
//! Managed companions follow the selected Steam game, including external launches.
use crate::{
    Result, addon_apps::Profile, display_refresh, files, installations::Installation, process,
    steam_fenix, steam_session, wine, wine_processes,
};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub struct Session {
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}
struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Session {
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    pub fn start(state_dir: &Path, item: &Installation) -> Result<Self> {
        let addon = Profile::steam(state_dir, item.clone());
        let fenix = steam_fenix::companion_profile(state_dir, item).ok();
        let mut names = Vec::new();
        if fenix.is_some() {
            names.extend_from_slice(wine_processes::FENIX);
        }
        if addon.enabled(crate::gsx::STARTUP) {
            names.extend_from_slice(crate::gsx::COMPANIONS);
        }
        if addon.enabled(crate::navigraph::STARTUP) {
            names.extend_from_slice(crate::navigraph::SIMLINK);
        }
        if addon.enabled(crate::mobiflight::STARTUP) {
            names.extend_from_slice(crate::mobiflight::PROCESSES);
        }
        let prefix_generation = item.prefix_generation();
        let profile = if let Some(profile) = fenix.clone() {
            profile
        } else {
            addon.prepare()?;
            crate::display_refresh::Profile {
                prefix: addon.prefix.clone(),
                runner: addon.runner()?,
                private: addon.root.join("private"),
                library: item.game_path().join("SimConnect_internal.dll"),
            }
        };
        let held = files::Lease::acquire(&profile.private.join("steam-companions.lock"), true)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&cancel);
        let done = Arc::new(AtomicBool::new(false));
        let completed = Arc::clone(&done);
        let item = item.clone();
        std::thread::spawn(move || {
            let _completed = Completion(completed);
            let _held = held;
            let refresh_cancel = Arc::new(AtomicBool::new(false));
            let refresh = if fenix.is_some() && profile.library.is_file() {
                let profile = profile.clone();
                let cancelled = Arc::clone(&refresh_cancel);
                Some(std::thread::spawn(move || {
                    display_refresh::worker(&profile, &cancelled)
                }))
            } else {
                None
            };
            let helper = profile
                .prefix
                .join("drive_c/windows/system32/FenixWindowGuard.exe");
            let mut guard = None;
            if fenix.is_some()
                && helper.is_file()
                && wine_processes::Processes::new(&profile.prefix)
                    .is_ok_and(|p| !p.any(Some(&["fenixwindowguard.exe"])))
            {
                let mut command = Command::new(crate::runtime::wine(&profile.runner));
                command
                    .arg(helper)
                    .env_clear()
                    .envs(wine::environment(&profile.prefix, &profile.runner))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                guard = process::spawn(&mut command, None).ok();
            }
            let mut ended = false;
            while !stopped.load(Ordering::Relaxed) {
                match steam_session::running(&item) {
                    Ok(false) => {
                        ended = true;
                        break;
                    }
                    Err(_) => break,
                    Ok(true) => (),
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            refresh_cancel.store(true, Ordering::Relaxed);
            if let Some(refresh) = refresh {
                let _ = refresh.join();
            }
            if let Some(mut guard) = guard {
                let _ = process::terminate(&mut guard);
            }
            // Closing/restarting Flightdeck leaves an ongoing Steam flight
            // alone. Only a confirmed game exit ends this profile's companions.
            if ended && item.prefix_generation() == prefix_generation {
                let _ = wine_processes::stop_prefix(&profile.prefix, &profile.runner, &names);
            }
        });
        Ok(Self { cancel, done })
    }
}
