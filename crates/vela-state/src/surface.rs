use super::*;
use unicode_segmentation::UnicodeSegmentation;

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
    pub continue_target: Option<String>,
    pub continue_resolution: Option<String>,
    pub continue_anchor_session_id: Option<String>,
    pub continue_anchor_title: Option<String>,
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

pub const SESSION_COMPRESSION_CHAR_LIMIT: usize = 2_000;

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
pub(crate) struct StoredSession {
    pub(crate) id: String,
    pub(crate) title: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RuntimeTurnLifecyclePayload {
    pub(crate) turn_id: String,
    pub(crate) phase: String,
    pub(crate) sequence: u64,
    #[serde(default)]
    pub(crate) step: Option<u64>,
    #[serde(default)]
    pub(crate) detail: Option<Value>,
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
        let report = {
            let tx = conn.unchecked_transaction()?;
            touch_session(&tx, &session.id)?;
            append_event(
                &tx,
                &session.id,
                "session_resumed",
                json!({
                    "action": action.label(),
                    "resume_target": resume,
                    "interaction_mode": interaction_mode.label(),
                })
                .to_string(),
            )?;
            tx.commit()?;
            SessionRuntimeReport {
                session_id: session.id,
                action,
                interaction_mode,
                title: session.title,
                continue_target: None,
                continue_resolution: None,
                continue_anchor_session_id: None,
                continue_anchor_title: None,
            }
        };
        append_request_message_if_present(&conn, &report.session_id, request)?;
        return Ok(report);
    }

    if let Some(target) = request.continue_last.as_deref() {
        let action = if target.trim().is_empty() {
            SessionAction::ResumedLatest
        } else {
            SessionAction::ResumedByTitle
        };
        let (session, anchor_id, anchor_title, continue_resolution) = if target.trim().is_empty() {
            let session = latest_session(&conn)?
                .with_context(|| format!("session not found for continue target {target}"))?;
            (session, None, None, "latest-global")
        } else {
            let anchor = find_session_by_id_or_title(&conn, target)?
                .with_context(|| format!("session not found for continue target {target}"))?;
            let session = latest_session_in_subtree(&conn, &anchor.id)?.unwrap_or(anchor.clone());
            let continue_resolution = if session.id == anchor.id {
                "exact-anchor"
            } else {
                "latest-in-subtree"
            };
            (
                session,
                Some(anchor.id),
                Some(anchor.title),
                continue_resolution,
            )
        };
        let report = {
            let tx = conn.unchecked_transaction()?;
            touch_session(&tx, &session.id)?;
            append_event(
                &tx,
                &session.id,
                "session_resumed",
                json!({
                    "action": action.label(),
                    "continue_target": target,
                    "continue_anchor_session_id": anchor_id,
                    "continue_anchor_title": anchor_title,
                    "resolved_session_id": session.id,
                    "continue_resolution": continue_resolution,
                    "interaction_mode": interaction_mode.label(),
                })
                .to_string(),
            )?;
            tx.commit()?;
            SessionRuntimeReport {
                session_id: session.id,
                action,
                interaction_mode,
                title: session.title,
                continue_target: Some(target.to_string()),
                continue_resolution: Some(continue_resolution.to_string()),
                continue_anchor_session_id: anchor_id,
                continue_anchor_title: anchor_title,
            }
        };
        append_request_message_if_present(&conn, &report.session_id, request)?;
        return Ok(report);
    }

    let now = unix_timestamp();
    let unique = unix_timestamp_nanos();
    let session_id = format!("session-{}", unique);
    let title = format!("{}-{}", request.command_name, unique);
    {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
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
            &tx,
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
        tx.commit()?;
    }
    append_request_message_if_present(&conn, &session_id, request)?;

    Ok(SessionRuntimeReport {
        session_id,
        action: SessionAction::Created,
        interaction_mode,
        title,
        continue_target: None,
        continue_resolution: None,
        continue_anchor_session_id: None,
        continue_anchor_title: None,
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
    if summary.graphemes(true).count() > SESSION_COMPRESSION_CHAR_LIMIT {
        anyhow::bail!(
            "compression summary exceeds {} characters",
            SESSION_COMPRESSION_CHAR_LIMIT
        );
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
    if latest_compression_summary_for_connection(&tx, &session.id)?
        .as_deref()
        .is_some_and(|existing| existing.trim() == summary)
    {
        anyhow::bail!("compression summary matches the latest persisted summary");
    }
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
    latest_compression_summary_for_connection(&conn, session_id)
}

fn latest_compression_summary_for_connection(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT summary FROM session_compressions WHERE session_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?)
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
            continue_target: None,
            continue_resolution: None,
            continue_anchor_session_id: None,
            continue_anchor_title: None,
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
        continue_target: None,
        continue_resolution: None,
        continue_anchor_session_id: None,
        continue_anchor_title: None,
    })
}
