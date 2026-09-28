#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the actual Linux Store webview with offline content, without accounts.

Requires the native build's Cargo environment and a desktop display.
The fixture forbids form submissions/network navigation and opens no real Store.
--legacy demonstrates the previous foreign-window attachment for comparison.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

RUST = r'''
// SPDX-License-Identifier: MIT
#[allow(dead_code)]
#[path = "../store_purchase.rs"]
mod store_purchase;
#[allow(dead_code)]
#[path = "../store_window_check.rs"]
mod store_window_check;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tao::{dpi::LogicalSize, event::Event, event_loop::{ControlFlow, EventLoopBuilder},
    platform::run_return::EventLoopExtRunReturn, window::WindowBuilder};
use wry::{WebViewBuilder, NewWindowResponse, raw_window_handle::{HasWindowHandle, RawWindowHandle}};

fn main() {
    let nonce = "a".repeat(32);
    let config = json!({"productId":"ABCD1234EFGH","skuId":"0001","availabilityId":"AVAIL1234567",
        "market":"AT","locale":"en-US","xToken":"SYNTHETIC-NOT-A-CREDENTIAL","expiresAt":4102444800_i64});
    let health = std::env::args().any(|s| s == "--health");
    let html = if health { store_window_check::html("de", &nonce) } else {store_purchase::render(&config, &nonce, "bootstrap")
        .replace("frame-src https://www.microsoft.com", "frame-src about:")
        .replace("form-action https://www.microsoft.com/store/purchase/buynowui/buynow", "form-action 'none'")
        .replace("<iframe id=", "<iframe srcdoc=\"<h1>Offline Store window test</h1><p>No account or payment service.</p>\" id=")};
    let mut events = EventLoopBuilder::<Value>::with_user_event().build();
    let window = WindowBuilder::new().with_title("Flightdeck offline Store test")
        .with_inner_size(LogicalSize::new(720., 820.)).build(&events).unwrap();
    let raw = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Xlib(w) => Some(w.window), _ => None,
    };
    println!("{}", json!({"window":raw,"pid":std::process::id()}));
    let proxy = events.create_proxy();
    let builder = WebViewBuilder::new().with_incognito(true).with_html(html)
        .with_devtools(false)
        .with_navigation_handler(|url| url.starts_with("about:") || url.starts_with("data:"))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_ipc_handler(move |request| {
            if let Ok(value) = serde_json::from_str::<Value>(request.body()) {
                let _ = proxy.send_event(value);
            }
        });
    let webview = if std::env::args().any(|s| s == "--legacy") {
        builder.build(&window).unwrap()
    } else { store_purchase::attach_webview(&window, builder).unwrap() };
    let start = Instant::now();
    let mut checked = false; let mut cancelled = false; let mut passed = false;
    events.run_return(|event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));
        if let Event::UserEvent(value) = event {
            if value["probe"] == "render" { println!("{}", value); }
            if value["nonce"] == nonce && (value["status"] == "cancel" || value["result"] == "cancelled") {
                passed = true; *flow = ControlFlow::Exit;
            }
        }
        if !checked && start.elapsed() > Duration::from_secs(3) {
            checked = true;
            webview.evaluate_script("window.ipc.postMessage(JSON.stringify({probe:'render',width:innerWidth,height:innerHeight,button:!!document.getElementById('cancel'),frame:!!document.getElementById('checkout')}))").unwrap();
        }
        if !cancelled && start.elapsed() > Duration::from_secs(8) {
            cancelled = true; webview.evaluate_script("document.getElementById('cancel').click()").unwrap();
        }
        if start.elapsed() > Duration::from_secs(15) { *flow = ControlFlow::Exit; }
    });
    assert!(passed, "Cancellation did not reach the native event loop");
    println!("{}", json!({"cancelled":true,"real_account_calls":0,"payment_requests":0}));
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", required=True, type=Path)
    parser.add_argument("--legacy", action="store_true")
    parser.add_argument("--health", action="store_true")
    parser.add_argument("--backend", choices=("x11", "wayland"), default="x11")
    args = parser.parse_args()
    stage = args.stage.resolve()
    output = Path(tempfile.mkdtemp(prefix="flightdeck-store-window-"))
    probe = stage / "xodus-src/crates/xodus-cli/src/bin/flightdeck-store-window-test.rs"
    probe.parent.mkdir(exist_ok=True)
    if probe.exists():
        raise RuntimeError("Refusing to replace an existing probe")
    try:
        probe.write_text(RUST)
        subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(stage / "xodus-src/Cargo.toml"),
                        "-p", "xodus-cli", "--bin", "flightdeck-store-window-test"], check=True)
    finally:
        probe.unlink(missing_ok=True)
    binary = Path(os.environ.get("CARGO_TARGET_DIR", stage / "xodus-src/target")) / "debug/flightdeck-store-window-test"
    with (output / "stderr.log").open("w") as errors:
        process = subprocess.Popen([str(binary), *(["--legacy"] if args.legacy else []), *(["--health"] if args.health else [])],
                                   stdout=subprocess.PIPE, stderr=errors, text=True,
                                   env={**os.environ, "GDK_BACKEND": args.backend})
        results = []
        try:
            for line in process.stdout:
                value = json.loads(line)
                results.append(value)
                if "window" in value:
                    window = value["window"]
                if value.get("probe") == "render":
                    if window is not None:
                        screenshot = ["import", "-window", str(window), str(output / "window.png")]
                    else:
                        clients = json.loads(subprocess.check_output(["hyprctl", "-j", "clients"]))
                        client = next(c for c in clients if c["pid"] == process.pid)
                        monitors = json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"]))
                        monitor = next(m for m in monitors if m["id"] == client["monitor"])
                        assert monitor["activeWorkspace"]["id"] == client["workspace"]["id"], "Test window is not visible"
                        x, y = client["at"]; width, height = client["size"]
                        screenshot = ["grim", "-g", f"{x},{y} {width}x{height}", str(output / "window.png")]
                    subprocess.run(screenshot,
                                   timeout=3, check=True, stdin=subprocess.DEVNULL)
            assert process.wait(timeout=20) == 0
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
    from PIL import Image
    with Image.open(output / "window.png") as screenshot:
        pixels = screenshot.width * screenshot.height
        counts = screenshot.convert("RGB").getcolors(pixels)
        white = sum(count for count, color in counts if min(color) > 235) / pixels
        colors = len(counts)
    render = next(v for v in results if v.get("probe") == "render")
    passed = render["width"] > 100 and render["height"] > 100 and render["button"] and (args.health or render["frame"]) and white > .1 and colors > 10
    report = {"passed": passed, "legacy_attachment": args.legacy, "backend": args.backend, "health_check": args.health, "white_fraction": white,
              "colors": colors, "events": results, "artifacts": str(output)}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    if not args.legacy:
        assert passed, "The real Linux webview did not render the offline fixture"


if __name__ == "__main__":
    main()
