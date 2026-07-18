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
