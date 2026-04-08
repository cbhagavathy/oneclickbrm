//! Heartbeat client: local web UI is gated until the server acknowledges heartbeat.
//! CLI: `heartbeat-client [--port|-p PORT] [--heartbeat-url|-u URL] [--interval|-i SECS]`

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

const LOG_MAX: usize = 40;

#[derive(Clone)]
struct AppState {
    gate: Arc<Mutex<Gate>>,
    inner: Arc<Mutex<ClientInner>>,
}

#[derive(Clone)]
enum Gate {
    /// Show login form until the user signs in (unless CLIENT_ID is set via CLI/env).
    NeedLogin,
    Checking,
    Unlocked,
    Locked(String),
}

struct ClientInner {
    heartbeat_url: String,
    interval_secs: u64,
    client_id: String,
    user_name: String,
    /// Server session row id (from POST /api/login); sent on heartbeats and logout.
    session_id: String,
    user_email: String,
    session_started_at: String,
    device_id: String,
    app_version: String,
    dashboard_port: u16,
    total_sent: u64,
    last_ok: bool,
    last_response: Option<Value>,
    last_error: Option<String>,
    log: VecDeque<LogEntry>,
}

#[derive(Clone, Serialize)]
struct LogEntry {
    at: String,
    ok: bool,
    detail: String,
}

#[derive(Serialize, Clone)]
struct HeartbeatBody {
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    device_id: String,
    app_version: String,
}

#[derive(Serialize)]
struct StatusResponse {
    gate: String,
    dashboard_access: bool,
    needs_login: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    heartbeat_url: String,
    interval_secs: u64,
    client_id: String,
    user_name: String,
    session_id: String,
    user_email: String,
    session_started_at: String,
    device_id: String,
    app_version: String,
    dashboard_port: u16,
    total_sent: u64,
    last_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    recent_log: Vec<LogEntry>,
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct ServerLoginResponse {
    ok: bool,
    message: String,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogoutBody {
    #[serde(default)]
    session_id: String,
}

#[derive(Serialize)]
struct ClientLoginApiResponse {
    ok: bool,
    message: String,
}

fn server_login_url(heartbeat_url: &str) -> String {
    let u = heartbeat_url.trim_end_matches('/');
    if let Some((base, _)) = u.rsplit_once('/') {
        format!("{base}/api/login")
    } else {
        format!("{u}/api/login")
    }
}

fn server_logout_url(heartbeat_url: &str) -> String {
    let u = heartbeat_url.trim_end_matches('/');
    if let Some((base, _)) = u.rsplit_once('/') {
        format!("{base}/api/logout")
    } else {
        format!("{u}/api/logout")
    }
}

#[derive(Parser)]
#[command(name = "heartbeat-client")]
#[command(about = "Local dashboard; unlocked only after the server acknowledges heartbeat.")]
struct Cli {
    /// Port for the local web dashboard
    #[arg(short = 'p', long = "port", default_value_t = 9860)]
    port: u16,
    /// Server heartbeat endpoint (POST JSON). Overrides HEARTBEAT_URL.
    #[arg(short = 'u', long = "heartbeat-url", env = "HEARTBEAT_URL")]
    heartbeat_url: Option<String>,
    /// Seconds between heartbeat attempts
    #[arg(short = 'i', long = "interval", default_value_t = 5, env = "HEARTBEAT_INTERVAL_SECS")]
    interval_secs: u64,
    /// Client ID from server registration (`/register`). Same as env CLIENT_ID.
    #[arg(long = "client-id", env = "CLIENT_ID")]
    client_id: Option<String>,
}

fn default_heartbeat_url() -> String {
    "http://127.0.0.1:9847/heartbeat".to_string()
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let g = state.gate.lock().await;
    match &*g {
        Gate::NeedLogin => Html(include_str!("../templates/client_login.html").to_string()),
        Gate::Checking => Html(include_str!("../templates/client_checking.html").to_string()),
        Gate::Locked(reason) => {
            let detail = escape_html(reason);
            let html = include_str!("../templates/client_blocked.html").replace("__DETAIL__", &detail);
            Html(html)
        }
        Gate::Unlocked => Html(include_str!("../templates/client_dashboard.html").to_string()),
    }
}

async fn api_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let (gate_str, dashboard_access, needs_login, error) = {
        let g = state.gate.lock().await;
        match &*g {
            Gate::NeedLogin => ("login".to_string(), false, true, None),
            Gate::Checking => ("checking".to_string(), false, false, None),
            Gate::Unlocked => ("unlocked".to_string(), true, false, None),
            Gate::Locked(reason) => ("locked".to_string(), false, false, Some(reason.clone())),
        }
    };

    let g = state.inner.lock().await;
    let recent_log: Vec<LogEntry> = g.log.iter().rev().take(25).cloned().collect();

    Json(StatusResponse {
        gate: gate_str,
        dashboard_access,
        needs_login,
        error,
        heartbeat_url: g.heartbeat_url.clone(),
        interval_secs: g.interval_secs,
        client_id: g.client_id.clone(),
        user_name: g.user_name.clone(),
        session_id: g.session_id.clone(),
        user_email: g.user_email.clone(),
        session_started_at: g.session_started_at.clone(),
        device_id: g.device_id.clone(),
        app_version: g.app_version.clone(),
        dashboard_port: g.dashboard_port,
        total_sent: g.total_sent,
        last_ok: g.last_ok,
        last_response: if dashboard_access {
            g.last_response.clone()
        } else {
            None
        },
        last_error: g.last_error.clone(),
        recent_log,
    })
}

async fn api_login(State(state): State<AppState>, Json(body): Json<LoginForm>) -> impl IntoResponse {
    let url = {
        let inner = state.inner.lock().await;
        server_login_url(&inner.heartbeat_url)
    };

    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ClientLoginApiResponse {
                    ok: false,
                    message: format!("HTTP client error: {e}"),
                }),
            )
                .into_response();
        }
    };

    let resp = match http
        .post(&url)
        .json(&serde_json::json!({
            "email": body.email.trim(),
            "password": body.password,
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(ClientLoginApiResponse {
                    ok: false,
                    message: format!("Could not reach server at {url}: {e:#}"),
                }),
            )
                .into_response();
        }
    };

    let status = resp.status();
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(ClientLoginApiResponse {
                    ok: false,
                    message: format!("Could not read server response: {e:#}"),
                }),
            )
                .into_response();
        }
    };

    let parsed: ServerLoginResponse = match serde_json::from_str(&text) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(ClientLoginApiResponse {
                    ok: false,
                    message: format!("Unexpected server response (HTTP {status}): {text}"),
                }),
            )
                .into_response();
        }
    };

    if !status.is_success() || !parsed.ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ClientLoginApiResponse {
                ok: false,
                message: parsed.message,
            }),
        )
            .into_response();
    }

    let Some(cid) = parsed.client_id.filter(|s| !s.trim().is_empty()) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ClientLoginApiResponse {
                ok: false,
                message: "Server did not return a client ID.".into(),
            }),
        )
            .into_response();
    };

    let sid = parsed.session_id.unwrap_or_default();
    let started = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    {
        let mut g = state.gate.lock().await;
        let mut inner = state.inner.lock().await;
        inner.client_id = cid;
        inner.user_name = parsed.name.unwrap_or_default();
        inner.session_id = sid;
        inner.user_email = body.email.trim().to_lowercase();
        inner.session_started_at = started;
        *g = Gate::Checking;
    }

    (
        StatusCode::OK,
        Json(ClientLoginApiResponse {
            ok: true,
            message: "Signed in. Connecting…".into(),
        }),
    )
        .into_response()
}

async fn api_logout(State(state): State<AppState>, Json(body): Json<LogoutBody>) -> impl IntoResponse {
    let (url, sid) = {
        let inner = state.inner.lock().await;
        let sid = if !body.session_id.trim().is_empty() {
            body.session_id.trim().to_string()
        } else {
            inner.session_id.clone()
        };
        (server_logout_url(&inner.heartbeat_url), sid)
    };

    if sid.is_empty() {
        let mut g = state.gate.lock().await;
        let mut inner = state.inner.lock().await;
        inner.client_id.clear();
        inner.user_name.clear();
        inner.session_id.clear();
        inner.user_email.clear();
        inner.session_started_at.clear();
        inner.last_response = None;
        inner.last_error = None;
        inner.log.clear();
        *g = Gate::NeedLogin;
        return (
            StatusCode::OK,
            Json(ClientLoginApiResponse {
                ok: true,
                message: "Signed out.".into(),
            }),
        )
            .into_response();
    }

    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ClientLoginApiResponse {
                    ok: false,
                    message: format!("HTTP client error: {e}"),
                }),
            )
                .into_response();
        }
    };

    let _ = http
        .post(&url)
        .json(&serde_json::json!({ "session_id": sid }))
        .send()
        .await;

    {
        let mut g = state.gate.lock().await;
        let mut inner = state.inner.lock().await;
        inner.client_id.clear();
        inner.user_name.clear();
        inner.session_id.clear();
        inner.user_email.clear();
        inner.session_started_at.clear();
        inner.last_response = None;
        inner.last_error = None;
        inner.log.clear();
        *g = Gate::NeedLogin;
    }

    (
        StatusCode::OK,
        Json(ClientLoginApiResponse {
            ok: true,
            message: "Signed out.".into(),
        }),
    )
        .into_response()
}

async fn send_heartbeat(
    client: &reqwest::Client,
    url: &str,
    body: &HeartbeatBody,
) -> Result<Value> {
    let resp = client
        .post(url)
        .json(body)
        .send()
        .await
        .context("request failed")?;

    let status = resp.status();
    let text = resp.text().await.context("read body")?;

    if !status.is_success() {
        anyhow::bail!("HTTP {status}: {text}");
    }

    serde_json::from_str(&text).context("parse JSON")
}

fn push_log(inner: &mut ClientInner, ok: bool, detail: String) {
    if inner.log.len() >= LOG_MAX {
        inner.log.pop_front();
    }
    inner.log.push_back(LogEntry {
        at: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        ok,
        detail,
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let heartbeat_url = cli
        .heartbeat_url
        .clone()
        .unwrap_or_else(default_heartbeat_url);
    let interval_secs = cli.interval_secs;
    let dashboard_port = cli.port;

    let client_id = cli
        .client_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let initial_gate = if client_id.is_some() {
        Gate::Checking
    } else {
        Gate::NeedLogin
    };

    let device_id = uuid::Uuid::new_v4().to_string();
    let app_version = env!("CARGO_PKG_VERSION").to_string();

    let state = AppState {
        gate: Arc::new(Mutex::new(initial_gate)),
        inner: Arc::new(Mutex::new(ClientInner {
            heartbeat_url: heartbeat_url.clone(),
            interval_secs,
            client_id: client_id.clone().unwrap_or_default(),
            user_name: String::new(),
            session_id: String::new(),
            user_email: String::new(),
            session_started_at: String::new(),
            device_id: device_id.clone(),
            app_version: app_version.clone(),
            dashboard_port,
            total_sent: 0,
            last_ok: false,
            last_response: None,
            last_error: None,
            log: VecDeque::with_capacity(LOG_MAX),
        })),
    };

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("build HTTP client")?;

    let loop_state = state.clone();
    let url_loop = heartbeat_url.clone();
    let device_loop = device_id.clone();
    let app_version_loop = app_version.clone();
    tokio::spawn(async move {
        loop {
            let (cid, sid) = {
                let inner = loop_state.inner.lock().await;
                (
                    inner.client_id.clone(),
                    inner.session_id.clone(),
                )
            };
            if cid.is_empty() {
                sleep(Duration::from_secs(1)).await;
                continue;
            }

            let session_id = if sid.trim().is_empty() {
                None
            } else {
                Some(sid)
            };

            let body = HeartbeatBody {
                client_id: cid,
                session_id,
                device_id: device_loop.clone(),
                app_version: app_version_loop.clone(),
            };

            let result = send_heartbeat(&http, &url_loop, &body).await;
            {
                let mut gate = loop_state.gate.lock().await;
                let mut inner = loop_state.inner.lock().await;
                inner.total_sent += 1;
                match result {
                    Ok(v) => {
                        *gate = Gate::Unlocked;
                        inner.last_ok = true;
                        inner.last_response = Some(v.clone());
                        inner.last_error = None;
                        let summary = v.to_string();
                        let short = if summary.len() > 200 {
                            format!("{}…", &summary[..200])
                        } else {
                            summary
                        };
                        push_log(&mut inner, true, short);
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        *gate = Gate::Locked(msg.clone());
                        inner.last_ok = false;
                        inner.last_error = Some(msg.clone());
                        push_log(&mut inner, false, format!("{e:#}"));
                    }
                }
            }
            sleep(Duration::from_secs(interval_secs)).await;
        }
    });

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/api/login", post(api_login))
        .route("/api/logout", post(api_logout))
        .with_state(state)
        .layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], dashboard_port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind local dashboard on {addr} (use --port)"))?;

    println!("heartbeat-client");
    println!("  Heartbeat URL : {heartbeat_url}");
    println!("  Interval      : {interval_secs}s");
    match &client_id {
        Some(id) => println!("  Client ID     : {id}"),
        None => println!("  Client ID     : (sign in via local dashboard, or set CLIENT_ID)"),
    }
    println!("  Device ID     : {device_id}");
    println!("  Local dashboard: http://127.0.0.1:{dashboard_port}/");
    println!("  (Dashboard is locked until heartbeat succeeds.)");
    println!("  Set OPEN_BROWSER=0 to skip opening the browser.");

    if std::env::var("OPEN_BROWSER").ok().as_deref() != Some("0") {
        let url = format!("http://127.0.0.1:{dashboard_port}/");
        let _ = open::that(&url);
    }

    axum::serve(listener, app).await.context("dashboard server exited")
}
