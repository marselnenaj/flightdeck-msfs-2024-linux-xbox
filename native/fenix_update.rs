// SPDX-License-Identifier: MIT
//! Independent, bounded Fenix release discovery. Installation keeps using the
//! launcher's recoverable Rust transaction, never the downloaded installer.
use crate::{
    Error, Result,
    backend::{Context, Launcher, string},
    error::require,
    fenix, fenix_bundle as bundle, files, launcher_update, resources, runtime, transaction as tx,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const PROJECT: &str = "https://github.com/marselnenaj/fenix-a320-linux-patch";
pub const API: &str =
    "https://api.github.com/repos/marselnenaj/fenix-a320-linux-patch/releases?per_page=50";
pub const ASSET: &str = "flightdeck-update.json";
const INVALID: &str =
    "Die Fenix-Updatebeschreibung ist ungültig. Der installierte Patch bleibt erhalten.";
const UPGRADE: &str = "Für diesen Fenix-Patch ist zuerst ein Flightdeck-Update erforderlich.";
const NETWORK: &str =
    "Fenix-Updates konnten nicht geprüft werden. Verbindung prüfen und erneut versuchen.";
const LIMIT: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub struct State {
    pub baseline: String,
    pub attempted: Option<Instant>,
    pub worker: Option<Arc<AtomicBool>>,
    pub status: Value,
    pub candidate: Value,
}

/// Release ordering includes numbered previews; a stable version always wins
/// over previews with the same base version.
pub fn version(text: &str) -> Result<([u32; 3], u32)> {
    let text = text.strip_prefix('v').unwrap_or(text);
    let (base, preview) = match text.split_once("-preview.") {
        Some((base, n)) => {
            require(
                !n.is_empty()
                    && n.len() <= 6
                    && !n.starts_with('0')
                    && n.bytes().all(|b| b.is_ascii_digit()),
                INVALID,
            )?;
            (base, n.parse::<u32>().map_err(|_| Error::Invalid(INVALID))?)
        }
        None => (text, u32::MAX),
    };
    Ok((launcher_update::version(base)?, preview))
}

fn open(url: &str) -> Result<reqwest::blocking::Response> {
    require(launcher_update::safe_url(url), INVALID)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(if url.ends_with(".zip") {
            600
        } else {
            30
        }))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 5 && launcher_update::safe_url(attempt.url().as_str()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|_| Error::Invalid(NETWORK))?;
    let response = client
        .get(url)
        .header("User-Agent", format!("Flightdeck/{}", crate::VERSION))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| Error::Invalid(NETWORK))?;
    require(response.status().is_success(), NETWORK)?;
    Ok(response)
}

fn read(url: &str, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let mut response = open(url)?;
    let mut bytes = Vec::new();
    let mut buffer = [0; 16384];
    loop {
        tx::interrupted(cancel)?;
        let n = response
            .read(&mut buffer)
            .map_err(|_| Error::Invalid(NETWORK))?;
        if n == 0 {
            break;
        }
        require(bytes.len() + n <= LIMIT, INVALID)?;
        bytes.extend_from_slice(&buffer[..n]);
    }
    Ok(bytes)
}

pub fn download(release: &Value, target: &Path, ctx: &Context) -> Result<PathBuf> {
    let Some(size) = release["size"].as_u64() else {
        return crate::bootstrap::download(
            string(release, "url")?,
            string(release, "sha256")?,
            target,
            &ctx.cancel,
            |transfer| ctx.update(json!({"transfer":transfer})),
        );
    };
    let hash = string(release, "sha256")?;
    require(
        size > 0 && size <= 512 * 1024 * 1024 && files::hex_digest(hash),
        INVALID,
    )?;
    tx::interrupted(&ctx.cancel)?;
    if tx::digest(target).is_ok_and(|v| v == hash) && target.metadata()?.len() == size {
        return Ok(target.into());
    }
    let parent = target.parent().ok_or(Error::Invalid(INVALID))?;
    files::private_dir(parent)?;
    let temp = parent.join(format!(".fenix-download-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut response = open(string(release, "url")?)?;
        require(response.content_length().is_none_or(|v| v == size), INVALID)?;
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        let mut received = 0u64;
        let mut buffer = [0; 128 * 1024];
        let mut last = Instant::now();
        loop {
            tx::interrupted(&ctx.cancel)?;
            let n = response
                .read(&mut buffer)
                .map_err(|_| Error::Invalid(NETWORK))?;
            if n == 0 {
                break;
            }
            received += n as u64;
            require(received <= size, INVALID)?;
            out.write_all(&buffer[..n])?;
            if last.elapsed() >= Duration::from_millis(500) {
                ctx.update(json!({"transfer":{"kind":"fenix","received_bytes":received,"total_bytes":size}}));
                last = Instant::now();
            }
        }
        out.sync_all()?;
        require(received == size && tx::digest(&temp)? == hash, INVALID)?;
        tx::interrupted(&ctx.cancel)?;
        fs::rename(&temp, target)?;
        files::directory(parent, true)?.sync_all()?;
        ctx.update(json!({"transfer":{"kind":"fenix","received_bytes":size,"total_bytes":size,"verified_bytes":size}}));
        Ok(target.to_path_buf())
    })();
    let _ = fs::remove_file(temp);
    result
}

/// Select only published releases advertising this explicit installer contract.
/// Preview installations follow previews until the first stable release.
pub fn release_metadata(feed: &Value, current: &str) -> Result<Option<Value>> {
    let current = version(current)?;
    let entries = feed.as_array().ok_or(Error::Invalid(INVALID))?;
    require(entries.len() <= 50, INVALID)?;
    let mut candidates = Vec::new();
    for raw in entries {
        if raw["draft"] != false {
            continue;
        }
        let Some(tag) = raw["tag_name"].as_str() else {
            continue;
        };
        let Ok(number) = version(tag) else {
            continue;
        };
        if number <= current || (current.1 == u32::MAX && number.1 != u32::MAX) {
            continue;
        }
        require(raw["prerelease"] == (number.1 != u32::MAX), INVALID)?;
        let assets: Vec<_> = raw["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| a["name"] == ASSET)
            .collect();
        if assets.is_empty() {
            continue;
        }
        require(assets.len() == 1, INVALID)?;
        let asset = assets[0];
        let digest = string(asset, "digest")?
            .strip_prefix("sha256:")
            .ok_or(Error::Invalid(INVALID))?;
        let url = format!("{PROJECT}/releases/download/{tag}/{ASSET}");
        require(
            asset["browser_download_url"] == url
                && asset["state"] == "uploaded"
                && asset["size"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= LIMIT as u64)
                && files::hex_digest(digest),
            INVALID,
        )?;
        candidates.push((number, json!({"version":tag.trim_start_matches('v'),"tag":tag,"url":url,"sha256":digest,"size":asset["size"]})));
    }
    candidates.sort_by_key(|v| (v.0.1 == u32::MAX, v.0));
    if let Some(last) = candidates.last() {
        require(
            candidates.iter().filter(|v| v.0 == last.0).count() == 1,
            INVALID,
        )?;
    }
    Ok(candidates.pop().map(|v| v.1))
}

/// A contract may replace hashes, but cannot add filesystem destinations,
/// change runner identities or silently change the launcher's setup policy.
pub fn compatible_manifest(lock: &Value) -> Result<()> {
    let embedded = bundle::manifest(None)?;
    require(
        lock["format"] == 1 && lock["repository"] == PROJECT,
        INVALID,
    )?;
    version(string(lock, "version")?)?;
    let keys = |v: &Value| v.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>());
    require(
        keys(&lock["variants"]) == keys(&embedded["variants"]),
        UPGRADE,
    )?;
    for variant in std::iter::once(None).chain(
        embedded["variants"]
            .as_object()
            .into_iter()
            .flat_map(|v| v.keys())
            .map(|v| Some(v.as_str())),
    ) {
        let actual = bundle::select_variant(lock.clone(), variant)?;
        let known = bundle::manifest(variant)?;
        for key in [
            "runner_files",
            "runner_version",
            "protocol",
            "display_renderer",
        ] {
            require(actual[key] == known[key], UPGRADE)?;
        }
        for key in ["files", "integration", "prefix_files"] {
            require(keys(&actual[key]) == keys(&known[key]), UPGRADE)?;
            require(
                actual[key].as_object().is_some_and(|m| {
                    m.values()
                        .all(|v| v.as_str().is_some_and(files::hex_digest))
                }),
                INVALID,
            )?;
        }
        require(
            actual["prefix_files"][crate::fenix_setup::GEOMETRY_PATH]
                == known["prefix_files"][crate::fenix_setup::GEOMETRY_PATH],
            UPGRADE,
        )?;
        for helper in [
            "FenixWindowGuard.exe",
            "FenixMCDURefresh.exe",
            "fenix-runtime",
        ] {
            require(
                actual["prefix_files"][format!("drive_c/windows/system32/{helper}")]
                    == actual["integration"][helper],
                INVALID,
            )?;
        }
        let previous = actual["previous_releases"]
            .as_object()
            .ok_or(Error::Invalid(INVALID))?;
        require(previous.len() <= 128, INVALID)?;
        for (v, snapshot) in known["previous_releases"].as_object().into_iter().flatten() {
            if version(v)? < version(string(lock, "version")?)? {
                require(previous.get(v) == Some(snapshot), UPGRADE)?;
            }
        }
        if version(string(lock, "version")?)? > version(string(&known, "version")?)? {
            let snapshot = previous
                .get(string(&known, "version")?)
                .ok_or(Error::Invalid(UPGRADE))?;
            require(
                snapshot["files"] == known["files"]
                    && snapshot["integration"] == known["integration"],
                UPGRADE,
            )?;
        }
    }
    Ok(())
}

pub fn descriptor(bytes: &[u8], metadata: &Value) -> Result<Value> {
    require(
        bytes.len() <= LIMIT
            && metadata["size"] == bytes.len() as u64
            && metadata["sha256"] == files::sha256(bytes),
        INVALID,
    )?;
    let value = crate::strict_json::decode(bytes).map_err(|_| Error::Invalid(INVALID))?;
    require(
        value["format"] == 1 && matches!(value["installer_contract"].as_u64(), Some(1 | 2)),
        UPGRADE,
    )?;
    require(
        launcher_update::version(string(&value, "minimum_flightdeck_version")?)?
            <= launcher_update::version(crate::VERSION)?,
        UPGRADE,
    )?;
    let release = &value["release"];
    let v = string(metadata, "version")?;
    require(
        release["version"] == v
            && value["bundle"]["version"] == v
            && release["url"]
                == format!(
                    "{PROJECT}/releases/download/v{v}/fenix-a320-linux-patch-{v}-linux-x86_64.zip"
                )
            && release["archive_root"] == format!("fenix-a320-linux-patch-{v}")
            && release["sha256"].as_str().is_some_and(files::hex_digest)
            && release["size"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= 512 * 1024 * 1024),
        INVALID,
    )?;
    compatible_manifest(&value["bundle"])?;
    Ok(value)
}

pub fn discover(current: &str, cancel: &AtomicBool) -> Result<Option<Value>> {
    let bytes = read(API, cancel)?;
    let feed = crate::strict_json::decode(&bytes).map_err(|_| Error::Invalid(INVALID))?;
    let Some(metadata) = release_metadata(&feed, current)? else {
        return Ok(None);
    };
    let data = read(string(&metadata, "url")?, cancel)?;
    Ok(Some(descriptor(&data, &metadata)?))
}

pub fn installed_manifest(state: &Value, variant: Option<&str>) -> Result<Value> {
    if state["bundle_manifest"].is_null() {
        return bundle::manifest(variant);
    }
    // Use the installation's retained identities even after a later launcher
    // changes its own embedded runner pins. Discovery already checked this
    // snapshot against the installer contract before writing the private state.
    let snapshot = &state["bundle_manifest"];
    require(
        snapshot["format"] == 1 && snapshot["repository"] == PROJECT,
        INVALID,
    )?;
    require(
        state["version"] == state["bundle_manifest"]["version"],
        INVALID,
    )?;
    version(string(state, "version")?)?;
    let lock = bundle::select_variant(snapshot.clone(), variant)?;
    for key in ["files", "runner_files", "prefix_files", "integration"] {
        let map = lock[key].as_object().ok_or(Error::Invalid(INVALID))?;
        require(
            !map.is_empty()
                && map.len() <= 256
                && map.iter().all(|(name, hash)| {
                    files::relative(name) && hash.as_str().is_some_and(files::hex_digest)
                }),
            INVALID,
        )?;
    }
    Ok(lock)
}

/// Never downgrade an installed independent update, including after a launcher
/// restart or when GitHub is offline.
pub fn selected(ctx: &Context) -> Result<Value> {
    let mut selected = json!({"bundle":bundle::manifest(None)?,"release":resources::json("compat/fenix/release.json")?});
    if let Some(item) = ctx.launcher.lock().steam.clone() {
        let record = crate::steam_fenix::recorded(&ctx.launcher.state_dir, &item)?;
        if version(string(&record, "version")?)?
            > version(string(&selected["release"], "version")?)?
        {
            selected =
                json!({"bundle":record["bundle_manifest"],"release":record["bundle_release"]});
        }
    }
    if let Some(root) = &ctx.runtime
        && files::exists(&root.join(fenix::MARKER))
    {
        let state = runtime::value(&root.join(fenix::MARKER))?;
        if state["state"] == "installed"
            && !state["bundle_manifest"].is_null()
            && version(string(&state, "version")?)?
                > version(string(&selected["release"], "version")?)?
        {
            installed_manifest(&state, state["variant"].as_str())?;
            selected = json!({"bundle":state["bundle_manifest"],"release":state["bundle_release"]});
        }
    }
    let candidate = ctx.launcher.lock().fenix_updates.candidate.clone();
    if !candidate.is_null()
        && version(string(&candidate["release"], "version")?)?
            > version(string(&selected["release"], "version")?)?
        && (version(string(&selected["release"], "version")?)?.1 != u32::MAX
            || version(string(&candidate["release"], "version")?)?.1 == u32::MAX)
    {
        selected = candidate;
    }
    Ok(selected)
}

fn retry_due(state: &State, now: Instant) -> bool {
    let seconds = if state.status["state"] == "failed" {
        300
    } else {
        1800
    };
    state.worker.is_none()
        && state
            .attempted
            .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(seconds))
}

fn has_patch(root: &Path) -> bool {
    files::exists(&root.join(fenix::MARKER))
        || files::exists(&root.join(crate::fenix_legacy::MARKER))
}

pub fn check(app: &Arc<Launcher>, force: bool) -> Result<Value> {
    let mut s = app.lock();
    Launcher::open(&s)?;
    if !force && s.steam.is_none() && !s.runtime.as_ref().is_some_and(|root| has_patch(root)) {
        return Ok(json!({"ok":true}));
    }
    let mut current = string(&bundle::manifest(None)?, "version")?.to_string();
    if let Some(item) = &s.steam {
        current = string(
            &crate::steam_fenix::recorded(&app.state_dir, item)?,
            "version",
        )?
        .to_string();
    }
    if let Some(root) = s.runtime.as_ref().filter(|_| s.steam.is_none())
        && let Ok(state) = runtime::value(&root.join(fenix::MARKER))
        && state["state"] == "installed"
        && let Some(installed) = state["version"].as_str()
        && let Ok(number) = version(installed)
        && number.1 == u32::MAX
        && number > version(&current)?
    {
        current = installed.into();
    }
    if s.fenix_updates.worker.is_some()
        || (!force
            && s.fenix_updates.baseline == current
            && !retry_due(&s.fenix_updates, Instant::now()))
    {
        return Ok(json!({"ok":true}));
    }
    s.fenix_updates.baseline = current.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    s.fenix_updates.worker = Some(Arc::clone(&cancel));
    s.fenix_updates.attempted = Some(Instant::now());
    s.fenix_updates.status = json!({"state":"checking"});
    drop(s);
    let app = Arc::clone(app);
    std::thread::spawn(move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Option<Value>> {
                discover(&current, &cancel)
            }))
            .unwrap_or(Err(Error::Invalid(NETWORK)));
        let mut s = app.lock();
        if !s.closing && !cancel.load(Ordering::Relaxed) {
            match result {
                Ok(candidate) => {
                    s.fenix_updates.candidate = candidate.unwrap_or(Value::Null);
                    s.fenix_updates.status = json!({"state":"complete","checked_at":files::now()});
                }
                Err(error) => {
                    s.fenix_updates.status = json!({"state":"failed","requires_launcher_update":matches!(error, Error::Invalid(UPGRADE)),"error":error.to_string(),"checked_at":files::now()})
                }
            }
        }
        s.fenix_updates.worker = None;
    });
    Ok(json!({"ok":true}))
}

pub fn ensure(ctx: &Context) -> Result<bool> {
    ctx.interrupted()?;
    let Some(root) = &ctx.runtime else {
        return Ok(false);
    };
    if !files::exists(&root.join(fenix::MARKER)) {
        if files::exists(&root.join(crate::fenix_legacy::MARKER))
            && crate::fenix_legacy::automatic_allowed(root)
        {
            let target = selected(ctx)?;
            if crate::fenix_legacy::inspect(root, &target["bundle"]).is_ok() {
                let payload =
                    bundle::obtain(&ctx.launcher.state_dir.join("fenix-bundles"), None, ctx)?;
                fenix::install(ctx, &payload)?;
                ctx.interrupted()?;
                return Ok(true);
            }
        }
        return Ok(false);
    }
    let state = runtime::value(&root.join(fenix::MARKER))?;
    if state["state"] != "installed" {
        return Ok(false);
    }
    let target = selected(ctx)?;
    if version(string(&target["bundle"], "version")?)? <= version(string(&state, "version")?)? {
        return Ok(false);
    }
    let lock = bundle::select_variant(target["bundle"].clone(), state["variant"].as_str())?;
    require(
        lock["previous_releases"]
            .get(string(&state, "version")?)
            .is_some(),
        UPGRADE,
    )?;
    let installed = installed_manifest(&state, state["variant"].as_str())?;
    let previous = if installed["version"] == state["version"] {
        &installed
    } else {
        &installed["previous_releases"][string(&state, "version")?]
    };
    let accepted = &lock["previous_releases"][string(&state, "version")?];
    require(
        accepted["files"] == previous["files"]
            && accepted["integration"] == previous["integration"],
        UPGRADE,
    )?;
    let payload = bundle::obtain(&ctx.launcher.state_dir.join("fenix-bundles"), None, ctx)?;
    fenix::install(ctx, &payload)?;
    ctx.interrupted()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_discovery_includes_existing_local_installations() {
        let temp = tempfile::tempdir().expect("runtime");
        assert!(!has_patch(temp.path()));
        files::private_dir(&temp.path().join("private")).expect("private metadata");
        files::atomic_json(&temp.path().join(crate::fenix_legacy::MARKER), &json!({}))
            .expect("legacy record");
        assert!(
            has_patch(temp.path()),
            "local installations need automatic checks too"
        );
        fs::remove_file(temp.path().join(crate::fenix_legacy::MARKER)).expect("remove legacy");
        files::atomic_json(&temp.path().join(fenix::MARKER), &json!({})).expect("managed record");
        assert!(has_patch(temp.path()));
    }
    #[test]
    fn failed_discovery_retries_after_five_minutes_and_success_after_thirty() {
        let at = Instant::now();
        let mut state = State {
            attempted: Some(at),
            status: json!({"state":"failed"}),
            ..State::default()
        };
        assert!(!retry_due(&state, at + Duration::from_secs(299)));
        assert!(retry_due(&state, at + Duration::from_secs(300)));
        state.status = json!({"state":"complete"});
        assert!(!retry_due(&state, at + Duration::from_secs(1799)));
        assert!(retry_due(&state, at + Duration::from_secs(1800)));
        state.worker = Some(Arc::new(AtomicBool::new(false)));
        assert!(!retry_due(&state, at + Duration::from_secs(3600)));
    }
}
