use std::{fmt, path::Path};

use serde::Serialize;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};

const TASK_STARTED_EVENT_TYPE: &str = "task.started";
const TASK_EVENT_PAYLOAD_VERSION: u32 = 1;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskStatus {
    Active,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    id: TaskId,
    goal: TaskGoal,
}

impl Task {
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    pub fn goal(&self) -> &TaskGoal {
        &self.goal
    }

    pub fn status(&self) -> TaskStatus {
        TaskStatus::Active
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TaskStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists { task_id: TaskId },
    InvalidHistory { event_count: usize },
}

impl fmt::Display for TaskStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "task event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "task replay error: {error}"),
            Self::AlreadyExists { task_id } => write!(formatter, "task {task_id} already exists"),
            Self::InvalidHistory { event_count } => {
                write!(
                    formatter,
                    "invalid task history with {event_count} start events"
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
            Self::AlreadyExists { .. } | Self::InvalidHistory { .. } => None,
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
        let event = TaskEvent { goal: goal.clone() };
        match self
            .event_log
            .append(&stream, ExpectedVersion::NoStream, &event)
        {
            Ok(_) => Ok(Task { id, goal }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(TaskStoreError::AlreadyExists { task_id: id }),
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
            [event] => Ok(Some(Task {
                id: id.clone(),
                goal: event.goal.clone(),
            })),
            _ => Err(TaskStoreError::InvalidHistory {
                event_count: events.len(),
            }),
        }
    }
}

fn task_stream(id: &TaskId) -> StreamId {
    StreamId::new(format!("task:{id}")).expect("a prefixed task stream is never empty")
}

#[derive(Debug, Serialize)]
struct TaskEvent {
    goal: TaskGoal,
}

impl Event for TaskEvent {
    fn event_type(&self) -> &'static str {
        TASK_STARTED_EVENT_TYPE
    }

    fn payload_version(&self) -> u32 {
        TASK_EVENT_PAYLOAD_VERSION
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if event_type != TASK_STARTED_EVENT_TYPE || payload_version != TASK_EVENT_PAYLOAD_VERSION {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
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
        Ok(Self { goal })
    }
}
