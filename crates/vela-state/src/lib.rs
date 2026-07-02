use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
/// Represents `PersistenceReport` data exposed by this crate.
pub struct PersistenceReport {
    pub state_db_path: PathBuf,
    pub sessions_dir: PathBuf,
    pub snapshot_pattern: String,
    pub state_db_existed_before: bool,
    pub bootstrap_runs: u64,
}

#[derive(Debug, Clone)]
/// Represents `SessionRequest` data exposed by this crate.
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
/// Represents `SessionRuntimeReport` data exposed by this crate.
pub struct SessionRuntimeReport {
    pub session_id: String,
    pub action: SessionAction,
    pub interaction_mode: InteractionMode,
    pub title: String,
}

#[derive(Debug, Clone)]
/// Represents `SessionSummary` data exposed by this crate.
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub message_count: u64,
    pub event_count: u64,
    pub parent_session_id: Option<String>,
}

#[derive(Debug, Clone)]
/// Represents `SessionSearchHit` data exposed by this crate.
pub struct SessionSearchHit {
    pub session_id: String,
    pub session_title: String,
    pub message_id: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
/// Represents `SessionMessageRecord` data exposed by this crate.
pub struct SessionMessageRecord {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone)]
/// Represents `SessionEventRecord` data exposed by this crate.
pub struct SessionEventRecord {
    pub id: String,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
/// Represents `RuntimeTurnLifecycleRecord` data exposed by this crate.
pub struct RuntimeTurnLifecycleRecord {
    pub event_id: String,
    pub turn_id: String,
    pub phase: String,
    pub sequence: u64,
    pub step: Option<u64>,
    pub detail_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
/// Represents `SessionBranchRecord` data exposed by this crate.
pub struct SessionBranchRecord {
    pub session_id: String,
    pub title: String,
    pub parent_session_id: Option<String>,
    pub parent_title: Option<String>,
    pub branch_note: Option<String>,
}

#[derive(Debug, Clone)]
/// Represents `SessionCompressionRecord` data exposed by this crate.
pub struct SessionCompressionRecord {
    pub id: String,
    pub session_id: String,
    pub summary: String,
    pub source_message_count: u64,
    pub source_event_count: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
/// Represents `SessionInspection` data exposed by this crate.
pub struct SessionInspection {
    pub session_id: String,
    pub title: String,
    pub branch: SessionBranchRecord,
    pub child_sessions: Vec<SessionSummary>,
    pub messages: Vec<SessionMessageRecord>,
    pub events: Vec<SessionEventRecord>,
    pub lifecycle: Vec<RuntimeTurnLifecycleRecord>,
    pub compressions: Vec<SessionCompressionRecord>,
}

#[derive(Debug, Clone, Copy)]
/// Enumerates supported `SessionAction` variants.
pub enum SessionAction {
    Created,
    ResumedById,
    ResumedByTitle,
    ResumedLatest,
}

impl SessionAction {
    /// Returns the stable string label used for persistence and display.
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
/// Enumerates supported `InteractionMode` variants.
pub enum InteractionMode {
    Interactive,
    SingleTurn,
}

impl InteractionMode {
    /// Returns the stable string label used for persistence and display.
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

#[derive(Debug, Deserialize)]
struct RuntimeTurnLifecyclePayload {
    turn_id: String,
    phase: String,
    sequence: u64,
    #[serde(default)]
    step: Option<u64>,
    #[serde(default)]
    detail: Option<Value>,
}

/// Initializes persistence state for this subsystem.
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

/// Returns the current session identity when available.
pub fn current_session_identity(state_db_path: &Path) -> Result<Option<(String, String)>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    Ok(latest_session(&conn)?.map(|session| (session.id, session.title)))
}

/// Returns the current session summary when available.
pub fn current_session_summary(state_db_path: &Path) -> Result<Option<SessionSummary>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session(&conn)? else {
        return Ok(None);
    };
    Ok(Some(load_summary(&conn, &session.id, &session.title)?))
}

/// Returns the current command session summary when available.
pub fn current_command_session_summary(
    state_db_path: &Path,
    command_name: &str,
) -> Result<Option<SessionSummary>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session_for_command(&conn, command_name)? else {
        return Ok(None);
    };
    Ok(Some(load_summary(&conn, &session.id, &session.title)?))
}

/// Searches session history and returns matching results.
pub fn search_session_history(
    state_db_path: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SessionSearchHit>> {
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

/// Inspects the most recently updated session with recent messages, events, lifecycle, and compression state.
pub fn inspect_latest_session(
    state_db_path: &Path,
    limit: usize,
) -> Result<Option<SessionInspection>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session(&conn)? else {
        return Ok(None);
    };
    Ok(Some(load_session_inspection(
        &conn,
        &session.id,
        &session.title,
        limit,
    )?))
}

/// Inspects one session by id or title with recent messages, events, lifecycle, lineage, and compression state.
pub fn inspect_session(
    state_db_path: &Path,
    target: &str,
    limit: usize,
) -> Result<Option<SessionInspection>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = find_session_by_id_or_title(&conn, target)? else {
        return Ok(None);
    };
    Ok(Some(load_session_inspection(
        &conn,
        &session.id,
        &session.title,
        limit,
    )?))
}

/// Appends event to session to persisted session state.
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

/// Appends message to session to persisted session state.
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

/// Appends event to latest session to persisted session state.
pub fn append_event_to_latest_session(
    state_db_path: &Path,
    event_type: &str,
    payload_json: String,
) -> Result<bool> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let Some(session) = latest_session(&conn)? else {
        return Ok(false);
    };
    append_event(&conn, &session.id, event_type, payload_json)?;
    Ok(true)
}

/// Resolves runtime session from persisted state and runtime inputs.
pub fn resolve_runtime_session(
    state_db_path: &Path,
    request: &SessionRequest,
) -> Result<SessionRuntimeReport> {
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
        let action = if target.trim().is_empty() {
            SessionAction::ResumedLatest
        } else {
            SessionAction::ResumedByTitle
        };
        let (session, anchor_id, anchor_title) = if target.trim().is_empty() {
            let session = latest_session(&conn)?
                .with_context(|| format!("session not found for continue target {target}"))?;
            (session.clone(), Some(session.id), Some(session.title))
        } else {
            let anchor = find_session_by_id_or_title(&conn, target)?
                .with_context(|| format!("session not found for continue target {target}"))?;
            let session = latest_session_in_subtree(&conn, &anchor.id)?.unwrap_or(anchor.clone());
            (session, Some(anchor.id), Some(anchor.title))
        };
        touch_session(&conn, &session.id)?;
        append_event(
            &conn,
            &session.id,
            "session_resumed",
            json!({
                "action": action.label(),
                "continue_target": target,
                "continue_anchor_session_id": anchor_id,
                "continue_anchor_title": anchor_title,
                "resolved_session_id": session.id,
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
        params![
            session_id,
            title,
            request.command_name,
            interaction_mode.label(),
            now
        ],
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

/// Creates a durable child session by copying continuity from a source session and recording explicit parent lineage.
pub fn branch_session(
    state_db_path: &Path,
    source: &str,
    new_title: Option<&str>,
    branch_note: Option<&str>,
) -> Result<SessionBranchRecord> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let source_session = find_session_by_id_or_title(&conn, source)?
        .with_context(|| format!("session not found for branch source {source}"))?;
    let mut detail_stmt =
        conn.prepare("SELECT command_name, interaction_mode FROM sessions WHERE id = ?1")?;
    let (command_name, interaction_mode): (String, String) = detail_stmt
        .query_row(params![source_session.id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

    let now = unix_timestamp();
    let unique = unix_timestamp_nanos();
    let session_id = format!("session-{}", unique);
    let title = new_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-branch-{}", source_session.title, unique));
    let note = branch_note
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO sessions(id, title, command_name, interaction_mode, created_at, updated_at, parent_session_id, branch_note)
         VALUES(?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
        params![session_id, title, command_name, interaction_mode, now, source_session.id, note],
    )?;

    let messages: Vec<(String, String, Option<String>, i64)> = {
        let mut message_stmt = tx.prepare(
            "SELECT role, content, metadata_json, created_at FROM messages WHERE session_id = ?1 ORDER BY created_at ASC, id ASC"
        )?;
        let rows = message_stmt.query_map(params![source_session.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        values
    };
    for (role, content, metadata_json, created_at) in messages {
        let id = format!("msg-{}-{}", session_id, unix_timestamp_nanos());
        tx.execute(
            "INSERT INTO messages(id, session_id, role, content, created_at, metadata_json)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, role, content, created_at, metadata_json],
        )?;
        tx.execute(
            "INSERT INTO message_fts(message_id, session_id, title, content) VALUES(?1, ?2, ?3, ?4)",
            params![id, session_id, title, content],
        )?;
    }

    let events: Vec<(String, String, i64)> = {
        let mut event_stmt = tx.prepare(
            "SELECT event_type, payload_json, created_at FROM session_events WHERE session_id = ?1 ORDER BY created_at ASC, id ASC"
        )?;
        let rows = event_stmt.query_map(params![source_session.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        values
    };
    for (event_type, payload_json, created_at) in events {
        let id = format!("evt-{}-{}", session_id, unix_timestamp_nanos());
        tx.execute(
            "INSERT INTO session_events(id, session_id, event_type, payload_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, event_type, payload_json, created_at],
        )?;
    }

    let compressions: Vec<(String, i64, i64, i64)> = {
        let mut compression_stmt = tx.prepare(
            "SELECT summary, source_message_count, source_event_count, created_at FROM session_compressions WHERE session_id = ?1 ORDER BY created_at ASC, id ASC"
        )?;
        let rows = compression_stmt.query_map(params![source_session.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        values
    };
    for (summary, source_message_count, source_event_count, created_at) in compressions {
        let id = format!("cmp-{}-{}", session_id, unix_timestamp_nanos());
        tx.execute(
            "INSERT INTO session_compressions(id, session_id, summary, source_message_count, source_event_count, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, summary, source_message_count, source_event_count, created_at],
        )?;
    }

    append_event(
        &tx,
        &session_id,
        "session_branched",
        json!({
            "parent_session_id": source_session.id,
            "parent_title": source_session.title,
            "branch_note": note,
        })
        .to_string(),
    )?;
    tx.commit()?;

    Ok(SessionBranchRecord {
        session_id,
        title,
        parent_session_id: Some(source_session.id),
        parent_title: Some(source_session.title),
        branch_note: note,
    })
}

/// Persists one compression summary for a session and records the matching audit event atomically.
pub fn compress_session(
    state_db_path: &Path,
    target: &str,
    summary: &str,
) -> Result<SessionCompressionRecord> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    let session = find_session_by_id_or_title(&conn, target)?
        .with_context(|| format!("session not found for compression target {target}"))?;
    let summary = summary.trim();
    if summary.is_empty() {
        anyhow::bail!("compression summary cannot be empty");
    }
    if latest_compression_summary(state_db_path, &session.id)?
        .as_deref()
        .is_some_and(|existing| existing.trim() == summary)
    {
        anyhow::bail!("compression summary matches the latest persisted summary");
    }
    let source_message_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
        params![session.id],
        |row| row.get(0),
    )?;
    let source_event_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM session_events WHERE session_id = ?1",
        params![session.id],
        |row| row.get(0),
    )?;
    let created_at = unix_timestamp();
    let id = format!("cmp-{}", unix_timestamp_nanos());
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO session_compressions(id, session_id, summary, source_message_count, source_event_count, created_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, session.id, summary, source_message_count as i64, source_event_count as i64, created_at],
    )?;
    append_event(
        &tx,
        &session.id,
        "session_compressed",
        json!({
            "compression_id": id,
            "summary": summary,
            "source_message_count": source_message_count,
            "source_event_count": source_event_count,
        })
        .to_string(),
    )?;
    touch_session_at(&tx, &session.id, created_at)?;
    tx.commit()?;
    Ok(SessionCompressionRecord {
        id,
        session_id: session.id,
        summary: summary.to_string(),
        source_message_count,
        source_event_count,
        created_at,
    })
}

/// Returns the latest persisted compression summary for one session when present.
pub fn latest_compression_summary(
    state_db_path: &Path,
    session_id: &str,
) -> Result<Option<String>> {
    let conn = Connection::open(state_db_path)
        .with_context(|| format!("failed to open {}", state_db_path.display()))?;
    Ok(conn.query_row(
        "SELECT summary FROM session_compressions WHERE session_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 1",
        params![session_id],
        |row| row.get(0),
    ).optional()?)
}

/// Resolves command session from persisted state and runtime inputs.
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
        params![
            session_id,
            title,
            command_name,
            interaction_mode.label(),
            now
        ],
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
            updated_at INTEGER NOT NULL,
            parent_session_id TEXT,
            branch_note TEXT
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
        CREATE TABLE IF NOT EXISTS session_compressions (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            summary TEXT NOT NULL,
            source_message_count INTEGER NOT NULL,
            source_event_count INTEGER NOT NULL,
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
        CREATE INDEX IF NOT EXISTS idx_session_compressions_session_created ON session_compressions(session_id, created_at);
        ",
    )?;
    ensure_messages_metadata_column(conn)?;
    ensure_sessions_branch_columns(conn)?;
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

fn ensure_messages_metadata_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(messages)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == "metadata_json" {
            return Ok(());
        }
    }
    conn.execute("ALTER TABLE messages ADD COLUMN metadata_json TEXT", [])?;
    Ok(())
}

fn ensure_sessions_branch_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_parent = false;
    let mut has_note = false;
    for column in columns {
        match column?.as_str() {
            "parent_session_id" => has_parent = true,
            "branch_note" => has_note = true,
            _ => {}
        }
    }
    if !has_parent {
        conn.execute("ALTER TABLE sessions ADD COLUMN parent_session_id TEXT", [])?;
    }
    if !has_note {
        conn.execute("ALTER TABLE sessions ADD COLUMN branch_note TEXT", [])?;
    }
    Ok(())
}

fn append_request_message_if_present(
    conn: &Connection,
    session_id: &str,
    request: &SessionRequest,
) -> Result<()> {
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
            return Ok(());
        }
    }

    if request.image_present {
        append_message(
            conn,
            session_id,
            "user",
            request.image_path.as_deref().unwrap_or("[image]"),
            Some(
                json!({
                    "command_name": request.command_name,
                    "image_path": request.image_path,
                    "image_only": true,
                })
                .to_string(),
            ),
        )?;
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

fn append_event(
    conn: &Connection,
    session_id: &str,
    event_type: &str,
    payload_json: String,
) -> Result<()> {
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

fn load_session_inspection(
    conn: &Connection,
    session_id: &str,
    title: &str,
    limit: usize,
) -> Result<SessionInspection> {
    let branch = load_branch_record(conn, session_id, title)?;
    let child_sessions = load_child_summaries(conn, session_id, limit)?;
    let mut message_stmt = conn.prepare(
        "SELECT id, role, content, created_at, metadata_json
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
            metadata_json: row.get(4)?,
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

    let mut lifecycle = Vec::new();
    for event in &events {
        if event.event_type != "runtime_turn_phase" {
            continue;
        }
        let payload: RuntimeTurnLifecyclePayload = match serde_json::from_str(&event.payload_json) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(event_id=%event.id, error=%error, "skipping malformed runtime lifecycle payload during session inspection");
                continue;
            }
        };
        lifecycle.push(RuntimeTurnLifecycleRecord {
            event_id: event.id.clone(),
            turn_id: payload.turn_id,
            phase: payload.phase,
            sequence: payload.sequence,
            step: payload.step,
            detail_json: payload.detail.map(|value| value.to_string()),
            created_at: event.created_at,
        });
    }

    let mut compression_stmt = conn.prepare(
        "SELECT id, summary, source_message_count, source_event_count, created_at
         FROM session_compressions
         WHERE session_id = ?1
         ORDER BY created_at DESC, id DESC
         LIMIT ?2",
    )?;
    let compression_rows =
        compression_stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(SessionCompressionRecord {
                id: row.get(0)?,
                session_id: session_id.to_string(),
                summary: row.get(1)?,
                source_message_count: row.get::<_, i64>(2)? as u64,
                source_event_count: row.get::<_, i64>(3)? as u64,
                created_at: row.get(4)?,
            })
        })?;
    let mut compressions = Vec::new();
    for row in compression_rows {
        compressions.push(row?);
    }
    compressions.reverse();

    Ok(SessionInspection {
        session_id: session_id.to_string(),
        title: title.to_string(),
        branch,
        child_sessions,
        messages,
        events,
        lifecycle,
        compressions,
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
    let parent_session_id: Option<String> = conn
        .query_row(
            "SELECT parent_session_id FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(SessionSummary {
        id: session_id.to_string(),
        title: title.to_string(),
        message_count,
        event_count,
        parent_session_id,
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

fn load_branch_record(
    conn: &Connection,
    session_id: &str,
    title: &str,
) -> Result<SessionBranchRecord> {
    let (parent_session_id, branch_note): (Option<String>, Option<String>) = conn.query_row(
        "SELECT parent_session_id, branch_note FROM sessions WHERE id = ?1",
        params![session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let parent_title = if let Some(parent_session_id) = parent_session_id.as_deref() {
        session_title(conn, parent_session_id)?
    } else {
        None
    };
    Ok(SessionBranchRecord {
        session_id: session_id.to_string(),
        title: title.to_string(),
        parent_session_id,
        parent_title,
        branch_note,
    })
}

fn latest_session_in_subtree(
    conn: &Connection,
    anchor_session_id: &str,
) -> Result<Option<StoredSession>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, parent_session_id, updated_at FROM sessions ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row?);
    }
    for (id, title, _parent_session_id, _updated_at) in &sessions {
        let mut cursor = Some(id.as_str());
        while let Some(current) = cursor {
            if current == anchor_session_id {
                return Ok(Some(StoredSession {
                    id: id.clone(),
                    title: title.clone(),
                }));
            }
            cursor = sessions
                .iter()
                .find(|(candidate_id, _, _, _)| candidate_id == current)
                .and_then(|(_, _, parent_session_id, _)| parent_session_id.as_deref());
        }
    }
    Ok(None)
}

fn load_child_summaries(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> Result<Vec<SessionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, title FROM sessions WHERE parent_session_id = ?1 ORDER BY updated_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![session_id, limit as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut children = Vec::new();
    for row in rows {
        let (child_id, child_title) = row?;
        children.push(load_summary(conn, &child_id, &child_title)?);
    }
    Ok(children)
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

fn latest_session_for_command(
    conn: &Connection,
    command_name: &str,
) -> Result<Option<StoredSession>> {
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
        let vela_home =
            std::env::temp_dir().join(format!("vela-state-test-{}", unix_timestamp_nanos()));
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
        let vela_home = std::env::temp_dir().join(format!(
            "vela-state-gateway-test-{}",
            unix_timestamp_nanos()
        ));
        let report = initialize_persistence(&vela_home).unwrap();

        let first = resolve_command_session(
            &report.state_db_path,
            "gateway",
            InteractionMode::Interactive,
        )
        .unwrap();
        let second = resolve_command_session(
            &report.state_db_path,
            "gateway",
            InteractionMode::Interactive,
        )
        .unwrap();

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

    #[test]
    fn branch_and_compression_preserve_lineage_and_inspection() {
        let vela_home =
            std::env::temp_dir().join(format!("vela-state-branch-test-{}", unix_timestamp_nanos()));
        let report = initialize_persistence(&vela_home).unwrap();
        let parent = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("parent turn".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
        )
        .unwrap();
        assert!(append_message_to_session(
            &report.state_db_path,
            &parent.session_id,
            "assistant",
            "parent reply",
            None
        )
        .unwrap());

        let branch = branch_session(
            &report.state_db_path,
            &parent.session_id,
            Some("branch-a"),
            Some("explore alternative"),
        )
        .unwrap();
        let compression = compress_session(
            &report.state_db_path,
            &branch.session_id,
            "branch compressed summary",
        )
        .unwrap();
        let inspection = inspect_session(&report.state_db_path, &branch.session_id, 20)
            .unwrap()
            .expect("branch inspection");
        assert_eq!(
            inspection.branch.parent_session_id.as_deref(),
            Some(parent.session_id.as_str())
        );
        assert_eq!(
            inspection.branch.branch_note.as_deref(),
            Some("explore alternative")
        );
        assert_eq!(inspection.messages.len(), 2);
        assert_eq!(inspection.compressions.len(), 1);
        assert!(inspection.child_sessions.is_empty());
        assert_eq!(inspection.compressions[0].id, compression.id);
        let parent_inspection = inspect_session(&report.state_db_path, &parent.session_id, 20)
            .unwrap()
            .expect("parent inspection");
        assert_eq!(parent_inspection.child_sessions.len(), 1);
        assert_eq!(parent_inspection.child_sessions[0].id, branch.session_id);
        let summary = load_summary(
            &Connection::open(&report.state_db_path).unwrap(),
            &branch.session_id,
            &branch.title,
        )
        .unwrap();
        assert_eq!(
            summary.parent_session_id.as_deref(),
            Some(parent.session_id.as_str())
        );

        let _ = fs::remove_dir_all(&vela_home);
    }

    #[test]
    fn continue_target_prefers_latest_session_in_branch_subtree() {
        let vela_home = std::env::temp_dir().join(format!(
            "vela-state-continue-test-{}",
            unix_timestamp_nanos()
        ));
        let report = initialize_persistence(&vela_home).unwrap();
        let root = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("root turn".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
        )
        .unwrap();
        let branch_a = branch_session(
            &report.state_db_path,
            &root.session_id,
            Some("branch-a"),
            None,
        )
        .unwrap();
        let _branch_b = branch_session(
            &report.state_db_path,
            &root.session_id,
            Some("branch-b"),
            None,
        )
        .unwrap();
        let branch_a_child = branch_session(
            &report.state_db_path,
            &branch_a.session_id,
            Some("branch-a-child"),
            None,
        )
        .unwrap();

        let continued = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("continue root".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: Some(root.title.clone()),
            },
        )
        .unwrap();
        assert_eq!(continued.session_id, branch_a_child.session_id);

        let continued_branch = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("continue branch-a".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: Some(branch_a.title.clone()),
            },
        )
        .unwrap();
        assert_eq!(continued_branch.session_id, branch_a_child.session_id);

        let _ = fs::remove_dir_all(&vela_home);
    }

    #[test]
    fn duplicate_compression_summary_is_rejected() {
        let vela_home = std::env::temp_dir().join(format!(
            "vela-state-compress-test-{}",
            unix_timestamp_nanos()
        ));
        let report = initialize_persistence(&vela_home).unwrap();
        let session = resolve_runtime_session(
            &report.state_db_path,
            &SessionRequest {
                command_name: "chat".to_string(),
                query_present: true,
                query_text: Some("compress me".to_string()),
                image_present: false,
                image_path: None,
                resume: None,
                continue_last: None,
            },
        )
        .unwrap();
        compress_session(&report.state_db_path, &session.session_id, "same summary").unwrap();
        let error = compress_session(&report.state_db_path, &session.session_id, "same summary")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("matches the latest persisted summary"));

        let _ = fs::remove_dir_all(&vela_home);
    }

    #[test]
    fn initialize_persistence_migrates_legacy_messages_without_metadata_column() {
        let vela_home = std::env::temp_dir().join(format!(
            "vela-state-legacy-metadata-test-{}",
            unix_timestamp_nanos()
        ));
        fs::create_dir_all(&vela_home).unwrap();
        let state_db_path = vela_home.join("state.db");
        let conn = Connection::open(&state_db_path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE state_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                command_name TEXT NOT NULL,
                interaction_mode TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE TABLE session_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE VIRTUAL TABLE message_fts USING fts5(
                message_id UNINDEXED,
                session_id UNINDEXED,
                title UNINDEXED,
                content
            );
            ",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions(id, title, command_name, interaction_mode, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["session-legacy", "Legacy", "chat", "single-turn", 1_i64, 1_i64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages(id, session_id, role, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "message-legacy",
                "session-legacy",
                "assistant",
                "hello",
                1_i64
            ],
        )
        .unwrap();
        drop(conn);

        let report = initialize_persistence(&vela_home).unwrap();
        let inspection = inspect_latest_session(&report.state_db_path, 10)
            .unwrap()
            .expect("legacy session inspection");
        assert_eq!(inspection.messages.len(), 1);
        assert_eq!(inspection.messages[0].metadata_json, None);

        let conn = Connection::open(&report.state_db_path).unwrap();
        let metadata_exists: bool = conn
            .prepare("PRAGMA table_info(messages)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .any(|name| name == "metadata_json");
        assert!(metadata_exists);

        let _ = fs::remove_dir_all(&vela_home);
    }
}
