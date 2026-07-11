use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod surface;

#[cfg(test)]
mod tests;

pub use surface::*;

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
            runtime_state TEXT NOT NULL DEFAULT 'ready',
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
    ensure_sessions_runtime_state_column(conn)?;
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

fn ensure_sessions_runtime_state_column(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == "runtime_state" {
            return Ok(());
        }
    }
    conn.execute(
        "ALTER TABLE sessions ADD COLUMN runtime_state TEXT NOT NULL DEFAULT 'ready'",
        [],
    )?;
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

    let runtime_state = load_session_runtime_state(conn, session_id)?;

    Ok(SessionInspection {
        session_id: session_id.to_string(),
        title: title.to_string(),
        runtime_state,
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
    let runtime_state = load_session_runtime_state(conn, session_id)?;
    Ok(SessionSummary {
        id: session_id.to_string(),
        title: title.to_string(),
        runtime_state,
        message_count,
        event_count,
        parent_session_id,
    })
}

fn load_session_runtime_state(conn: &Connection, session_id: &str) -> Result<String> {
    Ok(conn
        .query_row(
            "SELECT runtime_state FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_else(|| SessionRuntimeState::Ready.label().to_string()))
}

pub(crate) fn update_session_runtime_state(
    conn: &Connection,
    session_id: &str,
    runtime_state: SessionRuntimeState,
) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET runtime_state = ?2, updated_at = ?3 WHERE id = ?1",
        params![session_id, runtime_state.label(), unix_timestamp()],
    )?;
    Ok(())
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
        "SELECT id, title, parent_session_id, created_at FROM sessions ORDER BY created_at DESC, id DESC",
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
    let mut parents_by_id = HashMap::new();
    for row in rows {
        let (id, title, parent_session_id, created_at) = row?;
        parents_by_id.insert(id.clone(), parent_session_id.clone());
        sessions.push((id, title, parent_session_id, created_at));
    }
    for (id, title, _parent_session_id, _created_at) in &sessions {
        let mut cursor = Some(id.as_str());
        while let Some(current) = cursor {
            if current == anchor_session_id {
                return Ok(Some(StoredSession {
                    id: id.clone(),
                    title: title.clone(),
                }));
            }
            cursor = parents_by_id
                .get(current)
                .and_then(|parent| parent.as_deref());
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

fn find_session_by_id(conn: &Connection, session_id: &str) -> Result<Option<StoredSession>> {
    Ok(conn
        .query_row(
            "SELECT id, title FROM sessions WHERE id = ?1 LIMIT 1",
            params![session_id],
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
    if let Some(session) = find_session_by_id(conn, value)? {
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
