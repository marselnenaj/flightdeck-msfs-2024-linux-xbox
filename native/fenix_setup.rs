// SPDX-License-Identifier: MIT
//! Fenix dependencies and settings, preserving the official aircraft installation.
use crate::{
    Error, Result,
    backend::Context,
    bootstrap,
    error::require,
    fenix_bundle as bundle, files, transaction as tx,
    wine::{StagedWine, Wine},
    xml::{self, Element, Item},
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
pub const PROGRAM: &str = "drive_c/Program Files/FenixSim A320";
pub const CONFIG: &str = "drive_c/ProgramData/Fenix/FenixSim A320";
pub const GEOMETRY_HASH: &str = "663f1d59ec1c014b9ea47a6cef71b3d43579e9759a82f3ee2cbd80b8c6d9e85f";
pub const GEOMETRY_PATH: &str = "drive_c/windows/system32/d2d1_geometry.dll";
pub fn geometry(wine: &StagedWine<'_>, cache: &Path, payload: &Path, ctx: &Context) -> Result<()> {
    wine.check()?;
    let target = bundle::contained(&wine.wine.prefix, GEOMETRY_PATH)?;
    if tx::digest(&target).is_ok_and(|h| h == GEOMETRY_HASH) {
        return Ok(());
    }
    let sources = [
        (
            "Windows6.1-KB2670838-x64.msu",
            "https://download.microsoft.com/download/1/4/9/14936FE9-4D16-4019-A093-5E00182609EB/Windows6.1-KB2670838-x64.msu",
            "9fe71e7dcd2280ce323880b075ade6e56c49b68fc702a9b4c0a635f0f1fb9db8",
        ),
        (
            "msdelta.dll",
            "https://msdl.microsoft.com/download/symbols/msdelta.dll/559F38C482000/msdelta.dll",
            "29c10fb3ffa0e3cfd04c5247c3ed3a975575fad44d8a1e447b23b43213781653",
        ),
    ];
    ctx.progress("Preparing Direct2D geometry for Fenix route rendering …");
    let mut packages = Vec::new();
    for (name, url, hash) in sources {
        packages.push(bootstrap::download(
            url,
            hash,
            &cache.join(name),
            &ctx.cancel,
            |_| {},
        )?);
    }
    let helper = payload.join("integration/FenixGeometrySetup.exe");
    require(
        tx::digest(&helper)? == bundle::manifest(None)?["integration"]["FenixGeometrySetup.exe"],
        "Fenix geometry helper checksum mismatch.",
    )?;
    let work = tx::new_directory(cache, ".fenix-geometry-")?;
    let result = (|| {
        let windows = |p: &Path| format!("Z:{}", p.to_string_lossy().replace('/', "\\"));
        let run = |args: Vec<String>, seconds| -> Result<()> {
            wine.check()?;
            let mut cmd = vec![helper.to_string_lossy().into_owned()];
            cmd.extend(args);
            wine.wine
                .run(&cmd, None, &[0], Duration::from_secs(seconds))?;
            Ok(())
        };
        let check = |path: &Path, hash: &str| {
            require(
                tx::digest(path)? == hash,
                "Geometry dependency checksum mismatch.",
            )
        };
        let cabinet = work.join("update.cab");
        run(
            vec![
                "extract".into(),
                windows(&packages[0]),
                "Windows6.1-KB2670838-x64.cab".into(),
                windows(&cabinet),
            ],
            180,
        )?;
        check(
            &cabinet,
            "470b9ab769a46bbd1acc5e352156e2173a57cbbff03ff6f950d0989d9a150fbc",
        )?;
        for (member, hash) in [
            (
                "0",
                "e74c3bf4727f145ad81d9f1c2674cf6f8bab20acccee9d9c4cf2c375c75aed20",
            ),
            (
                "1",
                "1cc95120454739d69a08fddd7e0e0f125e76115c4eba0748db94c7753c84788c",
            ),
        ] {
            run(
                vec![
                    "extract".into(),
                    windows(&cabinet),
                    member.into(),
                    windows(&work.join(member)),
                ],
                180,
            )?;
            check(&work.join(member), hash)?;
        }
        let basis = work.join("basis.dll");
        let output = work.join("geometry.dll");
        run(
            vec![
                "delta".into(),
                windows(&packages[1]),
                "-".into(),
                windows(&work.join("0")),
                windows(&basis),
            ],
            60,
        )?;
        check(
            &basis,
            "82dc3ddb8c3441ef2de0eb43ed187bc56b747e422a2f240cd33c978356746a7d",
        )?;
        run(
            vec![
                "delta".into(),
                windows(&packages[1]),
                windows(&basis),
                windows(&work.join("1")),
                windows(&output),
            ],
            60,
        )?;
        check(&output, GEOMETRY_HASH)?;
        bundle::write(&target, &files::read(&output, 32 * 1024 * 1024)?, 0o644)
    })();
    let _ = fs::remove_dir_all(&work);
    result
}
pub fn ui_fonts(wine: &Wine<'_>) -> Result<()> {
    let registry = String::from_utf8_lossy(&files::read(
        &wine.prefix.join("system.reg"),
        64 * 1024 * 1024,
    )?)
    .into_owned();
    let section = registry
        .split(r"[Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts]")
        .nth(1)
        .and_then(|v| v.split('[').next())
        .unwrap_or("");
    for (name, family) in [("tahoma.ttf", "Tahoma"), ("tahomabd.ttf", "Tahoma Bold")] {
        let value = format!("{family} (TrueType)");
        let needle = format!("\"{value}\"=\"");
        if section
            .lines()
            .any(|v| v.starts_with(&needle) && v.len() > needle.len() + 1)
        {
            continue;
        }
        let target = bundle::contained(&wine.prefix, &format!("drive_c/windows/Fonts/{name}"))?;
        if !files::exists(&target) {
            let source = wine.runner.join("files/share/wine/fonts").join(name);
            bundle::write(
                &target,
                &files::read_public(&source, 32 * 1024 * 1024)?,
                0o644,
            )?;
        }
        files::open_at(rustix::fs::CWD, &target, false, false)?;
        wine.run(
            &[
                "reg",
                "add",
                r"HKLM\Software\Microsoft\Windows NT\CurrentVersion\Fonts",
                "/v",
                &value,
                "/t",
                "REG_SZ",
                "/d",
                name,
                "/f",
                "/reg:64",
            ],
            None,
            &[0],
            Duration::from_secs(180),
        )?;
    }
    Ok(())
}
pub fn graphics_fonts(wine: &StagedWine<'_>) -> Result<()> {
    wine.check()?;
    let wine = &wine.wine;
    for (arch, folder) in [("x86_64", "system32"), ("i386", "syswow64")] {
        for name in [
            "libvkd3d-1.dll",
            "libvkd3d-shader-1.dll",
            "libvkd3d-utils-1.dll",
        ] {
            bundle::write(
                &bundle::contained(&wine.prefix, &format!("drive_c/windows/{folder}/{name}"))?,
                &files::read_public(
                    &wine
                        .runner
                        .join(format!("files/lib/vkd3d/{arch}-windows/{name}")),
                    64 * 1024 * 1024,
                )?,
                0o644,
            )?;
        }
    }
    for (name, family) in [
        ("arial.ttf", "Arial"),
        ("arialbd.ttf", "Arial Bold"),
        ("cour.ttf", "Courier New"),
        ("courbd.ttf", "Courier New Bold"),
        ("georgia.ttf", "Georgia"),
        ("times.ttf", "Times New Roman"),
        ("micross.ttf", "Microsoft Sans Serif"),
    ] {
        bundle::write(
            &bundle::contained(&wine.prefix, &format!("drive_c/windows/Fonts/{name}"))?,
            &files::read_public(
                &wine.runner.join("files/share/fonts").join(name),
                32 * 1024 * 1024,
            )?,
            0o644,
        )?;
        wine.reg(
            r"HKLM\Software\Microsoft\Windows NT\CurrentVersion\Fonts",
            &format!("{family} (TrueType)"),
            name,
            "REG_SZ",
        )?;
    }
    ui_fonts(wine)?;
    wine.reg(
        r"HKCU\Software\Microsoft\Avalon.Graphics",
        "DisableHWAcceleration",
        "1",
        "REG_DWORD",
    )?;
    wine.reg(
        r"HKCU\Software\Wine\Explorer",
        "ShowSystray",
        "0",
        "REG_DWORD",
    )
}
pub fn settings_file(path: &Path, changes: &[(&str, &str)]) -> Result<bool> {
    if !files::exists(path) {
        return Ok(false);
    }
    let mut root = xml::parse(&files::read(path, 2 * 1024 * 1024)?)?;
    for (name, value) in changes {
        root.set(name, value);
    }
    files::atomic(path, &root.bytes()?)?;
    Ok(true)
}
pub fn user_folders(prefix: &Path) -> Result<Vec<PathBuf>> {
    let users = bundle::contained(prefix, "drive_c/users")?;
    let mut found = Vec::new();
    for (n, entry) in fs::read_dir(&users)?.enumerate() {
        require(n < 256, "Too many Windows user profiles.")?;
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if ["Public", "Default", "Default User"].contains(&name) {
            continue;
        }
        let dir = bundle::contained(prefix, &format!("drive_c/users/{name}"))?;
        if dir.is_dir() {
            require(
                dir.canonicalize()?.starts_with(prefix.canonicalize()?),
                "The Windows user directory escapes the profile.",
            )?;
            found.push(dir);
        }
    }
    Ok(found)
}
pub fn configure_prefix(prefix: &Path) -> Result<bool> {
    if !prefix.join(PROGRAM).join("Fenix.exe").is_file() {
        return Ok(false);
    }
    let settings = bundle::contained(prefix, &format!("{CONFIG}/fenixConfig.xml"))?;
    let persisted = bundle::contained(prefix, &format!("{CONFIG}/persistancy.xml"))?;
    let ready = settings_file(
        &settings,
        &[
            ("displayMode", "CPU"),
            ("newRender", "false"),
            ("preferCPU", "true"),
            ("multithread", "true"),
        ],
    )?;
    let legacy = settings_file(&persisted, &[("fcuReadoutsType", "0")])?;
    let mut candidates = Vec::new();
    for user in user_folders(prefix)? {
        let path = user.join("AppData/Roaming/Microsoft Flight Simulator 2024");
        if path.is_dir() {
            require(
                path.canonicalize()?.starts_with(prefix.canonicalize()?),
                "The simulator settings folder escapes the profile.",
            )?;
            candidates.push(path);
        }
    }
    require(
        candidates.len() == 1,
        "Could not identify one MSFS 2024 settings folder. Run the simulator once first.",
    )?;
    let path = candidates[0].join("exe.xml");
    let mut root = if files::exists(&path) {
        xml::parse(&files::read(&path, 2 * 1024 * 1024)?)?
    } else {
        let mut root = Element::new("SimBase.Document");
        root.start.push_attribute(("Type", "Launch"));
        root.start.push_attribute(("version", "1,0"));
        for (key, value) in [
            ("Descr", "Launch"),
            ("Filename", "exe.xml"),
            ("Disabled", "False"),
        ] {
            root.set(key, value);
        }
        root
    };
    let fenix = |v: &Element| {
        v.named("Launch.Addon")
            && ["Name", "Path"]
                .iter()
                .filter_map(|n| v.child(n))
                .any(|v| v.text().to_lowercase().contains("fenix"))
    };
    let mut found = false;
    root.items.retain(|v| {
        if let Item::Element(v) = v
            && fenix(v)
        {
            if found {
                return false;
            }
            found = true;
        }
        true
    });
    if !found {
        let mut node = Element::new("Launch.Addon");
        node.set("Name", "FenixA320");
        root.items.push(Item::Element(node));
    }
    let entry = root
        .items
        .iter_mut()
        .find_map(|v| match v {
            Item::Element(v) if fenix(v) => Some(v),
            _ => None,
        })
        .ok_or(Error::Invalid("Invalid Fenix autostart entry."))?;
    for (key, value) in [
        ("Name", "FenixA320"),
        ("Disabled", "False"),
        ("ManualLoad", "False"),
        (
            "Path",
            r"C:\Program Files\FenixSim A320\deps\FenixBootstrapper.exe",
        ),
    ] {
        entry.set(key, value);
    }
    files::atomic(&path, &root.bytes()?)?;
    Ok(ready && legacy)
}
pub fn manager(prefix: &Path) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    for user in user_folders(prefix)? {
        let path = user.join("AppData/Local/FenixApp/current/FenixApp.exe");
        if path.is_file() && path.canonicalize()?.starts_with(prefix.canonicalize()?) {
            files::open_at(rustix::fs::CWD, &path, false, false)?;
            candidates.push(path);
        }
    }
    require(
        candidates.len() == 1,
        "Install the official Fenix Installer first.",
    )?;
    Ok(candidates.remove(0))
}
