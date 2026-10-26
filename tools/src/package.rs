// SPDX-License-Identifier: MIT
//! Deterministic launcher packages from explicit, verified inputs. No game,
//! Wine prefix or account directory is read.
use crate::{
    components::{self, LIMITS, Members},
    util::{Failure, Result, digest, fail, pretty, read_regular, require, strict_json, version},
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

pub const PACKAGE_TOTAL_MAX: u64 = 384 * 1024 * 1024;
pub const PACKAGE_MEMBERS_MAX: usize = 4096;
const GRAPH_MAX: u64 = 32 * 1024 * 1024;

pub struct Inputs<'a> {
    pub binary: &'a Path,
    pub cargo_metadata: &'a Path,
    pub rust_notices: &'a Path,
    pub native: Option<&'a Path>,
    pub graphics: Option<&'a Path>,
    pub output: &'a Path,
    pub launcher_only: bool,
}

fn output(command: &mut Command, label: &str) -> Result<String> {
    let result = command
        .output()
        .map_err(|error| Failure(format!("{label}: {error}")))?;
    require(result.status.success(), format!("{label} failed"))?;
    String::from_utf8(result.stdout).map_err(|_| Failure(format!("{label} printed invalid text")))
}

/// Name, version and source of every `[[package]]` in Cargo.lock.
fn locked(root: &Path) -> Result<BTreeMap<(String, String), Option<String>>> {
    let text = String::from_utf8(read_regular(&root.join("Cargo.lock"), GRAPH_MAX)?)
        .map_err(|_| Failure("Cargo.lock is not UTF-8".into()))?;
    let mut packages = BTreeMap::new();
    for block in text.split("[[package]]").skip(1) {
        let field = |key: &str| {
            block.lines().find_map(|line| {
                let value = line.strip_prefix(key)?.strip_prefix(" = \"")?;
                Some(value.strip_suffix('"')?.to_string())
            })
        };
        let (Some(name), Some(version)) = (field("name"), field("version")) else {
            return fail("Cargo.lock contains an incomplete package");
        };
        packages.insert((name, version), field("source"));
    }
    Ok(packages)
}

fn license_name(name: &str) -> bool {
    let name = name.to_lowercase();
    ["license", "licence", "copying", "notice", "copyright"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn license_files(folder: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_dir() {
            license_files(&path, found)?;
        } else if kind.is_file()
            && path
                .file_name()
                .is_some_and(|name| license_name(&name.to_string_lossy()))
        {
            found.push(path);
        }
    }
    Ok(())
}

/// The packages `cargo tree` lists for the shipped launcher. Metadata unifies
/// test features and includes the GPU test renderer; the real normal and build
/// graph describes the shipped software renderer instead.
pub fn production(tree: &str) -> Result<BTreeSet<(String, String)>> {
    static LINE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\S+) v(\S+)(?: |$)").expect("pattern"));
    tree.lines()
        .map(|line| {
            LINE.captures(line)
                .map(|found| (found[1].to_string(), found[2].to_string()))
                .ok_or_else(|| Failure("Unexpected Cargo production dependency graph".into()))
        })
        .collect()
}

/// The notice text and dependency inventory of the shipped launcher.
pub fn notices(root: &Path, metadata: &Path, tree: &str) -> Result<(Vec<u8>, Value)> {
    let version = version(root)?;
    let graph = strict_json(
        &read_regular(metadata, GRAPH_MAX)?,
        "Cargo graph",
        GRAPH_MAX,
    )?;
    let wanted = production(tree)?;
    let expected = locked(root)?;
    let supplements = strict_json(
        &read_regular(&root.join("compat/rust-licenses.lock.json"), LIMITS.json)?,
        "Rust license sources",
        LIMITS.json,
    )?;
    let mut packages: Vec<&Value> = graph
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure("Cargo graph lists no packages".into()))?
        .iter()
        .collect();
    let text = |value: &'_ Value, key: &str| value[key].as_str().unwrap_or_default().to_string();
    packages.sort_by_key(|package| (text(package, "name"), text(package, "version")));
    let (mut inventory, mut texts, mut seen) = (Vec::new(), Vec::new(), BTreeSet::new());
    for package in packages {
        let identity = (text(package, "name"), text(package, "version"));
        if !wanted.contains(&identity) {
            continue;
        }
        let (name, package_version) = (&identity.0, &identity.1);
        let source = expected.get(&identity);
        require(
            source.is_some() && seen.insert(identity.clone()),
            "Cargo metadata does not match the locked dependency graph",
        )?;
        require(
            package["source"].as_str() == source.and_then(|source| source.as_deref()),
            "Cargo dependency source does not match Cargo.lock",
        )?;
        let folder = Path::new(package["manifest_path"].as_str().unwrap_or_default())
            .parent()
            .ok_or_else(|| Failure("Cargo metadata lacks a manifest path".into()))?
            .to_path_buf();
        let (mut license_root, mut provenance) = (folder.clone(), None);
        let mut paths = Vec::new();
        if package["source"].is_null() {
            let local = match name.as_str() {
                "flightdeck-linux" => Some(root.to_path_buf()),
                "flightdeck-ui" => Some(root.join("native/ui")),
                _ => None,
            };
            require(
                local.is_some_and(|local| {
                    fs::canonicalize(&folder).ok() == fs::canonicalize(local).ok()
                }) && *package_version == version,
                "Unexpected local package in Cargo metadata",
            )?;
            paths.push(folder.join("LICENSE"));
        } else {
            license_files(&folder, &mut paths)?;
            paths.sort();
        }
        if let Some(file) = package["license_file"].as_str() {
            let extra = folder.join(file);
            require(
                fs::canonicalize(&extra).is_ok_and(|extra| {
                    fs::canonicalize(&folder).is_ok_and(|folder| extra.starts_with(folder))
                }),
                "License path leaves its dependency",
            )?;
            if !paths.contains(&extra) {
                paths.push(extra);
            }
        }
        if paths.is_empty() {
            // Some monorepo crates omit the repository license from crates.io.
            // Only the pinned upstream text for this exact crate revision is used.
            let pinned = supplements
                .get("packages")
                .and_then(|packages| packages.get(format!("{name}@{package_version}")))
                .filter(|pinned| pinned["spdx"].is_string() && pinned["spdx"] == package["license"])
                .ok_or_else(|| Failure(format!("Missing dependency license text: {name}")))?;
            let revision = if let Some(source) =
                package["source"].as_str().filter(|s| s.starts_with("git+"))
            {
                source
                    .rsplit_once('#')
                    .map(|(_, revision)| revision.to_string())
            } else {
                strict_json(
                    &read_regular(&folder.join(".cargo_vcs_info.json"), 16384)?,
                    "Crate revision",
                    16384,
                )?["git"]["sha1"]
                    .as_str()
                    .map(str::to_string)
            };
            let path = root.join(pinned["path"].as_str().unwrap_or_default());
            let inside = fs::canonicalize(&path).is_ok_and(|path| {
                fs::canonicalize(root.join("compat/LICENSES/rust"))
                    .is_ok_and(|base| path.starts_with(base))
            });
            require(
                revision.as_deref() == pinned["revision"].as_str()
                    && revision
                        .as_ref()
                        .is_some_and(|r| r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()))
                    && pinned["revision"].is_string()
                    && inside
                    && Some(digest(&read_regular(&path, 1024 * 1024)?).as_str())
                        == pinned["sha256"].as_str(),
                format!("Pinned dependency license differs: {name}"),
            )?;
            paths.push(path);
            license_root = root.to_path_buf();
            provenance = Some(pinned.clone());
        }
        let mut included = Vec::new();
        for path in &paths {
            let data = read_regular(path, 4 * 1024 * 1024)?;
            let relative = path
                .strip_prefix(&license_root)
                .map_err(|_| Failure("License path leaves its dependency".into()))?
                .to_string_lossy()
                .into_owned();
            let content = std::str::from_utf8(&data)
                .map_err(|_| Failure(format!("License text is not UTF-8: {name}")))?;
            let rule = "=".repeat(72);
            texts.push(format!(
                "\n{rule}\n{name} {package_version} / {relative}\n{rule}\n{content}"
            ));
            included.push(json!({"name": relative, "sha256": digest(&data)}));
        }
        let mut entry = json!({"name": name, "version": package_version, "source": package["source"],
            "license": package["license"], "repository": package["repository"], "notices": included});
        if let Some(provenance) = provenance {
            entry["upstream_license"] = provenance;
        }
        inventory.push(entry);
    }
    require(
        seen == wanted && seen.contains(&("flightdeck-linux".to_string(), version.clone())),
        "Cargo metadata omits a production dependency",
    )?;
    let font = String::from_utf8(read_regular(&root.join("ui/OFL-Manrope.txt"), LIMITS.json)?)
        .map_err(|_| Failure("Font license is not UTF-8".into()))?;
    texts.push(format!("\nManrope / SIL Open Font License\n{font}"));
    let header = format!(
        "Flightdeck {version} Rust launcher\n\nLauncher/UI: MIT. Dependency license texts follow.\n\
         Bundled compatibility and graphics components retain their own licenses;\n\
         their original notices are in resources/native and resources/graphics.\n\
         The companion Flightdeck source archive contains the launcher sources and Cargo.lock.\n\
         When distributing the full package, also distribute the unchanged corresponding\n\
         Xodus/WineGDK, DXVK and VKD3D source archives listed in the release manifest.\n"
    );
    Ok((
        (header + &texts.concat()).into_bytes(),
        Value::Array(inventory),
    ))
}

/// The executable may import nothing newer than the glibc 2.39 baseline.
pub fn validate_abi(symbols: &str) -> Result<()> {
    static VERSION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"@GLIBC_([0-9.]+)").expect("pattern"));
    let newest = VERSION
        .captures_iter(symbols)
        .map(|found| {
            found[1]
                .split('.')
                .map(|part| part.parse::<u32>().unwrap_or(u32::MAX))
                .collect::<Vec<_>>()
        })
        .max();
    require(
        newest.is_some_and(|newest| newest <= vec![2, 39]),
        "Build on the glibc 2.39 baseline: this executable needs a newer or unknown glibc ABI",
    )
}

pub fn mode(name: &str) -> u32 {
    if ["bin/flightdeck", "install.sh", "Install Flightdeck.desktop"].contains(&name)
        || name.starts_with("resources/native/bin/")
    {
        0o755
    } else {
        0o644
    }
}

fn payload(root: &Path, inputs: &Inputs<'_>) -> Result<(Members, Value)> {
    let version = version(root)?;
    let raw = read_regular(inputs.binary, LIMITS.file)?;
    require(
        raw.len() >= 64 && raw.starts_with(b"\x7fELF\x02\x01") && raw[18..20] == [0x3e, 0],
        "Expected a Linux x86-64 release binary",
    )?;
    let binary = fs::canonicalize(inputs.binary)?;
    validate_abi(&output(
        Command::new("readelf")
            .args(["--dyn-syms", "--wide"])
            .arg(&binary),
        "readelf",
    )?)?;
    require(
        output(Command::new(&binary).arg("--version"), "The launcher")?.trim()
            == format!("flightdeck {version}"),
        "Binary version differs from Cargo.toml",
    )?;
    let tree = output(
        Command::new("cargo").current_dir(root).args([
            "tree",
            "--offline",
            "--locked",
            "--package",
            "flightdeck-linux",
            "--edges",
            "normal,build",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ]),
        "cargo tree",
    )?;
    let (mut notice, inventory) = notices(root, inputs.cargo_metadata, &tree)?;
    if !inputs.launcher_only {
        let media: Value = serde_json::from_str(&output(
            Command::new(&binary).args(["--native-probe", "media-bundle"]),
            "The media payload",
        )?)?;
        let lock = crate::media::lock(root)?;
        require(
            media["bundled"] == true
                && media["sha256"] == lock["files"]["winegstreamer.so"]["sha256"],
            "Full releases require the verified bundled-media feature",
        )?;
        notice.extend_from_slice(b"\nWine GStreamer system-memory compatibility module\nWine contributors; OnlyCrayZ-Labs contributors (2026 system-memory negotiation fix).\nThis separately loaded module is LGPL-2.1-or-later. Its exact original Wine and xgameruntime sources, patch and build instructions are supplied in media-sources/ and compat/media/ of the matching Flightdeck source archive.\n");
        notice.extend(fs::read(root.join("compat/LICENSES/LGPL-2.1.txt"))?);
    }
    let mut values = Members::new();
    values.insert("bin/flightdeck".into(), raw);
    values.insert("THIRD-PARTY-NOTICES.txt".into(), notice);
    values.insert(
        "RUST-STANDARD-LIBRARY-NOTICES.html".into(),
        read_regular(inputs.rust_notices, LIMITS.notice)?,
    );
    for name in [
        "LICENSE",
        "ui/mark.svg",
        "compat/bootstrap.lock.json",
        "compat/graphics.lock.json",
    ] {
        values.insert(name.into(), fs::read(root.join(name))?);
    }
    if let Some(native) = inputs.native {
        let lock = strict_json(
            &values["compat/bootstrap.lock.json"],
            "Bootstrap lock",
            LIMITS.json,
        )?;
        let lock = lock.get("native").cloned().unwrap_or_default();
        components::validate_native_lock(&lock)?;
        let archive = read_regular(native, components::NATIVE_ARCHIVE_MAX)?;
        require(
            Some(digest(&archive).as_str()) == lock["archive_sha256"].as_str(),
            "Compatibility archive differs from bootstrap lock",
        )?;
        let members = components::read_native(&archive, LIMITS)?;
        components::verify_native(&members, &lock, LIMITS)?;
        values.extend(
            members
                .into_iter()
                .map(|(name, data)| (format!("resources/native/{name}"), data)),
        );
    }
    if let Some(graphics) = inputs.graphics {
        let members =
            components::graphics_files(&values["compat/graphics.lock.json"], graphics, LIMITS)?;
        values.extend(
            members
                .into_iter()
                .map(|(name, data)| (format!("resources/graphics/{name}"), data)),
        );
    }
    values.insert(
        "README.txt".into(),
        format!(
            "Flightdeck {version}\n\nRun ./install.sh (terminal) or ./install.sh --gui.\n\
             No Python, pip or Rust toolchain is required to install or run this package.\n\
             The optional install dialog needs Zenity or KDialog.\n\
             For a development service: ./bin/flightdeck --no-browser\n\
             Uninstall: ~/.local/bin/flightdeck --uninstall\n\
             Settings, game installations and saves remain separate from the launcher.\n"
        )
        .into_bytes(),
    );
    let files: BTreeMap<&String, String> = values
        .iter()
        .map(|(name, data)| (name, digest(data)))
        .collect();
    let manifest =
        pretty(&json!({"schema": 1, "kind": "rust-launcher", "version": version, "files": files}));
    values.insert("FLIGHTDECK-PACKAGE.json".into(), manifest);
    for name in ["install.sh", "Install Flightdeck.desktop"] {
        values.insert(name.into(), fs::read(root.join(name))?);
    }
    require(
        values.values().map(|data| data.len() as u64).sum::<u64>() <= PACKAGE_TOTAL_MAX
            && values.len() <= PACKAGE_MEMBERS_MAX,
        "Native package exceeds the installer limits",
    )?;
    Ok((values, inventory))
}

/// The package as tar.gz and ZIP with identical members, both reproducible.
pub fn write_archives(values: &Members, output: &Path, base: &str) -> Result<PathBuf> {
    let archive = output.join(format!("{base}.tar.gz"));
    let compressed = flate2::GzBuilder::new()
        .mtime(0)
        .write(fs::File::create_new(&archive)?, flate2::Compression::best());
    let mut tar = tar::Builder::new(compressed);
    for (name, data) in values {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(mode(name));
        header.set_mtime(0);
        tar.append_data(&mut header, format!("flightdeck-linux/{name}"), &data[..])?;
    }
    tar.into_inner()?.finish()?.flush()?;
    let mut zipped = zip::ZipWriter::new(fs::File::create_new(output.join(format!("{base}.zip")))?);
    for (name, data) in values {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(9))
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(mode(name));
        let failed = |error: zip::result::ZipError| Failure(error.to_string());
        zipped
            .start_file(format!("flightdeck-linux/{name}"), options)
            .map_err(failed)?;
        zipped.write_all(data)?;
    }
    zipped
        .finish()
        .map_err(|error| Failure(error.to_string()))?
        .flush()?;
    Ok(archive)
}

pub fn create(root: &Path, inputs: &Inputs<'_>) -> Result<()> {
    require(
        inputs.launcher_only || (inputs.native.is_some() && inputs.graphics.is_some()),
        "Full packages require --native and --graphics; use --launcher-only for a development package",
    )?;
    require(
        !inputs.launcher_only || (inputs.native.is_none() && inputs.graphics.is_none()),
        "Do not mix launcher-only and full-package inputs",
    )?;
    let (values, inventory) = payload(root, inputs)?;
    if let Some(parent) = inputs.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(inputs.output)?;
    let base = if inputs.launcher_only {
        "Flightdeck-Launcher-Linux-x86_64"
    } else {
        "Flightdeck-Linux-x86_64"
    };
    let archive = write_archives(&values, inputs.output, base)?;
    fs::write(
        inputs.output.join("launcher-dependencies.json"),
        pretty(&inventory),
    )?;
    fs::write(
        inputs.output.join("THIRD-PARTY-NOTICES.txt"),
        &values["THIRD-PARTY-NOTICES.txt"],
    )?;
    let mut names: Vec<_> = fs::read_dir(inputs.output)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()?;
    names.sort();
    let mut sums = String::new();
    for name in names {
        let data = read_regular(&inputs.output.join(&name), LIMITS.total * 2)?;
        sums.push_str(&format!("{}  {name}\n", digest(&data)));
    }
    fs::write(inputs.output.join("SHA256SUMS"), sums)?;
    println!(
        "{}",
        json!({"status": "PASS", "version": version(root)?, "files": values.len(),
            "dependencies": inventory.as_array().map_or(0, Vec::len),
            "full_package": !inputs.launcher_only, "archive": archive})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_larger_cargo_graph_does_not_relax_the_component_manifest_limit() {
        let data = format!(
            "{{\"packages\":[],\"padding\":\"{}\"}}",
            "x".repeat(1024 * 1024)
        );
        let error = strict_json(data.as_bytes(), "manifest", LIMITS.json).expect_err("limit");
        assert!(error.0.contains("size limit"));
        let graph = strict_json(data.as_bytes(), "Cargo graph", GRAPH_MAX).expect("graph");
        assert_eq!(graph["packages"], json!([]));
        let error = strict_json(
            br#"{"packages":[],"packages":[]}"#,
            "Cargo graph",
            GRAPH_MAX,
        )
        .expect_err("duplicate");
        assert!(error.0.contains("invalid JSON"));
    }

    #[test]
    fn new_glibc_imports_cannot_silently_raise_the_release_requirement() {
        validate_abi("UND memcpy@GLIBC_2.14\nUND posix_spawn@GLIBC_2.39").expect("baseline");
        validate_abi("UND memcpy@GLIBC_2.2.5 (2)\nUND read@GLIBC_2.39")
            .expect("old symbol versions");
        for symbols in [
            "UND memcpy@GLIBC_2.14\nUND atan2f@GLIBC_2.43",
            "UND future@GLIBC_3.0",
            "UND memcpy@GLIBC_2.39.1",
            "",
        ] {
            let error = validate_abi(symbols).expect_err(symbols);
            assert!(error.0.contains("glibc 2.39"));
        }
    }

    #[test]
    fn only_complete_package_lines_form_the_production_graph() {
        let wanted =
            production("flightdeck-linux v0.2.11 (/src)\nserde v1.0.229\nsyn v2.0.1 (proc-macro)")
                .expect("graph");
        assert!(wanted.contains(&("serde".into(), "1.0.229".into())));
        assert_eq!(wanted.len(), 3);
        assert!(production("serde 1.0.229").is_err());
        assert!(production("warning: something\nserde v1").is_err());
    }

    #[test]
    fn executables_and_bundled_binaries_are_the_only_executable_members() {
        for name in [
            "bin/flightdeck",
            "install.sh",
            "Install Flightdeck.desktop",
            "resources/native/bin/xodus-cli",
        ] {
            assert_eq!(mode(name), 0o755, "{name}");
        }
        for name in [
            "LICENSE",
            "resources/native/runtime/xgameruntime.dll",
            "bin/other",
            "resources/graphics/dxgi.dll",
        ] {
            assert_eq!(mode(name), 0o644, "{name}");
        }
    }

    #[test]
    fn the_lock_file_of_this_checkout_lists_local_and_registry_packages() {
        let packages = locked(&crate::util::root()).expect("Cargo.lock");
        let version = version(&crate::util::root()).expect("version");
        assert_eq!(packages[&("flightdeck-linux".to_string(), version)], None);
        assert!(packages.iter().any(|((name, _), source)| {
            name == "serde"
                && source
                    .as_deref()
                    .is_some_and(|s| s.starts_with("registry+"))
        }));
    }
}
