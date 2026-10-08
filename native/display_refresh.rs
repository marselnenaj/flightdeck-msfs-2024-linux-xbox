// SPDX-License-Identifier: MIT
//! One bounded MCDU bitmap refresh per owned Fenix display process, with rollback.
use crate::{Error, Result, error::require, files, games::Game, process, runtime, wine_processes};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const SERVICES: &[&str] = &[
    "fenixdisplay.exe",
    "fenixsystem.exe",
    "fenixcdu.exe",
    "fenix.exe",
    "fenix.gqlgateway.exe",
];
const QUERY: &str = r#"{dataRef{home:dataRef(name:"fenix.controls.homeCockpit"){value}left:dataRef(name:"aircraft.mcdu1.display"){value}right:dataRef(name:"aircraft.mcdu2.display"){value}}}"#;
type Identities = BTreeMap<String, (i32, u64)>;
fn services(prefix: &Path) -> Result<Identities> {
    wine_processes::Processes::new(prefix)?.identities(SERVICES)
}
fn owns_api(services: &Identities) -> bool {
    (|| -> Result<bool> {
        let Some((pid, born)) = services.get("fenix.gqlgateway.exe") else {
            return Ok(false);
        };
        if wine_processes::birth(*pid)? != *born {
            return Ok(false);
        }
        let proc = PathBuf::from(format!("/proc/{pid}"));
        let mut sockets = BTreeSet::new();
        for (count, entry) in fs::read_dir(proc.join("fd"))?.enumerate() {
            require(count < 65536, "Too many Fenix service handles.")?;
            if let Ok(target) = fs::read_link(entry?.path()) {
                sockets.insert(target);
            }
        }
        let raw = files::read_public(&proc.join("net/tcp"), 8 * 1024 * 1024)?;
        for line in String::from_utf8_lossy(&raw).lines().skip(1) {
            let row: Vec<_> = line.split_whitespace().collect();
            if row.len() > 9
                && ["00000000:1F93", "0100007F:1F93"].contains(&row[1])
                && row[3] == "0A"
                && sockets.contains(&PathBuf::from(format!("socket:[{}]", row[9])))
            {
                return Ok(wine_processes::birth(*pid)? == *born);
            }
        }
        Ok(false)
    })()
    .unwrap_or(false)
}
struct Api {
    client: reqwest::blocking::Client,
    prefix: PathBuf,
}
fn local_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        // The owned Fenix endpoint is loopback HTTP, independent of system CAs.
        .tls_certs_only(std::iter::empty())
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| Error::Invalid("Fenix API unavailable."))
}
impl Api {
    fn query(&self, query: &str) -> Result<Value> {
        require(
            owns_api(&services(&self.prefix)?),
            "Fenix API ownership changed.",
        )?;
        let mut response = self
            .client
            .post("http://127.0.0.1:8083/graphql")
            .json(&json!({"query":query}))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|_| Error::Invalid("Fenix API unavailable."))?;
        require(
            response.status().as_u16() == 200,
            "Fenix API redirect rejected.",
        )?;
        let mut raw = Vec::new();
        response.by_ref().take(65537).read_to_end(&mut raw)?;
        require(raw.len() <= 65536, "Fenix API response too large.")?;
        let value: Value = serde_json::from_slice(&raw)?;
        require(
            (value["errors"].is_null() || value["errors"].as_array().is_some_and(Vec::is_empty))
                && value["data"]["dataRef"].is_object(),
            "Fenix API unavailable.",
        )?;
        Ok(value["data"]["dataRef"].clone())
    }
    fn home(&self, value: bool) -> Result<()> {
        let result = self.query(&format!(
            "mutation{{dataRef{{writeBool(name:\"fenix.controls.homeCockpit\",value:{value})}}}}"
        ))?;
        require(
            result["writeBool"] == true,
            "Fenix display preference was not accepted.",
        )
    }
    fn recover(&self, journal: &Path) -> Result<()> {
        if !files::exists(journal) {
            return Ok(());
        }
        let saved: Value = files::json(journal, 1024)?;
        require(
            saved["format"].as_u64() == Some(1) && saved["home"].is_boolean(),
            "Invalid display refresh journal.",
        )?;
        let value = saved["home"]
            .as_bool()
            .ok_or(Error::Invalid("Invalid display preference."))?;
        self.home(value)?;
        require(
            self.query(QUERY)?["home"]["value"] == value,
            "Display preference restoration pending.",
        )?;
        fs::remove_file(journal)?;
        files::directory(
            journal
                .parent()
                .ok_or(Error::Invalid("Invalid journal path."))?,
            false,
        )?
        .sync_all()?;
        Ok(())
    }
}
fn refresh(
    root: &Path,
    api: &Api,
    journal: &Path,
    identity: &Identities,
    cancel: &AtomicBool,
) -> Result<bool> {
    let prefix = root.join("local/msfs-prefix");
    require(
        services(&prefix)? == *identity && owns_api(identity),
        "Fenix process changed.",
    )?;
    let state = api.query(QUERY)?;
    let original = state["home"]["value"]
        .as_bool()
        .ok_or(Error::Invalid("MCDU display data not ready."))?;
    require(
        ["left", "right"].iter().all(|v| {
            state[v]["value"]
                .as_str()
                .is_some_and(|v| v.contains("<root>"))
        }),
        "MCDU display data not ready.",
    )?;
    let helper = prefix.join("drive_c/windows/system32/FenixMCDURefresh.exe");
    let library = root.join("games/MSFS2024/SimConnect_internal.dll");
    require(
        helper.is_file() && library.is_file(),
        "Installed display refresh dependency unavailable.",
    )?;
    let run = |mode: &str| -> Result<()> {
        let mut command = Command::new(runtime::wine(&root.join("runner")));
        command
            .arg(&helper)
            .arg(mode)
            .arg(format!(
                "Z:{}",
                library.to_string_lossy().replace('/', "\\")
            ))
            .current_dir(
                helper
                    .parent()
                    .ok_or(Error::Invalid("Invalid display helper path."))?,
            )
            .env("WINEPREFIX", &prefix)
            .env("WINEDEBUG", "-all")
            .stdin(Stdio::null());
        for key in [
            "WINE_DLL_FILE_MAP",
            "WINESERVERSOCKET",
            "WINEPRELOADRESERVE",
            "WINELOADERNOEXEC",
            "WINELOADER",
            "WINEDLLPATH",
        ] {
            command.env_remove(key);
        }
        process::output(&mut command, Duration::from_secs(15), 65536, cancel)?;
        Ok(())
    };
    run("--probe")?;
    if cancel.load(Ordering::Relaxed) || services(&prefix)? != *identity {
        return Ok(false);
    }
    files::atomic_json(journal, &json!({"format":1,"home":original}))?;
    let result = (|| {
        api.home(!original)?;
        std::thread::sleep(Duration::from_millis(350));
        if original {
            api.home(true)?;
            std::thread::sleep(Duration::from_millis(350));
        }
        run("--refresh")
    })();
    let recovered = api.recover(journal);
    result.and(recovered)?;
    Ok(true)
}
fn worker(root: &Path, cancel: &AtomicBool) -> Result<()> {
    if Game::for_runtime(root)? != Game::Msfs2024 {
        return Ok(());
    }
    let _lease = match files::Lease::acquire(&root.join("private/fenix-display-refresh.lock"), true)
    {
        Ok(v) => v,
        Err(Error::Invalid("Das Spiel oder ein anderer Runtimevorgang läuft bereits.")) => {
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let client = local_client()?;
    let api = Api {
        client,
        prefix: root.join("local/msfs-prefix"),
    };
    let journal = root.join("private/fenix-display-refresh.json");
    let mut seen = None;
    let mut completed = None;
    let mut since = Instant::now();
    let mut attempts = 0;
    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..10 {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let identity = services(&api.prefix).unwrap_or_default();
        let display = identity.get("fenixdisplay.exe").copied();
        if identity.len() != 5 || display.is_none() || !owns_api(&identity) {
            seen = None;
            since = Instant::now();
            continue;
        }
        if display != seen {
            seen = display;
            since = Instant::now();
            attempts = 0;
        }
        if files::exists(&journal) && api.recover(&journal).is_err() {
            continue;
        }
        if display == completed || attempts >= 3 || since.elapsed() < Duration::from_secs(8) {
            continue;
        }
        match refresh(root, &api, &journal, &identity, cancel) {
            Ok(true) => {
                completed = display;
                println!("Fenix: MCDU startup image refreshed; display preference restored.");
            }
            Ok(false) => {}
            Err(_) => {
                attempts += 1;
                since = Instant::now();
                println!("Fenix: display refresh deferred; waiting for local display services.");
            }
        }
    }
    if owns_api(&services(&api.prefix).unwrap_or_default()) {
        let _ = api.recover(&journal);
    }
    Ok(())
}
pub fn run(root: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let cancel = Arc::new(AtomicBool::new(false));
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async move{
        let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;let mut interrupt=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;let stopped=Arc::clone(&cancel);let mut task=tokio::task::spawn_blocking(move||worker(&root,&stopped));
        tokio::select!{result=&mut task=>return result.map_err(|_|Error::Invalid("The display refresh helper failed."))?,_ = term.recv()=>{},_ = interrupt.recv()=>{}}cancel.store(true,Ordering::Relaxed);task.await.map_err(|_|Error::Invalid("The display refresh helper failed."))?
    })
}

#[cfg(test)]
mod local_http_tests {
    use super::*;
    use std::{io::Write, net::TcpListener};

    #[test]
    fn fenix_loopback_transport_does_not_require_system_cas() {
        const CHILD: &str = "FLIGHTDECK_TEST_EMPTY_CA_FENIX";
        if std::env::var_os(CHILD).is_none() {
            let temp = tempfile::tempdir().expect("CA fixture");
            let ca = temp.path().join("empty.pem");
            fs::write(&ca, b"").expect("empty CA file");
            let result = Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "display_refresh::local_http_tests::fenix_loopback_transport_does_not_require_system_cas", "--nocapture"])
                .env(CHILD, "1").env("SSL_CERT_FILE", ca).env("SSL_CERT_DIR", temp.path())
                .env("HTTP_PROXY", "http://127.0.0.1:9").env("ALL_PROXY", "http://127.0.0.1:9").env("NO_PROXY", "")
                .env("http_proxy", "http://127.0.0.1:9").env("all_proxy", "http://127.0.0.1:9").env("no_proxy", "")
                .status().expect("isolated Fenix HTTP test");
            assert!(result.success(), "empty-CA child failed");
            return;
        }
        assert!(reqwest::blocking::Client::builder().build().is_err());
        let client = local_client().expect("loopback client without system CAs");
        let listener = TcpListener::bind("127.0.0.1:0").expect("local endpoint");
        listener.set_nonblocking(true).expect("bounded listener");
        let address = listener.local_addr().expect("port");
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "local request did not arrive");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("request deadline");
            let mut headers = Vec::new();
            let mut byte = [0_u8];
            while !headers.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("request");
                headers.push(byte[0]);
                assert!(headers.len() <= 8192);
            }
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("response");
        });
        let response = client
            .get(format!("http://{address}/graphql"))
            .send()
            .expect("real local HTTP request");
        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect must not be followed"
        );
        server.join().expect("local endpoint stopped");
    }
}
