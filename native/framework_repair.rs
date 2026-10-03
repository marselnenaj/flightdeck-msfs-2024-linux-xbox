// SPDX-License-Identifier: MIT
//! Native .NET bootstrap/repair policy, shared with the tested staging workflow.
use crate::{Error, Result, error::require, framework};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub const PACKAGES: [(&str, &str, &str); 2] = [
    (
        "dotNetFx40_Full_x86_x64.exe",
        "https://download.microsoft.com/download/9/5/A/95A9616B-7A37-4AF6-BC36-D6EA96C8DAAE/dotNetFx40_Full_x86_x64.exe",
        "65e064258f2e418816b304f646ff9e87af101e4c9552ab064bb74d281c38659f",
    ),
    (
        "NDP48-x86-x64-AllOS-ENU.exe",
        "https://download.microsoft.com/download/f/3/a/f3a6af84-da23-40a5-8d1c-49cc10c8e76f/NDP48-x86-x64-AllOS-ENU.exe",
        "0a3a390c47e639d0f7fc65b21195fee6b7f65b066f80f70c60fab191d14b7e40",
    ),
];
pub trait SetupWine {
    fn prefix(&self) -> &Path;
    fn run(&mut self, args: &[String], installer: bool, timeout: Duration) -> Result<()>;
    fn reg(&mut self, key: &str, name: &str, value: &str, kind: &str) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn list(&mut self) -> Result<String>;
}
fn settings(wine: &mut impl SetupWine) -> Result<()> {
    wine.reg(
        r"HKCU\Software\Wine\DllOverrides",
        "mscoree",
        "native",
        "REG_SZ",
    )?;
    for key in [
        r"HKLM\Software\Microsoft\.NETFramework",
        r"HKLM\Software\Wow6432Node\Microsoft\.NETFramework",
    ] {
        wine.reg(key, "OnlyUseLatestCLR", "1", "REG_DWORD")?;
    }
    Ok(())
}
fn windows(wine: &mut impl SetupWine, version: &str) -> Result<()> {
    wine.reg(r"HKCU\Software\Wine", "Version", version, "REG_SZ")
}
fn probe(wine: &mut impl SetupWine) -> Result<bool> {
    if !framework::status(wine.prefix()).ready {
        return Ok(false);
    }
    for architecture in ["Framework", "Framework64"] {
        let Ok(compiler) = framework::framework_path(wine.prefix(), architecture, "csc.exe") else {
            return Ok(false);
        };
        match wine.run(
            &[
                compiler.to_string_lossy().into_owned(),
                "/nologo".into(),
                "/help".into(),
            ],
            false,
            Duration::from_secs(120),
        ) {
            Ok(()) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => return Ok(false),
        }
    }
    Ok(true)
}
fn restart(wine: &mut impl SetupWine) -> Result<()> {
    wine.run(
        &["wineboot".into(), "-r".into()],
        false,
        Duration::from_secs(180),
    )?;
    wine.stop()
}
fn newer(wine: &impl SetupWine) -> bool {
    let state = framework::status(wine.prefix());
    [state.x86, state.x64]
        .iter()
        .any(|v| v.release >= 533320 && v.clr)
}
fn installer(wine: &mut impl SetupWine, package: &Path, args: &[&str]) -> Result<()> {
    let mut command = vec![package.to_string_lossy().into_owned()];
    command.extend(args.iter().map(|v| (*v).into()));
    wine.run(&command, true, Duration::from_secs(1800))
}
fn install40(wine: &mut impl SetupWine, package: &Path) -> Result<()> {
    windows(wine, "winxp")?;
    installer(wine, package, &["/q", "/c:install.exe /q /norestart"])?;
    restart(wine)
}
const NEWER: &str =
    "A newer .NET Framework is installed; the pinned 4.8 package cannot repair it safely.";
pub fn prepare(
    wine: &mut impl SetupWine,
    mut obtain: impl FnMut(&str, &str, &str) -> Result<PathBuf>,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    if framework::status(wine.prefix()).ready {
        settings(wine)?;
    }
    if probe(wine)? {
        progress("Microsoft .NET Framework 4.8 is already installed.");
        return Ok(());
    }
    progress("Checking and completing pending Microsoft .NET setup …");
    restart(wine)?;
    if framework::status(wine.prefix()).ready {
        settings(wine)?;
    }
    if probe(wine)? {
        progress("Microsoft .NET Framework 4.8 is ready.");
        return Ok(());
    }
    let mut removed = false;
    for line in wine.list()?.lines() {
        if let Some((guid, title)) = line.split_once('|')
            && title.starts_with("Wine Mono")
            && guid.len() == 38
            && guid.starts_with('{')
            && guid.ends_with('}')
            && uuid::Uuid::parse_str(&guid[1..37]).is_ok()
        {
            wine.run(
                &[
                    "uninstaller".into(),
                    "--silent".into(),
                    "--remove".into(),
                    guid.into(),
                ],
                false,
                Duration::from_secs(1800),
            )?;
            removed = true;
        }
    }
    if removed {
        wine.stop()?;
    }
    require(!newer(wine), NEWER)?;
    let mut packages = Vec::new();
    for (name, url, hash) in PACKAGES {
        progress(if name.starts_with("dotNet") {
            "Downloading Microsoft .NET Framework 4.0 …"
        } else {
            "Downloading Microsoft .NET Framework 4.8 …"
        });
        packages.push(obtain(name, url, hash)?);
    }
    progress("Installing Microsoft .NET Framework. This can take several minutes …");
    let result = (|| {
        let initial = framework::status(wine.prefix());
        if !initial.x86.clr && !initial.x64.clr {
            install40(wine, &packages[0])?;
        }
        settings(wine)?;
        windows(wine, "win7")?;
        for attempt in 0..2 {
            let state = framework::status(wine.prefix());
            let repair = state.x86.release >= 528040 || state.x64.release >= 528040;
            if attempt > 0 || repair {
                progress("Repairing Microsoft .NET Framework 4.8 automatically …");
            }
            let attempt_result = (|| {
                if attempt > 0 {
                    require(!newer(wine), NEWER)?;
                    progress(
                        "Reinstalling Microsoft .NET in the copied profile to complete the repair …",
                    );
                    if repair {
                        installer(wine, &packages[1], &["/uninstall", "/q", "/norestart"])?;
                        restart(wine)?;
                    }
                    install40(wine, &packages[0])?;
                    settings(wine)?;
                    windows(wine, "win7")?;
                }
                installer(
                    wine,
                    &packages[1],
                    if repair && attempt == 0 {
                        &["/repair", "/q", "/norestart"]
                    } else {
                        &["/q", "/norestart"]
                    },
                )
            })();
            if matches!(attempt_result, Err(Error::Cancelled)) {
                return Err(Error::Cancelled);
            }
            if attempt_result.is_err() {
                wine.stop()?;
            }
            settings(wine)?;
            restart(wine)?;
            if probe(wine)? {
                progress(
                    "Microsoft .NET Framework 4.8 was verified for 32-bit and 64-bit applications.",
                );
                return Ok(());
            }
        }
        Err(Error::Invalid(
            "Microsoft .NET Framework 4.8 could not be repaired automatically. The original profile is unchanged. See the private Fenix setup log.",
        ))
    })();
    // Cancellation must still flush and stop the owned staging server. The real
    // Wine adapter ignores cancellation only for cleanup, not installation.
    let reset = windows(wine, "win10");
    let stopped = wine.stop();
    result.and(reset).and(stopped)
}
