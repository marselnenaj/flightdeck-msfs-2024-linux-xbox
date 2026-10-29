// SPDX-License-Identifier: MIT
//! Pinned full-release inputs and the publication of a verified Actions build.
//! Only `publish` writes to GitHub. It requires the exact main-branch release
//! commit in GitHub Actions and never replaces a tag or an asset.
use crate::{
    components::{self, LIMITS, Members},
    package,
    util::{Failure, Result, digest, fail, pretty, read_regular, require, strict_json, version},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
    process::Command,
};

pub const REPOSITORY: &str = "marselnenaj/flightdeck-msfs-2024-linux-xbox";
pub const PACKAGE: &str = "Flightdeck-Linux-x86_64.tar.gz";
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;

/// What a release is built against: the previously published package for the
/// update and rollback test, and the corresponding source archives.
pub struct Pins {
    pub previous_base: &'static str,
    pub previous_sha: &'static str,
    /// The corresponding sources below describe exactly this native bundle.
    pub native_sha: &'static str,
    pub sources: BTreeMap<&'static str, &'static str>,
}
impl Pins {
    pub fn current() -> Self {
        Self {
            previous_base: "https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/releases/download/v0.3.4/",
            previous_sha: "1906680f03ec9235b3f5046c24c6064097c00f133707be7e5e6906e87161a29b",
            native_sha: "182c2ac5e43368faa6bf827a54fdae4ce649e770b10b5966b748b837f269cc6e",
            sources: [
                (
                    "flightdeck-native-sources-0.2.18.tar.gz",
                    "ce63fc52ff70d7d47d8e3e36232eb87f3f294ea6b0246715defb6c11a6673893",
                ),
                (
                    "flightdeck-dxvk-0.1.11-sources.tar.gz",
                    "f4cc973e7fa3816fb040a92f32799e33d7a7fb98c9e5032152a903bb5b234234",
                ),
                (
                    "flightdeck-vkd3d-0.1.17-sources.tar.gz",
                    "6f9f723b7c98ea01e42384f06fe36eef58d54ab32ea486cf8028d8c54029e07d",
                ),
            ]
            .into(),
        }
    }
}

pub struct Done {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The outside world of a release: the Actions environment, git and gh.
pub trait Host {
    fn variable(&self, name: &str) -> Option<String>;
    /// Runs a command in the checkout. Only `github` commands see the token.
    fn run(&self, command: &[&str], github: bool) -> Result<Done>;
}

pub struct Actions<'a>(pub &'a Path);
impl Host for Actions<'_> {
    fn variable(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn run(&self, command: &[&str], github: bool) -> Result<Done> {
        let mut process = Command::new(command[0]);
        process
            .args(&command[1..])
            .current_dir(self.0)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN");
        if github {
            let token = std::env::var("GH_TOKEN")
                .ok()
                .filter(|token| !token.is_empty())
                .ok_or_else(|| {
                    Failure("Publishing requires the workflow's ephemeral GH_TOKEN".into())
                })?;
            process.env("GH_TOKEN", token);
        }
        let result = process
            .output()
            .map_err(|error| Failure(format!("{}: {error}", command[0])))?;
        Ok(Done {
            code: result.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        })
    }
}

fn checked(host: &dyn Host, command: &[&str], github: bool) -> Result<String> {
    let done = host.run(command, github)?;
    require(
        done.code == 0,
        format!(
            "{} failed: {}",
            command[..command.len().min(3)].join(" "),
            done.stderr.trim()
        ),
    )?;
    Ok(done.stdout)
}

fn gh(host: &dyn Host, arguments: &[&str]) -> Result<Done> {
    let command = [&["gh", "release"], arguments, &["--repo", REPOSITORY]].concat();
    host.run(&command, true)
}

fn gh_checked(host: &dyn Host, arguments: &[&str]) -> Result<String> {
    let done = gh(host, arguments)?;
    require(
        done.code == 0,
        format!("gh release {} failed: {}", arguments[0], done.stderr.trim()),
    )?;
    Ok(done.stdout)
}

fn tag_refs(host: &dyn Host, tag: &str) -> Result<String> {
    let url = format!("https://github.com/{REPOSITORY}.git");
    let peeled = format!("refs/tags/{tag}^{{}}");
    let direct = format!("refs/tags/{tag}");
    Ok(
        checked(host, &["git", "ls-remote", &url, &direct, &peeled], false)?
            .trim()
            .to_string(),
    )
}

/// Only stable numeric launcher versions can be released.
pub fn stable_version(root: &Path) -> Result<String> {
    let value = version(root)?;
    let parts: Vec<&str> = value.split('.').collect();
    require(
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())),
        "Only stable numeric launcher versions can be released",
    )?;
    Ok(value)
}

/// Compares Git's actual subject; no commit message reaches a shell. A failed
/// pre-publication check can be corrected by a new release commit without
/// incrementing an unpublished version. A real tag is never reused.
pub fn eligible(host: &dyn Host, root: &Path) -> Result<bool> {
    if host.variable("GITHUB_EVENT_NAME").as_deref() != Some("push")
        || host.variable("GITHUB_REF").as_deref() != Some("refs/heads/main")
        || host.variable("GITHUB_REPOSITORY").as_deref() != Some(REPOSITORY)
    {
        return Ok(false);
    }
    let head = checked(host, &["git", "rev-parse", "HEAD"], false)?;
    if Some(head.trim()) != host.variable("GITHUB_SHA").as_deref() {
        return Ok(false);
    }
    let subject = checked(host, &["git", "log", "-1", "--format=%s"], false)?;
    let current = stable_version(root)?;
    Ok(subject.trim() == format!("Release Flightdeck {current}")
        && tag_refs(host, &format!("v{current}"))?.is_empty())
}

pub fn gate(host: &dyn Host, root: &Path) -> Result<()> {
    let output = format!(
        "eligible={}\nversion={}\n",
        eligible(host, root)?,
        stable_version(root)?
    );
    if let Some(path) = host.variable("GITHUB_OUTPUT") {
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?
            .write_all(output.as_bytes())?;
    }
    print!("{output}");
    Ok(())
}

fn checksum(path: &Path) -> Result<String> {
    Ok(digest(&read_regular(path, MAX_ARCHIVE)?))
}

fn download(url: &str, destination: &Path, expected: &str, offline: bool) -> Result<()> {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if fs::symlink_metadata(destination).is_ok() {
        return require(
            checksum(destination)? == expected,
            format!("Cached release input differs: {name}"),
        );
    }
    require(!offline, format!("Missing offline input: {name}"))?;
    require(
        url.starts_with(&format!(
            "https://github.com/{REPOSITORY}/releases/download/"
        )),
        "Release input URL must be from this repository",
    )?;
    let temporary = destination.with_file_name(format!("{name}.part"));
    require(
        fs::symlink_metadata(&temporary).is_err(),
        format!("An unfinished download exists: {name}.part"),
    )?;
    // Redirects stay on HTTPS, and a transient gateway error is retried.
    let status = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args([
            "--retry",
            "3",
            "--retry-delay",
            "5",
            "--connect-timeout",
            "60",
        ])
        .args(["--max-filesize", &MAX_ARCHIVE.to_string(), "--output"])
        .arg(&temporary)
        .arg(url)
        .status()
        .map_err(|error| Failure(format!("curl: {error}")))?;
    if !status.success() || checksum(&temporary)? != expected {
        let _ = fs::remove_file(&temporary);
        return fail(if status.success() {
            format!("Release checksum differs: {name}")
        } else {
            format!("Release download failed: {name}")
        });
    }
    fs::rename(temporary, destination)?;
    Ok(())
}

/// Every member of a full package, validated before anything is written.
pub fn package_contents(archive: &Path) -> Result<Members> {
    let data = read_regular(archive, MAX_ARCHIVE)?;
    let contents =
        components::regular_members(&data, package::PACKAGE_TOTAL_MAX, |name, contents| {
            require(
                name.starts_with("flightdeck-linux/")
                    && !contents.contains_key(name)
                    && contents.len() < package::PACKAGE_MEMBERS_MAX,
                "Unexpected full-package member",
            )?;
            Ok(LIMITS.file)
        })?;
    require(
        contents.contains_key("flightdeck-linux/FLIGHTDECK-PACKAGE.json"),
        "Missing native package manifest",
    )?;
    Ok(contents)
}

pub fn unpack(archive: &Path, destination: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let contents = package_contents(archive)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(destination)?;
    for (name, data) in &contents {
        let path = destination.join(name);
        fs::create_dir_all(path.parent().expect("member directory"))?;
        fs::write(&path, data)?;
        let relative = name.strip_prefix("flightdeck-linux/").unwrap_or(name);
        fs::set_permissions(&path, fs::Permissions::from_mode(package::mode(relative)))?;
    }
    Ok(())
}

pub fn prepare(root: &Path, pins: &Pins, cache: &Path, output: &Path, offline: bool) -> Result<()> {
    fs::create_dir_all(cache)?;
    let lock = strict_json(
        &read_regular(&root.join("compat/bootstrap.lock.json"), LIMITS.json)?,
        "Bootstrap lock",
        LIMITS.json,
    )?;
    let native = lock.get("native").cloned().unwrap_or_default();
    require(
        native["archive_sha256"] == pins.native_sha,
        "Update the corresponding-source pins when changing native components",
    )?;
    let url = native["archive_url"].as_str().unwrap_or_default();
    download(url, &cache.join("native.tar.gz"), pins.native_sha, offline)?;
    let archive = read_regular(&cache.join("native.tar.gz"), components::NATIVE_ARCHIVE_MAX)?;
    components::verify_native(&components::read_native(&archive, LIMITS)?, &native, LIMITS)?;
    let inputs = [(PACKAGE, pins.previous_sha)]
        .into_iter()
        .chain(pins.sources.iter().map(|(name, hash)| (*name, *hash)));
    for (name, hash) in inputs {
        download(
            &format!("{}{name}", pins.previous_base),
            &cache.join(name),
            hash,
            offline,
        )?;
    }
    unpack(&cache.join(PACKAGE), output)?;
    components::graphics_files(
        &read_regular(&root.join("compat/graphics.lock.json"), LIMITS.json)?,
        &output.join("flightdeck-linux/resources/graphics"),
        LIMITS,
    )?;
    println!("PASS: pinned native, graphics, rollback and corresponding-source inputs");
    Ok(())
}

fn asset_names(pins: &Pins, version: &str) -> BTreeSet<String> {
    [
        PACKAGE.to_string(),
        "Flightdeck-Linux-x86_64.zip".into(),
        format!("flightdeck-source-{version}.tar.gz"),
        "launcher-dependencies.json".into(),
        "THIRD-PARTY-NOTICES.txt".into(),
    ]
    .into_iter()
    .chain(pins.sources.keys().map(|name| name.to_string()))
    .collect()
}

fn listing(directory: &Path) -> Result<BTreeSet<String>> {
    fs::read_dir(directory)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect()
}

fn with(names: &BTreeSet<String>, extra: &[&str]) -> BTreeSet<String> {
    names
        .iter()
        .cloned()
        .chain(extra.iter().map(|name| name.to_string()))
        .collect()
}

fn sums(hashes: &BTreeMap<String, String>, names: &BTreeSet<String>) -> String {
    names
        .iter()
        .map(|name| format!("{}  {name}\n", hashes[name]))
        .collect()
}

/// Binds the tested assets and corresponding sources to a commit.
pub fn stage(
    root: &Path,
    pins: &Pins,
    assets: &Path,
    cache: &Path,
    check: &Path,
    commit: &str,
) -> Result<()> {
    require(
        commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Expected an exact source commit",
    )?;
    let result = Value::Object(strict_json(
        &read_regular(check, LIMITS.json)?,
        "Installation test",
        LIMITS.json,
    )?);
    require(
        result["status"] == "PASS" && result["checks"]["native_update_and_rollback"] == "PASS",
        "Native install/update/rollback check must pass before staging",
    )?;
    let package = package_contents(&assets.join(PACKAGE))?;
    let binary = package
        .get("flightdeck-linux/bin/flightdeck")
        .filter(|binary| !binary.is_empty());
    require(
        binary.is_some_and(|binary| result["native_binary_sha256"] == digest(binary)),
        "Installation check tested a different packaged executable",
    )?;
    for (name, expected) in &pins.sources {
        let source = read_regular(&cache.join(name), MAX_ARCHIVE)?;
        require(
            digest(&source) == *expected,
            "Corresponding source archive differs",
        )?;
        fs::File::create_new(assets.join(name))?.write_all(&source)?;
    }
    let version = stable_version(root)?;
    let names = asset_names(pins, &version);
    require(
        listing(assets)? == with(&names, &["SHA256SUMS"]),
        "Release directory contains missing or unexpected assets",
    )?;
    let mut hashes = BTreeMap::new();
    for name in &names {
        hashes.insert(name.clone(), checksum(&assets.join(name))?);
    }
    let manifest = json!({"schema": 1, "version": version, "commit": commit, "files": hashes,
        "native_installation_check": result});
    fs::write(assets.join("RELEASE-MANIFEST.json"), pretty(&manifest))?;
    hashes.insert(
        "RELEASE-MANIFEST.json".into(),
        checksum(&assets.join("RELEASE-MANIFEST.json"))?,
    );
    fs::write(
        assets.join("SHA256SUMS"),
        sums(&hashes, &with(&names, &["RELEASE-MANIFEST.json"])),
    )?;
    verify(root, pins, assets, commit)?;
    Ok(())
}

/// The complete, unchanged asset set of this version and commit, with hashes.
pub fn verify(
    root: &Path,
    pins: &Pins,
    directory: &Path,
    commit: &str,
) -> Result<BTreeMap<String, String>> {
    let version = stable_version(root)?;
    let names = asset_names(pins, &version);
    let expected = with(&names, &["SHA256SUMS", "RELEASE-MANIFEST.json"]);
    require(listing(directory)? == expected, "Unexpected release assets")?;
    let manifest = Value::Object(strict_json(
        &read_regular(&directory.join("RELEASE-MANIFEST.json"), LIMITS.json)?,
        "Release manifest",
        LIMITS.json,
    )?);
    let listed: Option<BTreeSet<String>> = manifest["files"]
        .as_object()
        .map(|files| files.keys().cloned().collect());
    require(
        manifest["schema"].as_u64() == Some(1)
            && manifest["commit"] == commit
            && manifest["version"] == version
            && listed.as_ref() == Some(&names),
        "Release manifest does not match this version and commit",
    )?;
    require(
        manifest["native_installation_check"]["checks"]["native_update_and_rollback"] == "PASS",
        "Release lacks a successful native update/rollback check",
    )?;
    let mut hashes = BTreeMap::new();
    for name in &expected {
        hashes.insert(name.clone(), checksum(&directory.join(name))?);
    }
    require(
        names
            .iter()
            .all(|name| manifest["files"][name] == hashes[name]),
        "Release assets changed after validation",
    )?;
    require(
        pins.sources
            .iter()
            .all(|(name, hash)| hashes[*name] == *hash),
        "Corresponding sources changed",
    )?;
    let wanted = sums(&hashes, &with(&names, &["RELEASE-MANIFEST.json"]));
    require(
        read_regular(&directory.join("SHA256SUMS"), LIMITS.json)? == wanted.as_bytes(),
        "Release checksums do not describe exactly these assets",
    )?;
    Ok(hashes)
}

fn clean_release_commit(host: &dyn Host, root: &Path) -> Result<bool> {
    Ok(eligible(host, root)?
        && checked(
            host,
            &["git", "status", "--porcelain", "--untracked-files=no"],
            false,
        )?
        .is_empty())
}

fn release_json(host: &dyn Host, arguments: &[&str]) -> Result<Value> {
    serde_json::from_str(&gh_checked(host, arguments)?)
        .map_err(|_| Failure("GitHub returned an unreadable release description".into()))
}

pub fn publish(
    host: &dyn Host,
    root: &Path,
    pins: &Pins,
    assets: &Path,
    notes: &Path,
    readback: &Path,
) -> Result<()> {
    require(
        clean_release_commit(host, root)?,
        "Publishing requires a clean, exact main-branch release commit in Actions",
    )?;
    let commit = host.variable("GITHUB_SHA").unwrap_or_default();
    let hashes = verify(root, pins, assets, &commit)?;
    let version = stable_version(root)?;
    let tag = format!("v{version}");
    require(
        tag_refs(host, &tag)?.is_empty(),
        "Tag already exists; never replace a published target",
    )?;
    let existing = gh(host, &["view", &tag, "--json", "tagName"])?;
    require(
        existing.code != 0 && existing.stderr.trim() == "release not found",
        "Release exists or its absence could not be verified",
    )?;
    // POST /git/refs creates exactly this ref or fails, including for an
    // existing ref. No PATCH, force push, delete or clobber is allowed.
    checked(
        host,
        &[
            "gh",
            "api",
            "--method",
            "POST",
            &format!("repos/{REPOSITORY}/git/refs"),
            "--raw-field",
            &format!("ref=refs/tags/{tag}"),
            "--raw-field",
            &format!("sha={commit}"),
        ],
        true,
    )?;
    // Any later failure leaves the exact tag and draft for review. A rerun
    // aborts; it never replaces an existing tag, draft or asset.
    let title = format!("Flightdeck {version}");
    let notes = notes.to_string_lossy();
    gh_checked(
        host,
        &[
            "create",
            &tag,
            "--draft",
            "--latest=false",
            "--verify-tag",
            "--target",
            &commit,
            "--title",
            &title,
            "--notes-file",
            &notes,
        ],
    )?;
    let paths: Vec<String> = hashes
        .keys()
        .map(|name| assets.join(name).to_string_lossy().into_owned())
        .collect();
    let upload: Vec<&str> = ["upload", &tag]
        .into_iter()
        .chain(paths.iter().map(String::as_str))
        .collect();
    gh_checked(host, &upload)?;
    let release = release_json(
        host,
        &[
            "view",
            &tag,
            "--json",
            "tagName,targetCommitish,isDraft,isPrerelease,assets",
        ],
    )?;
    let uploaded = release["assets"].as_array().cloned().unwrap_or_default();
    let mut inventory = uploaded.len() == hashes.len();
    for asset in &uploaded {
        let name = asset["name"].as_str().unwrap_or_default();
        inventory &= hashes.contains_key(name)
            && fs::metadata(assets.join(name))
                .is_ok_and(|info| asset["size"].as_u64() == Some(info.len()));
    }
    let named: BTreeSet<&str> = uploaded
        .iter()
        .filter_map(|asset| asset["name"].as_str())
        .collect();
    require(
        release["tagName"] == tag
            && release["targetCommitish"] == commit
            && release["isDraft"] == true
            && release["isPrerelease"] == false
            && inventory
            && named.len() == hashes.len(),
        "Draft target or asset inventory changed",
    )?;
    fs::create_dir_all(readback.parent().unwrap_or(Path::new(".")))?;
    fs::create_dir(readback)?;
    gh_checked(
        host,
        &["download", &tag, "--dir", &readback.to_string_lossy()],
    )?;
    require(
        verify(root, pins, readback, &commit)? == hashes,
        "Uploaded release bytes differ",
    )?;
    // A concurrent tag creation must not redirect publication to another commit.
    let refs = tag_refs(host, &tag)?;
    let targets: BTreeSet<&str> = refs
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    require(
        targets == BTreeSet::from([commit.as_str()]),
        "Release tag changed during draft verification",
    )?;
    gh_checked(
        host,
        &[
            "edit",
            &tag,
            "--draft=false",
            "--prerelease=false",
            "--latest",
        ],
    )?;
    let published = release_json(
        host,
        &[
            "view",
            &tag,
            "--json",
            "tagName,targetCommitish,isDraft,isPrerelease",
        ],
    )?;
    let latest = release_json(host, &["view", "--json", "tagName"])?;
    require(
        published["isDraft"] == false
            && published["isPrerelease"] == false
            && published["targetCommitish"] == commit
            && latest["tagName"] == tag,
        "Published release or Latest routing needs inspection",
    )?;
    println!("Published https://github.com/{REPOSITORY}/releases/tag/{tag}");
    Ok(())
}

/// The changelog section of one version, as the release body.
pub fn notes(changelog: &str, version: &str) -> Result<String> {
    let heading = format!("## {version} — ");
    let mut lines = changelog
        .lines()
        .skip_while(|line| !line.starts_with(&heading));
    require(
        lines.next().is_some(),
        "Release notes missing for the current version",
    )?;
    let body: Vec<&str> = lines.take_while(|line| !line.starts_with("## ")).collect();
    Ok(format!("{}\n", body.join("\n").trim()))
}

pub fn write_notes(root: &Path, output: &Path) -> Result<()> {
    let changelog = String::from_utf8(read_regular(
        &root.join("docs/changelog.md"),
        16 * LIMITS.json,
    )?)
    .map_err(|_| Failure("The changelog is not UTF-8".into()))?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, notes(&changelog, &stable_version(root)?)?)?;
    Ok(())
}

#[cfg(test)]
mod tests;
