use std::{
    error::Error,
    sync::{Arc, Barrier},
    thread,
};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    session::{
        SessionId, SessionIdError, SessionStatus, SessionStore, SessionStoreError, SessionTitle,
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
    let _: SessionIdError = SessionId::new("").unwrap_err();
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
            "UPDATE events SET event_type = 'session.renamed' WHERE stream_id = 'session:session-42'",
            [],
        )
        .unwrap();

    let error = SessionStore::open(&path).unwrap().load(&id).unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::Replay(ReplayError::UnsupportedEvent { ref event_type, payload_version: 1 })
            if event_type == "session.renamed"
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
