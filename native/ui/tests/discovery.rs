//! Exercise the actual native controller's asynchronous requests, not a prefilled menu.
mod common;
use flightdeck_ui::{App, Client, Message, Page};
use iced::futures::StreamExt;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !thread::panicking() {
                result.expect("HTTP fixture stopped");
            }
        }
    }
}
fn server() -> Server {
    let fixture = common::localized_fixture(flightdeck_ui::Language::De);
    let mut snapshot = fixture.snapshot;
    snapshot.get_mut("status").expect("status")["app"] = json!({"name":"Flightdeck"});
    snapshot.get_mut("status").expect("status")["csrf_token"] =
        json!("synthetic-session-000000000000000000000000");
    snapshot.get_mut("proton").expect("proton")["experimental"] = json!(true);
    snapshot.get_mut("proton").expect("proton")["selected"] = json!("cachyos-10.0-sunset");
    snapshot.get_mut("proton").expect("proton")["selected_path"] =
        json!("/synthetic/Steam/proton-cachyos");
    snapshot.extend(fixture.discoveries);
    snapshot.insert("setup/discover", json!({"runtimes":[]}));
    let socket = TcpListener::bind("127.0.0.1:0").expect("local fixture");
    socket.set_nonblocking(true).expect("nonblocking");
    let port = socket.local_addr().expect("port").port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let thread = thread::spawn(move || {
        while !stopped.load(Ordering::Acquire) {
            let (mut stream, _) = match socket.accept() {
                Ok(value) => value,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(e) => panic!("fixture: {e}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut bytes = Vec::new();
            let mut part = [0; 4096];
            let header_end = loop {
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break end + 4;
                }
                let size = stream.read(&mut part).expect("request");
                assert!(size > 0);
                bytes.extend_from_slice(&part[..size]);
                assert!(bytes.len() < 16384);
            };
            let header = String::from_utf8(bytes[..header_end].to_vec()).expect("request header");
            let request = header.lines().next().expect("request line");
            let method = request.split_whitespace().next().expect("method");
            let path = request
                .split_whitespace()
                .nth(1)
                .expect("path")
                .strip_prefix("/api/")
                .expect("API path");
            let headers: std::collections::BTreeMap<_, _> = header
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':'))
                .map(|(key, value)| (key.to_ascii_lowercase(), value.trim()))
                .collect();
            assert!(
                !headers.contains_key("transfer-encoding"),
                "bounded fixture requests only"
            );
            let length = headers
                .get("content-length")
                .map(|value| value.parse::<usize>().expect("content length"))
                .unwrap_or(0);
            assert!(length <= 4096, "bounded request body");
            while bytes.len() < header_end + length {
                let size = stream.read(&mut part).expect("request body");
                assert!(size > 0);
                bytes.extend_from_slice(&part[..size]);
            }
            assert_eq!(
                bytes.len(),
                header_end + length,
                "one request per connection"
            );
            let payload = match method {
                "GET" => {
                    assert_eq!(length, 0, "discovery has no request body");
                    snapshot
                        .get(path)
                        .expect("known discovery resource")
                        .to_string()
                }
                "POST" => {
                    assert_eq!(
                        path, "updates/check-startup",
                        "no installation or download action"
                    );
                    assert_eq!(
                        headers.get("x-flightdeck-token"),
                        Some(&"synthetic-session-000000000000000000000000")
                    );
                    assert_eq!(
                        headers.get("origin").copied(),
                        Some(format!("http://127.0.0.1:{port}").as_str())
                    );
                    assert_eq!(
                        headers.get("x-flightdeck-context"),
                        Some(&"\"/synthetic/msfs2024\"")
                    );
                    assert_eq!(headers.get("content-type"), Some(&"application/json"));
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes[header_end..]).expect("JSON body"),
                        json!({})
                    );
                    json!({"ok":true}).to_string()
                }
                _ => panic!("unexpected fixture method: {method}"),
            };
            captured
                .lock()
                .expect("requests")
                .push((method.into(), path.into()));
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).expect("response");
        }
    });
    Server {
        port,
        requests,
        stop,
        thread: Some(thread),
    }
}
async fn run(app: &mut App, task: iced::Task<Message>) {
    let mut tasks = vec![task];
    let mut count = 0;
    while let Some(task) = tasks.pop() {
        if let Some(mut stream) = iced_test::runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    count += 1;
                    assert!(count < 20, "unexpected refresh loop");
                    tasks.push(app.update(message));
                }
            }
        }
    }
}
#[test]
fn opening_setup_discovers_runners_and_selects_the_active_one_without_a_search_click() {
    let server = server();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let mut app = App::default();
        app.startup_checked = true;
        app.client = Some(
            Client::new(
                server.port,
                "synthetic-session-000000000000000000000000".into(),
            )
            .expect("client"),
        );
        let task = app.update(Message::Navigate(Page::Setup));
        run(&mut app, task).await;
        assert!(app.online);
        assert_eq!(
            app.discoveries["proton/discover"]["choices"]
                .as_array()
                .expect("choices")
                .len(),
            2
        );
        assert_eq!(app.forms.get("proton"), "/synthetic/Steam/proton-cachyos");
        assert_eq!(
            server
                .requests
                .lock()
                .expect("requests")
                .iter()
                .filter(|(method, path)| method == "GET" && path == "proton/discover")
                .count(),
            1
        );
        assert_eq!(
            server
                .requests
                .lock()
                .expect("requests")
                .iter()
                .filter(|(method, path)| method == "POST" && path == "updates/check-startup")
                .count(),
            1,
            "initial runtime discovery also checks updates without a search click"
        );
        let _ = app.update(Message::Field("proton", "custom".into()));
        let task = app.update(Message::Refresh);
        run(&mut app, task).await;
        assert_eq!(app.forms.get("proton"), "custom");
        assert_eq!(
            server
                .requests
                .lock()
                .expect("requests")
                .iter()
                .filter(|(method, path)| method == "GET" && path == "proton/discover")
                .count(),
            2
        );
        assert_eq!(
            server
                .requests
                .lock()
                .expect("requests")
                .iter()
                .filter(|(method, _)| method == "POST")
                .count(),
            1,
            "refresh must neither repeat the background check nor install anything"
        );
    });
}

#[test]
fn navigation_cancels_superseded_reads_without_waiting_for_http() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let mut app = common::fixture();
        app.client =
            Some(Client::new(1, "synthetic-session-0000000000000000".into()).expect("client"));
        let old = app.update(Message::Navigate(Page::Diagnostics));
        let _new = app.update(Message::Navigate(Page::Overview));
        let mut old = iced_test::runtime::task::into_stream(old).expect("old read task");
        assert!(
            tokio::time::timeout(Duration::from_secs(1), old.next())
                .await
                .expect("cancelled read completes immediately")
                .is_none()
        );
        assert!(app.online);
        assert_eq!(app.page, Page::Overview);
        let _launch = app.update(Message::Action(flightdeck_ui::Action::Launch));
        let _ = app.update(Message::Navigate(Page::Mods));
        assert!(
            app.pending,
            "navigation must never discard a submitted mutation"
        );
    });
}
