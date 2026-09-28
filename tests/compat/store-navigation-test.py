#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Regress the native Store form's origin using a loopback-only HTTP fixture.

The fixture reproduces the empty HTTP 403 response to Origin: null. It never
uses a real account, Microsoft endpoint, product or payment. Run with the same
Cargo environment and desktop display as store-window-test.py.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs

RUST = r'''
#[allow(dead_code)]
#[path = "../store_purchase.rs"] mod store_purchase;
use serde_json::{Value,json};
use std::time::{Duration,Instant};
use tao::{dpi::LogicalSize,event::Event,event_loop::{ControlFlow,EventLoopBuilder},
    platform::run_return::EventLoopExtRunReturn,window::WindowBuilder};
use wry::{WebViewBuilder,NewWindowResponse,
    raw_window_handle::{HasWindowHandle,RawWindowHandle}};
fn main(){
    let origin=std::env::var("FIXTURE_ORIGIN").unwrap();
    assert!(origin.starts_with("http://127.0.0.1:"));
    let mode=std::env::args().nth(1).unwrap();
    let url=format!("{origin}/store/purchase/buynowui/prefetch/buynow");
    let nonce="a".repeat(32);
    let config=json!({"productId":"ABCD1234EFGH","skuId":"0001","availabilityId":"AVAIL1234567",
        "market":"AT","locale":"en-US","xToken":"SYNTHETIC-NOT-A-CREDENTIAL","expiresAt":4102444800_i64});
    let content=store_purchase::html(&config,&nonce).replace("https://www.microsoft.com",&origin);
    // Reproduce the previous about:blank host without the newly added guard.
    let initial=if mode=="legacy" {content.replace(&format!("location.origin !== '{origin}'"),"false").replace("content=\"same-origin\"","content=\"no-referrer\"")}
        else {store_purchase::render(&config,&nonce,"bootstrap").replace("https://www.microsoft.com",&origin)};
    let mut events=EventLoopBuilder::<Value>::with_user_event().build();
    let window=WindowBuilder::new().with_title("Flightdeck offline Store navigation")
        .with_inner_size(LogicalSize::new(720.,820.)).build(&events).unwrap();
    let raw=match window.window_handle().unwrap().as_raw(){RawWindowHandle::Xlib(w)=>Some(w.window),_=>None};
    println!("{}",json!({"window":raw,"pid":std::process::id()}));
    let proxy=events.create_proxy(); let loaded=events.create_proxy(); let expected=url.clone();
    let allowed=origin.clone()+"/";
    let builder=WebViewBuilder::new().with_incognito(true).with_html(initial).with_devtools(false)
        .with_navigation_handler(move|u|u=="about:blank"||u.starts_with(&allowed))
        .with_new_window_req_handler(|_,_|NewWindowResponse::Deny)
        .with_on_page_load_handler(store_purchase::bootstrap_listener(expected,move||{
            let _=loaded.send_event(json!({"loaded":true}));
        }))
        .with_ipc_handler(move|request|{if let Ok(value)=serde_json::from_str::<Value>(request.body()){let _=proxy.send_event(value);}});
    let webview=store_purchase::attach_webview(&window,builder).unwrap();
    let started=Instant::now(); let mut checked=false; let mut attached=false; let mut cancelled=false;
    events.run_return(|event,_,flow|{
        *flow=ControlFlow::WaitUntil(Instant::now()+Duration::from_millis(50));
        if let Event::UserEvent(value)=event {
            if value["phase"]=="bootstrap_started" && mode!="cancel" {webview.load_url(&url).unwrap();}
            if value["loaded"]==true && !attached {
                attached=true;
                let target=if mode=="wrong-document" {format!("{url}?unexpected=1")}else{url.clone()};
                webview.evaluate_script(&store_purchase::bootstrap_script(&content,&nonce,&target)).unwrap();
            }
            println!("{}",value);
            if value["status"].is_string(){cancelled=true;*flow=ControlFlow::Exit;}
        }
        if !checked&&started.elapsed()>Duration::from_secs(2){
            checked=true;
            webview.evaluate_script("window.ipc.postMessage(JSON.stringify({probe:'render',heading:document.getElementById('status')?.textContent,forms:document.forms.length,origin:location.origin}))").unwrap();
        }
        if started.elapsed()>Duration::from_secs(4){webview.evaluate_script("document.getElementById('cancel')?.click()").unwrap();}
        if started.elapsed()>Duration::from_secs(8){*flow=ControlFlow::Exit;}
    });
    // A rejected document is not replaced by the host and has no host button.
    assert!(cancelled || mode=="wrong-document");
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", required=True, type=Path)
    parser.add_argument("--backend", choices=("x11", "wayland"), default="x11")
    args = parser.parse_args()
    stage = args.stage.resolve()
    output = Path(tempfile.mkdtemp(prefix="flightdeck-store-navigation-"))
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            requests.append({"method": "GET", "path": self.path})
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(b"<!doctype html><html><head><title>Offline public bootstrap</title></head><body></body></html>")

        def do_POST(self):
            fields = parse_qs(self.rfile.read(int(self.headers["Content-Length"])).decode())
            origin = self.headers.get("Origin")
            allowed = origin == fixture_origin
            requests.append({"method": "POST", "status": 200 if allowed else 403, "origin": origin,
                             "synthetic_token": fields.get("xToken") == ["SYNTHETIC-NOT-A-CREDENTIAL"]})
            self.send_response(200 if allowed else 403)
            self.send_header("Content-Type", "text/html")
            if not allowed:
                self.send_header("Content-Security-Policy", "frame-ancestors 'none'")
            self.end_headers()
            if allowed:
                self.wfile.write(b'<!doctype html><html><head><meta charset="utf-8"></head><body><h1>Offline confirmation loaded</h1><p>No account or payment service.</p><script>parent.postMessage("ReactPurchaseReadyToRender","*")</script></body></html>')

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    fixture_origin = f"http://127.0.0.1:{server.server_port}"
    threading.Thread(target=server.serve_forever, daemon=True).start()
    probe = stage / "xodus-src/crates/xodus-cli/src/bin/flightdeck-store-navigation-test.rs"
    assert not probe.exists()
    try:
        probe.parent.mkdir(exist_ok=True)
        probe.write_text(RUST)
        subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(stage / "xodus-src/Cargo.toml"),
                        "-p", "xodus-cli", "--bin", "flightdeck-store-navigation-test"], check=True)
    finally:
        probe.unlink(missing_ok=True)
    binary = Path(os.environ.get("CARGO_TARGET_DIR", stage / "xodus-src/target")) / "debug/flightdeck-store-navigation-test"
    reports = []
    try:
        for mode in ("legacy", "fixed", "cancel", "wrong-document"):
            requests.clear()
            events = []
            with (output / f"{mode}-stderr.log").open("w") as errors:
                process = subprocess.Popen([str(binary), mode], stdout=subprocess.PIPE, stderr=errors, text=True,
                                           env={**os.environ, "GDK_BACKEND": args.backend, "FIXTURE_ORIGIN": fixture_origin})
                try:
                    for line in process.stdout:
                        value = json.loads(line)
                        events.append(value)
                        if "window" in value:
                            window = value["window"]
                        if value.get("probe") == "render" and mode in ("fixed", "legacy"):
                            if window is not None:
                                command = ["import", "-window", str(window), str(output / f"{mode}.png")]
                            else:
                                clients = json.loads(subprocess.check_output(["hyprctl", "-j", "clients"]))
                                client = next(c for c in clients if c["pid"] == process.pid)
                                x, y = client["at"]; width, height = client["size"]
                                command = ["grim", "-g", f"{x},{y} {width}x{height}", str(output / f"{mode}.png")]
                            subprocess.run(command, timeout=3, check=True, stdin=subprocess.DEVNULL)
                    (output / f"{mode}-events.json").write_text(json.dumps({"events": events, "requests": list(requests)}, indent=2))
                    assert process.wait(timeout=12) == 0
                finally:
                    if process.poll() is None:
                        process.kill(); process.wait(timeout=5)
            ready = any(v.get("phase") == "checkout_ready" for v in events)
            posts = [r for r in requests if r["method"] == "POST"]
            if mode == "legacy":
                assert len(posts) == 1 and posts[0]["origin"] == "null" and posts[0]["status"] == 403 and not ready
            elif mode == "fixed":
                assert len(posts) == 1 and posts[0]["origin"] == fixture_origin and posts[0]["status"] == 200 and ready
                assert any(v.get("heading") == "Microsoft Store" for v in events)
            else:
                assert not posts and not ready
                if mode == "wrong-document":
                    assert any(v.get("phase") == "bootstrap_error" for v in events)
            reports.append({"mode": mode, "requests": list(requests), "events": events})
    finally:
        server.shutdown()
        server.server_close()
    report = {"passed": True, "backend": args.backend, "real_account_calls": 0, "real_payment_requests": 0, "cases": reports}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": True, "cases": len(reports), "backend": args.backend, "artifacts": str(output)}))


if __name__ == "__main__":
    main()
