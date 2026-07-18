use std::{cell::RefCell, error::Error, fmt, path::PathBuf, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{AssistantProvider, AssistantRuntime, ProviderError, RuntimeError},
    session::{
        SessionClosure, SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole,
    },
};

#[derive(Clone)]
struct FakeProvider {
    calls: SharedCalls,
    result: Result<SessionTurnContent, FakeProviderFailure>,
}

type RecordedTranscript = Vec<(SessionTurnRole, String)>;
type SharedCalls = Rc<RefCell<Vec<RecordedTranscript>>>;

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
fn executes_one_turn_against_the_durable_ordered_transcript() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("session-1").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(
            session_id.clone(),
            SessionTitle::new("Runtime contract").unwrap(),
        )
        .unwrap();
    sessions
        .append_turn(
            &session_id,
            SessionTurnRole::Human,
            SessionTurnContent::new("prior question").unwrap(),
        )
        .unwrap();
    sessions
        .append_turn(
            &session_id,
            SessionTurnRole::Assistant,
            SessionTurnContent::new("prior answer").unwrap(),
        )
        .unwrap();
    drop(sessions);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("new answer").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let session = runtime
        .execute_turn(
            &session_id,
            SessionTurnContent::new("new question").unwrap(),
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![
            (SessionTurnRole::Human, "prior question".to_owned()),
            (SessionTurnRole::Assistant, "prior answer".to_owned()),
            (SessionTurnRole::Human, "new question".to_owned()),
        ]]
    );
    assert_eq!(session.turns().len(), 4);
    assert_eq!(session.turns()[3].role(), SessionTurnRole::Assistant);
    assert_eq!(session.turns()[3].content().as_str(), "new answer");

    let reopened = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(reopened, session);
}

#[test]
fn missing_and_closed_sessions_do_not_invoke_the_provider() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("closed").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(session_id.clone(), SessionTitle::new("Closed").unwrap())
        .unwrap();
    sessions
        .close(&session_id, SessionClosure::new("finished").unwrap())
        .unwrap();
    drop(sessions);

    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = FakeProvider {
        calls: Rc::clone(&calls),
        result: Ok(SessionTurnContent::new("unused").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    assert!(matches!(
        runtime.execute_turn(
            &SessionId::new("missing").unwrap(),
            SessionTurnContent::new("hello").unwrap(),
        ),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::NotFound { .. }
        ))
    ));
    assert!(matches!(
        runtime.execute_turn(&session_id, SessionTurnContent::new("hello").unwrap(),),
        Err(RuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert!(calls.borrow().is_empty());

    let sessions = SessionStore::open(&path).unwrap();
    assert!(
        sessions
            .load(&SessionId::new("missing").unwrap())
            .unwrap()
            .is_none()
    );
    assert!(
        sessions
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
}

#[test]
fn provider_failure_preserves_only_the_new_human_turn_and_error_source() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("provider-failure").unwrap();
    let mut sessions = SessionStore::open(&path).unwrap();
    sessions
        .create(session_id.clone(), SessionTitle::new("Failure").unwrap())
        .unwrap();
    drop(sessions);

    let provider = FakeProvider {
        calls: Rc::new(RefCell::new(Vec::new())),
        result: Err(FakeProviderFailure),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let error = runtime
        .execute_turn(
            &session_id,
            SessionTurnContent::new("durable question").unwrap(),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    let provider_error = error.source().unwrap();
    assert_eq!(provider_error.to_string(), "provider unavailable");
    assert!(
        provider_error
            .source()
            .unwrap()
            .downcast_ref::<FakeProviderFailure>()
            .is_some()
    );
    let persisted = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.turns().len(), 1);
    assert_eq!(persisted.turns()[0].role(), SessionTurnRole::Human);
    assert_eq!(persisted.turns()[0].content().as_str(), "durable question");
}

#[test]
fn session_closure_after_provider_success_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("assistant-append-failure").unwrap();
    SessionStore::open(&path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Assistant append failure").unwrap(),
        )
        .unwrap();
    let calls = Rc::new(RefCell::new(0));
    let provider = ClosingProvider {
        path: path.clone(),
        session_id: session_id.clone(),
        calls: Rc::clone(&calls),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();

    let error = runtime
        .execute_turn(
            &session_id,
            SessionTurnContent::new("durable before provider call").unwrap(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Session(vela_kernel::session::SessionStoreError::SessionClosed { .. })
    ));
    assert_eq!(*calls.borrow(), 1);
    let persisted = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.closure().unwrap().as_str(),
        "closed during provider call"
    );
    assert_eq!(persisted.turns().len(), 1);
    assert_eq!(persisted.turns()[0].role(), SessionTurnRole::Human);
    assert_eq!(
        persisted.turns()[0].content().as_str(),
        "durable before provider call"
    );
}
