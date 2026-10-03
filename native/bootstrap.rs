// SPDX-License-Identifier: MIT
//! Pinned compatibility downloads and fresh, isolated Wine environments.
use crate::{
    Error, Result,
    backend::{Context, string},
    components,
    error::require,
    files, process, resources, runtime, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
const MAX_DOWNLOAD: u64 = 2 * 1024 * 1024 * 1024;
pub fn artifact_names(spec: &Value) -> Result<Vec<&str>> {
    let names = spec["files"]
        .as_object()
        .ok_or(Error::Invalid("Ungültige Komponentenliste."))?;
    let mut required = components::FILES.to_vec();
    if !spec["features"]
        .as_array()
        .is_some_and(|v| v.iter().any(|v| v == "connected-storage-read-v1"))
    {
        required.retain(|v| *v != "bin/flightdeck-connected-storage.exe");
    }
    require(
        required.iter().all(|n| {
            names
                .get(*n)
                .and_then(Value::as_str)
                .is_some_and(files::hex_digest)
        }),
        "Für die Laufzeitkomponenten fehlen gültige Prüfsummen.",
    )?;
    Ok(required)
}
pub fn verify_native(root: &Path, spec: &Value) -> Result<()> {
    files::directory(root, false)?;
    for name in artifact_names(spec)? {
        let file = tx::owned_file(root, &root.join(name))?;
        require(
            !name.starts_with("bin/") || file.metadata()?.mode() & 0o100 != 0,
            "Die Laufzeitkomponenten sind nicht ausführbar.",
        )?;
        require(
            files::digest(file)? == spec["files"][name],
            "Die Prüfsumme der Laufzeitkomponenten stimmt nicht. Flightdeck bitte erneut installieren.",
        )?;
    }
    Ok(())
}
pub fn cache_dir() -> PathBuf {
    files::xdg("XDG_DATA_HOME", ".local/share").join("flightdeck/components")
}
pub fn cached_native(lock: &Value) -> Result<PathBuf> {
    let map: BTreeMap<String, String> = serde_json::from_value(lock["native"]["files"].clone())?;
    Ok(cache_dir().join(format!(
        "native-{}",
        files::sha256(&serde_json::to_vec(&map)?)
    )))
}
pub fn native_candidates(lock: &Value) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("resources/native"));
        candidates.push(parent.join("../resources/native"));
        candidates.push(parent.join("../share/flightdeck/native"));
    }
    if let Some(source) = resources::source_root() {
        for p in [
            "flightdeck/resources/native",
            "build/compat/artifacts",
            "build/compat-marketplace-run23/artifacts",
        ] {
            candidates.push(source.join(p));
        }
    }
    candidates.push(cached_native(lock)?);
    Ok(candidates)
}
pub fn native_path(lock: &Value) -> Result<Option<PathBuf>> {
    Ok(native_candidates(lock)?
        .into_iter()
        .find(|p| verify_native(p, &lock["native"]).is_ok()))
}
pub fn availability() -> Value {
    let ready = cfg!(all(target_os = "linux", target_arch = "x86_64"))
        && resources::json("compat/bootstrap.lock.json").is_ok_and(|lock| {
            // This is only the capability shown while the UI polls. A pinned
            // downloadable archive already makes setup available; obtaining
            // components still verifies every byte before using them.
            lock["native"]["archive_sha256"]
                .as_str()
                .is_some_and(files::hex_digest)
                || native_path(&lock).ok().flatten().is_some()
        });
    json!({"available":ready,"reason":if ready {""}else{"Die Beschreibung der Installationskomponenten fehlt. Flightdeck bitte erneut installieren."}})
}
pub fn download(
    url: &str,
    expected: &str,
    target: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Value),
) -> Result<PathBuf> {
    tx::interrupted(cancel)?;
    require(
        files::hex_digest(expected) && url.starts_with("https://"),
        "Für den Komponentendownload fehlt eine gültige Prüfsumme.",
    )?;
    if tx::digest(target).is_ok_and(|v| v == expected) {
        let size = target.metadata()?.len();
        progress(
            json!({"kind":"components","received_bytes":size,"verified_bytes":size,"total_bytes":size,"completed_files":null,"total_files":null}),
        );
        return Ok(target.into());
    }
    let parent = target
        .parent()
        .ok_or(Error::Invalid("Ungültiger Downloadpfad."))?;
    files::private_dir(parent)?;
    let temp = parent.join(format!(".download-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let client = reqwest::blocking::ClientBuilder::from(
            reqwest::Client::builder().read_timeout(Duration::from_secs(30)),
        )
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600))
        .user_agent(format!("Flightdeck/{}", crate::VERSION))
        .build()
        .map_err(|_| Error::Invalid("Der Komponentendownload konnte nicht vorbereitet werden."))?;
        let mut response=client.get(url).send().and_then(reqwest::blocking::Response::error_for_status).map_err(|_|Error::Invalid("Die Laufzeitkomponenten konnten nicht heruntergeladen werden. Verbindung prüfen oder das vollständige Flightdeck-Paket verwenden."))?;
        require(
            response.url().scheme() == "https",
            "Der Komponentendownload wurde auf eine unsichere Adresse umgeleitet.",
        )?;
        let mut total = response
            .content_length()
            .filter(|v| *v > 0 && *v <= MAX_DOWNLOAD);
        let mut output = File::from(rustix::fs::openat(
            rustix::fs::CWD,
            &temp,
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0o600),
        )?);
        let mut count = 0_u64;
        let mut buffer = [0_u8; 128 * 1024];
        let mut last = Instant::now();
        loop {
            tx::interrupted(cancel)?;
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            require(
                count <= MAX_DOWNLOAD,
                "Der Komponentendownload ist unerwartet groß.",
            )?;
            output.write_all(&buffer[..n])?;
            if total.is_some_and(|v| count > v) {
                total = None;
            }
            if last.elapsed() >= Duration::from_millis(500) && total != Some(count) {
                progress(
                    json!({"kind":"components","received_bytes":count,"verified_bytes":0,"total_bytes":total,"completed_files":null,"total_files":null}),
                );
                last = Instant::now();
            }
        }
        output.sync_all()?;
        drop(output);
        tx::interrupted(cancel)?;
        require(
            tx::digest(&temp)? == expected,
            "Die Prüfsumme des Downloads stimmt nicht. Die Datei wurde nicht ausgeführt.",
        )?;
        fs::rename(&temp, target)?;
        files::directory(parent, true)?.sync_all()?;
        progress(
            json!({"kind":"components","received_bytes":count,"verified_bytes":count,"total_bytes":total.filter(|v|*v==count),"completed_files":null,"total_files":null}),
        );
        Ok(target.to_path_buf())
    })();
    let _ = fs::remove_file(temp);
    result
}
fn archive_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|v| matches!(v, Component::Normal(_) | Component::CurDir))
}
fn safe_link(path: &Path, target: &Path, hard: bool) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth = if hard {
        0
    } else {
        path.parent()
            .map(|p| {
                p.components()
                    .filter(|p| matches!(p, Component::Normal(_)))
                    .count()
            })
            .unwrap_or(0)
    };
    for part in target.components() {
        match part {
            Component::ParentDir => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return false,
        }
    }
    true
}
pub fn extract_reader(reader: impl Read, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    files::directory(destination, true)?;
    let mut archive = tar::Archive::new(reader);
    let mut total = 0_u64;
    for (count, entry) in archive.entries()?.enumerate() {
        tx::interrupted(cancel)?;
        require(
            count < 30000,
            "Das Komponentenarchiv enthält zu viele Dateien.",
        )?;
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        require(
            archive_path(&path),
            "Das Komponentenarchiv enthält einen ungültigen Pfad.",
        )?;
        total = total
            .checked_add(entry.size())
            .ok_or(Error::Invalid("Das Komponentenarchiv ist zu groß."))?;
        require(
            total <= 6 * 1024 * 1024 * 1024,
            "Das Komponentenarchiv überschreitet die erwartete Größe.",
        )?;
        let kind = entry.header().entry_type();
        require(
            kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link(),
            "Das Komponentenarchiv enthält unzulässige Dateien.",
        )?;
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()?
                .ok_or(Error::Invalid("Ungültige Archivverknüpfung."))?;
            require(
                safe_link(&path, &target, kind.is_hard_link()),
                "Das Komponentenarchiv enthält eine unsichere Verknüpfung.",
            )?;
        }
        entry.set_mask(0o077);
        entry.set_preserve_permissions(false);
        entry.set_unpack_xattrs(false);
        require(
            entry.unpack_in(destination)?,
            "Das Komponentenarchiv enthält einen ungültigen Pfad.",
        )?;
    }
    Ok(())
}
pub fn extract(archive: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let file = files::open_at(rustix::fs::CWD, archive, false, false)?;
    match archive.extension().and_then(|v| v.to_str()) {
        Some("xz") => extract_reader(xz2::read::XzDecoder::new(file), destination, cancel),
        Some("gz") => extract_reader(flate2::read::GzDecoder::new(file), destination, cancel),
        _ => extract_reader(file, destination, cancel),
    }
}
pub fn unzip_runner(
    archive: &Path,
    target: &Path,
    expected: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    if tx::digest(target).is_ok_and(|v| v == expected) {
        return Ok(());
    }
    let mut archive = zip::ZipArchive::new(files::open_at(rustix::fs::CWD, archive, false, false)?)
        .map_err(|_| Error::Invalid("Das Runner-Archiv ist beschädigt."))?;
    require(
        archive.len() <= 30000,
        "Das Runner-Archiv enthält zu viele Dateien.",
    )?;
    let mut selected = None;
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|_| Error::Invalid("Das Runner-Archiv ist beschädigt."))?;
        if file.name().ends_with(".tar.xz") {
            require(
                selected.is_none() && file.size() <= 1024 * 1024 * 1024,
                "Das Runner-Archiv enthält keine eindeutige Installation.",
            )?;
            selected = Some(i);
        }
    }
    let i = selected.ok_or(Error::Invalid(
        "Das Runner-Archiv enthält keine eindeutige Installation.",
    ))?;
    let temp = target.with_file_name(format!(".runner-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut member = archive
            .by_index(i)
            .map_err(|_| Error::Invalid("Das Runner-Archiv ist beschädigt."))?;
        let mut out = File::from(rustix::fs::openat(
            rustix::fs::CWD,
            &temp,
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0o600),
        )?);
        let mut buffer = [0_u8; 128 * 1024];
        let mut count = 0_u64;
        loop {
            tx::interrupted(cancel)?;
            let n = member.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            require(
                count <= 1024 * 1024 * 1024,
                "Das Runner-Archiv ist zu groß.",
            )?;
            out.write_all(&buffer[..n])?;
        }
        out.sync_all()?;
        drop(out);
        require(
            tx::digest(&temp)? == expected,
            "Die Prüfsumme des Proton-Runners stimmt nicht.",
        )?;
        fs::rename(&temp, target)?;
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}
pub fn wine_command(runner: &Path, prefix: &Path) -> Command {
    let mut c = Command::new(runtime::wine(runner));
    for (key, _) in std::env::vars() {
        if ["WINE", "PROTON", "DXVK", "VKD3D", "XODUS"]
            .iter()
            .any(|prefix| key.starts_with(prefix))
        {
            c.env_remove(key);
        }
    }
    c.env("WINEPREFIX", prefix)
        .env("WINEARCH", "win64")
        .env("WINEESYNC", "0")
        .env("WINEFSYNC", "0")
        .env("WINEDEBUG", "-all")
        .env("WINE_DISABLE_FAST_SYNC", "1")
        .env("WINELOADER", runtime::wine(runner))
        .env("WINESERVER", runner.join("files/bin/wineserver"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}
pub fn command(c: &mut Command, cancel: &AtomicBool) -> Result<()> {
    require(
        process::run(c, Duration::from_secs(300), cancel)?.success(),
        "Die Linux-Spielumgebung konnte nicht eingerichtet werden. Bitte die Grafiktreiber und Wine-Abhängigkeiten prüfen.",
    )
}
pub fn wait_wine(runner: &Path, prefix: &Path, cancel: &AtomicBool) -> Result<()> {
    command(
        Command::new(runner.join("files/bin/wineserver"))
            .arg("-w")
            .env("WINEPREFIX", prefix)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        cancel,
    )
}
pub fn stop_wine(runner: &Path, prefix: &Path) -> Result<()> {
    let cancel = AtomicBool::new(false);
    for arg in ["-k", "-w"] {
        let mut c = Command::new(runner.join("files/bin/wineserver"));
        c.arg(arg)
            .env("WINEPREFIX", prefix)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let code = process::run(&mut c, Duration::from_secs(30), &cancel)?;
        require(
            code.success() || (arg == "-k" && code.code() == Some(1)),
            "Die private Wine-Umgebung konnte nicht beendet werden.",
        )?;
    }
    Ok(())
}
pub fn install_graphics(runner: &Path, prefix: &Path, cancel: &AtomicBool) -> Result<()> {
    tx::prefix_system32(prefix)?;
    for (arch, folder) in [("x86_64-windows", "system32"), ("i386-windows", "syswow64")] {
        let target = prefix.join("drive_c/windows").join(folder);
        files::directory(&target, false)?;
        for (library, names) in [
            ("dxvk", &["dxgi", "d3d11", "d3d10core"][..]),
            ("vkd3d-proton", &["d3d12", "d3d12core"][..]),
        ] {
            for name in names {
                let source = runner
                    .join("files/lib/wine")
                    .join(library)
                    .join(arch)
                    .join(format!("{name}.dll"))
                    .canonicalize()?;
                require(
                    source.starts_with(runner.canonicalize()?),
                    "Ungültige Grafikbibliothek im Proton-Runner.",
                )?;
                tx::copy_path(&source, &target.join(format!("{name}.dll")), None, cancel)?;
            }
        }
    }
    let registry = prefix
        .parent()
        .ok_or(Error::Invalid("Ungültiger Profilpfad."))?
        .join("graphics.reg");
    files::atomic(&registry,b"Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Wine\\DllOverrides]\n\"dxgi\"=\"native\"\n\"d3d11\"=\"native\"\n\"d3d10core\"=\"native\"\n\"d3d12\"=\"native\"\n\"d3d12core\"=\"native\"\n")?;
    command(
        wine_command(runner, prefix)
            .args(["regedit", "/S"])
            .arg(&registry),
        cancel,
    )
}
pub fn prepare_prefix(runner: &Path, prefix: &Path, cancel: &AtomicBool) -> Result<()> {
    require(
        !files::exists(prefix),
        "Die neue Spielumgebung existiert bereits und wird nicht überschrieben.",
    )?;
    let result: Result<()> = (|| {
        command(
            wine_command(runner, prefix).args(["wineboot", "-u"]),
            cancel,
        )?;
        wait_wine(runner, prefix, cancel)?;
        install_graphics(runner, prefix, cancel)?;
        wait_wine(runner, prefix, cancel)?;
        Ok(())
    })();
    stop_wine(runner, prefix)?;
    result?;
    for name in ["system.reg", "user.reg"] {
        files::read(&prefix.join(name), 64 * 1024 * 1024)?;
    }
    Ok(())
}
pub fn obtain(ctx: &Context, destination: &Path) -> Result<Value> {
    let lock = resources::json("compat/bootstrap.lock.json")?;
    let parent = destination
        .parent()
        .ok_or(Error::Invalid("Ungültiger Zielpfad."))?;
    let work = tx::new_directory(parent, ".flightdeck-install-")?;
    ctx.update(json!({"workspace_path":work}));
    let cache = cache_dir();
    files::private_dir(&cache)?;
    let lease = loop {
        ctx.interrupted()?;
        match files::Lease::acquire(&cache.join(".lock"), true) {
            Ok(v) => break v,
            Err(Error::Invalid("Das Spiel oder ein anderer Runtimevorgang läuft bereits.")) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e),
        }
    };
    let artifacts = work.join("artifacts");
    files::private_dir(&artifacts)?;
    ctx.progress("Linux-Komponenten werden vorbereitet …");
    if let Some(source) = native_path(&lock)? {
        for name in artifact_names(&lock["native"])? {
            tx::copy_path(
                &source.join(name),
                &artifacts.join(name),
                lock["native"]["files"][name].as_str(),
                &ctx.cancel,
            )?;
        }
    } else {
        let archive = download(
            string(&lock["native"], "archive_url")?,
            string(&lock["native"], "archive_sha256")?,
            &cache.join("native.tar.gz"),
            &ctx.cancel,
            |v| ctx.update(json!({"transfer":v})),
        )?;
        extract(&archive, &artifacts, &ctx.cancel)?;
    }
    verify_native(&artifacts, &lock["native"])?;
    let cached = cached_native(&lock)?;
    if !files::exists(&cached) {
        let stage = tx::new_directory(&cache, ".native-")?;
        let result = (|| {
            for name in artifact_names(&lock["native"])? {
                tx::copy_path(
                    &artifacts.join(name),
                    &stage.join(name),
                    lock["native"]["files"][name].as_str(),
                    &ctx.cancel,
                )?;
            }
            verify_native(&stage, &lock["native"])?;
            tx::publish(&stage, &cached)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result?;
    }
    files::atomic_json(
        &artifacts.join("manifest.json"),
        &json!({"format":1,"files":lock["native"]["files"],"features":lock["native"]["features"],"cli_features":lock["native"]["cli_features"]}),
    )?;
    ctx.progress("Der geprüfte Proton-Runner wird heruntergeladen …");
    let runner_lock = &lock["runner"];
    let zip = download(
        string(runner_lock, "url")?,
        string(runner_lock, "zip_sha256")?,
        &cache.join("runner.zip"),
        &ctx.cancel,
        |v| ctx.update(json!({"transfer":v})),
    )?;
    let archive = cache.join("runner.tar.xz");
    unzip_runner(
        &zip,
        &archive,
        string(runner_lock, "archive_sha256")?,
        &ctx.cancel,
    )?;
    ctx.update(json!({"transfer":null}));
    ctx.progress("Proton wird entpackt und die Grafik eingerichtet …");
    extract(&archive, &work, &ctx.cancel)?;
    let name = string(runner_lock, "directory")?;
    require(
        files::relative(name) && !name.contains('/'),
        "Ungültiger Proton-Runnerpfad.",
    )?;
    let runner = work.join(name);
    require(
        tx::digest(&runner.join("files/lib/wine/x86_64-windows/xgameruntime.dll"))?
            == runner_lock["original_runtime_sha256"],
        "Der entpackte Proton-Runner passt nicht zur geprüften Runtime.",
    )?;
    drop(lease);
    let prefix = work.join("prefix");
    prepare_prefix(&runner, &prefix, &ctx.cancel)?;
    Ok(
        json!({"cli":artifacts.join("bin/xodus-cli"),"cli_sha256":lock["native"]["files"]["bin/xodus-cli"],"cli_features":lock["native"]["cli_features"],"artifacts_path":artifacts,"runner_path":runner,"prefix_path":prefix,"workspace":work}),
    )
}
