use std::{cell::RefCell, error::Error, fmt, path::PathBuf, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{AssistantProvider, AssistantRuntime, ProviderError, RuntimeError},
    session::{
        SessionClosure, SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole,
    },
    task::{
        TaskCancellation, TaskFailure, TaskGoal, TaskId, TaskObservationId, TaskObservationKind,
        TaskObservationText, TaskOutput, TaskStore, TaskStoreError,
    },
};

type RecordedTranscript = Vec<(SessionTurnRole, String)>;
type SharedCalls = Rc<RefCell<Vec<RecordedTranscript>>>;

#[derive(Clone)]
struct FakeProvider {
    calls: SharedCalls,
    result: Result<SessionTurnContent, FakeProviderFailure>,
}

impl AssistantProvider for FakeProvider {
    fn complete(
        &mut self,
        transcript: &[vela_kernel::session::SessionTurn],
    ) -> Result<SessionTurnContent, ProviderError> {
        self.calls.borrow_mut().push(
            transcript
                .iter()
                .map(|turn| (turn.role(), turn.content().as_str().to_owned()))
                .collect(),
        );
        self.result.clone().map_err(ProviderError::new)
    }
}

#[derive(Clone, Debug)]
struct FakeProviderFailure;

impl fmt::Display for FakeProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider unavailable")
    }
}

impl Error for FakeProviderFailure {}

struct ClosingProvider {
    path: PathBuf,
    session_id: SessionId,
    calls: Rc<RefCell<usize>>,
}

impl AssistantProvider for ClosingProvider {
    fn complete(
        &mut self,
        _transcript: &[vela_kernel::session::SessionTurn],
    ) -> Result<SessionTurnContent, ProviderError> {
        *self.calls.borrow_mut() += 1;
        SessionStore::open(&self.path)
            .unwrap()
            .close(
                &self.session_id,
                SessionClosure::new("closed during provider call").unwrap(),
            )
            .unwrap();
        Ok(SessionTurnContent::new("cannot be appended").unwrap())
    }
}

struct CompletingTaskProvider {
    path: PathBuf,
    task_id: TaskId,
}

impl AssistantProvider for CompletingTaskProvider {
    fn complete(
        &mut self,
        _transcript: &[vela_kernel::session::SessionTurn],
    ) -> Result<SessionTurnContent, ProviderError> {
        TaskStore::open(&self.path)
            .unwrap()
            .complete(&self.task_id, TaskOutput::new("won the race").unwrap())
            .unwrap();
        Ok(SessionTurnContent::new("late correction").unwrap())
    }
}

struct AppendingAttemptProvider {
    path: PathBuf,
    task_id: TaskId,
    observation_id: TaskObservationId,
}

impl AssistantProvider for AppendingAttemptProvider {
    fn complete(
        &mut self,
        _transcript: &[vela_kernel::session::SessionTurn],
    ) -> Result<SessionTurnContent, ProviderError> {
        TaskStore::open(&self.path)
            .unwrap()
            .append_observation(
                &self.task_id,
                self.observation_id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new("racing attempt").unwrap(),
            )
            .unwrap();
        Ok(SessionTurnContent::new("late final answer").unwrap())
    }
}

#[test]
fn executes_an_associated_task_turn_and_persists_attempt_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Task runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("answer carefully").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("grounded answer").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let outcome = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![(SessionTurnRole::Human, "question".to_owned())]]
    );
    assert_eq!(outcome.session().turns().len(), 2);
    assert_eq!(outcome.task().observations().len(), 1);
    assert_eq!(
        outcome.task().observations()[0].kind(),
        TaskObservationKind::Attempt
    );
    assert_eq!(outcome.task().observations()[0].id().as_str(), "attempt-1");
    assert_eq!(
        outcome.task().observations()[0].text().as_str(),
        "grounded answer"
    );

    let reopened_session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    let reopened_task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(reopened_session, *outcome.session());
    assert_eq!(reopened_task, *outcome.task());
}

#[test]
fn invalid_task_state_fails_before_transcript_write_or_provider_call() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Task runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    let unassociated = TaskId::new("unassociated").unwrap();
    tasks
        .start(unassociated.clone(), TaskGoal::new("unassociated").unwrap())
        .unwrap();
    let completed = TaskId::new("completed").unwrap();
    tasks
        .start(completed.clone(), TaskGoal::new("completed").unwrap())
        .unwrap();
    tasks.associate_session(&completed, &session_id).unwrap();
    tasks
        .complete(&completed, TaskOutput::new("done").unwrap())
        .unwrap();
    let cancelled = TaskId::new("cancelled").unwrap();
    tasks
        .start(cancelled.clone(), TaskGoal::new("cancelled").unwrap())
        .unwrap();
    tasks.associate_session(&cancelled, &session_id).unwrap();
    tasks
        .cancel(&cancelled, TaskCancellation::new("not needed").unwrap())
        .unwrap();
    let failed = TaskId::new("failed").unwrap();
    tasks
        .start(failed.clone(), TaskGoal::new("failed").unwrap())
        .unwrap();
    tasks.associate_session(&failed, &session_id).unwrap();
    tasks
        .fail(&failed, TaskFailure::new("could not complete").unwrap())
        .unwrap();
    let closed_session_task = TaskId::new("closed-session").unwrap();
    tasks
        .start(
            closed_session_task.clone(),
            TaskGoal::new("closed session").unwrap(),
        )
        .unwrap();
    tasks
        .associate_session(&closed_session_task, &session_id)
        .unwrap();
    drop(tasks);
    SessionStore::open(&path)
        .unwrap()
        .close(
            &session_id,
            SessionClosure::new("closed before runtime").unwrap(),
        )
        .unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("unused").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let missing = TaskId::new("missing").unwrap();
    assert!(matches!(
        runtime.execute_task_turn(
            &missing,
            SessionTurnContent::new("missing").unwrap(),
            TaskObservationId::new("missing-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::NotFound { .. }))
    ));
    assert!(matches!(
        runtime.execute_task_turn(
            &completed,
            SessionTurnContent::new("completed").unwrap(),
            TaskObservationId::new("completed-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. }))
    ));
    assert!(matches!(
        runtime.execute_task_turn(
            &cancelled,
            SessionTurnContent::new("cancelled").unwrap(),
            TaskObservationId::new("cancelled-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCancelled { .. }))
    ));
    assert!(matches!(
        runtime.execute_task_turn(
            &failed,
            SessionTurnContent::new("failed").unwrap(),
            TaskObservationId::new("failed-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyFailed { .. }))
    ));
    assert!(matches!(
        runtime.execute_task_turn(
            &unassociated,
            SessionTurnContent::new("unassociated").unwrap(),
            TaskObservationId::new("unassociated-attempt").unwrap(),
        ),
        Err(RuntimeError::TaskNotAssociated { .. })
    ));
    assert!(matches!(
        runtime.execute_task_turn(
            &closed_session_task,
            SessionTurnContent::new("closed").unwrap(),
            TaskObservationId::new("closed-attempt").unwrap(),
        ),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert!(matches!(
        runtime.complete_task_turn(
            &missing,
            SessionTurnContent::new("missing completion").unwrap(),
            TaskObservationId::new("missing-final-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::NotFound { .. }))
    ));
    assert!(matches!(
        runtime.complete_task_turn(
            &completed,
            SessionTurnContent::new("completed completion").unwrap(),
            TaskObservationId::new("completed-final-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. }))
    ));
    assert!(matches!(
        runtime.complete_task_turn(
            &cancelled,
            SessionTurnContent::new("cancelled completion").unwrap(),
            TaskObservationId::new("cancelled-final-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCancelled { .. }))
    ));
    assert!(matches!(
        runtime.complete_task_turn(
            &failed,
            SessionTurnContent::new("failed completion").unwrap(),
            TaskObservationId::new("failed-final-attempt").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyFailed { .. }))
    ));
    assert!(matches!(
        runtime.complete_task_turn(
            &unassociated,
            SessionTurnContent::new("unassociated completion").unwrap(),
            TaskObservationId::new("unassociated-final-attempt").unwrap(),
        ),
        Err(RuntimeError::TaskNotAssociated { .. })
    ));
    assert!(matches!(
        runtime.complete_task_turn(
            &closed_session_task,
            SessionTurnContent::new("closed completion").unwrap(),
            TaskObservationId::new("closed-final-attempt").unwrap(),
        ),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert!(calls.borrow().is_empty());
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
fn provider_failure_preserves_human_turn_without_task_observation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Task runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("fail safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let provider = FakeProvider {
        calls: Rc::new(RefCell::new(Vec::new())),
        result: Err(FakeProviderFailure),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    assert!(matches!(
        runtime.execute_task_turn(
            &task_id,
            SessionTurnContent::new("durable question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
        ),
        Err(RuntimeError::Provider(_))
    ));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .observations()
            .is_empty()
    );
}

#[test]
fn assistant_append_failure_preserves_only_the_human_turn_and_no_observation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Task runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(
            task_id.clone(),
            TaskGoal::new("fail after provider").unwrap(),
        )
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(0));
    let provider = ClosingProvider {
        path: path.clone(),
        session_id: session_id.clone(),
        calls: Rc::clone(&calls),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    assert!(matches!(
        runtime.execute_task_turn(
            &task_id,
            SessionTurnContent::new("durable question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
        ),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert_eq!(*calls.borrow(), 1);
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .observations()
            .is_empty()
    );
}

#[test]
fn observation_failure_preserves_both_transcript_turns_and_exposes_task_source() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Task runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("record attempt").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    tasks
        .append_observation(
            &task_id,
            TaskObservationId::new("duplicate").unwrap(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("prior attempt").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let provider = FakeProvider {
        calls: Rc::new(RefCell::new(Vec::new())),
        result: Ok(SessionTurnContent::new("new answer").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let error = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("new question").unwrap(),
            TaskObservationId::new("duplicate").unwrap(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert!(error.source().unwrap().is::<TaskStoreError>());
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].role(), SessionTurnRole::Assistant);
    assert_eq!(session.turns()[1].content().as_str(), "new answer");
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "prior attempt");
}

#[test]
fn executes_a_task_correction_turn_and_persists_parented_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Correction runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(
            task_id.clone(),
            TaskGoal::new("improve the answer").unwrap(),
        )
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let parent_id = TaskObservationId::new("attempt-1").unwrap();
    tasks
        .append_observation(
            &task_id,
            parent_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("first answer").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("corrected answer").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let outcome = runtime
        .execute_task_correction_turn(
            &task_id,
            &parent_id,
            SessionTurnContent::new("please correct that").unwrap(),
            TaskObservationId::new("correction-1").unwrap(),
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![(
            SessionTurnRole::Human,
            "please correct that".to_owned()
        )]]
    );
    assert_eq!(outcome.session().turns().len(), 2);
    let correction = &outcome.task().observations()[1];
    assert_eq!(correction.id().as_str(), "correction-1");
    assert_eq!(correction.kind(), TaskObservationKind::Correction);
    assert_eq!(correction.text().as_str(), "corrected answer");
    assert_eq!(correction.parent_attempt_id(), Some(&parent_id));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        *outcome.session()
    );
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        *outcome.task()
    );
}

#[test]
fn correction_preflight_rejects_invalid_evidence_without_transcript_or_provider_call() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Correction preflight").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("validate evidence").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let parent_id = TaskObservationId::new("attempt-1").unwrap();
    tasks
        .append_observation(
            &task_id,
            parent_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("first answer").unwrap(),
        )
        .unwrap();
    let diagnostic_id = TaskObservationId::new("diagnostic-1").unwrap();
    tasks
        .append_observation(
            &task_id,
            diagnostic_id.clone(),
            TaskObservationKind::Diagnostic,
            TaskObservationText::new("problem").unwrap(),
        )
        .unwrap();
    let duplicate_id = TaskObservationId::new("duplicate").unwrap();
    tasks
        .append_observation(
            &task_id,
            duplicate_id.clone(),
            TaskObservationKind::Diagnostic,
            TaskObservationText::new("existing").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("unused").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    assert!(matches!(
        runtime.execute_task_correction_turn(
            &task_id,
            &parent_id,
            SessionTurnContent::new("duplicate").unwrap(),
            duplicate_id,
        ),
        Err(RuntimeError::Task(
            TaskStoreError::DuplicateObservation { .. }
        ))
    ));
    assert!(matches!(
        runtime.execute_task_correction_turn(
            &task_id,
            &TaskObservationId::new("missing").unwrap(),
            SessionTurnContent::new("missing parent").unwrap(),
            TaskObservationId::new("correction-2").unwrap(),
        ),
        Err(RuntimeError::Task(
            TaskStoreError::ParentObservationNotFound { .. }
        ))
    ));
    assert!(matches!(
        runtime.execute_task_correction_turn(
            &task_id,
            &diagnostic_id,
            SessionTurnContent::new("wrong parent kind").unwrap(),
            TaskObservationId::new("correction-3").unwrap(),
        ),
        Err(RuntimeError::Task(
            TaskStoreError::ParentObservationNotAttempt { .. }
        ))
    ));
    assert!(calls.borrow().is_empty());
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
fn invalid_correction_text_preserves_transcript_and_exposes_source() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Invalid correction").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("correct safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let parent_id = TaskObservationId::new("attempt-1").unwrap();
    tasks
        .append_observation(
            &task_id,
            parent_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("first answer").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let provider = FakeProvider {
        calls: Rc::new(RefCell::new(Vec::new())),
        result: Ok(SessionTurnContent::new(" \n ").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();
    let error = runtime
        .execute_task_correction_turn(
            &task_id,
            &parent_id,
            SessionTurnContent::new("correct this").unwrap(),
            TaskObservationId::new("correction-1").unwrap(),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::InvalidCorrectionText(_)));
    assert!(
        error
            .source()
            .unwrap()
            .is::<vela_kernel::task::TaskObservationTextError>()
    );
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
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

#[test]
fn correction_append_race_preserves_transcript_and_reports_winning_terminal_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Correction race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let parent_id = TaskObservationId::new("attempt-1").unwrap();
    tasks
        .append_observation(
            &task_id,
            parent_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("first answer").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let provider = CompletingTaskProvider {
        path: path.clone(),
        task_id: task_id.clone(),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    assert!(matches!(
        runtime.execute_task_correction_turn(
            &task_id,
            &parent_id,
            SessionTurnContent::new("correct this").unwrap(),
            TaskObservationId::new("correction-1").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. }))
    ));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Completed);
}

#[test]
fn correction_task_and_session_preconditions_fail_before_provider_invocation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let open_session_id = SessionId::new("open-session").unwrap();
    let closed_session_id = SessionId::new("closed-session").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(
            open_session_id.clone(),
            SessionTitle::new("Open correction session").unwrap(),
        )
        .unwrap();
    sessions
        .create(
            closed_session_id.clone(),
            SessionTitle::new("Closed correction session").unwrap(),
        )
        .unwrap();

    let completed_id = TaskId::new("completed").unwrap();
    let unassociated_id = TaskId::new("unassociated").unwrap();
    let closed_id = TaskId::new("closed").unwrap();
    let completed_parent = TaskObservationId::new("completed-attempt").unwrap();
    let unassociated_parent = TaskObservationId::new("unassociated-attempt").unwrap();
    let closed_parent = TaskObservationId::new("closed-attempt").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    for (task_id, parent_id) in [
        (&completed_id, &completed_parent),
        (&unassociated_id, &unassociated_parent),
        (&closed_id, &closed_parent),
    ] {
        tasks
            .start(task_id.clone(), TaskGoal::new("correct safely").unwrap())
            .unwrap();
        tasks
            .append_observation(
                task_id,
                parent_id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new("first answer").unwrap(),
            )
            .unwrap();
    }
    tasks
        .associate_session(&completed_id, &open_session_id)
        .unwrap();
    tasks
        .complete(&completed_id, TaskOutput::new("done").unwrap())
        .unwrap();
    tasks
        .associate_session(&closed_id, &closed_session_id)
        .unwrap();
    drop(tasks);
    sessions
        .close(
            &closed_session_id,
            SessionClosure::new("closed before correction").unwrap(),
        )
        .unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("unused").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    assert!(matches!(
        runtime.execute_task_correction_turn(
            &TaskId::new("missing").unwrap(),
            &TaskObservationId::new("missing-attempt").unwrap(),
            SessionTurnContent::new("missing").unwrap(),
            TaskObservationId::new("missing-correction").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::NotFound { .. }))
    ));
    assert!(matches!(
        runtime.execute_task_correction_turn(
            &completed_id,
            &completed_parent,
            SessionTurnContent::new("completed").unwrap(),
            TaskObservationId::new("completed-correction").unwrap(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. }))
    ));
    assert!(matches!(
        runtime.execute_task_correction_turn(
            &unassociated_id,
            &unassociated_parent,
            SessionTurnContent::new("unassociated").unwrap(),
            TaskObservationId::new("unassociated-correction").unwrap(),
        ),
        Err(RuntimeError::TaskNotAssociated { .. })
    ));
    assert!(matches!(
        runtime.execute_task_correction_turn(
            &closed_id,
            &closed_parent,
            SessionTurnContent::new("closed").unwrap(),
            TaskObservationId::new("closed-correction").unwrap(),
        ),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert!(calls.borrow().is_empty());
    assert!(
        sessions
            .load(&open_session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
    assert!(
        sessions
            .load(&closed_session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
}

#[test]
fn correction_provider_and_assistant_failures_preserve_partial_commits() {
    for close_during_provider in [false, true] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new("session-1").unwrap();
        let task_id = TaskId::new("task-1").unwrap();
        SessionStore::open(&path)
            .unwrap()
            .create(
                session_id.clone(),
                SessionTitle::new("Correction failure").unwrap(),
            )
            .unwrap();
        let mut tasks = TaskStore::open(&path).unwrap();
        tasks
            .start(task_id.clone(), TaskGoal::new("correct safely").unwrap())
            .unwrap();
        tasks.associate_session(&task_id, &session_id).unwrap();
        let parent_id = TaskObservationId::new("attempt-1").unwrap();
        tasks
            .append_observation(
                &task_id,
                parent_id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new("first answer").unwrap(),
            )
            .unwrap();
        drop(tasks);

        let error = if close_during_provider {
            let provider = ClosingProvider {
                path: path.clone(),
                session_id: session_id.clone(),
                calls: Rc::new(RefCell::new(0)),
            };
            AssistantRuntime::open(&path, provider)
                .unwrap()
                .execute_task_correction_turn(
                    &task_id,
                    &parent_id,
                    SessionTurnContent::new("correct this").unwrap(),
                    TaskObservationId::new("correction-1").unwrap(),
                )
                .unwrap_err()
        } else {
            let provider = FakeProvider {
                calls: Rc::new(RefCell::new(Vec::new())),
                result: Err(FakeProviderFailure),
            };
            AssistantRuntime::open(&path, provider)
                .unwrap()
                .execute_task_correction_turn(
                    &task_id,
                    &parent_id,
                    SessionTurnContent::new("correct this").unwrap(),
                    TaskObservationId::new("correction-1").unwrap(),
                )
                .unwrap_err()
        };

        if close_during_provider {
            assert!(matches!(
                error,
                RuntimeError::Session(
                    vela_kernel::session::SessionStoreError::SessionClosed { .. }
                )
            ));
        } else {
            assert!(matches!(error, RuntimeError::Provider(_)));
        }
        let session = SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(session.turns().len(), 1);
        assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
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

#[test]
fn completes_a_task_turn_with_the_response_as_attempt_and_output() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(
            session_id.clone(),
            SessionTitle::new("Completion runtime").unwrap(),
        )
        .unwrap();
    for (role, content) in [
        (SessionTurnRole::Human, "earlier question"),
        (SessionTurnRole::Assistant, "earlier answer"),
    ] {
        sessions
            .append_turn(&session_id, role, SessionTurnContent::new(content).unwrap())
            .unwrap();
    }
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("finish the answer").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("final grounded answer").unwrap()),
    };
    let outcome = AssistantRuntime::open(&path, provider)
        .unwrap()
        .complete_task_turn(
            &task_id,
            SessionTurnContent::new("please finalize").unwrap(),
            TaskObservationId::new("final-attempt").unwrap(),
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![
            (SessionTurnRole::Human, "earlier question".to_owned()),
            (SessionTurnRole::Assistant, "earlier answer".to_owned()),
            (SessionTurnRole::Human, "please finalize".to_owned()),
        ]]
    );
    assert_eq!(outcome.session().turns().len(), 4);
    assert_eq!(
        outcome.task().status(),
        vela_kernel::task::TaskStatus::Completed
    );
    assert_eq!(
        outcome.task().output().unwrap().as_str(),
        "final grounded answer"
    );
    assert_eq!(outcome.task().observations().len(), 1);
    assert_eq!(
        outcome.task().observations()[0].kind(),
        TaskObservationKind::Attempt
    );
    assert_eq!(
        outcome.task().observations()[0].text().as_str(),
        "final grounded answer"
    );
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        *outcome.session()
    );
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        *outcome.task()
    );
}

#[test]
fn completion_preflight_rejects_duplicate_attempt_before_side_effects() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Completion preflight").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("finish safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let duplicate_id = TaskObservationId::new("duplicate").unwrap();
    tasks
        .append_observation(
            &task_id,
            duplicate_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("existing answer").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("unused").unwrap()),
    };
    let error = AssistantRuntime::open(&path, provider)
        .unwrap()
        .complete_task_turn(
            &task_id,
            SessionTurnContent::new("do not write").unwrap(),
            duplicate_id,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert!(calls.borrow().is_empty());
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
fn completion_provider_and_assistant_failures_write_no_task_result() {
    for close_during_provider in [false, true] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new("session-1").unwrap();
        let task_id = TaskId::new("task-1").unwrap();
        SessionStore::open(&path)
            .unwrap()
            .create(
                session_id.clone(),
                SessionTitle::new("Completion failure").unwrap(),
            )
            .unwrap();
        let mut tasks = TaskStore::open(&path).unwrap();
        tasks
            .start(task_id.clone(), TaskGoal::new("finish safely").unwrap())
            .unwrap();
        tasks.associate_session(&task_id, &session_id).unwrap();
        drop(tasks);

        let error = if close_during_provider {
            AssistantRuntime::open(
                &path,
                ClosingProvider {
                    path: path.clone(),
                    session_id: session_id.clone(),
                    calls: Rc::new(RefCell::new(0)),
                },
            )
            .unwrap()
            .complete_task_turn(
                &task_id,
                SessionTurnContent::new("final request").unwrap(),
                TaskObservationId::new("attempt-1").unwrap(),
            )
            .unwrap_err()
        } else {
            AssistantRuntime::open(
                &path,
                FakeProvider {
                    calls: Rc::new(RefCell::new(Vec::new())),
                    result: Err(FakeProviderFailure),
                },
            )
            .unwrap()
            .complete_task_turn(
                &task_id,
                SessionTurnContent::new("final request").unwrap(),
                TaskObservationId::new("attempt-1").unwrap(),
            )
            .unwrap_err()
        };

        if close_during_provider {
            assert!(matches!(
                error,
                RuntimeError::Session(
                    vela_kernel::session::SessionStoreError::SessionClosed { .. }
                )
            ));
        } else {
            assert!(matches!(error, RuntimeError::Provider(_)));
        }
        let session = SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(session.turns().len(), 1);
        assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
        let task = TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap();
        assert!(task.observations().is_empty());
        assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
        assert!(task.output().is_none());
    }
}

#[test]
fn invalid_completion_response_preserves_transcript_without_task_result() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Invalid completion").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("finish safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
            result: Ok(SessionTurnContent::new(" \n ").unwrap()),
        },
    )
    .unwrap()
    .complete_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(error, RuntimeError::InvalidAttemptText(_)));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
}

#[test]
fn completion_failure_after_attempt_preserves_attempt_and_winning_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Completion race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER complete_after_attempt
             AFTER INSERT ON events
             WHEN NEW.stream_id = 'task:task-1'
              AND NEW.event_type = 'task.observation_appended'
             BEGIN
               INSERT INTO events
                 (stream_id, stream_version, event_type, payload_version, payload)
               VALUES
                 (NEW.stream_id, NEW.stream_version + 1, 'task.completed', 2,
                  CAST('{\"output\":\"winning output\"}' AS BLOB));
             END;",
        )
        .unwrap();
    let provider = FakeProvider {
        calls: Rc::new(RefCell::new(Vec::new())),
        result: Ok(SessionTurnContent::new("late final answer").unwrap()),
    };

    let error = AssistantRuntime::open(&path, provider)
        .unwrap()
        .complete_task_turn(
            &task_id,
            SessionTurnContent::new("final request").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    ));
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "late final answer");
    assert_eq!(task.output().unwrap().as_str(), "winning output");
}

#[test]
fn completion_attempt_race_preserves_both_turns_without_runtime_completion() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    let observation_id = TaskObservationId::new("attempt-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Attempt race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let provider = AppendingAttemptProvider {
        path: path.clone(),
        task_id: task_id.clone(),
        observation_id: observation_id.clone(),
    };
    let error = AssistantRuntime::open(&path, provider)
        .unwrap()
        .complete_task_turn(
            &task_id,
            SessionTurnContent::new("final request").unwrap(),
            observation_id,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "racing attempt");
    assert!(task.output().is_none());
}

#[test]
fn fails_a_task_turn_with_response_as_attempt_and_caller_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(
            session_id.clone(),
            SessionTitle::new("Failure runtime").unwrap(),
        )
        .unwrap();
    sessions
        .append_turn(
            &session_id,
            SessionTurnRole::Human,
            SessionTurnContent::new("earlier question").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("try the operation").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let outcome = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::clone(&calls),
            result: Ok(SessionTurnContent::new("the operation did not succeed").unwrap()),
        },
    )
    .unwrap()
    .fail_task_turn(
        &task_id,
        SessionTurnContent::new("make one final attempt").unwrap(),
        TaskObservationId::new("final-attempt").unwrap(),
        TaskFailure::new("dependency remained unavailable").unwrap(),
    )
    .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![
            (SessionTurnRole::Human, "earlier question".to_owned()),
            (SessionTurnRole::Human, "make one final attempt".to_owned()),
        ]]
    );
    assert_eq!(outcome.session().turns().len(), 3);
    assert_eq!(
        outcome.task().status(),
        vela_kernel::task::TaskStatus::Failed
    );
    assert_eq!(
        outcome.task().failure().unwrap().as_str(),
        "dependency remained unavailable"
    );
    assert_eq!(outcome.task().observations().len(), 1);
    assert_eq!(
        outcome.task().observations()[0].kind(),
        TaskObservationKind::Attempt
    );
    assert_eq!(
        outcome.task().observations()[0].text().as_str(),
        "the operation did not succeed"
    );
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        *outcome.session()
    );
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        *outcome.task()
    );
}

#[test]
fn failure_preflight_rejects_duplicate_attempt_before_side_effects() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Failure preflight").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("fail safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let duplicate_id = TaskObservationId::new("duplicate").unwrap();
    tasks
        .append_observation(
            &task_id,
            duplicate_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("existing answer").unwrap(),
        )
        .unwrap();
    drop(tasks);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::clone(&calls),
            result: Ok(SessionTurnContent::new("unused").unwrap()),
        },
    )
    .unwrap()
    .fail_task_turn(
        &task_id,
        SessionTurnContent::new("do not write").unwrap(),
        duplicate_id,
        TaskFailure::new("known failure").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert!(calls.borrow().is_empty());
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
fn failure_task_and_session_preconditions_fail_before_provider_invocation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let open_session_id = SessionId::new("open-session").unwrap();
    let closed_session_id = SessionId::new("closed-session").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    for session_id in [&open_session_id, &closed_session_id] {
        sessions
            .create(
                session_id.clone(),
                SessionTitle::new("Failure precondition").unwrap(),
            )
            .unwrap();
    }

    let completed_id = TaskId::new("completed").unwrap();
    let unassociated_id = TaskId::new("unassociated").unwrap();
    let closed_id = TaskId::new("closed").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    for task_id in [&completed_id, &unassociated_id, &closed_id] {
        tasks
            .start(task_id.clone(), TaskGoal::new("fail safely").unwrap())
            .unwrap();
    }
    tasks
        .associate_session(&completed_id, &open_session_id)
        .unwrap();
    tasks
        .complete(&completed_id, TaskOutput::new("done").unwrap())
        .unwrap();
    tasks
        .associate_session(&closed_id, &closed_session_id)
        .unwrap();
    drop(tasks);
    sessions
        .close(
            &closed_session_id,
            SessionClosure::new("already closed").unwrap(),
        )
        .unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::clone(&calls),
            result: Ok(SessionTurnContent::new("unused").unwrap()),
        },
    )
    .unwrap();
    let failure = || TaskFailure::new("caller diagnostic").unwrap();

    assert!(matches!(
        runtime.fail_task_turn(
            &TaskId::new("missing").unwrap(),
            SessionTurnContent::new("missing").unwrap(),
            TaskObservationId::new("missing-attempt").unwrap(),
            failure(),
        ),
        Err(RuntimeError::Task(TaskStoreError::NotFound { .. }))
    ));
    assert!(matches!(
        runtime.fail_task_turn(
            &completed_id,
            SessionTurnContent::new("completed").unwrap(),
            TaskObservationId::new("completed-attempt").unwrap(),
            failure(),
        ),
        Err(RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. }))
    ));
    assert!(matches!(
        runtime.fail_task_turn(
            &unassociated_id,
            SessionTurnContent::new("unassociated").unwrap(),
            TaskObservationId::new("unassociated-attempt").unwrap(),
            failure(),
        ),
        Err(RuntimeError::TaskNotAssociated { .. })
    ));
    assert!(matches!(
        runtime.fail_task_turn(
            &closed_id,
            SessionTurnContent::new("closed").unwrap(),
            TaskObservationId::new("closed-attempt").unwrap(),
            failure(),
        ),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert!(calls.borrow().is_empty());
    assert!(
        sessions
            .load(&open_session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
    assert!(
        sessions
            .load(&closed_session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
}

#[test]
fn failure_provider_and_assistant_failures_write_no_task_result() {
    for close_during_provider in [false, true] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new("session-1").unwrap();
        let task_id = TaskId::new("task-1").unwrap();
        SessionStore::open(&path)
            .unwrap()
            .create(
                session_id.clone(),
                SessionTitle::new("Failure partial commits").unwrap(),
            )
            .unwrap();
        let mut tasks = TaskStore::open(&path).unwrap();
        tasks
            .start(task_id.clone(), TaskGoal::new("fail safely").unwrap())
            .unwrap();
        tasks.associate_session(&task_id, &session_id).unwrap();
        drop(tasks);

        let error = if close_during_provider {
            AssistantRuntime::open(
                &path,
                ClosingProvider {
                    path: path.clone(),
                    session_id: session_id.clone(),
                    calls: Rc::new(RefCell::new(0)),
                },
            )
            .unwrap()
            .fail_task_turn(
                &task_id,
                SessionTurnContent::new("final request").unwrap(),
                TaskObservationId::new("attempt-1").unwrap(),
                TaskFailure::new("caller diagnostic").unwrap(),
            )
            .unwrap_err()
        } else {
            AssistantRuntime::open(
                &path,
                FakeProvider {
                    calls: Rc::new(RefCell::new(Vec::new())),
                    result: Err(FakeProviderFailure),
                },
            )
            .unwrap()
            .fail_task_turn(
                &task_id,
                SessionTurnContent::new("final request").unwrap(),
                TaskObservationId::new("attempt-1").unwrap(),
                TaskFailure::new("caller diagnostic").unwrap(),
            )
            .unwrap_err()
        };

        if close_during_provider {
            assert!(matches!(
                error,
                RuntimeError::Session(
                    vela_kernel::session::SessionStoreError::SessionClosed { .. }
                )
            ));
        } else {
            assert!(matches!(error, RuntimeError::Provider(_)));
        }
        let session = SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(session.turns().len(), 1);
        assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
        let task = TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap();
        assert!(task.observations().is_empty());
        assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
        assert!(task.failure().is_none());
    }
}

#[test]
fn invalid_failure_response_preserves_transcript_without_task_result() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Invalid failure response").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("fail safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
            result: Ok(SessionTurnContent::new(" \n ").unwrap()),
        },
    )
    .unwrap()
    .fail_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
        TaskFailure::new("caller diagnostic").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(error, RuntimeError::InvalidAttemptText(_)));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
}

#[test]
fn task_failure_after_attempt_preserves_attempt_and_winning_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Failure race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER complete_after_failure_attempt
             AFTER INSERT ON events
             WHEN NEW.stream_id = 'task:task-1'
              AND NEW.event_type = 'task.observation_appended'
             BEGIN
               INSERT INTO events
                 (stream_id, stream_version, event_type, payload_version, payload)
               VALUES
                 (NEW.stream_id, NEW.stream_version + 1, 'task.completed', 2,
                  CAST('{\"output\":\"winning output\"}' AS BLOB));
             END;",
        )
        .unwrap();

    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
            result: Ok(SessionTurnContent::new("late attempt").unwrap()),
        },
    )
    .unwrap()
    .fail_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
        TaskFailure::new("losing diagnostic").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    ));
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "late attempt");
    assert_eq!(task.output().unwrap().as_str(), "winning output");
    assert!(task.failure().is_none());
}

#[test]
fn failure_attempt_race_preserves_both_turns_without_runtime_failure() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    let observation_id = TaskObservationId::new("attempt-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Failure attempt race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    drop(tasks);

    let error = AssistantRuntime::open(
        &path,
        AppendingAttemptProvider {
            path: path.clone(),
            task_id: task_id.clone(),
            observation_id: observation_id.clone(),
        },
    )
    .unwrap()
    .fail_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        observation_id,
        TaskFailure::new("caller diagnostic").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "racing attempt");
    assert!(task.failure().is_none());
}

#[test]
fn cancellation_persists_response_as_attempt_and_caller_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(
            session_id.clone(),
            SessionTitle::new("Cancellation runtime").unwrap(),
        )
        .unwrap();
    sessions
        .append_turn(
            &session_id,
            SessionTurnRole::Human,
            SessionTurnContent::new("earlier request").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("try the operation").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let outcome = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::clone(&calls),
            result: Ok(SessionTurnContent::new("stopping as requested").unwrap()),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("stop this task").unwrap(),
        TaskObservationId::new("final-attempt").unwrap(),
        TaskCancellation::new("no longer needed").unwrap(),
    )
    .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![
            (SessionTurnRole::Human, "earlier request".to_owned()),
            (SessionTurnRole::Human, "stop this task".to_owned()),
        ]]
    );
    assert_eq!(outcome.session().turns().len(), 3);
    assert_eq!(
        outcome.task().status(),
        vela_kernel::task::TaskStatus::Cancelled
    );
    assert_eq!(
        outcome.task().cancellation().unwrap().as_str(),
        "no longer needed"
    );
    assert_eq!(outcome.task().observations().len(), 1);
    assert_eq!(
        outcome.task().observations()[0].kind(),
        TaskObservationKind::Attempt
    );
    assert_eq!(
        outcome.task().observations()[0].text().as_str(),
        "stopping as requested"
    );
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        *outcome.session()
    );
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        *outcome.task()
    );
}

#[test]
fn cancellation_preflight_rejects_duplicate_attempt_before_side_effects() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Cancellation preflight").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("cancel safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let duplicate_id = TaskObservationId::new("duplicate").unwrap();
    tasks
        .append_observation(
            &task_id,
            duplicate_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("existing answer").unwrap(),
        )
        .unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::clone(&calls),
            result: Ok(SessionTurnContent::new("unused").unwrap()),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("do not write").unwrap(),
        duplicate_id,
        TaskCancellation::new("caller reason").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert!(calls.borrow().is_empty());
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
fn cancellation_task_and_session_preconditions_fail_before_provider_invocation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let open_session_id = SessionId::new("open-session").unwrap();
    let closed_session_id = SessionId::new("closed-session").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    for session_id in [&open_session_id, &closed_session_id] {
        sessions
            .create(
                session_id.clone(),
                SessionTitle::new("Cancellation precondition").unwrap(),
            )
            .unwrap();
    }

    let completed_id = TaskId::new("completed").unwrap();
    let cancelled_id = TaskId::new("cancelled").unwrap();
    let failed_id = TaskId::new("failed").unwrap();
    let unassociated_id = TaskId::new("unassociated").unwrap();
    let closed_id = TaskId::new("closed").unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    for task_id in [
        &completed_id,
        &cancelled_id,
        &failed_id,
        &unassociated_id,
        &closed_id,
    ] {
        tasks
            .start(task_id.clone(), TaskGoal::new("cancel safely").unwrap())
            .unwrap();
    }
    for task_id in [&completed_id, &cancelled_id, &failed_id] {
        tasks.associate_session(task_id, &open_session_id).unwrap();
    }
    tasks
        .complete(&completed_id, TaskOutput::new("done").unwrap())
        .unwrap();
    tasks
        .cancel(
            &cancelled_id,
            TaskCancellation::new("already cancelled").unwrap(),
        )
        .unwrap();
    tasks
        .fail(&failed_id, TaskFailure::new("already failed").unwrap())
        .unwrap();
    tasks
        .associate_session(&closed_id, &closed_session_id)
        .unwrap();
    drop(tasks);
    sessions
        .close(
            &closed_session_id,
            SessionClosure::new("already closed").unwrap(),
        )
        .unwrap();

    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::clone(&calls),
            result: Ok(SessionTurnContent::new("unused").unwrap()),
        },
    )
    .unwrap();
    macro_rules! assert_preflight {
        ($task_id:expr, $attempt_id:literal, $pattern:pat) => {
            assert!(matches!(
                runtime.cancel_task_turn(
                    &$task_id,
                    SessionTurnContent::new("do not write").unwrap(),
                    TaskObservationId::new($attempt_id).unwrap(),
                    TaskCancellation::new("caller reason").unwrap(),
                ),
                Err($pattern)
            ));
        };
    }
    assert_preflight!(
        TaskId::new("missing").unwrap(),
        "missing-attempt",
        RuntimeError::Task(TaskStoreError::NotFound { .. })
    );
    assert_preflight!(
        completed_id,
        "completed-attempt",
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    );
    assert_preflight!(
        cancelled_id,
        "cancelled-attempt",
        RuntimeError::Task(TaskStoreError::AlreadyCancelled { .. })
    );
    assert_preflight!(
        failed_id,
        "failed-attempt",
        RuntimeError::Task(TaskStoreError::AlreadyFailed { .. })
    );
    assert_preflight!(
        unassociated_id,
        "unassociated-attempt",
        RuntimeError::TaskNotAssociated { .. }
    );
    assert_preflight!(
        closed_id,
        "closed-attempt",
        RuntimeError::Session(vela_kernel::session::SessionStoreError::SessionClosed { .. })
    );

    assert!(calls.borrow().is_empty());
    assert!(
        sessions
            .load(&open_session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
    assert!(
        sessions
            .load(&closed_session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
}

#[test]
fn invalid_cancellation_response_preserves_transcript_without_task_result() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Invalid cancellation response").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("cancel safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();

    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
            result: Ok(SessionTurnContent::new(" \n ").unwrap()),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
        TaskCancellation::new("caller reason").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(error, RuntimeError::InvalidAttemptText(_)));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert!(task.cancellation().is_none());
}

#[test]
fn task_cancellation_after_attempt_preserves_attempt_and_winning_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Cancellation race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER complete_after_cancellation_attempt
             AFTER INSERT ON events
             WHEN NEW.stream_id = 'task:task-1'
              AND NEW.event_type = 'task.observation_appended'
             BEGIN
               INSERT INTO events
                 (stream_id, stream_version, event_type, payload_version, payload)
               VALUES
                 (NEW.stream_id, NEW.stream_version + 1, 'task.completed', 2,
                  CAST('{\"output\":\"winning output\"}' AS BLOB));
             END;",
        )
        .unwrap();

    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
            result: Ok(SessionTurnContent::new("late attempt").unwrap()),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
        TaskCancellation::new("losing reason").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    ));
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "late attempt");
    assert_eq!(task.output().unwrap().as_str(), "winning output");
    assert!(task.cancellation().is_none());
}

#[test]
fn cancellation_provider_failure_writes_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Cancellation provider failure").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("cancel safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();

    let error = AssistantRuntime::open(
        &path,
        FakeProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
            result: Err(FakeProviderFailure),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
        TaskCancellation::new("caller reason").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
}

#[test]
fn cancellation_assistant_append_failure_writes_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Cancellation assistant failure").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("cancel safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();

    let calls = Rc::new(RefCell::new(0));
    let error = AssistantRuntime::open(
        &path,
        ClosingProvider {
            path: path.clone(),
            session_id: session_id.clone(),
            calls: Rc::clone(&calls),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        TaskObservationId::new("attempt-1").unwrap(),
        TaskCancellation::new("caller reason").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Session(vela_kernel::session::SessionStoreError::SessionClosed { .. })
    ));
    assert_eq!(*calls.borrow(), 1);
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert_eq!(session.turns()[0].role(), SessionTurnRole::Human);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert!(task.cancellation().is_none());
}

#[test]
fn cancellation_attempt_race_preserves_both_turns_without_cancellation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    let observation_id = TaskObservationId::new("attempt-1").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Cancellation attempt race").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(&path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("race safely").unwrap())
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();

    let error = AssistantRuntime::open(
        &path,
        AppendingAttemptProvider {
            path: path.clone(),
            task_id: task_id.clone(),
            observation_id: observation_id.clone(),
        },
    )
    .unwrap()
    .cancel_task_turn(
        &task_id,
        SessionTurnContent::new("final request").unwrap(),
        observation_id,
        TaskCancellation::new("caller reason").unwrap(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        2
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "racing attempt");
    assert!(task.cancellation().is_none());
}
