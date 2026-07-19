use std::{
    sync::{Arc, Barrier},
    thread,
};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    task::{
        TaskCancellation, TaskFailure, TaskGoal, TaskId, TaskObservationId, TaskObservationIdError,
        TaskObservationKind, TaskObservationText, TaskObservationTextError, TaskOutput, TaskStatus,
        TaskStore, TaskStoreError,
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
        assert_eq!(observation.parent_attempt_id(), None);
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
    assert_eq!(persisted.1, 2);
    assert_eq!(
        persisted.2,
        br#"{"id":"attempt-1","kind":"attempt","text":"Tried the focused test","parent_attempt_id":null}"#
    );
}

#[test]
fn relates_non_attempt_observations_to_an_earlier_attempt_across_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("episode").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(
            task_id.clone(),
            TaskGoal::new("Explain one episode").unwrap(),
        )
        .unwrap();
    store
        .append_observation(
            &task_id,
            observation_id("attempt-1"),
            TaskObservationKind::Attempt,
            observation_text("Tried once"),
        )
        .unwrap();

    for (id, kind) in [
        ("diagnostic-1", TaskObservationKind::Diagnostic),
        ("correction-1", TaskObservationKind::Correction),
        ("verification-1", TaskObservationKind::Verification),
    ] {
        store
            .append_observation_for_attempt(
                &task_id,
                observation_id(id),
                kind,
                observation_text(id),
                observation_id("attempt-1"),
            )
            .unwrap();
    }
    drop(store);

    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 4);
    assert_eq!(task.observations()[0].parent_attempt_id(), None);
    for observation in &task.observations()[1..] {
        assert_eq!(
            observation
                .parent_attempt_id()
                .map(TaskObservationId::as_str),
            Some("attempt-1")
        );
    }
    let persisted: (u32, Vec<u8>) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT payload_version, payload FROM events WHERE stream_id = 'task:episode' AND stream_version = 3",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, 2);
    assert_eq!(
        persisted.1,
        br#"{"id":"diagnostic-1","kind":"diagnostic","text":"diagnostic-1","parent_attempt_id":"attempt-1"}"#
    );
}

#[test]
fn rejects_invalid_parent_attempt_relations_without_appending() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("invalid-relations").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(
            task_id.clone(),
            TaskGoal::new("Reject invalid relations").unwrap(),
        )
        .unwrap();
    store
        .append_observation(
            &task_id,
            observation_id("diagnostic-1"),
            TaskObservationKind::Diagnostic,
            observation_text("Ungrouped diagnostic"),
        )
        .unwrap();

    let missing = store
        .append_observation_for_attempt(
            &task_id,
            observation_id("correction-missing"),
            TaskObservationKind::Correction,
            observation_text("No parent"),
            observation_id("missing"),
        )
        .unwrap_err();
    assert!(matches!(
        missing,
        TaskStoreError::ParentObservationNotFound {
            task_id: ref actual_task,
            parent_observation_id: ref parent,
        } if actual_task == &task_id && parent.as_str() == "missing"
    ));

    let non_attempt = store
        .append_observation_for_attempt(
            &task_id,
            observation_id("correction-wrong-kind"),
            TaskObservationKind::Correction,
            observation_text("Wrong parent"),
            observation_id("diagnostic-1"),
        )
        .unwrap_err();
    assert!(matches!(
        non_attempt,
        TaskStoreError::ParentObservationNotAttempt {
            task_id: ref actual_task,
            parent_observation_id: ref parent,
            parent_kind: TaskObservationKind::Diagnostic,
        } if actual_task == &task_id && parent.as_str() == "diagnostic-1"
    ));

    store
        .append_observation(
            &task_id,
            observation_id("attempt-1"),
            TaskObservationKind::Attempt,
            observation_text("Actual attempt"),
        )
        .unwrap();
    let parented_attempt = store
        .append_observation_for_attempt(
            &task_id,
            observation_id("attempt-2"),
            TaskObservationKind::Attempt,
            observation_text("Nested attempt"),
            observation_id("attempt-1"),
        )
        .unwrap_err();
    assert!(matches!(
        parented_attempt,
        TaskStoreError::AttemptCannotHaveParent {
            task_id: ref actual_task,
            observation_id: ref actual_observation,
            parent_observation_id: ref parent,
        } if actual_task == &task_id
            && actual_observation.as_str() == "attempt-2"
            && parent.as_str() == "attempt-1"
    ));

    let event_count: u64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(event_count, 3);
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
fn racing_distinct_observations_both_persist_in_stream_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("distinct-race").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            task_id.clone(),
            TaskGoal::new("Keep both observations").unwrap(),
        )
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_store = TaskStore::open(&path).unwrap();
    let second_store = TaskStore::open(&path).unwrap();
    let other_id = task_id.clone();
    let other_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut store = first_store;
        other_barrier.wait();
        store.append_observation(
            &other_id,
            observation_id("first"),
            TaskObservationKind::Attempt,
            observation_text("First writer"),
        )
    });
    let second = thread::spawn(move || {
        let mut store = second_store;
        barrier.wait();
        store.append_observation(
            &task_id,
            observation_id("second"),
            TaskObservationKind::Diagnostic,
            observation_text("Second writer"),
        )
    });

    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&TaskId::new("distinct-race").unwrap())
        .unwrap()
        .unwrap();
    let ids: Vec<_> = task
        .observations()
        .iter()
        .map(|observation| observation.id().as_str())
        .collect();
    assert!(matches!(
        ids.as_slice(),
        ["first", "second"] | ["second", "first"]
    ));
}

#[test]
fn racing_duplicate_observation_ids_persist_exactly_one() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("duplicate-race").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            task_id.clone(),
            TaskGoal::new("Choose one meaning").unwrap(),
        )
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_store = TaskStore::open(&path).unwrap();
    let second_store = TaskStore::open(&path).unwrap();
    let other_id = task_id.clone();
    let second_id = task_id.clone();
    let other_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        let mut store = first_store;
        other_barrier.wait();
        store.append_observation(
            &other_id,
            observation_id("same"),
            TaskObservationKind::Attempt,
            observation_text("First meaning"),
        )
    });
    let second = thread::spawn(move || {
        let mut store = second_store;
        barrier.wait();
        store.append_observation(
            &second_id,
            observation_id("same"),
            TaskObservationKind::Correction,
            observation_text("Second meaning"),
        )
    });

    match (first.join().unwrap(), second.join().unwrap()) {
        (Ok(task), Err(TaskStoreError::DuplicateObservation { .. }))
        | (Err(TaskStoreError::DuplicateObservation { .. }), Ok(task)) => {
            assert_eq!(task.observations().len(), 1);
        }
        outcomes => panic!("unexpected duplicate observation race outcomes: {outcomes:?}"),
    }
    let persisted = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.observations().len(), 1);
}

#[test]
fn racing_observation_and_completion_never_append_after_terminal() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("terminal-race").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("Freeze evidence").unwrap())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let observation_store = TaskStore::open(&path).unwrap();
    let completion_store = TaskStore::open(&path).unwrap();
    let other_id = task_id.clone();
    let completion_id = task_id.clone();
    let other_barrier = Arc::clone(&barrier);
    let observation = thread::spawn(move || {
        let mut store = observation_store;
        other_barrier.wait();
        store.append_observation(
            &other_id,
            observation_id("racing"),
            TaskObservationKind::Verification,
            observation_text("Racing evidence"),
        )
    });
    let completion = thread::spawn(move || {
        let mut store = completion_store;
        barrier.wait();
        store.complete(&completion_id, TaskOutput::new("Done").unwrap())
    });

    let observation = observation.join().unwrap();
    let completed = completion.join().unwrap().unwrap();
    assert_eq!(completed.status(), TaskStatus::Completed);
    let expected_observation_count = match observation {
        Ok(task) => {
            assert_eq!(task.observations().len(), 1);
            1
        }
        Err(TaskStoreError::AlreadyCompleted { .. }) => {
            assert!(completed.observations().is_empty());
            0
        }
        outcome => panic!("unexpected observation race outcome: {outcome:?}"),
    };
    let persisted = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status(), TaskStatus::Completed);
    assert_eq!(persisted.observations().len(), expected_observation_count);
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
    let cancelled_id = TaskId::new("cancelled").unwrap();
    store
        .start(cancelled_id.clone(), TaskGoal::new("Cancel").unwrap())
        .unwrap();
    store
        .cancel(&cancelled_id, TaskCancellation::new("Cancelled").unwrap())
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
    assert!(matches!(
        store
            .append_observation(
                &cancelled_id,
                observation_id("late-cancelled"),
                TaskObservationKind::Correction,
                observation_text("Too late"),
            )
            .unwrap_err(),
        TaskStoreError::AlreadyCancelled { ref task_id } if task_id == &cancelled_id
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
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:corrupt-observation' AND stream_version = 2",
            [br#"{"id":"evidence","kind":"diagnostic","text":"Evidence","parent_attempt_id":" "}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap_err(),
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ref message,
        }) if message == "task observation id must not be blank"
    ));

    connection
        .execute(
            "UPDATE events SET payload_version = 3 WHERE stream_id = 'task:corrupt-observation' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap_err(),
        TaskStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 3,
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
fn version_one_observations_replay_as_ungrouped() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("version-one-observation").unwrap();
    let store = TaskStore::open(&path).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('task:version-one-observation', 1, 'task.started', 1, ?1)",
            [br#"{"goal":"Replay old evidence"}"#.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('task:version-one-observation', 2, 'task.observation_appended', 1, ?1)",
            [br#"{"id":"old-diagnostic","kind":"diagnostic","text":"Old evidence"}"#.as_slice()],
        )
        .unwrap();

    let task = store.load(&task_id).unwrap().unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].id().as_str(), "old-diagnostic");
    assert_eq!(task.observations()[0].parent_attempt_id(), None);
}

#[test]
fn invalid_version_two_parent_relations_are_invalid_history() {
    let cases: &[(&str, &[&[u8]])] = &[
        (
            "missing-parent-history",
            &[br#"{"id":"diagnostic","kind":"diagnostic","text":"Evidence","parent_attempt_id":"missing"}"#],
        ),
        (
            "parented-attempt-history",
            &[br#"{"id":"attempt","kind":"attempt","text":"Try","parent_attempt_id":"missing"}"#],
        ),
        (
            "non-attempt-parent-history",
            &[
                br#"{"id":"diagnostic","kind":"diagnostic","text":"Evidence","parent_attempt_id":null}"#,
                br#"{"id":"correction","kind":"correction","text":"Fix","parent_attempt_id":"diagnostic"}"#,
            ],
        ),
    ];

    for (external_id, observations) in cases {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        let task_id = TaskId::new(*external_id).unwrap();
        let store = TaskStore::open(&path).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO events VALUES (?1, 1, 'task.started', 1, ?2)",
                rusqlite::params![
                    format!("task:{external_id}"),
                    br#"{"goal":"Reject corrupt relation"}"#.as_slice()
                ],
            )
            .unwrap();
        for (index, payload) in observations.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO events VALUES (?1, ?2, 'task.observation_appended', 2, ?3)",
                    rusqlite::params![
                        format!("task:{external_id}"),
                        i64::try_from(index).unwrap() + 2,
                        payload,
                    ],
                )
                .unwrap();
        }

        assert!(matches!(
            store.load(&task_id).unwrap_err(),
            TaskStoreError::InvalidHistory { event_count }
                if event_count == observations.len() + 1
        ));
    }
}

#[test]
fn existing_task_history_without_observations_replays_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("legacy-shape").unwrap();
    let store = TaskStore::open(&path).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('task:legacy-shape', 1, 'task.started', 1, ?1)",
            [br#"{"goal":"No observations"}"#.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events VALUES ('task:legacy-shape', 2, 'task.completed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let loaded = store.load(&task_id).unwrap().unwrap();
    assert!(loaded.observations().is_empty());
    assert_eq!(loaded.status(), TaskStatus::Completed);
    assert_eq!(loaded.output(), None);
}
