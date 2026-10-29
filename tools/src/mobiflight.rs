// SPDX-License-Identifier: MIT
//! Build a deterministic HidSharp compatibility DLL, without touching a Wine profile.
use crate::util::{Failure, Result, digest, read_regular, require};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(clap::Args)]
pub struct Arguments {
    /// Folder containing hidsharp.2.6.4.nupkg and mono.cecil.0.11.6.nupkg.
    #[arg(long)]
    pub sources: PathBuf,
    /// Fetch missing original NuGet packages over HTTPS.
    #[arg(long)]
    pub download: bool,
    /// New disposable build folder; never an installed application folder.
    #[arg(long)]
    pub work: PathBuf,
    /// New output folder, normally build/mobiflight.
    #[arg(long)]
    pub output: PathBuf,
    /// Maintainer's .NET 10 SDK host; not required on user machines.
    #[arg(long, default_value = "dotnet")]
    pub dotnet: PathBuf,
}
pub fn lock(root: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&read_regular(
        &root.join("compat/mobiflight.lock.json"),
        65536,
    )?)?)
}
fn checked(command: &mut Command) -> Result<()> {
    require(
        command.stdin(Stdio::null()).status()?.success(),
        "MobiFlight compatibility build failed",
    )
}
fn pinned(path: &Path, hash: &Value, length: Option<u64>) -> Result<Vec<u8>> {
    let bytes = read_regular(path, 4 * 1024 * 1024)?;
    require(
        hash.as_str() == Some(digest(&bytes).as_str())
            && length.is_none_or(|n| n == bytes.len() as u64),
        "MobiFlight source/payload checksum or size differs",
    )?;
    Ok(bytes)
}
fn member(bytes: Vec<u8>, name: &str) -> Result<Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Failure(e.to_string()))?;
    let mut entry = archive.by_name(name).map_err(|e| Failure(e.to_string()))?;
    require(
        entry.size() <= 1024 * 1024 && entry.is_file(),
        "Unexpected NuGet member",
    )?;
    let mut bytes = Vec::new();
    (&mut entry).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    require(bytes.len() <= 1024 * 1024, "Oversized NuGet member")?;
    Ok(bytes)
}
pub fn verify_bundle(root: &Path, folder: &Path) -> Result<()> {
    let lock = lock(root)?;
    let bytes = pinned(
        &folder.join("HidSharp.dll"),
        &lock["hidsharp"]["patched_sha256"],
        lock["hidsharp"]["patched_bytes"].as_u64(),
    )?;
    require(
        bytes.starts_with(b"MZ"),
        "Expected the pinned Windows HidSharp DLL",
    )
}
pub fn source_files(root: &Path, folder: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let lock = lock(root)?;
    let mut result = BTreeMap::new();
    for (component, name) in [
        ("hidsharp", "hidsharp.2.6.4.nupkg"),
        ("cecil", "mono.cecil.0.11.6.nupkg"),
    ] {
        result.insert(
            format!("mobiflight-sources/{name}"),
            pinned(
                &folder.join(name),
                &lock[component]["package_sha256"],
                lock[component]["package_bytes"].as_u64(),
            )?,
        );
    }
    Ok(result)
}
pub fn build(root: &Path, args: &Arguments) -> Result<()> {
    let lock = lock(root)?;
    require(
        !args.work.exists() && !args.output.exists(),
        "MobiFlight work and output folders must be new",
    )?;
    fs::create_dir_all(&args.sources)?;
    for (component, name) in [
        ("hidsharp", "hidsharp.2.6.4.nupkg"),
        ("cecil", "mono.cecil.0.11.6.nupkg"),
    ] {
        let path = args.sources.join(name);
        if !path.exists() && args.download {
            let url = lock[component]["package_url"]
                .as_str()
                .ok_or_else(|| Failure("Missing NuGet URL".into()))?;
            require(
                url.starts_with("https://api.nuget.org/v3-flatcontainer/"),
                "Unexpected MobiFlight source origin",
            )?;
            let temporary = path.with_extension("part");
            require(!temporary.exists(), "Unfinished NuGet download exists")?;
            checked(
                Command::new("curl")
                    .args([
                        "--fail",
                        "--location",
                        "--silent",
                        "--show-error",
                        "--proto",
                        "=https",
                        "--proto-redir",
                        "=https",
                        "--connect-timeout",
                        "30",
                        "--max-time",
                        "120",
                        "--max-filesize",
                        "4194304",
                        "--output",
                    ])
                    .arg(&temporary)
                    .arg(url),
            )?;
            pinned(
                &temporary,
                &lock[component]["package_sha256"],
                lock[component]["package_bytes"].as_u64(),
            )?;
            fs::rename(&temporary, &path)?;
        }
    }
    let sources = source_files(root, &args.sources)?;
    let original = member(
        sources["mobiflight-sources/hidsharp.2.6.4.nupkg"].clone(),
        "lib/netstandard2.0/HidSharp.dll",
    )?;
    let cecil = member(
        sources["mobiflight-sources/mono.cecil.0.11.6.nupkg"].clone(),
        "lib/netstandard2.0/Mono.Cecil.dll",
    )?;
    require(
        digest(&original) == lock["hidsharp"]["original_sha256"]
            && original.len() == lock["hidsharp"]["original_bytes"],
        "Unexpected original HidSharp DLL",
    )?;
    require(
        digest(&cecil) == lock["cecil"]["dll_sha256"],
        "Unexpected Cecil DLL",
    )?;
    fs::create_dir_all(&args.work)?;
    let work = args.work.canonicalize()?;
    fs::create_dir(work.join("empty-feed"))?;
    fs::write(work.join("Mono.Cecil.dll"), cecil)?;
    fs::write(work.join("original.dll"), original)?;
    for name in ["Program.cs", "HidPatch.csproj"] {
        fs::write(
            work.join(name),
            read_regular(&root.join("compat/mobiflight").join(name), 65536)?,
        )?;
    }
    let mut build = Command::new(&args.dotnet);
    build
        .current_dir(&work)
        .env("DOTNET_CLI_HOME", work.join("dotnet-home"))
        .env("NUGET_PACKAGES", work.join("packages"))
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_GENERATE_ASPNET_CERTIFICATE", "false")
        .args([
            "build",
            "HidPatch.csproj",
            "--disable-build-servers",
            "--configuration",
            "Release",
            "--verbosity",
            "quiet",
        ])
        .arg(format!(
            "-p:CecilPath={}",
            work.join("Mono.Cecil.dll").display()
        ))
        .arg(format!(
            "-p:RestoreSources={}",
            work.join("empty-feed").display()
        ));
    checked(&mut build)?;
    for name in ["patched-a.dll", "patched-b.dll"] {
        checked(
            Command::new(&args.dotnet)
                .current_dir(&work)
                .arg(work.join("bin/Release/net10.0/HidPatch.dll"))
                .arg(work.join("original.dll"))
                .arg(work.join(name)),
        )?;
    }
    let first = pinned(
        &work.join("patched-a.dll"),
        &lock["hidsharp"]["patched_sha256"],
        lock["hidsharp"]["patched_bytes"].as_u64(),
    )?;
    require(
        first
            == pinned(
                &work.join("patched-b.dll"),
                &lock["hidsharp"]["patched_sha256"],
                lock["hidsharp"]["patched_bytes"].as_u64(),
            )?,
        "HidSharp build is not deterministic",
    )?;
    fs::create_dir_all(&args.output)?;
    fs::write(args.output.join("HidSharp.dll"), first)?;
    for name in ["HidSharp-LICENSE.txt", "README.md"] {
        fs::write(
            args.output.join(name),
            read_regular(&root.join("compat/mobiflight").join(name), 65536)?,
        )?;
    }
    verify_bundle(root, &args.output)
}
