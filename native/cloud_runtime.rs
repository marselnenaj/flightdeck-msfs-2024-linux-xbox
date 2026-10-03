// SPDX-License-Identifier: MIT
//! Fresh authenticated helper sessions, isolated from the game's Wine server.
use crate::{
    bootstrap,
    cloud::{self, Failure, Result, require},
    cloud_fs as fs,
    cloud_pipe::Pipe,
    cloud_prefix::{Libraries, Prefix},
    cloud_storage::{self as cs, ReadOperation, Response, Scope, Transport},
    cloud_write::{LeaseReply, Operations},
    files, process, resources,
    xml::{self, Element, Item},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
const HELPER: &str = "bin/flightdeck-connected-storage.exe";
const READ_FEATURE: &str = "connected-storage-read-v1";
const WRITE_FEATURE: &str = "connected-storage-sync-v1";
fn nodes<'a>(root: &'a Element, name: &str, case_insensitive: bool, out: &mut Vec<&'a Element>) {
    let local = root.start.local_name();
    let actual = local.as_ref();
    if if case_insensitive {
        actual.eq_ignore_ascii_case(name)
    } else {
        actual == name
    } {
        out.push(root);
    }
    for item in &root.items {
        if let Item::Element(child) = item {
            nodes(child, name, case_insensitive, out);
        }
    }
}
fn one<'a>(root: &'a Element, name: &str) -> Result<&'a Element> {
    let mut matches = Vec::new();
    nodes(root, name, false, &mut matches);
    require(matches.len() == 1, "invalid_scope")?;
    Ok(matches[0])
}
pub fn config(runtime: &Path) -> Result<Value> {
    let parse = || -> Result<Value> {
        let game = crate::games::Game::for_runtime(runtime)?;
        let folder = game.path(runtime);
        let path = if folder.join("MicrosoftGame.Config").exists() {
            folder.join("MicrosoftGame.Config")
        } else {
            folder.join("MicrosoftGame.config")
        };
        let bytes = files::read(&path, 128 * 1024)?;
        let text = crate::game_package::text(&bytes)?;
        let root = xml::parse(&bytes)?;
        require(
            root.start.local_name().as_ref() == "Game"
                && one(&root, "StoreId")?.text() == game.store_id(),
            "invalid_scope",
        )?;
        let title = one(&root, "TitleId")?.text();
        require(
            !title.is_empty() && title.len() <= 8 && title.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid_scope",
        )?;
        let title_id =
            u32::from_str_radix(&title, 16).map_err(|_| Failure::new("invalid_scope"))?;
        let mut scids = Vec::new();
        nodes(&root, "scid", true, &mut scids);
        require(scids.len() <= 1, "invalid_scope")?;
        let scid = scids
            .first()
            .map(|v| v.text().to_ascii_lowercase())
            .unwrap_or_default();
        let identity = one(&root, "Identity")?;
        let attribute = |name: &str| -> Result<String> {
            let attr = identity
                .start
                .try_get_attribute(name)
                .map_err(|_| Failure::new("invalid_scope"))?
                .ok_or_else(|| Failure::new("invalid_scope"))?;
            Ok(attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|_| Failure::new("invalid_scope"))?
                .into_owned())
        };
        let name = attribute("Name")?;
        let publisher = attribute("Publisher")?;
        require(
            (3..=50).contains(&name.len())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                && !publisher.is_empty()
                && publisher.len() <= 8192,
            "invalid_scope",
        )?;
        let bytes: Vec<u8> = publisher
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let hash = Sha256::digest(&bytes);
        let bits = (u64::from_be_bytes(hash[..8].try_into().expect("hash prefix")) as u128) << 1;
        let alphabet = b"0123456789abcdefghjkmnpqrstvwxyz";
        let suffix: String = (0u8..=60)
            .step_by(5)
            .rev()
            .map(|shift| alphabet[((bits >> shift) & 31) as usize] as char)
            .collect();
        let pfn = format!("{name}_{suffix}");
        Scope::new(
            "1",
            if scid.is_empty() {
                "00000000-0000-0000-0000-000000000000"
            } else {
                &scid
            },
            &pfn,
            title_id,
        )?;
        Ok(json!({"op":"init","config":text,"scid":scid,"pfn":pfn,"title_id":title_id}))
    };
    parse().map_err(|_| Failure::new("invalid_scope"))
}
struct Paths {
    native: PathBuf,
    helper: PathBuf,
    wine: PathBuf,
    original: PathBuf,
    lock: Value,
}
fn paths(runtime: &Path, verify: bool, write: bool) -> Result<Paths> {
    let lock = resources::json("compat/bootstrap.lock.json")?;
    let features = lock["native"]["features"]
        .as_array()
        .ok_or_else(|| Failure::new("transport"))?;
    require(
        features.iter().any(|v| v == READ_FEATURE)
            && (!write || features.iter().any(|v| v == WRITE_FEATURE)),
        "transport",
    )?;
    let native = if verify {
        bootstrap::native_path(&lock)?
    } else {
        bootstrap::native_candidates(&lock)?.into_iter().find(|p| {
            bootstrap::artifact_names(&lock["native"])
                .is_ok_and(|names| names.iter().all(|name| p.join(name).is_file()))
        })
    }
    .ok_or_else(|| Failure::new("transport"))?;
    let helper = native.join(HELPER);
    require(!helper.is_symlink() && helper.is_file(), "transport")?;
    let selected = crate::proton::selection(runtime)?;
    let base = selected
        .as_ref()
        .and_then(|s| s["base_runner"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| runtime.join("runner"));
    let mut wine = base.join("files/bin/wine");
    if !wine.is_file() {
        let name = lock["runner"]["directory"]
            .as_str()
            .ok_or_else(|| Failure::new("transport"))?;
        require(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
            "transport",
        )?;
        wine = runtime
            .join("runner-research")
            .join(name)
            .join("files/bin/wine");
    }
    require(
        wine.is_file() && wine.metadata()?.mode() & 0o111 != 0,
        "transport",
    )?;
    wine = wine.canonicalize()?;
    let runner = wine
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Failure::new("transport"))?;
    let mut original = runner.join("lib/wine/x86_64-windows/xgameruntime.dll");
    if !original.is_file() {
        original =
            runtime.join("local/msfs-prefix/drive_c/windows/system32/xgameruntime_original.dll");
    }
    require(original.is_file(), "transport")?;
    if verify {
        require(
            files::sha256(&files::read(&original, 32 * 1024 * 1024)?)
                == lock["runner"]["original_runtime_sha256"],
            "transport",
        )?;
    }
    Ok(Paths {
        native,
        helper,
        wine,
        original,
        lock,
    })
}
pub fn available(runtime: &Path, write: bool) -> bool {
    paths(runtime, false, write).is_ok() && config(runtime).is_ok()
}
fn material(paths: &Paths) -> Result<(String, Libraries)> {
    let mut hashes = BTreeMap::new();
    let mut libraries = Libraries::new();
    for (name, expected) in paths.lock["native"]["files"]
        .as_object()
        .ok_or_else(|| Failure::new("transport"))?
    {
        require(files::relative(name), "transport")?;
        let data = files::read(&paths.native.join(name), 32 * 1024 * 1024)?;
        let hash = files::sha256(&data);
        require(expected.as_str() == Some(&hash), "transport")?;
        hashes.insert(name.clone(), hash);
        if name == "runtime/xgameruntime.dll" {
            libraries.insert("xgameruntime.dll".into(), data);
        } else if name == "builtin/x86_64-windows/xodus_store_test.dll" {
            libraries.insert("xodus_store_test.dll".into(), data);
        }
    }
    let original = files::read(&paths.original, 32 * 1024 * 1024)?;
    let original_hash = files::sha256(&original);
    require(
        paths.lock["runner"]["original_runtime_sha256"] == original_hash,
        "transport",
    )?;
    libraries.insert("xgameruntime_original.dll".into(), original);
    let wine_hash = files::sha256(&files::read(&paths.wine, 32 * 1024 * 1024)?);
    let server = paths
        .wine
        .parent()
        .ok_or_else(|| Failure::new("transport"))?
        .join("wineserver");
    let inputs = json!({"schema":1,"native":hashes,"runner":paths.lock["runner"],"wine_path":paths.wine.canonicalize()?,"native_path":paths.native.canonicalize()?,"original":original_hash,"wine":wine_hash,"wineserver":files::sha256(&files::read(&server,32*1024*1024)?)});
    Ok((files::sha256(&serde_json::to_vec(&inputs)?), libraries))
}
struct SocketDirectory {
    root: File,
    folder: File,
    name: String,
    path: PathBuf,
}
impl SocketDirectory {
    fn new() -> Result<Self> {
        let root = File::from(rustix::fs::open(
            "/tmp",
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?);
        let name = format!("flightdeck-cs-{}", uuid::Uuid::new_v4().simple());
        rustix::fs::mkdirat(&root, &name, rustix::fs::Mode::from_raw_mode(0o700))?;
        let folder = fs::child(&root, &name, false)?;
        Ok(Self {
            path: Path::new("/tmp").join(&name),
            root,
            folder,
            name,
        })
    }
}
impl Drop for SocketDirectory {
    fn drop(&mut self) {
        if fs::linked(&self.root, &self.name, &self.folder).is_ok() {
            let _ = crate::cloud_prefix::remove_tree(&self.root, &self.name, &mut 100000, 0);
        }
    }
}
pub struct Client {
    lease: File,
    private: File,
    runtime: PathBuf,
    scope: Option<Scope>,
    can_write: bool,
    pipe: Option<Pipe>,
    broker: Option<Child>,
    prefix: Prefix,
    socket: SocketDirectory,
    env: BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    wine: PathBuf,
    cancel: Arc<AtomicBool>,
    closed: bool,
}
impl Client {
    /// Retains the caller's owned runtime lease through helper cleanup.
    pub fn open(runtime: &Path, lease: &File, cancel: Arc<AtomicBool>) -> Result<Self> {
        let root = fs::open(runtime, false)?;
        let private = fs::child(&root, "private", false)?;
        fs::lease(&private, lease)?;
        let paths = paths(runtime, true, false)?;
        let (binding, libraries) = material(&paths)?;
        let mut config = config(runtime)?;
        if let Some(seed) = fs::optional(&private, "connected-storage-device.seed", 32)? {
            require(seed.len() == 32, "local_storage")?;
        }
        config["device_file"] = json!(format!(
            "Z:{}",
            runtime
                .join("private/connected-storage-device.seed")
                .display()
        ));
        let prefix = Prefix::new(runtime, &binding, libraries)?;
        let socket = SocketDirectory::new()?;
        let mut env: BTreeMap<_, _> = std::env::vars_os().collect();
        for key in [
            "WINE_DLL_FILE_MAP",
            "XODUS_KEYRING_FILE",
            "XODUS_LOCAL_GAMESAVE",
            "XODUS_LOCAL_GAMESAVE_ROOT",
        ] {
            env.remove(std::ffi::OsStr::new(key));
        }
        for (key, value) in [
            ("WINEARCH", "win64"),
            ("WINEESYNC", "0"),
            ("WINEFSYNC", "0"),
            ("WINEDEBUG", "-all"),
            ("XODUS_LOG", "off"),
            ("RUST_LOG", "off"),
            ("RUST_BACKTRACE", "0"),
            ("XODUS_USER_SOCKET_SUFFIX", "xodus.sock"),
            ("XODUS_USER_RUNTIME", "1"),
            (
                "WINEDLLOVERRIDES",
                "winemenubuilder.exe,mscoree,mshtml=d;xgameruntime=n;xgameruntime_original=n,b;xodus_store_test=b",
            ),
        ] {
            env.insert(key.into(), value.into());
        }
        env.insert("WINEPREFIX".into(), prefix.path().into_os_string());
        env.insert(
            "XDG_RUNTIME_DIR".into(),
            socket.path.clone().into_os_string(),
        );
        env.insert(
            "WINEDLLPATH".into(),
            paths.native.join("builtin").into_os_string(),
        );
        for category in ["config", "data", "cache", "state"] {
            env.insert(
                format!("XDG_{}_HOME", category.to_uppercase()).into(),
                runtime.join("private/xdg").join(category).into_os_string(),
            );
        }
        let mut client = Self {
            lease: lease.try_clone()?,
            private,
            runtime: runtime.into(),
            scope: None,
            can_write: false,
            pipe: None,
            broker: None,
            prefix,
            socket,
            env,
            wine: paths.wine,
            cancel,
            closed: false,
        };
        cloud::check(&client.cancel)?;
        if !client.prefix.reusable {
            client.run(
                &client.wine,
                ["wineboot", "-u"],
                Duration::from_secs(60),
                false,
            )?;
            client.prefix.install_libraries()?;
        }
        let broker = process::spawn(
            &mut client.command(&paths.native.join("bin/xodus-service")),
            None,
        )?;
        client.broker = Some(broker);
        let started = Instant::now();
        loop {
            cloud::check(&client.cancel)?;
            if client
                .socket
                .path
                .join("xodus.sock")
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_socket() && m.uid() == files::uid())
            {
                break;
            }
            require(
                client
                    .broker
                    .as_mut()
                    .is_some_and(|p| p.try_wait().is_ok_and(|s| s.is_none()))
                    && started.elapsed() < Duration::from_secs(20),
                "transport",
            )?;
            std::thread::sleep(Duration::from_millis(50));
        }
        let child = process::spawn(
            client
                .command(&client.wine)
                .arg(&paths.helper)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()),
            None,
        )?;
        client.pipe = Some(Pipe::new(child, Arc::clone(&client.cancel))?);
        let (answer, body) =
            client
                .pipe()?
                .exchange(&config, &[], 0, Duration::from_secs(120), false)?;
        require(
            matches!(answer["protocol"].as_u64(), Some(1 | 2)) && body.is_empty(),
            "invalid_response",
        )?;
        let scope = Scope::from_reply(&answer["scope"])?;
        require(
            (config["scid"] == "" || config["scid"] == scope.scid())
                && config["title_id"].as_u64() == Some(scope.title_id() as u64)
                && config["pfn"] == scope.pfn(),
            "invalid_scope",
        )?;
        client.can_write = answer["protocol"] == 2
            && answer["features"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v == WRITE_FEATURE));
        client.scope = Some(scope);
        Ok(client)
    }
    fn command(&self, program: &Path) -> Command {
        let mut c = Command::new(program);
        c.env_clear()
            .envs(&self.env)
            .current_dir(&self.prefix.work)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        c
    }
    fn run<const N: usize>(
        &self,
        program: &Path,
        args: [&str; N],
        timeout: Duration,
        cleanup: bool,
    ) -> Result<()> {
        let mut child = process::spawn(self.command(program).args(args), None)?;
        let deadline = Instant::now() + timeout;
        let result = (|| {
            loop {
                if let Some(status) = child.try_wait()? {
                    return require(status.success(), "transport");
                }
                if !cleanup {
                    cloud::check(&self.cancel)?;
                }
                require(Instant::now() < deadline, "deadline")?;
                std::thread::sleep(Duration::from_millis(25));
            }
        })();
        let _ = process::terminate_group(&mut child);
        result
    }
    fn pipe(&mut self) -> Result<&mut Pipe> {
        fs::lease(&self.private, &self.lease)?;
        self.pipe.as_mut().ok_or_else(|| Failure::new("transport"))
    }
    fn response(
        &mut self,
        request: &Value,
        body: &[u8],
        maximum: usize,
        timeout: Duration,
        cleanup: bool,
    ) -> Result<Response> {
        let result = (|| {
            let (answer, body) = self
                .pipe()?
                .exchange(request, body, maximum, timeout, cleanup)?;
            let status = cloud::number(&answer["status"], 599)? as u16;
            require(status >= 100, "invalid_response")?;
            let headers: BTreeMap<String, String> =
                serde_json::from_value(answer["headers"].clone())?;
            Ok(Response {
                status,
                headers: cs::headers(headers)?,
                body,
            })
        })();
        if result.is_err()
            && let Some(pipe) = &mut self.pipe
        {
            pipe.fail();
        }
        result
    }
    fn write(
        &mut self,
        mut request: Value,
        body: &[u8],
        timeout: Duration,
        cleanup: bool,
    ) -> Result<Response> {
        require(
            self.can_write && body.len() <= 64 * 1024 * 1024,
            "invalid_request",
        )?;
        request["body_bytes"] = json!(body.len());
        request["max_bytes"] = json!(4 * 1024 * 1024);
        self.response(&request, body, 4 * 1024 * 1024, timeout, cleanup)
    }
    fn lease_reply(&mut self, op: &str, timeout: Duration) -> Result<LeaseReply> {
        let response = self.write(json!({"op":op}), &[], timeout, false)?;
        if !matches!(response.status, 200 | 201) {
            return Ok(LeaseReply {
                status: response.status,
                owner_change_id: String::new(),
                quota_bytes: 0,
            });
        }
        let value = cloud::json(&response.body)?;
        let owner = cloud::text(&value["owner_change_id"], 1024, false)?.to_owned();
        require(owner.chars().count() <= 256, "invalid_response")?;
        let quota = cloud::number(&value["quota_bytes"], u64::MAX)?;
        require(quota > 0, "invalid_response")?;
        Ok(LeaseReply {
            status: response.status,
            owner_change_id: owner,
            quota_bytes: quota,
        })
    }
    pub fn writable(&self) -> bool {
        self.can_write
    }
    pub fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut clean = true;
        if let Some(mut pipe) = self.pipe.take() {
            clean &= pipe.close().is_ok();
        }
        if let Some(mut child) = self.broker.take() {
            clean &= process::terminate_group(&mut child).is_ok();
        }
        if let Some(parent) = self.wine.parent() {
            let server = parent.join("wineserver");
            let _ = self.run(&server, ["-k"], Duration::from_secs(10), true);
            clean &= self
                .run(&server, ["-w"], Duration::from_secs(10), true)
                .is_ok();
        } else {
            clean = false;
        }
        if clean {
            self.prefix.complete();
            Ok(())
        } else {
            Err(Failure::new("transport"))
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
impl Transport for Client {
    fn scope(&self) -> &Scope {
        self.scope
            .as_ref()
            .expect("client initialization precedes public use")
    }
    fn read(
        &mut self,
        op: ReadOperation<'_>,
        timeout: Duration,
        maximum: usize,
    ) -> Result<Response> {
        require(maximum <= 64 * 1024 * 1024, "invalid_request")?;
        let atom = matches!(op, ReadOperation::Atom(_));
        require(atom || maximum <= 4 * 1024 * 1024, "invalid_request")?;
        let mut request = match op {
            ReadOperation::Index {
                skip: 0,
                continuation: None,
            } => json!({"op":"index"}),
            ReadOperation::Index {
                skip,
                continuation: Some(token),
            } => {
                require((1..=4096).contains(&skip), "invalid_request")?;
                cloud::text(&json!(token), 8192, false)?;
                json!({"op":"index","skip_items":skip,"continuation_token":token})
            }
            ReadOperation::Index { .. } => return Err(Failure::new("invalid_request")),
            ReadOperation::Container(name) => {
                cloud::text(&json!(name), 1024, false)?;
                require(name != "." && name != "..", "invalid_request")?;
                json!({"op":"container","wire_name":name})
            }
            ReadOperation::Atom(id) => {
                require(cloud::guid(id), "invalid_request")?;
                json!({"op":"atom","atom":id})
            }
        };
        request["max_bytes"] = json!(maximum);
        self.response(&request, &[], maximum, timeout, false)
    }
}
impl Operations for Client {
    fn acquire(&mut self, timeout: Duration) -> Result<LeaseReply> {
        self.lease_reply("lease_acquire", timeout)
    }
    fn renew(&mut self, timeout: Duration) -> Result<LeaseReply> {
        self.lease_reply("lease_renew", timeout)
    }
    fn release(&mut self, timeout: Duration) -> Result<u16> {
        Ok(self
            .write(json!({"op":"lease_release"}), &[], timeout, true)?
            .status)
    }
    fn upload_atom(&mut self, payload: &[u8], timeout: Duration) -> Result<String> {
        let response = self.write(json!({"op":"atom_upload"}), payload, timeout, false)?;
        if !(200..300).contains(&response.status) {
            return Err(Failure::new(if response.status == 409 {
                "lease_lost"
            } else if matches!(response.status, 401 | 403) {
                "authentication"
            } else {
                "transport"
            })
            .status(response.status));
        }
        let value = cloud::json(&response.body)?;
        let atom = value["atom"]
            .as_str()
            .ok_or_else(|| Failure::new("invalid_response"))?;
        require(cloud::guid(atom), "invalid_response")?;
        Ok(atom.into())
    }
    fn put_container(
        &mut self,
        name: &str,
        display: &str,
        modified: u64,
        atoms: &BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<u16> {
        let seconds = i64::try_from(modified).map_err(|_| Failure::new("invalid_request"))?;
        let date = chrono::DateTime::from_timestamp(seconds, 0)
            .ok_or_else(|| Failure::new("invalid_request"))?;
        Ok(self.write(json!({"op":"container_put","wire_name":name,"display_name":display,"modified":date.to_rfc3339_opts(chrono::SecondsFormat::Secs,true),"atoms":atoms}),&[],timeout,false)?.status)
    }
    fn delete_container(&mut self, name: &str, timeout: Duration) -> Result<u16> {
        Ok(self
            .write(
                json!({"op":"container_delete","wire_name":name}),
                &[],
                timeout,
                false,
            )?
            .status)
    }
    fn remote(
        &mut self,
        timeout: Duration,
        force: &BTreeSet<String>,
    ) -> Result<crate::save_state::State> {
        let runtime = self.runtime.clone();
        let cancel = Arc::clone(&self.cancel);
        let limits = cs::Limits {
            deadline: timeout.min(Duration::from_secs(180)),
            ..cs::Limits::default()
        };
        let snapshot = crate::cloud_cache::download(&runtime, self, &limits, &cancel, force)?;
        crate::cloud_import::read_snapshot(&runtime, self.scope(), &snapshot)
    }
}
