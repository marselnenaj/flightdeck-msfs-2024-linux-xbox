// SPDX-License-Identifier: MIT
//! Managed desktop add-ons share the selected simulator's existing Wine profile.
use crate::{
    Error, Result,
    backend::{Context, Launcher},
    error::require,
    fenix, files,
    games::Game,
    gsx,
    installations::{Installation, Source},
    mods, process, proton, runtime,
    wine::Wine,
    wine_processes,
};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

/// Real profile paths and separate launcher metadata; Steam never becomes a
/// synthetic Xbox runtime and never changes the saved Xbox selection.
#[derive(Clone)]
pub(crate) struct Profile {
    pub root: PathBuf,
    pub prefix: PathBuf,
    pub game: Game,
    pub item: Option<Installation>,
    pub context: String,
    bound_runner: Option<PathBuf>,
    pub revision: String,
}
impl Profile {
    pub fn xbox(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.into(),
            prefix: root.join("local/msfs-prefix"),
            game: Game::for_runtime(root)?,
            item: None,
            context: root.to_string_lossy().into_owned(),
            bound_runner: None,
            revision: String::new(),
        }
        .bind())
    }
    pub fn steam(state: &Path, item: Installation) -> Self {
        Self {
            root: state.join("steam").join(&item.id).join("addon-apps"),
            prefix: item.prefix(),
            game: item.game,
            context: item.context(),
            item: Some(item),
            bound_runner: None,
            revision: String::new(),
        }
        .bind()
    }
    pub fn selected(app: &Launcher) -> Result<Self> {
        let state = app.lock();
        if let Some(item) = state.steam.clone() {
            Ok(Self::steam(&app.state_dir, item))
        } else {
            Self::xbox(
                state
                    .runtime
                    .as_deref()
                    .ok_or(Error::Invalid("Zuerst eine Runtime auswählen."))?,
            )
        }
    }
    fn resolve_runner(&self) -> Result<PathBuf> {
        if let Some(item) = &self.item {
            steam_runner(item)
        } else {
            Ok(self.root.join("runner").canonicalize()?)
        }
    }
    fn bind(mut self) -> Self {
        use std::os::unix::fs::MetadataExt;
        self.bound_runner = self.resolve_runner().ok();
        let stamps = self.bound_runner.as_ref().map(|p| {
            [runtime::wine(p), p.join("version")].map(|p| {
                p.metadata().ok().map(|m| {
                    serde_json::json!([m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()])
                })
            })
        });
        self.revision = files::sha256(
            &serde_json::to_vec(&serde_json::json!([
                self.context,
                self.bound_runner,
                stamps,
                fenix::identity(&self.prefix).ok()
            ]))
            .unwrap_or_default(),
        );
        self
    }
    pub fn runner(&self) -> Result<PathBuf> {
        self.bound_runner
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.resolve_runner())
    }
    pub fn family(&self) -> Result<Option<String>> {
        if self.item.is_some() {
            Ok(None)
        } else {
            mods::family(&self.root)
        }
    }
    pub fn locations(&self) -> Result<mods::Locations> {
        if self.item.is_some() {
            mods::profile_locations(&self.prefix, self.game, None)
        } else {
            mods::locations(&self.root)
        }
    }
    pub fn validate(&self, recovery: bool) -> Result<()> {
        if let Some(item) = &self.item {
            item.validate()?;
            require(
                item.context() == self.context,
                "Die Installation hat sich geändert. Add-on-Status neu laden.",
            )?;
            fenix_patch::steam::checked(&item.target()?, recovery).map_err(|_| {
                Error::Invalid(
                    "Starte MSFS einmal in Steam, damit das Windows-Profil angelegt wird.",
                )
            })?;
            let marker = item.target()?.marker();
            if files::exists(&marker) {
                let state = runtime::value(&marker)?;
                require(
                    ["installed", "restored"].contains(&state["state"].as_str().unwrap_or("")),
                    "Fenix setup is incomplete. Restore or finish it in Flightdeck before starting MSFS.",
                )?;
            }
        } else if recovery {
            proton::check(&self.root)?;
        } else {
            validate(&self.root)?;
        }
        self.runner()?;
        if !recovery {
            require(
                gsx::complete(&self.root),
                "Die GSX-Einrichtung ist unvollständig. Unter Mods wiederherstellen.",
            )?;
            files::directory(&self.prefix.join("drive_c"), false)?;
            require(
                files::exists(&self.prefix.join("system.reg")),
                "Starte MSFS einmal, damit das Windows-Profil angelegt wird.",
            )?;
        }
        Ok(())
    }
    pub fn current(&self, app: &Launcher) -> Result<()> {
        let current = Self::selected(app)?;
        require(
            current.context == self.context && current.revision == self.revision,
            "Die Installation oder Proton-Auswahl hat sich geändert. Add-on-Status neu laden.",
        )
    }
    pub fn prepare(&self) -> Result<()> {
        files::private_dir(&self.root.join("private"))?;
        files::private_dir(&self.root.join("local"))?;
        Ok(())
    }
    pub fn stop(&self, names: &[&str]) -> Result<()> {
        wine_processes::stop_prefix(&self.prefix, &self.runner()?, names)
    }
    pub fn wine<'a>(&self, ctx: &'a Context, log: &str) -> Result<Wine<'a>> {
        self.wine_with_cancel(&ctx.cancel, log)
    }
    pub fn wine_with_cancel<'a>(
        &self,
        cancel: &'a std::sync::atomic::AtomicBool,
        log: &str,
    ) -> Result<Wine<'a>> {
        let mut wine = Wine::new(
            &self.prefix,
            &self.runner()?,
            &self.root.join("private").join(log),
            cancel,
        )?;
        wine.without_fenix();
        if let Some(item) = &self.item {
            let Source::Steam { root, .. } = &item.source else {
                return Err(Error::Invalid("Ungültige Steam-Installation."));
            };
            wine.set_environment(
                [
                    (
                        "STEAM_COMPAT_CLIENT_INSTALL_PATH",
                        root.to_string_lossy().into_owned(),
                    ),
                    (
                        "STEAM_COMPAT_DATA_PATH",
                        self.prefix
                            .parent()
                            .ok_or(Error::Invalid("Ungültiges Steam-Profil."))?
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    (
                        "STEAM_COMPAT_INSTALL_PATH",
                        item.game_path().to_string_lossy().into_owned(),
                    ),
                    ("SteamAppId", item.appid().to_owned()),
                    ("SteamGameId", item.appid().to_owned()),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            );
        }
        Ok(wine)
    }
    pub fn enabled(&self, marker: &str) -> bool {
        runtime::value(&self.root.join(marker)).is_ok_and(|v| {
            v["format"] == 1
                && v["enabled"] == true
                && (self.item.is_none()
                    || fenix::identity(&self.prefix)
                        .is_ok_and(|id| v["prefix_id"] == serde_json::json!(id)))
        })
    }
    pub fn rebind_startups(&self, old: serde_json::Value) -> Result<()> {
        if self.item.is_none() {
            return Ok(());
        }
        for marker in [crate::navigraph::STARTUP, crate::gsx::STARTUP] {
            let path = self.root.join(marker);
            if !files::exists(&path) {
                continue;
            }
            let mut value = runtime::value(&path)?;
            if value["format"] == 1 && value["prefix_id"] == old {
                value["prefix_id"] = serde_json::json!(fenix::identity(&self.prefix)?);
                files::atomic_json(&path, &value)?;
            }
        }
        Ok(())
    }
    pub fn lease(&self) -> Result<Vec<File>> {
        self.prepare()?;
        if let Some(item) = &self.item {
            // Share the standalone Fenix engine's mutation lock and Proton's
            // real prefix lock. Do not change Steam's file permissions.
            let state = item.target()?.state();
            files::private_dir(&state)?;
            let first = files::Lease::acquire(&state.join("lock"), true)?.0;
            let parent = files::directory(
                self.prefix
                    .parent()
                    .ok_or(Error::Invalid("Ungültiges Steam-Profil."))?,
                false,
            )?;
            use rustix::fs::{FlockOperation, Mode, OFlags};
            use std::os::unix::fs::MetadataExt;
            let file = File::from(rustix::fs::openat(
                &parent,
                "pfx.lock",
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            let info = file.metadata()?;
            require(
                info.is_file()
                    && info.uid() == files::uid()
                    && info.nlink() == 1
                    && info.mode() & 0o022 == 0,
                "Ungültige Steam-Profilsperre.",
            )?;
            rustix::fs::flock(&file,FlockOperation::NonBlockingLockExclusive).map_err(|_|Error::Invalid("Steam verwendet dieses Windows-Profil gerade. Versuche es nach dem Spielende erneut."))?;
            Ok(vec![first, file])
        } else {
            Ok(Vec::new())
        }
    }
}

const STEAM_RUNNER: &str = "Die Proton-Version dieses Steam-Spiels konnte nicht eindeutig erkannt werden. Wähle Proton in Steam unter Eigenschaften → Kompatibilität und starte MSFS einmal.";
fn tool_names(
    block: &fenix_patch::vdf::Block,
    out: &mut BTreeSet<String>,
    depth: usize,
) -> Result<()> {
    require(depth < 16, STEAM_RUNNER)?;
    for entry in &block.entries {
        if let fenix_patch::vdf::Item::Block(inner) = &entry.value {
            if entry.key.eq_ignore_ascii_case("compat_tools") {
                out.extend(inner.entries.iter().map(|e| e.key.clone()));
            }
            tool_names(inner, out, depth + 1)?;
        }
    }
    Ok(())
}
pub(crate) fn steam_runner(item: &Installation) -> Result<PathBuf> {
    let Source::Steam { root, .. } = &item.source else {
        return Err(Error::Invalid(STEAM_RUNNER));
    };
    let config = root.join("config/config.vdf");
    let selected = if files::exists(&config) {
        let raw = files::read_public(&config, 16 * 1024 * 1024)?;
        let text = std::str::from_utf8(&raw).map_err(|_| Error::Invalid(STEAM_RUNNER))?;
        fenix_patch::vdf::compat_tool(text, item.appid())
            .map_err(|_| Error::Invalid(STEAM_RUNNER))?
            .or(fenix_patch::vdf::compat_tool(text, "0")
                .map_err(|_| Error::Invalid(STEAM_RUNNER))?)
    } else {
        None
    };
    let mut bases = BTreeSet::from([
        root.join("compatibilitytools.d"),
        PathBuf::from("/usr/share/steam/compatibilitytools.d"),
    ]);
    bases.extend(
        crate::installations::libraries(root)?
            .into_iter()
            .map(|p| p.join("steamapps/common")),
    );
    let info = item
        .prefix()
        .parent()
        .ok_or(Error::Invalid(STEAM_RUNNER))?
        .join("config_info");
    let previous = if files::exists(&info) {
        Some(String::from_utf8_lossy(&files::read_public(&info, 64 * 1024)?).into_owned())
    } else {
        None
    };
    let mut matches = BTreeSet::new();
    for base in bases {
        let Ok(entries) = fs::read_dir(&base) else {
            continue;
        };
        for (n, entry) in entries.enumerate() {
            require(n < 4096, STEAM_RUNNER)?;
            let path = entry?.path();
            if !path.is_dir()
                || !path.join("proton").is_file()
                || !runtime::wine(&path).is_file()
                || !path.join("files/bin/wineserver").is_file()
            {
                continue;
            }
            let mut names = BTreeSet::new();
            for name in ["compatibilitytool.vdf", "compatibilitytools.vdf"] {
                let manifest = path.join(name);
                if !files::exists(&manifest) {
                    continue;
                }
                let raw = files::read_public(&manifest, 1024 * 1024)?;
                let block = fenix_patch::vdf::parse(
                    std::str::from_utf8(&raw).map_err(|_| Error::Invalid(STEAM_RUNNER))?,
                )
                .map_err(|_| Error::Invalid(STEAM_RUNNER))?;
                tool_names(&block, &mut names, 0)?;
            }
            let folder = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let builtin = selected.as_deref().is_some_and(|name| {
                name == "proton_experimental" && folder == "Proton - Experimental"
                    || name.strip_prefix("proton_").is_some_and(|major| {
                        !major.is_empty()
                            && major.bytes().all(|c| c.is_ascii_digit())
                            && folder == format!("Proton {major}.0")
                    })
            });
            let chosen = if let Some(name) = &selected {
                names.contains(name) || builtin
            } else {
                previous.as_deref().is_some_and(|text| {
                    text.lines().any(|line| {
                        Path::new(line)
                            .canonicalize()
                            .ok()
                            .zip(path.join("files/share/fonts").canonicalize().ok())
                            .is_some_and(|(a, b)| a == b)
                    })
                })
            };
            if chosen {
                matches.insert(path.canonicalize()?);
            }
        }
    }
    require(matches.len() == 1, STEAM_RUNNER)?;
    matches
        .into_iter()
        .next()
        .ok_or(Error::Invalid(STEAM_RUNNER))
}

pub(crate) fn job(
    app: &Launcher,
    kind: &str,
    profile: &Profile,
) -> (bool, bool, bool, serde_json::Value) {
    let mut s = app.lock();
    Launcher::poll(&mut s);
    let playing = if let Some(item) = &profile.item {
        crate::steam_session::running(item).unwrap_or(true)
    } else {
        s.process.is_some() || Launcher::external(&s)
    };
    let job = s
        .jobs
        .get(kind)
        .filter(|j| j["runtime_path"] == profile.context)
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    (s.active.is_some(), playing, s.closing, job)
}

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

pub(crate) fn interactive(
    ctx: &Context,
    profile: &Profile,
    wine: &Wine,
    arguments: &[&Path],
    names: &[&str],
    failure: &'static str,
) -> Result<()> {
    let prefix = &profile.prefix;
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
                profile.stop(names)?;
                process::terminate(&mut child)?;
                return ctx.interrupted();
            }
            let code = child.try_wait()?;
            let running = wine_processes::Processes::new(prefix)?.applications_running();
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
        let _ = profile.stop(names);
        let _ = process::terminate(&mut child);
    }
    result
}
