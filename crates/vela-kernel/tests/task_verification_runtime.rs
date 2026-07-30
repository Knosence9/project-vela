use std::{cell::RefCell, error::Error, io, path::Path, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        AssistantProvider, AssistantRuntime, ProviderError, RuntimeError, TaskVerificationRequest,
        TaskVerifier, TaskVerifierError,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurn, SessionTurnContent},
    task::{
        TaskFailure, TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText,
        TaskStatus, TaskStore, TaskStoreError,
    },
};

struct PanicProvider;

impl AssistantProvider for PanicProvider {
    fn complete(
        &mut self,
        _transcript: &[SessionTurn],
    ) -> Result<SessionTurnContent, ProviderError> {
        panic!("verification must not invoke the assistant provider")
    }
}

fn create_active_task(path: &Path, task_id: &TaskId, session_id: &SessionId) -> TaskObservationId {
    SessionStore::open(path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Verification session").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("verify exact work").unwrap())
        .unwrap();
    tasks.associate_session(task_id, session_id).unwrap();
    let attempt_id = TaskObservationId::new("attempt-1").unwrap();
    tasks
        .append_observation(
            task_id,
            attempt_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("candidate result").unwrap(),
        )
        .unwrap();
    attempt_id
}

struct RecordingVerifier {
    calls: Rc<RefCell<Vec<(String, String, String)>>>,
    result: Result<String, TaskVerifierError>,
}

impl TaskVerifier for RecordingVerifier {
    fn verify(
        &mut self,
        request: TaskVerificationRequest<'_>,
    ) -> Result<String, TaskVerifierError> {
        self.calls.borrow_mut().push((
            request.task().id().as_str().to_owned(),
            request.attempt().id().as_str().to_owned(),
            request.attempt().text().as_str().to_owned(),
        ));
        self.result
            .as_ref()
            .map(Clone::clone)
            .map_err(|error| TaskVerifierError::new(io::Error::other(error.to_string())))
    }
}

#[test]
fn records_independently_observed_verification_for_the_exact_attempt() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = TaskId::new("verified-task").unwrap();
    let session_id = SessionId::new("verified-session").unwrap();
    let attempt_id = create_active_task(&path, &task_id, &session_id);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut verifier = RecordingVerifier {
        calls: Rc::clone(&calls),
        result: Ok("just verify passed".to_owned()),
    };
    let mut runtime = AssistantRuntime::open(&path, PanicProvider).unwrap();

    let task = runtime
        .verify_task_attempt(
            &task_id,
            &attempt_id,
            TaskObservationId::new("verification-1").unwrap(),
            &mut verifier,
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[(
            "verified-task".to_owned(),
            "attempt-1".to_owned(),
            "candidate result".to_owned(),
        )]
    );
    assert_eq!(task.status(), TaskStatus::Active);
    let verification = task.observations().last().unwrap();
    assert_eq!(verification.id().as_str(), "verification-1");
    assert_eq!(verification.kind(), TaskObservationKind::Verification);
    assert_eq!(verification.text().as_str(), "just verify passed");
    assert_eq!(verification.parent_attempt_id(), Some(&attempt_id));
    assert!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
}

#[test]
fn verifies_an_unassociated_task_without_creating_a_session() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = TaskId::new("unassociated-task").unwrap();
    let attempt_id = TaskObservationId::new("attempt-1").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("verify directly").unwrap())
        .unwrap();
    tasks
        .append_observation(
            &task_id,
            attempt_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("candidate").unwrap(),
        )
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut verifier = RecordingVerifier {
        calls: Rc::clone(&calls),
        result: Ok("checked".to_owned()),
    };
    let mut runtime = AssistantRuntime::open(&path, PanicProvider).unwrap();

    let task = runtime
        .verify_task_attempt(
            &task_id,
            &attempt_id,
            TaskObservationId::new("verification-1").unwrap(),
            &mut verifier,
        )
        .unwrap();

    assert_eq!(calls.borrow().len(), 1);
    assert!(task.session_id().is_none());
    assert_eq!(
        task.observations().last().unwrap().kind(),
        TaskObservationKind::Verification
    );
}

#[test]
fn rejects_invalid_verification_lineage_before_verifier_effects() {
    for case in [
        "missing-parent",
        "non-attempt-parent",
        "duplicate-id",
        "terminal",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let task_id = TaskId::new(format!("task-{case}")).unwrap();
        let session_id = SessionId::new(format!("session-{case}")).unwrap();
        let attempt_id = create_active_task(&path, &task_id, &session_id);
        let mut tasks = TaskStore::open(&path).unwrap();
        let parent_id = match case {
            "missing-parent" => TaskObservationId::new("missing").unwrap(),
            "non-attempt-parent" => {
                let id = TaskObservationId::new("diagnostic-parent").unwrap();
                tasks
                    .append_observation_for_attempt(
                        &task_id,
                        id.clone(),
                        TaskObservationKind::Diagnostic,
                        TaskObservationText::new("diagnosis").unwrap(),
                        attempt_id.clone(),
                    )
                    .unwrap();
                id
            }
            _ => attempt_id.clone(),
        };
        let verification_id = TaskObservationId::new("verification-1").unwrap();
        if case == "duplicate-id" {
            tasks
                .append_observation_for_attempt(
                    &task_id,
                    verification_id.clone(),
                    TaskObservationKind::Verification,
                    TaskObservationText::new("already checked").unwrap(),
                    attempt_id.clone(),
                )
                .unwrap();
        }
        if case == "terminal" {
            tasks
                .fail(&task_id, TaskFailure::new("stopped").unwrap())
                .unwrap();
        }
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut verifier = RecordingVerifier {
            calls: Rc::clone(&calls),
            result: Ok("must not run".to_owned()),
        };
        let mut runtime = AssistantRuntime::open(&path, PanicProvider).unwrap();

        let error = runtime
            .verify_task_attempt(&task_id, &parent_id, verification_id, &mut verifier)
            .unwrap_err();

        assert!(
            matches!(error, RuntimeError::Task(_)),
            "case {case}: {error}"
        );
        assert!(calls.borrow().is_empty(), "case {case}");
    }
}

#[test]
fn verifier_failure_or_blank_output_writes_no_verification() {
    for (case, result) in [
        (
            "failure",
            Err(TaskVerifierError::new(io::Error::other("checker offline"))),
        ),
        ("blank", Ok(" \n ".to_owned())),
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let task_id = TaskId::new(format!("task-{case}")).unwrap();
        let session_id = SessionId::new(format!("session-{case}")).unwrap();
        let attempt_id = create_active_task(&path, &task_id, &session_id);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut verifier = RecordingVerifier {
            calls: Rc::clone(&calls),
            result,
        };
        let mut runtime = AssistantRuntime::open(&path, PanicProvider).unwrap();

        let error = runtime
            .verify_task_attempt(
                &task_id,
                &attempt_id,
                TaskObservationId::new("verification-1").unwrap(),
                &mut verifier,
            )
            .unwrap_err();

        match case {
            "failure" => {
                assert!(matches!(&error, RuntimeError::Verifier(_)));
                assert_eq!(error.source().unwrap().to_string(), "checker offline");
            }
            "blank" => assert!(matches!(error, RuntimeError::InvalidVerificationText(_))),
            _ => unreachable!(),
        }
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(
            TaskStore::open(&path)
                .unwrap()
                .load(&task_id)
                .unwrap()
                .unwrap()
                .observations()
                .len(),
            1
        );
    }
}

struct RacingVerifier {
    path: std::path::PathBuf,
    task_id: TaskId,
    parent_attempt_id: TaskObservationId,
    observation_id: TaskObservationId,
    calls: usize,
}

impl TaskVerifier for RacingVerifier {
    fn verify(
        &mut self,
        _request: TaskVerificationRequest<'_>,
    ) -> Result<String, TaskVerifierError> {
        self.calls += 1;
        TaskStore::open(&self.path)
            .unwrap()
            .append_observation_for_attempt(
                &self.task_id,
                self.observation_id.clone(),
                TaskObservationKind::Diagnostic,
                TaskObservationText::new("racing evidence").unwrap(),
                self.parent_attempt_id.clone(),
            )
            .unwrap();
        Ok("stale verification".to_owned())
    }
}

#[test]
fn racing_evidence_is_authoritative_without_retrying_the_verifier() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = TaskId::new("racing-task").unwrap();
    let session_id = SessionId::new("racing-session").unwrap();
    let attempt_id = create_active_task(&path, &task_id, &session_id);
    let verification_id = TaskObservationId::new("verification-1").unwrap();
    let mut verifier = RacingVerifier {
        path: path.clone(),
        task_id: task_id.clone(),
        parent_attempt_id: attempt_id.clone(),
        observation_id: verification_id.clone(),
        calls: 0,
    };
    let mut runtime = AssistantRuntime::open(&path, PanicProvider).unwrap();

    let error = runtime
        .verify_task_attempt(
            &task_id,
            &attempt_id,
            verification_id.clone(),
            &mut verifier,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert_eq!(verifier.calls, 1);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 2);
    assert_eq!(task.observations()[1].id(), &verification_id);
    assert_eq!(
        task.observations()[1].kind(),
        TaskObservationKind::Diagnostic
    );
}
