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
// Windows tools must resolve their runtimes and assemblies inside the managed
// profile, not through a Linux SDK's inherited paths or startup hooks. Keep
// unrelated runtime tuning and diagnostics intact; registry settings are separate.
const HOST_DOTNET_OVERRIDES: &[&str] = &[
    "DOTNET_ROOT",
    "DOTNET_ROOT(x86)",
    "DOTNET_ROOT_X86",
    "DOTNET_ROOT_X64",
    "DOTNET_ROOT_ARM64",
    "DOTNET_HOST_PATH",
    "DOTNET_STARTUP_HOOKS",
    "DOTNET_ADDITIONAL_DEPS",
    "DOTNET_SHARED_STORE",
    "DOTNET_SERVICING",
    "DOTNET_BUNDLE_EXTRACT_BASE_DIR",
    "DOTNET_ROLL_FORWARD",
    "DOTNET_ROLL_FORWARD_TO_PRERELEASE",
    "DOTNET_ROLL_FORWARD_ON_NO_CANDIDATE_FX",
    "DOTNET_MULTILEVEL_LOOKUP",
    "DOTNET_RUNTIME_ID",
];
pub fn environment(prefix: &Path, runner: &Path) -> BTreeMap<String, String> {
    environment_from(prefix, runner, std::env::vars())
}
fn environment_from(
    prefix: &Path,
    runner: &Path,
    inherited: impl IntoIterator<Item = (String, String)>,
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<_, _> = inherited
        .into_iter()
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
                // Wine passes these names to case-insensitive Windows lookups.
                && !HOST_DOTNET_OVERRIDES
                    .iter()
                    .any(|name| key.eq_ignore_ascii_case(name))
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

#[cfg(test)]
mod environment_tests {
    use super::*;

    fn scoped(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        environment_from(
            Path::new("/managed/prefix"),
            Path::new("/managed/runner"),
            values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        )
    }

    #[test]
    fn windows_tools_ignore_inherited_dotnet_loading_overrides() {
        let inherited = [
            ("DOTNET_ROOT", "/usr/share/dotnet"),
            ("DOTNET_ROOT(x86)", "/linux/dotnet-x86"),
            ("DOTNET_ROOT_X86", "/linux/dotnet-x86"),
            ("DOTNET_ROOT_X64", "/linux/dotnet-x64"),
            ("DOTNET_ROOT_ARM64", "/linux/dotnet-arm64"),
            ("DOTNET_HOST_PATH", "/usr/bin/dotnet"),
            ("DOTNET_STARTUP_HOOKS", r"C:\flightdeck-repro\missing.dll"),
            ("dotnet_startup_hooks", "/linux/instrumentation.dll"),
            ("DotNet_Root_X64", "/linux/other-dotnet"),
            ("DOTNET_ADDITIONAL_DEPS", "/linux/deps.json"),
            ("DOTNET_SHARED_STORE", "/linux/shared-store"),
            ("DOTNET_SERVICING", "/linux/servicing"),
            ("DOTNET_BUNDLE_EXTRACT_BASE_DIR", "/linux/bundles"),
            ("DOTNET_ROLL_FORWARD", "Disable"),
            ("DOTNET_ROLL_FORWARD_TO_PRERELEASE", "1"),
            ("DOTNET_ROLL_FORWARD_ON_NO_CANDIDATE_FX", "0"),
            ("DOTNET_MULTILEVEL_LOOKUP", "0"),
            ("DOTNET_RUNTIME_ID", "linux-x64"),
        ];
        let env = scoped(&inherited);
        for (key, _) in inherited {
            assert!(!env.contains_key(key), "inherited {key} reached Windows");
        }
    }

    #[test]
    fn windows_tools_preserve_unrelated_graphics_media_and_runtime_settings() {
        let inherited = [
            ("__NV_PRIME_RENDER_OFFLOAD", "1"),
            ("__GL_SHADER_DISK_CACHE_PATH", "/graphics/cache"),
            ("VK_ICD_FILENAMES", "/graphics/driver.json"),
            ("GST_PLUGIN_PATH", "/media/plugins"),
            ("LD_LIBRARY_PATH", "/native/libraries"),
            ("DOTNET_EnableDiagnostics", "1"),
            ("DOTNET_GCHeapHardLimit", "40000000"),
            ("COMPlus_TieredCompilation", "0"),
            ("COREHOST_TRACE", "1"),
            ("COREHOST_TRACEFILE", r"C:\diagnostics\host.log"),
            ("DOTNET_ROOT_EXTRA", "app-specific-value"),
        ];
        let env = scoped(&inherited);
        for (key, value) in inherited {
            assert_eq!(env.get(key).map(String::as_str), Some(value), "{key}");
        }
    }

    #[test]
    fn windows_tools_keep_profile_and_compatibility_environment() {
        let env = scoped(&[
            ("WINEPREFIX", "/other/prefix"),
            ("WINELOADER", "/other/wine"),
            ("WINEDLLPATH", "/other/libraries"),
            ("DOTNET_SYSTEM_GLOBALIZATION_USENLS", "0"),
            ("DOTNET_ReadyToRun", "1"),
            ("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--example-option"),
        ]);
        assert_eq!(env["WINEPREFIX"], "/managed/prefix");
        assert!(!env.contains_key("WINELOADER"));
        assert!(!env.contains_key("WINEDLLPATH"));
        assert_eq!(env["DOTNET_SYSTEM_GLOBALIZATION_USENLS"], "1");
        assert_eq!(env["DOTNET_ReadyToRun"], "0");
        assert_eq!(
            env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"],
            "--example-option --disable-features=HideCursorWhileTyping"
        );
    }
}
