//! Mock subscription server: heartbeat API + web dashboard + client download.
//! Run: `cargo run -p heartbeat-server`

mod auth;
mod db;

use axum::{
    body::Body,
    extract::{ConnectInfo, Json, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};

const MAX_EVENTS: usize = 200;

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<ServerInner>>,
    db: Arc<Mutex<rusqlite::Connection>>,
}

struct ServerInner {
    started: std::time::Instant,
    total_heartbeats: u64,
    events: VecDeque<HeartbeatEvent>,
    clients: HashMap<String, ClientRow>,
}

#[derive(Clone, Serialize)]
struct ClientRow {
    client_id: String,
    client_name: String,
    device_id: Option<String>,
    last_seen: String,
    beats_count: u64,
    acknowledged: bool,
    last_app_version: Option<String>,
}

#[derive(Clone, Serialize)]
struct HeartbeatEvent {
    received_at: String,
    client_id: Option<String>,
    client_name: Option<String>,
    device_id: Option<String>,
    app_version: Option<String>,
    acknowledged: bool,
    ack_detail: String,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    device_id: Option<String>,
    app_version: Option<String>,
    #[serde(default, alias = "clientId")]
    client_id: Option<String>,
    #[serde(default, alias = "sessionId")]
    session_id: Option<String>,
}

#[derive(Serialize)]
struct HeartbeatResponse {
    status: &'static str,
    subscription_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize)]
struct StatsResponse {
    uptime_secs: u64,
    total_heartbeats: u64,
    clients: Vec<ClientRow>,
    recent: Vec<HeartbeatEvent>,
    sessions: Vec<db::SessionListRow>,
}

#[derive(Debug, Deserialize)]
struct ControlRequest {
    action: ControlAction,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum ControlAction {
    Start,
    Stop,
    Restart,
}

#[derive(Serialize)]
struct ControlResponse {
    ok: bool,
    message: String,
}

fn workspace_root() -> PathBuf {
    if let Ok(p) = std::env::var("WORKSPACE_ROOT") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR should have a parent (workspace root)")
        .to_path_buf()
}

async fn api_control(Json(body): Json<ControlRequest>) -> impl IntoResponse {
    let root = workspace_root();
    if !root.join("scripts").join("stop.sh").exists() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ControlResponse {
                ok: false,
                message: "scripts/ not found next to this project. Set WORKSPACE_ROOT to the repo root.".into(),
            }),
        )
            .into_response();
    }

    let script = match body.action {
        ControlAction::Start => "start.sh",
        ControlAction::Stop => "stop.sh",
        ControlAction::Restart => "restart.sh",
    };
    let delay_ms: u64 = match body.action {
        ControlAction::Start => 0,
        ControlAction::Stop | ControlAction::Restart => 400,
    };

    let script = script.to_string();
    tokio::spawn(async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        let cmd = format!(
            "cd '{}' && exec ./scripts/{} server",
            root.display(),
            script
        );
        let _ = tokio::process::Command::new("/bin/bash")
            .arg("-lc")
            .arg(&cmd)
            .spawn();
    });

    let message = match body.action {
        ControlAction::Start => "Start requested. If the server was already running, the script will report that.",
        ControlAction::Stop => "Stop scheduled. This page will disconnect when the server stops.",
        ControlAction::Restart => "Restart scheduled. This page may disconnect briefly, then reload.",
    };

    (
        StatusCode::OK,
        Json(ControlResponse {
            ok: true,
            message: message.to_string(),
        }),
    )
        .into_response()
}

impl AppState {
    fn new(db: rusqlite::Connection) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ServerInner {
                started: std::time::Instant::now(),
                total_heartbeats: 0,
                events: VecDeque::with_capacity(MAX_EVENTS),
                clients: HashMap::new(),
            })),
            db: Arc::new(Mutex::new(db)),
        }
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../templates/server_dashboard.html"))
}

async fn register_page() -> Html<&'static str> {
    Html(include_str!("../templates/register.html"))
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    email: String,
    name: String,
    password: String,
    country: String,
}

#[derive(Serialize)]
struct RegisterResponse {
    ok: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
}

async fn api_register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();
    let name = body.name.trim();
    let country = body.country.trim();

    if email.is_empty() || !email.contains('@') || email.len() > 254 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                ok: false,
                message: "Enter a valid email address.".into(),
                client_id: None,
            }),
        )
            .into_response();
    }
    if name.is_empty() || name.len() > 200 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                ok: false,
                message: "Name is required (max 200 characters).".into(),
                client_id: None,
            }),
        )
            .into_response();
    }
    if body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                ok: false,
                message: "Password must be at least 8 characters.".into(),
                client_id: None,
            }),
        )
            .into_response();
    }
    if country.is_empty() || country.len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(RegisterResponse {
                ok: false,
                message: "Country is required (max 100 characters).".into(),
                client_id: None,
            }),
        )
            .into_response();
    }

    let password_hash = match auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RegisterResponse {
                    ok: false,
                    message: format!("Could not hash password: {e}"),
                    client_id: None,
                }),
            )
                .into_response();
        }
    };

    let created_at = Utc::now().to_rfc3339();
    let client_id = uuid::Uuid::new_v4().to_string();
    let client_id_response = client_id.clone();
    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RegisterResponse {
                    ok: false,
                    message: "Database lock error.".into(),
                    client_id: None,
                }),
            )
                .into_response();
        }
    };

    match conn.execute(
        "INSERT INTO users (email, name, password_hash, country, created_at, client_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![email, name, password_hash, country, created_at, client_id],
    ) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(RegisterResponse {
                ok: true,
                message: "Registration successful. Save your Client ID for the heartbeat app.".into(),
                client_id: Some(client_id_response),
            }),
        )
            .into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                (
                    StatusCode::CONFLICT,
                    Json(RegisterResponse {
                        ok: false,
                        message: "An account with this email already exists.".into(),
                        client_id: None,
                    }),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(RegisterResponse {
                        ok: false,
                        message: "Could not save registration.".into(),
                        client_id: None,
                    }),
                )
                    .into_response()
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    ok: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
}

async fn api_login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginResponse {
                ok: false,
                message: "Enter a valid email address.".into(),
                client_id: None,
                name: None,
                session_id: None,
            }),
        )
            .into_response();
    }
    if body.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginResponse {
                ok: false,
                message: "Password is required.".into(),
                client_id: None,
                name: None,
                session_id: None,
            }),
        )
            .into_response();
    }

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoginResponse {
                    ok: false,
                    message: "Database lock error.".into(),
                    client_id: None,
                    name: None,
                    session_id: None,
                }),
            )
                .into_response();
        }
    };

    let row = match db::lookup_user_by_email(&conn, &email) {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoginResponse {
                    ok: false,
                    message: "Could not verify credentials.".into(),
                    client_id: None,
                    name: None,
                    session_id: None,
                }),
            )
                .into_response();
        }
    };

    let Some((client_id, name, password_hash)) = row else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                ok: false,
                message: "Invalid email or password.".into(),
                client_id: None,
                name: None,
                session_id: None,
            }),
        )
            .into_response();
    };

    if !auth::verify_password(&body.password, &password_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                ok: false,
                message: "Invalid email or password.".into(),
                client_id: None,
                name: None,
                session_id: None,
            }),
        )
            .into_response();
    }

    let cid = client_id.trim().to_string();
    if cid.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                ok: false,
                message: "Account has no client ID. Re-register or contact support.".into(),
                client_id: None,
                name: None,
                session_id: None,
            }),
        )
            .into_response();
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let ip = addr.ip().to_string();

    if let Err(e) = db::insert_session(
        &conn,
        &session_id,
        &email,
        &cid,
        "heartbeat-client",
        &ip,
        &ua,
        &now,
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                ok: false,
                message: format!("Could not create session: {e}"),
                client_id: None,
                name: None,
                session_id: None,
            }),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(LoginResponse {
            ok: true,
            message: "Signed in.".into(),
            client_id: Some(cid),
            name: Some(name),
            session_id: Some(session_id),
        }),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct LogoutRequest {
    #[serde(default, alias = "sessionId")]
    session_id: String,
}

#[derive(Serialize)]
struct LogoutResponse {
    ok: bool,
    message: String,
}

async fn api_logout(State(state): State<AppState>, Json(body): Json<LogoutRequest>) -> impl IntoResponse {
    let sid = body.session_id.trim();
    if sid.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(LogoutResponse {
                ok: false,
                message: "session_id is required.".into(),
            }),
        )
            .into_response();
    }
    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogoutResponse {
                    ok: false,
                    message: "Database lock error.".into(),
                }),
            )
                .into_response();
        }
    };
    match db::delete_session(&conn, sid) {
        Ok(true) => (
            StatusCode::OK,
            Json(LogoutResponse {
                ok: true,
                message: "Signed out.".into(),
            }),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(LogoutResponse {
                ok: false,
                message: "Session not found or already ended.".into(),
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LogoutResponse {
                ok: false,
                message: "Could not end session.".into(),
            }),
        )
            .into_response(),
    }
}

async fn api_stats(State(state): State<AppState>) -> Json<StatsResponse> {
    let g = state.inner.lock().unwrap();
    let uptime_secs = g.started.elapsed().as_secs();
    let total_heartbeats = g.total_heartbeats;
    let recent: Vec<HeartbeatEvent> = g.events.iter().rev().take(50).cloned().collect();
    let mut clients: Vec<ClientRow> = g.clients.values().cloned().collect();
    clients.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    drop(g);

    let sessions = match state.db.lock() {
        Ok(c) => db::list_sessions_recent(&c, 50).unwrap_or_default(),
        Err(_) => vec![],
    };

    Json(StatsResponse {
        uptime_secs,
        total_heartbeats,
        clients,
        recent,
        sessions,
    })
}

async fn heartbeat(
    State(state): State<AppState>,
    Json(body): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let client_id_opt = body
        .client_id
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let Some(client_id_owned) = client_id_opt else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(HeartbeatResponse {
                status: "error",
                subscription_active: false,
                message: Some("client_id is required".into()),
            }),
        )
            .into_response();
    };

    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(HeartbeatResponse {
                    status: "error",
                    subscription_active: false,
                    message: Some("Database error.".into()),
                }),
            )
                .into_response();
        }
    };

    let client_name = match db::lookup_user_name_by_client_id(&conn, &client_id_owned) {
        Ok(Some(name)) => name,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(HeartbeatResponse {
                    status: "error",
                    subscription_active: false,
                    message: Some("Unknown or invalid client_id.".into()),
                }),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(HeartbeatResponse {
                    status: "error",
                    subscription_active: false,
                    message: Some("Database error.".into()),
                }),
            )
                .into_response();
        }
    };
    drop(conn);

    println!(
        "[heartbeat] client_id={} name={} device_id={:?} app_version={:?}",
        client_id_owned, client_name, body.device_id, body.app_version
    );

    let key = client_id_owned.clone();
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    let res = HeartbeatResponse {
        status: "ok",
        subscription_active: true,
        message: None,
    };

    let ack_detail = format!(
        "status={} subscription_active={}",
        res.status, res.subscription_active
    );

    {
        let mut g = state.inner.lock().unwrap();
        g.total_heartbeats += 1;

        g.clients
            .entry(key.clone())
            .and_modify(|c| {
                c.client_name = client_name.clone();
                c.last_seen = now.clone();
                c.beats_count += 1;
                c.acknowledged = true;
                c.last_app_version = body.app_version.clone();
                c.device_id = body.device_id.clone();
            })
            .or_insert(ClientRow {
                client_id: client_id_owned.clone(),
                client_name: client_name.clone(),
                device_id: body.device_id.clone(),
                last_seen: now.clone(),
                beats_count: 1,
                acknowledged: true,
                last_app_version: body.app_version.clone(),
            });

        let ev = HeartbeatEvent {
            received_at: now,
            client_id: Some(client_id_owned),
            client_name: Some(client_name),
            device_id: body.device_id.clone(),
            app_version: body.app_version.clone(),
            acknowledged: true,
            ack_detail: ack_detail.clone(),
        };
        if g.events.len() >= MAX_EVENTS {
            g.events.pop_front();
        }
        g.events.push_back(ev);
    }

    if let Some(ref sid) = body.session_id {
        let s = sid.trim();
        if !s.is_empty() {
            if let Ok(conn) = state.db.lock() {
                let now = Utc::now().to_rfc3339();
                let _ = db::touch_session(&conn, s, &now);
            }
        }
    }

    (StatusCode::OK, Json(res)).into_response()
}

fn resolve_client_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CLIENT_BINARY_PATH") {
        return PathBuf::from(p);
    }
    for rel in [
        "target/release/heartbeat-client",
        "target/debug/heartbeat-client",
    ] {
        let p = PathBuf::from(rel);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("target/release/heartbeat-client")
}

async fn download_client() -> impl IntoResponse {
    let path = resolve_client_binary();
    if !path.exists() {
        let msg = format!(
            "Client binary not found at {}. Build with: cargo build --release -p heartbeat-client (from workspace root), or set CLIENT_BINARY_PATH.",
            path.display()
        );
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            msg,
        )
            .into_response();
    }

    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(
                header::CONTENT_DISPOSITION,
                r#"attachment; filename="heartbeat-client""#,
            )
            .body(Body::from(bytes))
            .unwrap()
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Read failed: {e}"),
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9847);

    let root = workspace_root();
    let db_conn = db::init_db(&root).expect("open SQLite database (data/heartbeat.db)");
    let state = AppState::new(db_conn);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/register", get(register_page))
        .route("/health", get(health))
        .route("/api/stats", get(api_stats))
        .route("/api/control", post(api_control))
        .route("/api/register", post(api_register))
        .route("/api/login", post(api_login))
        .route("/api/logout", post(api_logout))
        .route("/heartbeat", post(heartbeat))
        .route("/download/client", get(download_client))
        .with_state(state)
        .layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await.expect("bind failed");
    println!("heartbeat-server listening on http://{addr}");
    println!("  Dashboard : http://{addr}/");
    println!("  Register  : http://{addr}/register");
    println!("  Database  : {}", root.join("data").join("heartbeat.db").display());
    println!("  Download  : http://{addr}/download/client");
    println!("  GET  /health | POST /heartbeat | POST /api/register | POST /api/login | POST /api/logout");

    let client_path = resolve_client_binary();
    if client_path.exists() {
        println!("  Client file for download: {}", client_path.display());
    } else {
        println!(
            "  (Optional) Build client for download: {}",
            client_path.display()
        );
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server exited");
}
