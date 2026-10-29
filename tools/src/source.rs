// SPDX-License-Identifier: MIT
//! The source release: an explicit allowlist, inspected for private material,
//! and an archive written only from the bytes that passed the inspection.
use crate::util::{Failure, Result, digest, fail, pretty, require};
use base64::Engine;
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const DIRECTORIES: [&str; 9] = [
    "compat", "scripts", "docs", "tests", "examples", "native", "tools", "ui", ".github",
];
pub const TOP_LEVEL: [&str; 8] = [
    "README.md",
    "BUILDING.md",
    "LICENSE",
    ".gitignore",
    "install.sh",
    "Install Flightdeck.desktop",
    "Cargo.toml",
    "Cargo.lock",
];
pub const RETIRED: [&str; 13] = [
    "tests/reference-web",
    "examples/native-ui/capture-reference.mjs",
    "ui/package.json",
    "flightdeck",
    "pyproject.toml",
    "build_support.py",
    "MANIFEST.in",
    "scripts/native-runtime",
    "scripts/install-launcher.py",
    "scripts/install-launcher-gui.py",
    "scripts/install-python.sh",
    "scripts/transition-release.py",
    "scripts/full-installer-release.py",
];
const SKIP: [&str; 10] = [
    "__pycache__",
    "node_modules",
    "build",
    "dist",
    "target",
    "artifacts",
    "coverage",
    ".git",
    ".venv",
    ".pytest_cache",
];
const EXTENSIONS: [&str; 28] = [
    ".py", ".sh", ".md", ".txt", ".json", ".toml", ".lock", ".patch", ".c", ".h", ".cpp", ".hpp",
    ".hip", ".inc", ".idl", ".spec", ".def", ".rs", ".css", ".js", ".mjs", ".html", ".svg",
    ".example", ".yml", ".yaml", ".cs", ".csproj",
];
const NAMES: [&str; 8] = [
    "LICENSE",
    "UPSTREAM-LICENSE",
    "COPYING",
    "COPYING.LIB",
    ".gitignore",
    "Makefile",
    "xodus-wine-launch",
    "native-build.Dockerfile",
];
// Only individually reviewed binary assets, never arbitrary local files.
const ASSETS: [(&str, &str); 4] = [
    (
        "docs/images/launcher-overview.png",
        "092c0ea836b6450470346cd397d42e045a06d2f48c0e15c4940b2f05415e308c",
    ),
    (
        "ui/flight-panorama.png",
        "f64fc375e0aaa806c4de91cd91ec8f18994a06ff64aa80d8b6d95f7c463c9304",
    ),
    (
        "ui/flight-panorama-2020.png",
        "cd81013284d468fe6d306438e66d9a0d7ee35d6bda02ec2b44dea010f8cf8344",
    ),
    (
        "ui/manrope-variable.woff2",
        "30b83738add8c9edd9e3450b98036a9a8fb5668d0cbd4eb0ce5fe6761197f21f",
    ),
];
const TEMPLATES: [&str; 3] = [".example", ".sample", ".template"];
const PRIVATE_DIRECTORIES: [&str; 6] = [
    ".ssh",
    ".aws",
    ".azure",
    "private",
    "local storage",
    "indexeddb",
];
const PRIVATE_NAMES: [&str; 10] = [
    ".netrc",
    "_netrc",
    ".git-credentials",
    "cookies",
    "login data",
    "credentials",
    "accounts",
    "token_cache",
    "msal_token_cache",
    "xodus-keyring",
];
const PRIVATE_STEMS: [&str; 5] = [
    "account",
    "token",
    "tokens",
    "storage-state",
    "storage_state",
];
const PRIVATE_FORMATS: [&str; 8] = [
    ".json", ".ron", ".txt", ".yaml", ".yml", ".db", ".sqlite", ".sqlite3",
];
const ALGORITHMS: [&str; 13] = [
    "HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "ES256", "ES384", "ES512", "PS256",
    "PS384", "PS512", "EdDSA",
];
const LITERAL_MAX: usize = 65536;

/// The part of a file name from its last dot, as long as that dot is neither
/// the first nor the last character.
fn suffix(name: &str) -> &str {
    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => &name[index..],
        _ => "",
    }
}
fn stem(name: &str) -> &str {
    &name[..name.len() - suffix(name).len()]
}

/// Whole names are checked, not source identifiers such as store_account.rs.
fn private_path(relative: &str) -> bool {
    let lower = relative.to_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    if parts.iter().any(|part| PRIVATE_DIRECTORIES.contains(part)) {
        return true;
    }
    if TEMPLATES.contains(&suffix(name)) {
        return false; // Examples still receive every content check.
    }
    name == ".env"
        || name.starts_with(".env.")
        || PRIVATE_NAMES.contains(&name)
        || ((PRIVATE_NAMES.contains(&stem(name)) || PRIVATE_STEMS.contains(&stem(name)))
            && PRIVATE_FORMATS.contains(&suffix(name)))
}

/// Standard or URL-safe base64, padded or not; anything else is not data.
fn decode64(value: &str) -> Option<Vec<u8>> {
    let mut text: String = value
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    while !text.len().is_multiple_of(4) {
        text.push('=');
    }
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

/// Short documented placeholders and formatting expressions are not secrets.
/// Test directories and labels such as "synthetic" earn no exemption.
fn opaque_literal(value: &str) -> bool {
    let value: String = value.chars().take(LITERAL_MAX).collect();
    value.chars().count() >= 32
        && value.chars().collect::<BTreeSet<_>>().len() >= 12
        && value.chars().any(|c| c.is_numeric())
        && value.chars().any(char::is_alphabetic)
}

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Three dot-separated base64url segments that decode to a signed token.
/// Every window of three adjacent segments is examined.
fn signed_token(text: &str) -> bool {
    let token = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let limits = [(10, 4096), (4, 32768), (40, 2048)];
    let mut chain: Vec<&str> = Vec::new();
    let mut found = false;
    let mut check = |chain: &[&str]| {
        for window in chain.windows(3) {
            let fits = window.iter().zip(limits).all(|(part, (low, high))| {
                part.chars().all(token) && (low..=high).contains(&part.len())
            });
            if !fits {
                continue;
            }
            let object = |part: &str| {
                decode64(part)
                    .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
                    .filter(Value::is_object)
            };
            let (Some(header), Some(_), Some(signature)) =
                (object(window[0]), object(window[1]), decode64(window[2]))
            else {
                continue;
            };
            if signature.len() >= 32
                && header["alg"]
                    .as_str()
                    .is_some_and(|alg| ALGORITHMS.contains(&alg))
            {
                found = true;
            }
        }
    };
    let mut rest = text;
    while !rest.is_empty() {
        let start = rest.find(word).unwrap_or(rest.len());
        let separator = &rest[..start];
        if separator != "." {
            check(&chain);
            chain.clear();
        }
        rest = &rest[start..];
        let end = rest.find(|c| !word(c)).unwrap_or(rest.len());
        if end > 0 {
            chain.push(&rest[..end]);
        }
        rest = &rest[end..];
    }
    check(&chain);
    found
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split([
        '\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
        '\u{2029}',
    ])
}

/// A PEM private-key block whose body really is base64 material.
fn private_key(text: &str) -> bool {
    static BEGIN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            "-----BEGIN ((?:(?:RSA|EC|DSA|OPENSSH|ENCRYPTED) )?PRIVATE KEY|PGP PRIVATE KEY BLOCK)-----",
        )
        .expect("pattern")
    });
    let text = text.replace("\\n", "\n");
    let mut position = 0;
    while let Some(captures) = BEGIN.captures_at(&text, position) {
        let begin = captures.get(0).expect("match");
        let end = format!("-----END {}-----", &captures[1]);
        let body = text[begin.end()..]
            .match_indices(&end)
            .find_map(|(offset, _)| {
                let body = &text[begin.end()..begin.end() + offset];
                let length = body.chars().count();
                (32..=65536).contains(&length).then_some(body)
            });
        let Some(body) = body else {
            position = begin.end();
            continue;
        };
        position = begin.end() + body.len() + end.len();
        let material: String = lines(body)
            .filter(|line| !line.contains(':'))
            .map(|line| {
                let line = line.trim();
                let line = line.strip_prefix('+').unwrap_or(line);
                line.strip_prefix('-').unwrap_or(line)
            })
            .collect();
        if !material.is_empty()
            && material
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c) || c.is_whitespace())
            && material.trim().chars().count() >= 40
        {
            return true;
        }
    }
    false
}

/// A JSON object with a key type and its private component. Bare source-code
/// identifiers or independently documented fields are not private keys.
fn private_jwk(text: &str) -> bool {
    static KEY_TYPE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""kty"\s*:"#).expect("pattern"));
    KEY_TYPE.find_iter(text).any(|found| {
        let Some(start) = text[..found.start()].rfind('{') else {
            return false;
        };
        let Some(Ok(Value::Object(item))) = serde_json::Deserializer::from_str(&text[start..])
            .into_iter::<Value>()
            .next()
        else {
            return false;
        };
        let Some(kind) = item.get("kty").and_then(Value::as_str) else {
            return false;
        };
        let field = if kind == "oct" { "k" } else { "d" };
        ["RSA", "EC", "OKP", "oct"].contains(&kind)
            && item
                .get(field)
                .and_then(Value::as_str)
                .and_then(decode64)
                .is_some_and(|data| data.len() >= 16)
    })
}

/// Conservative checks for recognizable material, not a complete secret audit.
/// The returned category never contains the value itself.
pub fn private_content(text: &str) -> Option<&'static str> {
    static ACCESS_KEYS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{60,255}|sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{32,255})\b",
        )
        .expect("pattern")
    });
    static LITERALS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?i)["']?\b(?:access_?token|refresh_?token|client_?secret|api_?key|password|_authToken)["']?\s*[:=]\s*["']([A-Za-z0-9+/=_~.-]{32,})["']"#,
        )
        .expect("pattern")
    });
    static HEADERS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:Bearer\s+|XBL3\.0 x=[0-9]{1,20};)([A-Za-z0-9+/=_~.-]{32,})")
            .expect("pattern")
    });
    if private_key(text) {
        return Some("Private key material");
    }
    if signed_token(text) {
        return Some("Embedded signed-token data");
    }
    if private_jwk(text) {
        return Some("Private JWK material");
    }
    if ACCESS_KEYS
        .find_iter(text)
        .any(|found| opaque_literal(found.as_str()))
    {
        return Some("Embedded access-key data");
    }
    let quoted = LITERALS
        .captures_iter(text)
        .any(|found| found[1].len() <= LITERAL_MAX && opaque_literal(&found[1]));
    let header = HEADERS
        .captures_iter(text)
        .any(|found| opaque_literal(&found[1]));
    (quoted || header).then_some("Embedded credential literal")
}

fn walk(root: &Path, directory: &Path, files: &mut BTreeSet<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("inside the source tree")
            .to_string_lossy()
            .into_owned();
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_symlink() {
            return fail(format!("Source export refuses symlinks: {relative}"));
        }
        if kind.is_dir() {
            walk(root, &path, files)?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        if private_path(&relative) {
            return fail(format!("Private data file in source export: {relative}"));
        }
        let template = name.starts_with(".env.") && TEMPLATES.contains(&suffix(&name));
        if !ASSETS.iter().any(|(asset, _)| *asset == relative)
            && !EXTENSIONS.contains(&suffix(&name))
            && !NAMES.contains(&name.as_str())
            && !template
        {
            return fail(format!("Unrecognized source-release file: {relative}"));
        }
        files.insert(path);
    }
    Ok(())
}

fn source_files(root: &Path) -> Result<BTreeSet<PathBuf>> {
    for name in RETIRED {
        if fs::symlink_metadata(root.join(name)).is_ok() {
            return fail(format!(
                "Retired Python application/build path in current source tree: {name}"
            ));
        }
    }
    let mut files = BTreeSet::new();
    for name in TOP_LEVEL {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(info) if info.file_type().is_symlink() => {
                return fail(format!("Source export refuses symlinks: {name}"));
            }
            Ok(info) if info.is_file() => {
                files.insert(path);
            }
            _ => {}
        }
    }
    for directory in DIRECTORIES {
        let path = root.join(directory);
        match fs::symlink_metadata(&path) {
            Ok(info) if info.file_type().is_symlink() => {
                return fail(format!("Source export refuses symlinks: {directory}"));
            }
            Ok(info) if info.is_dir() => walk(root, &path, &mut files)?,
            _ => {}
        }
    }
    Ok(files)
}

/// Every file of the source release with its SHA-256, after inspection.
pub fn inspect(root: &Path) -> Result<BTreeMap<String, String>> {
    static PERSONAL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("/(?:home|Users)/[a-zA-Z0-9_.-]+/").expect("pattern"));
    let mut hashes = BTreeMap::new();
    for path in source_files(root)? {
        let relative = path
            .strip_prefix(root)
            .expect("inside the source tree")
            .to_string_lossy()
            .into_owned();
        let data = fs::read(&path)?;
        if let Some((_, expected)) = ASSETS.iter().find(|(asset, _)| *asset == relative) {
            require(
                digest(&data) == *expected,
                format!("Unreviewed artwork in source export: {relative}"),
            )?;
            hashes.insert(relative, expected.to_string());
            continue;
        }
        let magic: [&[u8]; 4] = [b"MZ", b"\x7fELF", b"PK\x03\x04", b"\x1f\x8b"];
        if magic.iter().any(|magic| data.starts_with(magic)) || data.contains(&0) {
            return fail(format!("Binary content in source export: {relative}"));
        }
        let text = std::str::from_utf8(&data)
            .map_err(|_| Failure(format!("Source file is not UTF-8: {relative}")))?;
        // Actual machine paths are rejected; generic placeholders and code are fine.
        if PERSONAL.is_match(text) {
            return fail(format!(
                "Personal absolute path in source export: {relative}"
            ));
        }
        if let Some(category) = private_content(text) {
            return fail(format!("{category} in source export: {relative}"));
        }
        hashes.insert(relative, digest(&data));
    }
    Ok(hashes)
}

fn manifest(files: &BTreeMap<String, String>) -> Value {
    json!({"format": 1, "status": "PASS", "files": files})
}

pub fn check(root: &Path) -> Result<()> {
    let files = inspect(root)?;
    println!("{}", json!({"status": "PASS", "source_files": files.len()}));
    Ok(())
}

/// Writes the archive from the inspected file list. Every source is read and
/// verified before any output exists, and only those bytes are written.
pub fn archive(
    root: &Path,
    output: &Path,
    files: &BTreeMap<String, String>,
    timestamp: u64,
) -> Result<()> {
    archive_extra(root, output, files, timestamp, BTreeMap::new())
}
fn archive_extra(
    root: &Path,
    output: &Path,
    files: &BTreeMap<String, String>,
    timestamp: u64,
    extra: BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let output = std::path::absolute(output)?;
    require(
        fs::symlink_metadata(&output).is_err(),
        "Output exists; choose a new source archive path",
    )?;
    require(
        output.starts_with(root.join("build"))
            && !output
                .components()
                .any(|part| part == std::path::Component::ParentDir),
        "Source archives must be written under the ignored build directory",
    )?;
    let mut contents = BTreeMap::new();
    for (name, expected) in files {
        let data = fs::read(root.join(name))?;
        require(
            digest(&data) == *expected,
            format!("Source changed after inspection: {name}"),
        )?;
        contents.insert(name.clone(), data);
    }
    let mut recorded = files.clone();
    for (name, data) in extra {
        require(!contents.contains_key(&name), "Duplicate source input")?;
        recorded.insert(name.clone(), digest(&data));
        contents.insert(name, data);
    }
    contents.insert("SOURCE-MANIFEST.json".into(), pretty(&manifest(&recorded)));
    fs::create_dir_all(output.parent().expect("archive directory"))?;
    let stream = fs::File::create_new(&output)?;
    let compressed = flate2::GzBuilder::new()
        .mtime(timestamp as u32)
        .write(stream, flate2::Compression::best());
    let mut tar = tar::Builder::new(compressed);
    for (name, data) in &contents {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mtime(timestamp);
        header.set_mode(
            if data.starts_with(b"#!") || name == "Install Flightdeck.desktop" {
                0o755
            } else {
                0o644
            },
        );
        tar.append_data(&mut header, format!("flightdeck-linux/{name}"), &data[..])?;
    }
    tar.into_inner()?.finish()?.flush()?;
    println!(
        "{}",
        json!({"status": "PASS", "source_files": files.len(), "archive": output})
    );
    Ok(())
}

pub fn release(root: &Path, output: &Path) -> Result<()> {
    release_with_sources(root, output, None, None)
}
pub fn release_with_sources(
    root: &Path,
    output: &Path,
    media_sources: Option<&Path>,
    mobiflight_sources: Option<&Path>,
) -> Result<()> {
    let timestamp = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .map(|value| {
            value
                .parse()
                .map_err(|_| Failure("SOURCE_DATE_EPOCH is not a number of seconds".into()))
        })
        .transpose()?
        .unwrap_or(0);
    let mut extra = media_sources
        .map(|folder| crate::media::source_files(root, folder))
        .transpose()?
        .unwrap_or_default();
    if let Some(folder) = mobiflight_sources {
        extra.extend(crate::mobiflight::source_files(root, folder)?);
    }
    if extra.is_empty() {
        archive(root, output, &inspect(root)?, timestamp)
    } else {
        archive_extra(root, output, &inspect(root)?, timestamp, extra)
    }
}

#[cfg(test)]
mod tests;
