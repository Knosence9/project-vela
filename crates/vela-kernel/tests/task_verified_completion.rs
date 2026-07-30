use tempfile::tempdir;
use vela_kernel::task::{
    TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText, TaskOutput,
    TaskStatus, TaskStore, TaskStoreError, TaskVerificationCheck,
    TaskVerificationGateEvaluationError, TaskVerificationGateSet, TaskVerificationGateStatus,
    TaskVerificationOutcome, TaskVerifiedCompletionError,
};

fn observation_id(value: &str) -> TaskObservationId {
    TaskObservationId::new(value).unwrap()
}

fn text(value: &str) -> TaskObservationText {
    TaskObservationText::new(value).unwrap()
}

fn check(value: &str) -> TaskVerificationCheck {
    TaskVerificationCheck::new(value).unwrap()
}

fn gates() -> TaskVerificationGateSet {
    TaskVerificationGateSet::new(vec![check("lint"), check("tests")]).unwrap()
}

fn started_store(path: &std::path::Path, task_id: &TaskId) -> TaskStore {
    let mut store = TaskStore::open(path).unwrap();
    store
        .start(
            task_id.clone(),
            TaskGoal::new("Ship verified work").unwrap(),
        )
        .unwrap();
    store
        .append_observation(
            task_id,
            observation_id("attempt"),
            TaskObservationKind::Attempt,
            text("candidate output"),
        )
        .unwrap();
    store
}

fn verify(
    store: &mut TaskStore,
    task_id: &TaskId,
    id: &str,
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
            observation_id("attempt"),
        )
        .unwrap();
}

#[test]
fn completes_and_replays_exact_output_only_when_every_gate_passed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("verified-completion").unwrap();
    let mut store = started_store(&path, &task_id);
    verify(
        &mut store,
        &task_id,
        "lint-passed",
        "lint",
        TaskVerificationOutcome::Passed,
    );
    verify(
        &mut store,
        &task_id,
        "tests-passed",
        "tests",
        TaskVerificationOutcome::Passed,
    );
    let output = TaskOutput::new("release artifact").unwrap();

    let completed = store
        .complete_if_verification_gates_pass(
            &task_id,
            output.clone(),
            &observation_id("attempt"),
            &gates(),
        )
        .unwrap();

    assert_eq!(completed.status(), TaskStatus::Completed);
    assert_eq!(completed.output(), Some(&output));
    drop(store);
    let reopened = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status(), TaskStatus::Completed);
    assert_eq!(reopened.output(), Some(&output));
}

#[test]
fn pending_and_failed_gates_return_the_ordered_report_without_writing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("blocked-completion").unwrap();
    let mut store = started_store(&path, &task_id);
    verify(
        &mut store,
        &task_id,
        "lint-passed",
        "lint",
        TaskVerificationOutcome::Passed,
    );

    let pending = store
        .complete_if_verification_gates_pass(
            &task_id,
            TaskOutput::new("too early").unwrap(),
            &observation_id("attempt"),
            &gates(),
        )
        .unwrap_err();
    match pending {
        TaskVerifiedCompletionError::GatesPending { report } => {
            assert_eq!(report.status(), TaskVerificationGateStatus::Pending);
            assert_eq!(report.gates()[0].check().as_str(), "lint");
            assert_eq!(report.gates()[1].check().as_str(), "tests");
        }
        error => panic!("unexpected pending error: {error:?}"),
    }

    verify(
        &mut store,
        &task_id,
        "tests-failed",
        "tests",
        TaskVerificationOutcome::Failed,
    );
    let failed = store
        .complete_if_verification_gates_pass(
            &task_id,
            TaskOutput::new("still blocked").unwrap(),
            &observation_id("attempt"),
            &gates(),
        )
        .unwrap_err();
    assert!(matches!(
        failed,
        TaskVerifiedCompletionError::GatesFailed { ref report }
            if report.status() == TaskVerificationGateStatus::Failed
    ));

    let task = store.load(&task_id).unwrap().unwrap();
    assert_eq!(task.status(), TaskStatus::Active);
    assert_eq!(task.output(), None);
}

#[test]
fn invalid_attempt_identity_preserves_the_typed_evaluation_error_without_writing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("invalid-attempt-completion").unwrap();
    let mut store = started_store(&path, &task_id);
    store
        .append_observation(
            &task_id,
            observation_id("diagnostic"),
            TaskObservationKind::Diagnostic,
            text("not an attempt"),
        )
        .unwrap();

    let missing = store
        .complete_if_verification_gates_pass(
            &task_id,
            TaskOutput::new("missing").unwrap(),
            &observation_id("missing"),
            &gates(),
        )
        .unwrap_err();
    assert!(matches!(
        missing,
        TaskVerifiedCompletionError::GateEvaluation(
            TaskVerificationGateEvaluationError::AttemptNotFound { ref observation_id }
        ) if observation_id.as_str() == "missing"
    ));

    let non_attempt = store
        .complete_if_verification_gates_pass(
            &task_id,
            TaskOutput::new("wrong kind").unwrap(),
            &observation_id("diagnostic"),
            &gates(),
        )
        .unwrap_err();
    assert!(matches!(
        non_attempt,
        TaskVerifiedCompletionError::GateEvaluation(
            TaskVerificationGateEvaluationError::ObservationNotAttempt {
                ref observation_id,
                kind: TaskObservationKind::Diagnostic,
            }
        ) if observation_id.as_str() == "diagnostic"
    ));
    assert_eq!(
        store.load(&task_id).unwrap().unwrap().status(),
        TaskStatus::Active
    );
}

#[test]
fn authoritative_terminal_state_is_preserved() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("terminal-before-guard").unwrap();
    let mut store = started_store(&path, &task_id);
    verify(
        &mut store,
        &task_id,
        "lint-passed",
        "lint",
        TaskVerificationOutcome::Passed,
    );
    verify(
        &mut store,
        &task_id,
        "tests-passed",
        "tests",
        TaskVerificationOutcome::Passed,
    );
    store
        .complete(&task_id, TaskOutput::new("winner").unwrap())
        .unwrap();

    let error = store
        .complete_if_verification_gates_pass(
            &task_id,
            TaskOutput::new("loser").unwrap(),
            &observation_id("attempt"),
            &gates(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        TaskVerifiedCompletionError::Store(TaskStoreError::AlreadyCompleted { .. })
    ));
    assert_eq!(
        store
            .load(&task_id)
            .unwrap()
            .unwrap()
            .output()
            .unwrap()
            .as_str(),
        "winner"
    );
}
