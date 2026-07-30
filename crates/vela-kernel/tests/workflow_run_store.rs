use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    task::{
        TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText, TaskOutput,
        TaskStatus, TaskStore, TaskVerificationCheck, TaskVerificationGateEvaluationError,
        TaskVerificationGateStatus, TaskVerificationOutcome,
    },
    workflow::{
        RegisteredWorkflow, RegisteredWorkflowPhase, RegisteredWorkflowTransition, WorkflowId,
        WorkflowRunCancellation, WorkflowRunCancellationError, WorkflowRunFailure,
        WorkflowRunFailureError, WorkflowRunFilter, WorkflowRunHistoryEvent, WorkflowRunId,
        WorkflowRunIdError, WorkflowRunPauseReason, WorkflowRunPauseReasonError,
        WorkflowRunResumeReason, WorkflowRunResumeReasonError, WorkflowRunStatus, WorkflowRunStore,
        WorkflowRunStoreError, WorkflowVerifiedAdvanceError,
    },
};

#[test]
fn advances_an_attributed_gated_run_through_passed_task_verification() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("verified-release").unwrap();
    let attempt_id = TaskObservationId::new("release-attempt").unwrap();
    let run_id = WorkflowRunId::new("verified-run").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(
            task_id.clone(),
            TaskGoal::new("ship verified release").unwrap(),
        )
        .unwrap();
    tasks
        .append_observation(
            &task_id,
            attempt_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("candidate release").unwrap(),
        )
        .unwrap();
    tasks
        .append_verification_for_attempt(
            &task_id,
            TaskObservationId::new("release-check").unwrap(),
            TaskVerificationOutcome::Passed,
            TaskVerificationCheck::new("plan.approved").unwrap(),
            TaskObservationText::new("release checks passed").unwrap(),
            attempt_id.clone(),
        )
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    let started = runs
        .start_for_task(run_id.clone(), &task_id, &workflow("plan"))
        .unwrap();

    let advanced = runs
        .advance_if_task_verification_passes(&run_id, started.revision(), 0, &attempt_id)
        .unwrap();

    assert_eq!(advanced.current_phase().id(), "done");
    let history = WorkflowRunStore::open(&path)
        .unwrap()
        .history(&run_id)
        .unwrap()
        .unwrap();
    assert!(matches!(
        history[1].event(),
        WorkflowRunHistoryEvent::Advanced {
            gate_acknowledgement: Some(gate),
            ..
        } if gate == "plan.approved"
    ));
}

#[test]
fn reports_pending_and_failed_task_verification_without_advancing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("blocked-release").unwrap();
    let attempt_id = TaskObservationId::new("blocked-attempt").unwrap();
    let other_attempt_id = TaskObservationId::new("other-attempt").unwrap();
    let run_id = WorkflowRunId::new("blocked-run").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("check release").unwrap())
        .unwrap();
    for id in [&attempt_id, &other_attempt_id] {
        tasks
            .append_observation(
                &task_id,
                id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new(format!("candidate {id}")).unwrap(),
            )
            .unwrap();
    }
    tasks
        .append_verification_for_attempt(
            &task_id,
            TaskObservationId::new("other-pass").unwrap(),
            TaskVerificationOutcome::Passed,
            TaskVerificationCheck::new("plan.approved").unwrap(),
            TaskObservationText::new("other attempt passed").unwrap(),
            other_attempt_id,
        )
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    let started = runs
        .start_for_task(run_id.clone(), &task_id, &workflow("plan"))
        .unwrap();

    assert!(matches!(
        runs.advance_if_task_verification_passes(&run_id, started.revision(), 0, &attempt_id),
        Err(WorkflowVerifiedAdvanceError::GatesPending { ref report })
            if report.status() == TaskVerificationGateStatus::Pending
    ));
    tasks
        .append_verification_for_attempt(
            &task_id,
            TaskObservationId::new("failed-check").unwrap(),
            TaskVerificationOutcome::Failed,
            TaskVerificationCheck::new("plan.approved").unwrap(),
            TaskObservationText::new("release check failed").unwrap(),
            attempt_id.clone(),
        )
        .unwrap();
    assert!(matches!(
        runs.advance_if_task_verification_passes(&run_id, started.revision(), 0, &attempt_id),
        Err(WorkflowVerifiedAdvanceError::GatesFailed { ref report })
            if report.status() == TaskVerificationGateStatus::Failed
    ));
    assert_eq!(
        runs.load(&run_id).unwrap().unwrap().revision(),
        started.revision()
    );
}

#[test]
fn rejects_unattributed_and_ungated_verified_advancement() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let attempt_id = TaskObservationId::new("attempt").unwrap();
    let plain_id = WorkflowRunId::new("plain-run").unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    let plain = runs.start(plain_id.clone(), &workflow("plan")).unwrap();
    assert!(matches!(
        runs.advance_if_task_verification_passes(&plain_id, plain.revision(), 0, &attempt_id),
        Err(WorkflowVerifiedAdvanceError::RunNotTaskAttributed { .. })
    ));

    let task_id = TaskId::new("ungated-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("ungated work").unwrap())
        .unwrap();
    let ungated_id = WorkflowRunId::new("ungated-run").unwrap();
    let ungated = runs
        .start_for_task(ungated_id.clone(), &task_id, &advancing_workflow())
        .unwrap();
    assert!(matches!(
        runs.advance_if_task_verification_passes(&ungated_id, ungated.revision(), 0, &attempt_id,),
        Err(WorkflowVerifiedAdvanceError::TransitionUngated { .. })
    ));
}

#[test]
fn rejects_missing_and_non_attempt_verification_parents() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("lineage-task").unwrap();
    let diagnostic_id = TaskObservationId::new("diagnostic").unwrap();
    let run_id = WorkflowRunId::new("lineage-run").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("validate lineage").unwrap())
        .unwrap();
    tasks
        .append_observation(
            &task_id,
            diagnostic_id.clone(),
            TaskObservationKind::Diagnostic,
            TaskObservationText::new("not an attempt").unwrap(),
        )
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    let started = runs
        .start_for_task(run_id.clone(), &task_id, &workflow("plan"))
        .unwrap();

    assert!(matches!(
        runs.advance_if_task_verification_passes(
            &run_id,
            started.revision(),
            0,
            &TaskObservationId::new("missing").unwrap(),
        ),
        Err(WorkflowVerifiedAdvanceError::GateEvaluation(
            TaskVerificationGateEvaluationError::AttemptNotFound { .. }
        ))
    ));
    assert!(matches!(
        runs.advance_if_task_verification_passes(&run_id, started.revision(), 0, &diagnostic_id,),
        Err(WorkflowVerifiedAdvanceError::GateEvaluation(
            TaskVerificationGateEvaluationError::ObservationNotAttempt { .. }
        ))
    ));
    assert_eq!(
        runs.load(&run_id).unwrap().unwrap().revision(),
        started.revision()
    );
}

#[test]
fn attributes_a_workflow_run_immutably_to_an_active_task() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("release-task").unwrap();
    let run_id = WorkflowRunId::new("release-run").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("ship release").unwrap())
        .unwrap();

    let started = WorkflowRunStore::open(&path)
        .unwrap()
        .start_for_task(run_id.clone(), &task_id, &advancing_workflow())
        .unwrap();
    assert_eq!(started.task_id(), Some(&task_id));
    let payload_version: u32 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT payload_version FROM events WHERE stream_id = 'workflow-run:release-run'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(payload_version, 3);
    tasks
        .complete(&task_id, TaskOutput::new("task complete").unwrap())
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path).unwrap().start_for_task(
            run_id.clone(),
            &task_id,
            &advancing_workflow(),
        ),
        Err(WorkflowRunStoreError::AlreadyExists { .. })
    ));

    let reopened = WorkflowRunStore::open(&path)
        .unwrap()
        .load(&run_id)
        .unwrap()
        .unwrap();
    assert_eq!(reopened.task_id(), Some(&task_id));
    assert_eq!(
        WorkflowRunStore::open(&path).unwrap().list().unwrap()[0].task_id(),
        Some(&task_id)
    );
    assert!(matches!(
        WorkflowRunStore::open(&path).unwrap().history(&run_id).unwrap().unwrap()[0].event(),
        WorkflowRunHistoryEvent::TaskStarted { task_id: history_task_id, .. }
            if history_task_id == &task_id
    ));
}

#[test]
fn preserves_inert_phase_skills_through_start_load_and_listing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let run_id = WorkflowRunId::new("skilled-run").unwrap();
    let workflow = RegisteredWorkflow::new(
        WorkflowId::new("skilled.workflow").unwrap(),
        "plan",
        vec![
            RegisteredWorkflowPhase::new(
                "plan",
                false,
                vec![RegisteredWorkflowTransition::new("done", None)],
            )
            .with_skills(["research.skill", "review.skill"]),
            RegisteredWorkflowPhase::new("done", true, vec![]),
        ],
    );

    let started = WorkflowRunStore::open(&path)
        .unwrap()
        .start(run_id.clone(), &workflow)
        .unwrap();
    assert_eq!(
        started.current_phase().skills(),
        ["research.skill", "review.skill"]
    );

    let reopened = WorkflowRunStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .load(&run_id)
            .unwrap()
            .unwrap()
            .current_phase()
            .skills(),
        ["research.skill", "review.skill"]
    );
    assert_eq!(
        reopened.list().unwrap()[0].current_phase().skills(),
        ["research.skill", "review.skill"]
    );
}

#[test]
fn replays_legacy_started_payloads_without_skill_bindings() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("legacy-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("legacy work").unwrap())
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    let plain_id = WorkflowRunId::new("legacy-v1").unwrap();
    let task_run_id = WorkflowRunId::new("legacy-v2").unwrap();
    runs.start(plain_id.clone(), &advancing_workflow()).unwrap();
    runs.start_for_task(task_run_id.clone(), &task_id, &advancing_workflow())
        .unwrap();
    drop(runs);

    let connection = rusqlite::Connection::open(&path).unwrap();
    for (stream_id, payload_version) in [
        ("workflow-run:legacy-v1", 1_u32),
        ("workflow-run:legacy-v2", 2_u32),
    ] {
        let payload: Vec<u8> = connection
            .query_row(
                "SELECT payload FROM events WHERE stream_id = ?1",
                [stream_id],
                |row| row.get(0),
            )
            .unwrap();
        let mut payload: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        for phase in payload["workflow"]["phases"].as_array_mut().unwrap() {
            phase.as_object_mut().unwrap().remove("skills");
        }
        connection
            .execute(
                "UPDATE events SET payload_version = ?1, payload = ?2 WHERE stream_id = ?3",
                rusqlite::params![
                    payload_version,
                    serde_json::to_vec(&payload).unwrap(),
                    stream_id
                ],
            )
            .unwrap();
    }

    let reopened = WorkflowRunStore::open(&path).unwrap();
    let plain = reopened.load(&plain_id).unwrap().unwrap();
    assert!(
        plain
            .workflow()
            .phases()
            .iter()
            .all(|phase| phase.skills().is_empty())
    );
    assert!(plain.task_id().is_none());
    let attributed = reopened.load(&task_run_id).unwrap().unwrap();
    assert!(
        attributed
            .workflow()
            .phases()
            .iter()
            .all(|phase| phase.skills().is_empty())
    );
    assert_eq!(attributed.task_id(), Some(&task_id));
}

#[test]
fn started_payload_versions_reject_cross_version_skill_shapes() {
    for (run_id, mutate) in [
        ("v3-without-skills", "remove-skills"),
        ("v1-with-skills", "legacy-version"),
        ("v3-blank-skill", "blank-skill"),
        ("v3-terminal-skill", "terminal-skill"),
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        let run_id = WorkflowRunId::new(run_id).unwrap();
        WorkflowRunStore::open(&path)
            .unwrap()
            .start(run_id.clone(), &advancing_workflow())
            .unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let mut payload: serde_json::Value = serde_json::from_slice(
            &connection
                .query_row("SELECT payload FROM events", [], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .unwrap(),
        )
        .unwrap();
        let payload_version = match mutate {
            "remove-skills" => {
                for phase in payload["workflow"]["phases"].as_array_mut().unwrap() {
                    phase.as_object_mut().unwrap().remove("skills");
                }
                3_u32
            }
            "legacy-version" => 1_u32,
            "blank-skill" => {
                payload["workflow"]["phases"][0]["skills"] =
                    serde_json::Value::Array(vec![serde_json::Value::String(" ".into())]);
                3_u32
            }
            "terminal-skill" => {
                payload["workflow"]["phases"][2]["skills"] =
                    serde_json::Value::Array(vec![serde_json::Value::String(
                        "review.skill".into(),
                    )]);
                3_u32
            }
            _ => unreachable!(),
        };
        connection
            .execute(
                "UPDATE events SET payload_version = ?1, payload = ?2",
                rusqlite::params![payload_version, serde_json::to_vec(&payload).unwrap()],
            )
            .unwrap();

        assert!(matches!(
            WorkflowRunStore::open(&path)
                .unwrap()
                .load(&run_id)
                .unwrap_err(),
            WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
        ));
    }
}

#[test]
fn lists_exact_task_attribution_in_run_id_order_after_task_completion_and_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("release-task").unwrap();
    let other_task_id = TaskId::new("other-task").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    for (id, goal) in [
        (task_id.clone(), "ship release"),
        (other_task_id.clone(), "other work"),
    ] {
        tasks.start(id, TaskGoal::new(goal).unwrap()).unwrap();
    }
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    runs.start_for_task(
        WorkflowRunId::new("zulu").unwrap(),
        &task_id,
        &advancing_workflow(),
    )
    .unwrap();
    runs.start_for_task(
        WorkflowRunId::new("alpha").unwrap(),
        &task_id,
        &advancing_workflow(),
    )
    .unwrap();
    runs.start_for_task(
        WorkflowRunId::new("other").unwrap(),
        &other_task_id,
        &advancing_workflow(),
    )
    .unwrap();
    runs.start(
        WorkflowRunId::new("unassociated").unwrap(),
        &advancing_workflow(),
    )
    .unwrap();
    tasks
        .complete(&task_id, TaskOutput::new("task complete").unwrap())
        .unwrap();
    drop(runs);

    let listed = WorkflowRunStore::open(&path)
        .unwrap()
        .list_for_task(&task_id)
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|run| run.id().as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zulu"]
    );
    assert!(listed.iter().all(|run| run.task_id() == Some(&task_id)));
    assert!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .list_for_task(&TaskId::new("no-runs").unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn lists_exact_workflow_identity_in_run_id_order_across_lifecycle_and_attribution() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let workflow_id = WorkflowId::new("release.workflow").unwrap();
    let target = advancing_workflow();
    let other = RegisteredWorkflow::new(
        WorkflowId::new("other.workflow").unwrap(),
        target.start(),
        target.phases().to_vec(),
    );
    let task_id = TaskId::new("release-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("ship release").unwrap())
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();

    runs.start_for_task(
        WorkflowRunId::new("attributed-active").unwrap(),
        &task_id,
        &target,
    )
    .unwrap();
    let cancelled_id = WorkflowRunId::new("cancelled").unwrap();
    let cancelled = runs.start(cancelled_id.clone(), &target).unwrap();
    runs.cancel(
        &cancelled_id,
        cancelled.revision(),
        WorkflowRunCancellation::new("operator stopped").unwrap(),
    )
    .unwrap();
    let failed_id = WorkflowRunId::new("failed").unwrap();
    let failed = runs.start(failed_id.clone(), &target).unwrap();
    runs.fail(
        &failed_id,
        failed.revision(),
        WorkflowRunFailure::new("release failed").unwrap(),
    )
    .unwrap();
    let paused_id = WorkflowRunId::new("paused").unwrap();
    let paused = runs.start(paused_id.clone(), &target).unwrap();
    runs.pause(
        &paused_id,
        paused.revision(),
        WorkflowRunPauseReason::new("await approval").unwrap(),
    )
    .unwrap();
    let terminal_id = WorkflowRunId::new("terminal").unwrap();
    let terminal = runs.start(terminal_id.clone(), &target).unwrap();
    let review = runs
        .advance(&terminal_id, terminal.revision(), 0, None)
        .unwrap();
    runs.advance(&terminal_id, review.revision(), 0, Some("release.approved"))
        .unwrap();
    runs.start(WorkflowRunId::new("other").unwrap(), &other)
        .unwrap();
    drop(runs);

    let reopened = WorkflowRunStore::open(&path).unwrap();
    let listed = reopened.list_for_workflow(&workflow_id).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|run| (run.id().as_str(), run.status()))
            .collect::<Vec<_>>(),
        [
            ("attributed-active", WorkflowRunStatus::Active),
            ("cancelled", WorkflowRunStatus::Cancelled),
            ("failed", WorkflowRunStatus::Failed),
            ("paused", WorkflowRunStatus::Paused),
            ("terminal", WorkflowRunStatus::AuthoredTerminal),
        ]
    );
    assert_eq!(listed[0].task_id(), Some(&task_id));
    assert!(listed[1..].iter().all(|run| run.task_id().is_none()));
    assert!(listed.iter().all(|run| run.workflow().id() == &workflow_id));
    assert!(
        reopened
            .list_for_workflow(&WorkflowId::new("missing.workflow").unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn lists_exact_lifecycle_status_in_run_id_order_across_attribution_and_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("release-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("ship release").unwrap())
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();

    runs.start_for_task(
        WorkflowRunId::new("zulu-active").unwrap(),
        &task_id,
        &advancing_workflow(),
    )
    .unwrap();
    runs.start(
        WorkflowRunId::new("alpha-active").unwrap(),
        &advancing_workflow(),
    )
    .unwrap();
    let cancelled_id = WorkflowRunId::new("cancelled").unwrap();
    let cancelled = runs
        .start(cancelled_id.clone(), &advancing_workflow())
        .unwrap();
    runs.cancel(
        &cancelled_id,
        cancelled.revision(),
        WorkflowRunCancellation::new("operator stopped").unwrap(),
    )
    .unwrap();
    let failed_id = WorkflowRunId::new("failed").unwrap();
    let failed = runs
        .start(failed_id.clone(), &advancing_workflow())
        .unwrap();
    runs.fail(
        &failed_id,
        failed.revision(),
        WorkflowRunFailure::new("release failed").unwrap(),
    )
    .unwrap();
    let paused_id = WorkflowRunId::new("paused").unwrap();
    let paused = runs
        .start(paused_id.clone(), &advancing_workflow())
        .unwrap();
    runs.pause(
        &paused_id,
        paused.revision(),
        WorkflowRunPauseReason::new("await approval").unwrap(),
    )
    .unwrap();
    let terminal_id = WorkflowRunId::new("terminal").unwrap();
    let terminal = runs
        .start(terminal_id.clone(), &advancing_workflow())
        .unwrap();
    let review = runs
        .advance(&terminal_id, terminal.revision(), 0, None)
        .unwrap();
    runs.advance(&terminal_id, review.revision(), 0, Some("release.approved"))
        .unwrap();
    drop(runs);

    let reopened = WorkflowRunStore::open(&path).unwrap();
    for (status, expected_ids) in [
        (
            WorkflowRunStatus::Active,
            vec!["alpha-active", "zulu-active"],
        ),
        (WorkflowRunStatus::Paused, vec!["paused"]),
        (WorkflowRunStatus::AuthoredTerminal, vec!["terminal"]),
        (WorkflowRunStatus::Cancelled, vec!["cancelled"]),
        (WorkflowRunStatus::Failed, vec!["failed"]),
    ] {
        let listed = reopened.list_by_status(status).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|run| run.id().as_str())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert!(listed.iter().all(|run| run.status() == status));
    }
    let active = reopened.list_by_status(WorkflowRunStatus::Active).unwrap();
    assert!(active.iter().any(|run| run.task_id() == Some(&task_id)));
    assert!(
        WorkflowRunStore::open(directory.path().join("empty.sqlite3"))
            .unwrap()
            .list_by_status(WorkflowRunStatus::Active)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn composes_workflow_run_filters_with_and_semantics_after_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("release-task").unwrap();
    let other_task_id = TaskId::new("other-task").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    for id in [task_id.clone(), other_task_id.clone()] {
        tasks
            .start(id, TaskGoal::new("ship release").unwrap())
            .unwrap();
    }
    let target = advancing_workflow();
    let workflow_id = target.id().clone();
    let other = RegisteredWorkflow::new(
        WorkflowId::new("other.workflow").unwrap(),
        target.start(),
        target.phases().to_vec(),
    );
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    runs.start_for_task(
        WorkflowRunId::new("alpha-active").unwrap(),
        &task_id,
        &target,
    )
    .unwrap();
    for (id, owning_task, workflow) in [
        ("bravo-match", &task_id, &target),
        ("charlie-other-workflow", &task_id, &other),
        ("delta-other-task", &other_task_id, &target),
    ] {
        let run_id = WorkflowRunId::new(id).unwrap();
        let started = runs
            .start_for_task(run_id.clone(), owning_task, workflow)
            .unwrap();
        runs.pause(
            &run_id,
            started.revision(),
            WorkflowRunPauseReason::new("await approval").unwrap(),
        )
        .unwrap();
    }
    let unassociated_id = WorkflowRunId::new("echo-unassociated").unwrap();
    let unassociated = runs.start(unassociated_id.clone(), &target).unwrap();
    runs.pause(
        &unassociated_id,
        unassociated.revision(),
        WorkflowRunPauseReason::new("await approval").unwrap(),
    )
    .unwrap();
    drop(runs);

    let reopened = WorkflowRunStore::open(&path).unwrap();
    let all = reopened.list_filtered(WorkflowRunFilter::new()).unwrap();
    assert_eq!(
        all.iter().map(|run| run.id().as_str()).collect::<Vec<_>>(),
        reopened
            .list()
            .unwrap()
            .iter()
            .map(|run| run.id().as_str())
            .collect::<Vec<_>>()
    );

    let paused_target = reopened
        .list_filtered(
            WorkflowRunFilter::new()
                .for_workflow(&workflow_id)
                .with_status(WorkflowRunStatus::Paused),
        )
        .unwrap();
    assert_eq!(
        paused_target
            .iter()
            .map(|run| run.id().as_str())
            .collect::<Vec<_>>(),
        ["bravo-match", "delta-other-task", "echo-unassociated"]
    );

    let exact_intersection = reopened
        .list_filtered(
            WorkflowRunFilter::new()
                .for_task(&task_id)
                .for_workflow(&workflow_id)
                .with_status(WorkflowRunStatus::Paused),
        )
        .unwrap();
    assert_eq!(
        exact_intersection
            .iter()
            .map(|run| run.id().as_str())
            .collect::<Vec<_>>(),
        ["bravo-match"]
    );
    assert!(
        reopened
            .list_filtered(
                WorkflowRunFilter::new()
                    .for_task(&task_id)
                    .for_workflow(&workflow_id)
                    .with_status(WorkflowRunStatus::Failed),
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn compound_filtered_listing_fails_closed_on_nonmatching_malformed_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("release-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("ship release").unwrap())
        .unwrap();
    let target = advancing_workflow();
    let workflow_id = target.id().clone();
    let other = RegisteredWorkflow::new(
        WorkflowId::new("other.workflow").unwrap(),
        target.start(),
        target.phases().to_vec(),
    );
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    runs.start_for_task(
        WorkflowRunId::new("valid-match").unwrap(),
        &task_id,
        &target,
    )
    .unwrap();
    runs.start(WorkflowRunId::new("nonmatching-malformed").unwrap(), &other)
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'workflow-run:nonmatching-malformed'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path).unwrap().list_filtered(
            WorkflowRunFilter::new()
                .for_task(&task_id)
                .for_workflow(&workflow_id)
                .with_status(WorkflowRunStatus::Active),
        ),
        Err(WorkflowRunStoreError::Replay(
            ReplayError::MalformedPayload { .. }
        ))
    ));
}

#[test]
fn status_filtered_listing_fails_closed_on_unrelated_malformed_run_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    runs.start(
        WorkflowRunId::new("valid-active").unwrap(),
        &advancing_workflow(),
    )
    .unwrap();
    runs.start(
        WorkflowRunId::new("unrelated-malformed").unwrap(),
        &advancing_workflow(),
    )
    .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'workflow-run:unrelated-malformed'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .list_by_status(WorkflowRunStatus::Active)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

#[test]
fn workflow_filtered_listing_fails_closed_on_unrelated_malformed_run_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let workflow_id = WorkflowId::new("release.workflow").unwrap();
    let target = advancing_workflow();
    let unrelated = RegisteredWorkflow::new(
        WorkflowId::new("unrelated.workflow").unwrap(),
        target.start(),
        target.phases().to_vec(),
    );
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    runs.start(WorkflowRunId::new("valid-workflow").unwrap(), &target)
        .unwrap();
    runs.start(
        WorkflowRunId::new("unrelated-malformed").unwrap(),
        &unrelated,
    )
    .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'workflow-run:unrelated-malformed'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .list_for_workflow(&workflow_id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

#[test]
fn task_attributed_start_rejects_missing_and_terminal_tasks_atomically() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let missing_task = TaskId::new("missing-task").unwrap();
    let missing_run = WorkflowRunId::new("missing-run").unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    assert!(matches!(
        runs.start_for_task(missing_run.clone(), &missing_task, &advancing_workflow()),
        Err(WorkflowRunStoreError::TaskNotFound { .. })
    ));
    assert!(runs.load(&missing_run).unwrap().is_none());

    let task_id = TaskId::new("completed-task").unwrap();
    let terminal_run = WorkflowRunId::new("terminal-run").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("done already").unwrap())
        .unwrap();
    tasks
        .complete(&task_id, TaskOutput::new("done").unwrap())
        .unwrap();
    assert!(matches!(
        runs.start_for_task(terminal_run.clone(), &task_id, &advancing_workflow()),
        Err(WorkflowRunStoreError::TaskNotActive {
            status: TaskStatus::Completed,
            ..
        })
    ));
    assert!(runs.load(&terminal_run).unwrap().is_none());
}

#[test]
fn task_filtered_listing_fails_closed_on_unrelated_malformed_run_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("valid-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("query runs").unwrap())
        .unwrap();
    let mut runs = WorkflowRunStore::open(&path).unwrap();
    runs.start_for_task(
        WorkflowRunId::new("valid-attribution").unwrap(),
        &task_id,
        &advancing_workflow(),
    )
    .unwrap();
    runs.start(
        WorkflowRunId::new("unrelated-malformed").unwrap(),
        &advancing_workflow(),
    )
    .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'workflow-run:unrelated-malformed'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .list_for_task(&task_id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

#[test]
fn task_attributed_start_rejects_a_malformed_persisted_task_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("valid-task").unwrap();
    let run_id = WorkflowRunId::new("malformed-attribution").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("validate replay").unwrap())
        .unwrap();
    WorkflowRunStore::open(&path)
        .unwrap()
        .start_for_task(run_id.clone(), &task_id, &advancing_workflow())
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.task_id', '') AS BLOB) WHERE stream_id = 'workflow-run:malformed-attribution'",
            [],
        )
        .unwrap();

    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&run_id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .list_for_task(&task_id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

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
fn queries_exact_typed_lifecycle_history_after_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-history").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let review = store.advance(&id, started.revision(), 0, None).unwrap();
    let paused = store
        .pause(
            &id,
            review.revision(),
            WorkflowRunPauseReason::new(" waiting for operator \n").unwrap(),
        )
        .unwrap();
    let resumed = store
        .resume(
            &id,
            paused.revision(),
            WorkflowRunResumeReason::new(" operator approved \t").unwrap(),
        )
        .unwrap();
    store
        .cancel(
            &id,
            resumed.revision(),
            WorkflowRunCancellation::new(" deployment withdrawn ").unwrap(),
        )
        .unwrap();
    drop(store);

    let history = WorkflowRunStore::open(&path)
        .unwrap()
        .history(&id)
        .unwrap()
        .unwrap();
    assert_eq!(
        history
            .iter()
            .map(|entry| entry.revision())
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(matches!(
        history[0].event(),
        WorkflowRunHistoryEvent::Started {
            workflow_id,
            phase_id,
        } if workflow_id.as_str() == "release.workflow" && phase_id == "plan"
    ));
    assert!(matches!(
        history[1].event(),
        WorkflowRunHistoryEvent::Advanced {
            source_phase_id,
            target_phase_id,
            transition_index: 0,
            gate_acknowledgement: None,
        } if source_phase_id == "plan" && target_phase_id == "review"
    ));
    assert!(matches!(
        history[2].event(),
        WorkflowRunHistoryEvent::Paused { phase_id, reason }
            if phase_id == "review" && reason.as_str() == " waiting for operator \n"
    ));
    assert!(matches!(
        history[3].event(),
        WorkflowRunHistoryEvent::Resumed { phase_id, reason }
            if phase_id == "review" && reason.as_str() == " operator approved \t"
    ));
    assert!(matches!(
        history[4].event(),
        WorkflowRunHistoryEvent::Cancelled { phase_id, reason }
            if phase_id == "review" && reason.as_str() == " deployment withdrawn "
    ));
}

#[test]
fn history_preserves_exact_gate_evidence_and_missing_run_semantics() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("gated-history").unwrap();
    let missing = WorkflowRunId::new("missing-history").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    assert!(store.history(&missing).unwrap().is_none());
    let started = store.start(id.clone(), &workflow("plan")).unwrap();
    store
        .advance(&id, started.revision(), 0, Some("plan.approved"))
        .unwrap();

    let history = store.history(&id).unwrap().unwrap();
    assert!(matches!(
        history[1].event(),
        WorkflowRunHistoryEvent::Advanced {
            source_phase_id,
            target_phase_id,
            transition_index: 0,
            gate_acknowledgement: Some(gate),
        } if source_phase_id == "plan" && target_phase_id == "done" && gate == "plan.approved"
    ));
}

#[test]
fn history_rejects_corrupt_payloads_and_impossible_lifecycle_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let unsupported_id = WorkflowRunId::new("unsupported-history").unwrap();
    let impossible_id = WorkflowRunId::new("impossible-history").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    store
        .start(unsupported_id.clone(), &advancing_workflow())
        .unwrap();
    let started = store
        .start(impossible_id.clone(), &advancing_workflow())
        .unwrap();
    store
        .advance(&impossible_id, started.revision(), 0, None)
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET event_type = 'workflow_run.unknown' WHERE stream_id = 'workflow-run:unsupported-history'",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .history(&unsupported_id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::UnsupportedEvent { .. })
    ));

    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.target_phase_index', 0) AS BLOB) WHERE stream_id = 'workflow-run:impossible-history' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .history(&impossible_id)
            .unwrap_err(),
        WorkflowRunStoreError::InvalidHistory { .. }
    ));
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
fn failure_diagnostics_reject_empty_values_and_persist_exact_terminal_evidence() {
    assert_eq!(
        WorkflowRunFailure::new("").unwrap_err(),
        WorkflowRunFailureError
    );
    let failure = WorkflowRunFailure::new(" provider exhausted \n").unwrap();
    assert_eq!(failure.as_str(), " provider exhausted \n");

    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("failed-release").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let failed = store.fail(&id, started.revision(), failure).unwrap();

    assert_eq!(failed.revision(), 2);
    assert_eq!(failed.current_phase().id(), "plan");
    assert!(failed.is_failed());
    assert_eq!(failed.status(), WorkflowRunStatus::Failed);
    assert_eq!(
        failed.failure().map(WorkflowRunFailure::as_str),
        Some(" provider exhausted \n")
    );
    assert!(matches!(
        store.history(&id).unwrap().unwrap()[1].event(),
        WorkflowRunHistoryEvent::Failed { phase_id, failure }
            if phase_id == "plan" && failure.as_str() == " provider exhausted \n"
    ));
}

#[test]
fn failed_runs_reopen_with_pause_state_and_reject_every_later_mutation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("paused-failure").unwrap();
    let missing = WorkflowRunId::new("missing-failure").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    assert!(matches!(
        store
            .fail(&missing, 1, WorkflowRunFailure::new("missing").unwrap())
            .unwrap_err(),
        WorkflowRunStoreError::NotFound { .. }
    ));
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let paused = store
        .pause(
            &id,
            started.revision(),
            WorkflowRunPauseReason::new("await recovery").unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store
            .fail(
                &id,
                started.revision(),
                WorkflowRunFailure::new("stale").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::ConcurrentModification { .. }
    ));
    let failed = store
        .fail(
            &id,
            paused.revision(),
            WorkflowRunFailure::new("provider unavailable").unwrap(),
        )
        .unwrap();
    assert!(failed.is_paused());
    assert_eq!(failed.status(), WorkflowRunStatus::Failed);
    assert_eq!(failed.pause_reason().unwrap().as_str(), "await recovery");

    assert!(matches!(
        store.advance(&id, failed.revision(), 0, None).unwrap_err(),
        WorkflowRunStoreError::AlreadyFailed { .. }
    ));
    assert!(matches!(
        store
            .pause(
                &id,
                failed.revision(),
                WorkflowRunPauseReason::new("again").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyFailed { .. }
    ));
    assert!(matches!(
        store
            .resume(
                &id,
                failed.revision(),
                WorkflowRunResumeReason::new("again").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyFailed { .. }
    ));
    assert!(matches!(
        store
            .cancel(
                &id,
                failed.revision(),
                WorkflowRunCancellation::new("again").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyFailed { .. }
    ));
    assert!(matches!(
        store
            .fail(
                &id,
                failed.revision(),
                WorkflowRunFailure::new("again").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyFailed { .. }
    ));

    drop(store);
    let reopened = WorkflowRunStore::open(&path).unwrap().list().unwrap();
    assert_eq!(reopened.len(), 1);
    assert!(reopened[0].is_failed());
    assert!(reopened[0].is_paused());
    assert_eq!(reopened[0].status(), WorkflowRunStatus::Failed);
    assert_eq!(
        reopened[0].failure().unwrap().as_str(),
        "provider unavailable"
    );
}

#[test]
fn failure_rejects_cancelled_and_authored_terminal_runs_and_malformed_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = WorkflowRunStore::open(&path).unwrap();

    let cancelled_id = WorkflowRunId::new("cancelled-failure").unwrap();
    let cancelled = store
        .start(cancelled_id.clone(), &advancing_workflow())
        .unwrap();
    let cancelled = store
        .cancel(
            &cancelled_id,
            cancelled.revision(),
            WorkflowRunCancellation::new("stop").unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store
            .fail(
                &cancelled_id,
                cancelled.revision(),
                WorkflowRunFailure::new("late").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyCancelled { .. }
    ));

    let terminal_id = WorkflowRunId::new("terminal-failure").unwrap();
    let started = store.start(terminal_id.clone(), &workflow("plan")).unwrap();
    let terminal = store
        .advance(&terminal_id, started.revision(), 0, Some("plan.approved"))
        .unwrap();
    assert!(matches!(
        store
            .fail(
                &terminal_id,
                terminal.revision(),
                WorkflowRunFailure::new("late").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyTerminal { .. }
    ));

    let corrupt_id = WorkflowRunId::new("corrupt-failure").unwrap();
    let started = store
        .start(corrupt_id.clone(), &advancing_workflow())
        .unwrap();
    store
        .fail(
            &corrupt_id,
            started.revision(),
            WorkflowRunFailure::new("broken").unwrap(),
        )
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.source_phase_index', 1) AS BLOB) WHERE stream_id = 'workflow-run:corrupt-failure' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .history(&corrupt_id)
            .unwrap_err(),
        WorkflowRunStoreError::InvalidHistory { .. }
    ));
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.source_phase_index', 0, '$.failure', '') AS BLOB) WHERE stream_id = 'workflow-run:corrupt-failure' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&corrupt_id)
            .unwrap_err(),
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.failure', 'broken') AS BLOB) WHERE stream_id = 'workflow-run:corrupt-failure' AND stream_version = 2",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events (stream_id, stream_version, event_type, payload_version, payload) SELECT stream_id, 3, event_type, payload_version, payload FROM events WHERE stream_id = 'workflow-run:corrupt-failure' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .load(&corrupt_id)
            .unwrap_err(),
        WorkflowRunStoreError::InvalidHistory { event_count: 3 }
    ));
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
    assert_eq!(runs[0].status(), WorkflowRunStatus::Active);
    assert_eq!(runs[1].revision(), 2);
    assert_eq!(
        runs[1].cancellation().map(WorkflowRunCancellation::as_str),
        Some("operator stop")
    );
    assert_eq!(runs[1].status(), WorkflowRunStatus::Cancelled);
    assert_eq!(runs[2].revision(), 3);
    assert_eq!(runs[2].current_phase().id(), "done");
    assert!(runs[2].is_terminal());
    assert_eq!(runs[2].status(), WorkflowRunStatus::AuthoredTerminal);
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
    assert_eq!(loaded.status(), WorkflowRunStatus::Active);
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
    assert!(matches!(
        WorkflowRunStore::open(&path)
            .unwrap()
            .history(&id)
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
    assert_eq!(done.status(), WorkflowRunStatus::AuthoredTerminal);

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
    assert_eq!(loaded.status(), WorkflowRunStatus::Cancelled);
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

#[test]
fn pause_and_resume_reasons_reject_empty_values_and_preserve_exact_text() {
    assert_eq!(
        WorkflowRunPauseReason::new("").unwrap_err(),
        WorkflowRunPauseReasonError
    );
    assert_eq!(
        WorkflowRunResumeReason::new("").unwrap_err(),
        WorkflowRunResumeReasonError
    );
    assert_eq!(
        WorkflowRunPauseReason::new(" \n ").unwrap().as_str(),
        " \n "
    );
    assert_eq!(
        WorkflowRunResumeReason::new("operator ready")
            .unwrap()
            .as_str(),
        "operator ready"
    );
}

#[test]
fn pauses_and_resumes_an_exact_revision_without_changing_phase_or_topology() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("release-pause").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let pause_reason = WorkflowRunPauseReason::new("await operator").unwrap();
    let paused = store
        .pause(&id, started.revision(), pause_reason.clone())
        .unwrap();
    assert_eq!(paused.revision(), 2);
    assert_eq!(paused.current_phase().id(), "plan");
    assert_eq!(paused.workflow(), started.workflow());
    assert!(paused.is_paused());
    assert_eq!(paused.status(), WorkflowRunStatus::Paused);
    assert_eq!(paused.pause_reason(), Some(&pause_reason));
    assert!(matches!(
        store.advance(&id, paused.revision(), 0, None).unwrap_err(),
        WorkflowRunStoreError::Paused { .. }
    ));

    drop(store);
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let listed = store.list().unwrap();
    assert_eq!(listed[0].pause_reason(), Some(&pause_reason));
    let resumed = store
        .resume(
            &id,
            listed[0].revision(),
            WorkflowRunResumeReason::new("operator ready").unwrap(),
        )
        .unwrap();
    assert_eq!(resumed.revision(), 3);
    assert_eq!(resumed.current_phase().id(), "plan");
    assert!(!resumed.is_paused());
    assert_eq!(resumed.status(), WorkflowRunStatus::Active);
    assert_eq!(resumed.pause_reason(), None);
    assert_eq!(
        store
            .advance(&id, resumed.revision(), 0, None)
            .unwrap()
            .current_phase()
            .id(),
        "review"
    );
}

#[test]
fn pause_resume_failures_are_atomic_and_paused_runs_remain_cancellable() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("pause-state").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let pause = WorkflowRunPauseReason::new("hold").unwrap();
    assert!(matches!(
        store
            .resume(
                &id,
                started.revision(),
                WorkflowRunResumeReason::new("no hold").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::NotPaused { .. }
    ));
    let paused = store.pause(&id, started.revision(), pause.clone()).unwrap();
    assert!(matches!(
        store.pause(&id, paused.revision(), pause).unwrap_err(),
        WorkflowRunStoreError::AlreadyPaused { .. }
    ));
    assert!(matches!(
        store
            .resume(
                &id,
                started.revision(),
                WorkflowRunResumeReason::new("stale").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::ConcurrentModification { .. }
    ));
    let cancelled = store
        .cancel(
            &id,
            paused.revision(),
            WorkflowRunCancellation::new("stop while held").unwrap(),
        )
        .unwrap();
    assert!(cancelled.is_paused());
    assert_eq!(cancelled.status(), WorkflowRunStatus::Cancelled);
    assert!(matches!(
        store
            .resume(
                &id,
                cancelled.revision(),
                WorkflowRunResumeReason::new("too late").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyCancelled { .. }
    ));

    let terminal_id = WorkflowRunId::new("pause-terminal").unwrap();
    let terminal_started = store
        .start(terminal_id.clone(), &advancing_workflow())
        .unwrap();
    let terminal_review = store
        .advance(&terminal_id, terminal_started.revision(), 0, None)
        .unwrap();
    let terminal = store
        .advance(
            &terminal_id,
            terminal_review.revision(),
            0,
            Some("release.approved"),
        )
        .unwrap();
    assert!(matches!(
        store
            .pause(
                &terminal_id,
                terminal.revision(),
                WorkflowRunPauseReason::new("impossible").unwrap()
            )
            .unwrap_err(),
        WorkflowRunStoreError::AlreadyTerminal { .. }
    ));
}

#[test]
fn malformed_pause_and_resume_histories_fail_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = WorkflowRunId::new("pause-history").unwrap();
    let mut store = WorkflowRunStore::open(&path).unwrap();
    let started = store.start(id.clone(), &advancing_workflow()).unwrap();
    let paused = store
        .pause(
            &id,
            started.revision(),
            WorkflowRunPauseReason::new("hold").unwrap(),
        )
        .unwrap();
    store
        .resume(
            &id,
            paused.revision(),
            WorkflowRunResumeReason::new("continue").unwrap(),
        )
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.source_phase_index', 1) AS BLOB) WHERE stream_id = 'workflow-run:pause-history' AND stream_version = 3",
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
            "UPDATE events SET payload = CAST(json_set(payload, '$.source_phase_index', 0, '$.reason', '') AS BLOB) WHERE stream_id = 'workflow-run:pause-history' AND stream_version = 2",
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
