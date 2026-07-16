use std::{
    error::Error,
    sync::{Arc, Barrier},
    thread,
};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    session::{SessionId, SessionStore, SessionTitle},
    task::{
        TaskCancellation, TaskFailure, TaskGoal, TaskId, TaskOutput, TaskStatus, TaskStore,
        TaskStoreError,
    },
};

#[test]
fn lists_no_tasks_from_an_empty_store() {
    let directory = tempdir().unwrap();
    let store = TaskStore::open(directory.path().join("events.sqlite3")).unwrap();

    assert!(store.list().unwrap().is_empty());
}

#[test]
fn lists_latest_tasks_in_id_order_after_reopening_without_writing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let session_id = SessionId::new("planning").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(session_id.clone(), SessionTitle::new("Planning").unwrap())
        .unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    let later_id = TaskId::new("zeta").unwrap();
    store
        .start(later_id.clone(), TaskGoal::new("Later task").unwrap())
        .unwrap();
    store.associate_session(&later_id, &session_id).unwrap();
    let output = TaskOutput::new("Done").unwrap();
    store.complete(&later_id, output.clone()).unwrap();
    let earlier_id = TaskId::new("alpha").unwrap();
    let failure = TaskFailure::new("Unavailable").unwrap();
    store
        .start(earlier_id.clone(), TaskGoal::new("Earlier task").unwrap())
        .unwrap();
    store.fail(&earlier_id, failure.clone()).unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let before: u64 = connection
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    drop(connection);
    let tasks = TaskStore::open(&path).unwrap().list().unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id(), &earlier_id);
    assert_eq!(tasks[0].status(), TaskStatus::Failed);
    assert_eq!(tasks[0].failure(), Some(&failure));
    assert_eq!(tasks[0].session_id(), None);
    assert_eq!(tasks[1].id(), &later_id);
    assert_eq!(tasks[1].goal().as_str(), "Later task");
    assert_eq!(tasks[1].status(), TaskStatus::Completed);
    assert_eq!(tasks[1].output(), Some(&output));
    assert_eq!(tasks[1].session_id(), Some(&session_id));
    assert_eq!(
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row::<u64, _, _>("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap(),
        before
    );
}

#[test]
fn listing_tasks_excludes_session_streams_with_matching_external_ids() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let shared = "shared";
    SessionStore::open(&path)
        .unwrap()
        .create(
            SessionId::new(shared).unwrap(),
            SessionTitle::new("Session").unwrap(),
        )
        .unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            TaskId::new(shared).unwrap(),
            TaskGoal::new("Task with the same external ID").unwrap(),
        )
        .unwrap();

    let tasks = TaskStore::open(&path).unwrap().list().unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id().as_str(), shared);
}

#[test]
fn listing_tasks_rejects_malformed_creation_payloads_and_stream_ids() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    TaskStore::open(&path)
        .unwrap()
        .start(
            TaskId::new("corrupt").unwrap(),
            TaskGoal::new("Corrupt me").unwrap(),
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'task:corrupt'",
            [],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().list().unwrap_err(),
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET stream_id = 'task:', payload = X'7B22676F616C223A22436F7272757074206D65227D'",
            [],
        )
        .unwrap();
    assert!(matches!(
        TaskStore::open(&path).unwrap().list().unwrap_err(),
        TaskStoreError::InvalidStreamId { ref stream_id } if stream_id == "task:"
    ));
}

fn cancellation() -> TaskCancellation {
    TaskCancellation::new("request superseded").unwrap()
}

fn failure() -> TaskFailure {
    TaskFailure::new("provider request failed").unwrap()
}

fn output() -> TaskOutput {
    TaskOutput::new("task completed").unwrap()
}

#[test]
fn starts_and_loads_an_active_task_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("research:rust-agents").unwrap();
    let goal = TaskGoal::new("Compare supervision models").unwrap();

    let task = TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), goal.clone())
        .unwrap();

    assert_eq!(task.id(), &id);
    assert_eq!(task.goal(), &goal);
    assert_eq!(task.status(), TaskStatus::Active);
    assert_eq!(task.output(), None);
    assert_eq!(task.cancellation(), None);
    assert_eq!(task.failure(), None);

    let loaded = TaskStore::open(&path).unwrap().load(&id).unwrap().unwrap();
    assert_eq!(loaded, task);
}

#[test]
fn completes_and_loads_a_completed_task_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("research:rust-agents").unwrap();
    let goal = TaskGoal::new("Compare supervision models").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), goal.clone())
        .unwrap();

    let output = TaskOutput::new("Supervision models compared").unwrap();
    let completed = TaskStore::open(&path)
        .unwrap()
        .complete(&id, output.clone())
        .unwrap();

    assert_eq!(completed.id(), &id);
    assert_eq!(completed.goal(), &goal);
    assert_eq!(completed.status(), TaskStatus::Completed);
    assert_eq!(completed.output(), Some(&output));
    assert_eq!(completed.cancellation(), None);
    assert_eq!(completed.failure(), None);
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&id).unwrap(),
        Some(completed)
    );
}

#[test]
fn persists_completion_output_in_the_typed_event_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("completion-payload").unwrap();
    let output = TaskOutput::new(" compared 3 supervision models ").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Compare models").unwrap())
        .unwrap();

    let completed = store.complete(&id, output.clone()).unwrap();

    assert_eq!(completed.output().unwrap().as_str(), output.as_str());
    let (payload_version, payload): (u32, Vec<u8>) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT payload_version, payload FROM events WHERE stream_id = 'task:completion-payload' AND stream_version = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(payload_version, 2);
    assert_eq!(payload, br#"{"output":" compared 3 supervision models "}"#);
}

#[test]
fn loads_a_legacy_v1_completion_without_output() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("legacy-completion").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Read old history").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:legacy-completion', 2, 'task.completed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let completed = store.load(&id).unwrap().unwrap();

    assert_eq!(completed.status(), TaskStatus::Completed);
    assert_eq!(completed.output(), None);
}

#[test]
fn cancels_and_loads_a_cancelled_task_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("research:rust-agents").unwrap();
    let goal = TaskGoal::new("Compare supervision models").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), goal.clone())
        .unwrap();

    let reason = TaskCancellation::new("user changed direction").unwrap();
    let cancelled = TaskStore::open(&path)
        .unwrap()
        .cancel(&id, reason.clone())
        .unwrap();

    assert_eq!(cancelled.id(), &id);
    assert_eq!(cancelled.goal(), &goal);
    assert_eq!(cancelled.status(), TaskStatus::Cancelled);
    assert_eq!(cancelled.output(), None);
    assert_eq!(cancelled.failure(), None);
    assert_eq!(cancelled.cancellation(), Some(&reason));
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&id).unwrap(),
        Some(cancelled)
    );
}

#[test]
fn persists_cancellation_reason_in_the_typed_event_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("cancellation-payload").unwrap();
    let reason = TaskCancellation::new(" user changed direction ").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(
            id.clone(),
            TaskGoal::new("Preserve cancellation context").unwrap(),
        )
        .unwrap();

    let cancelled = store.cancel(&id, reason.clone()).unwrap();

    assert_eq!(cancelled.cancellation().unwrap().as_str(), reason.as_str());
    let (payload_version, payload): (u32, Vec<u8>) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT payload_version, payload FROM events WHERE stream_id = 'task:cancellation-payload' AND stream_version = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(payload_version, 2);
    assert_eq!(payload, br#"{"reason":" user changed direction "}"#);
}

#[test]
fn loads_a_legacy_v1_cancellation_without_a_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("legacy-cancellation").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Read old history").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:legacy-cancellation', 2, 'task.cancelled', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let cancelled = store.load(&id).unwrap().unwrap();

    assert_eq!(cancelled.status(), TaskStatus::Cancelled);
    assert_eq!(cancelled.cancellation(), None);
}

#[test]
fn fails_and_loads_a_failed_task_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("research:rust-agents").unwrap();
    let goal = TaskGoal::new("Compare supervision models").unwrap();
    let failure = TaskFailure::new("provider request timed out").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), goal.clone())
        .unwrap();

    let failed = TaskStore::open(&path)
        .unwrap()
        .fail(&id, failure.clone())
        .unwrap();

    assert_eq!(failed.id(), &id);
    assert_eq!(failed.goal(), &goal);
    assert_eq!(failed.status(), TaskStatus::Failed);
    assert_eq!(failed.output(), None);
    assert_eq!(failed.cancellation(), None);
    assert_eq!(failed.failure(), Some(&failure));
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&id).unwrap(),
        Some(failed)
    );
}

#[test]
fn persists_failure_diagnostic_in_the_typed_event_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("diagnostic-payload").unwrap();
    let diagnostic = TaskFailure::new(" provider request timed out ").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Preserve diagnostics").unwrap())
        .unwrap();

    let failed = store.fail(&id, diagnostic.clone()).unwrap();

    assert_eq!(failed.failure().unwrap().as_str(), diagnostic.as_str());
    let (payload_version, payload): (u32, Vec<u8>) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT payload_version, payload FROM events WHERE stream_id = 'task:diagnostic-payload' AND stream_version = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(payload_version, 2);
    assert_eq!(payload, br#"{"failure":" provider request timed out "}"#);
}

#[test]
fn loads_a_legacy_v1_failure_without_a_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("legacy-failure").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Read old history").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:legacy-failure', 2, 'task.failed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let failed = store.load(&id).unwrap().unwrap();

    assert_eq!(failed.status(), TaskStatus::Failed);
    assert_eq!(failed.failure(), None);
}

#[test]
fn rejects_failing_an_unknown_task_without_creating_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("missing").unwrap();
    let mut store = TaskStore::open(&path).unwrap();

    let error = store.fail(&id, failure()).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::NotFound { ref task_id } if task_id == &id
    ));
    assert_eq!(store.load(&id).unwrap(), None);
}

#[test]
fn rejects_cancelling_an_unknown_task_without_creating_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("missing").unwrap();
    let mut store = TaskStore::open(&path).unwrap();

    let error = store.cancel(&id, cancellation()).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::NotFound { ref task_id } if task_id == &id
    ));
    assert_eq!(store.load(&id).unwrap(), None);
}

#[test]
fn rejects_repeated_or_conflicting_terminal_transitions() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let completed_id = TaskId::new("completed").unwrap();
    let cancelled_id = TaskId::new("cancelled").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(completed_id.clone(), TaskGoal::new("Finish").unwrap())
        .unwrap();
    store.complete(&completed_id, output()).unwrap();
    store
        .start(cancelled_id.clone(), TaskGoal::new("Stop").unwrap())
        .unwrap();
    store.cancel(&cancelled_id, cancellation()).unwrap();

    assert!(matches!(
        store.cancel(&completed_id, cancellation()).unwrap_err(),
        TaskStoreError::AlreadyCompleted { task_id } if task_id == completed_id
    ));
    assert!(matches!(
        store.cancel(&cancelled_id, cancellation()).unwrap_err(),
        TaskStoreError::AlreadyCancelled { task_id } if task_id == cancelled_id
    ));
    assert!(matches!(
        store.complete(&cancelled_id, output()).unwrap_err(),
        TaskStoreError::AlreadyCancelled { task_id } if task_id == cancelled_id
    ));
    assert!(matches!(
        store.fail(&completed_id, failure()).unwrap_err(),
        TaskStoreError::AlreadyCompleted { task_id } if task_id == completed_id
    ));
    assert!(matches!(
        store.fail(&cancelled_id, failure()).unwrap_err(),
        TaskStoreError::AlreadyCancelled { task_id } if task_id == cancelled_id
    ));
}

#[test]
fn failed_tasks_reject_every_later_terminal_transition() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("failed").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Try once").unwrap())
        .unwrap();
    store.fail(&id, failure()).unwrap();

    let repeated_failure = store.fail(&id, failure()).unwrap_err();
    assert_eq!(
        repeated_failure.to_string(),
        "task failed has already failed"
    );
    assert!(repeated_failure.source().is_none());
    for error in [
        repeated_failure,
        store.complete(&id, output()).unwrap_err(),
        store.cancel(&id, cancellation()).unwrap_err(),
    ] {
        assert!(matches!(
            error,
            TaskStoreError::AlreadyFailed { ref task_id } if task_id == &id
        ));
    }
}

#[test]
fn racing_completion_and_failure_persist_one_terminal_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("failure-race").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), TaskGoal::new("Choose one outcome").unwrap())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let complete_path = path.clone();
    let complete_id = id.clone();
    let complete_barrier = Arc::clone(&barrier);
    let completion = thread::spawn(move || {
        let mut store = TaskStore::open(complete_path).unwrap();
        complete_barrier.wait();
        store.complete(&complete_id, output())
    });
    let failure_result = thread::spawn(move || {
        let mut store = TaskStore::open(path).unwrap();
        barrier.wait();
        store.fail(&id, failure())
    });

    match (completion.join().unwrap(), failure_result.join().unwrap()) {
        (Ok(task), Err(TaskStoreError::AlreadyCompleted { .. })) => {
            assert_eq!(task.status(), TaskStatus::Completed);
            assert_eq!(task.failure(), None);
        }
        (Err(TaskStoreError::AlreadyFailed { .. }), Ok(task)) => {
            assert_eq!(task.status(), TaskStatus::Failed);
            assert_eq!(task.failure(), Some(&failure()));
        }
        outcomes => panic!("unexpected terminal race outcomes: {outcomes:?}"),
    }
}

#[test]
fn racing_completion_and_cancellation_persist_one_terminal_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("race").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            id.clone(),
            TaskGoal::new("Choose one terminal state").unwrap(),
        )
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let complete_path = path.clone();
    let complete_id = id.clone();
    let complete_barrier = Arc::clone(&barrier);
    let completion = thread::spawn(move || {
        let mut store = TaskStore::open(complete_path).unwrap();
        complete_barrier.wait();
        store.complete(&complete_id, output())
    });
    let cancellation = thread::spawn(move || {
        let mut store = TaskStore::open(path).unwrap();
        barrier.wait();
        store.cancel(&id, cancellation())
    });

    let completion = completion.join().unwrap();
    let cancellation = cancellation.join().unwrap();
    match (completion, cancellation) {
        (Ok(task), Err(TaskStoreError::AlreadyCompleted { .. })) => {
            assert_eq!(task.status(), TaskStatus::Completed);
        }
        (Err(TaskStoreError::AlreadyCancelled { .. }), Ok(task)) => {
            assert_eq!(task.status(), TaskStatus::Cancelled);
        }
        outcomes => panic!("unexpected terminal race outcomes: {outcomes:?}"),
    }
}

#[test]
fn rejects_completing_an_unknown_task_without_creating_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("missing").unwrap();
    let mut store = TaskStore::open(&path).unwrap();

    let error = store.complete(&id, output()).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::NotFound { ref task_id } if task_id == &id
    ));
    assert_eq!(error.to_string(), "task missing was not found");
    assert!(error.source().is_none());
    assert_eq!(store.load(&id).unwrap(), None);
}

#[test]
fn rejects_completing_a_completed_task_and_preserves_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    let completed = store.complete(&id, output()).unwrap();

    let error = store.complete(&id, output()).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::AlreadyCompleted { ref task_id } if task_id == &id
    ));
    assert_eq!(error.to_string(), "task task-42 is already completed");
    assert!(error.source().is_none());
    assert_eq!(store.load(&id).unwrap(), Some(completed));
}

#[test]
fn loading_an_unknown_task_returns_none() {
    let directory = tempdir().unwrap();
    let store = TaskStore::open(directory.path().join("events.sqlite3")).unwrap();

    assert_eq!(store.load(&TaskId::new("missing").unwrap()).unwrap(), None);
}

#[test]
fn rejects_a_duplicate_start_and_preserves_the_original_task() {
    let directory = tempdir().unwrap();
    let mut store = TaskStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = TaskId::new("task-42").unwrap();
    let original_goal = TaskGoal::new("Review the kernel").unwrap();
    let original = store.start(id.clone(), original_goal).unwrap();

    let error = store
        .start(id.clone(), TaskGoal::new("Overwrite the task").unwrap())
        .unwrap_err();

    assert_eq!(error.to_string(), "task task-42 already exists");
    assert!(matches!(
        error,
        TaskStoreError::AlreadyExists { ref task_id } if task_id == &id
    ));
    assert!(error.source().is_none());
    assert_eq!(store.load(&id).unwrap(), Some(original));
}

#[test]
fn rejects_empty_task_values_before_opening_storage() {
    assert_eq!(
        TaskId::new("").unwrap_err().to_string(),
        "task id must not be empty"
    );
    assert_eq!(
        TaskGoal::new("").unwrap_err().to_string(),
        "task goal must not be empty"
    );
    assert_eq!(
        TaskOutput::new("").unwrap_err().to_string(),
        "task output must not be empty"
    );
    assert_eq!(
        TaskFailure::new("").unwrap_err().to_string(),
        "task failure diagnostic must not be empty"
    );
    assert_eq!(
        TaskCancellation::new("").unwrap_err().to_string(),
        "task cancellation reason must not be empty"
    );
}

#[test]
fn task_load_surfaces_an_unknown_event_discriminator() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET event_type = 'task.renamed' WHERE stream_id = 'task:task-42'",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::UnsupportedEvent {
            ref event_type,
            payload_version: 1,
        }) if event_type == "task.renamed"
    ));
    assert!(error.source().is_some());
}

#[test]
fn task_load_surfaces_an_unknown_payload_version() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = 'task:task-42'",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::UnsupportedEvent {
            event_type,
            payload_version: 2,
        }) if event_type == "task.started"
    ));
}

#[test]
fn task_load_surfaces_a_malformed_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'00' WHERE stream_id = 'task:task-42'",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
}

#[test]
fn task_load_rejects_an_empty_persisted_completion_output() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    store.complete(&id, output()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:task-42' AND stream_version = 2",
            [br#"{"output":""}"#.as_slice()],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));
}

#[test]
fn task_load_rejects_an_empty_persisted_cancellation_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    store.cancel(&id, cancellation()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:task-42' AND stream_version = 2",
            [br#"{"reason":""}"#.as_slice()],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));
}

#[test]
fn task_load_rejects_an_empty_persisted_failure_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    store.fail(&id, failure()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'task:task-42' AND stream_version = 2",
            [br#"{"failure":""}"#.as_slice()],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));
}

#[test]
fn rejects_terminal_events_before_start_as_invalid_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    TaskStore::open(&path).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:task-42', 1, 'task.completed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 1 }
    ));

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET event_type = 'task.cancelled' WHERE stream_id = 'task:task-42'",
            [],
        )
        .unwrap();
    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();
    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 1 }
    ));

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET event_type = 'task.failed', payload_version = 2, payload = X'7B226661696C757265223A226572726F72227D' WHERE stream_id = 'task:task-42'",
            [],
        )
        .unwrap();
    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();
    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 1 }
    ));
}

#[test]
fn rejects_events_after_completion_as_invalid_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    store.complete(&id, output()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:task-42', 3, 'task.completed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 3 }
    ));
}

#[test]
fn rejects_events_after_failure_as_invalid_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    store.fail(&id, failure()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:task-42', 3, 'task.failed', 2, X'7B226661696C757265223A226572726F72227D')",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 3 }
    ));
}

#[test]
fn rejects_events_after_cancellation_as_invalid_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("task-42").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(id.clone(), TaskGoal::new("Review the kernel").unwrap())
        .unwrap();
    store.cancel(&id, cancellation()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('task:task-42', 3, 'task.completed', 1, X'7B7D')",
            [],
        )
        .unwrap();

    let error = TaskStore::open(&path).unwrap().load(&id).unwrap_err();

    assert!(matches!(
        error,
        TaskStoreError::InvalidHistory { event_count: 3 }
    ));
}
