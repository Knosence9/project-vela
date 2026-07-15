use std::{
    error::Error,
    sync::{Arc, Barrier},
    thread,
};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    session::{
        SessionClosure, SessionClosureError, SessionId, SessionIdError, SessionReopenReason,
        SessionReopenReasonError, SessionStatus, SessionStore, SessionStoreError, SessionSummary,
        SessionSummaryError, SessionTitle,
    },
};

#[test]
fn creates_and_loads_an_open_session_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("planning:vela").unwrap();
    let title = SessionTitle::new(" Plan the next kernel slice ").unwrap();

    let session = SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), title.clone())
        .unwrap();

    assert_eq!(session.id(), &id);
    assert_eq!(session.title(), &title);
    assert_eq!(session.status(), SessionStatus::Open);
    assert_eq!(session.reopen_reason(), None);
    assert_eq!(
        SessionStore::open(&path).unwrap().load(&id).unwrap(),
        Some(session)
    );
}

#[test]
fn persists_the_title_in_the_typed_event_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let title = SessionTitle::new(" Planning session ").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(SessionId::new("payload").unwrap(), title.clone())
        .unwrap();

    let (event_type, payload_version, payload): (String, u32, Vec<u8>) =
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'session:payload' AND stream_version = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

    assert_eq!(event_type, "session.created");
    assert_eq!(payload_version, 1);
    assert_eq!(payload, br#"{"title":" Planning session "}"#);
    assert_eq!(title.as_str(), " Planning session ");
}

#[test]
fn renames_and_loads_a_session_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("rename").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .create(id.clone(), SessionTitle::new("Original").unwrap())
        .unwrap();

    let title = SessionTitle::new(" Renamed session ").unwrap();
    let session = store.rename(&id, title.clone()).unwrap();

    assert_eq!(session.title(), &title);
    assert_eq!(session.status(), SessionStatus::Open);
    assert_eq!(session.closure(), None);
    assert_eq!(session.reopen_reason(), None);
    assert_eq!(
        SessionStore::open(&path).unwrap().load(&id).unwrap(),
        Some(session)
    );
    let (event_type, payload_version, payload): (String, u32, Vec<u8>) =
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'session:rename' AND stream_version = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(event_type, "session.renamed");
    assert_eq!(payload_version, 1);
    assert_eq!(payload, br#"{"title":" Renamed session "}"#);
}

#[test]
fn repeated_renames_preserve_lifecycle_state_and_project_the_latest_title() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("rename-transitions").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .create(id.clone(), SessionTitle::new("Original").unwrap())
        .unwrap();
    let closure = SessionClosure::new("Complete").unwrap();
    store.close(&id, closure.clone()).unwrap();

    let closed = store
        .rename(&id, SessionTitle::new("Closed title").unwrap())
        .unwrap();
    assert_eq!(closed.status(), SessionStatus::Closed);
    assert_eq!(closed.closure(), Some(&closure));

    let reason = SessionReopenReason::new("Continue").unwrap();
    store.reopen(&id, reason.clone()).unwrap();
    let title = SessionTitle::new("Latest title").unwrap();
    let open = store.rename(&id, title.clone()).unwrap();

    assert_eq!(open.title(), &title);
    assert_eq!(open.status(), SessionStatus::Open);
    assert_eq!(open.closure(), None);
    assert_eq!(open.reopen_reason(), Some(&reason));
    assert_eq!(store.load(&id).unwrap(), Some(open));
}

#[test]
fn renaming_a_missing_session_returns_not_found_without_creating_a_stream() {
    let directory = tempdir().unwrap();
    let mut store = SessionStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = SessionId::new("missing-rename").unwrap();

    let error = store
        .rename(&id, SessionTitle::new("Title").unwrap())
        .unwrap_err();

    assert!(matches!(
        error,
        SessionStoreError::NotFound { ref session_id } if session_id == &id
    ));
    assert_eq!(store.load(&id).unwrap(), None);
}

#[test]
fn summarizes_and_loads_the_latest_summary_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("summary").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .create(id.clone(), SessionTitle::new("Investigate").unwrap())
        .unwrap();
    let closure = SessionClosure::new("Paused").unwrap();
    store.close(&id, closure.clone()).unwrap();

    let first = SessionSummary::new(" First finding ").unwrap();
    let summarized = store.summarize(&id, first).unwrap();
    assert_eq!(summarized.status(), SessionStatus::Closed);
    assert_eq!(summarized.closure(), Some(&closure));

    let latest = SessionSummary::new(" Latest finding ").unwrap();
    let summarized = store.summarize(&id, latest.clone()).unwrap();

    assert_eq!(summarized.summary(), Some(&latest));
    assert_eq!(
        SessionStore::open(&path).unwrap().load(&id).unwrap(),
        Some(summarized)
    );
    let (event_type, payload_version, payload): (String, u32, Vec<u8>) =
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'session:summary' AND stream_version = 4",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(event_type, "session.summarized");
    assert_eq!(payload_version, 1);
    assert_eq!(payload, br#"{"summary":" Latest finding "}"#);
    assert_eq!(latest.as_str(), " Latest finding ");
}

#[test]
fn summary_validation_and_missing_session_are_explicit() {
    assert_eq!(
        SessionSummary::new("").unwrap_err().to_string(),
        "session summary must not be empty"
    );
    let _: SessionSummaryError = SessionSummary::new("").unwrap_err();

    let directory = tempdir().unwrap();
    let mut store = SessionStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = SessionId::new("missing-summary").unwrap();
    let error = store
        .summarize(&id, SessionSummary::new("Finding").unwrap())
        .unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::NotFound { ref session_id } if session_id == &id
    ));
    assert_eq!(store.load(&id).unwrap(), None);
}

#[test]
fn racing_summaries_both_persist_as_valid_updates() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("race-summary").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Race summary").unwrap())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_path = path.clone();
    let first_id = id.clone();
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut store = SessionStore::open(first_path).unwrap();
        first_barrier.wait();
        store.summarize(&first_id, SessionSummary::new("First").unwrap())
    });
    let second_path = path.clone();
    let second_id = id.clone();
    let second = thread::spawn(move || {
        let mut store = SessionStore::open(second_path).unwrap();
        barrier.wait();
        store.summarize(&second_id, SessionSummary::new("Second").unwrap())
    });

    let first = first.join().unwrap().unwrap();
    let second = second.join().unwrap().unwrap();
    assert!(matches!(
        SessionStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap()
            .unwrap()
            .summary(),
        Some(summary) if summary == first.summary().unwrap() || summary == second.summary().unwrap()
    ));
    let summary_count: u32 = rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE stream_id = 'session:race-summary' AND event_type = 'session.summarized'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(summary_count, 2);
}

#[test]
fn load_rejects_invalid_summary_events_and_summary_before_creation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("invalid-summary").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Original").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-summary', 2, 'session.summarized', 1, X'7B2273756D6D617279223A22227D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = 'session:invalid-summary' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 2,
        }) if event_type == "session.summarized"
    ));

    connection
        .execute(
            "INSERT INTO events VALUES ('session:summary-before-create', 1, 'session.summarized', 1, X'7B2273756D6D617279223A2246696E64696E67227D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path)
            .unwrap()
            .load(&SessionId::new("summary-before-create").unwrap())
            .unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 1 }
    ));
}

#[test]
fn loading_an_unknown_session_returns_none() {
    let directory = tempdir().unwrap();
    let store = SessionStore::open(directory.path().join("events.sqlite3")).unwrap();

    assert_eq!(
        store.load(&SessionId::new("missing").unwrap()).unwrap(),
        None
    );
}

#[test]
fn rejects_empty_session_values() {
    assert_eq!(
        SessionId::new("").unwrap_err().to_string(),
        "session id must not be empty"
    );
    assert_eq!(
        SessionTitle::new("").unwrap_err().to_string(),
        "session title must not be empty"
    );
    assert_eq!(
        SessionClosure::new("").unwrap_err().to_string(),
        "session closure reason must not be empty"
    );
    assert_eq!(
        SessionReopenReason::new("").unwrap_err().to_string(),
        "session reopen reason must not be empty"
    );
    let _: SessionIdError = SessionId::new("").unwrap_err();
    let _: SessionClosureError = SessionClosure::new("").unwrap_err();
    let _: SessionReopenReasonError = SessionReopenReason::new("").unwrap_err();
}

#[test]
fn session_and_task_streams_with_the_same_external_id_are_isolated() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("shared-id").unwrap();
    let session = SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Session").unwrap())
        .unwrap();
    vela_kernel::task::TaskStore::open(&path)
        .unwrap()
        .start(
            vela_kernel::task::TaskId::new("shared-id").unwrap(),
            vela_kernel::task::TaskGoal::new("Task").unwrap(),
        )
        .unwrap();

    assert_eq!(
        SessionStore::open(&path).unwrap().load(&id).unwrap(),
        Some(session)
    );
}

#[test]
fn rejects_a_duplicate_create_and_preserves_the_original_session() {
    let directory = tempdir().unwrap();
    let mut store = SessionStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = SessionId::new("session-42").unwrap();
    let original = store
        .create(id.clone(), SessionTitle::new("Original").unwrap())
        .unwrap();

    let error = store
        .create(id.clone(), SessionTitle::new("Replacement").unwrap())
        .unwrap_err();

    assert!(matches!(
        error,
        SessionStoreError::AlreadyExists { ref session_id } if session_id == &id
    ));
    assert_eq!(error.to_string(), "session session-42 already exists");
    assert!(error.source().is_none());
    assert_eq!(store.load(&id).unwrap(), Some(original));
}

#[test]
fn racing_creates_persist_exactly_one_session() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    SessionStore::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_path = path.clone();
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut store = SessionStore::open(first_path).unwrap();
        first_barrier.wait();
        store.create(
            SessionId::new("race").unwrap(),
            SessionTitle::new("First").unwrap(),
        )
    });
    let second = thread::spawn(move || {
        let mut store = SessionStore::open(path).unwrap();
        barrier.wait();
        store.create(
            SessionId::new("race").unwrap(),
            SessionTitle::new("Second").unwrap(),
        )
    });

    match (first.join().unwrap(), second.join().unwrap()) {
        (Ok(winner), Err(SessionStoreError::AlreadyExists { .. }))
        | (Err(SessionStoreError::AlreadyExists { .. }), Ok(winner)) => {
            assert_eq!(winner.status(), SessionStatus::Open);
        }
        outcomes => panic!("unexpected create race outcomes: {outcomes:?}"),
    }
}

#[test]
fn load_surfaces_unknown_events_and_invalid_histories() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("session-42").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Review").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET event_type = 'session.archived' WHERE stream_id = 'session:session-42'",
            [],
        )
        .unwrap();

    let error = SessionStore::open(&path).unwrap().load(&id).unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::Replay(ReplayError::UnsupportedEvent { ref event_type, payload_version: 1 })
            if event_type == "session.archived"
    ));
    assert!(error.source().is_some());

    connection
        .execute(
            "UPDATE events SET event_type = 'session.created' WHERE stream_id = 'session:session-42'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:session-42', 2, 'session.created', 1, X'7B227469746C65223A22416761696E227D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 2 }
    ));
}

#[test]
fn load_surfaces_unknown_versions_and_malformed_payloads() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("session-42").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Review").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = 'session:session-42'",
            [],
        )
        .unwrap();

    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 2,
        }) if event_type == "session.created"
    ));

    connection
        .execute(
            "UPDATE events SET payload_version = 1, payload = X'7B227469746C65223A22227D' WHERE stream_id = 'session:session-42'",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
}

#[test]
fn load_rejects_invalid_rename_events() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("invalid-rename").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Original").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-rename', 2, 'session.renamed', 1, X'7B227469746C65223A22227D')",
            [],
        )
        .unwrap();

    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = 'session:invalid-rename' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 2,
        }) if event_type == "session.renamed"
    ));

    let before_create = SessionId::new("rename-before-create").unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:rename-before-create', 1, 'session.renamed', 1, X'7B227469746C65223A2252656E616D6564227D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path)
            .unwrap()
            .load(&before_create)
            .unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 1 }
    ));
}

#[test]
fn closes_and_loads_a_closed_session_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("review").unwrap();
    let title = SessionTitle::new("Review the implementation").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), title.clone())
        .unwrap();

    let closure = SessionClosure::new(" Review completed ").unwrap();
    let session = SessionStore::open(&path)
        .unwrap()
        .close(&id, closure.clone())
        .unwrap();

    assert_eq!(session.id(), &id);
    assert_eq!(session.title(), &title);
    assert_eq!(session.status(), SessionStatus::Closed);
    assert_eq!(session.closure(), Some(&closure));
    assert_eq!(session.reopen_reason(), None);
    assert_eq!(
        SessionStore::open(&path).unwrap().load(&id).unwrap(),
        Some(session)
    );

    let (event_type, payload_version, payload): (String, u32, Vec<u8>) =
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'session:review' AND stream_version = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(event_type, "session.closed");
    assert_eq!(payload_version, 2);
    assert_eq!(payload, br#"{"reason":" Review completed "}"#);
}

#[test]
fn closing_missing_and_closed_sessions_returns_domain_errors() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = SessionStore::open(&path).unwrap();
    let missing = SessionId::new("missing").unwrap();

    let error = store
        .close(&missing, SessionClosure::new("Missing").unwrap())
        .unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::NotFound { ref session_id } if session_id == &missing
    ));
    assert!(error.source().is_none());
    assert_eq!(store.load(&missing).unwrap(), None);

    let id = SessionId::new("closed").unwrap();
    store
        .create(id.clone(), SessionTitle::new("Closed once").unwrap())
        .unwrap();
    let closed = store
        .close(&id, SessionClosure::new("Closed").unwrap())
        .unwrap();
    let error = store
        .close(&id, SessionClosure::new("Closed").unwrap())
        .unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::AlreadyClosed { ref session_id } if session_id == &id
    ));
    assert_eq!(error.to_string(), "session closed is already closed");
    assert!(error.source().is_none());
    assert_eq!(store.load(&id).unwrap(), Some(closed));
}

#[test]
fn racing_closes_persist_exactly_one_terminal_event() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("race-close").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Race").unwrap())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_path = path.clone();
    let first_id = id.clone();
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut store = SessionStore::open(first_path).unwrap();
        first_barrier.wait();
        store.close(&first_id, SessionClosure::new("First").unwrap())
    });
    let second_path = path.clone();
    let second = thread::spawn(move || {
        let mut store = SessionStore::open(second_path).unwrap();
        barrier.wait();
        store.close(&id, SessionClosure::new("Closed").unwrap())
    });

    match (first.join().unwrap(), second.join().unwrap()) {
        (Ok(winner), Err(SessionStoreError::AlreadyClosed { .. }))
        | (Err(SessionStoreError::AlreadyClosed { .. }), Ok(winner)) => {
            assert_eq!(winner.status(), SessionStatus::Closed);
        }
        outcomes => panic!("unexpected close race outcomes: {outcomes:?}"),
    }
    let close_count: u32 = rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE stream_id = 'session:race-close' AND event_type = 'session.closed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(close_count, 1);
}

#[test]
fn reopens_and_loads_an_open_session_after_restarting() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("reopen").unwrap();
    let title = SessionTitle::new("Continue the session").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store.create(id.clone(), title.clone()).unwrap();
    store
        .close(&id, SessionClosure::new("Closed").unwrap())
        .unwrap();

    let reason = SessionReopenReason::new(" Continue with new evidence ").unwrap();
    let session = SessionStore::open(&path)
        .unwrap()
        .reopen(&id, reason.clone())
        .unwrap();

    assert_eq!(session.id(), &id);
    assert_eq!(session.title(), &title);
    assert_eq!(session.status(), SessionStatus::Open);
    assert_eq!(session.closure(), None);
    assert_eq!(session.reopen_reason(), Some(&reason));
    assert_eq!(
        SessionStore::open(&path).unwrap().load(&id).unwrap(),
        Some(session)
    );
    let (event_type, payload_version, payload): (String, u32, Vec<u8>) =
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'session:reopen' AND stream_version = 3",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(event_type, "session.reopened");
    assert_eq!(payload_version, 2);
    assert_eq!(payload, br#"{"reason":" Continue with new evidence "}"#);
}

#[test]
fn reopening_missing_and_open_sessions_returns_domain_errors() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = SessionStore::open(&path).unwrap();
    let missing = SessionId::new("missing-reopen").unwrap();

    assert!(matches!(
        store
            .reopen(&missing, SessionReopenReason::new("Retry").unwrap())
            .unwrap_err(),
        SessionStoreError::NotFound { ref session_id } if session_id == &missing
    ));
    assert_eq!(store.load(&missing).unwrap(), None);

    let id = SessionId::new("already-open").unwrap();
    let open = store
        .create(id.clone(), SessionTitle::new("Still open").unwrap())
        .unwrap();
    let error = store
        .reopen(&id, SessionReopenReason::new("Retry").unwrap())
        .unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::AlreadyOpen { ref session_id } if session_id == &id
    ));
    assert_eq!(error.to_string(), "session already-open is already open");
    assert!(error.source().is_none());
    assert_eq!(store.load(&id).unwrap(), Some(open));
}

#[test]
fn repeated_transitions_replace_the_active_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("repeat").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .create(id.clone(), SessionTitle::new("Repeat transitions").unwrap())
        .unwrap();
    store
        .close(&id, SessionClosure::new("First close").unwrap())
        .unwrap();
    store
        .reopen(&id, SessionReopenReason::new("More work").unwrap())
        .unwrap();

    let closure = SessionClosure::new("Second close").unwrap();
    let session = store.close(&id, closure.clone()).unwrap();

    assert_eq!(session.status(), SessionStatus::Closed);
    assert_eq!(session.closure(), Some(&closure));
    assert_eq!(session.reopen_reason(), None);

    let reason = SessionReopenReason::new("Second investigation").unwrap();
    let session = store.reopen(&id, reason.clone()).unwrap();
    assert_eq!(session.status(), SessionStatus::Open);
    assert_eq!(session.closure(), None);
    assert_eq!(session.reopen_reason(), Some(&reason));
    assert_eq!(store.load(&id).unwrap(), Some(session));
}

#[test]
fn racing_reopens_persist_exactly_one_reopen_event() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("race-reopen").unwrap();
    let mut setup = SessionStore::open(&path).unwrap();
    setup
        .create(id.clone(), SessionTitle::new("Race reopen").unwrap())
        .unwrap();
    setup
        .close(&id, SessionClosure::new("Closed").unwrap())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_path = path.clone();
    let first_id = id.clone();
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut store = SessionStore::open(first_path).unwrap();
        first_barrier.wait();
        store.reopen(&first_id, SessionReopenReason::new("First").unwrap())
    });
    let second_path = path.clone();
    let second = thread::spawn(move || {
        let mut store = SessionStore::open(second_path).unwrap();
        barrier.wait();
        store.reopen(&id, SessionReopenReason::new("Second").unwrap())
    });

    match (first.join().unwrap(), second.join().unwrap()) {
        (Ok(winner), Err(SessionStoreError::AlreadyOpen { .. }))
        | (Err(SessionStoreError::AlreadyOpen { .. }), Ok(winner)) => {
            assert_eq!(winner.status(), SessionStatus::Open);
        }
        outcomes => panic!("unexpected reopen race outcomes: {outcomes:?}"),
    }
    let reopen_count: u32 = rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE stream_id = 'session:race-reopen' AND event_type = 'session.reopened'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reopen_count, 1);
}

#[test]
fn load_rejects_closed_without_creation_and_duplicate_close_events() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("invalid-close").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Invalid history").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET event_type = 'session.closed', payload = X'7B7D' WHERE stream_id = 'session:invalid-close'",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 1 }
    ));

    connection
        .execute(
            "UPDATE events SET event_type = 'session.created', payload = X'7B227469746C65223A22496E76616C696420686973746F7279227D' WHERE stream_id = 'session:invalid-close'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-close', 2, 'session.closed', 1, X'7B22756E6578706563746564223A747275657D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));
    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'session:invalid-close' AND stream_version = 2",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-close', 3, 'session.closed', 1, X'7B7D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 3 }
    ));
}

#[test]
fn load_rejects_reopen_without_close_and_duplicate_reopen_events() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("invalid-reopen").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Invalid reopen").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-reopen', 2, 'session.reopened', 1, X'7B7D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 2 }
    ));

    connection
        .execute(
            "UPDATE events SET event_type = 'session.closed' WHERE stream_id = 'session:invalid-reopen' AND stream_version = 2",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-reopen', 3, 'session.reopened', 1, X'7B22756E6578706563746564223A747275657D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));
    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'session:invalid-reopen' AND stream_version = 3",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-reopen', 4, 'session.reopened', 1, X'7B7D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 4 }
    ));
}

#[test]
fn loads_legacy_close_events_without_a_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("legacy-close").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Legacy").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('session:legacy-close', 2, 'session.closed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let session = SessionStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();

    assert_eq!(session.status(), SessionStatus::Closed);
    assert_eq!(session.closure(), None);
}

#[test]
fn rejects_empty_version_two_close_reasons() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("empty-close").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Invalid close").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('session:empty-close', 2, 'session.closed', 2, X'7B22726561736F6E223A22227D')",
            [],
        )
        .unwrap();

    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload_version = 3 WHERE stream_id = 'session:empty-close' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 3,
        }) if event_type == "session.closed"
    ));
}

#[test]
fn loads_legacy_reopen_events_without_a_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("legacy-reopen").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Legacy reopen").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:legacy-reopen', 2, 'session.closed', 1, X'7B7D')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:legacy-reopen', 3, 'session.reopened', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let session = SessionStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();

    assert_eq!(session.status(), SessionStatus::Open);
    assert_eq!(session.closure(), None);
    assert_eq!(session.reopen_reason(), None);
}

#[test]
fn rejects_malformed_and_unsupported_reopen_reason_payloads() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("invalid-reopen-reason").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            SessionTitle::new("Invalid reopen reason").unwrap(),
        )
        .unwrap();
    store
        .close(&id, SessionClosure::new("Closed").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:invalid-reopen-reason', 3, 'session.reopened', 2, X'7B22726561736F6E223A22227D')",
            [],
        )
        .unwrap();

    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'session:invalid-reopen-reason' AND stream_version = 3",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload_version = 3 WHERE stream_id = 'session:invalid-reopen-reason' AND stream_version = 3",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 3,
        }) if event_type == "session.reopened"
    ));
}
