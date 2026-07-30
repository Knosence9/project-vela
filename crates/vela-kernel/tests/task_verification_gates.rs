use tempfile::tempdir;
use vela_kernel::task::{
    TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText, TaskOutput,
    TaskStore, TaskVerificationCheck, TaskVerificationGateEvaluationError, TaskVerificationGateSet,
    TaskVerificationGateSetError, TaskVerificationGateStatus, TaskVerificationOutcome,
};

fn observation_id(value: &str) -> TaskObservationId {
    TaskObservationId::new(value).unwrap()
}

fn check(value: &str) -> TaskVerificationCheck {
    TaskVerificationCheck::new(value).unwrap()
}

fn text(value: &str) -> TaskObservationText {
    TaskObservationText::new(value).unwrap()
}

fn append_attempt(store: &mut TaskStore, task_id: &TaskId, id: &str) {
    store
        .append_observation(
            task_id,
            observation_id(id),
            TaskObservationKind::Attempt,
            text(id),
        )
        .unwrap();
}

fn append_verification(
    store: &mut TaskStore,
    task_id: &TaskId,
    id: &str,
    attempt_id: &str,
    check_id: &str,
    outcome: TaskVerificationOutcome,
) {
    store
        .append_verification_for_attempt(
            task_id,
            observation_id(id),
            outcome,
            check(check_id),
            text(id),
            observation_id(attempt_id),
        )
        .unwrap();
}

#[test]
fn gate_sets_require_unique_checks_and_preserve_authored_order() {
    assert_eq!(
        TaskVerificationGateSet::new(Vec::new()).unwrap_err(),
        TaskVerificationGateSetError::Empty
    );
    assert!(matches!(
        TaskVerificationGateSet::new(vec![check("quality"), check("quality")]).unwrap_err(),
        TaskVerificationGateSetError::Duplicate { ref check } if check.as_str() == "quality"
    ));

    let gates = TaskVerificationGateSet::new(vec![check("lint"), check("test")]).unwrap();
    let checks: Vec<_> = gates
        .checks()
        .iter()
        .map(TaskVerificationCheck::as_str)
        .collect();
    assert_eq!(checks, ["lint", "test"]);
}

#[test]
fn evaluates_latest_exact_attempt_results_with_failed_precedence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("gate-results").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(task_id.clone(), TaskGoal::new("Evaluate gates").unwrap())
        .unwrap();
    append_attempt(&mut store, &task_id, "attempt-1");
    append_verification(
        &mut store,
        &task_id,
        "lint-failed",
        "attempt-1",
        "lint",
        TaskVerificationOutcome::Failed,
    );
    append_verification(
        &mut store,
        &task_id,
        "lint-passed",
        "attempt-1",
        "lint",
        TaskVerificationOutcome::Passed,
    );
    append_verification(
        &mut store,
        &task_id,
        "tests-failed",
        "attempt-1",
        "tests",
        TaskVerificationOutcome::Failed,
    );

    let task = store.load(&task_id).unwrap().unwrap();
    let gates =
        TaskVerificationGateSet::new(vec![check("lint"), check("tests"), check("security")])
            .unwrap();
    let report = task
        .evaluate_verification_gates(&observation_id("attempt-1"), &gates)
        .unwrap();

    assert_eq!(report.status(), TaskVerificationGateStatus::Failed);
    assert_eq!(report.gates().len(), 3);
    assert_eq!(report.gates()[0].check().as_str(), "lint");
    assert_eq!(
        report.gates()[0].status(),
        TaskVerificationGateStatus::Passed
    );
    assert_eq!(report.gates()[1].check().as_str(), "tests");
    assert_eq!(
        report.gates()[1].status(),
        TaskVerificationGateStatus::Failed
    );
    assert_eq!(report.gates()[2].check().as_str(), "security");
    assert_eq!(
        report.gates()[2].status(),
        TaskVerificationGateStatus::Pending
    );
}

#[test]
fn missing_gate_is_pending_and_unrelated_or_legacy_evidence_does_not_satisfy_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("pending-gates").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(
            task_id.clone(),
            TaskGoal::new("Keep provenance exact").unwrap(),
        )
        .unwrap();
    append_attempt(&mut store, &task_id, "attempt-1");
    append_attempt(&mut store, &task_id, "attempt-2");
    append_verification(
        &mut store,
        &task_id,
        "other-attempt-quality",
        "attempt-2",
        "quality",
        TaskVerificationOutcome::Passed,
    );
    store
        .append_observation_for_attempt(
            &task_id,
            observation_id("legacy-quality"),
            TaskObservationKind::Verification,
            text("legacy quality passed"),
            observation_id("attempt-1"),
        )
        .unwrap();
    append_verification(
        &mut store,
        &task_id,
        "lint-passed",
        "attempt-1",
        "lint",
        TaskVerificationOutcome::Passed,
    );

    let task = store.load(&task_id).unwrap().unwrap();
    let gates = TaskVerificationGateSet::new(vec![check("quality"), check("lint")]).unwrap();
    let report = task
        .evaluate_verification_gates(&observation_id("attempt-1"), &gates)
        .unwrap();

    assert_eq!(report.status(), TaskVerificationGateStatus::Pending);
    assert_eq!(
        report.gates()[0].status(),
        TaskVerificationGateStatus::Pending
    );
    assert_eq!(
        report.gates()[1].status(),
        TaskVerificationGateStatus::Passed
    );
}

#[test]
fn all_passed_gates_remain_readable_after_task_completion() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("completed-gates").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(
            task_id.clone(),
            TaskGoal::new("Finish after checks").unwrap(),
        )
        .unwrap();
    append_attempt(&mut store, &task_id, "attempt");
    append_verification(
        &mut store,
        &task_id,
        "quality-passed",
        "attempt",
        "quality",
        TaskVerificationOutcome::Passed,
    );
    let task = store
        .complete(&task_id, TaskOutput::new("done").unwrap())
        .unwrap();
    let gates = TaskVerificationGateSet::new(vec![check("quality")]).unwrap();

    let report = task
        .evaluate_verification_gates(&observation_id("attempt"), &gates)
        .unwrap();
    assert_eq!(report.status(), TaskVerificationGateStatus::Passed);
}

#[test]
fn rejects_missing_and_non_attempt_evaluation_parents() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("invalid-gate-parent").unwrap();
    let mut store = TaskStore::open(&path).unwrap();
    store
        .start(task_id.clone(), TaskGoal::new("Validate parent").unwrap())
        .unwrap();
    store
        .append_observation(
            &task_id,
            observation_id("diagnostic"),
            TaskObservationKind::Diagnostic,
            text("diagnostic"),
        )
        .unwrap();
    let task = store.load(&task_id).unwrap().unwrap();
    let gates = TaskVerificationGateSet::new(vec![check("quality")]).unwrap();

    assert!(matches!(
        task.evaluate_verification_gates(&observation_id("missing"), &gates),
        Err(TaskVerificationGateEvaluationError::AttemptNotFound { ref observation_id })
            if observation_id.as_str() == "missing"
    ));
    assert!(matches!(
        task.evaluate_verification_gates(&observation_id("diagnostic"), &gates),
        Err(TaskVerificationGateEvaluationError::ObservationNotAttempt {
            ref observation_id,
            kind: TaskObservationKind::Diagnostic,
        }) if observation_id.as_str() == "diagnostic"
    ));
}
