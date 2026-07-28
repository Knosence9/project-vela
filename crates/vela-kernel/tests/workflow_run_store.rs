use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    workflow::{
        RegisteredWorkflow, RegisteredWorkflowPhase, RegisteredWorkflowTransition, WorkflowId,
        WorkflowRunCancellation, WorkflowRunCancellationError, WorkflowRunId, WorkflowRunIdError,
        WorkflowRunStore, WorkflowRunStoreError,
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

fn advancing_workflow() -> RegisteredWorkflow {
    RegisteredWorkflow::new(
        WorkflowId::new("release.workflow").unwrap(),
        "plan",
        vec![
            RegisteredWorkflowPhase::new(
                "plan",
                false,
                vec![RegisteredWorkflowTransition::new("review", None)],
            ),
            RegisteredWorkflowPhase::new(
                "review",
                false,
                vec![RegisteredWorkflowTransition::new(
                    "done",
                    Some("release.approved".to_owned()),
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
fn cancellation_reasons_reject_empty_values_and_preserve_exact_text() {
    assert_eq!(
        WorkflowRunCancellation::new("").unwrap_err(),
        WorkflowRunCancellationError
    );
    assert_eq!(
        WorkflowRunCancellation::new(" \n ").unwrap().as_str(),
        " \n "
    );
}

#[test]
fn lists_every_workflow_run_in_exact_id_order_with_complete_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = WorkflowRunStore::open(&path).unwrap();
    assert!(store.list().unwrap().is_empty());

    let terminal_id = WorkflowRunId::new("z-terminal").unwrap();
    let terminal_started = store
        .start(terminal_id.clone(), &advancing_workflow())
        .unwrap();
    let terminal_review = store
        .advance(&terminal_id, terminal_started.revision(), 0, None)
        .unwrap();
    store
        .advance(
            &terminal_id,
            terminal_review.revision(),
            0,
            Some("release.approved"),
        )
        .unwrap();

    let cancelled_id = WorkflowRunId::new("m-cancelled").unwrap();
    let cancelled_started = store
        .start(cancelled_id.clone(), &advancing_workflow())
        .unwrap();
    store
        .cancel(
            &cancelled_id,
            cancelled_started.revision(),
            WorkflowRunCancellation::new("operator stop").unwrap(),
        )
        .unwrap();

    let active_id = WorkflowRunId::new("a-active").unwrap();
    store
        .start(active_id.clone(), &advancing_workflow())
        .unwrap();

    drop(store);
    let runs = WorkflowRunStore::open(&path).unwrap().list().unwrap();
    assert_eq!(
        runs.iter().map(|run| run.id()).collect::<Vec<_>>(),
        vec![&active_id, &cancelled_id, &terminal_id]
    );
    assert_eq!(runs[0].revision(), 1);
    assert_eq!(runs[0].current_phase().id(), "plan");
    assert_eq!(runs[1].revision(), 2);
    assert_eq!(
        runs[1].cancellation().map(WorkflowRunCancellation::as_str),
        Some("operator stop")
    );
    assert_eq!(runs[2].revision(), 3);
    assert_eq!(runs[2].current_phase().id(), "done");
    assert!(runs[2].is_terminal());
}

#[test]
fn listing_ignores_unrelated_streams_and_fails_closed_on_a_malformed_candidate() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = WorkflowRunStore::open(&path).unwrap();
    store
        .start(WorkflowRunId::new("valid").unwrap(), &workflow("plan"))
        .unwrap();
    store
        .start(WorkflowRunId::new("broken").unwrap(), &advancing_workflow())
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events (stream_id, stream_version, event_type, payload_version, payload) VALUES ('unrelated', 1, 'other.started', 1, X'7B7D')",
            [],
        )
        .unwrap();
    assert_eq!(
        WorkflowRunStore::open(&path).unwrap().list().unwrap().len(),
        2
    );

    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'workflow-run:broken'",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path).unwrap().list().unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

#[test]
fn listing_rejects_a_malformed_candidate_stream_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    WorkflowRunStore::open(&path)
        .unwrap()
        .start(WorkflowRunId::new("valid").unwrap(), &workflow("plan"))
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET stream_id = 'not-a-workflow-run' WHERE stream_id = 'workflow-run:valid'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path).unwrap().list().unwrap_err(),
        WorkflowRunStoreError::InvalidStreamId { stream_id }
            if stream_id == "not-a-workflow-run"
    ));
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
    assert_eq!(run.revision(), 1);
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

#[test]
fn advances_ungated_and_exact_gated_transitions_durably() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-advance").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();

    let review = store.advance(&id, started.revision(), 0, None).unwrap();
    assert_eq!(review.current_phase().id(), "review");
    assert_eq!(review.revision(), 2);
    assert!(!review.is_terminal());

    drop(store);
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let reopened = store.load(&id).unwrap().unwrap();
    assert_eq!(reopened.current_phase().id(), "review");
    assert_eq!(reopened.revision(), 2);

    let done = store
        .advance(&id, reopened.revision(), 0, Some("release.approved"))
        .unwrap();
    assert_eq!(done.current_phase().id(), "done");
    assert_eq!(done.revision(), 3);
    assert!(done.is_terminal());

    drop(store);
    let loaded = WorkflowRunStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.current_phase().id(), "done");
    assert_eq!(loaded.revision(), 3);
}

#[test]
fn advancement_failures_are_atomic_and_stale_revisions_are_not_reinterpreted() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-race").unwrap();
    let missing = WorkflowRunId::new("missing").unwrap();
    let mut first = WorkflowRunStore::open(&path).unwrap();
    let started = first.start(id.clone(), &advancing_workflow()).unwrap();
    let stale_revision = started.revision();

    assert!(matches!(
        first.advance(&missing, 1, 0, None).unwrap_err(),
        WorkflowRunStoreError::NotFound { run_id } if run_id == missing
    ));
    assert!(matches!(
        first.advance(&id, stale_revision, 1, None).unwrap_err(),
        WorkflowRunStoreError::InvalidTransition { .. }
    ));
    assert!(matches!(
        first
            .advance(&id, stale_revision, 0, Some("unexpected"))
            .unwrap_err(),
        WorkflowRunStoreError::InvalidTransition { .. }
    ));

    let review = first.advance(&id, stale_revision, 0, None).unwrap();
    let malformed_id = WorkflowRunId::new("malformed-target").unwrap();
    let malformed = RegisteredWorkflow::new(
        WorkflowId::new("malformed.workflow").unwrap(),
        "start",
        vec![RegisteredWorkflowPhase::new(
            "start",
            false,
            vec![RegisteredWorkflowTransition::new("missing", None)],
        )],
    );
    let malformed_run = first.start(malformed_id.clone(), &malformed).unwrap();
    assert!(matches!(
        first
            .advance(&malformed_id, malformed_run.revision(), 0, None)
            .unwrap_err(),
        WorkflowRunStoreError::InvalidTransition { .. }
    ));

    let mut second = WorkflowRunStore::open(&path).unwrap();
    assert!(matches!(
        second.advance(&id, stale_revision, 0, None).unwrap_err(),
        WorkflowRunStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 2,
        }
    ));
    assert!(matches!(
        first.advance(&id, review.revision(), 0, None).unwrap_err(),
        WorkflowRunStoreError::InvalidTransition { .. }
    ));

    let count: u64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE stream_id = ?1",
            [format!("workflow-run:{id}")],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn terminal_and_impossible_advancement_histories_fail_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-history").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let review = store.advance(&id, started.revision(), 0, None).unwrap();
    let done = store
        .advance(&id, review.revision(), 0, Some("release.approved"))
        .unwrap();
    assert!(matches!(
        store.advance(&id, done.revision(), 0, None).unwrap_err(),
        WorkflowRunStoreError::InvalidTransition { .. }
    ));
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.target_phase_index', 0) AS BLOB) WHERE stream_id = 'workflow-run:release-history' AND stream_version = 2",
            [],
        )
        .unwrap();
    let error = WorkflowRunStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap_err();
    assert!(
        matches!(error, WorkflowRunStoreError::InvalidHistory { .. }),
        "unexpected error: {error:?}"
    );
}

#[test]
fn cancels_an_exact_non_terminal_revision_without_rewriting_phase_or_topology() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-cancel").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let review = store.advance(&id, started.revision(), 0, None).unwrap();
    let reason = WorkflowRunCancellation::new("operator stopped release").unwrap();

    let cancelled = store
        .cancel(&id, review.revision(), reason.clone())
        .unwrap();
    assert_eq!(cancelled.revision(), 3);
    assert_eq!(cancelled.current_phase().id(), "review");
    assert_eq!(cancelled.workflow(), review.workflow());
    assert!(cancelled.is_cancelled());
    assert_eq!(cancelled.cancellation(), Some(&reason));
    assert!(!cancelled.is_terminal());

    drop(store);
    let loaded = WorkflowRunStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.current_phase().id(), "review");
    assert!(loaded.is_cancelled());
    assert_eq!(loaded.cancellation(), Some(&reason));
}

#[test]
fn cancellation_failures_are_atomic_and_freeze_the_winning_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-cancel-race").unwrap();
    let missing = WorkflowRunId::new("missing").unwrap();
    let reason = WorkflowRunCancellation::new("stop").unwrap();
    let mut first = WorkflowRunStore::open(&path).unwrap();
    let started = first.start(id.clone(), &advancing_workflow()).unwrap();
    let stale_revision = started.revision();

    assert!(matches!(
        first.cancel(&missing, 1, reason.clone()).unwrap_err(),
        WorkflowRunStoreError::NotFound { run_id } if run_id == missing
    ));
    let review = first.advance(&id, stale_revision, 0, None).unwrap();
    assert!(matches!(
        first
            .cancel(&id, stale_revision, reason.clone())
            .unwrap_err(),
        WorkflowRunStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 2,
        }
    ));
    let mut second = WorkflowRunStore::open(&path).unwrap();
    let cancelled = first
        .cancel(&id, review.revision(), reason.clone())
        .unwrap();
    assert!(matches!(
        second
            .advance(&id, review.revision(), 0, Some("release.approved"))
            .unwrap_err(),
        WorkflowRunStoreError::ConcurrentModification {
            expected_revision: 2,
            current_revision: 3,
        }
    ));
    assert!(matches!(
        first
            .cancel(&id, cancelled.revision(), reason.clone())
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyCancelled { .. }
    ));
    assert!(matches!(
        first
            .advance(&id, cancelled.revision(), 0, Some("release.approved"))
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyCancelled { .. }
    ));

    let terminal_id = WorkflowRunId::new("terminal").unwrap();
    let terminal_started = first
        .start(terminal_id.clone(), &advancing_workflow())
        .unwrap();
    let terminal_review = first
        .advance(&terminal_id, terminal_started.revision(), 0, None)
        .unwrap();
    let terminal = first
        .advance(
            &terminal_id,
            terminal_review.revision(),
            0,
            Some("release.approved"),
        )
        .unwrap();
    assert!(matches!(
        first
            .cancel(&terminal_id, terminal.revision(), reason)
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyTerminal { .. }
    ));

    let count: u64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE stream_id = ?1",
            [format!("workflow-run:{id}")],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
}

#[test]
fn malformed_and_impossible_cancellation_histories_fail_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("cancel-history").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    store
        .cancel(
            &id,
            started.revision(),
            WorkflowRunCancellation::new("stop").unwrap(),
        )
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.source_phase_index', 1) AS BLOB) WHERE stream_id = 'workflow-run:cancel-history' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap_err(),
        WorkflowRunStoreError::InvalidHistory { .. }
    ));

    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.source_phase_index', 0, '$.reason', '') AS BLOB) WHERE stream_id = 'workflow-run:cancel-history' AND stream_version = 2",
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
