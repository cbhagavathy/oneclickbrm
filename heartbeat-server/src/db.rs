use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

pub fn init_db(workspace_root: &Path) -> rusqlite::Result<Connection> {
    let dir = workspace_root.join("data");
    std::fs::create_dir_all(&dir).expect("create data directory");
    let path = dir.join("heartbeat.db");
    let conn = Connection::open(&path)?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT UNIQUE NOT NULL COLLATE NOCASE,
            name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            country TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
        "#,
    )?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(users)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    if !cols.iter().any(|c| c == "client_id") {
        conn.execute("ALTER TABLE users ADD COLUMN client_id TEXT", [])?;
        let mut stmt = conn.prepare("SELECT id FROM users WHERE client_id IS NULL OR TRIM(client_id) = ''")?;
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        for id in ids {
            let cid = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "UPDATE users SET client_id = ?1 WHERE id = ?2",
                params![cid, id],
            )?;
        }
    }
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_client_id ON users(client_id)",
        [],
    )?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY NOT NULL,
            email TEXT NOT NULL,
            client_id TEXT NOT NULL,
            client_kind TEXT NOT NULL DEFAULT 'heartbeat-client',
            created_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            remote_ip TEXT,
            user_agent TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_last_seen ON sessions(last_seen_at DESC);
        "#,
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionListRow {
    pub id: String,
    pub email: String,
    pub client_id: String,
    pub client_kind: String,
    pub created_at: String,
    pub last_seen_at: String,
    pub remote_ip: Option<String>,
    pub user_agent: Option<String>,
}

pub fn insert_session(
    conn: &Connection,
    id: &str,
    email: &str,
    client_id: &str,
    client_kind: &str,
    remote_ip: &str,
    user_agent: &str,
    now_rfc3339: &str,
) -> rusqlite::Result<()> {
    let ua = if user_agent.len() > 512 {
        &user_agent[..512]
    } else {
        user_agent
    };
    conn.execute(
        "INSERT INTO sessions (id, email, client_id, client_kind, created_at, last_seen_at, remote_ip, user_agent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            email,
            client_id,
            client_kind,
            now_rfc3339,
            now_rfc3339,
            remote_ip,
            ua
        ],
    )?;
    Ok(())
}

pub fn delete_session(conn: &Connection, session_id: &str) -> rusqlite::Result<bool> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return Ok(false);
    }
    let n = conn.execute("DELETE FROM sessions WHERE id = ?1", params![sid])?;
    Ok(n > 0)
}

pub fn touch_session(conn: &Connection, session_id: &str, now_rfc3339: &str) -> rusqlite::Result<bool> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return Ok(false);
    }
    let n = conn.execute(
        "UPDATE sessions SET last_seen_at = ?1 WHERE id = ?2",
        params![now_rfc3339, sid],
    )?;
    Ok(n > 0)
}

pub fn list_sessions_recent(conn: &Connection, limit: usize) -> rusqlite::Result<Vec<SessionListRow>> {
    let lim = limit.min(200);
    let mut stmt = conn.prepare(
        "SELECT id, email, client_id, client_kind, created_at, last_seen_at, remote_ip, user_agent FROM sessions ORDER BY last_seen_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![lim as i64], |r| {
        Ok(SessionListRow {
            id: r.get(0)?,
            email: r.get(1)?,
            client_id: r.get(2)?,
            client_kind: r.get(3)?,
            created_at: r.get(4)?,
            last_seen_at: r.get(5)?,
            remote_ip: r.get(6)?,
            user_agent: r.get(7)?,
        })
    })?;
    rows.collect()
}

/// Returns `(client_id, name, password_hash)` for login when `email` matches.
pub fn lookup_user_by_email(
    conn: &Connection,
    email: &str,
) -> rusqlite::Result<Option<(String, String, String)>> {
    let e = email.trim();
    if e.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT client_id, name, password_hash FROM users WHERE email = ?1 COLLATE NOCASE LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![e], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    Ok(rows.next().transpose()?)
}

/// Returns registered display name if `client_id` matches a row.
pub fn lookup_user_name_by_client_id(
    conn: &Connection,
    client_id: &str,
) -> rusqlite::Result<Option<String>> {
    let cid = client_id.trim();
    if cid.is_empty() {
        return Ok(None);
    }
    let mut stmt =
        conn.prepare("SELECT name FROM users WHERE client_id = ?1 COLLATE NOCASE LIMIT 1")?;
    let mut rows = stmt.query_map(params![cid], |r| r.get::<_, String>(0))?;
    Ok(rows.next().transpose()?)
}
