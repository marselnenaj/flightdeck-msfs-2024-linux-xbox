// SPDX-License-Identifier: MIT
//! Official portable MobiFlight in the selected simulator's existing profile.
//! HID support is narrowly pinned; device permissions are diagnosed, never changed here.
mod winebus;
use crate::{
    Error, Result,
    addon_apps::{self, Profile},
    backend::{Context, Launcher, string},
    bootstrap,
    error::require,
    fenix, fenix_setup, files, hidsharp_patch, navigraph, owned_tree, runtime, transaction as tx,
    wine_processes,
    xml::{self, Element, Item},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Seek, Write},
    os::unix::fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const PROGRAM: &str = "drive_c/Flightdeck MobiFlight";
const EXE: &str = "MFConnector.exe";
const RECEIPT: &str = ".flightdeck-mobiflight.json";
const JOURNAL: &str = "private/mobiflight-install.json";
const MODULE: &str = "mobiflight-event-module";
const EVENTS_LIMIT: usize = 4 * 1024 * 1024;
const EVENTS_ERROR: &str = "Die offiziellen MobiFlight-HubHop-Ereignisse konnten nicht vollständig geladen werden. Vorhandene Ereignisse bleiben unverändert; Vorbereitung erneut versuchen.";
pub const STARTUP: &str = "private/mobiflight-startup.json";
pub const PROCESSES: &[&str] = &["mfconnector.exe"];
pub const WINWING_UDEV_RULE: &str = "# WinWing HID access for the active local desktop session.\nSUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"4098\", TAG+=\"uaccess\"\n";
const MODULE_FILES: &[(&str, &str)] = &[
    (
        "ContentInfo/mobiflight-event-module/Thumbnail.jpg",
        "4a017d316e66b46a12ae21d5bfb4c5ea78899479a20553eb5f77799e88c702dd",
    ),
    (
        "layout.json",
        "73c7d2e67fa777b717a6dd638bfc5b30e6735bff6d1be52cb96fe016ab143724",
    ),
    (
        "manifest.json",
        "a17e557a0bc22df64276fc57bfc9db71d3c019c727c0384dd7cb3a9259d3ac1b",
    ),
    (
        "modules/events.user.txt.inactive",
        "c6b544d3c50b0abc87ad6b3e61aec9fe533cf1d0b0604dd94272048f9383db22",
    ),
    (
        "modules/MobiFlightWasmModule.wasm",
        "3b0bae23d0f12e6504c15b29b524aaf9f50dbac391eb70d921e5525e1b3f7416",
    ),
];
fn lock() -> Result<Value> {
    Ok(serde_json::from_str(include_str!(
        "../compat/mobiflight.lock.json"
    ))?)
}
fn checked_directory(path: &Path, prefix: &Path) -> Result<()> {
    files::directory(path, false)?;
    require(
        path.canonicalize()?.starts_with(prefix.canonicalize()?),
        "Der MobiFlight-Ordner muss im ausgewählten Windows-Profil liegen.",
    )
}
pub fn manager(prefix: &Path) -> Result<Option<PathBuf>> {
    let mut candidates = vec![prefix.join(PROGRAM)];
    for base in [
        "drive_c/Program Files",
        "drive_c/Program Files (x86)",
        "drive_c",
    ] {
        for name in ["MobiFlight", "MobiFlight Connector", "MobiFlightConnector"] {
            candidates.push(prefix.join(base).join(name));
        }
    }
    if prefix.join("drive_c/users").is_dir() {
        for user in fenix_setup::user_folders(prefix)? {
            for name in [
                "MobiFlight",
                "MobiFlight/MobiFlight Connector",
                "MobiFlight Connector",
                "Programs/MobiFlight",
                "Programs/MobiFlight Connector",
            ] {
                candidates.push(user.join("AppData/Local").join(name));
            }
        }
    }
    let mut found = BTreeSet::new();
    for path in candidates {
        if files::exists(&path.join(EXE)) {
            checked_directory(&path, prefix)?;
            tx::owned_file(&path, &path.join(EXE))?;
            found.insert(path.canonicalize()?);
        }
    }
    require(
        found.len() <= 1,
        "MobiFlight wurde mehrfach in diesem Windows-Profil gefunden. Die vorhandenen Installationen bleiben unverändert.",
    )?;
    Ok(found.into_iter().next())
}
fn windows_path(path: &Path, prefix: &Path) -> Result<String> {
    let drive = prefix.join("drive_c").canonicalize()?;
    let real = path.canonicalize()?;
    let relative = real
        .strip_prefix(drive)
        .map_err(|_| Error::Invalid("Ungültiger MobiFlight-Programmpfad."))?;
    Ok(format!(
        "C:\\{}",
        relative.to_string_lossy().replace('/', "\\")
    ))
}
fn community(profile: &Profile) -> Result<PathBuf> {
    let (locations, limited) = profile.locations()?;
    let folders = locations
        .into_iter()
        .map(|(p, _)| p.canonicalize().map_err(Error::from))
        .collect::<Result<BTreeSet<_>>>()?;
    require(
        !limited && folders.len() == 1,
        "Der tatsächliche Community-Ordner ist noch nicht eindeutig verfügbar. Starte MSFS einmal und lade den Status neu.",
    )?;
    let path = folders
        .into_iter()
        .next()
        .ok_or(Error::Invalid("Der Community-Ordner fehlt."))?;
    files::directory(&path, false)?;
    Ok(path)
}
fn wasm_state(profile: &Profile) -> (bool, bool) {
    community(profile)
        .ok()
        .map(|p| p.join(MODULE))
        .map(|p| {
            (
                p.join("modules/MobiFlightWasmModule.wasm").is_file(),
                p.join("modules/events.txt").is_file(),
            )
        })
        .unwrap_or((false, false))
}
fn stage_path(parent: &Path, id: &str) -> Result<PathBuf> {
    require(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Ungültige MobiFlight-Einrichtungssicherung.",
    )?;
    Ok(parent.join(format!(".flightdeck-mobiflight-stage-{id}")))
}
fn recover(profile: &Profile) -> Result<()> {
    let journal = profile.root.join(JOURNAL);
    if !files::exists(&journal) {
        return Ok(());
    }
    let record = runtime::value(&journal)?;
    require(
        record["format"] == 1,
        "Ungültige MobiFlight-Einrichtungssicherung.",
    )?;
    let previous_prefix: [u64; 2] = serde_json::from_value(record["prefix_id"].clone())?;
    if json!(previous_prefix) != fenix::identity(&profile.prefix)? {
        // Fenix/Proton can replace a profile after an interrupted add-on setup.
        // Its journal cannot authorize deletion in the replacement profile.
        // Preserve the journal and every old/copied stage, then start afresh
        // using only the currently selected profile's identity and new paths.
        tx::owned_file(&profile.root, &journal)?;
        tx::publish(
            &journal,
            &profile.root.join("private").join(format!(
                "mobiflight-install-before-profile-change-{}.json",
                uuid::Uuid::new_v4().simple()
            )),
        )?;
        return Ok(());
    }
    let parent = match record["kind"].as_str() {
        Some("program") => profile.prefix.join("drive_c"),
        Some("module") => community(profile)?,
        _ => {
            return Err(Error::Invalid(
                "Ungültige MobiFlight-Einrichtungssicherung.",
            ));
        }
    };
    require(
        record["parent_id"] == json!(owned_tree::identity(&parent)?),
        "Der MobiFlight-Zielordner wurde verändert. Die Sicherung bleibt erhalten.",
    )?;
    let stage = stage_path(&parent, string(&record, "id")?)?;
    if files::exists(&stage) {
        require(
            record["stage_id"] == json!(owned_tree::identity(&stage)?)
                || (record["stage_id"].is_null() && fs::read_dir(&stage)?.next().is_none()),
            "Die MobiFlight-Zwischenablage wurde verändert.",
        )?;
        owned_tree::remove(&stage, owned_tree::identity(&stage)?)?;
    }
    fs::remove_file(journal)?;
    Ok(())
}
fn new_stage(profile: &Profile, parent: &Path, kind: &str) -> Result<PathBuf> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let stage = stage_path(parent, &id)?;
    // Write intent first; then update with the newly created directory identity.
    files::atomic_json(
        &profile.root.join(JOURNAL),
        &json!({"format":1,"kind":kind,"id":id,"prefix_id":fenix::identity(&profile.prefix)?,"parent_id":owned_tree::identity(parent)?,"stage_id":null}),
    )?;
    fs::DirBuilder::new().mode(0o700).create(&stage)?;
    files::atomic_json(
        &profile.root.join(JOURNAL),
        &json!({"format":1,"kind":kind,"id":id,"prefix_id":fenix::identity(&profile.prefix)?,"parent_id":owned_tree::identity(parent)?,"stage_id":owned_tree::identity(&stage)?}),
    )?;
    Ok(stage)
}
/// Strict ZIP extractor used only after the official archive's hash and length match.
fn extract_zip<R: Read + Seek>(source: R, target: &Path, cancel: &AtomicBool) -> Result<()> {
    let invalid = "Ungültiges oder unerwartetes MobiFlight-Programmpaket.";
    let mut archive = zip::ZipArchive::new(source).map_err(|_| Error::Invalid(invalid))?;
    require(archive.len() <= 2000, invalid)?;
    archive
        .validate_header_consistency()
        .map_err(|_| Error::Invalid(invalid))?;
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    for i in 0..archive.len() {
        tx::interrupted(cancel)?;
        let mut entry = archive.by_index(i).map_err(|_| Error::Invalid(invalid))?;
        let archive_name = entry.name().map_err(|_| Error::Invalid(invalid))?;
        let name = archive_name.trim_end_matches('/');
        require(
            !name.is_empty() && !name.contains(['\\', ':', '\0']) && name.len() <= 512,
            invalid,
        )?;
        let relative = Path::new(name);
        require(
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
                && !name
                    .split('/')
                    .any(|p| p.is_empty() || p.ends_with(['.', ' '])),
            invalid,
        )?;
        require(seen.insert(name.to_lowercase()), invalid)?;
        let mode = entry
            .unix_mode()
            .unwrap_or(if entry.is_dir() { 0o040700 } else { 0o100600 });
        require(
            mode & 0o170000 == 0
                || mode & 0o170000 == if entry.is_dir() { 0o040000 } else { 0o100000 },
            invalid,
        )?;
        total = total
            .checked_add(entry.size())
            .ok_or(Error::Invalid(invalid))?;
        require(
            total <= 300 * 1024 * 1024 && entry.size() <= 64 * 1024 * 1024,
            invalid,
        )?;
        let output = target.join(relative);
        if entry.is_dir() {
            files::private_dir(&output)?;
            continue;
        }
        files::private_dir(output.parent().ok_or(Error::Invalid(invalid))?)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&output)?;
        let expected = entry.size();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            tx::interrupted(cancel)?;
            let count = entry
                .read(&mut buffer)
                .map_err(|_| Error::Invalid(invalid))?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            require(copied <= expected, invalid)?;
            file.write_all(&buffer[..count])?;
        }
        require(copied == expected, invalid)?;
        file.sync_all()?;
    }
    Ok(())
}
fn wasm_events(cancel: &AtomicBool) -> Result<Vec<u8>> {
    let invalid = EVENTS_ERROR;
    tx::interrupted(cancel)?;
    // Use HubHop's current endpoint directly. Its former /v1/export alias
    // now returns 301; redirects remain disabled for this text-data source.
    let spec = lock()?;
    let mut response = crate::http_client::builder(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::Invalid(invalid))?
        .get(string(&spec, "events_url")?)
        .send()
        .map_err(|_| Error::Invalid(invalid))?;
    require(
        response.status().is_success()
            && response
                .content_length()
                .is_none_or(|n| n <= EVENTS_LIMIT as u64),
        invalid,
    )?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 32768];
    loop {
        tx::interrupted(cancel)?;
        let count = response
            .read(&mut buffer)
            .map_err(|_| Error::Invalid(invalid))?;
        if count == 0 {
            break;
        }
        require(bytes.len() + count <= EVENTS_LIMIT, invalid)?;
        bytes.extend_from_slice(&buffer[..count]);
    }
    validate_wasm_events(&bytes)?;
    Ok(bytes)
}
fn validate_wasm_events(bytes: &[u8]) -> Result<()> {
    // HubHop includes valid event scripts longer than 128 KiB. The entire
    // response is bounded instead of imposing a second, incompatible line cap.
    require(bytes.len() <= EVENTS_LIMIT, EVENTS_ERROR)?;
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Invalid(EVENTS_ERROR))?;
    require(
        !text.contains('\0')
            && text.trim_start().starts_with("//")
            && text
                .lines()
                .filter(|l| !l.starts_with("//") && l.contains('#'))
                .count()
                >= 100,
        EVENTS_ERROR,
    )
}
fn missing_events(target: &Path, cancel: &AtomicBool) -> Result<()> {
    let path = target.join("modules/events.txt");
    if files::exists(&path) {
        return Ok(());
    }
    files::directory(target, false)?;
    require(
        files::digest(tx::owned_file(
            target,
            &target.join("modules/MobiFlightWasmModule.wasm"),
        )?)? == MODULE_FILES[4].1,
        "Vorhandenes MobiFlight-Modul ist unbekannt. Eigene Events bleiben unverändert.",
    )?;
    let bytes = wasm_events(cancel)?;
    // NOREPLACE keeps any events.txt created meanwhile by another application.
    let temporary = target.join("modules").join(format!(
        ".flightdeck-events-{}",
        uuid::Uuid::new_v4().simple()
    ));
    files::atomic(&temporary, &bytes)?;
    let result = tx::publish(&temporary, &path);
    if files::exists(&temporary) {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn prepare_wasm(profile: &Profile, program: &Path, cancel: &AtomicBool) -> Result<()> {
    let parent = community(profile)?;
    let target = parent.join(MODULE);
    // Preserve an existing module and all its event/user files. Only the exact
    // known, real module directory may receive a missing official events.txt.
    if files::exists(&target) {
        if fs::symlink_metadata(&target)?.is_dir()
            && !files::exists(&target.join("modules/events.txt"))
            && tx::digest(&target.join("modules/MobiFlightWasmModule.wasm"))
                .is_ok_and(|v| v == MODULE_FILES[4].1)
        {
            missing_events(&target, cancel)?;
        }
        return Ok(());
    }
    let source = program.join("MSFS2020-module").join(MODULE);
    for (name, hash) in MODULE_FILES {
        require(
            files::digest(tx::owned_file(program, &source.join(name))?)? == *hash,
            "Das gebündelte MobiFlight-WASM-Modul ist unbekannt oder unvollständig. Vorhandene Community-Dateien bleiben unverändert.",
        )?;
    }
    let stage = new_stage(profile, &parent, "module")?;
    for (name, hash) in MODULE_FILES {
        tx::copy(
            tx::owned_file(program, &source.join(name))?,
            &stage.join(name),
            Some(hash),
            cancel,
        )?;
    }
    missing_events(&stage, cancel)?;
    let layout_path = stage.join("layout.json");
    let mut layout: Value = serde_json::from_slice(&files::read(&layout_path, 65536)?)?;
    if let Some(content) = layout["content"].as_array_mut() {
        for entry in content
            .iter_mut()
            .filter(|e| e["path"] == "modules/events.txt")
        {
            entry["size"] = json!(fs::metadata(stage.join("modules/events.txt"))?.len());
        }
    }
    files::atomic_json(&layout_path, &layout)?;
    tx::interrupted(cancel)?;
    tx::publish(&stage, &target)?;
    fs::remove_file(profile.root.join(JOURNAL))?;
    Ok(())
}
fn prepare(profile: &Profile, ctx: &Context) -> Result<()> {
    recover(profile)?;
    let program = manager(&profile.prefix)?.ok_or(Error::Invalid(
        "Installiere zuerst MobiFlight in diesem Windows-Profil.",
    ))?;
    hidsharp_patch::apply(
        &program,
        &profile.root.join("private/mobiflight-hid-backup"),
        &ctx.cancel,
    )?;
    finish_preparation(profile, &program, ctx)
}
fn finish_preparation(profile: &Profile, program: &Path, ctx: &Context) -> Result<()> {
    ctx.update(json!({"phase":"wasm","message":"Die MobiFlight-App ist vorhanden. WASM-Modul und fehlende HubHop-Ereignisse werden vorbereitet …"}));
    prepare_wasm(profile, program, &ctx.cancel)?;
    if winebus::snapshot(profile)["state"] == "required" {
        ctx.update(json!({"phase":"hid"}));
        winebus::configure(ctx, profile)?;
    }
    Ok(())
}
fn install(ctx: &Context, profile: &Profile) -> Result<()> {
    recover(profile)?;
    if manager(&profile.prefix)?.is_some() {
        return prepare(profile, ctx);
    }
    // Fail before downloading or publishing if the release omitted its verified payload.
    hidsharp_patch::payload()?;
    let spec = lock()?;
    let archive = ctx
        .launcher
        .state_dir
        .join("addon-downloads/mobiflight")
        .join(format!("{}.zip", string(&spec, "version")?));
    require(
        tx::free_bytes(&profile.prefix)? >= 512 * 1024 * 1024,
        "Für MobiFlight fehlen mindestens 512 MiB freier Speicherplatz.",
    )?;
    ctx.progress("Das offizielle portable MobiFlight-Paket wird heruntergeladen und geprüft …");
    let archive = bootstrap::download(
        string(&spec, "archive_url")?,
        string(&spec, "archive_sha256")?,
        &archive,
        &ctx.cancel,
        |v| ctx.update(json!({"transfer":v})),
    )?;
    let source = files::open_at(rustix::fs::CWD, &archive, false, false)?;
    require(
        source.metadata()?.len() == spec["archive_bytes"].as_u64().unwrap_or(0),
        "Das MobiFlight-Paket hat eine unerwartete Größe.",
    )?;
    let target = profile.prefix.join(PROGRAM);
    require(
        !files::exists(&target),
        "Der MobiFlight-Zielordner enthält bereits Dateien und bleibt unverändert.",
    )?;
    let stage = new_stage(profile, &profile.prefix.join("drive_c"), "program")?;
    extract_zip(source, &stage, &ctx.cancel)?;
    for name in [
        EXE,
        "MFConnector.dll",
        "coreclr.dll",
        "hostfxr.dll",
        "MFConnector.runtimeconfig.json",
    ] {
        tx::owned_file(&stage, &stage.join(name))?;
    }
    let config: Value = serde_json::from_slice(&files::read(
        &stage.join("MFConnector.runtimeconfig.json"),
        65536,
    )?)?;
    require(
        config["runtimeOptions"]["tfm"] == "net10.0"
            && hidsharp_patch::state(&stage.join("HidSharp.dll")) == "required",
        "Das MobiFlight-Paket enthält unerwartete Laufzeitkomponenten.",
    )?;
    hidsharp_patch::apply(
        &stage,
        &profile.root.join("private/mobiflight-hid-backup"),
        &ctx.cancel,
    )?;
    files::atomic_json(
        &stage.join(RECEIPT),
        &json!({"format":1,"archive_sha256":spec["archive_sha256"],"version":spec["version"],"prefix_id":fenix::identity(&profile.prefix)?}),
    )?;
    ctx.begin_commit(
        "publish",
        "MobiFlight wird im ausgewählten Windows-Profil eingerichtet …",
    )?;
    tx::publish(&stage, &target)?;
    fs::remove_file(profile.root.join(JOURNAL))?;
    finish_preparation(profile, &target, ctx)
}
fn is_entry(element: &Element) -> bool {
    element.named("Launch.Addon")
        && element.child("Path").is_some_and(|p| {
            p.text()
                .trim()
                .trim_matches('"')
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .is_some_and(|p| p.eq_ignore_ascii_case(EXE))
        })
}
fn enabled(element: &Element) -> bool {
    element
        .child("Disabled")
        .is_none_or(|v| !v.text().trim().eq_ignore_ascii_case("true"))
}
pub fn startup_document(raw: Option<&[u8]>, executable: &str, enable: bool) -> Result<Vec<u8>> {
    require(
        executable.starts_with("C:\\")
            && executable
                .rsplit('\\')
                .next()
                .is_some_and(|v| v.eq_ignore_ascii_case(EXE))
            && !executable.contains(['\0', '\r', '\n']),
        "Ungültiger MobiFlight-Startpfad.",
    )?;
    let mut tree = if let Some(raw) = raw {
        xml::parse(raw)?
    } else {
        let mut tree = Element::new("SimBase.Document");
        tree.start.push_attribute(("Type", "Launch"));
        tree.start.push_attribute(("version", "1,0"));
        for (key, value) in [
            ("Descr", "Launch"),
            ("Filename", "exe.xml"),
            ("Disabled", "False"),
        ] {
            tree.set(key, value);
        }
        tree
    };
    require(
        tree.named("SimBase.Document") && (!enable || enabled(&tree)),
        "Der automatische Add-on-Start ist in exe.xml ausgeschaltet oder ungültig.",
    )?;
    let mut found = false;
    tree.items.retain(|item| {
        if let Item::Element(node) = item
            && is_entry(node)
        {
            if found {
                return false;
            }
            found = true;
        }
        true
    });
    if !found && enable {
        let mut node = Element::new("Launch.Addon");
        node.set("Path", executable);
        tree.items.push(Item::Element(node));
    }
    if let Some(node) = tree.items.iter_mut().find_map(|i| match i {
        Item::Element(n) if is_entry(n) => Some(n),
        _ => None,
    }) {
        if enable {
            for (key, value) in [
                ("Name", "MobiFlight"),
                ("Disabled", "False"),
                ("ManualLoad", "False"),
                ("Path", executable),
            ] {
                node.set(key, value);
            }
        } else {
            node.set("Disabled", "True");
        }
    }
    tree.bytes()
}
pub(crate) fn configure_for(profile: &Profile, enable: bool) -> Result<()> {
    let program = manager(&profile.prefix)?;
    require(
        !enable || program.is_some(),
        "Installiere zuerst MobiFlight in diesem Profil.",
    )?;
    let path = navigraph::settings_folder_for(profile)?.join("exe.xml");
    let raw = if files::exists(&path) {
        Some(files::read(&path, 2 * 1024 * 1024)?)
    } else {
        None
    };
    let executable = program
        .map(|p| windows_path(&p.join(EXE), &profile.prefix))
        .transpose()?
        .unwrap_or_else(|| "C:\\Flightdeck MobiFlight\\MFConnector.exe".into());
    let bytes = startup_document(raw.as_deref(), &executable, enable)?;
    if let Some(raw) = raw
        && raw != bytes
    {
        files::atomic(
            &profile.root.join("private").join(format!(
                "mobiflight-exe-{}.xml",
                uuid::Uuid::new_v4().simple()
            )),
            &raw,
        )?;
    }
    files::atomic(&path, &bytes)?;
    files::atomic_json(
        &profile.root.join(STARTUP),
        &json!({"format":1,"enabled":enable,"prefix_id":fenix::identity(&profile.prefix)?}),
    )
}
pub fn startup_enabled(root: &Path) -> bool {
    runtime::value(&root.join(STARTUP)).is_ok_and(|v| v["format"] == 1 && v["enabled"] == true)
}
fn actual_startup(profile: &Profile) -> bool {
    (|| -> Result<bool> {
        let path = navigraph::settings_folder_for(profile)?.join("exe.xml");
        let tree = xml::parse(&files::read(&path, 2 * 1024 * 1024)?)?;
        Ok(enabled(&tree)
            && tree
                .items
                .iter()
                .any(|i| matches!(i, Item::Element(n) if is_entry(n) && enabled(n))))
    })()
    .unwrap_or(false)
}
/// Inspect permissions and configuration only; never opens a HID node or autoloads a remapper.
pub fn device_access() -> Value {
    let mut devices = Vec::new();
    let mut limited = false;
    if let Ok(entries) = fs::read_dir("/sys/class/hidraw") {
        for (index, entry) in entries.enumerate() {
            if index >= 256 {
                limited = true;
                break;
            }
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name
                .strip_prefix("hidraw")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            {
                continue;
            }
            let Ok(event) = entry.path().join("device/uevent").canonicalize() else {
                continue;
            };
            if !event.starts_with("/sys/devices") {
                continue;
            }
            let Ok(raw) = files::read_public(&event, 16384) else {
                continue;
            };
            let text = String::from_utf8_lossy(&raw);
            let Some(id) = text.lines().find_map(|l| l.strip_prefix("HID_ID=")) else {
                continue;
            };
            let parts: Vec<_> = id.split(':').collect();
            if parts.len() != 3 || parts[1] != "00004098" {
                continue;
            }
            let node = Path::new("/dev").join(&name);
            let character =
                fs::symlink_metadata(&node).is_ok_and(|m| m.file_type().is_char_device());
            let access = character
                && rustix::fs::access(
                    &node,
                    rustix::fs::Access::READ_OK | rustix::fs::Access::WRITE_OK,
                )
                .is_ok();
            devices.push(json!({"node":node,"vendor":"4098","product":parts[2].trim_start_matches('0'),"read_write":access}));
        }
    }
    let remapper = files::xdg("XDG_CONFIG_HOME", ".config").join("input-remapper-2/config.json");
    let possible_grab = files::json::<Value>(&remapper, 2 * 1024 * 1024)
        .is_ok_and(|v| v["autoload"].as_object().is_some_and(|m| !m.is_empty()));
    let state = if limited {
        "limited"
    } else if devices.is_empty() {
        "no_winwing_device"
    } else if devices.iter().all(|d| d["read_write"] == true) {
        "ready"
    } else {
        "permission_required"
    };
    json!({"state":state,"devices":devices,"udev_rule":WINWING_UDEV_RULE,"possible_input_remapper_grab":possible_grab,"verified_in_simulator":false})
}
pub fn snapshot(app: &Launcher) -> Value {
    let selected = Profile::selected(app);
    let context = Launcher::context(&app.lock());
    let (busy, playing, closing, job) = selected
        .as_ref()
        .map(|p| addon_apps::job(app, "mobiflight", p))
        .unwrap_or((false, false, false, Value::Null));
    let mut value = json!({"state":"unavailable","runtime_path":context,"installed":false,"hid_fix":"missing","payload_available":hidsharp_patch::payload().is_ok(),"wasm_installed":false,"wasm_events_ready":false,"settings_found":false,"startup":false,"running":false,"idle":false,"can_change":false,"can_stop":false,"busy":busy||playing,"job":job,"message":"","device_access":device_access(),"verified_in_simulator":false});
    let result = (|| -> Result<()> {
        let profile = selected?;
        value["profile_token"] = json!(profile.revision);
        value["source"] = json!(if profile.item.is_some() {
            "steam"
        } else {
            "xbox"
        });
        value["installation_id"] = json!(profile.item.as_ref().map(|i| &i.id));
        value["game"] = json!(profile.game.id());
        profile.validate(false)?;
        let program = manager(&profile.prefix)?;
        let hid = program
            .as_ref()
            .map(|p| hidsharp_patch::state(&p.join("HidSharp.dll")))
            .unwrap_or("missing");
        let processes = wine_processes::Processes::new(&profile.prefix)?;
        let running = processes.any(Some(PROCESSES));
        let idle = wine_processes::idle(&profile.prefix).is_ok();
        let (wasm, events) = wasm_state(&profile);
        value["state"] = json!("available");
        let spec = lock()?;
        value["available_version"] = spec["version"].clone();
        value["version"] = if program.as_ref().is_some_and(|p| {
            tx::digest(&p.join(EXE)).is_ok_and(|v| spec["executable_sha256"] == v)
                && tx::digest(&p.join("MFConnector.dll"))
                    .is_ok_and(|v| spec["application_sha256"] == v)
        }) {
            spec["version"].clone()
        } else {
            Value::Null
        };
        value["installed"] = json!(program.is_some());
        value["hid_fix"] = json!(hid);
        value["wasm_installed"] = json!(wasm);
        value["wasm_events_ready"] = json!(events);
        value["settings_found"] = json!(navigraph::settings_folder_for(&profile).is_ok());
        value["startup"] = json!(profile.enabled(STARTUP) && actual_startup(&profile));
        value["winebus"] = winebus::snapshot(&profile);
        value["running"] = json!(running);
        value["idle"] = json!(idle);
        value["can_change"] = json!(idle && !busy && !playing && !closing);
        value["can_stop"] = json!(
            running
                && !playing
                && !processes.any(Some(wine_processes::GAMES))
                && !closing
                && (!busy
                    || (job["state"] == "running"
                        && job["operation"] == "open"
                        && job["stopping"] != true))
        );
        value["message"] = json!(if value["winebus"]["state"] == "disabled_by_user" {
            "WinWing-HID ist durch eine vorhandene Geräte-Einstellung deaktiviert. Flightdeck erhält diese Auswahl; ändere sie bewusst, wenn du WinWing mit MobiFlight verwenden möchtest."
        } else if hid == "unsupported" {
            "Diese HidSharp-Version wird nicht automatisch geändert. MobiFlight und vorhandene Konfigurationen bleiben erhalten."
        } else if program.is_some() && (!wasm || !events) {
            "Die MobiFlight-App ist vorhanden, aber WASM-Modul oder offizielle Ereignisse fehlen noch. Mit Vorbereitung / Reparatur wird die Einrichtung ohne erneuten App-Download fortgesetzt."
        } else if value["winebus"]["state"] == "unsupported_runner" {
            "MobiFlight ist verfügbar. WinWing-HID ist mit diesem Runner noch nicht geprüft; vorhandene Geräte-Einstellungen bleiben erhalten."
        } else {
            "MobiFlight verwendet das ausgewählte MSFS-Profil. Prüfe Geräte und Verbindung im Simulator; input-remapper wird nicht automatisch aktiviert."
        });
        Ok(())
    })();
    if let Err(error) = result {
        value["message"] = json!(error.to_string());
    }
    value
}
pub fn start(app: &Arc<Launcher>, operation: &str, data: &Value) -> Result<Value> {
    require(
        [
            "install",
            "prepare",
            "open",
            "stop",
            "startup-enable",
            "startup-disable",
            "configure-hid",
        ]
        .contains(&operation),
        "Unbekannter MobiFlight-Vorgang.",
    )?;
    let profile = Profile::selected(app)?;
    require(
        profile.context == string(data, "runtime_path")?
            && data["profile_token"] == profile.revision,
        "Die Installation oder Proton-Auswahl hat sich geändert. MobiFlight-Status neu laden.",
    )?;
    if operation == "stop" {
        require(
            snapshot(app)["can_stop"] == true,
            "Beende MSFS und laufende Installationen, bevor du MobiFlight schließt.",
        )?;
        let mut state = app.lock();
        Launcher::open(&state)?;
        if state
            .active
            .as_ref()
            .is_some_and(|a| a.kind == "mobiflight")
            && state
                .jobs
                .get("mobiflight")
                .is_some_and(|j| j["state"] == "running" && j["operation"] == "open")
        {
            if let Some(active) = &state.active {
                active.pause.store(true, Ordering::Relaxed);
            }
            let job = state
                .jobs
                .get_mut("mobiflight")
                .ok_or(Error::Invalid("MobiFlight läuft nicht."))?;
            job["stopping"] = json!(true);
            return Ok(json!({"ok":true,"job_id":job["id"]}));
        }
    } else {
        profile.validate(false)?;
    }
    let operation = operation.to_string();
    app.start_job("mobiflight", &operation.clone(), profile.item.is_none(), move |ctx| {
        profile.current(&ctx.launcher)?;
        let _held = profile.lease()?;
        if operation != "stop" { profile.validate(false)?; wine_processes::idle(&profile.prefix)?; }
        match operation.as_str() {
            "install" => install(ctx, &profile)?,
            "prepare" => prepare(&profile, ctx)?,
            "configure-hid" => winebus::configure(ctx, &profile)?,
            "startup-enable" => { recover(&profile)?; configure_for(&profile, true)?; },
            "startup-disable" => { recover(&profile)?; configure_for(&profile, false)?; },
            "stop" => profile.stop(PROCESSES)?,
            "open" => {
                let program = manager(&profile.prefix)?.ok_or(Error::Invalid("Installiere zuerst MobiFlight in diesem Windows-Profil."))?;
                let wine = profile.wine(ctx, "mobiflight-wine.log")?;
                // Official MobiFlight sets invariant cultures itself. Do not force a
                // host locale that may not be installed or replace Steam's environment.
                ctx.progress("MobiFlight ist geöffnet. Richte Geräte und die Simulator-Verbindung ein; schließe MobiFlight danach vollständig.");
                addon_apps::interactive(ctx, &profile, &wine, &[&program.join(EXE)], PROCESSES, "MobiFlight wurde mit einem Fehler beendet. Siehe das lokale MobiFlight-Protokoll.")?;
            },
            _ => unreachable!(),
        }
        Ok(json!({"state":"complete","stopping":false,"message":match operation.as_str() { "open"|"stop" => "MobiFlight wurde geschlossen.", "configure-hid" => "Die WinWing-Geräteliste wurde mit den vorhandenen Einstellungen zusammengeführt. Prüfe Geräte und Verbindung in MobiFlight.", "startup-enable" => "MobiFlight startet mit dem ausgewählten MSFS-Profil. Die Verbindung im Simulator ist noch unbestätigt.", "startup-disable" => "Der automatische MobiFlight-Start ist ausgeschaltet.", _ => "MobiFlight und fehlende offizielle WASM-Ereignisse wurden vorbereitet. Prüfe die Geräteverbindung in MobiFlight und im Simulator." }}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn fixture() -> (tempfile::TempDir, Profile) {
        let temp = tempfile::tempdir().expect("fixture");
        files::private_dir(&temp.path().join("local/msfs-prefix/drive_c")).expect("prefix");
        files::private_dir(&temp.path().join("private")).expect("metadata");
        let profile = Profile::xbox(temp.path()).expect("profile");
        (temp, profile)
    }
    #[test]
    fn hubhop_long_event_scripts_are_valid_but_invalid_or_unbounded_exports_are_rejected() {
        let mut events = String::from("//Synthetic HubHop export\n");
        for index in 0..100 {
            events.push_str(&format!("Test_{index}#1 (>L:Test, Number)\n"));
        }
        // The real 2026-10-30 export contains a 138,367-byte event line.
        events.push_str("Long_Event#");
        events.push_str(&"1 ".repeat(70_000));
        validate_wasm_events(events.as_bytes()).expect("official-sized long event");
        assert!(validate_wasm_events(b"<html>Service unavailable</html>").is_err());
        assert!(validate_wasm_events(b"//Invalid UTF-8\n\xff").is_err());
        let mut nul = events.clone().into_bytes();
        nul.push(0);
        assert!(validate_wasm_events(&nul).is_err());
        events.push_str(&"x".repeat(EVENTS_LIMIT));
        assert!(validate_wasm_events(events.as_bytes()).is_err());
    }
    #[test]
    fn interrupted_stage_recovery_checks_identity_and_preserves_published_and_foreign_files() {
        let (_temp, profile) = fixture();
        let parent = profile.prefix.join("drive_c");
        let stage = new_stage(&profile, &parent, "program").expect("stage");
        files::atomic(&stage.join("partial"), b"only staged files").expect("partial");
        files::atomic(&parent.join("keep"), b"unrelated").expect("unrelated");
        recover(&profile).expect("recover");
        assert!(!stage.exists());
        assert_eq!(
            files::read(&parent.join("keep"), 100).expect("kept"),
            b"unrelated"
        );
        let stage = new_stage(&profile, &parent, "program").expect("stage");
        let mut record = runtime::value(&profile.root.join(JOURNAL)).expect("journal");
        record["prefix_id"] = json!([0, 0]);
        files::atomic_json(&profile.root.join(JOURNAL), &record).expect("stale journal");
        recover(&profile).expect("preserve journal from a replaced profile");
        assert!(stage.is_dir());
        assert!(!profile.root.join(JOURNAL).exists());
        let archived = fs::read_dir(profile.root.join("private"))
            .expect("metadata")
            .map(|entry| entry.expect("entry").path())
            .find(|path| {
                path.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .starts_with("mobiflight-install-before-profile-change-")
            })
            .expect("original journal preserved");
        assert_eq!(
            runtime::value(&archived).expect("preserved journal"),
            record
        );
        // A new attempt uses a different stage and never reuses/deletes the
        // old profile's paths merely because their names still exist.
        let next = new_stage(&profile, &parent, "program").expect("new attempt");
        assert_ne!(next, stage);
        recover(&profile).expect("recover current attempt");
        assert!(stage.is_dir());
        assert!(!next.exists());
        assert!(archived.is_file());
    }
    #[test]
    fn existing_module_and_user_events_are_preserved_without_downloading() {
        let (_temp, profile) = fixture();
        let community = profile.prefix.join("drive_c/Actual Packages/Community");
        let module = community.join(MODULE);
        files::private_dir(&module.join("modules")).expect("module");
        files::atomic_json(
            &profile.root.join("private/runtime.json"),
            &json!({"game_id":"msfs2024","community_path":community}),
        )
        .expect("actual community");
        for (name, bytes) in [
            (
                "modules/MobiFlightWasmModule.wasm",
                b"newer user module".as_slice(),
            ),
            ("modules/events.txt", b"existing official events"),
            ("modules/events.user.txt", b"my custom events"),
        ] {
            files::atomic(&module.join(name), bytes).expect("user file");
        }
        prepare_wasm(
            &profile,
            Path::new("/unused-program"),
            &AtomicBool::new(false),
        )
        .expect("preserved");
        assert_eq!(
            files::read(&module.join("modules/events.user.txt"), 100).expect("user data"),
            b"my custom events"
        );
        assert_eq!(
            files::read(&module.join("modules/events.txt"), 100).expect("events"),
            b"existing official events"
        );
        assert_eq!(
            files::read(&module.join("modules/MobiFlightWasmModule.wasm"), 100).expect("module"),
            b"newer user module"
        );
    }
    #[test]
    #[ignore = "requires network and FLIGHTDECK_MOBIFLIGHT_ARCHIVE pointing to the official ZIP"]
    fn official_hubhop_export_completes_interrupted_wasm_setup() {
        let archive = PathBuf::from(
            std::env::var_os("FLIGHTDECK_MOBIFLIGHT_ARCHIVE")
                .expect("explicit official archive fixture"),
        );
        let spec = lock().expect("pins");
        assert_eq!(
            tx::digest(&archive).expect("archive hash"),
            spec["archive_sha256"]
        );
        let (_temp, profile) = fixture();
        let program = profile.prefix.join(PROGRAM);
        files::private_dir(&program).expect("program");
        extract_zip(
            files::open_at(rustix::fs::CWD, &archive, false, false).expect("archive"),
            &program,
            &AtomicBool::new(false),
        )
        .expect("official archive");
        let community = profile.prefix.join("drive_c/Actual Packages/Community");
        files::private_dir(&community).expect("community");
        files::atomic_json(
            &profile.root.join("private/runtime.json"),
            &json!({"game_id":"msfs2024","community_path":community}),
        )
        .expect("actual community");
        let interrupted = new_stage(&profile, &community, "module").expect("interrupted setup");
        files::atomic(&interrupted.join("partial"), b"incomplete download").expect("partial");
        files::atomic(&community.join("keep.txt"), b"unrelated add-on").expect("unrelated");
        recover(&profile).expect("resume interrupted setup");
        assert!(!interrupted.exists());

        prepare_wasm(&profile, &program, &AtomicBool::new(false)).expect("live HubHop export");
        let module = community.join(MODULE);
        let events = module.join("modules/events.txt");
        let expected_events = tx::digest(&events).expect("published official events");
        let layout = runtime::value(&module.join("layout.json")).expect("layout");
        let entry = layout["content"]
            .as_array()
            .expect("content")
            .iter()
            .find(|entry| entry["path"] == "modules/events.txt")
            .expect("events entry");
        assert_eq!(
            entry["size"],
            fs::metadata(&events).expect("events metadata").len()
        );
        assert!(!profile.root.join(JOURNAL).exists());
        assert!(program.join(EXE).is_file());
        files::atomic(&module.join("modules/events.user.txt"), b"my events")
            .expect("custom events");
        // With a published module, repair needs neither the source package nor a
        // second HubHop download and must retain the user's event definitions.
        prepare_wasm(
            &profile,
            Path::new("/unused-program"),
            &AtomicBool::new(false),
        )
        .expect("repair again");
        assert_eq!(
            tx::digest(&events).expect("retained events"),
            expected_events
        );
        assert_eq!(
            files::read(&module.join("modules/events.user.txt"), 100).expect("user events"),
            b"my events"
        );
        assert_eq!(
            files::read(&community.join("keep.txt"), 100).expect("unrelated"),
            b"unrelated add-on"
        );
    }
    #[test]
    #[cfg(feature = "bundled-mobiflight")]
    #[ignore = "requires FLIGHTDECK_MOBIFLIGHT_ARCHIVE pointing to the original official ZIP"]
    fn official_archive_can_be_staged_and_the_hid_fix_is_idempotent_with_original_backup() {
        let path = PathBuf::from(
            std::env::var_os("FLIGHTDECK_MOBIFLIGHT_ARCHIVE")
                .expect("explicit official archive fixture"),
        );
        let spec = lock().expect("pins");
        assert_eq!(
            tx::digest(&path).expect("archive digest"),
            spec["archive_sha256"]
        );
        let temp = tempfile::tempdir().expect("stage");
        let program = temp.path().join("program");
        files::private_dir(&program).expect("program stage");
        extract_zip(
            files::open_at(rustix::fs::CWD, &path, false, false).expect("archive"),
            &program,
            &AtomicBool::new(false),
        )
        .expect("official extraction constraints");
        for (name, hash) in MODULE_FILES {
            assert_eq!(
                tx::digest(&program.join("MSFS2020-module").join(MODULE).join(name))
                    .expect("module file"),
                *hash
            );
        }
        assert_eq!(
            hidsharp_patch::state(&program.join("HidSharp.dll")),
            "required"
        );
        let backup = temp.path().join("backup");
        hidsharp_patch::apply(&program, &backup, &AtomicBool::new(false))
            .expect("known original patched");
        assert_eq!(
            hidsharp_patch::state(&program.join("HidSharp.dll")),
            "ready"
        );
        assert_eq!(
            tx::digest(&backup.join(format!("HidSharp-{}.dll", hidsharp_patch::ORIGINAL)))
                .expect("original backup"),
            hidsharp_patch::ORIGINAL
        );
        hidsharp_patch::apply(&program, &backup, &AtomicBool::new(false)).expect("idempotent");
    }
    #[test]
    fn unsafe_zip_paths_and_links_are_rejected_before_publication() {
        for (name, mode) in [
            ("../escape", 0o100600),
            ("C:/escape", 0o100600),
            ("a\\escape", 0o100600),
            ("x.", 0o100600),
        ] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default().unix_permissions(mode),
                )
                .expect("entry");
            writer.write_all(b"x").expect("write");
            let archive = writer.finish().expect("archive").into_inner();
            let temp = tempfile::tempdir().expect("target");
            assert!(
                extract_zip(Cursor::new(archive), temp.path(), &AtomicBool::new(false)).is_err(),
                "{name}"
            );
        }
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["one.dll", "ONE.DLL"] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("entry");
            writer.write_all(b"x").expect("write");
        }
        let archive = writer.finish().expect("archive").into_inner();
        let temp = tempfile::tempdir().expect("target");
        assert!(extract_zip(Cursor::new(archive), temp.path(), &AtomicBool::new(false)).is_err());
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink(
                "link",
                "../escape",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("link");
        let archive = writer.finish().expect("archive").into_inner();
        assert!(extract_zip(Cursor::new(archive), temp.path(), &AtomicBool::new(false)).is_err());
    }
}
