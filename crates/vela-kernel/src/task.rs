use std::{fmt, path::Path};

use serde::Serialize;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};
use crate::session::{SessionId, SessionStatus, SessionStore, SessionStoreError, session_stream};

const TASK_STARTED_EVENT_TYPE: &str = "task.started";
const TASK_COMPLETED_EVENT_TYPE: &str = "task.completed";
const TASK_CANCELLED_EVENT_TYPE: &str = "task.cancelled";
const TASK_FAILED_EVENT_TYPE: &str = "task.failed";
const TASK_SESSION_ASSOCIATED_EVENT_TYPE: &str = "task.session_associated";
const TASK_EVENT_PAYLOAD_VERSION: u32 = 1;
const TASK_COMPLETED_PAYLOAD_VERSION: u32 = 2;
const TASK_CANCELLED_PAYLOAD_VERSION: u32 = 2;
const TASK_FAILED_PAYLOAD_VERSION: u32 = 2;

/// An opaque, non-empty identifier for one task.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskIdError> {
        let value = value.into();
        if value.is_empty() {
            Err(TaskIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskIdError;

impl fmt::Display for TaskIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task id must not be empty")
    }
}

impl std::error::Error for TaskIdError {}

/// The non-empty objective recorded when a task starts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskGoal(String);

impl TaskGoal {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskGoalError> {
        let value = value.into();
        if value.is_empty() {
            Err(TaskGoalError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskGoalError;

impl fmt::Display for TaskGoalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task goal must not be empty")
    }
}

impl std::error::Error for TaskGoalError {}

/// The non-empty output recorded when a task completes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskOutput(String);

impl TaskOutput {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskOutputError> {
        let value = value.into();
        if value.is_empty() {
            Err(TaskOutputError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskOutputError;

impl fmt::Display for TaskOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task output must not be empty")
    }
}

impl std::error::Error for TaskOutputError {}

/// The non-empty reason recorded when a task is cancelled.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskCancellation(String);

impl TaskCancellation {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskCancellationError> {
        let value = value.into();
        if value.is_empty() {
            Err(TaskCancellationError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskCancellationError;

impl fmt::Display for TaskCancellationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task cancellation reason must not be empty")
    }
}

impl std::error::Error for TaskCancellationError {}

/// The non-empty diagnostic recorded when a task fails.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskFailure(String);

impl TaskFailure {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskFailureError> {
        let value = value.into();
        if value.is_empty() {
            Err(TaskFailureError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskFailureError;

impl fmt::Display for TaskFailureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task failure diagnostic must not be empty")
    }
}

impl std::error::Error for TaskFailureError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskStatus {
    Active,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    id: TaskId,
    goal: TaskGoal,
    status: TaskStatus,
    output: Option<TaskOutput>,
    cancellation: Option<TaskCancellation>,
    failure: Option<TaskFailure>,
    session_id: Option<SessionId>,
}

impl Task {
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    pub fn goal(&self) -> &TaskGoal {
        &self.goal
    }

    pub fn status(&self) -> TaskStatus {
        self.status
    }

    pub fn output(&self) -> Option<&TaskOutput> {
        self.output.as_ref()
    }

    pub fn cancellation(&self) -> Option<&TaskCancellation> {
        self.cancellation.as_ref()
    }

    pub fn failure(&self) -> Option<&TaskFailure> {
        self.failure.as_ref()
    }

    pub fn session_id(&self) -> Option<&SessionId> {
        self.session_id.as_ref()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TaskStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists {
        task_id: TaskId,
    },
    NotFound {
        task_id: TaskId,
    },
    AlreadyCompleted {
        task_id: TaskId,
    },
    AlreadyCancelled {
        task_id: TaskId,
    },
    AlreadyFailed {
        task_id: TaskId,
    },
    SessionNotFound {
        session_id: SessionId,
    },
    SessionClosed {
        session_id: SessionId,
    },
    AlreadyAssociated {
        task_id: TaskId,
        session_id: SessionId,
    },
    Session(SessionStoreError),
    InvalidHistory {
        event_count: usize,
    },
}

impl fmt::Display for TaskStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "task event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "task replay error: {error}"),
            Self::AlreadyExists { task_id } => write!(formatter, "task {task_id} already exists"),
            Self::NotFound { task_id } => write!(formatter, "task {task_id} was not found"),
            Self::AlreadyCompleted { task_id } => {
                write!(formatter, "task {task_id} is already completed")
            }
            Self::AlreadyCancelled { task_id } => {
                write!(formatter, "task {task_id} is already cancelled")
            }
            Self::AlreadyFailed { task_id } => {
                write!(formatter, "task {task_id} has already failed")
            }
            Self::SessionNotFound { session_id } => {
                write!(formatter, "session {session_id} was not found")
            }
            Self::SessionClosed { session_id } => {
                write!(formatter, "session {session_id} is closed")
            }
            Self::AlreadyAssociated {
                task_id,
                session_id,
            } => write!(
                formatter,
                "task {task_id} is already associated with session {session_id}"
            ),
            Self::Session(error) => write!(formatter, "task session-store error: {error}"),
            Self::InvalidHistory { event_count } => {
                write!(
                    formatter,
                    "invalid task history with {event_count} lifecycle events"
                )
            }
        }
    }
}

impl std::error::Error for TaskStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::AlreadyCompleted { .. }
            | Self::AlreadyCancelled { .. }
            | Self::AlreadyFailed { .. }
            | Self::SessionNotFound { .. }
            | Self::SessionClosed { .. }
            | Self::AlreadyAssociated { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous task lifecycle store backed by the typed event log.
pub struct TaskStore {
    event_log: EventLog,
    sessions: SessionStore,
}

impl TaskStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        let event_log = EventLog::open(path).map_err(TaskStoreError::EventLog)?;
        let sessions = SessionStore::open(path).map_err(TaskStoreError::Session)?;
        Ok(Self {
            event_log,
            sessions,
        })
    }

    pub fn start(&mut self, id: TaskId, goal: TaskGoal) -> Result<Task, TaskStoreError> {
        let stream = task_stream(&id);
        let event = TaskEvent::Started { goal: goal.clone() };
        match self
            .event_log
            .append(&stream, ExpectedVersion::NoStream, &event)
        {
            Ok(_) => Ok(Task {
                id,
                goal,
                status: TaskStatus::Active,
                output: None,
                cancellation: None,
                failure: None,
                session_id: None,
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(TaskStoreError::AlreadyExists { task_id: id }),
            Err(error) => Err(TaskStoreError::EventLog(error)),
        }
    }

    pub fn complete(&mut self, id: &TaskId, output: TaskOutput) -> Result<Task, TaskStoreError> {
        self.transition_to_terminal(
            id,
            TaskStatus::Completed,
            TaskEvent::Completed {
                output: Some(output.clone()),
            },
            Some(output),
            None,
            None,
        )
    }

    pub fn cancel(
        &mut self,
        id: &TaskId,
        cancellation: TaskCancellation,
    ) -> Result<Task, TaskStoreError> {
        self.transition_to_terminal(
            id,
            TaskStatus::Cancelled,
            TaskEvent::Cancelled {
                reason: Some(cancellation.clone()),
            },
            None,
            Some(cancellation),
            None,
        )
    }

    pub fn fail(&mut self, id: &TaskId, failure: TaskFailure) -> Result<Task, TaskStoreError> {
        self.transition_to_terminal(
            id,
            TaskStatus::Failed,
            TaskEvent::Failed {
                failure: Some(failure.clone()),
            },
            None,
            None,
            Some(failure),
        )
    }

    pub fn associate_session(
        &mut self,
        id: &TaskId,
        session_id: &SessionId,
    ) -> Result<Task, TaskStoreError> {
        loop {
            let Some(loaded) = self.load_versioned(id)? else {
                return Err(TaskStoreError::NotFound {
                    task_id: id.clone(),
                });
            };
            if let Some(existing) = loaded.task.session_id() {
                return Err(TaskStoreError::AlreadyAssociated {
                    task_id: id.clone(),
                    session_id: existing.clone(),
                });
            }

            let (session, session_version) = self
                .sessions
                .load_with_version(session_id)
                .map_err(TaskStoreError::Session)?
                .ok_or_else(|| TaskStoreError::SessionNotFound {
                    session_id: session_id.clone(),
                })?;
            if session.status() == SessionStatus::Closed {
                return Err(TaskStoreError::SessionClosed {
                    session_id: session_id.clone(),
                });
            }

            let event = TaskEvent::SessionAssociated {
                task_id: id.clone(),
                session_id: session_id.clone(),
            };
            match self.event_log.append_if_stream_unchanged(
                &task_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                &session_stream(session_id),
                ExpectedVersion::Exact(session_version),
                &event,
            ) {
                Ok(_) => {
                    return Ok(Task {
                        session_id: Some(session_id.clone()),
                        ..loaded.task
                    });
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(TaskStoreError::EventLog(error)),
            }
        }
    }

    fn transition_to_terminal(
        &mut self,
        id: &TaskId,
        status: TaskStatus,
        event: TaskEvent,
        output: Option<TaskOutput>,
        cancellation: Option<TaskCancellation>,
        failure: Option<TaskFailure>,
    ) -> Result<Task, TaskStoreError> {
        loop {
            let loaded = match self.load_versioned(id)? {
                Some(loaded) if loaded.task.status == TaskStatus::Active => loaded,
                Some(loaded) => return Err(terminal_state_error(id, loaded.task.status)),
                None => {
                    return Err(TaskStoreError::NotFound {
                        task_id: id.clone(),
                    });
                }
            };

            match self.event_log.append(
                &task_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                &event,
            ) {
                Ok(_) => {
                    return Ok(Task {
                        status,
                        output,
                        cancellation,
                        failure,
                        ..loaded.task
                    });
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(TaskStoreError::EventLog(error)),
            }
        }
    }

    pub fn load(&self, id: &TaskId) -> Result<Option<Task>, TaskStoreError> {
        self.load_versioned(id)
            .map(|loaded| loaded.map(|loaded| loaded.task))
    }

    fn load_versioned(&self, id: &TaskId) -> Result<Option<VersionedTask>, TaskStoreError> {
        let events = self
            .event_log
            .replay::<TaskEvent>(&task_stream(id))
            .map_err(TaskStoreError::Replay)?;
        let Some(TaskEvent::Started { goal }) = events.first() else {
            return if events.is_empty() {
                Ok(None)
            } else {
                Err(TaskStoreError::InvalidHistory {
                    event_count: events.len(),
                })
            };
        };

        let mut task = Task {
            id: id.clone(),
            goal: goal.clone(),
            status: TaskStatus::Active,
            output: None,
            cancellation: None,
            failure: None,
            session_id: None,
        };
        for event in &events[1..] {
            match event {
                TaskEvent::SessionAssociated {
                    task_id,
                    session_id,
                } if task.session_id.is_none() && task_id == id => {
                    task.session_id = Some(session_id.clone());
                }
                TaskEvent::Completed { output } if task.status == TaskStatus::Active => {
                    task.status = TaskStatus::Completed;
                    task.output = output.clone();
                }
                TaskEvent::Cancelled { reason } if task.status == TaskStatus::Active => {
                    task.status = TaskStatus::Cancelled;
                    task.cancellation = reason.clone();
                }
                TaskEvent::Failed { failure } if task.status == TaskStatus::Active => {
                    task.status = TaskStatus::Failed;
                    task.failure = failure.clone();
                }
                _ => {
                    return Err(TaskStoreError::InvalidHistory {
                        event_count: events.len(),
                    });
                }
            }
        }

        Ok(Some(VersionedTask {
            task,
            stream_version: u64::try_from(events.len()).map_err(|_| {
                TaskStoreError::InvalidHistory {
                    event_count: events.len(),
                }
            })?,
        }))
    }
}

struct VersionedTask {
    task: Task,
    stream_version: u64,
}

fn terminal_state_error(id: &TaskId, status: TaskStatus) -> TaskStoreError {
    match status {
        TaskStatus::Completed => TaskStoreError::AlreadyCompleted {
            task_id: id.clone(),
        },
        TaskStatus::Cancelled => TaskStoreError::AlreadyCancelled {
            task_id: id.clone(),
        },
        TaskStatus::Failed => TaskStoreError::AlreadyFailed {
            task_id: id.clone(),
        },
        TaskStatus::Active => TaskStoreError::InvalidHistory { event_count: 1 },
    }
}

fn task_stream(id: &TaskId) -> StreamId {
    StreamId::new(format!("task:{id}")).expect("a prefixed task stream is never empty")
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum TaskEvent {
    Started {
        goal: TaskGoal,
    },
    Completed {
        output: Option<TaskOutput>,
    },
    Cancelled {
        reason: Option<TaskCancellation>,
    },
    Failed {
        failure: Option<TaskFailure>,
    },
    SessionAssociated {
        task_id: TaskId,
        session_id: SessionId,
    },
}

impl Event for TaskEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Started { .. } => TASK_STARTED_EVENT_TYPE,
            Self::Completed { .. } => TASK_COMPLETED_EVENT_TYPE,
            Self::Cancelled { .. } => TASK_CANCELLED_EVENT_TYPE,
            Self::Failed { .. } => TASK_FAILED_EVENT_TYPE,
            Self::SessionAssociated { .. } => TASK_SESSION_ASSOCIATED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        match self {
            Self::Completed { .. } => TASK_COMPLETED_PAYLOAD_VERSION,
            Self::Cancelled { .. } => TASK_CANCELLED_PAYLOAD_VERSION,
            Self::Failed { .. } => TASK_FAILED_PAYLOAD_VERSION,
            Self::Started { .. } | Self::SessionAssociated { .. } => TASK_EVENT_PAYLOAD_VERSION,
        }
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        let supported = match event_type {
            TASK_STARTED_EVENT_TYPE => payload_version == TASK_EVENT_PAYLOAD_VERSION,
            TASK_COMPLETED_EVENT_TYPE => matches!(
                payload_version,
                TASK_EVENT_PAYLOAD_VERSION | TASK_COMPLETED_PAYLOAD_VERSION
            ),
            TASK_CANCELLED_EVENT_TYPE => matches!(
                payload_version,
                TASK_EVENT_PAYLOAD_VERSION | TASK_CANCELLED_PAYLOAD_VERSION
            ),
            TASK_FAILED_EVENT_TYPE => matches!(
                payload_version,
                TASK_EVENT_PAYLOAD_VERSION | TASK_FAILED_PAYLOAD_VERSION
            ),
            TASK_SESSION_ASSOCIATED_EVENT_TYPE => payload_version == TASK_EVENT_PAYLOAD_VERSION,
            _ => false,
        };
        if !supported {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }

        if (event_type == TASK_CANCELLED_EVENT_TYPE || event_type == TASK_COMPLETED_EVENT_TYPE)
            && payload_version == TASK_EVENT_PAYLOAD_VERSION
        {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {}

            serde_json::from_slice::<Payload>(payload).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(match event_type {
                TASK_COMPLETED_EVENT_TYPE => Self::Completed { output: None },
                TASK_CANCELLED_EVENT_TYPE => Self::Cancelled { reason: None },
                _ => unreachable!("empty terminal event types were validated above"),
            });
        }

        if event_type == TASK_COMPLETED_EVENT_TYPE {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {
                output: String,
            }

            let payload: Payload =
                serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let output =
                TaskOutput::new(payload.output).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            return Ok(Self::Completed {
                output: Some(output),
            });
        }

        if event_type == TASK_CANCELLED_EVENT_TYPE {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {
                reason: String,
            }

            let payload: Payload =
                serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let reason = TaskCancellation::new(payload.reason).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(Self::Cancelled {
                reason: Some(reason),
            });
        }

        if event_type == TASK_FAILED_EVENT_TYPE && payload_version == TASK_EVENT_PAYLOAD_VERSION {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {}

            serde_json::from_slice::<Payload>(payload).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(Self::Failed { failure: None });
        }

        if event_type == TASK_FAILED_EVENT_TYPE {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {
                failure: String,
            }

            let payload: Payload =
                serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let failure = TaskFailure::new(payload.failure).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(Self::Failed {
                failure: Some(failure),
            });
        }

        if event_type == TASK_SESSION_ASSOCIATED_EVENT_TYPE {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {
                task_id: String,
                session_id: String,
            }

            let payload: Payload =
                serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let task_id =
                TaskId::new(payload.task_id).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let session_id = SessionId::new(payload.session_id).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(Self::SessionAssociated {
                task_id,
                session_id,
            });
        }

        #[derive(serde::Deserialize)]
        struct Payload {
            goal: String,
        }

        let payload: Payload =
            serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                message: error.to_string(),
            })?;
        let goal = TaskGoal::new(payload.goal).map_err(|error| DecodeError::MalformedPayload {
            message: error.to_string(),
        })?;
        Ok(Self::Started { goal })
    }
}
