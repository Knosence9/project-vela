use std::{collections::BTreeMap, error::Error, fmt, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};
use crate::task::{TaskId, TaskStatus, TaskStore, TaskStoreError, task_stream};

const TOOL_INVOCATION_INTENDED_EVENT_TYPE: &str = "tool.invocation_intended";
const TOOL_INVOCATION_DENIED_EVENT_TYPE: &str = "tool.invocation_denied";
const TOOL_INVOCATION_SUCCEEDED_EVENT_TYPE: &str = "tool.invocation_succeeded";
const TOOL_INVOCATION_FAILED_EVENT_TYPE: &str = "tool.invocation_failed";
const TOOL_INVOCATION_EVENT_PAYLOAD_VERSION: u32 = 1;
const TOOL_INVOCATION_ASSOCIATED_PAYLOAD_VERSION: u32 = 2;
const TOOL_INVOCATION_STREAM_PREFIX: &str = "tool-invocation:";

/// An opaque, non-blank stable identifier for one tool adapter.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
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

/// Stable permission metadata for one registered adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolMetadata {
    id: ToolId,
    effect: ToolEffect,
}

impl ToolMetadata {
    pub fn id(&self) -> &ToolId {
        &self.id
    }

    pub fn effect(&self) -> ToolEffect {
        self.effect
    }
}

/// A duplicate adapter identity rejected during registration.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ToolRegistryError {
    DuplicateId { tool_id: ToolId },
}

impl fmt::Display for ToolRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { tool_id } => {
                write!(formatter, "tool {tool_id} is already registered")
            }
        }
    }
}

impl Error for ToolRegistryError {}

/// An unknown adapter identity or existing invocation-protocol failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolRegistryInvocationError {
    NotFound { tool_id: ToolId },
    Invocation(ToolInvocationError),
}

impl fmt::Display for ToolRegistryInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { tool_id } => write!(formatter, "tool {tool_id} is not registered"),
            Self::Invocation(error) => error.fmt(formatter),
        }
    }
}

impl Error for ToolRegistryInvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NotFound { .. } => None,
            Self::Invocation(error) => Some(error),
        }
    }
}

/// An in-memory, process-local owner and deterministic directory of tool adapters.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<ToolId, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one adapter without replacing an adapter that owns the same ID.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) -> Result<(), ToolRegistryError> {
        let tool_id = tool.id().clone();
        if self.tools.contains_key(&tool_id) {
            return Err(ToolRegistryError::DuplicateId { tool_id });
        }
        self.tools.insert(tool_id, Box::new(tool));
        Ok(())
    }

    /// Returns metadata in ascending stable-ID order.
    pub fn metadata(&self) -> Vec<ToolMetadata> {
        self.tools
            .iter()
            .map(|(id, tool)| ToolMetadata {
                id: id.clone(),
                effect: tool.effect(),
            })
            .collect()
    }

    /// Resolves one adapter, then delegates to the existing per-invocation permission protocol.
    pub fn invoke<A: ToolAuthorizer>(
        &mut self,
        tool_id: &ToolId,
        authorizer: &mut A,
        input: &Value,
    ) -> Result<Value, ToolRegistryInvocationError> {
        let tool =
            self.tools
                .get_mut(tool_id)
                .ok_or_else(|| ToolRegistryInvocationError::NotFound {
                    tool_id: tool_id.clone(),
                })?;
        invoke_tool(tool.as_mut(), authorizer, input)
            .map_err(ToolRegistryInvocationError::Invocation)
    }
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
pub fn invoke_tool<T: Tool + ?Sized, A: ToolAuthorizer>(
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
    task_id: Option<TaskId>,
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

    /// The task fixed in this invocation's intent, when it was task-associated.
    pub fn task_id(&self) -> Option<&TaskId> {
        self.task_id.as_ref()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ToolInvocationStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    Task(TaskStoreError),
    AlreadyExists { invocation_id: ToolInvocationId },
    TaskNotFound { task_id: TaskId },
    TaskNotActive { task_id: TaskId, status: TaskStatus },
    TaskChanged { task_id: TaskId },
    InvalidStreamId { stream_id: String },
    InvalidHistory { event_count: usize },
}

impl fmt::Display for ToolInvocationStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "tool invocation event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "tool invocation replay error: {error}"),
            Self::Task(error) => write!(formatter, "tool invocation task-store error: {error}"),
            Self::AlreadyExists { invocation_id } => {
                write!(formatter, "tool invocation {invocation_id} already exists")
            }
            Self::TaskNotFound { task_id } => write!(formatter, "task {task_id} was not found"),
            Self::TaskNotActive { task_id, status } => {
                write!(formatter, "task {task_id} is not active: {status:?}")
            }
            Self::TaskChanged { task_id } => {
                write!(formatter, "task {task_id} changed before invocation intent")
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
            Self::Task(error) => Some(error),
            Self::AlreadyExists { .. }
            | Self::TaskNotFound { .. }
            | Self::TaskNotActive { .. }
            | Self::TaskChanged { .. }
            | Self::InvalidStreamId { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous metadata-only invocation store backed by the typed event log.
pub struct ToolInvocationStore {
    event_log: EventLog,
    tasks: TaskStore,
}

impl ToolInvocationStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ToolInvocationStoreError> {
        let path = path.as_ref();
        let event_log = EventLog::open(path).map_err(ToolInvocationStoreError::EventLog)?;
        let tasks = TaskStore::open(path).map_err(ToolInvocationStoreError::Task)?;
        Ok(Self { event_log, tasks })
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

    fn record_task_intent(
        &mut self,
        task_id: &TaskId,
        id: &ToolInvocationId,
        tool_id: ToolId,
        effect: ToolEffect,
    ) -> Result<(), ToolInvocationStoreError> {
        let Some((task, task_version)) = self
            .tasks
            .load_with_version(task_id)
            .map_err(ToolInvocationStoreError::Task)?
        else {
            return Err(ToolInvocationStoreError::TaskNotFound {
                task_id: task_id.clone(),
            });
        };
        if task.status() != TaskStatus::Active {
            return Err(ToolInvocationStoreError::TaskNotActive {
                task_id: task_id.clone(),
                status: task.status(),
            });
        }

        self.record_task_intent_at_version(task_id, task_version, id, tool_id, effect)
    }

    fn record_task_intent_at_version(
        &mut self,
        task_id: &TaskId,
        task_version: u64,
        id: &ToolInvocationId,
        tool_id: ToolId,
        effect: ToolEffect,
    ) -> Result<(), ToolInvocationStoreError> {
        let event = ToolInvocationEvent::TaskIntended {
            tool_id,
            effect,
            task_id: task_id.clone(),
        };
        match self.event_log.append_if_stream_unchanged(
            &tool_invocation_stream(id),
            ExpectedVersion::NoStream,
            &task_stream(task_id),
            ExpectedVersion::Exact(task_version),
            &event,
        ) {
            Ok(_) => Ok(()),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(ToolInvocationStoreError::AlreadyExists {
                invocation_id: id.clone(),
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::Exact(_),
                ..
            }) => Err(ToolInvocationStoreError::TaskChanged {
                task_id: task_id.clone(),
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
    finish_durable_invocation(store, &invocation_id, tool, authorizer, input)
}

/// Persists task-associated intent, then invokes once using the existing permission protocol.
///
/// The task must exist and remain active until intent commits. Task validation and intent
/// persistence failures occur before authorization or adapter execution and are never retried.
pub fn invoke_tool_for_task_durable<T: Tool, A: ToolAuthorizer>(
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    tool: &mut T,
    authorizer: &mut A,
    input: &Value,
) -> Result<Value, DurableToolInvocationError> {
    store
        .record_task_intent(task_id, &invocation_id, tool.id().clone(), tool.effect())
        .map_err(DurableToolInvocationError::Store)?;
    finish_durable_invocation(store, &invocation_id, tool, authorizer, input)
}

fn finish_durable_invocation<T: Tool, A: ToolAuthorizer>(
    store: &mut ToolInvocationStore,
    invocation_id: &ToolInvocationId,
    tool: &mut T,
    authorizer: &mut A,
    input: &Value,
) -> Result<Value, DurableToolInvocationError> {
    let result = invoke_tool(tool, authorizer, input);
    let status = match &result {
        Ok(_) => ToolInvocationStatus::Succeeded,
        Err(ToolInvocationError::Denied { .. }) => ToolInvocationStatus::Denied,
        Err(ToolInvocationError::Tool { .. }) => ToolInvocationStatus::Failed,
    };
    if let Err(error) = store.record_outcome(invocation_id, status) {
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
    let Some(first) = events.first() else {
        return Ok(None);
    };
    let Some((tool_id, effect, task_id)) = first.intent_metadata() else {
        return Err(ToolInvocationStoreError::InvalidHistory {
            event_count: events.len(),
        });
    };
    let status = match events.as_slice() {
        [intent] if intent.intent_metadata().is_some() => ToolInvocationStatus::Pending,
        [intent, ToolInvocationEvent::Denied {}] if intent.intent_metadata().is_some() => {
            ToolInvocationStatus::Denied
        }
        [intent, ToolInvocationEvent::Succeeded {}] if intent.intent_metadata().is_some() => {
            ToolInvocationStatus::Succeeded
        }
        [intent, ToolInvocationEvent::Failed {}] if intent.intent_metadata().is_some() => {
            ToolInvocationStatus::Failed
        }
        _ => {
            return Err(ToolInvocationStoreError::InvalidHistory {
                event_count: events.len(),
            });
        }
    };
    Ok(Some(ToolInvocation {
        id: id.clone(),
        tool_id: tool_id.clone(),
        effect,
        status,
        task_id: task_id.cloned(),
    }))
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ToolInvocationEvent {
    Intended {
        tool_id: ToolId,
        effect: ToolEffect,
    },
    TaskIntended {
        tool_id: ToolId,
        effect: ToolEffect,
        task_id: TaskId,
    },
    Denied {},
    Succeeded {},
    Failed {},
}

impl ToolInvocationEvent {
    fn intent_metadata(&self) -> Option<(&ToolId, ToolEffect, Option<&TaskId>)> {
        match self {
            Self::Intended { tool_id, effect } => Some((tool_id, *effect, None)),
            Self::TaskIntended {
                tool_id,
                effect,
                task_id,
            } => Some((tool_id, *effect, Some(task_id))),
            Self::Denied {} | Self::Succeeded {} | Self::Failed {} => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntendedPayload {
    tool_id: String,
    effect: ToolEffect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskIntendedPayload {
    tool_id: String,
    effect: ToolEffect,
    task_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPayload {}

impl Event for ToolInvocationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Intended { .. } | Self::TaskIntended { .. } => {
                TOOL_INVOCATION_INTENDED_EVENT_TYPE
            }
            Self::Denied {} => TOOL_INVOCATION_DENIED_EVENT_TYPE,
            Self::Succeeded {} => TOOL_INVOCATION_SUCCEEDED_EVENT_TYPE,
            Self::Failed {} => TOOL_INVOCATION_FAILED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        match self {
            Self::TaskIntended { .. } => TOOL_INVOCATION_ASSOCIATED_PAYLOAD_VERSION,
            Self::Intended { .. } | Self::Denied {} | Self::Succeeded {} | Self::Failed {} => {
                TOOL_INVOCATION_EVENT_PAYLOAD_VERSION
            }
        }
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if event_type == TOOL_INVOCATION_INTENDED_EVENT_TYPE {
            return match payload_version {
                TOOL_INVOCATION_EVENT_PAYLOAD_VERSION => {
                    let decoded: IntendedPayload = decode_payload(payload)?;
                    let tool_id = decode_tool_id(decoded.tool_id)?;
                    Ok(Self::Intended {
                        tool_id,
                        effect: decoded.effect,
                    })
                }
                TOOL_INVOCATION_ASSOCIATED_PAYLOAD_VERSION => {
                    let decoded: TaskIntendedPayload = decode_payload(payload)?;
                    let tool_id = decode_tool_id(decoded.tool_id)?;
                    let task_id = TaskId::new(decoded.task_id).map_err(|error| {
                        DecodeError::MalformedPayload {
                            message: error.to_string(),
                        }
                    })?;
                    Ok(Self::TaskIntended {
                        tool_id,
                        effect: decoded.effect,
                        task_id,
                    })
                }
                _ => Err(DecodeError::UnsupportedEvent {
                    event_type: event_type.to_owned(),
                    payload_version,
                }),
            };
        }
        if payload_version != TOOL_INVOCATION_EVENT_PAYLOAD_VERSION {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }
        match event_type {
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

fn decode_tool_id(value: String) -> Result<ToolId, DecodeError> {
    ToolId::new(value).map_err(|error| DecodeError::MalformedPayload {
        message: error.to_string(),
    })
}

fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, DecodeError> {
    serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::task::{TaskGoal, TaskOutput};

    #[test]
    fn task_change_after_validation_rejects_intent_without_retry() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        let task_id = TaskId::new("task-1").unwrap();
        let mut tasks = TaskStore::open(&path).unwrap();
        tasks
            .start(task_id.clone(), TaskGoal::new("exercise tool").unwrap())
            .unwrap();
        let mut store = ToolInvocationStore::open(&path).unwrap();

        tasks
            .complete(&task_id, TaskOutput::new("done").unwrap())
            .unwrap();
        let invocation_id = ToolInvocationId::new("raced").unwrap();
        let error = store
            .record_task_intent_at_version(
                &task_id,
                1,
                &invocation_id,
                ToolId::new("test.echo").unwrap(),
                ToolEffect::Pure,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            ToolInvocationStoreError::TaskChanged { task_id: changed } if changed == task_id
        ));
        assert!(store.load(&invocation_id).unwrap().is_none());
    }
}
