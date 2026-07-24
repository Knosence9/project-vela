use std::{cell::Cell, error::Error, fmt, rc::Rc};

use serde_json::{Value, json};
use vela_kernel::tool::{
    PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolRegistry,
    ToolRegistryError, ToolRegistryInvocationError, ToolRequest,
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
