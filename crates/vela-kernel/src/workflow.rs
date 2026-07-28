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
const WORKFLOW_RUN_EVENT_PAYLOAD_VERSION: u32 = 1;

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

/// Read-only state projected from one durable workflow-run event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowRun {
    id: WorkflowRunId,
    workflow: RegisteredWorkflow,
    current_phase_index: usize,
    revision: u64,
    cancellation: Option<WorkflowRunCancellation>,
}

impl WorkflowRun {
    pub fn id(&self) -> &WorkflowRunId {
        &self.id
    }

    pub fn workflow(&self) -> &RegisteredWorkflow {
        &self.workflow
    }

    pub fn current_phase(&self) -> &RegisteredWorkflowPhase {
        &self.workflow.phases()[self.current_phase_index]
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
    AlreadyExists {
        run_id: WorkflowRunId,
    },
    NotFound {
        run_id: WorkflowRunId,
    },
    AlreadyCancelled {
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
}

impl fmt::Display for WorkflowRunStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "workflow-run event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "workflow-run replay error: {error}"),
            Self::AlreadyExists { run_id } => {
                write!(formatter, "workflow run {run_id} already exists")
            }
            Self::NotFound { run_id } => write!(formatter, "workflow run {run_id} was not found"),
            Self::AlreadyCancelled { run_id } => {
                write!(formatter, "workflow run {run_id} is already cancelled")
            }
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
        }
    }
}

impl Error for WorkflowRunStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::InvalidDefinition { source } | Self::InvalidTransition { source } => Some(source),
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::AlreadyCancelled { .. }
            | Self::AlreadyTerminal { .. }
            | Self::ConcurrentModification { .. }
            | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous durable workflow-run store backed by the typed event log.
pub struct WorkflowRunStore {
    event_log: EventLog,
}

impl WorkflowRunStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkflowRunStoreError> {
        EventLog::open(path)
            .map(|event_log| Self { event_log })
            .map_err(WorkflowRunStoreError::EventLog)
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
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(WorkflowRunStoreError::AlreadyExists { run_id: id }),
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

    pub fn load(&self, id: &WorkflowRunId) -> Result<Option<WorkflowRun>, WorkflowRunStoreError> {
        let events = self
            .event_log
            .replay::<WorkflowRunEvent>(&workflow_run_stream(id))
            .map_err(WorkflowRunStoreError::Replay)?;
        let event_count = events.len();
        let Some(WorkflowRunEvent::Started { workflow }) = events.first().cloned() else {
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
        };
        for event in events.into_iter().skip(1) {
            if run.is_cancelled() {
                return Err(WorkflowRunStoreError::InvalidHistory { event_count });
            }
            match event {
                WorkflowRunEvent::Advanced {
                    source_phase_index,
                    transition_index,
                    target_phase_index,
                    gate_acknowledgement,
                } => {
                    if source_phase_index != run.current_phase_index
                        || resolve_transition(
                            &run,
                            transition_index,
                            gate_acknowledgement.as_deref(),
                        )
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
                WorkflowRunEvent::Started { .. } => {
                    return Err(WorkflowRunStoreError::InvalidHistory { event_count });
                }
            }
            run.revision += 1;
        }
        Ok(Some(run))
    }
}

fn workflow_run_stream(id: &WorkflowRunId) -> StreamId {
    StreamId::new(format!("workflow-run:{id}"))
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
}

impl Event for WorkflowRunEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Started { .. } => WORKFLOW_RUN_STARTED_EVENT_TYPE,
            Self::Advanced { .. } => WORKFLOW_RUN_ADVANCED_EVENT_TYPE,
            Self::Cancelled { .. } => WORKFLOW_RUN_CANCELLED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        WORKFLOW_RUN_EVENT_PAYLOAD_VERSION
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if payload_version != WORKFLOW_RUN_EVENT_PAYLOAD_VERSION {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }
        match event_type {
            WORKFLOW_RUN_STARTED_EVENT_TYPE => {
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
                let workflow = payload
                    .workflow
                    .clone()
                    .into_workflow()
                    .map_err(|message| DecodeError::MalformedPayload { message })?;
                start_phase_index(&workflow).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
                Ok(Self::Started {
                    workflow: payload.workflow,
                })
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
