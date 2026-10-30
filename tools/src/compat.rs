// SPDX-License-Identifier: MIT
//! The native compatibility components: staging their pinned, patched sources
//! and packaging the built binaries with notices and full rebuild sources.
//! Nothing here scans a game, prefix, home directory or account store; it
//! never uploads, creates a tag or modifies installed files.
use crate::{
    components::{NATIVE_FEATURES, NATIVE_FILES},
    util::{Failure, Result, digest, fail, pretty, require, version},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub const CLI_FEATURES: [&str; 4] = [
    "streaming-resume-files-v1",
    "package-info-json-v1",
    "streaming-integrity-index-v1",
    "streaming-progress-v1",
];
pub const HELPER_SOURCES: [&str; 5] = [
    "runtime/tools/ConnectedStorageProtocol.h",
    "runtime/tools/ConnectedStorageWrite.h",
    "runtime/tools/connected-storage.cpp",
    "runtime/tools/connected-storage-http.cpp",
    "runtime/tools/build-connected-storage.sh",
];

fn sha(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn json_file(path: &Path) -> Result<Value> {
    let data = fs::read(path).map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    Ok(serde_json::from_slice(&data)?)
}

fn regular(path: &Path) -> Result<fs::Metadata> {
    let info = fs::symlink_metadata(path)
        .map_err(|error| Failure(format!("{}: {error}", path.display())))?;
    require(info.is_file(), "Expected a regular build input")?;
    Ok(info)
}

/// The bytes of a build input, only if they still have the recorded hash.
fn frozen(path: &Path, expected: &str) -> Result<Vec<u8>> {
    regular(path)?;
    let content = fs::read(path)?;
    require(
        digest(&content) == expected,
        format!(
            "Build input changed before packaging: {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ),
    )?;
    Ok(content)
}

/// Every regular file below `root`, as sorted relative paths. Links and a
/// mutable Git checkout are refused.
fn files_below(root: &Path) -> Result<Vec<PathBuf>> {
    fn walk(root: &Path, directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let relative = path.strip_prefix(root).expect("below the root");
            require(
                !relative.components().any(|part| part.as_os_str() == ".git"),
                "Do not package a mutable Git checkout",
            )?;
            let kind = fs::symlink_metadata(&path)?.file_type();
            require(
                !kind.is_symlink(),
                format!("Unexpected source link: {}", relative.display()),
            )?;
            if kind.is_dir() {
                walk(root, &path, found)?;
            } else if kind.is_file() {
                found.push(relative.to_path_buf());
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    walk(root, root, &mut found)?;
    found.sort();
    Ok(found)
}

fn license_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let words = ["license", "licence", "copyright", "copying", "notice"];
    Ok(files_below(folder)?
        .into_iter()
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            words.iter().any(|word| name.contains(word))
        })
        .map(|path| folder.join(path))
        .collect())
}

pub struct Release<'a> {
    pub stage: &'a Path,
    pub vendor: &'a Path,
    pub vendor_config: &'a Path,
    pub graph: &'a Path,
    pub supplements: &'a Path,
    pub toolchain_notices: &'a [PathBuf],
    pub output: &'a Path,
}

/// The notice text of the native components and their dependency inventory.
/// The resolved graph includes build dependencies; retaining their notices is
/// intentionally conservative.
pub fn notices(root: &Path, job: &Release<'_>) -> Result<(Vec<u8>, Value)> {
    let version = version(root)?;
    let graph = json_file(job.graph)?;
    let supplements = json_file(&job.supplements.join("manifest.json"))?;
    let mut sections = vec![format!(
        "Flightdeck {version} native compatibility components\n\n\
         The launcher is MIT; native components retain their own licenses.\n\
         Xodus CLI/service: GPL-3.0-only. WineGDK builtin, Flightdeck proxy and ConnectedStorage helper: LGPL-2.1-or-later.\n\
         Complete corresponding sources, Cargo dependencies and rebuild scripts are supplied in\n\
         flightdeck-native-sources-{version}.tar.gz alongside this release.\n\
         You may modify and replace these components, including for debugging modifications.\n\
         The Proton runner, Linux system libraries and game are not contained in this package.\n"
    )];
    let rule = "=".repeat(72);
    let section = |label: &str, path: &Path| -> Result<String> {
        let data =
            fs::read(path).map_err(|error| Failure(format!("{}: {error}", path.display())))?;
        let text = String::from_utf8(data)
            .map_err(|_| Failure(format!("Notice is not UTF-8: {}", path.display())))?;
        Ok(format!("\n{rule}\n{label}\n{rule}\n{text}"))
    };
    let licenses = root.join("compat/LICENSES");
    let mut own: Vec<PathBuf> = fs::read_dir(&licenses)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    own.sort();
    // Subdirectories hold the launcher's pinned Rust licence texts, not these.
    for path in own.iter().filter(|path| path.is_file()) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        sections.push(section(
            &format!("Flightdeck compatibility / {name}"),
            path,
        )?);
    }
    sections.push(section(
        "Component provenance",
        &root.join("compat/THIRD_PARTY_NOTICES.md"),
    )?);
    let header = fs::read_to_string(root.join("compat/runtime/include/vendor/nlohmann_json.hpp"))?;
    let (comment, _) = header
        .split_once("*/")
        .ok_or_else(|| Failure("The bundled JSON header has no licence comment".into()))?;
    sections.push(format!("\nnlohmann JSON 3.6.1\n{comment}*/\n"));
    for name in [
        "LICENSE",
        "libs/xml2/COPYING",
        "libs/c++/LICENSE.TXT",
        "libs/c++abi/LICENSE.TXT",
        "libs/compiler-rt/LICENSE.TXT",
    ] {
        sections.push(section(
            &format!("Wine and bundled library / {name}"),
            &job.stage.join("wine-src").join(name),
        )?);
    }
    for path in job.toolchain_notices {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        sections.push(section(&format!("Build runtime / {name}"), path)?);
    }
    let mut inventory = Vec::new();
    for package in graph.as_array().into_iter().flatten() {
        let name = package["name"].as_str().unwrap_or_default();
        let package_version = package["version"].as_str().unwrap_or_default();
        let label = format!("{name} {package_version}");
        let folder = Path::new(package["manifest_path"].as_str().unwrap_or_default())
            .parent()
            .ok_or_else(|| Failure(format!("No manifest path: {label}")))?;
        // Other-target sources are vendored too, for the complete locked rebuild.
        let mut files = if package["source"].is_null() {
            // Imported libraries retain their own licenses even when Cargo
            // resolves them as local workspace packages.
            let license = folder.join("LICENSE");
            vec![if license.exists() {
                regular(&license)?;
                license
            } else {
                job.stage.join("xodus-src/LICENSE")
            }]
        } else {
            license_files(folder)?
        };
        for item in supplements["files"].as_array().into_iter().flatten() {
            if item["package"] != name || item["version"] != package_version {
                continue;
            }
            let path = job
                .supplements
                .join(item["path"].as_str().unwrap_or_default());
            let inside = fs::canonicalize(&path).is_ok_and(|path| {
                fs::canonicalize(job.supplements).is_ok_and(|base| path.starts_with(base))
            });
            require(
                inside && Some(sha(&path)?.as_str()) == item["sha256"].as_str(),
                format!("Invalid supplementary license: {label}"),
            )?;
            files.push(path);
        }
        require(
            !files.is_empty(),
            format!("Missing original license text: {label}"),
        )?;
        let mut included = Vec::new();
        for path in &files {
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            sections.push(section(&format!("{label} / {file}"), path)?);
            included.push(json!({"name": file, "sha256": sha(path)?}));
        }
        let license = package["license"]
            .as_str()
            .filter(|license| !license.is_empty())
            .unwrap_or(if package["source"].is_null() {
                "GPL-3.0-only"
            } else {
                "MIT (repository LICENSE)"
            });
        inventory.push(
            json!({"name": name, "version": package_version, "source": package["source"],
            "repository": package["repository"], "license": license, "notices": included}),
        );
    }
    Ok((sections.join("\n").into_bytes(), Value::Array(inventory)))
}

pub enum Source {
    Bytes(Vec<u8>),
    File(PathBuf),
}

/// An archive with stable names, timestamps and ownership, and without
/// absolute paths or links.
pub fn archive(output: &Path, files: &BTreeMap<String, Source>) -> Result<()> {
    let compressed = flate2::GzBuilder::new()
        .mtime(0)
        .write(fs::File::create_new(output)?, flate2::Compression::best());
    let mut tar = tar::Builder::new(compressed);
    for (name, value) in files {
        require(
            !name.starts_with('/')
                && !Path::new(name)
                    .components()
                    .any(|part| part == Component::ParentDir),
            "Invalid archive member",
        )?;
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mtime(0);
        match value {
            Source::Bytes(data) => {
                header.set_size(data.len() as u64);
                header.set_mode(if data.starts_with(b"#!") || name.starts_with("bin/") {
                    0o755
                } else {
                    0o644
                });
                tar.append_data(&mut header, name, &data[..])?;
            }
            Source::File(path) => {
                let info = fs::symlink_metadata(path)?;
                require(
                    info.is_file(),
                    format!("Unexpected source symlink or non-file: {name}"),
                )?;
                header.set_size(info.len());
                header.set_mode(if info.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                });
                tar.append_data(&mut header, name, fs::File::open(path)?.take(info.len()))?;
            }
        }
    }
    tar.into_inner()?.finish()?.flush()?;
    Ok(())
}

fn tree(root: &Path, prefix: &str, result: &mut BTreeMap<String, Source>) -> Result<()> {
    for relative in files_below(root)? {
        result.insert(
            format!("{prefix}/{}", relative.to_string_lossy()),
            Source::File(root.join(relative)),
        );
    }
    Ok(())
}

fn features(value: &Value, required: &[&str]) -> bool {
    let found: Option<BTreeSet<&str>> = value
        .as_array()
        .and_then(|items| items.iter().map(Value::as_str).collect());
    found.is_some_and(|found| required.iter().all(|name| found.contains(name)))
}

fn readme(version: &str) -> String {
    format!(
        "# Flightdeck {version} corresponding native sources\n\n\
This archive supplies the patched WineGDK source, patched Xodus CLI/service,\n\
Flightdeck native proxy and ConnectedStorage helper, all locked Cargo\n\
dependency sources and original notices.\n\
There are no game files, credentials, Wine prefixes or proprietary SDK inputs.\n\n\
Install the toolchain described in BUILDING.md. From this archive's root run:\n\n\
    CARGO_NET_OFFLINE=true BUILD_JOBS=8 bash ./build-compat.sh .\n\n\
The root .cargo/config.toml uses the included vendor directory. Host development\n\
packages and compilers remain external system prerequisites. The original build\n\
used Rust 1.98, GCC/MinGW 16.2 and WIDL from this Wine source. Output is artifacts/.\n\
For the 0.3.7 broker update, native-build.Dockerfile supplies Ubuntu 24.04\n\
prerequisites. Mount Rust 1.98 at /opt/rust and build xodus-cli/xodus-service\n\
with --release --locked --offline. See BUILDING.md for provider and retained ABI provenance.\n\
The build produces six binaries, including bin/flightdeck-connected-storage.exe.\n\
Its complete sources and build script are under runtime/tools/. It performs no\n\
authentication or cloud requests during a build. The helper provides scoped\n\
ConnectedStorage reads and lease-bound writes. The launcher coordinates automatic\n\
before-play/after-exit sync and advanced manual comparison, import and upload;\n\
the helper itself does not run an autonomous background sync service.\n\n\
You can modify/rebuild/replace the LGPL libraries, proxy, helper and GPL CLI/service;\n\
Flightdeck's advanced preparation accepts your rebuilt artifact manifest. No\n\
signature restriction prevents modified components. Keep applicable notices.\n\n\
The native executable build requires glibc 2.39 or newer, GTK 3, WebKitGTK 4.1,\n\
OpenSSL 3 and a Linux Secret Service provider. Linux system libraries and the\n\
separately downloaded upstream Proton runner are not distributed in this archive.\n"
    )
}

/// Packages built components with the given notice text and inventory.
pub fn create_with(
    root: &Path,
    job: &Release<'_>,
    notice: Vec<u8>,
    inventory: Value,
) -> Result<()> {
    if let Some(parent) = job.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(job.output)?;
    let version = version(root)?;
    let lock = json_file(&root.join("compat/bootstrap.lock.json"))?;
    let manifest = json_file(&job.stage.join("artifacts/manifest.json"))?;
    require(
        features(&manifest["features"], &NATIVE_FEATURES)
            && features(&lock["native"]["features"], &NATIVE_FEATURES),
        "Native build/lock is missing a required ConnectedStorage capability",
    )?;
    require(
        features(&manifest["cli_features"], &CLI_FEATURES)
            && features(&lock["native"]["cli_features"], &CLI_FEATURES),
        "Native build/lock is missing a required CLI capability",
    )?;
    let built = manifest["files"].as_object().cloned().unwrap_or_default();
    require(
        built.len() == NATIVE_FILES.len()
            && NATIVE_FILES.iter().all(|name| built.contains_key(*name))
            && manifest["files"] == lock["native"]["files"],
        "Build artifacts differ from the selected release",
    )?;
    require(
        Some(sha(&job.stage.join("source-manifest.json"))?.as_str())
            == manifest["source_manifest_sha256"].as_str(),
        "Source manifest does not belong to this build",
    )?;
    let source_manifest = json_file(&job.stage.join("source-manifest.json"))?;
    let listed = source_manifest["files"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    require(
        HELPER_SOURCES.iter().all(|name| listed.contains_key(*name)),
        "Corresponding ConnectedStorage helper sources are missing",
    )?;
    let mut checked = BTreeMap::new();
    for (name, checksum) in &listed {
        checked.insert(
            name.clone(),
            frozen(&job.stage.join(name), checksum.as_str().unwrap_or_default())?,
        );
    }
    let mut native = BTreeMap::new();
    for (name, checksum) in &built {
        let data = frozen(
            &job.stage.join("artifacts").join(name),
            checksum.as_str().unwrap_or_default(),
        )?;
        native.insert(name.clone(), Source::Bytes(data));
    }
    native.insert(
        "THIRD-PARTY-NOTICES.txt".into(),
        Source::Bytes(notice.clone()),
    );
    native.insert(
        "manifest.json".into(),
        Source::Bytes(pretty(&json!({"format": 1, "files": manifest["files"]}))),
    );
    let artifact = job
        .output
        .join(format!("flightdeck-compat-{version}-linux-x86_64.tar.gz"));
    archive(&artifact, &native)?;
    fs::write(job.output.join("THIRD-PARTY-NOTICES.txt"), &notice)?;
    fs::write(
        job.output.join("dependency-inventory.json"),
        pretty(&inventory),
    )?;
    let mut sources = BTreeMap::new();
    tree(&job.stage.join("wine-src"), "wine-src", &mut sources)?;
    tree(&job.stage.join("xodus-src"), "xodus-src", &mut sources)?;
    tree(job.vendor, "vendor", &mut sources)?;
    tree(&root.join("compat/LICENSES"), "LICENSES", &mut sources)?;
    tree(job.supplements, "license-supplements", &mut sources)?;
    // Only the original inputs of this exact build's manifest are sources;
    // generated DLLs and later edits to the working tree are not.
    for (name, data) in checked {
        sources.insert(name, Source::Bytes(data));
    }
    let file = |path: PathBuf| Source::File(path);
    sources.insert(
        "source-manifest.json".into(),
        file(job.stage.join("source-manifest.json")),
    );
    sources.insert(
        "build-compat.sh".into(),
        file(root.join("scripts/build-compat.sh")),
    );
    let config = String::from_utf8(fs::read(job.vendor_config)?)
        .map_err(|_| Failure("The vendor configuration is not UTF-8".into()))?;
    let directory = regex::Regex::new(r#"directory = "[^"]+""#).expect("pattern");
    sources.insert(
        ".cargo/config.toml".into(),
        Source::Bytes(
            directory
                .replace_all(&config, r#"directory = "vendor""#)
                .into_owned()
                .into_bytes(),
        ),
    );
    sources.insert(
        "THIRD-PARTY-NOTICES.txt".into(),
        Source::Bytes(notice.clone()),
    );
    sources.insert(
        "dependency-inventory.json".into(),
        file(job.output.join("dependency-inventory.json")),
    );
    sources.insert("BUILDING.md".into(), file(root.join("BUILDING.md")));
    sources.insert(
        "native-build.Dockerfile".into(),
        file(root.join("compat/native-build.Dockerfile")),
    );
    sources.insert(
        "README.md".into(),
        Source::Bytes(readme(&version).into_bytes()),
    );
    let source_archive = job
        .output
        .join(format!("flightdeck-native-sources-{version}.tar.gz"));
    archive(&source_archive, &sources)?;
    let result = json!({"format": 1, "native_sha256": sha(&artifact)?, "notice_sha256": digest(&notice),
        "source_sha256": sha(&source_archive)?, "source_files": sources.len(),
        "dependency_packages": inventory.as_array().map_or(0, Vec::len), "artifacts": manifest["files"]});
    fs::write(job.output.join("validation.json"), pretty(&result))?;
    println!("{result}");
    Ok(())
}

pub fn create(root: &Path, job: &Release<'_>) -> Result<()> {
    let (notice, inventory) = notices(root, job)?;
    create_with(root, job, notice, inventory)
}

/// Extracts a tar stream below `destination`. Only regular files, directories
/// and relative links that stay inside are accepted.
fn unpack(stream: impl Read, destination: &Path, strip_root: bool) -> Result<()> {
    let mut roots = BTreeSet::new();
    let mut archive = tar::Archive::new(stream);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        // An archive of a Git commit starts with a global header naming it.
        if matches!(
            kind,
            tar::EntryType::XGlobalHeader | tar::EntryType::XHeader
        ) {
            continue;
        }
        let path = entry.path()?.into_owned();
        let mut parts: Vec<_> = path
            .components()
            .map(|part| match part {
                Component::Normal(name) => Ok(name.to_os_string()),
                Component::CurDir => Ok(Default::default()),
                _ => fail("Archive member leaves its destination"),
            })
            .collect::<Result<_>>()?;
        parts.retain(|part| !part.is_empty());
        if strip_root && !parts.is_empty() {
            roots.insert(parts.remove(0));
            require(
                roots.len() == 1,
                "Archive must contain exactly one root directory",
            )?;
        }
        if parts.is_empty() {
            continue;
        }
        let target = parts
            .iter()
            .fold(destination.to_path_buf(), |path, part| path.join(part));
        let depth = parts.len() - 1;
        match kind {
            tar::EntryType::Directory => fs::create_dir_all(&target)?,
            tar::EntryType::Regular => {
                fs::create_dir_all(target.parent().expect("member directory"))?;
                let mut file = fs::File::create_new(&target)?;
                std::io::copy(&mut entry, &mut file)?;
                let executable = entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0);
                fs::set_permissions(
                    &target,
                    fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
                )?;
            }
            tar::EntryType::Symlink => {
                let link = entry
                    .link_name()?
                    .ok_or_else(|| Failure("Archive link has no target".into()))?
                    .into_owned();
                let mut level = depth as i64;
                for part in link.components() {
                    match part {
                        Component::Normal(_) => level += 1,
                        Component::ParentDir => level -= 1,
                        Component::CurDir => {}
                        _ => return fail("Archive link leaves its destination"),
                    }
                    require(level >= 0, "Archive link leaves its destination")?;
                }
                fs::create_dir_all(target.parent().expect("member directory"))?;
                std::os::unix::fs::symlink(link, &target)?;
            }
            _ => return fail("Archive contains an unsupported member type"),
        }
    }
    Ok(())
}

fn git(arguments: &[&str], directory: &Path, ceiling: Option<&Path>) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command.args(arguments).current_dir(directory);
    if let Some(ceiling) = ceiling {
        // An enclosing checkout must not be inherited when staging under build/.
        command.env("GIT_CEILING_DIRECTORIES", ceiling);
    }
    let result = command
        .output()
        .map_err(|error| Failure(format!("git: {error}")))?;
    require(
        result.status.success(),
        format!(
            "git {} failed: {}",
            arguments[0],
            String::from_utf8_lossy(&result.stderr).trim()
        ),
    )?;
    Ok(result.stdout)
}

/// Verifies the pinned upstream inputs and applies the reviewed patches into
/// a new source staging directory. The supplied repository is not modified.
pub fn stage(
    root: &Path,
    winegdk_archive: &Path,
    xodus_repo: &Path,
    destination: &Path,
) -> Result<()> {
    let lock = json_file(&root.join("compat/upstreams.lock.json"))?;
    let destination = std::path::absolute(destination)?;
    require(
        fs::symlink_metadata(&destination).is_err(),
        "Destination already exists; choose a new directory",
    )?;
    let wine_archive = fs::canonicalize(winegdk_archive)?;
    require(
        Some(sha(&wine_archive)?.as_str()) == lock["winegdk"]["archive_sha256"].as_str(),
        "WineGDK archive checksum mismatch",
    )?;
    let xodus_repo = fs::canonicalize(xodus_repo)?;
    let revision = lock["xodus"]["revision"].as_str().unwrap_or_default();
    let actual = git(
        &["rev-parse", &format!("{revision}^{{commit}}")],
        &xodus_repo,
        None,
    )?;
    require(
        String::from_utf8_lossy(&actual).trim() == revision && !revision.is_empty(),
        "Xodus revision does not match lock",
    )?;
    // git archive reads the exact commit, never the caller's mutable worktree.
    let xodus_archive = git(&["archive", revision], &xodus_repo, None)?;
    let (wine_source, xodus_source) = (destination.join("wine-src"), destination.join("xodus-src"));
    fs::create_dir_all(&wine_source)?;
    fs::create_dir(&xodus_source)?;
    unpack(
        flate2::read::GzDecoder::new(fs::File::open(&wine_archive)?),
        &wine_source,
        true,
    )?;
    unpack(&xodus_archive[..], &xodus_source, false)?;
    for (component, target) in [("winegdk", &wine_source), ("xodus", &xodus_source)] {
        let patch = root.join(lock[component]["patch"].as_str().unwrap_or_default());
        require(
            Some(sha(&patch)?.as_str()) == lock[component]["patch_sha256"].as_str(),
            format!("{component} patch checksum mismatch"),
        )?;
        // The small login patch uses zero-context hunks to avoid whitespace-only
        // context lines in the shipped patch. Both the pristine upstream revision
        // and every resulting source file are checked against pinned digests.
        let patch = patch.to_string_lossy();
        git(
            &["apply", "--unidiff-zero", "--check", &patch],
            target,
            target.parent(),
        )?;
        git(
            &["apply", "--unidiff-zero", &patch],
            target,
            target.parent(),
        )?;
    }
    let runtime = destination.join("runtime");
    for relative in files_below(&root.join("compat/runtime"))? {
        let target = runtime.join(&relative);
        fs::create_dir_all(target.parent().expect("runtime directory"))?;
        fs::copy(root.join("compat/runtime").join(&relative), target)?;
    }
    let expected = json_file(&root.join("compat/source-deltas.json"))?;
    // The helper ships as a sixth binary; its full source and build inputs must
    // belong to the reviewed source delta, not merely to an untracked worktree.
    for name in HELPER_SOURCES {
        let name = name.strip_prefix("runtime/").unwrap_or(name);
        require(
            expected["runtime_source_files"].get(name).is_some(),
            format!("ConnectedStorage helper source is missing from the reviewed delta: {name}"),
        )?;
    }
    for (key, target) in [
        ("winegdk_patched_files", &wine_source),
        ("xodus_patched_files", &xodus_source),
        ("runtime_source_files", &runtime),
    ] {
        for (relative, checksum) in expected[key].as_object().into_iter().flatten() {
            require(
                Some(sha(&target.join(relative))?.as_str()) == checksum.as_str(),
                format!("Staged source mismatch: {relative}"),
            )?;
        }
    }
    let mut files = BTreeMap::new();
    for component in ["wine-src/dlls/xgameruntime", "xodus-src", "runtime"] {
        fn walk(base: &Path, directory: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                let kind = fs::symlink_metadata(&path)?.file_type();
                if kind.is_dir() {
                    walk(base, &path, files)?;
                } else if kind.is_file() {
                    let relative = path.strip_prefix(base).expect("staged file");
                    files.insert(relative.to_string_lossy().into_owned(), sha(&path)?);
                }
            }
            Ok(())
        }
        walk(&destination, &destination.join(component), &mut files)?;
    }
    let manifest = json!({"format": 1, "upstreams": lock, "files": files});
    let path = destination.join("source-manifest.json");
    fs::write(&path, pretty(&manifest))?;
    println!(
        "{}",
        json!({"status": "PASS", "files": files.len(), "manifest": path})
    );
    Ok(())
}

#[cfg(test)]
mod tests;
