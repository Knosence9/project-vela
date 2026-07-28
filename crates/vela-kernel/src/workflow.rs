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
