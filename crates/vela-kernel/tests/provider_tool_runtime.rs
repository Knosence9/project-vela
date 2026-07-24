use std::{cell::Cell, error::Error, fmt, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        ProviderError, ProviderToolResponse, ProviderToolStepError, ProviderToolStepOutcome,
        ToolAssistantProvider, ToolStepContinuation, execute_provider_tool_step,
    },
    session::{SessionTurn, SessionTurnContent},
    task::{TaskGoal, TaskId, TaskStore},
    tool::{
        DurableToolInvocationError, DurableToolRegistryInvocationError, PermissionDecision, Tool,
        ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationId, ToolInvocationStatus,
        ToolInvocationStore, ToolInvocationStoreError, ToolMetadata, ToolRegistry, ToolRequest,
    },
};

#[derive(Debug)]
struct FakeFailure(&'static str);

impl fmt::Display for FakeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FakeFailure {}

struct FakeProvider {
    calls: Rc<Cell<usize>>,
    response: Option<Result<ProviderToolResponse, ProviderError>>,
    observed_tools: Vec<Vec<(String, ToolEffect)>>,
}

impl FakeProvider {
    fn returning(response: ProviderToolResponse, calls: Rc<Cell<usize>>) -> Self {
        Self {
            calls,
            response: Some(Ok(response)),
            observed_tools: Vec::new(),
        }
    }

    fn failing(calls: Rc<Cell<usize>>) -> Self {
        Self {
            calls,
            response: Some(Err(ProviderError::new(FakeFailure("provider failed")))),
            observed_tools: Vec::new(),
        }
    }
}

impl ToolAssistantProvider for FakeProvider {
    fn complete_with_tools(
        &mut self,
        _transcript: &[SessionTurn],
        tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError> {
        self.calls.set(self.calls.get() + 1);
        self.observed_tools.push(
            tools
                .iter()
                .map(|tool| (tool.id().as_str().to_owned(), tool.effect()))
                .collect(),
        );
        self.response
            .take()
            .expect("provider called more than once")
    }
}

struct FakeTool {
    id: ToolId,
    calls: Rc<Cell<usize>>,
    fail: bool,
}

impl FakeTool {
    fn new(id: &str, calls: Rc<Cell<usize>>) -> Self {
        Self {
            id: ToolId::new(id).unwrap(),
            calls,
            fail: false,
        }
    }
}

impl Tool for FakeTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }

    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            Err(ToolError::new(FakeFailure("adapter failed")))
        } else {
            Ok(json!({"echo": input}))
        }
    }
}

struct RecordingAuthorizer {
    decision: PermissionDecision,
    calls: usize,
    inputs: Vec<Value>,
}

impl ToolAuthorizer for RecordingAuthorizer {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        self.inputs.push(request.input().clone());
        self.decision
    }
}

fn authorizer(decision: PermissionDecision) -> RecordingAuthorizer {
    RecordingAuthorizer {
        decision,
        calls: 0,
        inputs: Vec::new(),
    }
}

fn active_task(path: &std::path::Path) -> TaskId {
    let task_id = TaskId::new("task-1").unwrap();
    TaskStore::open(path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("use one tool").unwrap())
        .unwrap();
    task_id
}

#[test]
fn final_content_returns_complete_without_tool_evidence_or_second_provider_call() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = FakeProvider::returning(
        ProviderToolResponse::Final(SessionTurnContent::new("finished").unwrap()),
        calls.clone(),
    );
    let mut registry = ToolRegistry::new();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("caller-owned").unwrap();
    let mut allow = authorizer(PermissionDecision::Allow);

    let outcome = execute_provider_tool_step(
        &mut provider,
        &[],
        &mut registry,
        &mut store,
        &TaskId::new("not-consulted").unwrap(),
        invocation_id.clone(),
        &mut allow,
    )
    .unwrap();

    assert!(matches!(
        outcome,
        ProviderToolStepOutcome::Final { ref content, continuation: ToolStepContinuation::Complete }
            if content.as_str() == "finished"
    ));
    assert_eq!(calls.get(), 1);
    assert_eq!(allow.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());
}

#[test]
fn one_allowed_tool_request_dispatches_durably_and_returns_exact_output_for_continuation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = active_task(&path);
    let tool_id = ToolId::new("tool.echo").unwrap();
    let input = json!({"secret": "memory-only", "count": 2});
    let provider_calls = Rc::new(Cell::new(0));
    let tool_calls = Rc::new(Cell::new(0));
    let mut provider = FakeProvider::returning(
        ProviderToolResponse::ToolRequest {
            tool_id: tool_id.clone(),
            input: input.clone(),
        },
        provider_calls.clone(),
    );
    let mut registry = ToolRegistry::new();
    registry
        .register(FakeTool::new(tool_id.as_str(), tool_calls.clone()))
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("caller-owned").unwrap();
    let mut allow = authorizer(PermissionDecision::Allow);

    let outcome = execute_provider_tool_step(
        &mut provider,
        &[],
        &mut registry,
        &mut store,
        &task_id,
        invocation_id.clone(),
        &mut allow,
    )
    .unwrap();

    assert!(matches!(
        outcome,
        ProviderToolStepOutcome::ToolCompleted {
            tool_id: ref actual_id,
            input: ref actual_input,
            ref output,
            continuation: ToolStepContinuation::ProviderRequired,
        } if actual_id == &tool_id
            && actual_input == &input
            && output == &json!({"echo": input})
    ));
    assert_eq!(provider_calls.get(), 1);
    assert_eq!(tool_calls.get(), 1);
    assert_eq!(allow.calls, 1);
    assert_eq!(allow.inputs, vec![input]);
    assert_eq!(
        provider.observed_tools,
        vec![vec![("tool.echo".into(), ToolEffect::Pure)]]
    );
    let invocation = store.load(&invocation_id).unwrap().unwrap();
    assert_eq!(invocation.id(), &invocation_id);
    assert_eq!(invocation.task_id(), Some(&task_id));
    assert_eq!(invocation.status(), ToolInvocationStatus::Succeeded);

    let retained: Vec<Vec<u8>> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare("SELECT payload FROM events WHERE stream_id = ?1 ORDER BY stream_version")
        .unwrap()
        .query_map(("tool-invocation:caller-owned",), |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        retained
            .iter()
            .all(|payload| !payload.windows(11).any(|window| window == b"memory-only"))
    );
}

#[test]
fn provider_failure_precedes_lookup_intent_authorization_and_tool_execution() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = FakeProvider::failing(calls.clone());
    let tool_calls = Rc::new(Cell::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(FakeTool::new("tool.echo", tool_calls.clone()))
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("provider-failure").unwrap();
    let mut allow = authorizer(PermissionDecision::Allow);

    let error = execute_provider_tool_step(
        &mut provider,
        &[],
        &mut registry,
        &mut store,
        &TaskId::new("missing").unwrap(),
        invocation_id.clone(),
        &mut allow,
    )
    .unwrap_err();

    assert!(matches!(error, ProviderToolStepError::Provider(_)));
    assert_eq!(error.source().unwrap().to_string(), "provider failed");
    assert_eq!(calls.get(), 1);
    assert_eq!(tool_calls.get(), 0);
    assert_eq!(allow.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());
}

#[test]
fn unknown_tool_is_typed_and_writes_no_intent() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = active_task(&path);
    let missing = ToolId::new("tool.missing").unwrap();
    let mut provider = FakeProvider::returning(
        ProviderToolResponse::ToolRequest {
            tool_id: missing.clone(),
            input: json!(null),
        },
        Rc::new(Cell::new(0)),
    );
    let mut registry = ToolRegistry::new();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("unknown-tool").unwrap();
    let mut allow = authorizer(PermissionDecision::Allow);

    let error = execute_provider_tool_step(
        &mut provider,
        &[],
        &mut registry,
        &mut store,
        &task_id,
        invocation_id.clone(),
        &mut allow,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ProviderToolStepError::Invocation(DurableToolRegistryInvocationError::NotFound {
            ref tool_id
        }) if tool_id == &missing
    ));
    assert_eq!(allow.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());
}

#[test]
fn denial_adapter_failure_and_preexisting_identity_preserve_existing_typed_failures() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = active_task(&path);
    let tool_id = ToolId::new("tool.echo").unwrap();
    let tool_calls = Rc::new(Cell::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(FakeTool::new(tool_id.as_str(), tool_calls.clone()))
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();

    let mut denied_provider = FakeProvider::returning(
        ProviderToolResponse::ToolRequest {
            tool_id: tool_id.clone(),
            input: json!("denied"),
        },
        Rc::new(Cell::new(0)),
    );
    let denied_id = ToolInvocationId::new("denied").unwrap();
    let mut deny = authorizer(PermissionDecision::Deny);
    let denied = execute_provider_tool_step(
        &mut denied_provider,
        &[],
        &mut registry,
        &mut store,
        &task_id,
        denied_id.clone(),
        &mut deny,
    )
    .unwrap_err();
    assert!(matches!(
        denied,
        ProviderToolStepError::Invocation(DurableToolRegistryInvocationError::Invocation(
            DurableToolInvocationError::Invocation(_)
        ))
    ));
    assert_eq!(
        store.load(&denied_id).unwrap().unwrap().status(),
        ToolInvocationStatus::Denied
    );
    assert_eq!(tool_calls.get(), 0);

    let mut duplicate_provider = FakeProvider::returning(
        ProviderToolResponse::ToolRequest {
            tool_id: tool_id.clone(),
            input: json!("duplicate"),
        },
        Rc::new(Cell::new(0)),
    );
    let mut allow = authorizer(PermissionDecision::Allow);
    let duplicate = execute_provider_tool_step(
        &mut duplicate_provider,
        &[],
        &mut registry,
        &mut store,
        &task_id,
        denied_id,
        &mut allow,
    )
    .unwrap_err();
    assert!(matches!(
        duplicate,
        ProviderToolStepError::Invocation(DurableToolRegistryInvocationError::Invocation(
            DurableToolInvocationError::Store(ToolInvocationStoreError::AlreadyExists { .. })
        ))
    ));
    assert_eq!(allow.calls, 0);

    let mut missing_task_provider = FakeProvider::returning(
        ProviderToolResponse::ToolRequest {
            tool_id: tool_id.clone(),
            input: json!("missing task"),
        },
        Rc::new(Cell::new(0)),
    );
    let missing_task = execute_provider_tool_step(
        &mut missing_task_provider,
        &[],
        &mut registry,
        &mut store,
        &TaskId::new("missing").unwrap(),
        ToolInvocationId::new("missing-task").unwrap(),
        &mut allow,
    )
    .unwrap_err();
    assert!(matches!(
        missing_task,
        ProviderToolStepError::Invocation(DurableToolRegistryInvocationError::Invocation(
            DurableToolInvocationError::Store(ToolInvocationStoreError::TaskNotFound { .. })
        ))
    ));
    assert_eq!(allow.calls, 0);

    let failing_calls = Rc::new(Cell::new(0));
    let failing_id = ToolId::new("tool.fail").unwrap();
    registry
        .register(FakeTool {
            id: failing_id.clone(),
            calls: failing_calls.clone(),
            fail: true,
        })
        .unwrap();
    let mut failure_provider = FakeProvider::returning(
        ProviderToolResponse::ToolRequest {
            tool_id: failing_id,
            input: json!(null),
        },
        Rc::new(Cell::new(0)),
    );
    let mut allow = authorizer(PermissionDecision::Allow);
    let failure = execute_provider_tool_step(
        &mut failure_provider,
        &[],
        &mut registry,
        &mut store,
        &task_id,
        ToolInvocationId::new("adapter-failure").unwrap(),
        &mut allow,
    )
    .unwrap_err();
    assert!(matches!(
        failure,
        ProviderToolStepError::Invocation(DurableToolRegistryInvocationError::Invocation(
            DurableToolInvocationError::Invocation(_)
        ))
    ));
    assert_eq!(
        failure
            .source()
            .unwrap()
            .source()
            .unwrap()
            .source()
            .unwrap()
            .to_string(),
        "tool tool.fail failed: adapter failed"
    );
    assert_eq!(failing_calls.get(), 1);
}
