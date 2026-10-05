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
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("HTTP fixture stopped");
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
            while !bytes.ends_with(b"\r\n\r\n") {
                let size = stream.read(&mut part).expect("request");
                assert!(size > 0);
                bytes.extend_from_slice(&part[..size]);
                assert!(bytes.len() < 16384);
            }
            let header = String::from_utf8(bytes).expect("request header");
            let request = header.lines().next().expect("request line");
            assert!(
                request.starts_with("GET "),
                "discovery must not mutate the installation"
            );
            let path = request
                .split_whitespace()
                .nth(1)
                .expect("path")
                .strip_prefix("/api/")
                .expect("API path");
            captured.lock().expect("requests").push(path.into());
            let payload = snapshot.get(path).unwrap_or(&Value::Null).to_string();
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
                .filter(|p| *p == "proton/discover")
                .count(),
            1
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
                .filter(|p| *p == "proton/discover")
                .count(),
            2
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
