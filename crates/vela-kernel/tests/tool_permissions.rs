use std::{error::Error, fmt};

use serde_json::{Value, json};
use vela_kernel::tool::{
    PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationError,
    ToolRequest, invoke_tool,
};

#[derive(Debug)]
struct FakeToolError;

impl fmt::Display for FakeToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fake tool failed")
    }
}

impl Error for FakeToolError {}

struct FakeTool {
    id: ToolId,
    calls: usize,
    result: Option<Result<Value, ToolError>>,
}

impl FakeTool {
    fn succeeding(output: Value) -> Self {
        Self {
            id: ToolId::new("test.echo").expect("valid tool id"),
            calls: 0,
            result: Some(Ok(output)),
        }
    }

    fn failing() -> Self {
        Self {
            id: ToolId::new("test.echo").expect("valid tool id"),
            calls: 0,
            result: Some(Err(ToolError::new(FakeToolError))),
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

    fn invoke(&mut self, _input: &Value) -> Result<Value, ToolError> {
        self.calls += 1;
        self.result.take().expect("tool must not be retried")
    }
}

struct RecordingAuthorizer {
    decision: PermissionDecision,
    calls: usize,
    observed_id: Option<ToolId>,
    observed_effect: Option<ToolEffect>,
    observed_input: Option<Value>,
}

impl RecordingAuthorizer {
    fn new(decision: PermissionDecision) -> Self {
        Self {
            decision,
            calls: 0,
            observed_id: None,
            observed_effect: None,
            observed_input: None,
        }
    }
}

impl ToolAuthorizer for RecordingAuthorizer {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        self.observed_id = Some(request.tool_id().clone());
        self.observed_effect = Some(request.effect());
        self.observed_input = Some(request.input().clone());
        self.decision
    }
}

#[test]
fn tool_ids_are_non_blank_stable_values() {
    assert!(ToolId::new("").is_err());
    assert!(ToolId::new(" \t\n").is_err());

    let id = ToolId::new("test.echo").expect("valid tool id");
    assert_eq!(id.as_str(), "test.echo");
    assert_eq!(id.to_string(), "test.echo");
    assert_eq!(id, ToolId::new("test.echo").expect("same valid id"));
}

#[test]
fn denial_observes_the_exact_request_before_skipping_the_tool() {
    let input = json!({"message": "hello"});
    let mut tool = FakeTool::succeeding(json!({"ignored": true}));
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Deny);

    let error = invoke_tool(&mut tool, &mut authorizer, &input).expect_err("must deny");

    assert!(matches!(
        error,
        ToolInvocationError::Denied { ref tool_id, effect: ToolEffect::Pure }
            if tool_id.as_str() == "test.echo"
    ));
    assert_eq!(authorizer.calls, 1);
    assert_eq!(authorizer.observed_id.as_ref(), Some(tool.id()));
    assert_eq!(authorizer.observed_effect, Some(ToolEffect::Pure));
    assert_eq!(authorizer.observed_input.as_ref(), Some(&input));
    assert_eq!(tool.calls, 0);
}

#[test]
fn allowance_invokes_once_and_returns_the_exact_output() {
    let input = json!({"message": "hello"});
    let output = json!({"message": "hello", "echoed": true});
    let mut tool = FakeTool::succeeding(output.clone());
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);

    let actual = invoke_tool(&mut tool, &mut authorizer, &input).expect("must allow");

    assert_eq!(actual, output);
    assert_eq!(authorizer.calls, 1);
    assert_eq!(tool.calls, 1);
}

#[test]
fn adapter_failure_is_sourced_and_never_retried() {
    let mut tool = FakeTool::failing();
    let mut authorizer = RecordingAuthorizer::new(PermissionDecision::Allow);

    let error = invoke_tool(&mut tool, &mut authorizer, &json!(null)).expect_err("must fail");

    assert!(matches!(error, ToolInvocationError::Tool { .. }));
    assert_eq!(error.to_string(), "tool test.echo failed: fake tool failed");
    let tool_error = error.source().expect("invocation error source");
    assert_eq!(tool_error.to_string(), "fake tool failed");
    assert_eq!(
        tool_error
            .source()
            .expect("adapter error source")
            .to_string(),
        "fake tool failed"
    );
    assert_eq!(authorizer.calls, 1);
    assert_eq!(tool.calls, 1);
}
