// SPDX-License-Identifier: MIT
//! Loopback HTTP contract with strict origin, request-size and CSRF validation.
use crate::{Error, Result, api, backend::Launcher, i18n, resources};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::State,
    http::{Request, Response, StatusCode, header},
    routing::any,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use subtle::ConstantTimeEq;
#[derive(Clone)]
pub struct Service {
    pub launcher: Arc<Launcher>,
    pub token: String,
    pub port: u16,
    pub desktop: bool,
    pub pending: Arc<AtomicBool>,
    pub shutdown: Arc<tokio::sync::Notify>,
}
fn reply(status: StatusCode, body: impl Into<Bytes>, kind: &str) -> Response<Body> {
    let body = body.into();
    Response::builder().status(status).header(header::CONTENT_TYPE,kind).header(header::CONTENT_LENGTH,body.len()).header(header::CACHE_CONTROL,"no-store").header("X-Content-Type-Options","nosniff").header("Referrer-Policy","no-referrer").header("Cross-Origin-Resource-Policy","same-origin").header("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'").body(Body::from(body)).expect("constant response headers")
}
fn json_reply(
    status: StatusCode,
    mut value: Value,
    locale: &str,
    translate: bool,
) -> Response<Body> {
    if translate {
        i18n::localize(&mut value, locale);
    }
    match serde_json::to_vec(&value) {
        Ok(data) => reply(status, data, "application/json; charset=utf-8"),
        Err(_) => reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            b"{}".to_vec(),
            "application/json",
        ),
    }
}
fn error(status: StatusCode, message: &str, locale: &str) -> Response<Body> {
    json_reply(status, json!({"ok":false,"error":message}), locale, true)
}
async fn handle(State(service): State<Service>, request: Request<Body>) -> Response<Body> {
    let locale = i18n::language(
        request
            .headers()
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|h| h.to_str().ok()),
    );
    let headers = request.headers();
    let authorities = [
        format!("127.0.0.1:{}", service.port),
        format!("localhost:{}", service.port),
    ];
    let hosts = headers.get_all(header::HOST).iter().collect::<Vec<_>>();
    if hosts.len() != 1
        || !hosts[0]
            .to_str()
            .ok()
            .is_some_and(|h| authorities.iter().any(|a| a == h))
    {
        return error(
            StatusCode::FORBIDDEN,
            "Nur lokaler Zugriff ist erlaubt.",
            locale,
        );
    }
    let origins = headers.get_all(header::ORIGIN).iter().collect::<Vec<_>>();
    if !origins.is_empty()
        && (origins.len() != 1
            || !origins[0]
                .to_str()
                .ok()
                .is_some_and(|h| authorities.iter().any(|a| format!("http://{a}") == h)))
    {
        return error(
            StatusCode::FORBIDDEN,
            "Diese Anfrage kommt nicht von Flightdeck.",
            locale,
        );
    }
    if headers
        .get("Sec-Fetch-Site")
        .is_some_and(|v| v == "cross-site")
    {
        return error(
            StatusCode::FORBIDDEN,
            "Zugriff von einer anderen Website ist gesperrt.",
            locale,
        );
    }
    let path = request.uri().path().to_string();
    let is_get = request.method() == axum::http::Method::GET;
    let is_post = request.method() == axum::http::Method::POST;
    if !is_get && !is_post {
        return error(StatusCode::METHOD_NOT_ALLOWED, "Nicht gefunden.", locale);
    }
    let mut payload = json!({});
    if is_post {
        let tokens = headers
            .get_all("X-Flightdeck-Token")
            .iter()
            .collect::<Vec<_>>();
        if tokens.len() != 1
            || tokens[0]
                .as_bytes()
                .ct_eq(service.token.as_bytes())
                .unwrap_u8()
                != 1
        {
            return error(
                StatusCode::FORBIDDEN,
                "Die Sitzung ist ungültig. Bitte die Seite neu laden.",
                locale,
            );
        }
        if headers.contains_key(header::TRANSFER_ENCODING)
            || !headers
                .get(header::CONTENT_TYPE)
                .and_then(|s| s.to_str().ok())
                .is_some_and(|s| {
                    s.split(';')
                        .next()
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/json"))
                })
        {
            return error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Eine JSON-Anfrage ist erforderlich.",
                locale,
            );
        }
        let lengths = headers
            .get_all(header::CONTENT_LENGTH)
            .iter()
            .collect::<Vec<_>>();
        let length = if lengths.len() == 1 {
            lengths[0]
                .to_str()
                .ok()
                .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                .and_then(|v| v.parse::<usize>().ok())
        } else {
            None
        };
        if length.is_none_or(|n| n > 16384) {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Die Anfrage ist zu groß oder unvollständig.",
                locale,
            );
        }
        let body = match tokio::time::timeout(
            Duration::from_secs(10),
            to_bytes(request.into_body(), 16384),
        )
        .await
        {
            Ok(Ok(body)) if Some(body.len()) == length => body,
            _ => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "Die Anfrage enthält kein gültiges JSON-Objekt.",
                    locale,
                );
            }
        };
        payload = match crate::cloud::json(&body) {
            Ok(value) if matches!(value, Value::Object(_)) => value,
            _ => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "Die Anfrage enthält kein gültiges JSON-Objekt.",
                    locale,
                );
            }
        };
    }
    if is_get && !path.starts_with("/api/") {
        let name = if path == "/" {
            "index.html"
        } else {
            path.trim_start_matches('/')
        };
        if !name.contains('/')
            && let Some(data) = resources::asset(&format!("ui/{name}"))
        {
            let content = match name.rsplit('.').next().unwrap_or("") {
                "html" => "text/html; charset=utf-8",
                "js" => "text/javascript; charset=utf-8",
                "css" => "text/css; charset=utf-8",
                "svg" => "image/svg+xml",
                "png" => "image/png",
                "woff2" => "font/woff2",
                _ => return error(StatusCode::NOT_FOUND, "Nicht gefunden.", locale),
            };
            return reply(StatusCode::OK, Bytes::from_static(data), content);
        }
        return error(StatusCode::NOT_FOUND, "Nicht gefunden.", locale);
    }
    if is_post && path == "/api/desktop/refresh" {
        if !service.desktop {
            return json_reply(
                StatusCode::OK,
                json!({"ok":true,"refresh":"unsupported"}),
                locale,
                true,
            );
        }
        service.pending.store(true, Ordering::Relaxed);
        let result = service.launcher.refresh();
        if result["refresh"] == "restarting" {
            service.shutdown.notify_one();
        }
        return json_reply(StatusCode::OK, result, locale, true);
    }
    let app = Arc::clone(&service.launcher);
    let target = path.clone();
    let port = service.port;
    let result = tokio::task::spawn_blocking(move || {
        if is_get {
            api::get(&app, &target)
        } else {
            if let Err(e) = Launcher::open(&app.lock()) {
                return Some(Err(e));
            }
            if target == "/api/launcher-update/restart" {
                return Some(crate::launcher_update::restart(&app, port));
            }
            api::post(&app, &target, &payload, locale)
        }
    })
    .await;
    match result {
        Ok(Some(Ok(mut value))) => {
            if path == "/api/status" {
                value["csrf_token"] = json!(service.token);
                value["service"] = json!({"release":resources::release_identity(),"desktop":service.desktop,"update_pending":service.pending.load(Ordering::Relaxed),"message":if service.pending.load(Ordering::Relaxed){"Ein Launcher-Update ist bereit. Bitte Spiel oder Einrichtung abschließen und Flightdeck erneut öffnen. Die aktuelle Sitzung läuft weiter."}else{""}});
            }
            json_reply(
                StatusCode::OK,
                value,
                locale,
                !path.starts_with("/api/problem-reports"),
            )
        }
        Ok(Some(Err(e))) => error(
            if matches!(e, Error::Io(_)) {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::CONFLICT
            },
            &e.to_string(),
            locale,
        ),
        Ok(None) => error(StatusCode::NOT_FOUND, "Nicht gefunden.", locale),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Lokale Daten konnten nicht gelesen werden. Bitte die Runtime prüfen.",
            locale,
        ),
    }
}
pub async fn serve(
    launcher: Arc<Launcher>,
    port: u16,
    no_browser: bool,
    language: Option<&str>,
    desktop: bool,
) -> Result<()> {
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    let port = listener.local_addr()?.port();
    let service = Service {
        launcher: Arc::clone(&launcher),
        port,
        token: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        desktop,
        pending: Arc::new(AtomicBool::new(false)),
        shutdown: Arc::new(tokio::sync::Notify::new()),
    };
    let url = format!(
        "http://127.0.0.1:{port}{}",
        language.map(|s| format!("/?lang={s}")).unwrap_or_default()
    );
    if desktop {
        crate::desktop::write_record(&launcher.state_dir, &service)?;
    }
    println!("Flightdeck: {url}");
    if !no_browser {
        crate::process::open_uri(&url)?;
    }
    let notify = Arc::clone(&service.shutdown);
    let cleanup_launcher = Arc::clone(&launcher);
    let router = Router::new().fallback(any(handle)).with_state(service);
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::select! {_ = tokio::signal::ctrl_c()=>{},_=notify.notified()=>{}}
            cleanup_launcher.close();
            // Keep the status/stop endpoint and coordinator alive until owned
            // jobs have durably completed. In particular, a running game still
            // needs its post-exit backup and cloud transfer.
            while {
                let state = cleanup_launcher.lock();
                state.active.is_some()
                    || state.launcher_updates.worker.is_some()
                    || state.startup_updates.worker.is_some()
            } {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await?;
    launcher.close();
    Ok(())
}
