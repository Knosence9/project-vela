use std::{error::Error, fmt, path::Path};

use serde::Serialize;

use crate::{
    event_log::{
        DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
    },
    task::{TaskGoal, TaskGoalError},
};

const SCHEDULE_CREATED_EVENT_TYPE: &str = "schedule.created";
const SCHEDULE_CANCELLED_EVENT_TYPE: &str = "schedule.cancelled";
const SCHEDULE_EVENT_PAYLOAD_VERSION: u32 = 1;
const SCHEDULE_STREAM_PREFIX: &str = "schedule:";

/// An opaque, non-blank identity for one durable schedule intent.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScheduleId(String);

impl ScheduleId {
    pub fn new(value: impl Into<String>) -> Result<Self, ScheduleIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(ScheduleIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScheduleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleIdError;

impl fmt::Display for ScheduleIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("schedule id must not be blank")
    }
}

impl Error for ScheduleIdError {}

/// A caller-supplied, non-blank reason for withdrawing pending schedule intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ScheduleCancellation(String);

impl ScheduleCancellation {
    pub fn new(value: impl Into<String>) -> Result<Self, ScheduleCancellationError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(ScheduleCancellationError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleCancellationError;

impl fmt::Display for ScheduleCancellationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("schedule cancellation reason must not be blank")
    }
}

impl Error for ScheduleCancellationError {}

/// A deterministic caller-owned instant in non-negative Unix milliseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScheduleInstant(u64);

impl ScheduleInstant {
    pub const fn from_unix_millis(unix_millis: u64) -> Self {
        Self(unix_millis)
    }

    pub const fn unix_millis(self) -> u64 {
        self.0
    }
}

/// One inert durable intent to create a task no earlier than a caller-owned instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledTask {
    id: ScheduleId,
    goal: TaskGoal,
    due_at: ScheduleInstant,
    cancellation: Option<ScheduleCancellation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleStatus {
    Pending,
    Cancelled,
}

impl ScheduledTask {
    pub fn id(&self) -> &ScheduleId {
        &self.id
    }

    pub fn goal(&self) -> &TaskGoal {
        &self.goal
    }

    pub fn due_at(&self) -> ScheduleInstant {
        self.due_at
    }

    pub fn status(&self) -> ScheduleStatus {
        if self.cancellation.is_some() {
            ScheduleStatus::Cancelled
        } else {
            ScheduleStatus::Pending
        }
    }

    pub fn cancellation(&self) -> Option<&ScheduleCancellation> {
        self.cancellation.as_ref()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ScheduleStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists { schedule_id: ScheduleId },
    NotFound { schedule_id: ScheduleId },
    AlreadyCancelled { schedule_id: ScheduleId },
    InvalidStreamId { stream_id: String },
    InvalidHistory { event_count: usize },
}

impl fmt::Display for ScheduleStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "schedule event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "schedule replay error: {error}"),
            Self::AlreadyExists { schedule_id } => {
                write!(formatter, "schedule {schedule_id} already exists")
            }
            Self::NotFound { schedule_id } => write!(formatter, "schedule {schedule_id} not found"),
            Self::AlreadyCancelled { schedule_id } => {
                write!(formatter, "schedule {schedule_id} is already cancelled")
            }
            Self::InvalidStreamId { stream_id } => {
                write!(formatter, "invalid schedule stream id {stream_id:?}")
            }
            Self::InvalidHistory { event_count } => {
                write!(
                    formatter,
                    "invalid schedule history with {event_count} events"
                )
            }
        }
    }
}

impl Error for ScheduleStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::AlreadyCancelled { .. }
            | Self::InvalidStreamId { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous durable store for inert one-shot task schedule intents.
pub struct ScheduleStore {
    event_log: EventLog,
}

impl ScheduleStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ScheduleStoreError> {
        EventLog::open(path)
            .map(|event_log| Self { event_log })
            .map_err(ScheduleStoreError::EventLog)
    }

    pub fn schedule(
        &mut self,
        id: ScheduleId,
        goal: TaskGoal,
        due_at: ScheduleInstant,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let event = ScheduleEvent::Created {
            goal: goal.clone(),
            due_at_unix_millis: due_at.unix_millis(),
        };
        match self
            .event_log
            .append(&schedule_stream(&id), ExpectedVersion::NoStream, &event)
        {
            Ok(_) => Ok(ScheduledTask {
                id,
                goal,
                due_at,
                cancellation: None,
            }),
            Err(EventLogError::WrongExpectedVersion { .. }) => {
                Err(ScheduleStoreError::AlreadyExists { schedule_id: id })
            }
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    pub fn load(&self, id: &ScheduleId) -> Result<Option<ScheduledTask>, ScheduleStoreError> {
        let events = self
            .event_log
            .replay::<ScheduleEvent>(&schedule_stream(id))
            .map_err(ScheduleStoreError::Replay)?;
        Self::project(id.clone(), events)
    }

    pub fn cancel(
        &mut self,
        id: &ScheduleId,
        cancellation: ScheduleCancellation,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        if scheduled.status() == ScheduleStatus::Cancelled {
            return Err(ScheduleStoreError::AlreadyCancelled {
                schedule_id: id.clone(),
            });
        }

        let event = ScheduleEvent::Cancelled {
            reason: cancellation.clone(),
        };
        match self
            .event_log
            .append(&schedule_stream(id), ExpectedVersion::Exact(1), &event)
        {
            Ok(_) => {
                scheduled.cancellation = Some(cancellation);
                Ok(scheduled)
            }
            Err(error @ EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) if current.status() == ScheduleStatus::Cancelled => {
                    Err(ScheduleStoreError::AlreadyCancelled {
                        schedule_id: id.clone(),
                    })
                }
                Some(_) => Err(ScheduleStoreError::EventLog(error)),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Returns intents due at or before the caller-owned cutoff, ordered by instant then ID.
    pub fn list_due(
        &self,
        cutoff: ScheduleInstant,
    ) -> Result<Vec<ScheduledTask>, ScheduleStoreError> {
        let streams = self
            .event_log
            .replay_streams_with_event_type::<ScheduleEvent>(SCHEDULE_CREATED_EVENT_TYPE)
            .map_err(ScheduleStoreError::Replay)?;
        let mut due = Vec::with_capacity(streams.len());

        for (stream_id, events) in streams {
            let Some(external_id) = stream_id.strip_prefix(SCHEDULE_STREAM_PREFIX) else {
                return Err(ScheduleStoreError::InvalidStreamId { stream_id });
            };
            let id =
                ScheduleId::new(external_id).map_err(|_| ScheduleStoreError::InvalidStreamId {
                    stream_id: stream_id.clone(),
                })?;
            let Some(scheduled) = Self::project(id, events)? else {
                return Err(ScheduleStoreError::InvalidHistory { event_count: 0 });
            };
            if scheduled.status() == ScheduleStatus::Pending && scheduled.due_at <= cutoff {
                due.push(scheduled);
            }
        }

        due.sort_by(|left, right| {
            left.due_at
                .cmp(&right.due_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(due)
    }

    fn project(
        id: ScheduleId,
        events: Vec<ScheduleEvent>,
    ) -> Result<Option<ScheduledTask>, ScheduleStoreError> {
        match events.as_slice() {
            [] => Ok(None),
            [
                ScheduleEvent::Created {
                    goal,
                    due_at_unix_millis,
                },
            ] => Ok(Some(ScheduledTask {
                id,
                goal: goal.clone(),
                due_at: ScheduleInstant::from_unix_millis(*due_at_unix_millis),
                cancellation: None,
            })),
            [
                ScheduleEvent::Created {
                    goal,
                    due_at_unix_millis,
                },
                ScheduleEvent::Cancelled { reason },
            ] => Ok(Some(ScheduledTask {
                id,
                goal: goal.clone(),
                due_at: ScheduleInstant::from_unix_millis(*due_at_unix_millis),
                cancellation: Some(reason.clone()),
            })),
            _ => Err(ScheduleStoreError::InvalidHistory {
                event_count: events.len(),
            }),
        }
    }
}

fn schedule_stream(id: &ScheduleId) -> StreamId {
    StreamId::new(format!("{SCHEDULE_STREAM_PREFIX}{id}"))
        .expect("a prefixed schedule stream is never empty")
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ScheduleEvent {
    Created {
        goal: TaskGoal,
        due_at_unix_millis: u64,
    },
    Cancelled {
        reason: ScheduleCancellation,
    },
}

impl Event for ScheduleEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => SCHEDULE_CREATED_EVENT_TYPE,
            Self::Cancelled { .. } => SCHEDULE_CANCELLED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        SCHEDULE_EVENT_PAYLOAD_VERSION
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if payload_version != SCHEDULE_EVENT_PAYLOAD_VERSION {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }

        match event_type {
            SCHEDULE_CREATED_EVENT_TYPE => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    goal: String,
                    due_at_unix_millis: u64,
                }

                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let goal = TaskGoal::new(payload.goal).map_err(|error: TaskGoalError| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Created {
                    goal,
                    due_at_unix_millis: payload.due_at_unix_millis,
                })
            }
            SCHEDULE_CANCELLED_EVENT_TYPE => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    reason: String,
                }

                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let reason = ScheduleCancellation::new(payload.reason).map_err(
                    |error: ScheduleCancellationError| DecodeError::MalformedPayload {
                        message: error.to_string(),
                    },
                )?;
                Ok(Self::Cancelled { reason })
            }
            _ => Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            }),
        }
    }
}
