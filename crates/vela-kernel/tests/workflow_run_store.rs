use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    workflow::{
        RegisteredWorkflow, RegisteredWorkflowPhase, RegisteredWorkflowTransition, WorkflowId,
        WorkflowRunId, WorkflowRunIdError, WorkflowRunStore, WorkflowRunStoreError,
    },
};

fn workflow(start: &str) -> RegisteredWorkflow {
    RegisteredWorkflow::new(
        WorkflowId::new("release.workflow").unwrap(),
        start,
        vec![
            RegisteredWorkflowPhase::new(
                "plan",
                false,
                vec![RegisteredWorkflowTransition::new(
                    "done",
                    Some("plan.approved".to_owned()),
                )],
            ),
            RegisteredWorkflowPhase::new("done", true, vec![]),
        ],
    )
}

#[test]
fn run_ids_reject_blank_values_and_preserve_exact_non_blank_text() {
    assert_eq!(WorkflowRunId::new(" \n ").unwrap_err(), WorkflowRunIdError);
    assert_eq!(
        WorkflowRunId::new(" run:release ").unwrap().as_str(),
        " run:release "
    );
}

#[test]
fn starts_at_the_declared_phase_and_replays_the_exact_owned_topology() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-42").unwrap();
    let definition = workflow("plan");

    let run = WorkflowRunStore::open(&path)
        .unwrap()
        .start(id.clone(), &definition)
        .unwrap();

    assert_eq!(run.id(), &id);
    assert_eq!(run.workflow(), &definition);
    assert_eq!(run.current_phase().id(), "plan");
    assert!(!run.is_terminal());
    drop(definition);

    let loaded = WorkflowRunStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.id(), &id);
    assert_eq!(loaded.workflow().id().as_str(), "release.workflow");
    assert_eq!(loaded.workflow().start(), "plan");
    assert_eq!(loaded.workflow().phases().len(), 2);
    assert_eq!(
        loaded.workflow().phases()[0].transitions()[0].target(),
        "done"
    );
    assert_eq!(
        loaded.workflow().phases()[0].transitions()[0].gate(),
        Some("plan.approved")
    );
    assert_eq!(loaded.current_phase().id(), "plan");
}

#[test]
fn duplicate_and_invalid_start_fail_without_an_extra_event() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("same").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    store.start(id.clone(), &workflow("plan")).unwrap();

    assert!(matches!(
        store.start(id.clone(), &workflow("plan")).unwrap_err(),
        WorkflowRunStoreError::AlreadyExists { run_id } if run_id == id
    ));
    assert!(matches!(
        store
            .start(WorkflowRunId::new("invalid").unwrap(), &workflow("missing"))
            .unwrap_err(),
        WorkflowRunStoreError::InvalidDefinition { .. }
    ));

    let count: u64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn missing_runs_and_corrupt_history_fail_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("corrupt").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    assert!(store.load(&id).unwrap().is_none());
    store.start(id.clone(), &workflow("plan")).unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET event_type = 'workflow_run.unknown' WHERE stream_id = 'workflow-run:corrupt'",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::UnsupportedEvent { .. })
    ));

    connection
        .execute(
            "UPDATE events SET event_type = 'workflow_run.started', payload = X'7B7D' WHERE stream_id = 'workflow-run:corrupt'",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

#[test]
fn extra_start_events_are_rejected_as_invalid_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("history").unwrap();
    WorkflowRunStore::open(&path)
        .unwrap()
        .start(id.clone(), &workflow("plan"))
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events SELECT stream_id, 2, event_type, payload_version, payload FROM events WHERE stream_id = 'workflow-run:history'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap_err(),
        WorkflowRunStoreError::InvalidHistory { event_count: 2 }
    ));
}
