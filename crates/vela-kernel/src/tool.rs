use std::{error::Error, fmt, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};

const TOOL_INVOCATION_INTENDED_EVENT_TYPE: &str = "tool.invocation_intended";
const TOOL_INVOCATION_DENIED_EVENT_TYPE: &str = "tool.invocation_denied";
const TOOL_INVOCATION_SUCCEEDED_EVENT_TYPE: &str = "tool.invocation_succeeded";
const TOOL_INVOCATION_FAILED_EVENT_TYPE: &str = "tool.invocation_failed";
const TOOL_INVOCATION_EVENT_PAYLOAD_VERSION: u32 = 1;
const TOOL_INVOCATION_STREAM_PREFIX: &str = "tool-invocation:";

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
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
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

/// A caller-supplied, non-blank identity for one durable invocation attempt.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolInvocationId(String);

impl ToolInvocationId {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolInvocationIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(ToolInvocationIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolInvocationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolInvocationIdError;

impl fmt::Display for ToolInvocationIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("tool invocation id must not be blank")
    }
}

impl Error for ToolInvocationIdError {}

/// The replayed state of one durable tool invocation stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolInvocationStatus {
    /// Authorization or execution may have been attempted; the invocation must not be resumed.
    Pending,
    Denied,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInvocation {
    id: ToolInvocationId,
    tool_id: ToolId,
    effect: ToolEffect,
    status: ToolInvocationStatus,
}

impl ToolInvocation {
    pub fn id(&self) -> &ToolInvocationId {
        &self.id
    }

    pub fn tool_id(&self) -> &ToolId {
        &self.tool_id
    }

    pub fn effect(&self) -> ToolEffect {
        self.effect
    }

    pub fn status(&self) -> ToolInvocationStatus {
        self.status
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ToolInvocationStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists { invocation_id: ToolInvocationId },
    InvalidStreamId { stream_id: String },
    InvalidHistory { event_count: usize },
}

impl fmt::Display for ToolInvocationStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "tool invocation event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "tool invocation replay error: {error}"),
            Self::AlreadyExists { invocation_id } => {
                write!(formatter, "tool invocation {invocation_id} already exists")
            }
            Self::InvalidStreamId { stream_id } => {
                write!(formatter, "invalid tool invocation stream id {stream_id}")
            }
            Self::InvalidHistory { event_count } => write!(
                formatter,
                "invalid tool invocation history with {event_count} events"
            ),
        }
    }
}

impl Error for ToolInvocationStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::AlreadyExists { .. }
            | Self::InvalidStreamId { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous metadata-only invocation store backed by the typed event log.
pub struct ToolInvocationStore {
    event_log: EventLog,
}

impl ToolInvocationStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ToolInvocationStoreError> {
        EventLog::open(path)
            .map(|event_log| Self { event_log })
            .map_err(ToolInvocationStoreError::EventLog)
    }

    pub fn load(
        &self,
        id: &ToolInvocationId,
    ) -> Result<Option<ToolInvocation>, ToolInvocationStoreError> {
        let events = self
            .event_log
            .replay::<ToolInvocationEvent>(&tool_invocation_stream(id))
            .map_err(ToolInvocationStoreError::Replay)?;
        project_tool_invocation(id, events)
    }

    /// Replays every persisted tool invocation in ascending invocation-ID order.
    pub fn list(&self) -> Result<Vec<ToolInvocation>, ToolInvocationStoreError> {
        let streams = self
            .event_log
            .replay_streams_with_event_type::<ToolInvocationEvent>(
                TOOL_INVOCATION_INTENDED_EVENT_TYPE,
            )
            .map_err(ToolInvocationStoreError::Replay)?;
        let mut invocations = Vec::with_capacity(streams.len());

        for (stream_id, events) in streams {
            let Some(external_id) = stream_id.strip_prefix(TOOL_INVOCATION_STREAM_PREFIX) else {
                return Err(ToolInvocationStoreError::InvalidStreamId { stream_id });
            };
            let id = ToolInvocationId::new(external_id).map_err(|_| {
                ToolInvocationStoreError::InvalidStreamId {
                    stream_id: stream_id.clone(),
                }
            })?;
            let Some(invocation) = project_tool_invocation(&id, events)? else {
                return Err(ToolInvocationStoreError::InvalidHistory { event_count: 0 });
            };
            invocations.push(invocation);
        }

        invocations.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(invocations)
    }

    fn record_intent(
        &mut self,
        id: &ToolInvocationId,
        tool_id: ToolId,
        effect: ToolEffect,
    ) -> Result<(), ToolInvocationStoreError> {
        let event = ToolInvocationEvent::Intended { tool_id, effect };
        match self.event_log.append(
            &tool_invocation_stream(id),
            ExpectedVersion::NoStream,
            &event,
        ) {
            Ok(_) => Ok(()),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(ToolInvocationStoreError::AlreadyExists {
                invocation_id: id.clone(),
            }),
            Err(error) => Err(ToolInvocationStoreError::EventLog(error)),
        }
    }

    fn record_outcome(
        &mut self,
        id: &ToolInvocationId,
        status: ToolInvocationStatus,
    ) -> Result<(), ToolInvocationStoreError> {
        let event = match status {
            ToolInvocationStatus::Denied => ToolInvocationEvent::Denied {},
            ToolInvocationStatus::Succeeded => ToolInvocationEvent::Succeeded {},
            ToolInvocationStatus::Failed => ToolInvocationEvent::Failed {},
            ToolInvocationStatus::Pending => {
                return Err(ToolInvocationStoreError::InvalidHistory { event_count: 1 });
            }
        };
        self.event_log
            .append(
                &tool_invocation_stream(id),
                ExpectedVersion::Exact(1),
                &event,
            )
            .map(|_| ())
            .map_err(ToolInvocationStoreError::EventLog)
    }
}

/// Failure before invocation, a persisted invocation failure, or a terminal append failure.
#[derive(Debug)]
pub enum DurableToolInvocationError {
    /// Intent could not be recorded, so authorization and execution did not occur.
    Store(ToolInvocationStoreError),
    /// The durable terminal outcome was recorded and the invocation was denied or failed.
    Invocation(ToolInvocationError),
    /// Authorization/execution completed, but its terminal outcome could not be appended.
    TerminalPersistence {
        /// The exact in-memory result; it is never persisted by this protocol.
        result: Result<Value, ToolInvocationError>,
        error: ToolInvocationStoreError,
    },
}

impl fmt::Display for DurableToolInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "tool invocation was not started: {error}"),
            Self::Invocation(error) => error.fmt(formatter),
            Self::TerminalPersistence { error, .. } => {
                write!(
                    formatter,
                    "tool invocation outcome was not persisted: {error}"
                )
            }
        }
    }
}

impl Error for DurableToolInvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) | Self::TerminalPersistence { error, .. } => Some(error),
            Self::Invocation(error) => Some(error),
        }
    }
}

/// Persists intent, delegates to [`invoke_tool`], and then persists metadata-only outcome.
///
/// The operation never retries. An intent-only stream is ambiguous and must not be resumed.
pub fn invoke_tool_durable<T: Tool, A: ToolAuthorizer>(
    store: &mut ToolInvocationStore,
    invocation_id: ToolInvocationId,
    tool: &mut T,
    authorizer: &mut A,
    input: &Value,
) -> Result<Value, DurableToolInvocationError> {
    store
        .record_intent(&invocation_id, tool.id().clone(), tool.effect())
        .map_err(DurableToolInvocationError::Store)?;

    let result = invoke_tool(tool, authorizer, input);
    let status = match &result {
        Ok(_) => ToolInvocationStatus::Succeeded,
        Err(ToolInvocationError::Denied { .. }) => ToolInvocationStatus::Denied,
        Err(ToolInvocationError::Tool { .. }) => ToolInvocationStatus::Failed,
    };
    if let Err(error) = store.record_outcome(&invocation_id, status) {
        return Err(DurableToolInvocationError::TerminalPersistence { result, error });
    }
    result.map_err(DurableToolInvocationError::Invocation)
}

fn tool_invocation_stream(id: &ToolInvocationId) -> StreamId {
    StreamId::new(format!("{TOOL_INVOCATION_STREAM_PREFIX}{id}"))
        .expect("a prefixed tool invocation stream is never empty")
}

fn project_tool_invocation(
    id: &ToolInvocationId,
    events: Vec<ToolInvocationEvent>,
) -> Result<Option<ToolInvocation>, ToolInvocationStoreError> {
    let Some(ToolInvocationEvent::Intended { tool_id, effect }) = events.first() else {
        return if events.is_empty() {
            Ok(None)
        } else {
            Err(ToolInvocationStoreError::InvalidHistory {
                event_count: events.len(),
            })
        };
    };
    let status = match events.as_slice() {
        [ToolInvocationEvent::Intended { .. }] => ToolInvocationStatus::Pending,
        [
            ToolInvocationEvent::Intended { .. },
            ToolInvocationEvent::Denied {},
        ] => ToolInvocationStatus::Denied,
        [
            ToolInvocationEvent::Intended { .. },
            ToolInvocationEvent::Succeeded {},
        ] => ToolInvocationStatus::Succeeded,
        [
            ToolInvocationEvent::Intended { .. },
            ToolInvocationEvent::Failed {},
        ] => ToolInvocationStatus::Failed,
        _ => {
            return Err(ToolInvocationStoreError::InvalidHistory {
                event_count: events.len(),
            });
        }
    };
    Ok(Some(ToolInvocation {
        id: id.clone(),
        tool_id: tool_id.clone(),
        effect: *effect,
        status,
    }))
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ToolInvocationEvent {
    Intended { tool_id: ToolId, effect: ToolEffect },
    Denied {},
    Succeeded {},
    Failed {},
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntendedPayload {
    tool_id: String,
    effect: ToolEffect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPayload {}

impl Event for ToolInvocationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Intended { .. } => TOOL_INVOCATION_INTENDED_EVENT_TYPE,
            Self::Denied {} => TOOL_INVOCATION_DENIED_EVENT_TYPE,
            Self::Succeeded {} => TOOL_INVOCATION_SUCCEEDED_EVENT_TYPE,
            Self::Failed {} => TOOL_INVOCATION_FAILED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        TOOL_INVOCATION_EVENT_PAYLOAD_VERSION
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if payload_version != TOOL_INVOCATION_EVENT_PAYLOAD_VERSION {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }
        match event_type {
            TOOL_INVOCATION_INTENDED_EVENT_TYPE => {
                let decoded: IntendedPayload = decode_payload(payload)?;
                let tool_id = ToolId::new(decoded.tool_id).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Intended {
                    tool_id,
                    effect: decoded.effect,
                })
            }
            TOOL_INVOCATION_DENIED_EVENT_TYPE => {
                let _: EmptyPayload = decode_payload(payload)?;
                Ok(Self::Denied {})
            }
            TOOL_INVOCATION_SUCCEEDED_EVENT_TYPE => {
                let _: EmptyPayload = decode_payload(payload)?;
                Ok(Self::Succeeded {})
            }
            TOOL_INVOCATION_FAILED_EVENT_TYPE => {
                let _: EmptyPayload = decode_payload(payload)?;
                Ok(Self::Failed {})
            }
            _ => Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            }),
        }
    }
}

fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, DecodeError> {
    serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
        message: error.to_string(),
    })
}
