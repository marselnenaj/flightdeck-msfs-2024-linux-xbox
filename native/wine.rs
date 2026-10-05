// SPDX-License-Identifier: MIT
//! Scoped Wine commands. Destructive setup is restricted to unpublished copies.
use crate::{
    Error, Result, backend::Context, error::require, files, process, runtime, transaction as tx,
};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::Duration,
};

pub const DOTNET_COMPATIBILITY: &[(&str, &str)] = &[
    ("DOTNET_SYSTEM_GLOBALIZATION_USENLS", "1"),
    ("DOTNET_ReadyToRun", "0"),
];
pub fn environment(prefix: &Path, runner: &Path) -> BTreeMap<String, String> {
    let mut env: BTreeMap<_, _> = std::env::vars()
        .filter(|(key, _)| {
            ![
                "WINE_DLL_FILE_MAP",
                "WINELOADER",
                "WINEDLLPATH",
                "WINESERVERSOCKET",
                "WINEPRELOADRESERVE",
                "WINELOADERNOEXEC",
            ]
            .contains(&key.as_str())
        })
        .collect();
    for (key, value) in [
        ("WINEARCH", "win64"),
        ("WINEESYNC", "0"),
        ("WINEFSYNC", "0"),
        ("WINEDEBUG", "-all"),
        ("WINE_DISABLE_FAST_SYNC", "1"),
        (
            "WINE_TRACK_WRITECOPY",
            "apps:Fenix.exe,FenixSystem.exe,FenixDisplay.exe,FenixCDU.exe,FlightSimulator2024.exe",
        ),
        ("WINE_D2D1_DISPLAY_EFFECTS", "FenixDisplay.exe;FenixCDU.exe"),
        ("WINE_D2D1_GEOMETRY_PROVIDER", "FenixDisplay.exe"),
        ("WINE_FENIX_HELPER_WINDOWS", "1"),
        (
            "WINE_DWRITE_UNHINTED_OUTLINES",
            "FenixDisplay.exe;FenixCDU.exe",
        ),
        ("WINEDLLOVERRIDES", "winemenubuilder.exe=d"),
    ] {
        env.insert(key.into(), value.into());
    }
    for (key, value) in DOTNET_COMPATIBILITY {
        env.insert((*key).into(), (*value).into());
    }
    for (key, path) in [
        ("WINEPREFIX", prefix.into()),
        ("WINE", runtime::wine(runner)),
        ("WINESERVER", runner.join("files/bin/wineserver")),
    ] {
        env.insert(key.into(), path.to_string_lossy().into_owned());
    }
    let flags = env
        .entry("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS".into())
        .or_default();
    if !flags
        .split_whitespace()
        .any(|v| v == "--disable-features=HideCursorWhileTyping")
    {
        flags.push_str(" --disable-features=HideCursorWhileTyping");
    }
    env
}
pub struct Wine<'a> {
    pub prefix: PathBuf,
    pub runner: PathBuf,
    pub log: File,
    pub cancel: &'a AtomicBool,
    fenix: bool,
}
impl<'a> Wine<'a> {
    pub fn new(prefix: &Path, runner: &Path, log: &Path, cancel: &'a AtomicBool) -> Result<Self> {
        tx::prefix_system32(prefix)?;
        let runner = runner.canonicalize()?;
        files::directory(&runner, false)?;
        Ok(Self {
            prefix: prefix.canonicalize()?,
            runner,
            log: process::log(log)?,
            cancel,
            fenix: true,
        })
    }
    pub fn command(&self) -> Result<Command> {
        let mut c = Command::new(runtime::wine(&self.runner));
        c.env_clear()
            .envs(self.environment())
            .current_dir(&self.prefix)
            .stdin(Stdio::null())
            .stdout(self.log.try_clone()?)
            .stderr(self.log.try_clone()?);
        Ok(c)
    }
    pub fn without_fenix(&mut self) {
        self.fenix = false;
    }
    fn environment(&self) -> BTreeMap<String, String> {
        let mut env = environment(&self.prefix, &self.runner);
        if !self.fenix {
            for key in [
                "WINE_TRACK_WRITECOPY",
                "WINE_D2D1_DISPLAY_EFFECTS",
                "WINE_D2D1_GEOMETRY_PROVIDER",
                "WINE_FENIX_HELPER_WINDOWS",
                "WINE_DWRITE_UNHINTED_OUTLINES",
            ] {
                env.remove(key);
            }
        }
        env
    }
    pub fn run<S: AsRef<OsStr>>(
        &self,
        args: &[S],
        overrides: Option<&str>,
        accepted: &[i32],
        timeout: Duration,
    ) -> Result<i32> {
        let mut c = self.command()?;
        c.args(args);
        if let Some(value) = overrides {
            c.env("WINEDLLOVERRIDES", value);
        }
        let code = process::run(&mut c, timeout, self.cancel)?
            .code()
            .unwrap_or(-1);
        require(
            accepted.contains(&code),
            "Windows setup failed. See the private Fenix setup log.",
        )?;
        Ok(code)
    }
    pub fn reg(&self, key: &str, name: &str, value: &str, kind: &str) -> Result<()> {
        self.run(
            &["reg", "add", key, "/v", name, "/t", kind, "/d", value, "/f"],
            None,
            &[0],
            Duration::from_secs(180),
        )?;
        Ok(())
    }
    pub fn stop_staged(&self) -> Result<()> {
        for arg in ["-k", "-w"] {
            let mut c = Command::new(self.runner.join("files/bin/wineserver"));
            c.arg(arg)
                .env_clear()
                .envs(environment(&self.prefix, &self.runner))
                .stdin(Stdio::null())
                .stdout(self.log.try_clone()?)
                .stderr(self.log.try_clone()?);
            let code = process::run(&mut c, Duration::from_secs(30), &AtomicBool::new(false))?;
            require(
                code.success() || (arg == "-k" && code.code() == Some(1)),
                "Die private Wine-Umgebung konnte nicht beendet werden.",
            )?;
        }
        Ok(())
    }
}
/// The active profile must stay distinct throughout setup and cleanup.
pub struct StagedWine<'a> {
    pub wine: Wine<'a>,
    active: PathBuf,
    identity: (u64, u64),
}
impl<'a> StagedWine<'a> {
    pub fn new(
        root: &Path,
        stage: &Path,
        runner: &Path,
        log: &Path,
        ctx: &'a Context,
    ) -> Result<Self> {
        let root = root.canonicalize()?;
        let active = root.join("local/msfs-prefix");
        let stage = stage.canonicalize()?;
        require(
            stage.starts_with(root.join("local")) && stage != active,
            ".NET setup requires an unpublished profile copy.",
        )?;
        let original = files::directory(&active, false)?.metadata()?;
        let copied = files::directory(&stage, false)?.metadata()?;
        require(
            (original.dev(), original.ino()) != (copied.dev(), copied.ino()),
            ".NET setup cannot modify the active profile.",
        )?;
        Ok(Self {
            wine: Wine::new(&stage, runner, log, &ctx.cancel)?,
            active,
            identity: (copied.dev(), copied.ino()),
        })
    }
    pub fn check(&self) -> Result<()> {
        let copied = files::directory(&self.wine.prefix, false)?.metadata()?;
        let active = files::directory(&self.active, false)?.metadata()?;
        require(
            (copied.dev(), copied.ino()) == self.identity
                && (active.dev(), active.ino()) != self.identity,
            "The staged Windows profile changed; setup was stopped.",
        )
    }
    pub fn stop(&self) -> Result<()> {
        self.check()?;
        self.wine.stop_staged()
    }
}
impl crate::framework_repair::SetupWine for StagedWine<'_> {
    fn prefix(&self) -> &Path {
        &self.wine.prefix
    }
    fn run(&mut self, args: &[String], installer: bool, timeout: Duration) -> Result<()> {
        self.check()?;
        self.wine.run(
            args,
            installer.then_some("fusion=b;winemenubuilder.exe=d"),
            if installer { &[0, 194] } else { &[0] },
            timeout,
        )?;
        Ok(())
    }
    fn reg(&mut self, key: &str, name: &str, value: &str, kind: &str) -> Result<()> {
        self.check()?;
        self.wine.reg(key, name, value, kind)
    }
    fn stop(&mut self) -> Result<()> {
        StagedWine::stop(self)
    }
    fn list(&mut self) -> Result<String> {
        self.check()?;
        let bytes = process::output(
            self.wine.command()?.args(["uninstaller", "--list"]),
            Duration::from_secs(120),
            1024 * 1024,
            self.wine.cancel,
        )?;
        String::from_utf8(bytes).map_err(|_| Error::Invalid("Invalid Windows installer listing."))
    }
}
