use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct PersistenceReport {
    pub state_db_path: PathBuf,
    pub sessions_dir: PathBuf,
    pub snapshot_pattern: String,
    pub state_db_existed_before: bool,
    pub bootstrap_runs: u64,
}

#[derive(Debug, Clone)]
pub struct SessionRequest {
    pub command_name: String,
    pub query_present: bool,
    pub query_text: Option<String>,
    pub image_present: bool,
    pub image_path: Option<String>,
    pub resume: Option<String>,
    pub continue_last: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionRuntimeReport {
    pub session_id: String,
    pub action: SessionAction,
    pub interaction_mode: InteractionMode,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub message_count: u64,
    pub event_count: u64,
}

#[derive(Debug, Clone)]
pub struct SessionSearchHit {
    pub session_id: String,
    pub session_title: String,
    pub message_id: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct SessionMessageRecord {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct SessionEventRecord {
    pub id: String,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct SessionInspection {
    pub session_id: String,
    pub title: String,
    pub messages: Vec<SessionMessageRecord>,
    pub events: Vec<SessionEventRecord>,
}

#[derive(Debug, Clone, Copy)]
pub enum SessionAction {
    Created,
    ResumedById,
    ResumedByTitle,
    ResumedLatest,
}

impl SessionAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::ResumedById => "resumed-by-id",
            Self::ResumedByTitle => "resumed-by-title",
            Self::ResumedLatest => "resumed-latest",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InteractionMode {
    Interactive,
    SingleTurn,
}

impl InteractionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::SingleTurn => "single-turn",
        }
    }
}

#[derive(Debug, Clone)]
struct StoredSession {
    id: String,
    title: String,
}

pub fn initialize_persistence(vela_home: &Path) -> Result<PersistenceReport> {
    let sessions_dir = vela_home.join("sessions");
    fs::create_dir_all(&sessions_dir)
        .with_context(|| format!("failed to create {}", sessions_dir.display()))?;

    let state_db_path = vela_home.join("state.db");
    let existed_before = state_db_path.is_file();
    let conn = Connection::open(&state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    initialize_schema(&conn)?;

    let next_runs = current_meta_u64(&conn, "bootstrap_runs")?.unwrap_or(0) + 1;
    set_meta(&conn, "bootstrap_runs", &next_runs.to_string())?;
    set_meta(&conn, "snapshot_pattern", "sessions/session_<id>.json")?;

    Ok(PersistenceReport {
        state_db_path,
        sessions_dir,
        snapshot_pattern: "sessions/session_<id>.json".to_string(),
        state_db_existed_before: existed_before,
        bootstrap_runs: next_runs,
    })
}

pub fn current_session_identity(state_db_path: &Path) -> Result<Option<(String, String)>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    Ok(latest_session(&conn)?.map(|session| (session.id, session.title)))
}

pub fn current_session_summary(state_db_path: &Path) -> Result<Option<SessionSummary>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session(&conn)? else {
        return Ok(None);
    };
    Ok(Some(load_summary(&conn, &session.id, &session.title)?))
}

pub fn current_command_session_summary(state_db_path: &Path, command_name: &str) -> Result<Option<SessionSummary>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session_for_command(&conn, command_name)? else {
        return Ok(None);
    };
    Ok(Some(load_summary(&conn, &session.id, &session.title)?))
}

pub fn search_session_history(state_db_path: &Path, query: &str, limit: usize) -> Result<Vec<SessionSearchHit>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let query = fts_query(query);
    let mut stmt = conn.prepare(
        "SELECT session_id, title, message_id, snippet(message_fts, 3, '[', ']', '…', 12)
         FROM message_fts
         WHERE message_fts MATCH ?1
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, limit as i64], |row| {
        Ok(SessionSearchHit {
            session_id: row.get(0)?,
            session_title: row.get(1)?,
            message_id: row.get(2)?,
            snippet: row.get(3)?,
        })
    })?;
    let mut hits = Vec::new();
    for row in rows {
        hits.push(row?);
    }
    Ok(hits)
}

pub fn inspect_latest_session(state_db_path: &Path, limit: usize) -> Result<Option<SessionInspection>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session(&conn)? else {
        return Ok(None);
    };
    Ok(Some(load_session_inspection(&conn, &session.id, &session.title, limit)?))
}

pub fn append_event_to_session(
    state_db_path: &Path,
    session_id: &str,
    event_type: &str,
    payload_json: String,
) -> Result<bool> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    if session_title(&conn, session_id)?.is_none() {
        return Ok(false);
    }
    append_event(&conn, session_id, event_type, payload_json)?;
    Ok(true)
}

pub fn append_message_to_session(
    state_db_path: &Path,
    session_id: &str,
    role: &str,
    content: &str,
    metadata_json: Option<String>,
) -> Result<bool> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    if session_title(&conn, session_id)?.is_none() {
        return Ok(false);
    }
    append_message(&conn, session_id, role, content, metadata_json)?;
    Ok(true)
}

pub fn append_event_to_latest_session(state_db_path: &Path, event_type: &str, payload_json: String) -> Result<bool> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session(&conn)? else {
        return Ok(false);
    };
    append_event(&conn, &session.id, event_type, payload_json)?;
    Ok(true)
}

pub fn resolve_runtime_session(state_db_path: &Path, request: &SessionRequest) -> Result<SessionRuntimeReport> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;

    let interaction_mode = if request.query_present || request.image_present {
        InteractionMode::SingleTurn
    } else {
        InteractionMode::Interactive
    };

    if let Some(resume) = request.resume.as_deref() {
        let session = find_session_by_id_or_title(&conn, resume)?
            .with_context(|| format!("session not found for resume target {resume}"))?;
        let action = if session.id == resume {
            SessionAction::ResumedById
        } else {
            SessionAction::ResumedByTitle
        };
        touch_session(&conn, &session.id)?;
        append_event(
            &conn,
            &session.id,
            "session_resumed",
            json!({
                "action": action.label(),
                "resume_target": resume,
                "interaction_mode": interaction_mode.label(),
            })
            .to_string(),
        )?;
        append_request_message_if_present(&conn, &session.id, request)?;
        return Ok(SessionRuntimeReport {
            session_id: session.id,
            action,
            interaction_mode,
            title: session.title,
        });
    }

    if let Some(target) = request.continue_last.as_deref() {
        let session = if target.trim().is_empty() {
            latest_session(&conn)?
        } else {
            find_session_by_title(&conn, target)?
        }
        .with_context(|| format!("session not found for continue target {target}"))?;
        touch_session(&conn, &session.id)?;
        let action = if target.trim().is_empty() {
            SessionAction::ResumedLatest
        } else {
            SessionAction::ResumedByTitle
        };
        append_event(
            &conn,
            &session.id,
            "session_resumed",
            json!({
                "action": action.label(),
                "continue_target": target,
                "interaction_mode": interaction_mode.label(),
            })
            .to_string(),
        )?;
        append_request_message_if_present(&conn, &session.id, request)?;
        return Ok(SessionRuntimeReport {
            session_id: session.id,
            action,
            interaction_mode,
            title: session.title,
        });
    }

    let now = unix_timestamp();
    let unique = unix_timestamp_nanos();
    let session_id = format!("session-{}", unique);
    let title = format!("{}-{}", request.command_name, unique);
    conn.execute(
        "INSERT INTO sessions(id, title, command_name, interaction_mode, created_at, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?5)",
        params![session_id, title, request.command_name, interaction_mode.label(), now],
    )?;
    append_event(
        &conn,
        &session_id,
        "session_created",
        json!({
            "command_name": request.command_name,
            "interaction_mode": interaction_mode.label(),
            "query_present": request.query_present,
            "image_present": request.image_present,
        })
        .to_string(),
    )?;
    append_request_message_if_present(&conn, &session_id, request)?;

    Ok(SessionRuntimeReport {
        session_id,
        action: SessionAction::Created,
        interaction_mode,
        title,
    })
}

pub fn resolve_command_session(
    state_db_path: &Path,
    command_name: &str,
    interaction_mode: InteractionMode,
) -> Result<SessionRuntimeReport> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;

    if let Some(session) = latest_session_for_command(&conn, command_name)? {
        let tx = conn.unchecked_transaction()?;
        touch_session(&tx, &session.id)?;
        append_event(
            &tx,
            &session.id,
            "session_resumed",
            json!({
                "action": SessionAction::ResumedLatest.label(),
                "command_name": command_name,
                "interaction_mode": interaction_mode.label(),
                "source": "gateway",
            })
            .to_string(),
        )?;
        tx.commit()?;
        return Ok(SessionRuntimeReport {
            session_id: session.id,
            action: SessionAction::ResumedLatest,
            interaction_mode,
            title: session.title,
        });
    }

    let tx = conn.unchecked_transaction()?;
    let now = unix_timestamp();
    let unique = unix_timestamp_nanos();
    let session_id = format!("session-{}", unique);
    let title = format!("{}-{}", command_name, unique);
    tx.execute(
        "INSERT INTO sessions(id, title, command_name, interaction_mode, created_at, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?5)",
        params![session_id, title, command_name, interaction_mode.label(), now],
    )?;
    append_event(
        &tx,
        &session_id,
        "session_created",
        json!({
            "command_name": command_name,
            "interaction_mode": interaction_mode.label(),
            "query_present": false,
            "image_present": false,
            "source": "gateway",
        })
        .to_string(),
    )?;
    tx.commit()?;

    Ok(SessionRuntimeReport {
        session_id,
        action: SessionAction::Created,
        interaction_mode,
        title,
    })
}

fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS state_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            command_name TEXT NOT NULL,
            interaction_mode TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            metadata_json TEXT,
            FOREIGN KEY(session_id) REFERENCES sessions(id)
        );
        CREATE TABLE IF NOT EXISTS session_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id)
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
            message_id UNINDEXED,
            session_id UNINDEXED,
            title UNINDEXED,
            content
        );
        CREATE INDEX IF NOT EXISTS idx_messages_session_created ON messages(session_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_session_events_session_created ON session_events(session_id, created_at);
        ",
    )?;
    conn.execute(
        "INSERT INTO message_fts(message_id, session_id, title, content)
         SELECT m.id, m.session_id, s.title, m.content
         FROM messages m
         JOIN sessions s ON s.id = m.session_id
         WHERE NOT EXISTS (
           SELECT 1 FROM message_fts f WHERE f.message_id = m.id
         )",
        [],
    )?;
    Ok(())
}

fn append_request_message_if_present(conn: &Connection, session_id: &str, request: &SessionRequest) -> Result<()> {
    if let Some(text) = request.query_text.as_deref() {
        if !text.trim().is_empty() {
            append_message(
                conn,
                session_id,
                "user",
                text,
                Some(
                    json!({
                        "command_name": request.command_name,
                        "image_path": request.image_path,
                    })
                    .to_string(),
                ),
            )?;
        }
    }
    Ok(())
}

fn append_message(
    conn: &Connection,
    session_id: &str,
    role: &str,
    content: &str,
    metadata_json: Option<String>,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let now = unix_timestamp();
    let id = format!("msg-{}-{}", session_id, unix_timestamp_nanos());
    tx.execute(
        "INSERT INTO messages(id, session_id, role, content, created_at, metadata_json)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, session_id, role, content, now, metadata_json],
    )?;
    let session_title = session_title(&tx, session_id)?.unwrap_or_else(|| session_id.to_string());
    tx.execute(
        "INSERT INTO message_fts(message_id, session_id, title, content)
         VALUES(?1, ?2, ?3, ?4)",
        params![id, session_id, session_title, content],
    )?;
    touch_session_at(&tx, session_id, now)?;
    tx.commit()?;
    Ok(())
}

fn append_event(conn: &Connection, session_id: &str, event_type: &str, payload_json: String) -> Result<()> {
    let now = unix_timestamp();
    let id = format!("evt-{}-{}", session_id, unix_timestamp_nanos());
    conn.execute(
        "INSERT INTO session_events(id, session_id, event_type, payload_json, created_at)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![id, session_id, event_type, payload_json, now],
    )?;
    touch_session_at(conn, session_id, now)?;
    Ok(())
}

fn load_session_inspection(conn: &Connection, session_id: &str, title: &str, limit: usize) -> Result<SessionInspection> {
    let mut message_stmt = conn.prepare(
        "SELECT id, role, content, created_at
         FROM messages
         WHERE session_id = ?1
         ORDER BY created_at DESC, id DESC
         LIMIT ?2",
    )?;
    let message_rows = message_stmt.query_map(params![session_id, limit as i64], |row| {
        Ok(SessionMessageRecord {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    let mut messages = Vec::new();
    for row in message_rows {
        messages.push(row?);
    }
    messages.reverse();

    let mut event_stmt = conn.prepare(
        "SELECT id, event_type, payload_json, created_at
         FROM session_events
         WHERE session_id = ?1
         ORDER BY created_at DESC, id DESC
         LIMIT ?2",
    )?;
    let event_rows = event_stmt.query_map(params![session_id, limit as i64], |row| {
        Ok(SessionEventRecord {
            id: row.get(0)?,
            event_type: row.get(1)?,
            payload_json: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    let mut events = Vec::new();
    for row in event_rows {
        events.push(row?);
    }
    events.reverse();

    Ok(SessionInspection {
        session_id: session_id.to_string(),
        title: title.to_string(),
        messages,
        events,
    })
}

fn load_summary(conn: &Connection, session_id: &str, title: &str) -> Result<SessionSummary> {
    let message_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    let event_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM session_events WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    Ok(SessionSummary {
        id: session_id.to_string(),
        title: title.to_string(),
        message_count,
        event_count,
    })
}

fn current_meta_u64(conn: &Connection, key: &str) -> Result<Option<u64>> {
    let current: Option<String> = conn
        .query_row(
            "SELECT value FROM state_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(current.as_deref().and_then(|s| s.parse::<u64>().ok()))
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO state_meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn latest_session(conn: &Connection) -> Result<Option<StoredSession>> {
    Ok(conn
        .query_row(
            "SELECT id, title FROM sessions ORDER BY updated_at DESC LIMIT 1",
            [],
            |row| {
                Ok(StoredSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            },
        )
        .optional()?)
}

fn latest_session_for_command(conn: &Connection, command_name: &str) -> Result<Option<StoredSession>> {
    Ok(conn
        .query_row(
            "SELECT id, title FROM sessions WHERE command_name = ?1 ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![command_name],
            |row| {
                Ok(StoredSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            },
        )
        .optional()?)
}

fn find_session_by_title(conn: &Connection, title: &str) -> Result<Option<StoredSession>> {
    Ok(conn
        .query_row(
            "SELECT id, title FROM sessions WHERE title = ?1 ORDER BY updated_at DESC LIMIT 1",
            params![title],
            |row| {
                Ok(StoredSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            },
        )
        .optional()?)
}

fn find_session_by_id_or_title(conn: &Connection, value: &str) -> Result<Option<StoredSession>> {
    if let Some(session) = conn
        .query_row(
            "SELECT id, title FROM sessions WHERE id = ?1 LIMIT 1",
            params![value],
            |row| {
                Ok(StoredSession {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            },
        )
        .optional()?
    {
        return Ok(Some(session));
    }
    find_session_by_title(conn, value)
}

fn session_title(conn: &Connection, session_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT title FROM sessions WHERE id = ?1 LIMIT 1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn touch_session(conn: &Connection, session_id: &str) -> Result<()> {
    touch_session_at(conn, session_id, unix_timestamp())
}

fn touch_session_at(conn: &Connection, session_id: &str, timestamp: i64) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
        params![session_id, timestamp],
    )?;
    Ok(())
}

fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_event_targets_requested_session() {
        let vela_home = std::env::temp_dir().join(format!("vela-state-test-{}", unix_timestamp_nanos()));
        let report = initialize_persistence(&vela_home).unwrap();

        let first = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("first".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
        )
        .unwrap();
        let _second = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("second".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
        )
        .unwrap();

        assert!(append_event_to_session(
            &report.state_db_path,
            &first.session_id,
            "review_candidate_created",
            "{}".to_string(),
        )
        .unwrap());

        let conn = Connection::open(&report.state_db_path).unwrap();
        let first_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_events WHERE session_id = ?1 AND event_type = 'review_candidate_created'",
                params![first.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(first_count, 1);

        let _ = fs::remove_dir_all(&vela_home);
    }

    #[test]
    fn resolve_command_session_reuses_latest_matching_command() {
        let vela_home = std::env::temp_dir().join(format!("vela-state-gateway-test-{}", unix_timestamp_nanos()));
        let report = initialize_persistence(&vela_home).unwrap();

        let first = resolve_command_session(&report.state_db_path, "gateway", InteractionMode::Interactive).unwrap();
        let second = resolve_command_session(&report.state_db_path, "gateway", InteractionMode::Interactive).unwrap();

        assert_eq!(first.session_id, second.session_id);
        assert!(matches!(first.action, SessionAction::Created));
        assert!(matches!(second.action, SessionAction::ResumedLatest));

        assert!(append_message_to_session(
            &report.state_db_path,
            &second.session_id,
            "system",
            "Gateway bootstrap ready.",
            Some(json!({"source": "gateway", "direction": "egress"}).to_string()),
        )
        .unwrap());

        let summary = current_command_session_summary(&report.state_db_path, "gateway")
            .unwrap()
            .unwrap();
        assert_eq!(summary.id, first.session_id);
        assert_eq!(summary.message_count, 1);

        let _ = fs::remove_dir_all(&vela_home);
    }
}
