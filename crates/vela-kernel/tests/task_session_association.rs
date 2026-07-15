use std::{
    sync::{Arc, Barrier},
    thread,
};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    session::{SessionClosure, SessionId, SessionReopenReason, SessionStore, SessionTitle},
    task::{TaskGoal, TaskId, TaskOutput, TaskStatus, TaskStore, TaskStoreError},
};

fn event_count(path: &std::path::Path, task_id: &TaskId) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE stream_id = ?1",
            [format!("task:{task_id}")],
            |row| row.get(0),
        )
        .unwrap()
}

fn create_task_and_session(path: &std::path::Path, task_id: &TaskId, session_id: &SessionId) {
    TaskStore::open(path)
        .unwrap()
        .start(
            task_id.clone(),
            TaskGoal::new("Connect persisted work").unwrap(),
        )
        .unwrap();
    SessionStore::open(path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Kernel work").unwrap(),
        )
        .unwrap();
}

#[test]
fn associates_a_task_with_an_open_session_and_reloads_the_association() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-42").unwrap();
    let session_id = SessionId::new("session-7").unwrap();
    create_task_and_session(&path, &task_id, &session_id);

    let associated = TaskStore::open(&path)
        .unwrap()
        .associate_session(&task_id, &session_id)
        .unwrap();

    assert_eq!(associated.session_id(), Some(&session_id));
    assert_eq!(event_count(&path, &task_id), 2);
    let (event_type, payload_version, payload): (String, u32, Vec<u8>) =
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'task:task-42' AND stream_version = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(event_type, "task.session_associated");
    assert_eq!(payload_version, 1);
    assert_eq!(
        payload,
        br#"{"task_id":"task-42","session_id":"session-7"}"#
    );

    let loaded = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.session_id(), Some(&session_id));
}

#[test]
fn rejects_unknown_task_without_appending_an_event() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("missing-task").unwrap();
    let session_id = SessionId::new("session-7").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Kernel work").unwrap(),
        )
        .unwrap();

    let error = TaskStore::open(&path)
        .unwrap()
        .associate_session(&task_id, &session_id)
        .unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::NotFound { task_id: ref id } if id == &task_id
    ));
    assert_eq!(event_count(&path, &task_id), 0);
}

#[test]
fn rejects_unknown_session_without_appending_an_event() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-42").unwrap();
    let session_id = SessionId::new("missing-session").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("Connect work").unwrap())
        .unwrap();

    let error = TaskStore::open(&path)
        .unwrap()
        .associate_session(&task_id, &session_id)
        .unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::SessionNotFound { session_id: ref id } if id == &session_id
    ));
    assert_eq!(event_count(&path, &task_id), 1);
}

#[test]
fn rejects_a_closed_session_without_appending_an_event() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-42").unwrap();
    let session_id = SessionId::new("session-7").unwrap();
    create_task_and_session(&path, &task_id, &session_id);
    SessionStore::open(&path)
        .unwrap()
        .close(&session_id, SessionClosure::new("work paused").unwrap())
        .unwrap();

    let error = TaskStore::open(&path)
        .unwrap()
        .associate_session(&task_id, &session_id)
        .unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::SessionClosed { session_id: ref id } if id == &session_id
    ));
    assert_eq!(event_count(&path, &task_id), 1);
}

#[test]
fn rejects_reassociation_to_the_same_or_another_session() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-42").unwrap();
    let first = SessionId::new("session-1").unwrap();
    let second = SessionId::new("session-2").unwrap();
    create_task_and_session(&path, &task_id, &first);
    SessionStore::open(&path)
        .unwrap()
        .create(second.clone(), SessionTitle::new("Other work").unwrap())
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks.associate_session(&task_id, &first).unwrap();

    for requested in [&first, &second] {
        let error = tasks.associate_session(&task_id, requested).unwrap_err();
        assert!(matches!(
            error,
            TaskStoreError::AlreadyAssociated {
                task_id: ref id,
                ref session_id,
            } if id == &task_id && session_id == &first
        ));
    }
    assert_eq!(event_count(&path, &task_id), 2);
}

#[test]
fn racing_associations_persist_exactly_one_session() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("racing-task").unwrap();
    let first = SessionId::new("session-1").unwrap();
    let second = SessionId::new("session-2").unwrap();
    create_task_and_session(&path, &task_id, &first);
    SessionStore::open(&path)
        .unwrap()
        .create(second.clone(), SessionTitle::new("Other work").unwrap())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let count_path = path.clone();

    let first_path = path.clone();
    let first_task = task_id.clone();
    let first_session = first.clone();
    let first_barrier = Arc::clone(&barrier);
    let first_result = thread::spawn(move || {
        let mut tasks = TaskStore::open(first_path).unwrap();
        first_barrier.wait();
        tasks.associate_session(&first_task, &first_session)
    });
    let second_task = task_id.clone();
    let second_session = second.clone();
    let second_result = thread::spawn(move || {
        let mut tasks = TaskStore::open(path).unwrap();
        barrier.wait();
        tasks.associate_session(&second_task, &second_session)
    });

    match (first_result.join().unwrap(), second_result.join().unwrap()) {
        (Ok(task), Err(TaskStoreError::AlreadyAssociated { session_id, .. }))
        | (Err(TaskStoreError::AlreadyAssociated { session_id, .. }), Ok(task)) => {
            assert_eq!(task.session_id(), Some(&session_id));
        }
        outcomes => panic!("unexpected association race outcomes: {outcomes:?}"),
    }
    assert_eq!(event_count(&count_path, &task_id), 2);
}

#[test]
fn existing_task_history_loads_without_a_session_association() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("legacy-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("Existing history").unwrap())
        .unwrap();

    let loaded = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();

    assert_eq!(loaded.session_id(), None);
}

#[test]
fn association_and_terminal_lifecycle_preserve_each_other_in_either_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let session_id = SessionId::new("session-7").unwrap();
    let associated_first = TaskId::new("associated-first").unwrap();
    create_task_and_session(&path, &associated_first, &session_id);

    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .associate_session(&associated_first, &session_id)
        .unwrap();
    let completed = tasks
        .complete(
            &associated_first,
            TaskOutput::new("association retained").unwrap(),
        )
        .unwrap();
    assert_eq!(completed.status(), TaskStatus::Completed);
    assert_eq!(completed.session_id(), Some(&session_id));

    let terminal_first = TaskId::new("terminal-first").unwrap();
    tasks
        .start(
            terminal_first.clone(),
            TaskGoal::new("Associate completed work").unwrap(),
        )
        .unwrap();
    tasks
        .complete(
            &terminal_first,
            TaskOutput::new("already completed").unwrap(),
        )
        .unwrap();
    let associated = tasks
        .associate_session(&terminal_first, &session_id)
        .unwrap();
    assert_eq!(associated.status(), TaskStatus::Completed);
    assert_eq!(associated.session_id(), Some(&session_id));
}

#[test]
fn rejects_invalid_persisted_association_payloads_and_task_mismatches() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-42").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("Validate history").unwrap())
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('task:task-42', 2, 'task.session_associated', 1, ?1)",
            [br#"{"task_id":"task-42","session_id":""}"#.as_slice()],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&task_id).unwrap_err();
    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:task-42' AND stream_version = 2",
            [br#"{"task_id":"another-task","session_id":"session-7"}"#.as_slice()],
        )
        .unwrap();
    let error = TaskStore::open(&path).unwrap().load(&task_id).unwrap_err();
    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 2 }
    ));
}

#[test]
fn session_close_and_reopen_leave_task_association_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-42").unwrap();
    let session_id = SessionId::new("session-7").unwrap();
    create_task_and_session(&path, &task_id, &session_id);
    TaskStore::open(&path)
        .unwrap()
        .associate_session(&task_id, &session_id)
        .unwrap();

    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .close(
            &session_id,
            SessionClosure::new("checkpoint reached").unwrap(),
        )
        .unwrap();
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .session_id(),
        Some(&session_id)
    );
    sessions
        .reopen(
            &session_id,
            SessionReopenReason::new("continue work").unwrap(),
        )
        .unwrap();
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .session_id(),
        Some(&session_id)
    );
}
