use std::{
    sync::{Arc, Barrier},
    thread,
};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    session::{
        SessionClosure, SessionId, SessionReopenReason, SessionStatus, SessionStore,
        SessionStoreError, SessionTitle, SessionTurnContent, SessionTurnContentError,
        SessionTurnRole,
    },
};

#[test]
fn persists_ordered_conversation_turns_across_close_reopen_and_database_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("conversation").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .create(id.clone(), SessionTitle::new("Conversation").unwrap())
        .unwrap();

    let human = SessionTurnContent::new(" Human asks. ").unwrap();
    let assistant = SessionTurnContent::new(" Assistant answers. ").unwrap();
    let session = store
        .append_turn(&id, SessionTurnRole::Human, human.clone())
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    let session = store
        .append_turn(&id, SessionTurnRole::Assistant, assistant.clone())
        .unwrap();
    assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
    assert_eq!(session.turns()[0].content(), &human);
    assert_eq!(session.turns()[1].role(), SessionTurnRole::Assistant);
    assert_eq!(session.turns()[1].content(), &assistant);

    store
        .close(&id, SessionClosure::new("Pause").unwrap())
        .unwrap();
    store
        .reopen(&id, SessionReopenReason::new("Continue").unwrap())
        .unwrap();
    let latest = SessionTurnContent::new("After reopening").unwrap();
    let session = store
        .append_turn(&id, SessionTurnRole::Human, latest.clone())
        .unwrap();
    assert_eq!(session.status(), SessionStatus::Open);
    assert_eq!(session.turns().len(), 3);
    assert_eq!(session.turns()[2].content(), &latest);
    drop(store);

    let reopened_store = SessionStore::open(&path).unwrap();
    assert_eq!(reopened_store.load(&id).unwrap(), Some(session.clone()));
    assert_eq!(reopened_store.list().unwrap(), vec![session]);
    let persisted: (String, u32, Vec<u8>) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'session:conversation' AND stream_version = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, "session.turn_appended");
    assert_eq!(persisted.1, 1);
    assert_eq!(
        persisted.2,
        br#"{"role":"human","content":" Human asks. "}"#
    );
}

#[test]
fn rejects_empty_content_missing_sessions_and_closed_sessions_without_writing() {
    assert_eq!(
        SessionTurnContent::new("").unwrap_err().to_string(),
        "session turn content must not be empty"
    );
    let _: SessionTurnContentError = SessionTurnContent::new("").unwrap_err();

    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("closed").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    let missing = store
        .append_turn(
            &id,
            SessionTurnRole::Human,
            SessionTurnContent::new("Hello").unwrap(),
        )
        .unwrap_err();
    assert!(matches!(
        missing,
        SessionStoreError::NotFound { ref session_id } if session_id == &id
    ));
    assert_eq!(store.load(&id).unwrap(), None);

    store
        .create(id.clone(), SessionTitle::new("Closed").unwrap())
        .unwrap();
    store
        .close(&id, SessionClosure::new("Done").unwrap())
        .unwrap();
    let before: u32 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    let error = store
        .append_turn(
            &id,
            SessionTurnRole::Assistant,
            SessionTurnContent::new("Too late").unwrap(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::SessionClosed { ref session_id } if session_id == &id
    ));
    assert_eq!(
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row::<u32, _, _>("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap(),
        before
    );
}

#[test]
fn concurrent_turn_appends_both_persist_in_one_valid_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("race-turns").unwrap();
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
        store.append_turn(
            &first_id,
            SessionTurnRole::Human,
            SessionTurnContent::new("First").unwrap(),
        )
    });
    let second_path = path.clone();
    let second_id = id.clone();
    let second = thread::spawn(move || {
        let mut store = SessionStore::open(second_path).unwrap();
        barrier.wait();
        store.append_turn(
            &second_id,
            SessionTurnRole::Assistant,
            SessionTurnContent::new("Second").unwrap(),
        )
    });

    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    let loaded = SessionStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.turns().len(), 2);
    assert!(
        loaded
            .turns()
            .iter()
            .any(|turn| turn.content().as_str() == "First")
    );
    assert!(
        loaded
            .turns()
            .iter()
            .any(|turn| turn.content().as_str() == "Second")
    );
}

#[test]
fn rejects_malformed_turn_payloads_versions_and_invalid_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = SessionId::new("corrupt-turn").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Corrupt").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('session:corrupt-turn', 2, 'session.turn_appended', 1, X'7B22726F6C65223A22746F6F6C222C22636F6E74656E74223A2248656C6C6F227D')",
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
            "UPDATE events SET payload = X'7B22726F6C65223A2268756D616E222C22636F6E74656E74223A22227D' WHERE stream_id = 'session:corrupt-turn' AND stream_version = 2",
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
            "UPDATE events SET payload_version = 2 WHERE stream_id = 'session:corrupt-turn' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path).unwrap().load(&id).unwrap_err(),
        SessionStoreError::Replay(ReplayError::UnsupportedEvent { ref event_type, payload_version: 2 })
            if event_type == "session.turn_appended"
    ));

    connection
        .execute(
            "INSERT INTO events VALUES ('session:turn-before-create', 1, 'session.turn_appended', 1, X'7B22726F6C65223A2268756D616E222C22636F6E74656E74223A2248656C6C6F227D')",
            [],
        )
        .unwrap();
    assert!(matches!(
        SessionStore::open(&path)
            .unwrap()
            .load(&SessionId::new("turn-before-create").unwrap())
            .unwrap_err(),
        SessionStoreError::InvalidHistory { event_count: 1 }
    ));
}
