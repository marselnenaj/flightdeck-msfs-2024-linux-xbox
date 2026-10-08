//! Local HTTP must work without system TLS roots; external TLS remains fail-closed.
use flightdeck_ui::Client;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn authenticated_loopback_clients_work_without_system_ca_certificates() {
    const CHILD: &str = "FLIGHTDECK_TEST_EMPTY_CA_LOOPBACK";
    if std::env::var_os(CHILD).is_none() {
        let temp = tempfile::tempdir().expect("CA fixture");
        let ca = temp.path().join("empty.pem");
        std::fs::write(&ca, b"").expect("empty CA file");
        let result = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "authenticated_loopback_clients_work_without_system_ca_certificates",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("SSL_CERT_FILE", ca)
            .env("SSL_CERT_DIR", temp.path())
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "")
            .env("http_proxy", "http://127.0.0.1:9")
            .env("all_proxy", "http://127.0.0.1:9")
            .env("no_proxy", "")
            .status()
            .expect("isolated client test");
        assert!(result.success(), "empty-CA child failed");
        return;
    }
    assert!(
        reqwest::blocking::Client::builder().build().is_err(),
        "fixture must reproduce an unavailable system root store"
    );
    const TOKEN: &str = "synthetic-local-client-token-000000";
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server");
    listener.set_nonblocking(true).expect("nonblocking server");
    let port = listener.local_addr().expect("port").port();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut count = 0;
        while count < 4 {
            let (mut stream, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "local requests were not received"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("accept failed: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read deadline");
            let mut request = Vec::new();
            let mut byte = [0_u8];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("request headers");
                request.push(byte[0]);
                assert!(request.len() <= 8192);
            }
            let request = String::from_utf8(request).expect("HTTP headers");
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains(&format!("x-flightdeck-token: {TOKEN}"))
            );
            let redirect = request.starts_with("GET /api/redirect ");
            assert!(redirect || request.starts_with("GET /api/status "));
            let body = if redirect {
                "{}".into()
            } else {
                json!({"app":{"name":"Flightdeck"},"csrf_token":TOKEN,"runtime":{},"game":{}})
                    .to_string()
            };
            let status = if redirect {
                "302 Found\r\nLocation: http://127.0.0.1:9/forbidden"
            } else {
                "200 OK"
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("local response");
            count += 1;
        }
    });
    let record = json!({"port":port,"token":TOKEN});
    assert!(flightdeck::desktop::request(&record, "/api/status", None).is_some());
    assert!(flightdeck::desktop::request(&record, "/api/redirect", None).is_none());
    let client = Client::new(port, TOKEN.into()).expect("UI client without system CAs");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let status = client
                .discover("status", "en")
                .await
                .expect("authenticated local status");
            assert!(status.get("csrf_token").is_none(), "token remains private");
            assert!(
                client.discover("redirect", "en").await.is_err(),
                "redirects must remain rejected"
            );
        });
    server.join().expect("bounded local server");
}
