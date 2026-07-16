use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    task::{
        TaskFailure, TaskGoal, TaskId, TaskObservationId, TaskObservationIdError,
        TaskObservationKind, TaskObservationText, TaskObservationTextError, TaskOutput, TaskStore,
        TaskStoreError,
    },
};

fn observation_id(value: &str) -> TaskObservationId {
    TaskObservationId::new(value).unwrap()
}

fn observation_text(value: &str) -> TaskObservationText {
    TaskObservationText::new(value).unwrap()
}

#[test]
fn persists_every_observation_kind_in_append_order_across_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("observed").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(task_id.clone(), TaskGoal::new("Build safely").unwrap())
        .unwrap();

    let expected = [
        (
            TaskObservationKind::Attempt,
            "attempt-1",
            "Tried the focused test",
        ),
        (
            TaskObservationKind::Diagnostic,
            "diagnostic-1",
            "The assertion failed",
        ),
        (
            TaskObservationKind::Correction,
            "correction-1",
            "Fixed the projection",
        ),
        (
            TaskObservationKind::Verification,
            "verification-1",
            "The quality gate passed",
        ),
    ];
    for (kind, id, text) in expected {
        store
            .append_observation(&task_id, observation_id(id), kind, observation_text(text))
            .unwrap();
    }
    drop(store);

    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), expected.len());
    for (observation, (kind, id, text)) in task.observations().iter().zip(expected) {
        assert_eq!(observation.id().as_str(), id);
        assert_eq!(observation.kind(), kind);
        assert_eq!(observation.text().as_str(), text);
    }
    let persisted: (String, u32, Vec<u8>) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT event_type, payload_version, payload FROM events WHERE stream_id = 'task:observed' AND stream_version = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, "task.observation_appended");
    assert_eq!(persisted.1, 1);
    assert_eq!(
        persisted.2,
        br#"{"id":"attempt-1","kind":"attempt","text":"Tried the focused test"}"#
    );
}

#[test]
fn rejects_empty_or_whitespace_only_observation_values() {
    for invalid in ["", " ", "\n\t"] {
        let id_error: TaskObservationIdError = TaskObservationId::new(invalid).unwrap_err();
        assert_eq!(
            id_error.to_string(),
            "task observation id must not be blank"
        );
        let text_error: TaskObservationTextError = TaskObservationText::new(invalid).unwrap_err();
        assert_eq!(
            text_error.to_string(),
            "task observation text must not be blank"
        );
    }
    assert_eq!(observation_id(" id ").as_str(), " id ");
    assert_eq!(observation_text(" evidence ").as_str(), " evidence ");
}

#[test]
fn rejects_duplicate_observation_ids_without_appending() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("duplicate").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(task_id.clone(), TaskGoal::new("Keep IDs unique").unwrap())
        .unwrap();
    store
        .append_observation(
            &task_id,
            observation_id("same"),
            TaskObservationKind::Attempt,
            observation_text("First meaning"),
        )
        .unwrap();

    let error = store
        .append_observation(
            &task_id,
            observation_id("same"),
            TaskObservationKind::Correction,
            observation_text("Second meaning"),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        TaskStoreError::DuplicateObservation { task_id: ref actual_task, ref observation_id }
            if actual_task == &task_id && observation_id.as_str() == "same"
    ));
    let task = store.load(&task_id).unwrap().unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].kind(), TaskObservationKind::Attempt);
}

#[test]
fn rejects_unknown_and_terminal_tasks_without_changing_streams() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let missing_id = TaskId::new("missing").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    assert!(matches!(
        store
            .append_observation(
                &missing_id,
                observation_id("missing-observation"),
                TaskObservationKind::Diagnostic,
                observation_text("No task"),
            )
            .unwrap_err(),
        TaskStoreError::NotFound { ref task_id } if task_id == &missing_id
    ));
    assert_eq!(store.load(&missing_id).unwrap(), None);

    let completed_id = TaskId::new("completed").unwrap();
    store
        .start(completed_id.clone(), TaskGoal::new("Complete").unwrap())
        .unwrap();
    store
        .complete(&completed_id, TaskOutput::new("Done").unwrap())
        .unwrap();
    let failed_id = TaskId::new("failed").unwrap();
    store
        .start(failed_id.clone(), TaskGoal::new("Fail").unwrap())
        .unwrap();
    store
        .fail(&failed_id, TaskFailure::new("Failed").unwrap())
        .unwrap();
    let before: u64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();

    assert!(matches!(
        store
            .append_observation(
                &completed_id,
                observation_id("late-completed"),
                TaskObservationKind::Verification,
                observation_text("Too late"),
            )
            .unwrap_err(),
        TaskStoreError::AlreadyCompleted { ref task_id } if task_id == &completed_id
    ));
    assert!(matches!(
        store
            .append_observation(
                &failed_id,
                observation_id("late-failed"),
                TaskObservationKind::Diagnostic,
                observation_text("Too late"),
            )
            .unwrap_err(),
        TaskStoreError::AlreadyFailed { ref task_id } if task_id == &failed_id
    ));
    assert_eq!(
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row::<u64, _, _>("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap(),
        before
    );
}

#[test]
fn malformed_observations_and_unsupported_kinds_are_replay_errors() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("corrupt-observation").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(task_id.clone(), TaskGoal::new("Replay strictly").unwrap())
        .unwrap();
    store
        .append_observation(
            &task_id,
            observation_id("evidence"),
            TaskObservationKind::Attempt,
            observation_text("Try"),
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:corrupt-observation' AND stream_version = 2",
            [br#"{"id":"evidence","kind":"unknown","text":"Try"}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap_err(),
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:corrupt-observation' AND stream_version = 2",
            [br#"{"id":"evidence","kind":"attempt","text":" "}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap_err(),
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = 'task:corrupt-observation' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap_err(),
        TaskStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 2,
        }) if event_type == "task.observation_appended"
    ));
}

#[test]
fn duplicate_observation_ids_in_persisted_history_are_invalid() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("duplicate-history").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(task_id.clone(), TaskGoal::new("Replay uniquely").unwrap())
        .unwrap();
    store
        .append_observation(
            &task_id,
            observation_id("same"),
            TaskObservationKind::Attempt,
            observation_text("First"),
        )
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:duplicate-history', 3, 'task.observation_appended', 1, ?1)",
            [br#"{"id":"same","kind":"correction","text":"Second"}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap_err(),
        TaskStoreError::InvalidHistory { event_count: 3 }
    ));
}

#[test]
fn existing_task_history_without_observations_replays_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("legacy-shape").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    let started = store
        .start(task_id.clone(), TaskGoal::new("No observations").unwrap())
        .unwrap();
    assert!(started.observations().is_empty());
    store
        .complete(&task_id, TaskOutput::new("Done").unwrap())
        .unwrap();

    let loaded = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(loaded.observations().is_empty());
    assert_eq!(loaded.output().unwrap().as_str(), "Done");
}
