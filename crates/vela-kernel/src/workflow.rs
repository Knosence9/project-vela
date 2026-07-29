use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};
use crate::task::{TaskId, TaskStatus, TaskStore, TaskStoreError, task_stream};

/// An opaque, non-blank stable identifier for one registered workflow.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkflowId(String);

impl WorkflowId {
    pub fn new(value: impl Into<String>) -> Result<Self, WorkflowIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(WorkflowIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkflowId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkflowIdError;

impl fmt::Display for WorkflowIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("workflow id must not be blank")
    }
}

impl Error for WorkflowIdError {}

/// One immutable transition in a registered inert workflow definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredWorkflowTransition {
    target: String,
    gate: Option<String>,
}

impl RegisteredWorkflowTransition {
    pub fn new(target: impl Into<String>, gate: Option<String>) -> Self {
        Self {
            target: target.into(),
            gate,
        }
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn gate(&self) -> Option<&str> {
        self.gate.as_deref()
    }
}

/// One immutable phase in a registered inert workflow definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredWorkflowPhase {
    id: String,
    terminal: bool,
    transitions: Vec<RegisteredWorkflowTransition>,
}

impl RegisteredWorkflowPhase {
    pub fn new(
        id: impl Into<String>,
        terminal: bool,
        transitions: Vec<RegisteredWorkflowTransition>,
    ) -> Self {
        Self {
            id: id.into(),
            terminal,
            transitions,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal
    }

    pub fn transitions(&self) -> &[RegisteredWorkflowTransition] {
        &self.transitions
    }
}

/// Immutable owned topology for one process-local registered workflow.
#[derive(Clone, Eq, PartialEq)]
pub struct RegisteredWorkflow {
    id: WorkflowId,
    start: String,
    phases: Vec<RegisteredWorkflowPhase>,
}

impl RegisteredWorkflow {
    pub fn new(
        id: WorkflowId,
        start: impl Into<String>,
        phases: Vec<RegisteredWorkflowPhase>,
    ) -> Self {
        Self {
            id,
            start: start.into(),
            phases,
        }
    }

    pub fn id(&self) -> &WorkflowId {
        &self.id
    }

    pub fn start(&self) -> &str {
        &self.start
    }

    pub fn phases(&self) -> &[RegisteredWorkflowPhase] {
        &self.phases
    }
}

impl fmt::Debug for RegisteredWorkflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegisteredWorkflow")
            .field("id", &self.id)
            .field("start", &self.start)
            .field("phases_len", &self.phases.len())
            .finish()
    }
}

/// A deterministic failure to begin or advance one borrowed workflow cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WorkflowCursorError {
    StartNotFound {
        phase_id: String,
    },
    StartAmbiguous {
        phase_id: String,
    },
    TerminalPhase {
        phase_id: String,
    },
    TransitionNotFound {
        phase_id: String,
        transition_index: usize,
    },
    GateAcknowledgementMissing {
        phase_id: String,
        transition_index: usize,
        gate_id: String,
    },
    GateAcknowledgementUnexpected {
        phase_id: String,
        transition_index: usize,
        gate_id: String,
    },
    GateAcknowledgementMismatch {
        phase_id: String,
        transition_index: usize,
        expected_gate_id: String,
        actual_gate_id: String,
    },
    TargetNotFound {
        phase_id: String,
        transition_index: usize,
        target_phase_id: String,
    },
    TargetAmbiguous {
        phase_id: String,
        transition_index: usize,
        target_phase_id: String,
    },
}

impl fmt::Display for WorkflowCursorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartNotFound { phase_id } => {
                write!(formatter, "workflow start phase {phase_id} was not found")
            }
            Self::StartAmbiguous { phase_id } => {
                write!(formatter, "workflow start phase {phase_id} is ambiguous")
            }
            Self::TerminalPhase { phase_id } => {
                write!(formatter, "workflow phase {phase_id} is terminal")
            }
            Self::TransitionNotFound {
                phase_id,
                transition_index,
            } => write!(
                formatter,
                "workflow phase {phase_id} has no transition at index {transition_index}"
            ),
            Self::GateAcknowledgementMissing {
                phase_id,
                transition_index,
                gate_id,
            } => write!(
                formatter,
                "workflow phase {phase_id} transition {transition_index} requires gate acknowledgement {gate_id}"
            ),
            Self::GateAcknowledgementUnexpected {
                phase_id,
                transition_index,
                gate_id,
            } => write!(
                formatter,
                "workflow phase {phase_id} transition {transition_index} does not accept gate acknowledgement {gate_id}"
            ),
            Self::GateAcknowledgementMismatch {
                phase_id,
                transition_index,
                expected_gate_id,
                actual_gate_id,
            } => write!(
                formatter,
                "workflow phase {phase_id} transition {transition_index} requires gate acknowledgement {expected_gate_id}, not {actual_gate_id}"
            ),
            Self::TargetNotFound {
                phase_id,
                transition_index,
                target_phase_id,
            } => write!(
                formatter,
                "workflow phase {phase_id} transition {transition_index} target {target_phase_id} was not found"
            ),
            Self::TargetAmbiguous {
                phase_id,
                transition_index,
                target_phase_id,
            } => write!(
                formatter,
                "workflow phase {phase_id} transition {transition_index} target {target_phase_id} is ambiguous"
            ),
        }
    }
}

impl Error for WorkflowCursorError {}

const WORKFLOW_RUN_STARTED_EVENT_TYPE: &str = "workflow_run.started";
const WORKFLOW_RUN_ADVANCED_EVENT_TYPE: &str = "workflow_run.advanced";
const WORKFLOW_RUN_CANCELLED_EVENT_TYPE: &str = "workflow_run.cancelled";
const WORKFLOW_RUN_PAUSED_EVENT_TYPE: &str = "workflow_run.paused";
const WORKFLOW_RUN_RESUMED_EVENT_TYPE: &str = "workflow_run.resumed";
const WORKFLOW_RUN_FAILED_EVENT_TYPE: &str = "workflow_run.failed";
const WORKFLOW_RUN_EVENT_PAYLOAD_VERSION: u32 = 1;
const WORKFLOW_RUN_TASK_STARTED_PAYLOAD_VERSION: u32 = 2;
const WORKFLOW_RUN_STREAM_PREFIX: &str = "workflow-run:";

/// An opaque, non-blank identity for one durable workflow run.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WorkflowRunId(String);

impl WorkflowRunId {
    pub fn new(value: impl Into<String>) -> Result<Self, WorkflowRunIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(WorkflowRunIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkflowRunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkflowRunIdError;

impl fmt::Display for WorkflowRunIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("workflow run id must not be blank")
    }
}

impl Error for WorkflowRunIdError {}

/// The non-empty caller-owned reason recorded when a workflow run is cancelled.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct WorkflowRunCancellation(String);

impl WorkflowRunCancellation {
    pub fn new(value: impl Into<String>) -> Result<Self, WorkflowRunCancellationError> {
        let value = value.into();
        if value.is_empty() {
            Err(WorkflowRunCancellationError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkflowRunCancellationError;

impl fmt::Display for WorkflowRunCancellationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("workflow run cancellation reason must not be empty")
    }
}

impl Error for WorkflowRunCancellationError {}

macro_rules! workflow_run_reason {
    ($reason:ident, $error:ident, $message:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $reason(String);

        impl $reason {
            pub fn new(value: impl Into<String>) -> Result<Self, $error> {
                let value = value.into();
                if value.is_empty() {
                    Err($error)
                } else {
                    Ok(Self(value))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $error;

        impl fmt::Display for $error {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str($message)
            }
        }

        impl Error for $error {}
    };
}

workflow_run_reason!(
    WorkflowRunPauseReason,
    WorkflowRunPauseReasonError,
    "workflow run pause reason must not be empty"
);
workflow_run_reason!(
    WorkflowRunResumeReason,
    WorkflowRunResumeReasonError,
    "workflow run resume reason must not be empty"
);
workflow_run_reason!(
    WorkflowRunFailure,
    WorkflowRunFailureError,
    "workflow run failure diagnostic must not be empty"
);

/// One semantic lifecycle event from a validated durable workflow-run history.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WorkflowRunHistoryEvent {
    Started {
        workflow_id: WorkflowId,
        phase_id: String,
    },
    TaskStarted {
        workflow_id: WorkflowId,
        phase_id: String,
        task_id: TaskId,
    },
    Advanced {
        source_phase_id: String,
        target_phase_id: String,
        transition_index: usize,
        gate_acknowledgement: Option<String>,
    },
    Cancelled {
        phase_id: String,
        reason: WorkflowRunCancellation,
    },
    Paused {
        phase_id: String,
        reason: WorkflowRunPauseReason,
    },
    Resumed {
        phase_id: String,
        reason: WorkflowRunResumeReason,
    },
    Failed {
        phase_id: String,
        failure: WorkflowRunFailure,
    },
}

/// One revision-bearing entry from a validated durable workflow-run history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowRunHistoryEntry {
    revision: u64,
    event: WorkflowRunHistoryEvent,
}

impl WorkflowRunHistoryEntry {
    /// The one-based event-stream revision occupied by this lifecycle event.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn event(&self) -> &WorkflowRunHistoryEvent {
        &self.event
    }
}

/// The mutually exclusive lifecycle classification of a validated workflow run.
///
/// Cancellation and failure take precedence over a retained pause marker. Exact
/// reasons and diagnostics remain available through [`WorkflowRun`] accessors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WorkflowRunStatus {
    Active,
    Paused,
    AuthoredTerminal,
    Cancelled,
    Failed,
}

/// Read-only state projected from one durable workflow-run event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowRun {
    id: WorkflowRunId,
    workflow: RegisteredWorkflow,
    current_phase_index: usize,
    revision: u64,
    cancellation: Option<WorkflowRunCancellation>,
    pause_reason: Option<WorkflowRunPauseReason>,
    failure: Option<WorkflowRunFailure>,
    task_id: Option<TaskId>,
}

impl WorkflowRun {
    pub fn id(&self) -> &WorkflowRunId {
        &self.id
    }

    pub fn workflow(&self) -> &RegisteredWorkflow {
        &self.workflow
    }

    /// The task immutably attributed when this run started, if any.
    pub fn task_id(&self) -> Option<&TaskId> {
        self.task_id.as_ref()
    }

    pub fn current_phase(&self) -> &RegisteredWorkflowPhase {
        &self.workflow.phases()[self.current_phase_index]
    }

    /// Classifies this validated projection without adding lifecycle evidence.
    pub fn status(&self) -> WorkflowRunStatus {
        if self.is_failed() {
            WorkflowRunStatus::Failed
        } else if self.is_cancelled() {
            WorkflowRunStatus::Cancelled
        } else if self.is_terminal() {
            WorkflowRunStatus::AuthoredTerminal
        } else if self.is_paused() {
            WorkflowRunStatus::Paused
        } else {
            WorkflowRunStatus::Active
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.current_phase().is_terminal()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_some()
    }

    pub fn cancellation(&self) -> Option<&WorkflowRunCancellation> {
        self.cancellation.as_ref()
    }

    pub fn is_paused(&self) -> bool {
        self.pause_reason.is_some()
    }

    pub fn pause_reason(&self) -> Option<&WorkflowRunPauseReason> {
        self.pause_reason.as_ref()
    }

    pub fn is_failed(&self) -> bool {
        self.failure.is_some()
    }

    pub fn failure(&self) -> Option<&WorkflowRunFailure> {
        self.failure.as_ref()
    }

    /// The exact event-stream revision this state projects.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum WorkflowRunStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    Task(TaskStoreError),
    AlreadyExists {
        run_id: WorkflowRunId,
    },
    NotFound {
        run_id: WorkflowRunId,
    },
    InvalidStreamId {
        stream_id: String,
    },
    AlreadyCancelled {
        run_id: WorkflowRunId,
    },
    AlreadyFailed {
        run_id: WorkflowRunId,
    },
    AlreadyPaused {
        run_id: WorkflowRunId,
    },
    NotPaused {
        run_id: WorkflowRunId,
    },
    Paused {
        run_id: WorkflowRunId,
    },
    AlreadyTerminal {
        run_id: WorkflowRunId,
    },
    InvalidDefinition {
        source: WorkflowCursorError,
    },
    InvalidTransition {
        source: WorkflowCursorError,
    },
    ConcurrentModification {
        expected_revision: u64,
        current_revision: u64,
    },
    InvalidHistory {
        event_count: usize,
    },
    TaskNotFound {
        task_id: TaskId,
    },
    TaskNotActive {
        task_id: TaskId,
        status: TaskStatus,
    },
    TaskChanged {
        task_id: TaskId,
    },
}

impl fmt::Display for WorkflowRunStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "workflow-run event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "workflow-run replay error: {error}"),
            Self::Task(error) => write!(formatter, "workflow-run task-store error: {error}"),
            Self::AlreadyExists { run_id } => {
                write!(formatter, "workflow run {run_id} already exists")
            }
            Self::NotFound { run_id } => write!(formatter, "workflow run {run_id} was not found"),
            Self::InvalidStreamId { stream_id } => {
                write!(formatter, "invalid workflow-run stream id {stream_id:?}")
            }
            Self::AlreadyCancelled { run_id } => {
                write!(formatter, "workflow run {run_id} is already cancelled")
            }
            Self::AlreadyFailed { run_id } => {
                write!(formatter, "workflow run {run_id} has already failed")
            }
            Self::AlreadyPaused { run_id } => {
                write!(formatter, "workflow run {run_id} is already paused")
            }
            Self::NotPaused { run_id } => write!(formatter, "workflow run {run_id} is not paused"),
            Self::Paused { run_id } => write!(formatter, "workflow run {run_id} is paused"),
            Self::AlreadyTerminal { run_id } => {
                write!(
                    formatter,
                    "workflow run {run_id} is already at a terminal phase"
                )
            }
            Self::InvalidDefinition { source } => {
                write!(formatter, "workflow run definition is invalid: {source}")
            }
            Self::InvalidTransition { source } => {
                write!(formatter, "workflow run transition is invalid: {source}")
            }
            Self::ConcurrentModification {
                expected_revision,
                current_revision,
            } => write!(
                formatter,
                "workflow run changed concurrently: expected revision {expected_revision}, current revision is {current_revision}"
            ),
            Self::InvalidHistory { event_count } => write!(
                formatter,
                "invalid workflow-run history with {event_count} events"
            ),
            Self::TaskNotFound { task_id } => write!(formatter, "task {task_id} was not found"),
            Self::TaskNotActive { task_id, status } => {
                write!(formatter, "task {task_id} is not active: {status:?}")
            }
            Self::TaskChanged { task_id } => {
                write!(
                    formatter,
                    "task {task_id} changed before workflow run start"
                )
            }
        }
    }
}

impl Error for WorkflowRunStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::InvalidDefinition { source } | Self::InvalidTransition { source } => Some(source),
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::InvalidStreamId { .. }
            | Self::AlreadyCancelled { .. }
            | Self::AlreadyFailed { .. }
            | Self::AlreadyPaused { .. }
            | Self::NotPaused { .. }
            | Self::Paused { .. }
            | Self::AlreadyTerminal { .. }
            | Self::ConcurrentModification { .. }
            | Self::InvalidHistory { .. }
            | Self::TaskNotFound { .. }
            | Self::TaskNotActive { .. }
            | Self::TaskChanged { .. } => None,
        }
    }
}

/// A synchronous durable workflow-run store backed by the typed event log.
pub struct WorkflowRunStore {
    event_log: EventLog,
    tasks: TaskStore,
}

impl WorkflowRunStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkflowRunStoreError> {
        let path = path.as_ref();
        let event_log = EventLog::open(path).map_err(WorkflowRunStoreError::EventLog)?;
        let tasks = TaskStore::open(path).map_err(WorkflowRunStoreError::Task)?;
        Ok(Self { event_log, tasks })
    }

    pub fn start(
        &mut self,
        id: WorkflowRunId,
        workflow: &RegisteredWorkflow,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let current_phase_index = start_phase_index(workflow)
            .map_err(|source| WorkflowRunStoreError::InvalidDefinition { source })?;
        let event = WorkflowRunEvent::Started {
            workflow: WorkflowSnapshot::from(workflow),
            task_id: None,
        };
        match self
            .event_log
            .append(&workflow_run_stream(&id), ExpectedVersion::NoStream, &event)
        {
            Ok(_) => Ok(WorkflowRun {
                id,
                workflow: workflow.clone(),
                current_phase_index,
                revision: 1,
                cancellation: None,
                pause_reason: None,
                failure: None,
                task_id: None,
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(WorkflowRunStoreError::AlreadyExists { run_id: id }),
            Err(error) => Err(WorkflowRunStoreError::EventLog(error)),
        }
    }

    /// Starts one run with immutable attribution to an existing active task.
    pub fn start_for_task(
        &mut self,
        id: WorkflowRunId,
        task_id: &TaskId,
        workflow: &RegisteredWorkflow,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        start_phase_index(workflow)
            .map_err(|source| WorkflowRunStoreError::InvalidDefinition { source })?;
        if self.load(&id)?.is_some() {
            return Err(WorkflowRunStoreError::AlreadyExists { run_id: id });
        }
        let Some((task, task_version)) = self
            .tasks
            .load_with_version(task_id)
            .map_err(WorkflowRunStoreError::Task)?
        else {
            return Err(WorkflowRunStoreError::TaskNotFound {
                task_id: task_id.clone(),
            });
        };
        if task.status() != TaskStatus::Active {
            return Err(WorkflowRunStoreError::TaskNotActive {
                task_id: task_id.clone(),
                status: task.status(),
            });
        }
        self.start_for_task_at_version(id, task_id, task_version, workflow)
    }

    fn start_for_task_at_version(
        &mut self,
        id: WorkflowRunId,
        task_id: &TaskId,
        task_version: u64,
        workflow: &RegisteredWorkflow,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let current_phase_index = start_phase_index(workflow)
            .map_err(|source| WorkflowRunStoreError::InvalidDefinition { source })?;
        let event = WorkflowRunEvent::Started {
            workflow: WorkflowSnapshot::from(workflow),
            task_id: Some(task_id.clone()),
        };
        match self.event_log.append_if_stream_unchanged(
            &workflow_run_stream(&id),
            ExpectedVersion::NoStream,
            &task_stream(task_id),
            ExpectedVersion::Exact(task_version),
            &event,
        ) {
            Ok(_) => Ok(WorkflowRun {
                id,
                workflow: workflow.clone(),
                current_phase_index,
                revision: 1,
                cancellation: None,
                pause_reason: None,
                failure: None,
                task_id: Some(task_id.clone()),
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(WorkflowRunStoreError::AlreadyExists { run_id: id }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::Exact(_),
                ..
            }) => Err(WorkflowRunStoreError::TaskChanged {
                task_id: task_id.clone(),
            }),
            Err(error) => Err(WorkflowRunStoreError::EventLog(error)),
        }
    }

    /// Advances one exact projected revision through a caller-selected authored edge.
    ///
    /// Stale revisions fail without retry so phase-relative intent is never reinterpreted.
    pub fn advance(
        &mut self,
        id: &WorkflowRunId,
        expected_revision: u64,
        transition_index: usize,
        gate_acknowledgement: Option<&str>,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let Some(mut run) = self.load(id)? else {
            return Err(WorkflowRunStoreError::NotFound { run_id: id.clone() });
        };
        if run.revision != expected_revision {
            return Err(WorkflowRunStoreError::ConcurrentModification {
                expected_revision,
                current_revision: run.revision,
            });
        }
        if run.is_cancelled() {
            return Err(WorkflowRunStoreError::AlreadyCancelled { run_id: id.clone() });
        }
        if run.is_failed() {
            return Err(WorkflowRunStoreError::AlreadyFailed { run_id: id.clone() });
        }
        if run.is_paused() {
            return Err(WorkflowRunStoreError::Paused { run_id: id.clone() });
        }
        let source_phase_index = run.current_phase_index;
        let target_phase_index =
            resolve_transition(&run, transition_index, gate_acknowledgement)
                .map_err(|source| WorkflowRunStoreError::InvalidTransition { source })?;
        let event = WorkflowRunEvent::Advanced {
            source_phase_index,
            transition_index,
            target_phase_index,
            gate_acknowledgement: gate_acknowledgement.map(str::to_owned),
        };
        match self.event_log.append(
            &workflow_run_stream(id),
            ExpectedVersion::Exact(expected_revision),
            &event,
        ) {
            Ok(revision) => {
                run.current_phase_index = target_phase_index;
                run.revision = revision;
                Ok(run)
            }
            Err(EventLogError::WrongExpectedVersion {
                current: Some(current_revision),
                ..
            }) => Err(WorkflowRunStoreError::ConcurrentModification {
                expected_revision,
                current_revision,
            }),
            Err(error) => Err(WorkflowRunStoreError::EventLog(error)),
        }
    }

    /// Pauses one exact projected revision without changing its phase or topology.
    pub fn pause(
        &mut self,
        id: &WorkflowRunId,
        expected_revision: u64,
        reason: WorkflowRunPauseReason,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let Some(mut run) = self.load(id)? else {
            return Err(WorkflowRunStoreError::NotFound { run_id: id.clone() });
        };
        validate_revision(&run, expected_revision)?;
        if run.is_cancelled() {
            return Err(WorkflowRunStoreError::AlreadyCancelled { run_id: id.clone() });
        }
        if run.is_failed() {
            return Err(WorkflowRunStoreError::AlreadyFailed { run_id: id.clone() });
        }
        if run.is_terminal() {
            return Err(WorkflowRunStoreError::AlreadyTerminal { run_id: id.clone() });
        }
        if run.is_paused() {
            return Err(WorkflowRunStoreError::AlreadyPaused { run_id: id.clone() });
        }
        let event = WorkflowRunEvent::Paused {
            source_phase_index: run.current_phase_index,
            reason: reason.clone(),
        };
        let revision = append_run_event(&mut self.event_log, id, expected_revision, &event)?;
        run.revision = revision;
        run.pause_reason = Some(reason);
        Ok(run)
    }

    /// Resumes one exact projected revision without changing its phase or topology.
    pub fn resume(
        &mut self,
        id: &WorkflowRunId,
        expected_revision: u64,
        reason: WorkflowRunResumeReason,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let Some(mut run) = self.load(id)? else {
            return Err(WorkflowRunStoreError::NotFound { run_id: id.clone() });
        };
        validate_revision(&run, expected_revision)?;
        if run.is_cancelled() {
            return Err(WorkflowRunStoreError::AlreadyCancelled { run_id: id.clone() });
        }
        if run.is_failed() {
            return Err(WorkflowRunStoreError::AlreadyFailed { run_id: id.clone() });
        }
        if !run.is_paused() {
            return Err(WorkflowRunStoreError::NotPaused { run_id: id.clone() });
        }
        let event = WorkflowRunEvent::Resumed {
            source_phase_index: run.current_phase_index,
            reason,
        };
        let revision = append_run_event(&mut self.event_log, id, expected_revision, &event)?;
        run.revision = revision;
        run.pause_reason = None;
        Ok(run)
    }

    /// Cancels one exact projected revision without changing its phase or topology.
    pub fn cancel(
        &mut self,
        id: &WorkflowRunId,
        expected_revision: u64,
        cancellation: WorkflowRunCancellation,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let Some(mut run) = self.load(id)? else {
            return Err(WorkflowRunStoreError::NotFound { run_id: id.clone() });
        };
        if run.revision != expected_revision {
            return Err(WorkflowRunStoreError::ConcurrentModification {
                expected_revision,
                current_revision: run.revision,
            });
        }
        if run.is_cancelled() {
            return Err(WorkflowRunStoreError::AlreadyCancelled { run_id: id.clone() });
        }
        if run.is_failed() {
            return Err(WorkflowRunStoreError::AlreadyFailed { run_id: id.clone() });
        }
        if run.is_terminal() {
            return Err(WorkflowRunStoreError::AlreadyTerminal { run_id: id.clone() });
        }
        let event = WorkflowRunEvent::Cancelled {
            source_phase_index: run.current_phase_index,
            reason: cancellation.clone(),
        };
        match self.event_log.append(
            &workflow_run_stream(id),
            ExpectedVersion::Exact(expected_revision),
            &event,
        ) {
            Ok(revision) => {
                run.revision = revision;
                run.cancellation = Some(cancellation);
                Ok(run)
            }
            Err(EventLogError::WrongExpectedVersion {
                current: Some(current_revision),
                ..
            }) => Err(WorkflowRunStoreError::ConcurrentModification {
                expected_revision,
                current_revision,
            }),
            Err(error) => Err(WorkflowRunStoreError::EventLog(error)),
        }
    }

    /// Fails one exact projected revision without changing its phase or topology.
    pub fn fail(
        &mut self,
        id: &WorkflowRunId,
        expected_revision: u64,
        failure: WorkflowRunFailure,
    ) -> Result<WorkflowRun, WorkflowRunStoreError> {
        let Some(mut run) = self.load(id)? else {
            return Err(WorkflowRunStoreError::NotFound { run_id: id.clone() });
        };
        validate_revision(&run, expected_revision)?;
        if run.is_failed() {
            return Err(WorkflowRunStoreError::AlreadyFailed { run_id: id.clone() });
        }
        if run.is_cancelled() {
            return Err(WorkflowRunStoreError::AlreadyCancelled { run_id: id.clone() });
        }
        if run.is_terminal() {
            return Err(WorkflowRunStoreError::AlreadyTerminal { run_id: id.clone() });
        }
        let event = WorkflowRunEvent::Failed {
            source_phase_index: run.current_phase_index,
            failure: failure.clone(),
        };
        let revision = append_run_event(&mut self.event_log, id, expected_revision, &event)?;
        run.revision = revision;
        run.failure = Some(failure);
        Ok(run)
    }

    pub fn load(&self, id: &WorkflowRunId) -> Result<Option<WorkflowRun>, WorkflowRunStoreError> {
        let events = self
            .event_log
            .replay::<WorkflowRunEvent>(&workflow_run_stream(id))
            .map_err(WorkflowRunStoreError::Replay)?;
        project_workflow_run(id, events)
    }

    /// Returns one exact run's validated semantic lifecycle history in revision order.
    pub fn history(
        &self,
        id: &WorkflowRunId,
    ) -> Result<Option<Vec<WorkflowRunHistoryEntry>>, WorkflowRunStoreError> {
        let events = self
            .event_log
            .replay::<WorkflowRunEvent>(&workflow_run_stream(id))
            .map_err(WorkflowRunStoreError::Replay)?;
        let Some(run) = project_workflow_run(id, events.clone())? else {
            return Ok(None);
        };
        Ok(Some(project_workflow_run_history(&run, events)))
    }

    /// Replays every persisted workflow run in ascending run-ID order.
    pub fn list(&self) -> Result<Vec<WorkflowRun>, WorkflowRunStoreError> {
        let streams = self
            .event_log
            .replay_streams_with_event_type::<WorkflowRunEvent>(WORKFLOW_RUN_STARTED_EVENT_TYPE)
            .map_err(WorkflowRunStoreError::Replay)?;
        let mut runs = Vec::with_capacity(streams.len());

        for (stream_id, events) in streams {
            let Some(external_id) = stream_id.strip_prefix(WORKFLOW_RUN_STREAM_PREFIX) else {
                return Err(WorkflowRunStoreError::InvalidStreamId { stream_id });
            };
            let id = WorkflowRunId::new(external_id).map_err(|_| {
                WorkflowRunStoreError::InvalidStreamId {
                    stream_id: stream_id.clone(),
                }
            })?;
            let Some(run) = project_workflow_run(&id, events)? else {
                return Err(WorkflowRunStoreError::InvalidHistory { event_count: 0 });
            };
            runs.push(run);
        }

        runs.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(runs)
    }

    /// Replays runs attributed to one exact task in ascending run-ID order.
    ///
    /// This historical query does not require the task to exist or remain active.
    pub fn list_for_task(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<WorkflowRun>, WorkflowRunStoreError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|run| run.task_id() == Some(task_id))
            .collect())
    }
}

fn validate_revision(
    run: &WorkflowRun,
    expected_revision: u64,
) -> Result<(), WorkflowRunStoreError> {
    if run.revision == expected_revision {
        Ok(())
    } else {
        Err(WorkflowRunStoreError::ConcurrentModification {
            expected_revision,
            current_revision: run.revision,
        })
    }
}

fn append_run_event(
    event_log: &mut EventLog,
    id: &WorkflowRunId,
    expected_revision: u64,
    event: &WorkflowRunEvent,
) -> Result<u64, WorkflowRunStoreError> {
    match event_log.append(
        &workflow_run_stream(id),
        ExpectedVersion::Exact(expected_revision),
        event,
    ) {
        Ok(revision) => Ok(revision),
        Err(EventLogError::WrongExpectedVersion {
            current: Some(current_revision),
            ..
        }) => Err(WorkflowRunStoreError::ConcurrentModification {
            expected_revision,
            current_revision,
        }),
        Err(error) => Err(WorkflowRunStoreError::EventLog(error)),
    }
}

fn project_workflow_run(
    id: &WorkflowRunId,
    events: Vec<WorkflowRunEvent>,
) -> Result<Option<WorkflowRun>, WorkflowRunStoreError> {
    let event_count = events.len();
    let Some(WorkflowRunEvent::Started { workflow, task_id }) = events.first().cloned() else {
        return if events.is_empty() {
            Ok(None)
        } else {
            Err(WorkflowRunStoreError::InvalidHistory { event_count })
        };
    };
    let workflow = workflow.into_workflow().map_err(|message| {
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            message,
        })
    })?;
    let current_phase_index = start_phase_index(&workflow).map_err(|error| {
        WorkflowRunStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            message: error.to_string(),
        })
    })?;
    let mut run = WorkflowRun {
        id: id.clone(),
        workflow,
        current_phase_index,
        revision: 1,
        cancellation: None,
        pause_reason: None,
        failure: None,
        task_id,
    };
    for event in events.into_iter().skip(1) {
        if run.is_cancelled() || run.is_failed() {
            return Err(WorkflowRunStoreError::InvalidHistory { event_count });
        }
        match event {
            WorkflowRunEvent::Advanced {
                source_phase_index,
                transition_index,
                target_phase_index,
                gate_acknowledgement,
            } => {
                if run.is_paused()
                    || source_phase_index != run.current_phase_index
                    || resolve_transition(&run, transition_index, gate_acknowledgement.as_deref())
                        .ok()
                        != Some(target_phase_index)
                {
                    return Err(WorkflowRunStoreError::InvalidHistory { event_count });
                }
                run.current_phase_index = target_phase_index;
            }
            WorkflowRunEvent::Cancelled {
                source_phase_index,
                reason,
            } => {
                if source_phase_index != run.current_phase_index || run.is_terminal() {
                    return Err(WorkflowRunStoreError::InvalidHistory { event_count });
                }
                run.cancellation = Some(reason);
            }
            WorkflowRunEvent::Paused {
                source_phase_index,
                reason,
            } => {
                if source_phase_index != run.current_phase_index
                    || run.is_terminal()
                    || run.is_paused()
                {
                    return Err(WorkflowRunStoreError::InvalidHistory { event_count });
                }
                run.pause_reason = Some(reason);
            }
            WorkflowRunEvent::Resumed {
                source_phase_index,
                reason: _,
            } => {
                if source_phase_index != run.current_phase_index || !run.is_paused() {
                    return Err(WorkflowRunStoreError::InvalidHistory { event_count });
                }
                run.pause_reason = None;
            }
            WorkflowRunEvent::Failed {
                source_phase_index,
                failure,
            } => {
                if source_phase_index != run.current_phase_index || run.is_terminal() {
                    return Err(WorkflowRunStoreError::InvalidHistory { event_count });
                }
                run.failure = Some(failure);
            }
            WorkflowRunEvent::Started { .. } => {
                return Err(WorkflowRunStoreError::InvalidHistory { event_count });
            }
        }
        run.revision += 1;
    }
    Ok(Some(run))
}

fn project_workflow_run_history(
    run: &WorkflowRun,
    events: Vec<WorkflowRunEvent>,
) -> Vec<WorkflowRunHistoryEntry> {
    events
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            let event = match event {
                WorkflowRunEvent::Started { task_id, .. } => {
                    let phase_index = start_phase_index(run.workflow())
                        .expect("the workflow-run projector validated the start phase");
                    let workflow_id = run.workflow().id().clone();
                    let phase_id = run.workflow().phases()[phase_index].id().to_owned();
                    match task_id {
                        Some(task_id) => WorkflowRunHistoryEvent::TaskStarted {
                            workflow_id,
                            phase_id,
                            task_id,
                        },
                        None => WorkflowRunHistoryEvent::Started {
                            workflow_id,
                            phase_id,
                        },
                    }
                }
                WorkflowRunEvent::Advanced {
                    source_phase_index,
                    transition_index,
                    target_phase_index,
                    gate_acknowledgement,
                } => WorkflowRunHistoryEvent::Advanced {
                    source_phase_id: run.workflow().phases()[source_phase_index].id().to_owned(),
                    target_phase_id: run.workflow().phases()[target_phase_index].id().to_owned(),
                    transition_index,
                    gate_acknowledgement,
                },
                WorkflowRunEvent::Cancelled {
                    source_phase_index,
                    reason,
                } => WorkflowRunHistoryEvent::Cancelled {
                    phase_id: run.workflow().phases()[source_phase_index].id().to_owned(),
                    reason,
                },
                WorkflowRunEvent::Paused {
                    source_phase_index,
                    reason,
                } => WorkflowRunHistoryEvent::Paused {
                    phase_id: run.workflow().phases()[source_phase_index].id().to_owned(),
                    reason,
                },
                WorkflowRunEvent::Resumed {
                    source_phase_index,
                    reason,
                } => WorkflowRunHistoryEvent::Resumed {
                    phase_id: run.workflow().phases()[source_phase_index].id().to_owned(),
                    reason,
                },
                WorkflowRunEvent::Failed {
                    source_phase_index,
                    failure,
                } => WorkflowRunHistoryEvent::Failed {
                    phase_id: run.workflow().phases()[source_phase_index].id().to_owned(),
                    failure,
                },
            };
            WorkflowRunHistoryEntry {
                revision: u64::try_from(index + 1)
                    .expect("an in-memory event count fits the persisted revision type"),
                event,
            }
        })
        .collect()
}

fn workflow_run_stream(id: &WorkflowRunId) -> StreamId {
    StreamId::new(format!("{WORKFLOW_RUN_STREAM_PREFIX}{id}"))
        .expect("a prefixed workflow-run stream is never empty")
}

fn start_phase_index(workflow: &RegisteredWorkflow) -> Result<usize, WorkflowCursorError> {
    let phase_id = WorkflowCursor::new(workflow)?.current_phase().id();
    Ok(workflow
        .phases()
        .iter()
        .position(|phase| phase.id() == phase_id)
        .expect("the validated start phase exists"))
}

fn resolve_transition(
    run: &WorkflowRun,
    transition_index: usize,
    gate_acknowledgement: Option<&str>,
) -> Result<usize, WorkflowCursorError> {
    let mut cursor = WorkflowCursor {
        workflow: &run.workflow,
        current_phase_index: run.current_phase_index,
    };
    cursor.advance(transition_index, gate_acknowledgement)?;
    Ok(cursor.current_phase_index)
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum WorkflowRunEvent {
    Started {
        workflow: WorkflowSnapshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<TaskId>,
    },
    Advanced {
        source_phase_index: usize,
        transition_index: usize,
        target_phase_index: usize,
        gate_acknowledgement: Option<String>,
    },
    Cancelled {
        source_phase_index: usize,
        reason: WorkflowRunCancellation,
    },
    Paused {
        source_phase_index: usize,
        reason: WorkflowRunPauseReason,
    },
    Resumed {
        source_phase_index: usize,
        reason: WorkflowRunResumeReason,
    },
    Failed {
        source_phase_index: usize,
        failure: WorkflowRunFailure,
    },
}

impl Event for WorkflowRunEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Started { .. } => WORKFLOW_RUN_STARTED_EVENT_TYPE,
            Self::Advanced { .. } => WORKFLOW_RUN_ADVANCED_EVENT_TYPE,
            Self::Cancelled { .. } => WORKFLOW_RUN_CANCELLED_EVENT_TYPE,
            Self::Paused { .. } => WORKFLOW_RUN_PAUSED_EVENT_TYPE,
            Self::Resumed { .. } => WORKFLOW_RUN_RESUMED_EVENT_TYPE,
            Self::Failed { .. } => WORKFLOW_RUN_FAILED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        match self {
            Self::Started {
                task_id: Some(_), ..
            } => WORKFLOW_RUN_TASK_STARTED_PAYLOAD_VERSION,
            _ => WORKFLOW_RUN_EVENT_PAYLOAD_VERSION,
        }
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if payload_version != WORKFLOW_RUN_EVENT_PAYLOAD_VERSION
            && !(event_type == WORKFLOW_RUN_STARTED_EVENT_TYPE
                && payload_version == WORKFLOW_RUN_TASK_STARTED_PAYLOAD_VERSION)
        {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }
        match event_type {
            WORKFLOW_RUN_STARTED_EVENT_TYPE => {
                let (workflow, task_id) = if payload_version
                    == WORKFLOW_RUN_TASK_STARTED_PAYLOAD_VERSION
                {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Payload {
                        workflow: WorkflowSnapshot,
                        task_id: String,
                    }
                    let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                        DecodeError::MalformedPayload {
                            message: error.to_string(),
                        }
                    })?;
                    let task_id = TaskId::new(payload.task_id).map_err(|error| {
                        DecodeError::MalformedPayload {
                            message: error.to_string(),
                        }
                    })?;
                    (payload.workflow, Some(task_id))
                } else {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Payload {
                        workflow: WorkflowSnapshot,
                    }
                    let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                        DecodeError::MalformedPayload {
                            message: error.to_string(),
                        }
                    })?;
                    (payload.workflow, None)
                };
                let decoded_workflow = workflow
                    .clone()
                    .into_workflow()
                    .map_err(|message| DecodeError::MalformedPayload { message })?;
                start_phase_index(&decoded_workflow).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Started { workflow, task_id })
            }
            WORKFLOW_RUN_ADVANCED_EVENT_TYPE => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    source_phase_index: usize,
                    transition_index: usize,
                    target_phase_index: usize,
                    gate_acknowledgement: Option<String>,
                }
                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Advanced {
                    source_phase_index: payload.source_phase_index,
                    transition_index: payload.transition_index,
                    target_phase_index: payload.target_phase_index,
                    gate_acknowledgement: payload.gate_acknowledgement,
                })
            }
            WORKFLOW_RUN_CANCELLED_EVENT_TYPE => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    source_phase_index: usize,
                    reason: String,
                }
                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let reason = WorkflowRunCancellation::new(payload.reason).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Cancelled {
                    source_phase_index: payload.source_phase_index,
                    reason,
                })
            }
            WORKFLOW_RUN_PAUSED_EVENT_TYPE => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    source_phase_index: usize,
                    reason: String,
                }
                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let reason = WorkflowRunPauseReason::new(payload.reason).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Paused {
                    source_phase_index: payload.source_phase_index,
                    reason,
                })
            }
            WORKFLOW_RUN_RESUMED_EVENT_TYPE => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    source_phase_index: usize,
                    reason: String,
                }
                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let reason = WorkflowRunResumeReason::new(payload.reason).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Resumed {
                    source_phase_index: payload.source_phase_index,
                    reason,
                })
            }
            WORKFLOW_RUN_FAILED_EVENT_TYPE => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Payload {
                    source_phase_index: usize,
                    failure: String,
                }
                let payload: Payload = serde_json::from_slice(payload).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                let failure = WorkflowRunFailure::new(payload.failure).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Failed {
                    source_phase_index: payload.source_phase_index,
                    failure,
                })
            }
            _ => Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkflowSnapshot {
    id: String,
    start: String,
    phases: Vec<WorkflowPhaseSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkflowPhaseSnapshot {
    id: String,
    terminal: bool,
    transitions: Vec<WorkflowTransitionSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkflowTransitionSnapshot {
    target: String,
    gate: Option<String>,
}

impl From<&RegisteredWorkflow> for WorkflowSnapshot {
    fn from(workflow: &RegisteredWorkflow) -> Self {
        Self {
            id: workflow.id().as_str().to_owned(),
            start: workflow.start().to_owned(),
            phases: workflow
                .phases()
                .iter()
                .map(|phase| WorkflowPhaseSnapshot {
                    id: phase.id().to_owned(),
                    terminal: phase.is_terminal(),
                    transitions: phase
                        .transitions()
                        .iter()
                        .map(|transition| WorkflowTransitionSnapshot {
                            target: transition.target().to_owned(),
                            gate: transition.gate().map(str::to_owned),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl WorkflowSnapshot {
    fn into_workflow(self) -> Result<RegisteredWorkflow, String> {
        let id = WorkflowId::new(self.id).map_err(|error| error.to_string())?;
        Ok(RegisteredWorkflow::new(
            id,
            self.start,
            self.phases
                .into_iter()
                .map(|phase| {
                    RegisteredWorkflowPhase::new(
                        phase.id,
                        phase.terminal,
                        phase
                            .transitions
                            .into_iter()
                            .map(|transition| {
                                RegisteredWorkflowTransition::new(
                                    transition.target,
                                    transition.gate,
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        ))
    }
}

/// A borrowed, process-local cursor advanced only by explicit caller choices.
#[derive(Debug)]
pub struct WorkflowCursor<'a> {
    workflow: &'a RegisteredWorkflow,
    current_phase_index: usize,
}

impl<'a> WorkflowCursor<'a> {
    /// Begins at the workflow's exact declared start phase without advancing it.
    pub fn new(workflow: &'a RegisteredWorkflow) -> Result<Self, WorkflowCursorError> {
        let mut matches = workflow
            .phases()
            .iter()
            .enumerate()
            .filter(|(_, phase)| phase.id() == workflow.start());
        let current_phase_index = matches.next().map(|(index, _)| index).ok_or_else(|| {
            WorkflowCursorError::StartNotFound {
                phase_id: workflow.start().to_owned(),
            }
        })?;
        if matches.next().is_some() {
            return Err(WorkflowCursorError::StartAmbiguous {
                phase_id: workflow.start().to_owned(),
            });
        }
        Ok(Self {
            workflow,
            current_phase_index,
        })
    }

    pub fn workflow(&self) -> &'a RegisteredWorkflow {
        self.workflow
    }

    pub fn current_phase(&self) -> &'a RegisteredWorkflowPhase {
        &self.workflow.phases()[self.current_phase_index]
    }

    pub fn is_terminal(&self) -> bool {
        self.current_phase().is_terminal()
    }

    /// Advances through one authored transition after exact gate acknowledgement.
    ///
    /// Every failure is checked before the cursor's current phase changes.
    pub fn advance(
        &mut self,
        transition_index: usize,
        gate_acknowledgement: Option<&str>,
    ) -> Result<(), WorkflowCursorError> {
        let phase = self.current_phase();
        if phase.is_terminal() {
            return Err(WorkflowCursorError::TerminalPhase {
                phase_id: phase.id().to_owned(),
            });
        }
        let transition = phase.transitions().get(transition_index).ok_or_else(|| {
            WorkflowCursorError::TransitionNotFound {
                phase_id: phase.id().to_owned(),
                transition_index,
            }
        })?;

        match (transition.gate(), gate_acknowledgement) {
            (Some(gate_id), None) => {
                return Err(WorkflowCursorError::GateAcknowledgementMissing {
                    phase_id: phase.id().to_owned(),
                    transition_index,
                    gate_id: gate_id.to_owned(),
                });
            }
            (None, Some(gate_id)) => {
                return Err(WorkflowCursorError::GateAcknowledgementUnexpected {
                    phase_id: phase.id().to_owned(),
                    transition_index,
                    gate_id: gate_id.to_owned(),
                });
            }
            (Some(expected_gate_id), Some(actual_gate_id))
                if expected_gate_id != actual_gate_id =>
            {
                return Err(WorkflowCursorError::GateAcknowledgementMismatch {
                    phase_id: phase.id().to_owned(),
                    transition_index,
                    expected_gate_id: expected_gate_id.to_owned(),
                    actual_gate_id: actual_gate_id.to_owned(),
                });
            }
            (Some(_), Some(_)) | (None, None) => {}
        }

        let target_phase_id = transition.target();
        let mut targets = self
            .workflow
            .phases()
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.id() == target_phase_id);
        let target_index = targets.next().map(|(index, _)| index).ok_or_else(|| {
            WorkflowCursorError::TargetNotFound {
                phase_id: phase.id().to_owned(),
                transition_index,
                target_phase_id: target_phase_id.to_owned(),
            }
        })?;
        if targets.next().is_some() {
            return Err(WorkflowCursorError::TargetAmbiguous {
                phase_id: phase.id().to_owned(),
                transition_index,
                target_phase_id: target_phase_id.to_owned(),
            });
        }

        self.current_phase_index = target_index;
        Ok(())
    }
}

/// A duplicate exact workflow identity rejected during atomic registration.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WorkflowRegistryError {
    DuplicateId { workflow_id: WorkflowId },
}

impl fmt::Display for WorkflowRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { workflow_id } => {
                write!(formatter, "workflow {workflow_id} is already registered")
            }
        }
    }
}

impl Error for WorkflowRegistryError {}

/// A caller-owned, process-local deterministic directory of inert workflow definitions.
#[derive(Debug, Default)]
pub struct WorkflowRegistry {
    workflows: BTreeMap<WorkflowId, RegisteredWorkflow>,
}

impl WorkflowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one homogeneous batch atomically without replacing existing definitions.
    pub fn register_all<I>(&mut self, workflows: I) -> Result<(), WorkflowRegistryError>
    where
        I: IntoIterator<Item = RegisteredWorkflow>,
    {
        let workflows = workflows.into_iter().collect::<Vec<_>>();
        let mut batch_ids = BTreeSet::new();
        let mut collisions = BTreeSet::new();
        for workflow in &workflows {
            let workflow_id = workflow.id().clone();
            if self.workflows.contains_key(&workflow_id) || !batch_ids.insert(workflow_id.clone()) {
                collisions.insert(workflow_id);
            }
        }
        if let Some(workflow_id) = collisions.into_iter().next() {
            return Err(WorkflowRegistryError::DuplicateId { workflow_id });
        }
        for workflow in workflows {
            self.workflows.insert(workflow.id().clone(), workflow);
        }
        Ok(())
    }

    pub fn get(&self, workflow_id: &WorkflowId) -> Option<&RegisteredWorkflow> {
        self.workflows.get(workflow_id)
    }

    /// Iterates registered workflows in ascending exact-ID order.
    pub fn workflows(&self) -> impl Iterator<Item = &RegisteredWorkflow> {
        self.workflows.values()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::task::{TaskGoal, TaskOutput};

    #[test]
    fn task_revision_race_rejects_attributed_start_without_creating_a_run() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        let task_id = TaskId::new("racing-task").unwrap();
        let run_id = WorkflowRunId::new("racing-run").unwrap();
        let mut tasks = TaskStore::open(&path).unwrap();
        tasks
            .start(task_id.clone(), TaskGoal::new("race start").unwrap())
            .unwrap();
        let mut runs = WorkflowRunStore::open(&path).unwrap();
        let (_, observed_version) = runs.tasks.load_with_version(&task_id).unwrap().unwrap();
        tasks
            .complete(&task_id, TaskOutput::new("won race").unwrap())
            .unwrap();
        let workflow = RegisteredWorkflow::new(
            WorkflowId::new("race.workflow").unwrap(),
            "done",
            vec![RegisteredWorkflowPhase::new("done", true, vec![])],
        );

        assert!(matches!(
            runs.start_for_task_at_version(run_id.clone(), &task_id, observed_version, &workflow,),
            Err(WorkflowRunStoreError::TaskChanged { .. })
        ));
        assert!(runs.load(&run_id).unwrap().is_none());
    }
}
