use std::{error::Error, fmt, path::Path};

use serde::Serialize;

use crate::{
    event_log::{
        DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
    },
    task::{TaskEvent, TaskGoal, TaskGoalError, TaskId, TaskIdError, task_stream},
};

const SCHEDULE_CREATED_EVENT_TYPE: &str = "schedule.created";
const SCHEDULE_CANCELLED_EVENT_TYPE: &str = "schedule.cancelled";
const SCHEDULE_CLAIMED_EVENT_TYPE: &str = "schedule.claimed";
const SCHEDULE_RELEASED_EVENT_TYPE: &str = "schedule.released";
const SCHEDULE_MATERIALIZED_EVENT_TYPE: &str = "schedule.materialized";
const SCHEDULE_EVENT_PAYLOAD_VERSION: u32 = 1;
const SCHEDULE_STREAM_PREFIX: &str = "schedule:";
const RECURRENCE_CREATED_EVENT_TYPE: &str = "recurrence.fixed_interval_created";
const RECURRENCE_EVENT_PAYLOAD_VERSION: u32 = 1;
const RECURRENCE_STREAM_PREFIX: &str = "recurrence:";

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

    /// Derives one zero-based fixed-interval occurrence without iteration,
    /// wrapping, or saturation.
    pub fn checked_advance_by(
        self,
        interval: ScheduleInterval,
        offset: u64,
    ) -> Result<Self, ScheduleOccurrenceError> {
        interval
            .millis()
            .checked_mul(offset)
            .and_then(|elapsed| self.0.checked_add(elapsed))
            .map(Self)
            .ok_or(ScheduleOccurrenceError {
                instant: self,
                interval,
                offset,
            })
    }

    /// Advances by one exact fixed interval without wrapping or saturation.
    pub fn checked_advance(self, interval: ScheduleInterval) -> Result<Self, ScheduleAdvanceError> {
        self.0
            .checked_add(interval.0)
            .map(Self)
            .ok_or(ScheduleAdvanceError {
                instant: self,
                interval,
            })
    }
}

/// A deterministic positive fixed interval in exact milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleInterval(u64);

impl ScheduleInterval {
    pub const fn from_millis(millis: u64) -> Result<Self, ScheduleIntervalError> {
        if millis == 0 {
            Err(ScheduleIntervalError)
        } else {
            Ok(Self(millis))
        }
    }

    pub const fn millis(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleIntervalError;

impl fmt::Display for ScheduleIntervalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("schedule interval must be greater than zero milliseconds")
    }
}

impl Error for ScheduleIntervalError {}

/// Evidence that an indexed fixed-interval occurrence exceeded the instant range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleOccurrenceError {
    instant: ScheduleInstant,
    interval: ScheduleInterval,
    offset: u64,
}

impl ScheduleOccurrenceError {
    pub const fn instant(&self) -> ScheduleInstant {
        self.instant
    }

    pub const fn interval(&self) -> ScheduleInterval {
        self.interval
    }

    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

impl fmt::Display for ScheduleOccurrenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "schedule instant {} cannot advance by interval {} milliseconds at offset {}",
            self.instant.unix_millis(),
            self.interval.millis(),
            self.offset
        )
    }
}

impl Error for ScheduleOccurrenceError {}

/// Evidence that exact fixed-interval advancement exceeded the instant range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleAdvanceError {
    instant: ScheduleInstant,
    interval: ScheduleInterval,
}

impl ScheduleAdvanceError {
    pub const fn instant(&self) -> ScheduleInstant {
        self.instant
    }

    pub const fn interval(&self) -> ScheduleInterval {
        self.interval
    }
}

impl fmt::Display for ScheduleAdvanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "schedule instant {} cannot advance by {} milliseconds",
            self.instant.unix_millis(),
            self.interval.millis()
        )
    }
}

impl Error for ScheduleAdvanceError {}

/// An opaque, non-blank identity for one durable recurrence definition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecurrenceId(String);

impl RecurrenceId {
    pub fn new(value: impl Into<String>) -> Result<Self, RecurrenceIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(RecurrenceIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecurrenceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecurrenceIdError;

impl fmt::Display for RecurrenceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("recurrence id must not be blank")
    }
}

impl Error for RecurrenceIdError {}

/// The exact positive number of occurrences in a finite recurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccurrenceCount(u64);

impl OccurrenceCount {
    pub const fn new(value: u64) -> Result<Self, OccurrenceCountError> {
        if value == 0 {
            Err(OccurrenceCountError)
        } else {
            Ok(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    const fn final_offset(self) -> u64 {
        self.0 - 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccurrenceCountError;

impl fmt::Display for OccurrenceCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("recurrence occurrence count must be greater than zero")
    }
}

impl Error for OccurrenceCountError {}

/// A positive, allocation-bounded recurrence occurrence page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccurrencePageSize(u16);

impl OccurrencePageSize {
    /// The largest accepted occurrence page size.
    pub const MAX: u64 = 1024;

    /// Validates a caller-owned page size before projection or allocation.
    pub const fn new(value: u64) -> Result<Self, OccurrencePageSizeError> {
        if value == 0 {
            Err(OccurrencePageSizeError::Zero)
        } else if value > Self::MAX {
            Err(OccurrencePageSizeError::TooLarge {
                requested: value,
                maximum: Self::MAX,
            })
        } else {
            Ok(Self(value as u16))
        }
    }

    /// Returns the exact validated page size.
    pub const fn get(self) -> u64 {
        self.0 as u64
    }
}

/// Why a caller-owned occurrence page size is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OccurrencePageSizeError {
    Zero,
    TooLarge { requested: u64, maximum: u64 },
}

impl fmt::Display for OccurrencePageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("recurrence occurrence page size must be positive"),
            Self::TooLarge { requested, maximum } => write!(
                formatter,
                "recurrence occurrence page size {requested} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for OccurrencePageSizeError {}

/// One immutable, inert, finite fixed-interval recurrence definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedIntervalRecurrence {
    id: RecurrenceId,
    goal: TaskGoal,
    anchor: ScheduleInstant,
    interval: ScheduleInterval,
    occurrence_count: OccurrenceCount,
    final_occurrence: ScheduleInstant,
    revision: u64,
}

impl FixedIntervalRecurrence {
    pub fn id(&self) -> &RecurrenceId {
        &self.id
    }

    pub fn goal(&self) -> &TaskGoal {
        &self.goal
    }

    pub const fn anchor(&self) -> ScheduleInstant {
        self.anchor
    }

    pub const fn interval(&self) -> ScheduleInterval {
        self.interval
    }

    pub const fn occurrence_count(&self) -> OccurrenceCount {
        self.occurrence_count
    }

    pub const fn final_occurrence(&self) -> ScheduleInstant {
        self.final_occurrence
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Projects one exact zero-based occurrence without reading storage or time.
    pub fn occurrence_at(
        &self,
        offset: u64,
    ) -> Result<RecurrenceOccurrence, RecurrenceOccurrenceLookupError> {
        if offset >= self.occurrence_count.get() {
            return Err(RecurrenceOccurrenceLookupError::OutOfRange {
                recurrence_id: self.id.clone(),
                requested_offset: offset,
                occurrence_count: self.occurrence_count,
            });
        }
        let instant = self
            .anchor
            .checked_advance_by(self.interval, offset)
            .expect("validated finite recurrence occurrences are representable");
        Ok(RecurrenceOccurrence {
            recurrence_id: self.id.clone(),
            goal: self.goal.clone(),
            offset,
            instant,
            recurrence_revision: self.revision,
        })
    }

    /// Projects one bounded page in increasing offset order without storage or time access.
    pub fn occurrences_page(
        &self,
        start_offset: u64,
        page_size: OccurrencePageSize,
    ) -> Result<RecurrenceOccurrencePage, RecurrenceOccurrenceLookupError> {
        if start_offset >= self.occurrence_count.get() {
            return Err(RecurrenceOccurrenceLookupError::OutOfRange {
                recurrence_id: self.id.clone(),
                requested_offset: start_offset,
                occurrence_count: self.occurrence_count,
            });
        }

        let end_offset = start_offset
            .saturating_add(page_size.get())
            .min(self.occurrence_count.get());
        let mut occurrences = Vec::with_capacity((end_offset - start_offset) as usize);
        for offset in start_offset..end_offset {
            occurrences.push(
                self.occurrence_at(offset)
                    .expect("page bounds contain only authored recurrence occurrences"),
            );
        }
        let next_offset = (end_offset < self.occurrence_count.get()).then_some(end_offset);

        Ok(RecurrenceOccurrencePage {
            occurrences,
            next_offset,
        })
    }
}

/// One bounded, inert page of exact recurrence occurrence projections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecurrenceOccurrencePage {
    occurrences: Vec<RecurrenceOccurrence>,
    next_offset: Option<u64>,
}

impl RecurrenceOccurrencePage {
    /// Returns the exact occurrence projections in increasing offset order.
    pub fn occurrences(&self) -> &[RecurrenceOccurrence] {
        &self.occurrences
    }

    /// Returns the first unreturned authored offset when another page exists.
    pub const fn next_offset(&self) -> Option<u64> {
        self.next_offset
    }
}

/// One inert read-only projection from a finite recurrence definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecurrenceOccurrence {
    recurrence_id: RecurrenceId,
    goal: TaskGoal,
    offset: u64,
    instant: ScheduleInstant,
    recurrence_revision: u64,
}

impl RecurrenceOccurrence {
    pub fn recurrence_id(&self) -> &RecurrenceId {
        &self.recurrence_id
    }

    pub fn goal(&self) -> &TaskGoal {
        &self.goal
    }

    pub const fn offset(&self) -> u64 {
        self.offset
    }

    pub const fn instant(&self) -> ScheduleInstant {
        self.instant
    }

    pub const fn recurrence_revision(&self) -> u64 {
        self.recurrence_revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RecurrenceOccurrenceLookupError {
    OutOfRange {
        recurrence_id: RecurrenceId,
        requested_offset: u64,
        occurrence_count: OccurrenceCount,
    },
}

impl fmt::Display for RecurrenceOccurrenceLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange {
                recurrence_id,
                requested_offset,
                occurrence_count,
            } => write!(
                formatter,
                "recurrence {recurrence_id} offset {requested_offset} is outside occurrence count {}",
                occurrence_count.get()
            ),
        }
    }
}

impl Error for RecurrenceOccurrenceLookupError {}

#[derive(Debug)]
#[non_exhaustive]
pub enum RecurrenceStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists {
        recurrence_id: RecurrenceId,
    },
    OccurrenceOverflow {
        recurrence_id: RecurrenceId,
        source: ScheduleOccurrenceError,
    },
    InvalidHistory {
        event_count: usize,
    },
}

impl fmt::Display for RecurrenceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "recurrence event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "recurrence replay error: {error}"),
            Self::AlreadyExists { recurrence_id } => {
                write!(formatter, "recurrence {recurrence_id} already exists")
            }
            Self::OccurrenceOverflow {
                recurrence_id,
                source,
            } => write!(
                formatter,
                "recurrence {recurrence_id} is not representable: {source}"
            ),
            Self::InvalidHistory { event_count } => {
                write!(
                    formatter,
                    "invalid recurrence history with {event_count} events"
                )
            }
        }
    }
}

impl Error for RecurrenceStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::OccurrenceOverflow { source, .. } => Some(source),
            Self::AlreadyExists { .. } | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous durable store for inert finite fixed-interval recurrence definitions.
pub struct RecurrenceStore {
    event_log: EventLog,
}

impl RecurrenceStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RecurrenceStoreError> {
        EventLog::open(path)
            .map(|event_log| Self { event_log })
            .map_err(RecurrenceStoreError::EventLog)
    }

    pub fn create(
        &mut self,
        id: RecurrenceId,
        goal: TaskGoal,
        anchor: ScheduleInstant,
        interval: ScheduleInterval,
        occurrence_count: OccurrenceCount,
    ) -> Result<FixedIntervalRecurrence, RecurrenceStoreError> {
        let final_occurrence = anchor
            .checked_advance_by(interval, occurrence_count.final_offset())
            .map_err(|source| RecurrenceStoreError::OccurrenceOverflow {
                recurrence_id: id.clone(),
                source,
            })?;
        let event = RecurrenceEvent::Created {
            goal: goal.clone(),
            anchor_unix_millis: anchor.unix_millis(),
            interval_millis: interval.millis(),
            occurrence_count: occurrence_count.get(),
        };
        match self
            .event_log
            .append(&recurrence_stream(&id), ExpectedVersion::NoStream, &event)
        {
            Ok(_) => Ok(FixedIntervalRecurrence {
                id,
                goal,
                anchor,
                interval,
                occurrence_count,
                final_occurrence,
                revision: 1,
            }),
            Err(EventLogError::WrongExpectedVersion { .. }) => {
                Err(RecurrenceStoreError::AlreadyExists { recurrence_id: id })
            }
            Err(error) => Err(RecurrenceStoreError::EventLog(error)),
        }
    }

    pub fn load(
        &self,
        id: &RecurrenceId,
    ) -> Result<Option<FixedIntervalRecurrence>, RecurrenceStoreError> {
        let events = self
            .event_log
            .replay::<RecurrenceEvent>(&recurrence_stream(id))
            .map_err(RecurrenceStoreError::Replay)?;
        match events.as_slice() {
            [] => Ok(None),
            [
                RecurrenceEvent::Created {
                    goal,
                    anchor_unix_millis,
                    interval_millis,
                    occurrence_count,
                },
            ] => {
                let anchor = ScheduleInstant::from_unix_millis(*anchor_unix_millis);
                let interval = ScheduleInterval::from_millis(*interval_millis)
                    .expect("decoded recurrence intervals are positive");
                let occurrence_count = OccurrenceCount::new(*occurrence_count)
                    .expect("decoded recurrence counts are positive");
                let final_occurrence = anchor
                    .checked_advance_by(interval, occurrence_count.final_offset())
                    .expect("decoded recurrence ranges are representable");
                Ok(Some(FixedIntervalRecurrence {
                    id: id.clone(),
                    goal: goal.clone(),
                    anchor,
                    interval,
                    occurrence_count,
                    final_occurrence,
                    revision: 1,
                }))
            }
            _ => Err(RecurrenceStoreError::InvalidHistory {
                event_count: events.len(),
            }),
        }
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
    task_id: Option<TaskId>,
    revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleStatus {
    Pending,
    Cancelled,
    Claimed,
    Materialized,
}

/// One validated persisted transition in an exact schedule's lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScheduleHistoryEvent {
    Created {
        goal: TaskGoal,
        due_at: ScheduleInstant,
    },
    Cancelled {
        reason: ScheduleCancellation,
    },
    Claimed,
    Released {
        reason: ScheduleRelease,
    },
    Materialized {
        task_id: TaskId,
    },
}

/// One revision-bearing entry from a validated durable schedule history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleHistoryEntry {
    revision: u64,
    event: ScheduleHistoryEvent,
}

impl ScheduleHistoryEntry {
    /// The one-based event-stream revision occupied by this lifecycle event.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn event(&self) -> &ScheduleHistoryEvent {
        &self.event
    }
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
        if self.task_id.is_some() {
            ScheduleStatus::Materialized
        } else if self.cancellation.is_some() {
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

    pub fn task_id(&self) -> Option<&TaskId> {
        self.task_id.as_ref()
    }

    /// The exact persisted revision represented by this projection.
    pub fn revision(&self) -> u64 {
        self.revision
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
    AlreadyMaterialized {
        schedule_id: ScheduleId,
        task_id: TaskId,
    },
    TaskAlreadyExists {
        task_id: TaskId,
    },
    AmbiguousTaskBinding {
        task_id: TaskId,
        schedule_count: usize,
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
            Self::AlreadyMaterialized {
                schedule_id,
                task_id,
            } => write!(
                formatter,
                "schedule {schedule_id} is already materialized as task {task_id}"
            ),
            Self::TaskAlreadyExists { task_id } => {
                write!(formatter, "task {task_id} already exists")
            }
            Self::AmbiguousTaskBinding {
                task_id,
                schedule_count,
            } => write!(
                formatter,
                "task {task_id} is bound to {schedule_count} schedules"
            ),
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
            | Self::AlreadyMaterialized { .. }
            | Self::TaskAlreadyExists { .. }
            | Self::AmbiguousTaskBinding { .. }
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

    /// Opens existing schedule evidence without database creation or write authority.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, ScheduleStoreError> {
        EventLog::open_read_only(path)
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
                task_id: None,
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

    /// Returns complete validated lifecycle evidence in exact revision order.
    pub fn history(
        &self,
        id: &ScheduleId,
    ) -> Result<Option<Vec<ScheduleHistoryEntry>>, ScheduleStoreError> {
        let events = self
            .event_log
            .replay::<ScheduleEvent>(&schedule_stream(id))
            .map_err(ScheduleStoreError::Replay)?;
        if events.is_empty() {
            return Ok(None);
        }
        Self::project(id.clone(), events.clone())?;

        Ok(Some(
            events
                .into_iter()
                .enumerate()
                .map(|(index, event)| ScheduleHistoryEntry {
                    revision: index as u64 + 1,
                    event: ScheduleHistoryEvent::from(event),
                })
                .collect(),
        ))
    }

    /// Resolves exact schedule provenance for one materialized task without changing lifecycle state.
    pub fn find_by_task_id(
        &self,
        task_id: &TaskId,
    ) -> Result<Option<ScheduledTask>, ScheduleStoreError> {
        let mut matches = self
            .discover()?
            .into_iter()
            .filter(|scheduled| scheduled.task_id() == Some(task_id))
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(ScheduleStoreError::AmbiguousTaskBinding {
                task_id: task_id.clone(),
                schedule_count: matches.len(),
            });
        }
        Ok(matches.pop())
    }

    /// Withdraws one exact pending revision without granting interruption authority.
    pub fn cancel(
        &mut self,
        id: &ScheduleId,
        expected_revision: u64,
        cancellation: ScheduleCancellation,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        validate_revision(id, &scheduled, expected_revision)?;
        if let Some(error) = terminal_schedule_error(id, &scheduled) {
            return Err(error);
        }

        let event = ScheduleEvent::Cancelled {
            reason: cancellation.clone(),
        };
        match self.event_log.append(
            &schedule_stream(id),
            ExpectedVersion::Exact(expected_revision),
            &event,
        ) {
            Ok(_) => {
                scheduled.cancellation = Some(cancellation);
                scheduled.revision += 1;
                Ok(scheduled)
            }
            Err(EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) => Err(ScheduleStoreError::ConcurrentModification {
                    schedule_id: id.clone(),
                    expected_revision,
                    current_revision: current.revision,
                }),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Durably reserves one exact due revision without dispatching or executing it.
    pub fn claim(
        &mut self,
        id: &ScheduleId,
        expected_revision: u64,
        cutoff: ScheduleInstant,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        validate_revision(id, &scheduled, expected_revision)?;
        if let Some(error) = terminal_schedule_error(id, &scheduled) {
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
            ExpectedVersion::Exact(expected_revision),
            &ScheduleEvent::Claimed {},
        ) {
            Ok(_) => {
                scheduled.claimed = true;
                scheduled.revision += 1;
                Ok(scheduled)
            }
            Err(EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) => Err(ScheduleStoreError::ConcurrentModification {
                    schedule_id: id.clone(),
                    expected_revision,
                    current_revision: current.revision,
                }),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Returns an exact claimed revision to pending eligibility with recovery evidence.
    pub fn release(
        &mut self,
        id: &ScheduleId,
        expected_revision: u64,
        release: ScheduleRelease,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        validate_revision(id, &scheduled, expected_revision)?;
        require_claimed(id, &scheduled)?;

        let event = ScheduleEvent::Released {
            reason: release.clone(),
        };
        match self.event_log.append(
            &schedule_stream(id),
            ExpectedVersion::Exact(expected_revision),
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
                    expected_revision,
                    current_revision: current.revision,
                }),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Atomically turns one exact claimed revision into one inert active task.
    pub fn materialize(
        &mut self,
        id: &ScheduleId,
        expected_revision: u64,
        task_id: TaskId,
    ) -> Result<ScheduledTask, ScheduleStoreError> {
        let Some(mut scheduled) = self.load(id)? else {
            return Err(ScheduleStoreError::NotFound {
                schedule_id: id.clone(),
            });
        };
        validate_revision(id, &scheduled, expected_revision)?;
        require_claimed(id, &scheduled)?;

        let schedule_event = ScheduleEvent::Materialized {
            task_id: task_id.clone(),
        };
        let task_event = TaskEvent::Started {
            goal: scheduled.goal.clone(),
        };
        match self.event_log.append_pair(
            &schedule_stream(id),
            ExpectedVersion::Exact(expected_revision),
            &schedule_event,
            &task_stream(&task_id),
            ExpectedVersion::NoStream,
            &task_event,
        ) {
            Ok(_) => {
                scheduled.task_id = Some(task_id);
                scheduled.revision += 1;
                Ok(scheduled)
            }
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                ..
            }) => Err(ScheduleStoreError::TaskAlreadyExists { task_id }),
            Err(EventLogError::WrongExpectedVersion { .. }) => match self.load(id)? {
                Some(current) => Err(ScheduleStoreError::ConcurrentModification {
                    schedule_id: id.clone(),
                    expected_revision,
                    current_revision: current.revision,
                }),
                None => Err(ScheduleStoreError::NotFound {
                    schedule_id: id.clone(),
                }),
            },
            Err(error) => Err(ScheduleStoreError::EventLog(error)),
        }
    }

    /// Atomically materializes the earliest pending due intent as one inert active task.
    ///
    /// Selection uses the same due-instant then exact-ID order as [`Self::list_due`].
    /// A competing persisted schedule transition restarts selection. The caller-owned
    /// task identity is never replaced or regenerated. If that identity already has a
    /// task stream, this returns [`ScheduleStoreError::TaskAlreadyExists`] without
    /// changing the selected schedule.
    pub fn materialize_next_due(
        &mut self,
        cutoff: ScheduleInstant,
        task_id: TaskId,
    ) -> Result<Option<ScheduledTask>, ScheduleStoreError> {
        loop {
            let Some(mut next) = self.list_due(cutoff)?.into_iter().next() else {
                return Ok(None);
            };
            let expected_revision = next.revision();
            let schedule_event = ScheduleEvent::Materialized {
                task_id: task_id.clone(),
            };
            let task_event = TaskEvent::Started {
                goal: next.goal.clone(),
            };
            match self.event_log.append_pair(
                &schedule_stream(next.id()),
                ExpectedVersion::Exact(expected_revision),
                &schedule_event,
                &task_stream(&task_id),
                ExpectedVersion::NoStream,
                &task_event,
            ) {
                Ok(_) => {
                    next.task_id = Some(task_id);
                    next.revision += 1;
                    return Ok(Some(next));
                }
                Err(EventLogError::WrongExpectedVersion {
                    expected: ExpectedVersion::NoStream,
                    ..
                }) => return Err(ScheduleStoreError::TaskAlreadyExists { task_id }),
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(ScheduleStoreError::EventLog(error)),
            }
        }
    }

    /// Reserves the earliest still-pending due intent without dispatching or executing it.
    ///
    /// Selection uses the same due-instant then exact-ID order as [`Self::list_due`].
    /// A competing persisted transition causes selection to restart so callers either
    /// receive one claimed schedule or observe that no eligible intent remains.
    pub fn claim_next_due(
        &mut self,
        cutoff: ScheduleInstant,
    ) -> Result<Option<ScheduledTask>, ScheduleStoreError> {
        loop {
            let Some(next) = self.list_due(cutoff)?.into_iter().next() else {
                return Ok(None);
            };
            match self.claim(next.id(), next.revision(), cutoff) {
                Ok(claimed) => return Ok(Some(claimed)),
                Err(ScheduleStoreError::ConcurrentModification { .. }) => {}
                Err(error) => return Err(error),
            }
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

    /// Returns schedules with the exact persisted status, ordered by exact schedule ID.
    pub fn list_by_status(
        &self,
        status: ScheduleStatus,
    ) -> Result<Vec<ScheduledTask>, ScheduleStoreError> {
        let mut schedules = self.discover()?;
        schedules.retain(|scheduled| scheduled.status() == status);
        schedules.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(schedules)
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
            task_id: None,
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
                (
                    ScheduleStatus::Pending | ScheduleStatus::Claimed,
                    ScheduleEvent::Materialized { task_id },
                ) => {
                    scheduled.task_id = Some(task_id);
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

fn terminal_schedule_error(
    id: &ScheduleId,
    scheduled: &ScheduledTask,
) -> Option<ScheduleStoreError> {
    match scheduled.status() {
        ScheduleStatus::Pending => None,
        ScheduleStatus::Cancelled => Some(ScheduleStoreError::AlreadyCancelled {
            schedule_id: id.clone(),
        }),
        ScheduleStatus::Claimed => Some(ScheduleStoreError::AlreadyClaimed {
            schedule_id: id.clone(),
        }),
        ScheduleStatus::Materialized => Some(ScheduleStoreError::AlreadyMaterialized {
            schedule_id: id.clone(),
            task_id: scheduled
                .task_id
                .clone()
                .expect("materialized status always carries a task ID"),
        }),
    }
}

fn validate_revision(
    id: &ScheduleId,
    scheduled: &ScheduledTask,
    expected_revision: u64,
) -> Result<(), ScheduleStoreError> {
    if scheduled.revision == expected_revision {
        Ok(())
    } else {
        Err(ScheduleStoreError::ConcurrentModification {
            schedule_id: id.clone(),
            expected_revision,
            current_revision: scheduled.revision,
        })
    }
}

fn require_claimed(id: &ScheduleId, scheduled: &ScheduledTask) -> Result<(), ScheduleStoreError> {
    match scheduled.status() {
        ScheduleStatus::Claimed => Ok(()),
        ScheduleStatus::Pending => Err(ScheduleStoreError::NotClaimed {
            schedule_id: id.clone(),
        }),
        ScheduleStatus::Cancelled => Err(ScheduleStoreError::AlreadyCancelled {
            schedule_id: id.clone(),
        }),
        ScheduleStatus::Materialized => Err(ScheduleStoreError::AlreadyMaterialized {
            schedule_id: id.clone(),
            task_id: scheduled
                .task_id
                .clone()
                .expect("materialized status always carries a task ID"),
        }),
    }
}

fn schedule_stream(id: &ScheduleId) -> StreamId {
    StreamId::new(format!("{SCHEDULE_STREAM_PREFIX}{id}"))
        .expect("a prefixed schedule stream is never empty")
}

fn recurrence_stream(id: &RecurrenceId) -> StreamId {
    StreamId::new(format!("{RECURRENCE_STREAM_PREFIX}{id}"))
        .expect("a prefixed recurrence stream is never empty")
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum RecurrenceEvent {
    Created {
        goal: TaskGoal,
        anchor_unix_millis: u64,
        interval_millis: u64,
        occurrence_count: u64,
    },
}

impl Event for RecurrenceEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => RECURRENCE_CREATED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        RECURRENCE_EVENT_PAYLOAD_VERSION
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if event_type != RECURRENCE_CREATED_EVENT_TYPE
            || payload_version != RECURRENCE_EVENT_PAYLOAD_VERSION
        {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }

        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Payload {
            goal: String,
            anchor_unix_millis: u64,
            interval_millis: u64,
            occurrence_count: u64,
        }

        let payload: Payload =
            serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                message: error.to_string(),
            })?;
        let goal = TaskGoal::new(payload.goal).map_err(|error: TaskGoalError| {
            DecodeError::MalformedPayload {
                message: error.to_string(),
            }
        })?;
        let interval = ScheduleInterval::from_millis(payload.interval_millis).map_err(|error| {
            DecodeError::MalformedPayload {
                message: error.to_string(),
            }
        })?;
        let occurrence_count = OccurrenceCount::new(payload.occurrence_count).map_err(|error| {
            DecodeError::MalformedPayload {
                message: error.to_string(),
            }
        })?;
        ScheduleInstant::from_unix_millis(payload.anchor_unix_millis)
            .checked_advance_by(interval, occurrence_count.final_offset())
            .map_err(|error| DecodeError::MalformedPayload {
                message: error.to_string(),
            })?;

        Ok(Self::Created {
            goal,
            anchor_unix_millis: payload.anchor_unix_millis,
            interval_millis: payload.interval_millis,
            occurrence_count: payload.occurrence_count,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
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
    Materialized {
        task_id: TaskId,
    },
}

impl From<ScheduleEvent> for ScheduleHistoryEvent {
    fn from(event: ScheduleEvent) -> Self {
        match event {
            ScheduleEvent::Created {
                goal,
                due_at_unix_millis,
            } => Self::Created {
                goal,
                due_at: ScheduleInstant::from_unix_millis(due_at_unix_millis),
            },
            ScheduleEvent::Cancelled { reason } => Self::Cancelled { reason },
            ScheduleEvent::Claimed {} => Self::Claimed,
            ScheduleEvent::Released { reason } => Self::Released { reason },
            ScheduleEvent::Materialized { task_id } => Self::Materialized { task_id },
        }
    }
}

impl Event for ScheduleEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => SCHEDULE_CREATED_EVENT_TYPE,
            Self::Cancelled { .. } => SCHEDULE_CANCELLED_EVENT_TYPE,
            Self::Claimed {} => SCHEDULE_CLAIMED_EVENT_TYPE,
            Self::Released { .. } => SCHEDULE_RELEASED_EVENT_TYPE,
            Self::Materialized { .. } => SCHEDULE_MATERIALIZED_EVENT_TYPE,
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
            SCHEDULE_MATERIALIZED_EVENT_TYPE => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    task_id: String,
                }

                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let task_id = TaskId::new(payload.task_id).map_err(|error: TaskIdError| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Materialized { task_id })
            }
            _ => Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            }),
        }
    }
}
