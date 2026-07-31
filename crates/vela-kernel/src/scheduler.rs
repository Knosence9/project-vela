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
const SCHEDULE_CLAIMED_EVENT_TYPE: &str = "schedule.claimed";
const SCHEDULE_RELEASED_EVENT_TYPE: &str = "schedule.released";
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

/// A caller-supplied, non-blank reason for recovering a claimed schedule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ScheduleRelease(String);

impl ScheduleRelease {
    pub fn new(value: impl Into<String>) -> Result<Self, ScheduleReleaseError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(ScheduleReleaseError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleReleaseError;

impl fmt::Display for ScheduleReleaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("schedule release reason must not be blank")
    }
}

impl Error for ScheduleReleaseError {}

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
    claimed: bool,
    latest_release: Option<ScheduleRelease>,
    revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleStatus {
    Pending,
    Cancelled,
    Claimed,
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
        } else if self.claimed {
            ScheduleStatus::Claimed
        } else {
            ScheduleStatus::Pending
        }
    }

    pub fn cancellation(&self) -> Option<&ScheduleCancellation> {
        self.cancellation.as_ref()
    }

    pub fn latest_release(&self) -> Option<&ScheduleRelease> {
        self.latest_release.as_ref()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ScheduleStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists {
        schedule_id: ScheduleId,
    },
    NotFound {
        schedule_id: ScheduleId,
    },
    AlreadyCancelled {
        schedule_id: ScheduleId,
    },
    AlreadyClaimed {
        schedule_id: ScheduleId,
    },
    NotClaimed {
        schedule_id: ScheduleId,
    },
    ConcurrentModification {
        schedule_id: ScheduleId,
        expected_revision: u64,
        current_revision: u64,
    },
    NotDue {
        schedule_id: ScheduleId,
        due_at: ScheduleInstant,
        cutoff: ScheduleInstant,
    },
    InvalidStreamId {
        stream_id: String,
    },
    InvalidHistory {
        event_count: usize,
    },
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
            Self::AlreadyClaimed { schedule_id } => {
                write!(formatter, "schedule {schedule_id} is already claimed")
            }
            Self::NotClaimed { schedule_id } => {
                write!(formatter, "schedule {schedule_id} is not claimed")
            }
            Self::ConcurrentModification {
                schedule_id,
                expected_revision,
                current_revision,
            } => write!(
                formatter,
                "schedule {schedule_id} changed from revision {expected_revision} to {current_revision}"
            ),
            Self::NotDue {
                schedule_id,
                due_at,
                cutoff,
            } => write!(
                formatter,
                "schedule {schedule_id} is due at {} after cutoff {}",
                due_at.unix_millis(),
                cutoff.unix_millis()
            ),
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
            | Self::AlreadyClaimed { .. }
            | Self::NotClaimed { .. }
            | Self::ConcurrentModification { .. }
            | Self::NotDue { .. }
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
                claimed: false,
                latest_release: None,
                revision: 1,
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
        if let Some(error) = terminal_schedule_error(id, scheduled.status()) {
            return Err(error);
        }

        let event = ScheduleEvent::Cancelled {
            reason: cancellation.clone(),
        };
        match self.event_log.append(
            &schedule_stream(id),
            ExpectedVersion::Exact(scheduled.revision),
            &event,
        ) {
            Ok(_) => {
                scheduled.cancellation = Some(cancellation);
                scheduled.revision += 1;
                Ok(scheduled)
            }
            Err(EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) if current.status() == ScheduleStatus::Cancelled => {
                    Err(ScheduleStoreError::AlreadyCancelled {
                        schedule_id: id.clone(),
                    })
                }
                Some(current) if current.status() == ScheduleStatus::Claimed => {
                    Err(ScheduleStoreError::AlreadyClaimed {
                        schedule_id: id.clone(),
                    })
                }
                Some(current) => Err(ScheduleStoreError::ConcurrentModification {
                    schedule_id: id.clone(),
                    expected_revision: scheduled.revision,
                    current_revision: current.revision,
                }),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Durably reserves one due intent without dispatching or executing it.
    pub fn claim(
        &mut self,
        id: &ScheduleId,
        cutoff: ScheduleInstant,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        if let Some(error) = terminal_schedule_error(id, scheduled.status()) {
            return Err(error);
        }
        if scheduled.due_at > cutoff {
            return Err(ScheduleStoreError::NotDue {
                schedule_id: id.clone(),
                due_at: scheduled.due_at,
                cutoff,
            });
        }

        match self.event_log.append(
            &schedule_stream(id),
            ExpectedVersion::Exact(scheduled.revision),
            &ScheduleEvent::Claimed {},
        ) {
            Ok(_) => {
                scheduled.claimed = true;
                scheduled.revision += 1;
                Ok(scheduled)
            }
            Err(EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) if current.status() == ScheduleStatus::Cancelled => {
                    Err(ScheduleStoreError::AlreadyCancelled {
                        schedule_id: id.clone(),
                    })
                }
                Some(current) if current.status() == ScheduleStatus::Claimed => {
                    Err(ScheduleStoreError::AlreadyClaimed {
                        schedule_id: id.clone(),
                    })
                }
                Some(current) => Err(ScheduleStoreError::ConcurrentModification {
                    schedule_id: id.clone(),
                    expected_revision: scheduled.revision,
                    current_revision: current.revision,
                }),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Returns a claimed intent to pending eligibility with exact recovery evidence.
    pub fn release(
        &mut self,
        id: &ScheduleId,
        release: ScheduleRelease,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        match scheduled.status() {
            ScheduleStatus::Claimed => {}
            ScheduleStatus::Cancelled => {
                return Err(ScheduleStoreError::AlreadyCancelled {
                    schedule_id: id.clone(),
                });
            }
            ScheduleStatus::Pending => {
                return Err(ScheduleStoreError::NotClaimed {
                    schedule_id: id.clone(),
                });
            }
        }

        let event = ScheduleEvent::Released {
            reason: release.clone(),
        };
        match self.event_log.append(
            &schedule_stream(id),
            ExpectedVersion::Exact(scheduled.revision),
            &event,
        ) {
            Ok(_) => {
                scheduled.claimed = false;
                scheduled.latest_release = Some(release);
                scheduled.revision += 1;
                Ok(scheduled)
            }
            Err(EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) => Err(ScheduleStoreError::ConcurrentModification {
                    schedule_id: id.clone(),
                    expected_revision: scheduled.revision,
                    current_revision: current.revision,
                }),
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
        let mut due = self.discover()?;
        due.retain(|scheduled| {
            scheduled.status() == ScheduleStatus::Pending && scheduled.due_at <= cutoff
        });
        due.sort_by(|left, right| {
            left.due_at
                .cmp(&right.due_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(due)
    }

    /// Returns every durable schedule intent ordered by exact schedule ID.
    pub fn list(&self) -> Result<Vec<ScheduledTask>, ScheduleStoreError> {
        let mut schedules = self.discover()?;
        schedules.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(schedules)
    }

    fn discover(&self) -> Result<Vec<ScheduledTask>, ScheduleStoreError> {
        let streams = self
            .event_log
            .replay_streams_with_event_type::<ScheduleEvent>(SCHEDULE_CREATED_EVENT_TYPE)
            .map_err(ScheduleStoreError::Replay)?;
        let mut schedules = Vec::with_capacity(streams.len());

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
            schedules.push(scheduled);
        }

        Ok(schedules)
    }

    fn project(
        id: ScheduleId,
        events: Vec<ScheduleEvent>,
    ) -> Result<Option<ScheduledTask>, ScheduleStoreError> {
        let event_count = events.len();
        let mut events = events.into_iter();
        let Some(ScheduleEvent::Created {
            goal,
            due_at_unix_millis,
        }) = events.next()
        else {
            return if event_count == 0 {
                Ok(None)
            } else {
                Err(ScheduleStoreError::InvalidHistory { event_count })
            };
        };
        let mut scheduled = ScheduledTask {
            id,
            goal,
            due_at: ScheduleInstant::from_unix_millis(due_at_unix_millis),
            cancellation: None,
            claimed: false,
            latest_release: None,
            revision: event_count as u64,
        };

        for event in events {
            match (scheduled.status(), event) {
                (ScheduleStatus::Pending, ScheduleEvent::Cancelled { reason }) => {
                    scheduled.cancellation = Some(reason);
                }
                (ScheduleStatus::Pending, ScheduleEvent::Claimed {}) => {
                    scheduled.claimed = true;
                }
                (ScheduleStatus::Claimed, ScheduleEvent::Released { reason }) => {
                    scheduled.claimed = false;
                    scheduled.latest_release = Some(reason);
                }
                _ => return Err(ScheduleStoreError::InvalidHistory { event_count }),
            }
        }

        Ok(Some(scheduled))
    }
}

fn terminal_schedule_error(id: &ScheduleId, status: ScheduleStatus) -> Option<ScheduleStoreError> {
    match status {
        ScheduleStatus::Pending => None,
        ScheduleStatus::Cancelled => Some(ScheduleStoreError::AlreadyCancelled {
            schedule_id: id.clone(),
        }),
        ScheduleStatus::Claimed => Some(ScheduleStoreError::AlreadyClaimed {
            schedule_id: id.clone(),
        }),
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
    Claimed {},
    Released {
        reason: ScheduleRelease,
    },
}

impl Event for ScheduleEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => SCHEDULE_CREATED_EVENT_TYPE,
            Self::Cancelled { .. } => SCHEDULE_CANCELLED_EVENT_TYPE,
            Self::Claimed {} => SCHEDULE_CLAIMED_EVENT_TYPE,
            Self::Released { .. } => SCHEDULE_RELEASED_EVENT_TYPE,
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
            SCHEDULE_CLAIMED_EVENT_TYPE => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {}

                serde_json::from_slice::<Payload>(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Claimed {})
            }
            SCHEDULE_RELEASED_EVENT_TYPE => {
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
                let reason = ScheduleRelease::new(payload.reason).map_err(
                    |error: ScheduleReleaseError| DecodeError::MalformedPayload {
                        message: error.to_string(),
                    },
                )?;
                Ok(Self::Released { reason })
            }
            _ => Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            }),
        }
    }
}
