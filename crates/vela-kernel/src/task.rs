use std::{fmt, path::Path};

use serde::Serialize;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};

const TASK_STARTED_EVENT_TYPE: &str = "task.started";
const TASK_COMPLETED_EVENT_TYPE: &str = "task.completed";
const TASK_CANCELLED_EVENT_TYPE: &str = "task.cancelled";
const TASK_FAILED_EVENT_TYPE: &str = "task.failed";
const TASK_EVENT_PAYLOAD_VERSION: u32 = 1;
const TASK_FAILED_PAYLOAD_VERSION: u32 = 2;

/// An opaque, non-empty identifier for one task.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
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
    failure: Option<TaskFailure>,
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

    pub fn failure(&self) -> Option<&TaskFailure> {
        self.failure.as_ref()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TaskStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists { task_id: TaskId },
    NotFound { task_id: TaskId },
    AlreadyCompleted { task_id: TaskId },
    AlreadyCancelled { task_id: TaskId },
    AlreadyFailed { task_id: TaskId },
    InvalidHistory { event_count: usize },
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
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::AlreadyCompleted { .. }
            | Self::AlreadyCancelled { .. }
            | Self::AlreadyFailed { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous task lifecycle store backed by the typed event log.
pub struct TaskStore {
    event_log: EventLog,
}

impl TaskStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        EventLog::open(path)
            .map(|event_log| Self { event_log })
            .map_err(TaskStoreError::EventLog)
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
                failure: None,
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(TaskStoreError::AlreadyExists { task_id: id }),
            Err(error) => Err(TaskStoreError::EventLog(error)),
        }
    }

    pub fn complete(&mut self, id: &TaskId) -> Result<Task, TaskStoreError> {
        self.transition_to_terminal(
            id,
            TaskStatus::Completed,
            TaskEvent::Completed(EmptyPayload {}),
            None,
        )
    }

    pub fn cancel(&mut self, id: &TaskId) -> Result<Task, TaskStoreError> {
        self.transition_to_terminal(
            id,
            TaskStatus::Cancelled,
            TaskEvent::Cancelled(EmptyPayload {}),
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
            Some(failure),
        )
    }

    fn transition_to_terminal(
        &mut self,
        id: &TaskId,
        status: TaskStatus,
        event: TaskEvent,
        failure: Option<TaskFailure>,
    ) -> Result<Task, TaskStoreError> {
        let task = match self.load(id)? {
            Some(task) if task.status == TaskStatus::Active => task,
            Some(task) => return Err(terminal_state_error(id, task.status)),
            None => {
                return Err(TaskStoreError::NotFound {
                    task_id: id.clone(),
                });
            }
        };

        match self
            .event_log
            .append(&task_stream(id), ExpectedVersion::Exact(1), &event)
        {
            Ok(_) => Ok(Task {
                status,
                failure,
                ..task
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::Exact(1),
                current: Some(2),
            }) => {
                let winner = self.load(id)?.ok_or_else(|| TaskStoreError::NotFound {
                    task_id: id.clone(),
                })?;
                Err(terminal_state_error(id, winner.status))
            }
            Err(error) => Err(TaskStoreError::EventLog(error)),
        }
    }

    pub fn load(&self, id: &TaskId) -> Result<Option<Task>, TaskStoreError> {
        let events = self
            .event_log
            .replay::<TaskEvent>(&task_stream(id))
            .map_err(TaskStoreError::Replay)?;
        match events.as_slice() {
            [] => Ok(None),
            [TaskEvent::Started { goal }] => Ok(Some(Task {
                id: id.clone(),
                goal: goal.clone(),
                status: TaskStatus::Active,
                failure: None,
            })),
            [TaskEvent::Started { goal }, TaskEvent::Completed(_)] => Ok(Some(Task {
                id: id.clone(),
                goal: goal.clone(),
                status: TaskStatus::Completed,
                failure: None,
            })),
            [TaskEvent::Started { goal }, TaskEvent::Cancelled(_)] => Ok(Some(Task {
                id: id.clone(),
                goal: goal.clone(),
                status: TaskStatus::Cancelled,
                failure: None,
            })),
            [TaskEvent::Started { goal }, TaskEvent::Failed { failure }] => Ok(Some(Task {
                id: id.clone(),
                goal: goal.clone(),
                status: TaskStatus::Failed,
                failure: failure.clone(),
            })),
            _ => Err(TaskStoreError::InvalidHistory {
                event_count: events.len(),
            }),
        }
    }
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
    Started { goal: TaskGoal },
    Completed(EmptyPayload),
    Cancelled(EmptyPayload),
    Failed { failure: Option<TaskFailure> },
}

#[derive(Debug, Serialize)]
struct EmptyPayload {}

impl Event for TaskEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Started { .. } => TASK_STARTED_EVENT_TYPE,
            Self::Completed(_) => TASK_COMPLETED_EVENT_TYPE,
            Self::Cancelled(_) => TASK_CANCELLED_EVENT_TYPE,
            Self::Failed { .. } => TASK_FAILED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        match self {
            Self::Failed { .. } => TASK_FAILED_PAYLOAD_VERSION,
            _ => TASK_EVENT_PAYLOAD_VERSION,
        }
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        let supported = match event_type {
            TASK_STARTED_EVENT_TYPE | TASK_COMPLETED_EVENT_TYPE | TASK_CANCELLED_EVENT_TYPE => {
                payload_version == TASK_EVENT_PAYLOAD_VERSION
            }
            TASK_FAILED_EVENT_TYPE => matches!(
                payload_version,
                TASK_EVENT_PAYLOAD_VERSION | TASK_FAILED_PAYLOAD_VERSION
            ),
            _ => false,
        };
        if !supported {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }

        if matches!(
            event_type,
            TASK_COMPLETED_EVENT_TYPE | TASK_CANCELLED_EVENT_TYPE
        ) {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {}

            serde_json::from_slice::<Payload>(payload).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(match event_type {
                TASK_COMPLETED_EVENT_TYPE => Self::Completed(EmptyPayload {}),
                TASK_CANCELLED_EVENT_TYPE => Self::Cancelled(EmptyPayload {}),
                _ => unreachable!("empty terminal event types were validated above"),
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
