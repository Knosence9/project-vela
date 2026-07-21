use std::{error::Error, fmt};

use serde::Serialize;
use serde_json::Value;

/// An opaque, non-blank stable identifier for one tool adapter.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolId(String);

impl ToolId {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(ToolIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolIdError;

impl fmt::Display for ToolIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("tool id must not be blank")
    }
}

impl Error for ToolIdError {}

/// The maximum external effect declared by a tool adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ToolEffect {
    /// No observation or mutation outside the current process.
    Pure,
    /// Observes external state without intending to mutate it.
    ExternalRead,
    /// Mutates external state without intending destructive removal.
    ExternalWrite,
    /// May delete, irreversibly replace, or otherwise destructively mutate external state.
    Destructive,
}

/// One exact invocation presented to permission policy before adapter execution.
#[derive(Clone, Copy, Debug)]
pub struct ToolRequest<'a> {
    tool_id: &'a ToolId,
    effect: ToolEffect,
    input: &'a Value,
}

impl<'a> ToolRequest<'a> {
    pub fn tool_id(&self) -> &'a ToolId {
        self.tool_id
    }

    pub fn effect(&self) -> ToolEffect {
        self.effect
    }

    pub fn input(&self) -> &'a Value {
        self.input
    }
}

/// An explicit permission decision scoped to one exact invocation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

/// Caller-owned policy evaluated before each tool invocation.
pub trait ToolAuthorizer {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision;
}

/// An extension-owned synchronous adapter behind the kernel tool protocol.
///
/// `id` and `effect` are permission metadata accessors and must not produce tool effects.
pub trait Tool {
    /// Returns the adapter's stable identity without producing a tool effect.
    fn id(&self) -> &ToolId;

    /// Returns the adapter's maximum declared effect without producing that effect.
    fn effect(&self) -> ToolEffect;

    /// Executes once after explicit authorization.
    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError>;
}

/// An adapter failure that preserves the extension-specific error as its source.
#[derive(Debug)]
pub struct ToolError {
    source: Box<dyn Error>,
}

impl ToolError {
    pub fn new(error: impl Error + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for ToolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// A permission denial or adapter failure from one tool invocation.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolInvocationError {
    Denied { tool_id: ToolId, effect: ToolEffect },
    Tool { tool_id: ToolId, error: ToolError },
}

impl fmt::Display for ToolInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied { tool_id, effect } => {
                write!(formatter, "permission denied for {effect:?} tool {tool_id}")
            }
            Self::Tool { tool_id, error } => write!(formatter, "tool {tool_id} failed: {error}"),
        }
    }
}

impl Error for ToolInvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Denied { .. } => None,
            Self::Tool { error, .. } => Some(error),
        }
    }
}

/// Authorizes and invokes one tool adapter without persistence, retry, or rollback.
pub fn invoke_tool<T: Tool, A: ToolAuthorizer>(
    tool: &mut T,
    authorizer: &mut A,
    input: &Value,
) -> Result<Value, ToolInvocationError> {
    let tool_id = tool.id().clone();
    let effect = tool.effect();
    let decision = authorizer.authorize(ToolRequest {
        tool_id: &tool_id,
        effect,
        input,
    });
    if decision == PermissionDecision::Deny {
        return Err(ToolInvocationError::Denied { tool_id, effect });
    }

    tool.invoke(input)
        .map_err(|error| ToolInvocationError::Tool { tool_id, error })
}
