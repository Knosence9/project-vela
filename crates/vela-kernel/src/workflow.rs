use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
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
