use std::{cell::Cell, error::Error, fmt, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    task::{TaskGoal, TaskId, TaskStore},
    tool::{
        DurableToolInvocationError, DurableToolRegistryInvocationError, PermissionDecision, Tool,
        ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationId, ToolInvocationStatus,
        ToolInvocationStore, ToolInvocationStoreError, ToolRegistry, ToolRegistryError,
        ToolRegistryInvocationError, ToolRequest,
    },
};

#[derive(Debug)]
struct FakeError;

impl fmt::Display for FakeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("registry fake failed")
    }
}

impl Error for FakeError {}

struct FakeTool {
    id: ToolId,
    effect: ToolEffect,
    calls: Rc<Cell<usize>>,
    fails: bool,
}

impl FakeTool {
    fn new(id: &str, effect: ToolEffect, calls: Rc<Cell<usize>>) -> Self {
        Self {
            id: ToolId::new(id).unwrap(),
            effect,
            calls,
            fails: false,
        }
    }

    fn failing(id: &str, calls: Rc<Cell<usize>>) -> Self {
        Self {
            fails: true,
            ..Self::new(id, ToolEffect::Pure, calls)
        }
    }
}

impl Tool for FakeTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        self.effect
    }

    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        self.calls.set(self.calls.get() + 1);
        if self.fails {
            Err(ToolError::new(FakeError))
        } else {
            Ok(input.clone())
        }
    }
}

struct RecordingAuthorizer {
    decision: PermissionDecision,
    calls: usize,
    observed: Vec<(ToolId, ToolEffect, Value)>,
}

impl RecordingAuthorizer {
    fn new(decision: PermissionDecision) -> Self {
        Self {
            decision,
            calls: 0,
            observed: Vec::new(),
        }
    }
}

impl ToolAuthorizer for RecordingAuthorizer {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        self.observed.push((
            request.tool_id().clone(),
            request.effect(),
            request.input().clone(),
        ));
        self.decision
    }
}

#[test]
fn registry_lists_metadata_in_id_order_independent_of_registration_order() {
    let mut registry = ToolRegistry::new();
    let write_calls = Rc::new(Cell::new(0));
    let pure_calls = Rc::new(Cell::new(0));
    let read_calls = Rc::new(Cell::new(0));
    let destructive_calls = Rc::new(Cell::new(0));
    registry
        .register(FakeTool::new(
            "tool.write",
            ToolEffect::ExternalWrite,
            write_calls.clone(),
        ))
        .unwrap();
    registry
        .register(FakeTool::new(
            "tool.pure",
            ToolEffect::Pure,
            pure_calls.clone(),
        ))
        .unwrap();
    registry
        .register(FakeTool::new(
            "tool.read",
            ToolEffect::ExternalRead,
            read_calls.clone(),
        ))
        .unwrap();
    registry
        .register(FakeTool::new(
            "tool.destroy",
            ToolEffect::Destructive,
            destructive_calls.clone(),
        ))
        .unwrap();

    let metadata = registry.metadata();
    assert_eq!(
        [
            write_calls.get(),
            pure_calls.get(),
            read_calls.get(),
            destructive_calls.get(),
        ],
        [0; 4]
    );
    let actual: Vec<_> = metadata
        .iter()
        .map(|entry| (entry.id().as_str(), entry.effect()))
        .collect();
    assert_eq!(
        actual,
        vec![
            ("tool.destroy", ToolEffect::Destructive),
            ("tool.pure", ToolEffect::Pure),
            ("tool.read", ToolEffect::ExternalRead),
            ("tool.write", ToolEffect::ExternalWrite),
        ]
    );

    for (id, expected_effect) in [
        ("tool.destroy", ToolEffect::Destructive),
        ("tool.pure", ToolEffect::Pure),
        ("tool.read", ToolEffect::ExternalRead),
        ("tool.write", ToolEffect::ExternalWrite),
    ] {
        let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);
        registry
            .invoke(&ToolId::new(id).unwrap(), &mut authorizer, &json!(null))
            .unwrap();
        assert_eq!(authorizer.observed[0].1, expected_effect);
    }
}

#[test]
fn duplicate_registration_is_rejected_without_replacing_the_original() {
    let original_calls = Rc::new(Cell::new(0));
    let duplicate_calls = Rc::new(Cell::new(0));
    let id = ToolId::new("tool.same").unwrap();
    let mut registry = ToolRegistry::new();
    registry
        .register(FakeTool::new(
            id.as_str(),
            ToolEffect::Pure,
            original_calls.clone(),
        ))
        .unwrap();

    let error = registry
        .register(FakeTool::new(
            id.as_str(),
            ToolEffect::Destructive,
            duplicate_calls.clone(),
        ))
        .unwrap_err();
    assert!(matches!(
        error,
        ToolRegistryError::DuplicateId { ref tool_id } if tool_id == &id
    ));

    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);
    registry.invoke(&id, &mut authorizer, &json!("ok")).unwrap();
    assert_eq!(original_calls.get(), 1);
    assert_eq!(duplicate_calls.get(), 0);
    assert_eq!(authorizer.observed[0].1, ToolEffect::Pure);
}

#[test]
fn registry_invocation_preserves_allow_deny_and_sourced_failure_behavior() {
    let allowed_calls = Rc::new(Cell::new(0));
    let denied_calls = Rc::new(Cell::new(0));
    let failed_calls = Rc::new(Cell::new(0));
    let mut registry = ToolRegistry::new();
    let allowed_id = ToolId::new("tool.allowed").unwrap();
    let denied_id = ToolId::new("tool.denied").unwrap();
    let failed_id = ToolId::new("tool.failed").unwrap();
    registry
        .register(FakeTool::new(
            allowed_id.as_str(),
            ToolEffect::ExternalRead,
            allowed_calls.clone(),
        ))
        .unwrap();
    registry
        .register(FakeTool::new(
            denied_id.as_str(),
            ToolEffect::ExternalWrite,
            denied_calls.clone(),
        ))
        .unwrap();
    registry
        .register(FakeTool::failing(failed_id.as_str(), failed_calls.clone()))
        .unwrap();

    let input = json!({"exact": true});
    let mut allow = RecordingAuthorizer::new(PermissionDecision::Allow);
    assert_eq!(
        registry.invoke(&allowed_id, &mut allow, &input).unwrap(),
        input
    );
    assert_eq!(
        allow.observed[0],
        (allowed_id, ToolEffect::ExternalRead, input)
    );
    assert_eq!(allowed_calls.get(), 1);

    let mut deny = RecordingAuthorizer::new(PermissionDecision::Deny);
    let denied = registry
        .invoke(&denied_id, &mut deny, &json!(null))
        .unwrap_err();
    assert!(matches!(denied, ToolRegistryInvocationError::Invocation(_)));
    assert_eq!(denied_calls.get(), 0);

    let mut allow = RecordingAuthorizer::new(PermissionDecision::Allow);
    let failed = registry
        .invoke(&failed_id, &mut allow, &json!(null))
        .unwrap_err();
    assert!(matches!(failed, ToolRegistryInvocationError::Invocation(_)));
    assert_eq!(failed_calls.get(), 1);
    assert_eq!(
        failed.source().unwrap().source().unwrap().to_string(),
        "registry fake failed"
    );
}

#[test]
fn unknown_id_fails_before_authorization() {
    let missing = ToolId::new("tool.missing").unwrap();
    let mut registry = ToolRegistry::new();
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);

    let error = registry
        .invoke(
            &missing,
            &mut authorizer,
            &json!({"secret": "not observed"}),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ToolRegistryInvocationError::NotFound { ref tool_id } if tool_id == &missing
    ));
    assert_eq!(authorizer.calls, 0);
}

#[test]
fn registry_dispatches_a_known_tool_through_durable_task_invocation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("task-1").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            task_id.clone(),
            TaskGoal::new("use registered tool").unwrap(),
        )
        .unwrap();
    let invocation_id = ToolInvocationId::new("call-1").unwrap();
    let tool_id = ToolId::new("tool.echo").unwrap();
    let calls = Rc::new(Cell::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(FakeTool::new(
            tool_id.as_str(),
            ToolEffect::ExternalRead,
            calls.clone(),
        ))
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);
    let input = json!({"exact": "value"});

    let output = registry
        .invoke_for_task_durable(
            &mut store,
            &task_id,
            invocation_id.clone(),
            &tool_id,
            &mut authorizer,
            &input,
        )
        .unwrap();

    assert_eq!(output, input);
    assert_eq!(calls.get(), 1);
    assert_eq!(authorizer.calls, 1);
    let invocation = store.load(&invocation_id).unwrap().unwrap();
    assert_eq!(invocation.tool_id(), &tool_id);
    assert_eq!(invocation.task_id(), Some(&task_id));
    assert_eq!(invocation.status(), ToolInvocationStatus::Succeeded);

    let mut duplicate_authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);
    let duplicate = registry
        .invoke_for_task_durable(
            &mut store,
            &task_id,
            invocation_id,
            &tool_id,
            &mut duplicate_authorizer,
            &json!(null),
        )
        .unwrap_err();
    assert!(matches!(
        duplicate,
        DurableToolRegistryInvocationError::Invocation(DurableToolInvocationError::Store(
            ToolInvocationStoreError::AlreadyExists { .. }
        ))
    ));
    assert_eq!(duplicate_authorizer.calls, 0);
    assert_eq!(calls.get(), 1);

    let denied_id = ToolInvocationId::new("denied-call").unwrap();
    let mut deny = RecordingAuthorizer::new(PermissionDecision::Deny);
    let denied = registry
        .invoke_for_task_durable(
            &mut store,
            &task_id,
            denied_id.clone(),
            &tool_id,
            &mut deny,
            &json!(null),
        )
        .unwrap_err();
    assert!(matches!(
        denied,
        DurableToolRegistryInvocationError::Invocation(DurableToolInvocationError::Invocation(_))
    ));
    assert_eq!(deny.calls, 1);
    assert_eq!(calls.get(), 1);
    assert_eq!(
        store.load(&denied_id).unwrap().unwrap().status(),
        ToolInvocationStatus::Denied
    );
}

#[test]
fn unknown_durable_registry_id_fails_before_intent_or_authorization() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let invocation_id = ToolInvocationId::new("unknown-call").unwrap();
    let missing = ToolId::new("tool.missing").unwrap();
    let mut registry = ToolRegistry::new();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);

    let error = registry
        .invoke_for_task_durable(
            &mut store,
            &TaskId::new("missing-task").unwrap(),
            invocation_id.clone(),
            &missing,
            &mut authorizer,
            &json!({"secret": "not persisted"}),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        DurableToolRegistryInvocationError::NotFound { ref tool_id } if tool_id == &missing
    ));
    assert_eq!(authorizer.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());
}

#[test]
fn durable_registry_dispatch_preserves_existing_pre_execution_and_invocation_failures() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let tool_id = ToolId::new("tool.failing").unwrap();
    let calls = Rc::new(Cell::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(FakeTool::failing(tool_id.as_str(), calls.clone()))
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);

    let task_error = registry
        .invoke_for_task_durable(
            &mut store,
            &TaskId::new("missing").unwrap(),
            ToolInvocationId::new("missing-task-call").unwrap(),
            &tool_id,
            &mut authorizer,
            &json!(null),
        )
        .unwrap_err();
    assert!(matches!(
        task_error,
        DurableToolRegistryInvocationError::Invocation(DurableToolInvocationError::Store(
            ToolInvocationStoreError::TaskNotFound { .. }
        ))
    ));
    assert_eq!(authorizer.calls, 0);
    assert_eq!(calls.get(), 0);

    let task_id = TaskId::new("task-1").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            task_id.clone(),
            TaskGoal::new("fail registered tool").unwrap(),
        )
        .unwrap();
    let failure = registry
        .invoke_for_task_durable(
            &mut store,
            &task_id,
            ToolInvocationId::new("failed-call").unwrap(),
            &tool_id,
            &mut authorizer,
            &json!(null),
        )
        .unwrap_err();
    assert!(matches!(
        failure,
        DurableToolRegistryInvocationError::Invocation(DurableToolInvocationError::Invocation(_))
    ));
    assert_eq!(
        failure.source().unwrap().source().unwrap().to_string(),
        "tool tool.failing failed: registry fake failed"
    );
    assert_eq!(authorizer.calls, 1);
    assert_eq!(calls.get(), 1);
}
