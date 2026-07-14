use std::error::Error;

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    task::{TaskGoal, TaskId, TaskStatus, TaskStore, TaskStoreError},
};

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

    let completed = TaskStore::open(&path).unwrap().complete(&id).unwrap();

    assert_eq!(completed.id(), &id);
    assert_eq!(completed.goal(), &goal);
    assert_eq!(completed.status(), TaskStatus::Completed);
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&id).unwrap(),
        Some(completed)
    );
}

#[test]
fn rejects_completing_an_unknown_task_without_creating_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = TaskId::new("missing").unwrap();
    let mut store = TaskStore::open(&path).unwrap();

    let error = store.complete(&id).unwrap_err();

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
    let completed = store.complete(&id).unwrap();

    let error = store.complete(&id).unwrap_err();

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
fn rejects_empty_task_ids_and_goals_before_opening_storage() {
    assert_eq!(
        TaskId::new("").unwrap_err().to_string(),
        "task id must not be empty"
    );
    assert_eq!(
        TaskGoal::new("").unwrap_err().to_string(),
        "task goal must not be empty"
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
fn rejects_completion_before_start_as_invalid_history() {
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
    store.complete(&id).unwrap();
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
