// SPDX-License-Identifier: MIT
//! Licensed Xodus image descriptors exposed to Wine, without decrypting disk files.
use crate::{
    Error, Result, error::require, files, games::Game, inherited_fd, process, runtime,
    transaction as tx,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileExt, symlink},
            process::ExitStatusExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Instant,
};
struct Mapping {
    file: File,
    name: String,
}
struct View {
    path: PathBuf,
}
impl Drop for View {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
struct OwnedChild(Child);
struct OwnedRefresh(Child);
impl Drop for OwnedRefresh {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|v| v.is_some()) {
            return;
        }
        if let Some(pid) = rustix::process::Pid::from_raw(self.0.id() as i32) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        let _ = process::wait(
            &mut self.0,
            std::time::Duration::from_secs(20),
            &std::sync::atomic::AtomicBool::new(false),
        );
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = process::terminate(&mut self.0);
    }
}
fn nt(path: &Path) -> Result<String> {
    Ok(format!(
        r"\??\Z:{}",
        path.to_str()
            .ok_or(Error::Invalid("Invalid game image path."))?
            .replace('/', "\\")
    ))
}
fn source(value: &str) -> PathBuf {
    value
        .strip_prefix(r"\??\Z:\")
        .map(|v| PathBuf::from(format!("/{}", v.replace('\\', "/"))))
        .unwrap_or_else(|| PathBuf::from(value))
}
fn mappings(value: &str) -> Result<Vec<Mapping>> {
    require(
        !value.is_empty() && value.len() <= 2 * 1024 * 1024,
        "Missing or invalid WINE_DLL_FILE_MAP.",
    )?;
    let mut descriptors = BTreeMap::new();
    let mut result = Vec::new();
    for item in value.split('|') {
        let (number, name) = item
            .split_once(':')
            .ok_or(Error::Invalid("Missing or invalid WINE_DLL_FILE_MAP."))?;
        require(
            !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
                && result.len() < 16384,
            "Invalid image descriptor map.",
        )?;
        let number = number
            .parse::<i32>()
            .map_err(|_| Error::Invalid("Invalid image descriptor."))?;
        if let std::collections::btree_map::Entry::Vacant(entry) = descriptors.entry(number) {
            let file = inherited_fd::duplicate(number)?;
            require(
                file.metadata()?.is_file(),
                "Mapped image is not a regular file.",
            )?;
            entry.insert(file);
        }
        result.push(Mapping {
            file: descriptors
                .get(&number)
                .ok_or(Error::Invalid("Missing mapped image."))?
                .try_clone()?,
            name: name.into(),
        });
    }
    Ok(result)
}
fn pe_headers(file: &File) -> Result<Vec<u8>> {
    let mut dos = [0_u8; 64];
    file.read_exact_at(&mut dos, 0)?;
    require(
        &dos[..2] == b"MZ",
        "Mapped entry point is not a PE executable.",
    )?;
    let offset = u32::from_le_bytes(dos[60..64].try_into().expect("fixed DOS header slice")) as u64;
    require(offset <= 1024 * 1024, "Invalid PE header offset.")?;
    let mut pe = [0_u8; 88];
    file.read_exact_at(&mut pe, offset)?;
    require(&pe[..4] == b"PE\0\0", "Invalid PE signature.")?;
    let size = u32::from_le_bytes(pe[84..88].try_into().expect("fixed PE header slice")) as u64;
    require(
        (offset + 88..=file.metadata()?.len().min(1024 * 1024)).contains(&size),
        "Invalid PE header length.",
    )?;
    let mut headers = vec![0; size as usize];
    file.read_exact_at(&mut headers, 0)?;
    Ok(headers)
}
fn skip(path: &Path, view: &Path) -> bool {
    path == view
        || path.file_name().is_some_and(|v| {
            let name = v.to_string_lossy();
            name.starts_with(".xodus-fenix-") || name.starts_with(".xodus-launch-")
        })
}
fn portable_tree(original: &Path, view: &Path, mapped: &[Mapping]) -> Result<()> {
    let mut images = BTreeMap::new();
    for mapping in mapped {
        require(
            mapping.name.starts_with(r"\??\Z:\"),
            "Portable Proton requires Z: image mappings.",
        )?;
        let path = source(&mapping.name);
        let relative = path
            .strip_prefix(original)
            .map_err(|_| Error::Invalid("An image mapping is outside the game directory."))?;
        let name = relative
            .to_str()
            .ok_or(Error::Invalid("Invalid mapped path."))?;
        require(
            files::relative(name)
                && images
                    .insert(
                        name.to_lowercase(),
                        (relative.to_path_buf(), mapping.file.as_raw_fd()),
                    )
                    .is_none(),
            "Duplicate or invalid image mapping.",
        )?;
    }
    let mut linked = BTreeSet::new();
    let mut pending = vec![PathBuf::new()];
    let mut count = 0;
    while let Some(relative) = pending.pop() {
        require(
            relative.components().count() < 128,
            "Mapped game image nesting is too deep.",
        )?;
        let mut seen = BTreeSet::new();
        for entry in fs::read_dir(original.join(&relative))? {
            let entry = entry?;
            if skip(&entry.path(), view) {
                continue;
            }
            count += 1;
            require(
                count <= 1_000_000,
                "The game directory contains too many entries.",
            )?;
            let rel = relative.join(entry.file_name());
            let key = rel
                .to_str()
                .ok_or(Error::Invalid("Invalid game resource path."))?
                .to_lowercase();
            require(seen.insert(key.clone()), "Ambiguous game image paths.")?;
            let target = view.join(&rel);
            if let Some((_, fd)) = images.get(&key) {
                symlink(format!("/proc/{}/fd/{fd}", std::process::id()), &target)?;
                linked.insert(key);
            } else if images.keys().any(|v| v.starts_with(&format!("{key}/"))) {
                require(
                    entry.file_type()?.is_dir(),
                    "Invalid mapped image directory.",
                )?;
                files::private_dir(&target)?;
                pending.push(rel);
            } else {
                symlink(entry.path(), target)?;
            }
        }
    }
    require(
        linked == images.keys().cloned().collect(),
        "Mapped image is missing from the game directory.",
    )
}
pub fn run(root: &Path, arguments: &[OsString]) -> Result<u8> {
    let root = root.canonicalize()?;
    let raw = arguments
        .first()
        .and_then(|v| v.to_str())
        .ok_or(Error::Invalid(
            "Expected the executable path supplied by xodus-cli run.",
        ))?;
    let source = source(raw).canonicalize()?;
    let game = Game::for_runtime(&root)?;
    let game_root = game.path(&root).canonicalize()?;
    require(
        source.starts_with(&game_root)
            && source
                .file_name()
                .is_some_and(|v| v.eq_ignore_ascii_case(game.executable())),
        "The entry point does not match the configured game.",
    )?;
    let mapped = mappings(&std::env::var("WINE_DLL_FILE_MAP").unwrap_or_default())?;
    let payload = mapped
        .iter()
        .find(|m| nt(&source).is_ok_and(|name| m.name.to_lowercase() == name.to_lowercase()))
        .ok_or(Error::Invalid(
            "The entry point is not present in Xodus' inherited fd map.",
        ))?;
    let headers = pe_headers(&payload.file)?;
    let parent = source
        .parent()
        .ok_or(Error::Invalid("Invalid entry point path."))?;
    let view = View {
        path: tx::new_directory(parent, ".xodus-launch-")?,
    };
    let stub = view.path.join(
        source
            .file_name()
            .ok_or(Error::Invalid("Invalid entry point filename."))?,
    );
    let portable = std::env::var("FLIGHTDECK_PROTON_LOADER").is_ok_and(|v| v == "portable");
    if portable {
        portable_tree(parent, &view.path, &mapped)?;
    } else {
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            if entry.path() != source && !skip(&entry.path(), &view.path) {
                symlink(entry.path(), view.path.join(entry.file_name()))?;
            }
        }
        let mut file = File::create(&stub)?;
        file.write_all(&headers)?;
        file.set_len(payload.file.metadata()?.len())?;
    }
    let prefix = root.join("local/msfs-prefix").canonicalize()?;
    if let Some(value) = std::env::var_os("WINEPREFIX") {
        require(
            Path::new(&value).canonicalize()? == prefix,
            "The Wine prefix differs from this runtime configuration.",
        )?;
    }
    let wine = std::env::var_os("XODUS_WINE_RUNNER")
        .map(PathBuf::from)
        .unwrap_or(root.join("runner/files/bin/wine"))
        .canonicalize()?;
    require(
        runtime::executable(&wine),
        "The selected Wine runner is not executable.",
    )?;
    let mut env: BTreeMap<_, _> = std::env::vars_os().collect();
    env.insert("WINEPREFIX".into(), prefix.as_os_str().into());
    env.insert(
        "XODUS_STORE_PACKAGE_SCOPE".into(),
        "FlightdeckBaseGameOnlyV1".into(),
    );
    if portable {
        env.remove(std::ffi::OsStr::new("WINE_DLL_FILE_MAP"));
    } else {
        let parent_nt = format!("{}\\", nt(parent)?);
        let mut aliases = Vec::new();
        for mapping in &mapped {
            aliases.push(format!("{}:{}", mapping.file.as_raw_fd(), mapping.name));
            if mapping
                .name
                .to_lowercase()
                .starts_with(&parent_nt.to_lowercase())
            {
                aliases.push(format!(
                    "{}:{}\\{}",
                    mapping.file.as_raw_fd(),
                    nt(&view.path)?,
                    mapping
                        .name
                        .get(parent_nt.len()..)
                        .ok_or(Error::Invalid("Invalid mapped path alias."))?
                ));
            }
        }
        env.insert("WINE_DLL_FILE_MAP".into(), aliases.join("|").into());
    }
    // Install signal handlers before the game starts. Helpers never inherit any
    // licensed image descriptors; only the game process gets that explicit set.
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async{
        let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;let mut interrupt=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut guard=None;let guard_path=prefix.join("drive_c/windows/system32/FenixWindowGuard.exe");if std::env::var("WINE_FENIX_WINDOW_GUARD").is_ok_and(|v|v=="1")&&guard_path.is_file(){let mut c=Command::new(&wine);c.arg(&guard_path).env_clear().envs(&env).env_remove("WINE_DLL_FILE_MAP").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());guard=Some(OwnedChild(process::spawn(&mut c,None)?));}
        let mut refresh=None;if std::env::var("WINE_FENIX_DISPLAY_REFRESH").is_ok_and(|v|v=="1")&&prefix.join("drive_c/windows/system32/FenixMCDURefresh.exe").is_file(){let mut c=Command::new(std::env::current_exe()?);c.args(["display-refresh","--runtime"]).arg(&root).env_clear().envs(&env).env_remove("WINE_DLL_FILE_MAP").stdin(Stdio::null());refresh=Some(OwnedRefresh(process::spawn(&mut c,None)?));}
        let mut command=Command::new(&wine);command.arg(&stub).args(&arguments[1..]).env_clear().envs(&env).current_dir(if portable{&view.path}else{parent});let descriptors:Vec<_>=mapped.iter().map(|m|&m.file).collect();let mut game=OwnedChild(process::spawn_files(&mut command,&descriptors)?);let started=Instant::now();
        let status=loop{if let Some(code)=game.0.try_wait()?{break code;}let signal=tokio::select!{_ = term.recv()=>Some(rustix::process::Signal::TERM),_ = interrupt.recv()=>Some(rustix::process::Signal::INT),_ = tokio::time::sleep(std::time::Duration::from_millis(25))=>None};if let Some(signal)=signal&&let Some(pid)=rustix::process::Pid::from_raw(game.0.id() as i32){let _=rustix::process::kill_process(pid,signal);}};
        let code=status.code().unwrap_or_else(||128+status.signal().unwrap_or(1));eprintln!("xodus-wine-launch: wine_pid={} exit_code={code} elapsed_seconds={:.3}",game.0.id(),started.elapsed().as_secs_f64());drop(refresh);drop(guard);Ok(code.clamp(0,255) as u8)
    })
}
