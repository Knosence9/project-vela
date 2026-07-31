use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};
use crate::session::{
    Session, SessionEvent, SessionId, SessionStatus, SessionStore, SessionStoreError,
    SessionTurnContent, SessionTurnRole, session_stream,
};

const TASK_STARTED_EVENT_TYPE: &str = "task.started";
const TASK_COMPLETED_EVENT_TYPE: &str = "task.completed";
const TASK_CANCELLED_EVENT_TYPE: &str = "task.cancelled";
const TASK_FAILED_EVENT_TYPE: &str = "task.failed";
const TASK_SESSION_ASSOCIATED_EVENT_TYPE: &str = "task.session_associated";
const TASK_OBSERVATION_APPENDED_EVENT_TYPE: &str = "task.observation_appended";
const TASK_EVENT_PAYLOAD_VERSION: u32 = 1;
const TASK_OBSERVATION_PAYLOAD_VERSION: u32 = 2;
const TASK_STRUCTURED_VERIFICATION_PAYLOAD_VERSION: u32 = 3;
const TASK_IDENTIFIED_VERIFICATION_PAYLOAD_VERSION: u32 = 4;
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

/// A caller-supplied identifier that is unique among one task's observations.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskObservationId(String);

impl TaskObservationId {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskObservationIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(TaskObservationIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskObservationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskObservationIdError;

impl fmt::Display for TaskObservationIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task observation id must not be blank")
    }
}

impl std::error::Error for TaskObservationIdError {}

/// The stable typed category of persisted task execution evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskObservationKind {
    Attempt,
    Diagnostic,
    Correction,
    Verification,
}

/// The independently observed result of checking one exact task attempt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskVerificationOutcome {
    Passed,
    Failed,
}

/// A caller-owned non-blank identity for one independent verification check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskVerificationCheck(String);

impl TaskVerificationCheck {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskVerificationCheckError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(TaskVerificationCheckError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskVerificationCheckError;

impl fmt::Display for TaskVerificationCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task verification check must not be blank")
    }
}

impl std::error::Error for TaskVerificationCheckError {}

/// An ordered, non-empty set of unique checks required for one task attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskVerificationGateSet(Vec<TaskVerificationCheck>);

impl TaskVerificationGateSet {
    pub fn new(checks: Vec<TaskVerificationCheck>) -> Result<Self, TaskVerificationGateSetError> {
        if checks.is_empty() {
            return Err(TaskVerificationGateSetError::Empty);
        }
        for (index, check) in checks.iter().enumerate() {
            if checks[..index].contains(check) {
                return Err(TaskVerificationGateSetError::Duplicate {
                    check: check.clone(),
                });
            }
        }
        Ok(Self(checks))
    }

    pub fn checks(&self) -> &[TaskVerificationCheck] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TaskVerificationGateSetError {
    Empty,
    Duplicate { check: TaskVerificationCheck },
}

impl fmt::Display for TaskVerificationGateSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("task verification gate set must not be empty"),
            Self::Duplicate { check } => {
                write!(
                    formatter,
                    "task verification gate set repeats check {}",
                    check.as_str()
                )
            }
        }
    }
}

impl std::error::Error for TaskVerificationGateSetError {}

/// The current state of one gate or the aggregate required gate set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskVerificationGateStatus {
    Pending,
    Passed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskVerificationGate {
    check: TaskVerificationCheck,
    status: TaskVerificationGateStatus,
}

impl TaskVerificationGate {
    pub fn check(&self) -> &TaskVerificationCheck {
        &self.check
    }

    pub fn status(&self) -> TaskVerificationGateStatus {
        self.status
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskVerificationGateReport {
    status: TaskVerificationGateStatus,
    gates: Vec<TaskVerificationGate>,
}

impl TaskVerificationGateReport {
    pub fn status(&self) -> TaskVerificationGateStatus {
        self.status
    }

    pub fn gates(&self) -> &[TaskVerificationGate] {
        &self.gates
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TaskVerificationGateEvaluationError {
    AttemptNotFound {
        observation_id: TaskObservationId,
    },
    ObservationNotAttempt {
        observation_id: TaskObservationId,
        kind: TaskObservationKind,
    },
}

impl fmt::Display for TaskVerificationGateEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AttemptNotFound { observation_id } => write!(
                formatter,
                "task attempt observation {observation_id} was not found"
            ),
            Self::ObservationNotAttempt {
                observation_id,
                kind,
            } => write!(
                formatter,
                "task observation {observation_id} has kind {kind:?}, not attempt"
            ),
        }
    }
}

impl std::error::Error for TaskVerificationGateEvaluationError {}

/// A rejected attempt to complete a task through caller-required verification gates.
#[derive(Debug)]
#[non_exhaustive]
pub enum TaskVerifiedCompletionError {
    Store(TaskStoreError),
    GateEvaluation(TaskVerificationGateEvaluationError),
    GatesPending { report: TaskVerificationGateReport },
    GatesFailed { report: TaskVerificationGateReport },
}

impl fmt::Display for TaskVerifiedCompletionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "verified task completion failed: {error}"),
            Self::GateEvaluation(error) => {
                write!(
                    formatter,
                    "verified task completion gate evaluation failed: {error}"
                )
            }
            Self::GatesPending { .. } => {
                formatter.write_str("verified task completion gates are pending")
            }
            Self::GatesFailed { .. } => {
                formatter.write_str("verified task completion gates failed")
            }
        }
    }
}

impl std::error::Error for TaskVerifiedCompletionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::GateEvaluation(error) => Some(error),
            Self::GatesPending { .. } | Self::GatesFailed { .. } => None,
        }
    }
}

impl From<TaskStoreError> for TaskVerifiedCompletionError {
    fn from(error: TaskStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<TaskVerificationGateEvaluationError> for TaskVerifiedCompletionError {
    fn from(error: TaskVerificationGateEvaluationError) -> Self {
        Self::GateEvaluation(error)
    }
}

/// Non-blank opaque UTF-8 evidence recorded for one task observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskObservationText(String);

impl TaskObservationText {
    pub fn new(value: impl Into<String>) -> Result<Self, TaskObservationTextError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(TaskObservationTextError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskObservationTextError;

impl fmt::Display for TaskObservationTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("task observation text must not be blank")
    }
}

impl std::error::Error for TaskObservationTextError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskObservation {
    id: TaskObservationId,
    kind: TaskObservationKind,
    text: TaskObservationText,
    parent_attempt_id: Option<TaskObservationId>,
    verification_outcome: Option<TaskVerificationOutcome>,
    verification_check: Option<TaskVerificationCheck>,
}

impl TaskObservation {
    pub fn id(&self) -> &TaskObservationId {
        &self.id
    }

    pub fn kind(&self) -> TaskObservationKind {
        self.kind
    }

    pub fn text(&self) -> &TaskObservationText {
        &self.text
    }

    /// The earlier attempt that this evidence describes, when grouped into an episode.
    pub fn parent_attempt_id(&self) -> Option<&TaskObservationId> {
        self.parent_attempt_id.as_ref()
    }

    /// The structured checker outcome, when written by the typed verification boundary.
    pub fn verification_outcome(&self) -> Option<TaskVerificationOutcome> {
        self.verification_outcome
    }

    /// The caller-owned check identity, when written by identified verification.
    pub fn verification_check(&self) -> Option<&TaskVerificationCheck> {
        self.verification_check.as_ref()
    }
}

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
    observations: Vec<TaskObservation>,
}

impl Task {
    pub(crate) fn into_completed(mut self, output: TaskOutput) -> Self {
        self.status = TaskStatus::Completed;
        self.output = Some(output);
        self
    }

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

    pub fn observations(&self) -> &[TaskObservation] {
        &self.observations
    }

    /// Evaluates caller-required checks from durable evidence for one exact attempt.
    pub fn evaluate_verification_gates(
        &self,
        attempt_id: &TaskObservationId,
        gate_set: &TaskVerificationGateSet,
    ) -> Result<TaskVerificationGateReport, TaskVerificationGateEvaluationError> {
        let Some(attempt) = self
            .observations
            .iter()
            .find(|observation| observation.id == *attempt_id)
        else {
            return Err(TaskVerificationGateEvaluationError::AttemptNotFound {
                observation_id: attempt_id.clone(),
            });
        };
        if attempt.kind != TaskObservationKind::Attempt {
            return Err(TaskVerificationGateEvaluationError::ObservationNotAttempt {
                observation_id: attempt_id.clone(),
                kind: attempt.kind,
            });
        }

        let gates: Vec<_> = gate_set
            .checks()
            .iter()
            .map(|check| {
                let status = self
                    .observations
                    .iter()
                    .rev()
                    .find(|observation| {
                        observation.parent_attempt_id.as_ref() == Some(attempt_id)
                            && observation.verification_check.as_ref() == Some(check)
                    })
                    .and_then(|observation| observation.verification_outcome)
                    .map_or(
                        TaskVerificationGateStatus::Pending,
                        |outcome| match outcome {
                            TaskVerificationOutcome::Passed => TaskVerificationGateStatus::Passed,
                            TaskVerificationOutcome::Failed => TaskVerificationGateStatus::Failed,
                        },
                    );
                TaskVerificationGate {
                    check: check.clone(),
                    status,
                }
            })
            .collect();
        let status = if gates
            .iter()
            .any(|gate| gate.status == TaskVerificationGateStatus::Failed)
        {
            TaskVerificationGateStatus::Failed
        } else if gates
            .iter()
            .any(|gate| gate.status == TaskVerificationGateStatus::Pending)
        {
            TaskVerificationGateStatus::Pending
        } else {
            TaskVerificationGateStatus::Passed
        };

        Ok(TaskVerificationGateReport { status, gates })
    }

    pub(crate) fn validate_observation_append(
        &self,
        observation_id: &TaskObservationId,
        kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
    ) -> Result<(), TaskStoreError> {
        if self.status != TaskStatus::Active {
            return Err(terminal_state_error(&self.id, self.status));
        }
        if self
            .observations
            .iter()
            .any(|observation| observation.id == *observation_id)
        {
            return Err(TaskStoreError::DuplicateObservation {
                task_id: self.id.clone(),
                observation_id: observation_id.clone(),
            });
        }
        validate_observation_parent(
            &self.id,
            &self.observations,
            observation_id,
            kind,
            parent_attempt_id,
        )
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
    DuplicateObservation {
        task_id: TaskId,
        observation_id: TaskObservationId,
    },
    AttemptCannotHaveParent {
        task_id: TaskId,
        observation_id: TaskObservationId,
        parent_observation_id: TaskObservationId,
    },
    ParentObservationNotFound {
        task_id: TaskId,
        parent_observation_id: TaskObservationId,
    },
    ParentObservationNotAttempt {
        task_id: TaskId,
        parent_observation_id: TaskObservationId,
        parent_kind: TaskObservationKind,
    },
    Session(SessionStoreError),
    InvalidStreamId {
        stream_id: String,
    },
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
            Self::DuplicateObservation {
                task_id,
                observation_id,
            } => write!(
                formatter,
                "task {task_id} already has observation {observation_id}"
            ),
            Self::AttemptCannotHaveParent {
                task_id,
                observation_id,
                parent_observation_id,
            } => write!(
                formatter,
                "task {task_id} attempt {observation_id} cannot have parent observation {parent_observation_id}"
            ),
            Self::ParentObservationNotFound {
                task_id,
                parent_observation_id,
            } => write!(
                formatter,
                "task {task_id} has no parent observation {parent_observation_id}"
            ),
            Self::ParentObservationNotAttempt {
                task_id,
                parent_observation_id,
                parent_kind,
            } => write!(
                formatter,
                "task {task_id} parent observation {parent_observation_id} has kind {parent_kind:?}, not attempt"
            ),
            Self::Session(error) => write!(formatter, "task session-store error: {error}"),
            Self::InvalidStreamId { stream_id } => {
                write!(formatter, "invalid task stream id {stream_id}")
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
            Self::Session(error) => Some(error),
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::AlreadyCompleted { .. }
            | Self::AlreadyCancelled { .. }
            | Self::AlreadyFailed { .. }
            | Self::SessionNotFound { .. }
            | Self::SessionClosed { .. }
            | Self::AlreadyAssociated { .. }
            | Self::DuplicateObservation { .. }
            | Self::AttemptCannotHaveParent { .. }
            | Self::ParentObservationNotFound { .. }
            | Self::ParentObservationNotAttempt { .. }
            | Self::InvalidStreamId { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

enum ObservationAppendMetadata {
    Legacy {
        kind: TaskObservationKind,
        parent_attempt_id: Option<TaskObservationId>,
    },
    IdentifiedVerification {
        outcome: TaskVerificationOutcome,
        check: TaskVerificationCheck,
        parent_attempt_id: TaskObservationId,
    },
}

impl ObservationAppendMetadata {
    fn into_parts(
        self,
    ) -> (
        TaskObservationKind,
        Option<TaskObservationId>,
        Option<TaskVerificationOutcome>,
        Option<TaskVerificationCheck>,
    ) {
        match self {
            Self::Legacy {
                kind,
                parent_attempt_id,
            } => (kind, parent_attempt_id, None, None),
            Self::IdentifiedVerification {
                outcome,
                check,
                parent_attempt_id,
            } => (
                TaskObservationKind::Verification,
                Some(parent_attempt_id),
                Some(outcome),
                Some(check),
            ),
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
                observations: Vec::new(),
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

    /// Completes an active task only when every caller-required gate passes for one exact attempt.
    ///
    /// A concurrent task-stream change causes the current state and gates to be evaluated again,
    /// so completion is never authorized from a stale green report.
    pub fn complete_if_verification_gates_pass(
        &mut self,
        id: &TaskId,
        output: TaskOutput,
        attempt_id: &TaskObservationId,
        gate_set: &TaskVerificationGateSet,
    ) -> Result<Task, TaskVerifiedCompletionError> {
        self.complete_if_verification_gates_pass_before_append(
            id,
            output,
            attempt_id,
            gate_set,
            &mut || {},
        )
    }

    fn complete_if_verification_gates_pass_before_append(
        &mut self,
        id: &TaskId,
        output: TaskOutput,
        attempt_id: &TaskObservationId,
        gate_set: &TaskVerificationGateSet,
        before_append: &mut impl FnMut(),
    ) -> Result<Task, TaskVerifiedCompletionError> {
        loop {
            let loaded = match self.load_versioned(id)? {
                Some(loaded) if loaded.task.status == TaskStatus::Active => loaded,
                Some(loaded) => {
                    return Err(terminal_state_error(id, loaded.task.status).into());
                }
                None => {
                    return Err(TaskStoreError::NotFound {
                        task_id: id.clone(),
                    }
                    .into());
                }
            };
            let report = loaded
                .task
                .evaluate_verification_gates(attempt_id, gate_set)?;
            match report.status() {
                TaskVerificationGateStatus::Pending => {
                    return Err(TaskVerifiedCompletionError::GatesPending { report });
                }
                TaskVerificationGateStatus::Failed => {
                    return Err(TaskVerifiedCompletionError::GatesFailed { report });
                }
                TaskVerificationGateStatus::Passed => {}
            }

            before_append();
            let event = TaskEvent::Completed {
                output: Some(output.clone()),
            };
            match self.event_log.append(
                &task_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                &event,
            ) {
                Ok(_) => {
                    return Ok(Task {
                        status: TaskStatus::Completed,
                        output: Some(output),
                        ..loaded.task
                    });
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(TaskStoreError::EventLog(error).into()),
            }
        }
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

    pub fn append_observation(
        &mut self,
        id: &TaskId,
        observation_id: TaskObservationId,
        kind: TaskObservationKind,
        text: TaskObservationText,
    ) -> Result<Task, TaskStoreError> {
        self.append_observation_with_parent(id, observation_id, kind, text, None)
    }

    /// Atomically appends one assistant turn and its matching task Attempt evidence.
    pub(crate) fn append_attempt_with_assistant_turn(
        &mut self,
        id: &TaskId,
        session_id: &SessionId,
        observation_id: TaskObservationId,
        text: TaskObservationText,
        assistant_content: SessionTurnContent,
    ) -> Result<(Session, Task), TaskStoreError> {
        self.append_observation_with_assistant_turn(
            id,
            session_id,
            observation_id,
            TaskObservationKind::Attempt,
            text,
            None,
            assistant_content,
        )
    }

    /// Atomically appends one assistant turn and its matching task Correction evidence.
    pub(crate) fn append_correction_with_assistant_turn(
        &mut self,
        id: &TaskId,
        session_id: &SessionId,
        observation_id: TaskObservationId,
        text: TaskObservationText,
        parent_attempt_id: TaskObservationId,
        assistant_content: SessionTurnContent,
    ) -> Result<(Session, Task), TaskStoreError> {
        self.append_observation_with_assistant_turn(
            id,
            session_id,
            observation_id,
            TaskObservationKind::Correction,
            text,
            Some(parent_attempt_id),
            assistant_content,
        )
    }

    /// Atomically appends one assistant turn and its matching task Diagnostic evidence.
    pub(crate) fn append_diagnostic_with_assistant_turn(
        &mut self,
        id: &TaskId,
        session_id: &SessionId,
        observation_id: TaskObservationId,
        text: TaskObservationText,
        parent_attempt_id: TaskObservationId,
        assistant_content: SessionTurnContent,
    ) -> Result<(Session, Task), TaskStoreError> {
        self.append_observation_with_assistant_turn(
            id,
            session_id,
            observation_id,
            TaskObservationKind::Diagnostic,
            text,
            Some(parent_attempt_id),
            assistant_content,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the atomic boundary retains both stream identities and complete evidence"
    )]
    fn append_observation_with_assistant_turn(
        &mut self,
        id: &TaskId,
        session_id: &SessionId,
        observation_id: TaskObservationId,
        kind: TaskObservationKind,
        text: TaskObservationText,
        parent_attempt_id: Option<TaskObservationId>,
        assistant_content: SessionTurnContent,
    ) -> Result<(Session, Task), TaskStoreError> {
        loop {
            let Some(mut loaded_task) = self.load_versioned(id)? else {
                return Err(TaskStoreError::NotFound {
                    task_id: id.clone(),
                });
            };
            loaded_task.task.validate_observation_append(
                &observation_id,
                kind,
                parent_attempt_id.as_ref(),
            )?;
            if loaded_task.task.session_id() != Some(session_id) {
                return Err(TaskStoreError::SessionNotFound {
                    session_id: session_id.clone(),
                });
            }
            let Some((mut session, session_version)) = self
                .sessions
                .load_with_version(session_id)
                .map_err(TaskStoreError::Session)?
            else {
                return Err(TaskStoreError::SessionNotFound {
                    session_id: session_id.clone(),
                });
            };
            if session.status() == SessionStatus::Closed {
                return Err(TaskStoreError::SessionClosed {
                    session_id: session_id.clone(),
                });
            }
            let session_event = SessionEvent::TurnAppended {
                role: SessionTurnRole::Assistant,
                content: assistant_content.clone(),
            };
            let task_event = TaskEvent::ObservationAppended {
                id: observation_id.clone(),
                kind,
                text: text.clone(),
                parent_attempt_id: parent_attempt_id.clone(),
                verification_outcome: None,
                verification_check: None,
            };
            match self.event_log.append_pair(
                &session_stream(session_id),
                ExpectedVersion::Exact(session_version),
                &session_event,
                &task_stream(id),
                ExpectedVersion::Exact(loaded_task.stream_version),
                &task_event,
            ) {
                Ok(_) => {
                    session.apply_appended_turn(SessionTurnRole::Assistant, assistant_content);
                    loaded_task.task.observations.push(TaskObservation {
                        id: observation_id,
                        kind,
                        text,
                        parent_attempt_id,
                        verification_outcome: None,
                        verification_check: None,
                    });
                    return Ok((session, loaded_task.task));
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(TaskStoreError::EventLog(error)),
            }
        }
    }

    /// Appends non-attempt evidence related to one earlier attempt in the same task.
    pub fn append_observation_for_attempt(
        &mut self,
        id: &TaskId,
        observation_id: TaskObservationId,
        kind: TaskObservationKind,
        text: TaskObservationText,
        parent_attempt_id: TaskObservationId,
    ) -> Result<Task, TaskStoreError> {
        self.append_observation_with_parent(id, observation_id, kind, text, Some(parent_attempt_id))
    }

    /// Appends an identified typed independent-check outcome for one earlier task attempt.
    pub fn append_verification_for_attempt(
        &mut self,
        id: &TaskId,
        observation_id: TaskObservationId,
        outcome: TaskVerificationOutcome,
        check: TaskVerificationCheck,
        text: TaskObservationText,
        parent_attempt_id: TaskObservationId,
    ) -> Result<Task, TaskStoreError> {
        self.append_observation_with_metadata(
            id,
            observation_id,
            text,
            ObservationAppendMetadata::IdentifiedVerification {
                outcome,
                check,
                parent_attempt_id,
            },
        )
    }

    fn append_observation_with_parent(
        &mut self,
        id: &TaskId,
        observation_id: TaskObservationId,
        kind: TaskObservationKind,
        text: TaskObservationText,
        parent_attempt_id: Option<TaskObservationId>,
    ) -> Result<Task, TaskStoreError> {
        self.append_observation_with_metadata(
            id,
            observation_id,
            text,
            ObservationAppendMetadata::Legacy {
                kind,
                parent_attempt_id,
            },
        )
    }

    fn append_observation_with_metadata(
        &mut self,
        id: &TaskId,
        observation_id: TaskObservationId,
        text: TaskObservationText,
        metadata: ObservationAppendMetadata,
    ) -> Result<Task, TaskStoreError> {
        let (kind, parent_attempt_id, verification_outcome, verification_check) =
            metadata.into_parts();
        loop {
            let Some(mut loaded) = self.load_versioned(id)? else {
                return Err(TaskStoreError::NotFound {
                    task_id: id.clone(),
                });
            };
            loaded.task.validate_observation_append(
                &observation_id,
                kind,
                parent_attempt_id.as_ref(),
            )?;

            match self.event_log.append(
                &task_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                &TaskEvent::ObservationAppended {
                    id: observation_id.clone(),
                    kind,
                    text: text.clone(),
                    parent_attempt_id: parent_attempt_id.clone(),
                    verification_outcome,
                    verification_check: verification_check.clone(),
                },
            ) {
                Ok(_) => {
                    loaded.task.observations.push(TaskObservation {
                        id: observation_id,
                        kind,
                        text,
                        parent_attempt_id,
                        verification_outcome,
                        verification_check,
                    });
                    return Ok(loaded.task);
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(TaskStoreError::EventLog(error)),
            }
        }
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

    pub(crate) fn load_with_version(
        &self,
        id: &TaskId,
    ) -> Result<Option<(Task, u64)>, TaskStoreError> {
        self.load_versioned(id)
            .map(|loaded| loaded.map(|loaded| (loaded.task, loaded.stream_version)))
    }

    /// Replays every persisted task in ascending ID order.
    pub fn list(&self) -> Result<Vec<Task>, TaskStoreError> {
        let streams = self
            .event_log
            .replay_streams_with_event_type::<TaskEvent>(TASK_STARTED_EVENT_TYPE)
            .map_err(TaskStoreError::Replay)?;
        let mut tasks = Vec::with_capacity(streams.len());

        for (stream_id, events) in streams {
            let Some(external_id) = stream_id.strip_prefix("task:") else {
                return Err(TaskStoreError::InvalidStreamId { stream_id });
            };
            let id = TaskId::new(external_id).map_err(|_| TaskStoreError::InvalidStreamId {
                stream_id: stream_id.clone(),
            })?;
            let Some(loaded) = Self::project_versioned(&id, events)? else {
                return Err(TaskStoreError::InvalidHistory { event_count: 0 });
            };
            tasks.push(loaded.task);
        }

        tasks.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(tasks)
    }

    /// Replays the tasks associated with an existing session in ascending ID order.
    pub fn list_for_session(&self, session_id: &SessionId) -> Result<Vec<Task>, TaskStoreError> {
        if self
            .sessions
            .load(session_id)
            .map_err(TaskStoreError::Session)?
            .is_none()
        {
            return Err(TaskStoreError::SessionNotFound {
                session_id: session_id.clone(),
            });
        }

        let associations = self
            .event_log
            .replay_event_type::<TaskEvent>(TASK_SESSION_ASSOCIATED_EVENT_TYPE)
            .map_err(TaskStoreError::Replay)?;
        let mut tasks = Vec::new();
        for (stream_id, association) in associations {
            let TaskEvent::SessionAssociated {
                task_id,
                session_id: associated_session_id,
            } = association
            else {
                unreachable!("the event query selects only task session associations")
            };
            if associated_session_id != *session_id {
                continue;
            }
            let Some(loaded) = self.load_versioned(&task_id)? else {
                return Err(TaskStoreError::InvalidHistory { event_count: 1 });
            };
            if stream_id != task_stream(&task_id).as_str() {
                return Err(TaskStoreError::InvalidHistory {
                    event_count: usize::try_from(loaded.stream_version).unwrap_or(usize::MAX),
                });
            }
            tasks.push(loaded.task);
        }
        tasks.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(tasks)
    }

    fn load_versioned(&self, id: &TaskId) -> Result<Option<VersionedTask>, TaskStoreError> {
        let events = self
            .event_log
            .replay::<TaskEvent>(&task_stream(id))
            .map_err(TaskStoreError::Replay)?;
        Self::project_versioned(id, events)
    }

    fn project_versioned(
        id: &TaskId,
        events: Vec<TaskEvent>,
    ) -> Result<Option<VersionedTask>, TaskStoreError> {
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
            observations: Vec::new(),
        };
        for event in &events[1..] {
            match event {
                TaskEvent::ObservationAppended {
                    id,
                    kind,
                    text,
                    parent_attempt_id,
                    verification_outcome,
                    verification_check,
                } if task.status == TaskStatus::Active
                    && !task.observations.iter().any(|item| item.id == *id)
                    && observation_parent_is_valid(
                        &task.observations,
                        id,
                        *kind,
                        parent_attempt_id.as_ref(),
                    ) =>
                {
                    task.observations.push(TaskObservation {
                        id: id.clone(),
                        kind: *kind,
                        text: text.clone(),
                        parent_attempt_id: parent_attempt_id.clone(),
                        verification_outcome: *verification_outcome,
                        verification_check: verification_check.clone(),
                    });
                }
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

fn observation_parent_is_valid(
    observations: &[TaskObservation],
    observation_id: &TaskObservationId,
    kind: TaskObservationKind,
    parent_attempt_id: Option<&TaskObservationId>,
) -> bool {
    if parent_attempt_id.is_some() && kind == TaskObservationKind::Attempt {
        return false;
    }
    parent_attempt_id.is_none_or(|parent_id| {
        parent_id != observation_id
            && observations.iter().any(|observation| {
                observation.id == *parent_id && observation.kind == TaskObservationKind::Attempt
            })
    })
}

fn validate_observation_parent(
    task_id: &TaskId,
    observations: &[TaskObservation],
    observation_id: &TaskObservationId,
    kind: TaskObservationKind,
    parent_attempt_id: Option<&TaskObservationId>,
) -> Result<(), TaskStoreError> {
    let Some(parent_id) = parent_attempt_id else {
        return Ok(());
    };
    if kind == TaskObservationKind::Attempt {
        return Err(TaskStoreError::AttemptCannotHaveParent {
            task_id: task_id.clone(),
            observation_id: observation_id.clone(),
            parent_observation_id: parent_id.clone(),
        });
    }
    let Some(parent) = observations
        .iter()
        .find(|observation| observation.id == *parent_id)
    else {
        return Err(TaskStoreError::ParentObservationNotFound {
            task_id: task_id.clone(),
            parent_observation_id: parent_id.clone(),
        });
    };
    if parent.kind != TaskObservationKind::Attempt {
        return Err(TaskStoreError::ParentObservationNotAttempt {
            task_id: task_id.clone(),
            parent_observation_id: parent_id.clone(),
            parent_kind: parent.kind,
        });
    }
    Ok(())
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

pub(crate) fn task_stream(id: &TaskId) -> StreamId {
    StreamId::new(format!("task:{id}")).expect("a prefixed task stream is never empty")
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum TaskEvent {
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
    ObservationAppended {
        id: TaskObservationId,
        kind: TaskObservationKind,
        text: TaskObservationText,
        parent_attempt_id: Option<TaskObservationId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        verification_outcome: Option<TaskVerificationOutcome>,
        #[serde(skip_serializing_if = "Option::is_none")]
        verification_check: Option<TaskVerificationCheck>,
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
            Self::ObservationAppended { .. } => TASK_OBSERVATION_APPENDED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        match self {
            Self::Completed { .. } => TASK_COMPLETED_PAYLOAD_VERSION,
            Self::Cancelled { .. } => TASK_CANCELLED_PAYLOAD_VERSION,
            Self::Failed { .. } => TASK_FAILED_PAYLOAD_VERSION,
            Self::ObservationAppended {
                verification_check: Some(_),
                ..
            } => TASK_IDENTIFIED_VERIFICATION_PAYLOAD_VERSION,
            Self::ObservationAppended {
                verification_outcome: Some(_),
                ..
            } => TASK_STRUCTURED_VERIFICATION_PAYLOAD_VERSION,
            Self::ObservationAppended { .. } => TASK_OBSERVATION_PAYLOAD_VERSION,
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
            TASK_OBSERVATION_APPENDED_EVENT_TYPE => matches!(
                payload_version,
                TASK_EVENT_PAYLOAD_VERSION
                    | TASK_OBSERVATION_PAYLOAD_VERSION
                    | TASK_STRUCTURED_VERIFICATION_PAYLOAD_VERSION
                    | TASK_IDENTIFIED_VERIFICATION_PAYLOAD_VERSION
            ),
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

        if event_type == TASK_OBSERVATION_APPENDED_EVENT_TYPE {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct LegacyPayload {
                id: String,
                kind: TaskObservationKind,
                text: String,
            }

            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {
                id: String,
                kind: TaskObservationKind,
                text: String,
                parent_attempt_id: Option<String>,
            }

            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct StructuredVerificationPayload {
                id: String,
                kind: TaskObservationKind,
                text: String,
                parent_attempt_id: String,
                verification_outcome: TaskVerificationOutcome,
            }

            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct IdentifiedVerificationPayload {
                id: String,
                kind: TaskObservationKind,
                text: String,
                parent_attempt_id: String,
                verification_outcome: TaskVerificationOutcome,
                verification_check: String,
            }

            let (id, kind, text, parent_attempt_id, verification_outcome, verification_check) =
                match payload_version {
                    TASK_EVENT_PAYLOAD_VERSION => {
                        let payload: LegacyPayload =
                            serde_json::from_slice(payload).map_err(|error| {
                                DecodeError::MalformedPayload {
                                    message: error.to_string(),
                                }
                            })?;
                        (payload.id, payload.kind, payload.text, None, None, None)
                    }
                    TASK_OBSERVATION_PAYLOAD_VERSION => {
                        let payload: Payload =
                            serde_json::from_slice(payload).map_err(|error| {
                                DecodeError::MalformedPayload {
                                    message: error.to_string(),
                                }
                            })?;
                        (
                            payload.id,
                            payload.kind,
                            payload.text,
                            payload.parent_attempt_id,
                            None,
                            None,
                        )
                    }
                    TASK_STRUCTURED_VERIFICATION_PAYLOAD_VERSION => {
                        let payload: StructuredVerificationPayload =
                            serde_json::from_slice(payload).map_err(|error| {
                                DecodeError::MalformedPayload {
                                    message: error.to_string(),
                                }
                            })?;
                        if payload.kind != TaskObservationKind::Verification {
                            return Err(DecodeError::MalformedPayload {
                                message:
                                    "structured verification outcome requires verification kind"
                                        .to_owned(),
                            });
                        }
                        (
                            payload.id,
                            payload.kind,
                            payload.text,
                            Some(payload.parent_attempt_id),
                            Some(payload.verification_outcome),
                            None,
                        )
                    }
                    TASK_IDENTIFIED_VERIFICATION_PAYLOAD_VERSION => {
                        let payload: IdentifiedVerificationPayload =
                            serde_json::from_slice(payload).map_err(|error| {
                                DecodeError::MalformedPayload {
                                    message: error.to_string(),
                                }
                            })?;
                        if payload.kind != TaskObservationKind::Verification {
                            return Err(DecodeError::MalformedPayload {
                                message: "identified verification requires verification kind"
                                    .to_owned(),
                            });
                        }
                        (
                            payload.id,
                            payload.kind,
                            payload.text,
                            Some(payload.parent_attempt_id),
                            Some(payload.verification_outcome),
                            Some(payload.verification_check),
                        )
                    }
                    _ => unreachable!("observation payload version was validated above"),
                };
            let id = TaskObservationId::new(id).map_err(|error| DecodeError::MalformedPayload {
                message: error.to_string(),
            })?;
            let text =
                TaskObservationText::new(text).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let parent_attempt_id = parent_attempt_id
                .map(TaskObservationId::new)
                .transpose()
                .map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let verification_check = verification_check
                .map(TaskVerificationCheck::new)
                .transpose()
                .map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            return Ok(Self::ObservationAppended {
                id,
                kind,
                text,
                parent_attempt_id,
                verification_outcome,
                verification_check,
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

#[cfg(test)]
mod verified_completion_concurrency_tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn re_evaluates_after_a_verification_write_wins_the_completion_race() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        let task_id = TaskId::new("guarded-completion-race").unwrap();
        let attempt_id = TaskObservationId::new("attempt").unwrap();
        let check = TaskVerificationCheck::new("quality").unwrap();
        let gates = TaskVerificationGateSet::new(vec![check.clone()]).unwrap();
        let mut store = TaskStore::open(&path).unwrap();
        store
            .start(task_id.clone(), TaskGoal::new("Keep gates fresh").unwrap())
            .unwrap();
        store
            .append_observation(
                &task_id,
                attempt_id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new("candidate").unwrap(),
            )
            .unwrap();
        store
            .append_verification_for_attempt(
                &task_id,
                TaskObservationId::new("passed").unwrap(),
                TaskVerificationOutcome::Passed,
                check.clone(),
                TaskObservationText::new("passed").unwrap(),
                attempt_id.clone(),
            )
            .unwrap();

        let mut racing_store = Some(TaskStore::open(&path).unwrap());
        let result = store.complete_if_verification_gates_pass_before_append(
            &task_id,
            TaskOutput::new("stale completion").unwrap(),
            &attempt_id,
            &gates,
            &mut || {
                if let Some(mut racing_store) = racing_store.take() {
                    racing_store
                        .append_verification_for_attempt(
                            &task_id,
                            TaskObservationId::new("failed").unwrap(),
                            TaskVerificationOutcome::Failed,
                            check.clone(),
                            TaskObservationText::new("failed after evaluation").unwrap(),
                            attempt_id.clone(),
                        )
                        .unwrap();
                }
            },
        );

        assert!(matches!(
            result,
            Err(TaskVerifiedCompletionError::GatesFailed { ref report })
                if report.status() == TaskVerificationGateStatus::Failed
        ));
        assert_eq!(
            store.load(&task_id).unwrap().unwrap().status(),
            TaskStatus::Active
        );
    }

    #[test]
    fn preserves_a_concurrent_cancellation_that_wins_the_completion_race() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        let task_id = TaskId::new("guarded-completion-terminal-race").unwrap();
        let attempt_id = TaskObservationId::new("attempt").unwrap();
        let check = TaskVerificationCheck::new("quality").unwrap();
        let gates = TaskVerificationGateSet::new(vec![check.clone()]).unwrap();
        let mut store = TaskStore::open(&path).unwrap();
        store
            .start(
                task_id.clone(),
                TaskGoal::new("Keep terminal state authoritative").unwrap(),
            )
            .unwrap();
        store
            .append_observation(
                &task_id,
                attempt_id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new("candidate").unwrap(),
            )
            .unwrap();
        store
            .append_verification_for_attempt(
                &task_id,
                TaskObservationId::new("passed").unwrap(),
                TaskVerificationOutcome::Passed,
                check,
                TaskObservationText::new("passed").unwrap(),
                attempt_id.clone(),
            )
            .unwrap();

        let mut racing_store = Some(TaskStore::open(&path).unwrap());
        let result = store.complete_if_verification_gates_pass_before_append(
            &task_id,
            TaskOutput::new("stale completion").unwrap(),
            &attempt_id,
            &gates,
            &mut || {
                if let Some(mut racing_store) = racing_store.take() {
                    racing_store
                        .cancel(
                            &task_id,
                            TaskCancellation::new("racing cancellation").unwrap(),
                        )
                        .unwrap();
                }
            },
        );

        assert!(matches!(
            result,
            Err(TaskVerifiedCompletionError::Store(
                TaskStoreError::AlreadyCancelled { .. }
            ))
        ));
        assert_eq!(
            store.load(&task_id).unwrap().unwrap().status(),
            TaskStatus::Cancelled
        );
    }
}
