use std::{cell::RefCell, error::Error, fmt, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        ProviderError, ProviderToolContinuation, ProviderToolResponse, ProviderToolStepOutcome,
        ToolAssistantContinuationProvider, ToolAssistantProvider, ToolAssistantRuntime,
        ToolTaskRuntimeError, ToolTaskTurnOutcome,
    },
    session::{
        SessionClosure, SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole,
    },
    task::{TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskStore, TaskStoreError},
    tool::{
        PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationId,
        ToolInvocationStatus, ToolInvocationStore, ToolMetadata, ToolRegistry, ToolRequest,
    },
};

#[derive(Clone, Debug)]
struct FakeProviderFailure;
impl fmt::Display for FakeProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider unavailable")
    }
}
impl Error for FakeProviderFailure {}

type RecordedTranscript = Vec<(SessionTurnRole, String)>;
type SharedCalls = Rc<RefCell<Vec<RecordedTranscript>>>;

struct Provider {
    calls: SharedCalls,
    response: Option<Result<ProviderToolResponse, ProviderError>>,
}
impl ToolAssistantProvider for Provider {
    fn complete_with_tools(
        &mut self,
        transcript: &[vela_kernel::session::SessionTurn],
        _tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError> {
        self.calls.borrow_mut().push(
            transcript
                .iter()
                .map(|turn| (turn.role(), turn.content().as_str().to_owned()))
                .collect(),
        );
        self.response
            .take()
            .expect("provider called more than once")
    }
}

struct StatefulProvider {
    initial_calls: Rc<RefCell<usize>>,
    continuation_calls: Rc<RefCell<usize>>,
}

impl ToolAssistantProvider for StatefulProvider {
    fn complete_with_tools(
        &mut self,
        _transcript: &[vela_kernel::session::SessionTurn],
        _tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError> {
        *self.initial_calls.borrow_mut() += 1;
        Ok(ProviderToolResponse::ToolRequest {
            tool_id: ToolId::new("tool.echo").unwrap(),
            input: json!({"step": 1}),
        })
    }
}

impl ToolAssistantContinuationProvider for StatefulProvider {
    fn complete_after_tool(
        &mut self,
        continuation: ProviderToolContinuation<'_>,
        _tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError> {
        assert_eq!(*self.initial_calls.borrow(), 1);
        assert_eq!(
            continuation.prior_result().output(),
            &json!({"echo": {"step": 1}})
        );
        *self.continuation_calls.borrow_mut() += 1;
        Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("continued final").unwrap(),
        ))
    }
}

struct EchoTool {
    id: ToolId,
    calls: Rc<RefCell<usize>>,
}
impl Tool for EchoTool {
    fn id(&self) -> &ToolId {
        &self.id
    }
    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }
    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        *self.calls.borrow_mut() += 1;
        Ok(json!({"echo": input}))
    }
}

struct Allow {
    calls: usize,
}
impl ToolAuthorizer for Allow {
    fn authorize(&mut self, _request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        PermissionDecision::Allow
    }
}

fn setup(path: &std::path::Path) -> (SessionId, TaskId) {
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Tool task runtime").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(path).unwrap();
    tasks
        .start(
            task_id.clone(),
            TaskGoal::new("use tools carefully").unwrap(),
        )
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    (session_id, task_id)
}

#[test]
fn final_response_persists_assistant_turn_and_attempt() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("final answer").unwrap(),
        ))),
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut registry = ToolRegistry::new();
    let mut authorizer = Allow { calls: 0 };

    let outcome = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            ToolInvocationId::new("unused-invocation").unwrap(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap();

    let ToolTaskTurnOutcome::Final { session, task } = outcome else {
        panic!("expected final task turn")
    };
    assert_eq!(
        calls.borrow().as_slice(),
        &[vec![(SessionTurnRole::Human, "question".into())]]
    );
    assert_eq!(authorizer.calls, 0);
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].content().as_str(), "final answer");
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].kind(), TaskObservationKind::Attempt);
    assert_eq!(task.observations()[0].text().as_str(), "final answer");
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        session
    );
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        task
    );
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("unused-invocation").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn tool_response_persists_human_turn_and_metadata_only_invocation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let tool_id = ToolId::new("tool.echo").unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::ToolRequest {
            tool_id: tool_id.clone(),
            input: json!({"secret": "memory-only-input"}),
        })),
    };
    let tool_calls = Rc::new(RefCell::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(EchoTool {
            id: tool_id.clone(),
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let invocation_id = ToolInvocationId::new("invocation-1").unwrap();
    let mut authorizer = Allow { calls: 0 };

    let outcome = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("use the tool").unwrap(),
            TaskObservationId::new("future-attempt").unwrap(),
            invocation_id.clone(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap();

    let prior = outcome.tool_result().unwrap();
    assert_eq!(prior.tool_id(), &tool_id);
    assert_eq!(prior.input(), &json!({"secret": "memory-only-input"}));
    assert_eq!(
        prior.output(),
        &json!({"echo": {"secret": "memory-only-input"}})
    );
    let ToolTaskTurnOutcome::ToolCompleted {
        session,
        task_id: completed_task_id,
        tool_id: completed_id,
        input,
        output,
        continuation,
    } = outcome
    else {
        panic!("expected completed tool step")
    };
    assert_eq!(completed_task_id, task_id);
    assert_eq!(completed_id, tool_id);
    assert_eq!(input, json!({"secret": "memory-only-input"}));
    assert_eq!(output, json!({"echo": {"secret": "memory-only-input"}}));
    assert_eq!(
        continuation,
        vela_kernel::runtime::ToolStepContinuation::ProviderRequired
    );
    assert_eq!((*tool_calls.borrow(), authorizer.calls), (1, 1));
    assert_eq!(session.turns().len(), 1);
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        session
    );
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .observations()
            .is_empty()
    );
    let invocation = ToolInvocationStore::open(&path)
        .unwrap()
        .load(&invocation_id)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.task_id(), Some(&task_id));
    assert_eq!(invocation.status(), ToolInvocationStatus::Succeeded);
    let retained: Vec<Vec<u8>> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare("SELECT payload FROM events WHERE stream_id = ?1 ORDER BY stream_version")
        .unwrap()
        .query_map(("tool-invocation:invocation-1",), |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(retained.iter().all(|payload| {
        !payload
            .windows(17)
            .any(|window| window == b"memory-only-input")
    }));
}

#[test]
fn explicit_continuation_reuses_the_same_provider_instance() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (_, task_id) = setup(&path);
    let initial_calls = Rc::new(RefCell::new(0));
    let continuation_calls = Rc::new(RefCell::new(0));
    let provider = StatefulProvider {
        initial_calls: initial_calls.clone(),
        continuation_calls: continuation_calls.clone(),
    };
    let mut registry = ToolRegistry::new();
    registry
        .register(EchoTool {
            id: ToolId::new("tool.echo").unwrap(),
            calls: Rc::new(RefCell::new(0)),
        })
        .unwrap();
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut authorizer = Allow { calls: 0 };

    let first = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("start").unwrap(),
            TaskObservationId::new("future-attempt").unwrap(),
            ToolInvocationId::new("invocation-1").unwrap(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap();
    let continuation = first.continuation().unwrap();
    let final_step = runtime
        .continue_provider_step(
            continuation,
            ToolInvocationId::new("unused-final-id").unwrap(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap();

    assert!(matches!(
        final_step,
        ProviderToolStepOutcome::Final { ref content, .. }
            if content.as_str() == "continued final"
    ));
    assert_eq!(
        (*initial_calls.borrow(), *continuation_calls.borrow()),
        (1, 1)
    );
}

#[test]
fn preflight_rejects_duplicate_attempt_before_transcript_and_provider() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let observation_id = TaskObservationId::new("duplicate").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .append_observation(
            &task_id,
            observation_id.clone(),
            TaskObservationKind::Attempt,
            vela_kernel::task::TaskObservationText::new("existing").unwrap(),
        )
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("unused").unwrap(),
        ))),
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut registry = ToolRegistry::new();
    let mut authorizer = Allow { calls: 0 };

    let error = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("not persisted").unwrap(),
            observation_id,
            ToolInvocationId::new("unused").unwrap(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ToolTaskRuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    assert!(calls.borrow().is_empty());
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        0
    );
}

#[test]
fn duplicate_invocation_id_is_rejected_before_transcript_and_provider() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let tool_id = ToolId::new("tool.echo").unwrap();
    let invocation_id = ToolInvocationId::new("duplicate-invocation").unwrap();
    let mut registry = ToolRegistry::new();
    registry
        .register(EchoTool {
            id: tool_id.clone(),
            calls: Rc::new(RefCell::new(0)),
        })
        .unwrap();
    let mut authorizer = Allow { calls: 0 };
    registry
        .invoke_for_task_durable(
            &mut ToolInvocationStore::open(&path).unwrap(),
            &task_id,
            invocation_id.clone(),
            &tool_id,
            &mut authorizer,
            &json!({}),
        )
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::ToolRequest {
            tool_id,
            input: json!({}),
        })),
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();

    let error = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("not persisted").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            invocation_id,
            &mut registry,
            &mut authorizer,
        )
        .unwrap_err();

    assert!(matches!(error, ToolTaskRuntimeError::Invocation(_)));
    assert!(calls.borrow().is_empty());
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .len(),
        0
    );
}

#[test]
fn task_association_and_session_preflight_precede_provider_side_effects() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let unassociated = TaskId::new("unassociated").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            unassociated.clone(),
            TaskGoal::new("not associated").unwrap(),
        )
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("unused").unwrap(),
        ))),
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut registry = ToolRegistry::new();
    let mut authorizer = Allow { calls: 0 };

    let missing = runtime.execute_task_turn(
        &TaskId::new("missing").unwrap(),
        SessionTurnContent::new("missing").unwrap(),
        TaskObservationId::new("missing-attempt").unwrap(),
        ToolInvocationId::new("missing-invocation").unwrap(),
        &mut registry,
        &mut authorizer,
    );
    assert!(matches!(
        missing,
        Err(ToolTaskRuntimeError::Task(TaskStoreError::NotFound { .. }))
    ));
    let unassociated_error = runtime.execute_task_turn(
        &unassociated,
        SessionTurnContent::new("unassociated").unwrap(),
        TaskObservationId::new("unassociated-attempt").unwrap(),
        ToolInvocationId::new("unassociated-invocation").unwrap(),
        &mut registry,
        &mut authorizer,
    );
    assert!(matches!(
        unassociated_error,
        Err(ToolTaskRuntimeError::TaskNotAssociated { .. })
    ));

    SessionStore::open(&path)
        .unwrap()
        .close(
            &session_id,
            SessionClosure::new("closed before turn").unwrap(),
        )
        .unwrap();
    let closed = runtime.execute_task_turn(
        &task_id,
        SessionTurnContent::new("closed").unwrap(),
        TaskObservationId::new("closed-attempt").unwrap(),
        ToolInvocationId::new("closed-invocation").unwrap(),
        &mut registry,
        &mut authorizer,
    );
    assert!(matches!(
        closed,
        Err(ToolTaskRuntimeError::Session(
            vela_kernel::session::SessionStoreError::SessionClosed { .. }
        ))
    ));
    assert!(calls.borrow().is_empty());
    assert_eq!(authorizer.calls, 0);
}

#[test]
fn invalid_attempt_text_preserves_both_transcript_turns() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let provider = Provider {
        calls: Rc::new(RefCell::new(Vec::new())),
        response: Some(Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("   ").unwrap(),
        ))),
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut registry = ToolRegistry::new();
    let mut authorizer = Allow { calls: 0 };

    let error = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            ToolInvocationId::new("unused").unwrap(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap_err();

    assert!(matches!(error, ToolTaskRuntimeError::InvalidAttemptText(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].content().as_str(), "   ");
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
fn provider_failure_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id) = setup(&path);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        calls: calls.clone(),
        response: Some(Err(ProviderError::new(FakeProviderFailure))),
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut registry = ToolRegistry::new();
    let mut authorizer = Allow { calls: 0 };

    let error = runtime
        .execute_task_turn(
            &task_id,
            SessionTurnContent::new("durable human").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            ToolInvocationId::new("unused").unwrap(),
            &mut registry,
            &mut authorizer,
        )
        .unwrap_err();

    assert!(matches!(error, ToolTaskRuntimeError::Provider(_)));
    assert_eq!(error.source().unwrap().to_string(), "provider unavailable");
    assert_eq!(calls.borrow().len(), 1);
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert_eq!(session.turns()[0].content().as_str(), "durable human");
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
