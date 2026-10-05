// SPDX-License-Identifier: MIT
//! Atomic per-user native launcher releases, with legacy rollback compatibility.
use crate::{
    Error, Result, backend::string, error::require, files, log_reader::regex, ordered_json,
    owned_tree, resources, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    sync::atomic::AtomicBool,
};
pub const APP: &str = "flightdeck-source-launcher";
pub const PACKAGE: &str = "FLIGHTDECK-PACKAGE.json";
const INVALID: &str = "Die installierte Flightdeck-Version konnte nicht zugeordnet werden.";
const CHANGED: &str = "Flightdeck wurde inzwischen geändert. Bitte den Launcher neu öffnen.";
const FOREIGN: &str = "Vorhandene fremde oder geänderte Dateien bleiben erhalten. Bitte den Installationsordner prüfen.";
const MAX_FILE: usize = 128 * 1024 * 1024;
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = files::expand(path.to_str().ok_or(Error::Invalid(INVALID))?);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    require(
        !path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|b| b"\r\n\0".contains(b)),
        "Installationspfade dürfen keine Zeilenumbrüche enthalten.",
    )?;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::RootDir => out.push("/"),
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            Component::ParentDir => {
                require(out.pop(), INVALID)?;
            }
            _ => return Err(Error::Invalid(INVALID)),
        }
    }
    Ok(out)
}
pub fn no_links(path: &Path) -> Result<()> {
    for part in path.ancestors() {
        require(
            !part.is_symlink(),
            "Der Installationspfad enthält einen symbolischen Link.",
        )?;
    }
    Ok(())
}
fn directory(path: &Path) -> Result<()> {
    no_links(path)?;
    if !files::exists(path) {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    files::directory(path, false)?;
    Ok(())
}
fn read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    no_links(path)?;
    files::read(path, maximum)
}
fn json_file(path: &Path) -> Result<Value> {
    Ok(crate::cloud::json(&read(path, 2 * 1024 * 1024)?)?)
}
fn json_bytes(value: &Value) -> Result<Vec<u8>> {
    Ok(ordered_json::encoded(value)?)
}
fn write(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    directory(path.parent().ok_or(Error::Invalid(INVALID))?)?;
    no_links(path)?;
    files::atomic(path, data)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    files::open_at(rustix::fs::CWD, path, false, false)?.sync_all()?;
    Ok(())
}
fn safe_name(name: &str) -> bool {
    files::relative(name) && name.len() <= 1024 && !name.chars().any(char::is_control)
}
fn hashes(value: &Value) -> Result<BTreeMap<String, String>> {
    let entries: BTreeMap<String, String> = serde_json::from_value(value.clone())?;
    require(
        !entries.is_empty()
            && entries.len() <= 4096
            && entries
                .iter()
                .all(|(name, hash)| safe_name(name) && files::hex_digest(hash)),
        INVALID,
    )?;
    Ok(entries)
}
pub struct Snapshot {
    pub files: BTreeMap<String, Vec<u8>>,
    pub id: String,
    pub manifest: Value,
}
pub fn snapshot(source: &Path) -> Result<Snapshot> {
    let source = absolute(source)?;
    no_links(&source)?;
    let package_bytes = read(&source.join(PACKAGE), 2 * 1024 * 1024)?;
    let package = crate::cloud::json(&package_bytes)?;
    require(
        package["schema"] == 1
            && package["kind"] == "rust-launcher"
            && package["version"] == crate::VERSION,
        "Das native Flightdeck-Paket ist unvollständig oder gehört zu einer anderen Version.",
    )?;
    let hashes = hashes(&package["files"])?;
    for required in [
        "bin/flightdeck",
        "ui/mark.svg",
        "LICENSE",
        "THIRD-PARTY-NOTICES.txt",
        "RUST-STANDARD-LIBRARY-NOTICES.html",
    ] {
        require(
            hashes.contains_key(required),
            "Das native Flightdeck-Paket ist unvollständig.",
        )?;
    }
    let native = resources::json("compat/bootstrap.lock.json")?;
    let graphics = resources::json("compat/graphics.lock.json")?;
    let mut allowed: BTreeSet<String> = [
        "bin/flightdeck",
        "ui/mark.svg",
        "LICENSE",
        "THIRD-PARTY-NOTICES.txt",
        "RUST-STANDARD-LIBRARY-NOTICES.html",
        "README.txt",
        "compat/bootstrap.lock.json",
        "compat/graphics.lock.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for key in native["native"]["files"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .map(String::as_str)
        .chain(["manifest.json", "THIRD-PARTY-NOTICES.txt"])
    {
        allowed.insert(format!("resources/native/{key}"));
    }
    for key in graphics["files"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .map(String::as_str)
        .chain(["manifest.json", "LICENSE"])
    {
        allowed.insert(format!("resources/graphics/{key}"));
    }
    let mut contents = BTreeMap::new();
    let mut total = 0usize;
    for (name, hash) in hashes {
        require(
            allowed.contains(&name),
            "Das native Flightdeck-Paket enthält unbekannte Dateien.",
        )?;
        let data = read(&source.join(&name), MAX_FILE)?;
        total = total
            .checked_add(data.len())
            .ok_or(Error::Invalid(INVALID))?;
        require(
            total <= 384 * 1024 * 1024 && files::sha256(&data) == hash,
            "Eine Paketdatei wurde verändert oder überschreitet die erlaubte Größe.",
        )?;
        contents.insert(name, data);
    }
    let binary = &contents["bin/flightdeck"];
    require(
        binary.len() >= 64
            && binary.starts_with(b"\x7fELF\x02\x01")
            && binary.get(18..20) == Some(&[62, 0]),
        "Das Paket enthält kein ausführbares Linux-x86_64-Programm.",
    )?;
    if contents.keys().any(|k| k.starts_with("resources/native/")) {
        let expected = native["native"]["files"]
            .as_object()
            .ok_or(Error::Invalid(INVALID))?;
        for (name, hash) in expected
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .chain(std::iter::once((
                "THIRD-PARTY-NOTICES.txt",
                &native["native"]["notice_sha256"],
            )))
        {
            require(
                contents
                    .get(&format!("resources/native/{name}"))
                    .is_some_and(|data| json!(files::sha256(data)) == *hash),
                "Das geprüfte Kompatibilitätspaket ist unvollständig.",
            )?;
        }
        let manifest: Value = crate::cloud::json(
            contents
                .get("resources/native/manifest.json")
                .ok_or(Error::Invalid(INVALID))?,
        )?;
        require(manifest == json!({"format":1,"files":expected}), INVALID)?;
    }
    if contents
        .keys()
        .any(|k| k.starts_with("resources/graphics/"))
    {
        let expected = graphics["files"]
            .as_object()
            .ok_or(Error::Invalid(INVALID))?;
        for (name, hash) in expected
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .chain(std::iter::once(("LICENSE", &graphics["license_sha256"])))
        {
            require(
                contents
                    .get(&format!("resources/graphics/{name}"))
                    .is_some_and(|data| json!(files::sha256(data)) == *hash),
                "Das geprüfte Grafikpaket ist unvollständig.",
            )?;
        }
        let manifest: Value = crate::cloud::json(
            contents
                .get("resources/graphics/manifest.json")
                .ok_or(Error::Invalid(INVALID))?,
        )?;
        require(
            manifest == json!({"schema":2,"base":graphics["base"],"files":expected}),
            INVALID,
        )?;
    }
    // Store the package description too, so an installed release remains a
    // complete input for the native installer and every byte is tracked.
    contents.insert(PACKAGE.into(), package_bytes);
    let map: Value = contents
        .iter()
        .map(|(k, v)| (k.clone(), json!(files::sha256(v))))
        .collect::<serde_json::Map<_, _>>()
        .into();
    let id = files::sha256(&json_bytes(&map)?);
    let manifest = json!({"app":APP,"format":1,"release":id,"files":map});
    Ok(Snapshot {
        files: contents,
        id,
        manifest,
    })
}
pub fn release_manifest(root: &Path, id: &str) -> Result<(PathBuf, Value)> {
    require(files::hex_digest(id), INVALID)?;
    let folder = root.join("releases").join(id);
    let record = json_file(&folder.join("release.json"))?;
    let checked = hashes(&record["files"])?;
    require(
        record["app"] == APP
            && record["format"] == 1
            && record["release"] == id
            && files::sha256(&json_bytes(&json!(checked))?) == id,
        INVALID,
    )?;
    Ok((folder, record))
}
pub fn verify_release(root: &Path, id: &str) -> Result<PathBuf> {
    let (folder, record) = release_manifest(root, id)?;
    let expected = hashes(&record["files"])?;
    let entries = tx::walk(&folder, &AtomicBool::new(false))?;
    require(entries.len() <= 10000, INVALID)?;
    // Older managed releases may contain interpreter caches. Remove only
    // recognized generated files for modules covered by their release hashes.
    for path in &entries {
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "__pycache__")
            && !path.is_dir()
        {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let c = regex(r"^([A-Za-z_][A-Za-z0-9_]*)\.cpython-\d+(?:\.opt-\d+)?\.pyc$")
                .captures(name)
                .ok_or(Error::Invalid(FOREIGN))?;
            let source = path
                .parent()
                .and_then(Path::parent)
                .ok_or(Error::Invalid(FOREIGN))?
                .join(format!("{}.py", &c[1]));
            require(
                source
                    .strip_prefix(&folder)
                    .ok()
                    .and_then(|p| p.to_str())
                    .is_some_and(|p| expected.contains_key(p)),
                FOREIGN,
            )?;
            files::open_at(rustix::fs::CWD, path, false, false)?;
        }
    }
    for path in entries.iter().rev() {
        no_links(path)?;
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "__pycache__")
            && !path.is_dir()
        {
            fs::remove_file(path)?;
        } else if path.file_name().is_some_and(|n| n == "__pycache__") {
            fs::remove_dir(path)?;
        }
    }
    for path in tx::walk(&folder, &AtomicBool::new(false))? {
        no_links(&path)?;
        if !path.is_dir() {
            let name = path
                .strip_prefix(&folder)
                .ok()
                .and_then(|p| p.to_str())
                .ok_or(Error::Invalid(INVALID))?;
            require(
                name == "release.json" || expected.contains_key(name),
                FOREIGN,
            )?;
        }
    }
    for (name, hash) in expected {
        require(
            files::sha256(&read(&folder.join(name), MAX_FILE)?) == hash,
            "Installierte Flightdeck-Dateien wurden verändert. Bitte den Installer erneut ausführen.",
        )?;
    }
    Ok(folder)
}
pub fn load(root: &Path) -> Result<Option<Value>> {
    let path = root.join("installation.json");
    if !files::exists(&path) {
        return Ok(None);
    }
    let v = json_file(&path)?;
    require(
        v["app"] == APP
            && v["format"] == 1
            && v["data_dir"].as_str() == root.to_str()
            && v["current"].as_str().is_some_and(files::hex_digest)
            && v.get("manager")
                .unwrap_or(&v["current"])
                .as_str()
                .is_some_and(files::hex_digest)
            && v.get("language")
                .is_none_or(|s| matches!(s.as_str(), Some("de" | "en")))
            && (v["previous"].is_null() || v["previous"].as_str().is_some_and(files::hex_digest)),
        INVALID,
    )?;
    let entries = v["entries"].as_object().ok_or(Error::Invalid(INVALID))?;
    require(
        entries.contains_key("launcher") && entries.len() <= 2,
        INVALID,
    )?;
    for (role, entry) in entries {
        let name = match role.as_str() {
            "launcher" => "flightdeck",
            "desktop" => "flightdeck.desktop",
            _ => return Err(Error::Invalid(INVALID)),
        };
        let path = Path::new(string(entry, "path")?);
        require(
            path.is_absolute()
                && path.file_name().is_some_and(|p| p == name)
                && entry["sha256"].as_str().is_some_and(files::hex_digest),
            INVALID,
        )?;
    }
    Ok(Some(v))
}
fn lock(root: &Path) -> Result<files::Lease> {
    directory(root)?;
    files::Lease::acquire(&root.join(".install.lock"), true)
}
fn shell(value: &Path) -> Result<String> {
    let value = value.to_str().ok_or(Error::Invalid(INVALID))?;
    require(!value.contains(['\n', '\r', '\0']), INVALID)?;
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}
pub fn launcher_bytes(root: &Path, manager: &str) -> Result<Vec<u8>> {
    require(files::hex_digest(manager), INVALID)?;
    Ok(format!("#!/bin/sh\n# Managed by Flightdeck; SPDX-License-Identifier: MIT\nexec {} --managed-root {} \"$@\"\n",shell(&root.join("releases").join(manager).join("bin/flightdeck"))?,shell(root)?).into_bytes())
}
pub fn desktop_bytes(launcher: &Path, icon: &Path, language: &str) -> Result<Vec<u8>> {
    let raw = launcher.to_str().ok_or(Error::Invalid(INVALID))?;
    let mut quoted = String::new();
    for c in raw.chars() {
        match c {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    let comment = if language == "de" {
        "MSFS Xbox PC unter Linux starten"
    } else {
        "Launch MSFS Xbox PC on Linux"
    };
    Ok(format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName=Flightdeck\nComment={comment}\nComment[de]=MSFS Xbox PC unter Linux starten\nComment[en]=Launch MSFS Xbox PC on Linux\nExec=\"{quoted}\"\nIcon={}\nTerminal=false\nCategories=Game;Simulation;\nStartupNotify=false\n",icon.display()).into_bytes())
}
type Entry = (PathBuf, Vec<u8>, u32);
fn apply_entries(
    root: &Path,
    old: Option<&Value>,
    state: &mut Value,
    entries: BTreeMap<&str, Entry>,
) -> Result<()> {
    let mut backups = Vec::new();
    let mut publishing = false;
    for (role, (path, _, _)) in &entries {
        directory(path.parent().ok_or(Error::Invalid(INVALID))?)?;
        no_links(path)?;
        let previous = old.map(|v| &v["entries"][*role]).unwrap_or(&Value::Null);
        if files::exists(path) {
            let data = read(path, MAX_FILE)?;
            require(
                previous["path"].as_str() == path.to_str()
                    && previous["sha256"] == files::sha256(&data),
                FOREIGN,
            )?;
        } else {
            require(previous.is_null(), FOREIGN)?;
        }
    }
    let publish = (|| -> Result<()> {
        for (role, (path, data, mode)) in entries {
            let before = if files::exists(&path) {
                Some((
                    read(&path, MAX_FILE)?,
                    fs::metadata(&path)?.permissions().mode() & 0o777,
                ))
            } else {
                None
            };
            backups.push((path.clone(), before));
            write(&path, &data, mode)?;
            state["entries"][role] = json!({"path":path,"sha256":files::sha256(&data)});
        }
        let bytes = json_bytes(state)?;
        publishing = true;
        write(&root.join("installation.json"), &bytes, 0o600)
    })();
    if let Err(error) = publish {
        if publishing && !load(root).is_ok_and(|current| current.as_ref() == old) {
            return Err(error);
        } // Published or uncertain: retain entries and release for recovery.
        for (path, before) in backups.into_iter().rev() {
            if let Some((bytes, mode)) = before {
                write(&path, &bytes, mode)?;
            } else if files::exists(&path) {
                fs::remove_file(path)?;
            }
        }
        return Err(error);
    }
    Ok(())
}
pub struct Options<'a> {
    pub source: &'a Path,
    pub root: &'a Path,
    pub bin_dir: &'a Path,
    pub applications_dir: &'a Path,
    pub desktop: bool,
    pub language: &'a str,
    pub expected_current: Option<&'a str>,
}
pub fn install(options: Options<'_>) -> Result<Value> {
    require(
        matches!(options.language, "de" | "en"),
        "Bitte --language de oder --language en wählen.",
    )?;
    let snapshot = snapshot(options.source)?;
    let root = absolute(options.root)?;
    let mut bin = absolute(options.bin_dir)?;
    let mut applications = absolute(options.applications_dir)?;
    let mut desktop = options.desktop;
    let _lease = lock(&root)?;
    let old = load(&root)?;
    require(
        options
            .expected_current
            .is_none_or(|id| old.as_ref().is_some_and(|v| v["current"] == id)),
        CHANGED,
    )?;
    if let Some(old) = &old {
        verify_release(&root, string(old, "current")?)?;
        bin = Path::new(string(&old["entries"]["launcher"], "path")?)
            .parent()
            .ok_or(Error::Invalid(INVALID))?
            .into();
        if old["entries"]["desktop"].is_object() {
            applications = Path::new(string(&old["entries"]["desktop"], "path")?)
                .parent()
                .ok_or(Error::Invalid(INVALID))?
                .into();
            desktop = true;
        }
    } else {
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            require(
                entry.file_name() == ".install.lock"
                    || (entry.file_name() == "releases"
                        && entry.file_type()?.is_dir()
                        && fs::read_dir(entry.path())?.next().is_none()),
                "Das Zielverzeichnis enthält fremde Dateien. Bitte ein leeres Installationsziel wählen.",
            )?;
        }
    }
    let releases = root.join("releases");
    require(
        !bin.starts_with(&releases) && !applications.starts_with(&releases),
        FOREIGN,
    )?;
    directory(&releases)?;
    let target = releases.join(&snapshot.id);
    let created = !files::exists(&target);
    if created {
        let staging = tx::new_directory(&releases, ".staging-")?;
        let staging_id = owned_tree::identity(&staging)?;
        let result = (|| -> Result<()> {
            for (name, data) in &snapshot.files {
                let mode = if name == "bin/flightdeck" || name.starts_with("resources/native/bin/")
                {
                    0o755
                } else {
                    0o644
                };
                write(&staging.join(name), data, mode)?;
            }
            write(
                &staging.join("release.json"),
                &json_bytes(&snapshot.manifest)?,
                0o644,
            )?;
            tx::publish(&staging, &target)
        })();
        if files::exists(&staging) {
            let _ = owned_tree::remove(&staging, staging_id);
        }
        result?;
    } else {
        verify_release(&root, &snapshot.id)?;
    }
    let previous = old
        .as_ref()
        .map(|v| {
            if v["current"] == snapshot.id {
                v["previous"].clone()
            } else {
                v["current"].clone()
            }
        })
        .unwrap_or(Value::Null);
    let mut state = json!({"app":APP,"format":1,"data_dir":root,"current":snapshot.id,"previous":previous,"manager":snapshot.id,"language":options.language,"entries":old.as_ref().map(|v|v["entries"].clone()).unwrap_or(json!({}))});
    let launcher = bin.join("flightdeck");
    let mut entries: BTreeMap<&str, Entry> = [(
        "launcher",
        (
            launcher.clone(),
            launcher_bytes(&root, &snapshot.id)?,
            0o700,
        ),
    )]
    .into();
    if desktop {
        entries.insert(
            "desktop",
            (
                applications.join("flightdeck.desktop"),
                desktop_bytes(&launcher, &target.join("ui/mark.svg"), options.language)?,
                0o644,
            ),
        );
    }
    if let Err(error) = apply_entries(&root, old.as_ref(), &mut state, entries) {
        if created && load(&root).is_ok_and(|current| current.as_ref() == old.as_ref()) {
            let _ = owned_tree::remove(&target, owned_tree::identity(&target)?);
        }
        return Err(error);
    }
    Ok(state)
}
pub fn rollback(root: &Path, language: &str, expected: Option<&str>) -> Result<Value> {
    require(
        matches!(language, "de" | "en"),
        "Bitte --language de oder --language en wählen.",
    )?;
    let root = absolute(root)?;
    let _lease = lock(&root)?;
    let old = load(&root)?.ok_or(Error::Invalid(
        "Hier ist kein Flightdeck-Launcher installiert.",
    ))?;
    require(expected.is_none_or(|id| old["current"] == id), CHANGED)?;
    let id = old["previous"].as_str().ok_or(Error::Invalid(
        "Keine vorherige Launcher-Version für ein Rollback vorhanden.",
    ))?;
    let target = verify_release(&root, id)?;
    let manager = old
        .get("manager")
        .unwrap_or(&old["current"])
        .as_str()
        .ok_or(Error::Invalid(INVALID))?;
    let manager_path = verify_release(&root, manager)?;
    require(
        manager_path.join("bin/flightdeck").is_file(),
        "Für diesen Rollback bitte den ursprünglichen Installer verwenden.",
    )?;
    let launcher = PathBuf::from(string(&old["entries"]["launcher"], "path")?);
    let mut state = old.clone();
    state["current"] = json!(id);
    state["previous"] = old["current"].clone();
    state["language"] = json!(language);
    let mut entries: BTreeMap<&str, Entry> = [(
        "launcher",
        (launcher.clone(), launcher_bytes(&root, manager)?, 0o700),
    )]
    .into();
    if old["entries"]["desktop"].is_object() {
        entries.insert(
            "desktop",
            (
                PathBuf::from(string(&old["entries"]["desktop"], "path")?),
                desktop_bytes(&launcher, &target.join("ui/mark.svg"), language)?,
                0o644,
            ),
        );
    }
    apply_entries(&root, Some(&old), &mut state, entries)?;
    Ok(state)
}
pub fn context() -> Result<Option<(PathBuf, String)>> {
    let exe = std::env::current_exe()?;
    let Some(source) = exe.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let Some(id) = source
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| files::hex_digest(s))
    else {
        return Ok(None);
    };
    let Some(releases) = source
        .parent()
        .filter(|p| p.file_name().is_some_and(|s| s == "releases"))
    else {
        return Ok(None);
    };
    let root = releases.parent().ok_or(Error::Invalid(INVALID))?;
    let state = load(root)?.ok_or(Error::Invalid(INVALID))?;
    require(
        state["current"] == id || state["previous"] == id || state["manager"] == id,
        INVALID,
    )?;
    Ok(Some((root.into(), id.into())))
}
fn unlink_owned(path: &Path, hash: &str, retained: &mut BTreeSet<PathBuf>) {
    if !files::exists(path) {
        return;
    }
    if read(path, MAX_FILE).is_ok_and(|data| files::sha256(&data) == hash)
        && fs::remove_file(path).is_ok()
    {
        return;
    }
    retained.insert(path.into());
}
pub fn uninstall(root: &Path) -> Result<Vec<PathBuf>> {
    let root = absolute(root)?;
    let _lease = lock(&root)?;
    let state = load(&root)?.ok_or(Error::Invalid(
        "Hier ist kein Flightdeck-Launcher installiert.",
    ))?;
    let releases = root.join("releases");
    no_links(&releases)?;
    files::directory(&releases, false)?;
    let mut retained = BTreeSet::new();
    for entry in state["entries"]
        .as_object()
        .into_iter()
        .flat_map(|v| v.values())
    {
        unlink_owned(
            Path::new(string(entry, "path")?),
            string(entry, "sha256")?,
            &mut retained,
        );
    }
    for entry in fs::read_dir(&releases)? {
        let entry = entry?;
        let release = release_manifest(&root, &entry.file_name().to_string_lossy());
        let Ok((folder, record)) = release else {
            retained.insert(entry.path());
            continue;
        };
        for (name, hash) in hashes(&record["files"])? {
            unlink_owned(&folder.join(name), &hash, &mut retained);
        }
        if let Ok(mut paths) = tx::walk(&folder, &AtomicBool::new(false)) {
            paths.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
            for path in paths {
                if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                    let _ = fs::remove_dir(path);
                }
            }
        }
        if fs::read_dir(&folder)?.all(|p| p.is_ok_and(|e| e.file_name() == "release.json")) {
            fs::remove_file(folder.join("release.json"))?;
            fs::remove_dir(folder)?;
        } else {
            retained.insert(folder);
        }
    }
    fs::remove_file(root.join("installation.json"))?;
    let _ = fs::remove_dir(releases);
    files::directory(&root, false)?.sync_all()?;
    Ok(retained.into_iter().collect())
}
/// Resolve the selected release once per invocation. Shell glue contains only
/// this stable root; parsing, validation and all management execute in Rust.
pub fn managed(root: &Path, arguments: &[std::ffi::OsString]) -> Result<()> {
    use std::{
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        time::Duration,
    };
    let root = absolute(root)?;
    let state = load(&root)?.ok_or(Error::Invalid(INVALID))?;
    let mut language = state["language"].as_str().unwrap_or("en").to_owned();
    let mut explicit = false;
    let mut args = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let value = arguments[index].to_str().ok_or(Error::Invalid(INVALID))?;
        let selected = if value == "--language" {
            index += 1;
            Some(
                arguments
                    .get(index)
                    .and_then(|s| s.to_str())
                    .ok_or(Error::Invalid(
                        "Bitte --language de oder --language en wählen.",
                    ))?,
            )
        } else {
            value.strip_prefix("--language=")
        };
        if let Some(selected) = selected {
            require(
                matches!(selected, "de" | "en"),
                "Bitte --language de oder --language en wählen.",
            )?;
            language = selected.into();
            explicit = true;
        } else {
            args.push(arguments[index].clone());
        }
        index += 1;
    }
    let action = args.first().and_then(|s| s.to_str());
    if matches!(action, Some("--uninstall" | "--rollback" | "--update")) {
        require(
            args.len() == if action == Some("--update") { 2 } else { 1 },
            "Diese Verwaltungsaktion enthält ungültige zusätzliche Optionen.",
        )?;
        match action {
            Some("--uninstall") => {
                let retained = uninstall(&root)?;
                println!(
                    "{}",
                    crate::i18n::text(
                        "Launcher entfernt. Einstellungen, Runtimes und Spielstände bleiben erhalten.",
                        &language
                    )
                );
                for path in retained {
                    println!(
                        "{}: {}",
                        if language == "de" {
                            "Behalten"
                        } else {
                            "Retained"
                        },
                        path.display()
                    );
                }
                return Ok(());
            }
            Some("--rollback") => {
                rollback(&root, &language, Some(string(&state, "current")?))?;
            }
            _ => {
                let source = absolute(Path::new(&args[1]))?;
                let executable = package_executable(&source)?;
                let status = crate::process::run(
                    Command::new(executable)
                        .arg("install")
                        .arg("--source")
                        .arg(&source)
                        .arg("--data-dir")
                        .arg(&root)
                        .args([
                            "--language",
                            &language,
                            "--no-desktop",
                            "--no-launch",
                            "--expected-current",
                            string(&state, "current")?,
                        ])
                        .stdin(Stdio::null()),
                    Duration::from_secs(180),
                    &AtomicBool::new(false),
                )?;
                require(
                    status.success(),
                    "Das Launcher-Update konnte nicht installiert werden. Die bisherige Version bleibt verfügbar.",
                )?;
            }
        }
        args.clear();
    }
    let state = load(&root)?.ok_or(Error::Invalid(INVALID))?;
    let selected = verify_release(&root, string(&state, "current")?)?;
    let native = selected.join("bin/flightdeck").is_file();
    let mut command = if native {
        Command::new(selected.join("bin/flightdeck"))
    } else {
        // Only an explicitly selected older release uses its own historical
        // interpreter. Native releases never import the retained Python code.
        require(selected.join("flightdeck/__main__.py").is_file(), INVALID)?;
        let mut command = Command::new("python3");
        command.args(["-B","-c","import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module('flightdeck',run_name='__main__')"]).arg(&selected).env_remove("PYTHONHOME").env_remove("PYTHONPATH");
        command
    };
    let management = matches!(
        args.first().and_then(|s| s.to_str()),
        Some(
            "install"
                | "play"
                | "diagnose-run"
                | "framework-check"
                | "runtime-check"
                | "verify"
                | "save-check"
                | "wine-launch"
                | "run-game"
                | "display-refresh"
                | "desktop-handoff"
        )
    );
    if !management
        && !args.iter().any(|s| {
            [
                "--desktop",
                "--desktop-service",
                "--no-browser",
                "--refresh-components",
                "--native-probe",
                "--help",
                "-h",
                "--version",
                "-V",
            ]
            .iter()
            .any(|flag| s == flag)
        })
    {
        args.insert(0, "--desktop".into());
    }
    // Saved installation language belongs to management messages. Only an
    // explicit launch option may override the native interface's saved preference.
    if explicit
        && (native
            || read(&selected.join("flightdeck/__main__.py"), MAX_FILE)?
                .windows(b"--language".len())
                .any(|v| v == b"--language"))
    {
        command.args(["--language", &language]);
    }
    command.args(args).current_dir(&selected);
    Err(command.exec().into())
}
pub fn package_executable(source: &Path) -> Result<PathBuf> {
    let package = json_file(&source.join(PACKAGE))?;
    let executable = source.join("bin/flightdeck");
    let bytes = read(&executable, MAX_FILE)?;
    require(
        package["schema"] == 1
            && package["kind"] == "rust-launcher"
            && package["files"]["bin/flightdeck"] == files::sha256(&bytes)
            && bytes.starts_with(b"\x7fELF"),
        "Das native Flightdeck-Paket ist ungültig.",
    )?;
    Ok(executable)
}
