use std::{cell::Cell, error::Error, fmt, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        ProviderError, ProviderToolContinuation, ProviderToolResponse, ProviderToolStepError,
        ProviderToolStepOutcome, ToolAssistantContinuationProvider, ToolStepContinuation,
        continue_provider_tool_step,
    },
    session::SessionTurnContent,
    task::{TaskGoal, TaskId, TaskStore},
    tool::{
        PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationId,
        ToolInvocationStatus, ToolInvocationStore, ToolMetadata, ToolRegistry, ToolRequest,
    },
};

#[derive(Debug)]
struct FakeFailure(&'static str);
impl fmt::Display for FakeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl Error for FakeFailure {}

struct Provider {
    calls: Rc<Cell<usize>>,
    response: Option<Result<ProviderToolResponse, ProviderError>>,
    observed: Option<(String, Value, Value)>,
}
impl ToolAssistantContinuationProvider for Provider {
    fn complete_after_tool(
        &mut self,
        continuation: ProviderToolContinuation<'_>,
        _tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError> {
        let result = continuation.prior_result();
        self.calls.set(self.calls.get() + 1);
        self.observed = Some((
            result.tool_id().as_str().to_owned(),
            result.input().clone(),
            result.output().clone(),
        ));
        self.response
            .take()
            .expect("provider called more than once")
    }
}

struct EchoTool {
    id: ToolId,
    calls: Rc<Cell<usize>>,
}
impl Tool for EchoTool {
    fn id(&self) -> &ToolId {
        &self.id
    }
    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }
    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        self.calls.set(self.calls.get() + 1);
        Ok(json!({"echo": input}))
    }
}

struct Authorizer {
    calls: usize,
}
impl ToolAuthorizer for Authorizer {
    fn authorize(&mut self, _request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        PermissionDecision::Allow
    }
}

fn prior() -> ProviderToolStepOutcome {
    ProviderToolStepOutcome::ToolCompleted {
        tool_id: ToolId::new("tool.prior").unwrap(),
        input: json!({"secret": "request"}),
        output: json!({"secret": "response"}),
        continuation: ToolStepContinuation::ProviderRequired,
    }
}

#[test]
fn continuation_delivers_exact_prior_result_and_can_finish_without_new_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("finished after tool").unwrap(),
        ))),
        observed: None,
    };
    let mut registry = ToolRegistry::new();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let next_id = ToolInvocationId::new("unused-next-id").unwrap();
    let mut authorizer = Authorizer { calls: 0 };

    let outcome = continue_provider_tool_step(
        &mut provider,
        ProviderToolContinuation::new(&[], prior().tool_result().unwrap()),
        &mut registry,
        &mut store,
        &TaskId::new("not-consulted").unwrap(),
        next_id.clone(),
        &mut authorizer,
    )
    .unwrap();

    assert!(matches!(outcome,
        ProviderToolStepOutcome::Final { ref content, continuation: ToolStepContinuation::Complete }
            if content.as_str() == "finished after tool"));
    assert_eq!(calls.get(), 1);
    assert_eq!(authorizer.calls, 0);
    assert_eq!(
        provider.observed,
        Some((
            "tool.prior".into(),
            json!({"secret": "request"}),
            json!({"secret": "response"})
        ))
    );
    assert!(store.load(&next_id).unwrap().is_none());
}

#[test]
fn continuation_dispatches_one_subsequent_tool_with_fresh_identity() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = TaskId::new("task-1").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("continue").unwrap())
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let next_tool_id = ToolId::new("tool.next").unwrap();
    let mut provider = Provider {
        calls: calls.clone(),
        response: Some(Ok(ProviderToolResponse::ToolRequest {
            tool_id: next_tool_id.clone(),
            input: json!({"secret": "memory-only-next"}),
        })),
        observed: None,
    };
    let tool_calls = Rc::new(Cell::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(EchoTool {
            id: next_tool_id.clone(),
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let next_id = ToolInvocationId::new("caller-owned-next").unwrap();
    let mut authorizer = Authorizer { calls: 0 };

    let outcome = continue_provider_tool_step(
        &mut provider,
        ProviderToolContinuation::new(&[], prior().tool_result().unwrap()),
        &mut registry,
        &mut store,
        &task_id,
        next_id.clone(),
        &mut authorizer,
    )
    .unwrap();

    assert!(matches!(outcome,
        ProviderToolStepOutcome::ToolCompleted { ref tool_id,
            continuation: ToolStepContinuation::ProviderRequired, .. } if tool_id == &next_tool_id));
    assert_eq!((calls.get(), tool_calls.get(), authorizer.calls), (1, 1, 1));
    let invocation = store.load(&next_id).unwrap().unwrap();
    assert_eq!(invocation.task_id(), Some(&task_id));
    assert_eq!(invocation.status(), ToolInvocationStatus::Succeeded);

    let retained: Vec<Vec<u8>> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare("SELECT payload FROM events WHERE stream_id = ?1 ORDER BY stream_version")
        .unwrap()
        .query_map(("tool-invocation:caller-owned-next",), |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(retained.iter().all(|payload| {
        !payload
            .windows(16)
            .any(|window| window == b"memory-only-next")
    }));
}

#[test]
fn continuation_provider_failure_precedes_new_invocation_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = Provider {
        calls: calls.clone(),
        response: Some(Err(ProviderError::new(FakeFailure("continuation failed")))),
        observed: None,
    };
    let mut registry = ToolRegistry::new();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let next_id = ToolInvocationId::new("not-written").unwrap();
    let mut authorizer = Authorizer { calls: 0 };

    let error = continue_provider_tool_step(
        &mut provider,
        ProviderToolContinuation::new(&[], prior().tool_result().unwrap()),
        &mut registry,
        &mut store,
        &TaskId::new("missing").unwrap(),
        next_id.clone(),
        &mut authorizer,
    )
    .unwrap_err();

    assert!(matches!(error, ProviderToolStepError::Provider(_)));
    assert_eq!(error.source().unwrap().to_string(), "continuation failed");
    assert_eq!((calls.get(), authorizer.calls), (1, 0));
    assert!(store.load(&next_id).unwrap().is_none());
}
