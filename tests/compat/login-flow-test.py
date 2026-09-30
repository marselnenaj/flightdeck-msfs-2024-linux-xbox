#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise login challenge handoff using loopback pages and synthetic tokens."""
import argparse
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading


RUST = r'''
// SPDX-License-Identifier: MIT
// Synthetic pages and tokens only. Exercise the production GTK/WebKit runtime
// without opening Microsoft, initializing the keyring or exchanging credentials.
mod webview {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/webview.rs"));

    pub fn fixture_request(url: String) -> WebviewRequest {
        WebviewRequest::new("Flightdeck synthetic email verification", url, HeaderMap::new())
    }
}

use webview::{HandlerControl, RuntimeCommands, SessionHandler, SessionId};
use xodus::models::live::DAProperty;

struct Handler {
    origin: String,
    mode: String,
    step: usize,
}

impl SessionHandler for Handler {
    type Output = usize;

    fn bootstrap(&mut self, runtime: &mut RuntimeCommands) -> Result<(), Box<dyn std::error::Error>> {
        runtime.open_session(webview::fixture_request(format!("{}/password?{}", self.origin, self.mode)));
        Ok(())
    }

    fn on_token(&mut self, session: SessionId, data: DAProperty, runtime: &mut RuntimeCommands)
        -> Result<HandlerControl<usize>, Box<dyn std::error::Error>>
    {
        if self.mode == "response-error" {
            return Err(xodus::api::live::RSTError::InvalidResponse.into());
        }
        let expected = ["password", "email-code", "verified"][self.step];
        if data.username != expected {
            return Err("Unexpected or stale callback during verification".into());
        }
        self.step += 1;
        runtime.close_session(session);
        if self.step == 3 {
            return Ok(HandlerControl::Complete(self.step));
        }
        let next = if self.step == 1 { "email-code" } else { "verified" };
        runtime.open_session(webview::fixture_request(format!("{}/{next}?{}", self.origin, self.mode)));
        Ok(HandlerControl::Continue)
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap();
    let expect_error = mode == "response-error";
    let handler = Handler {
        origin: std::env::var("FIXTURE_ORIGIN").unwrap(),
        mode,
        step: 0,
    };
    match webview::run_sessions(handler) {
        Ok(Some(3)) if !expect_error => println!("Completed password, email code and verification"),
        Err(error) if expect_error && matches!(error.downcast_ref::<xodus::api::live::RSTError>(),
                                              Some(xodus::api::live::RSTError::InvalidResponse)) => {
            println!("Preserved response error classification");
        }
        _ => std::process::exit(1),
    }
}
'''

def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--backend", choices=("x11", "wayland"), default="x11")
    args = parser.parse_args()
    stage = args.stage.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            path, _, mode = self.path.partition("?")
            step = path.lstrip("/")
            cookie = self.headers.get("Cookie", "")
            cookie_ok = "verification=synthetic-session" in cookie
            requests.append({"step": step, "cookie_present": cookie_ok})
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            if step == "password":
                self.send_header("Set-Cookie", "verification=synthetic-session; Path=/; HttpOnly; SameSite=Strict")
            self.end_headers()
            payload = dict(sDAToken="SYNTHETIC-TOKEN", sDASessionKey="SYNTHETIC-KEY",
                           sDAStartTime="2026-09-30T12:00:00Z", sDAExpires="2026-09-30T13:00:00Z",
                           sSTSInlineFlowToken="SYNTHETIC-FLOW", sSigninName=step, K="SYNTHETIC-PUID")
            if mode in ("cookie", "combined") and step != "password" and not cookie_ok:
                payload["sSigninName"] = "missing-cookie"
            post = "window.external.notify(" + json.dumps(json.dumps(payload)) + ");"
            if mode in ("duplicate", "combined") and step != "verified":
                post += post
            page = ("<!doctype html><html><body><h1>" + step + "</h1>"
                    "<p>Offline email-code verification fixture</p><script>"
                    "requestAnimationFrame(() => setTimeout(() => {" + post + "}, 200));"
                    "</script></body></html>")
            self.wfile.write(page.encode())

    probe = stage / "xodus-src/crates/xodus-cli/src/bin/flightdeck-login-flow-test.rs"
    if probe.exists():
        raise RuntimeError("Probe already exists")
    probe.parent.mkdir(exist_ok=True)
    try:
        probe.write_text(RUST)
        with (args.output / "build.log").open("w") as log:
            subprocess.run(["cargo", "build", "--offline", "--locked", "--manifest-path",
                            str(stage / "xodus-src/Cargo.toml"), "-p", "xodus-cli",
                            "--bin", "flightdeck-login-flow-test"], stdout=log, stderr=log, check=True)
    finally:
        probe.unlink(missing_ok=True)

    binary = Path(os.environ.get("CARGO_TARGET_DIR", stage / "xodus-src/target")) / "debug/flightdeck-login-flow-test"
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    results = []
    try:
        for mode in ("normal", "duplicate", "cookie", "combined", "response-error"):
            requests.clear()
            with tempfile.TemporaryDirectory(prefix="login-profile-", dir=args.output) as profile:
                env = {**os.environ, "GDK_BACKEND": args.backend,
                       "FIXTURE_ORIGIN": f"http://127.0.0.1:{server.server_port}"}
                env.update({f"XDG_{kind.upper()}_HOME": str(Path(profile) / kind)
                            for kind in ("config", "cache", "data", "state")})
                with (args.output / f"{mode}.log").open("w") as log:
                    try:
                        result = subprocess.run([str(binary), mode], env=env, stdout=log, stderr=log, timeout=20)
                        exit_code = result.returncode
                    except subprocess.TimeoutExpired:
                        exit_code = "timeout"
                expected_steps = ["password"] if mode == "response-error" else ["password", "email-code", "verified"]
                passed = exit_code == 0 and [r["step"] for r in requests] == expected_steps
                results.append({"mode": mode, "passed": passed, "exit_code": exit_code,
                                "requests": list(requests)})
    finally:
        server.shutdown()
        server.server_close()
    (args.output / "result.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))
    return 0 if all(r["passed"] for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(run())
