use std::{error::Error, fmt, path::Path};

use crate::session::{
    Session, SessionId, SessionStatus, SessionStore, SessionStoreError, SessionTurn,
    SessionTurnContent, SessionTurnRole,
};
use crate::task::{
    Task, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText,
    TaskObservationTextError, TaskOutput, TaskOutputError, TaskStatus, TaskStore, TaskStoreError,
};

/// A synchronous, provider-neutral source for one assistant response.
pub trait AssistantProvider {
    /// Produces one assistant turn from the complete durable conversation.
    fn complete(&mut self, transcript: &[SessionTurn])
    -> Result<SessionTurnContent, ProviderError>;
}

/// A provider failure that preserves the provider-specific error as its source.
#[derive(Debug)]
pub struct ProviderError {
    source: Box<dyn Error>,
}

impl ProviderError {
    pub fn new(error: impl Error + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for ProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// A failure before, during, or after one provider invocation.
#[derive(Debug)]
#[non_exhaustive]
pub enum RuntimeError {
    Session(SessionStoreError),
    Provider(ProviderError),
    Task(TaskStoreError),
    TaskNotAssociated { task_id: TaskId },
    InvalidAttemptText(TaskObservationTextError),
    InvalidTaskOutput(TaskOutputError),
    InvalidCorrectionText(TaskObservationTextError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "assistant runtime session error: {error}"),
            Self::Provider(error) => write!(formatter, "assistant provider error: {error}"),
            Self::Task(error) => write!(formatter, "assistant runtime task error: {error}"),
            Self::TaskNotAssociated { task_id } => {
                write!(formatter, "task {task_id} is not associated with a session")
            }
            Self::InvalidAttemptText(error) => {
                write!(formatter, "assistant attempt observation error: {error}")
            }
            Self::InvalidTaskOutput(error) => {
                write!(formatter, "assistant task output error: {error}")
            }
            Self::InvalidCorrectionText(error) => {
                write!(formatter, "assistant correction observation error: {error}")
            }
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Provider(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::InvalidAttemptText(error) => Some(error),
            Self::InvalidTaskOutput(error) => Some(error),
            Self::InvalidCorrectionText(error) => Some(error),
            Self::TaskNotAssociated { .. } => None,
        }
    }
}

/// The durable session and task projections after one task-associated turn.
#[derive(Debug)]
pub struct TaskTurnOutcome {
    session: Session,
    task: Task,
}

impl TaskTurnOutcome {
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn task(&self) -> &Task {
        &self.task
    }
}

/// Synchronous orchestration for one tool-free assistant turn.
pub struct AssistantRuntime<P> {
    sessions: SessionStore,
    tasks: TaskStore,
    provider: P,
}

impl<P: AssistantProvider> AssistantRuntime<P> {
    pub fn open(path: impl AsRef<Path>, provider: P) -> Result<Self, RuntimeError> {
        let path = path.as_ref();
        let sessions = SessionStore::open(path).map_err(RuntimeError::Session)?;
        let tasks = TaskStore::open(path).map_err(RuntimeError::Task)?;
        Ok(Self {
            sessions,
            tasks,
            provider,
        })
    }

    /// Durably appends the human turn, invokes the provider, then durably appends its response.
    ///
    /// Provider failure leaves the human turn committed and appends no assistant turn. The
    /// runtime does not retry provider calls because they may have external effects.
    pub fn execute_turn(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
    ) -> Result<Session, RuntimeError> {
        let session = self
            .sessions
            .append_turn(session_id, SessionTurnRole::Human, human_content)
            .map_err(RuntimeError::Session)?;
        let assistant_content = self
            .provider
            .complete(session.turns())
            .map_err(RuntimeError::Provider)?;
        self.sessions
            .append_turn(session_id, SessionTurnRole::Assistant, assistant_content)
            .map_err(RuntimeError::Session)
    }

    /// Executes one turn for an active task and records the assistant response as an attempt.
    ///
    /// The task must already be associated with a session. Transcript writes commit before the
    /// task observation, so an observation failure does not roll back either conversation turn.
    pub fn execute_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        observation_id: TaskObservationId,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (_, session_id) = self.load_active_associated_task(task_id)?;

        let session = self.execute_turn(&session_id, human_content)?;
        let assistant_content = session
            .turns()
            .last()
            .expect("a successful assistant turn has a final turn")
            .content();
        let attempt_text = TaskObservationText::new(assistant_content.as_str())
            .map_err(RuntimeError::InvalidAttemptText)?;
        let task = self
            .tasks
            .append_observation(
                task_id,
                observation_id,
                TaskObservationKind::Attempt,
                attempt_text,
            )
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes one turn and records its response as a correction to an earlier attempt.
    ///
    /// Deterministic task evidence errors are rejected before transcript persistence. A racing
    /// task change can still make the authoritative append fail after both turns commit.
    pub fn execute_task_correction_turn(
        &mut self,
        task_id: &TaskId,
        parent_attempt_id: &TaskObservationId,
        human_content: SessionTurnContent,
        correction_observation_id: TaskObservationId,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (task, session_id) = self.load_active_associated_task(task_id)?;
        task.validate_observation_append(
            &correction_observation_id,
            TaskObservationKind::Correction,
            Some(parent_attempt_id),
        )
        .map_err(RuntimeError::Task)?;

        let session = self.execute_turn(&session_id, human_content)?;
        let assistant_content = session
            .turns()
            .last()
            .expect("a successful assistant turn has a final turn")
            .content();
        let correction_text = TaskObservationText::new(assistant_content.as_str())
            .map_err(RuntimeError::InvalidCorrectionText)?;
        let task = self
            .tasks
            .append_observation_for_attempt(
                task_id,
                correction_observation_id,
                TaskObservationKind::Correction,
                correction_text,
                parent_attempt_id.clone(),
            )
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes a caller-requested final turn, records its response as an attempt, and completes
    /// the task with that same response.
    ///
    /// Deterministic task, observation, and session constraints are checked before the provider
    /// call. Transcript turns, the attempt, and completion then commit in that order, so a later
    /// failure preserves every earlier commit. The response is task output, not verification.
    pub fn complete_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (task, session_id) = self.load_active_associated_task(task_id)?;
        task.validate_observation_append(
            &attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
        )
        .map_err(RuntimeError::Task)?;
        self.ensure_session_writable(&session_id)?;

        let session = self.execute_turn(&session_id, human_content)?;
        let assistant_content = session
            .turns()
            .last()
            .expect("a successful assistant turn has a final turn")
            .content();
        let attempt_text = TaskObservationText::new(assistant_content.as_str())
            .map_err(RuntimeError::InvalidAttemptText)?;
        let output =
            TaskOutput::new(assistant_content.as_str()).map_err(RuntimeError::InvalidTaskOutput)?;
        self.tasks
            .append_observation(
                task_id,
                attempt_observation_id,
                TaskObservationKind::Attempt,
                attempt_text,
            )
            .map_err(RuntimeError::Task)?;
        let task = self
            .tasks
            .complete(task_id, output)
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    fn ensure_session_writable(&self, session_id: &SessionId) -> Result<(), RuntimeError> {
        let session = self
            .sessions
            .load(session_id)
            .map_err(RuntimeError::Session)?
            .ok_or_else(|| {
                RuntimeError::Session(SessionStoreError::NotFound {
                    session_id: session_id.clone(),
                })
            })?;
        if session.status() == SessionStatus::Closed {
            return Err(RuntimeError::Session(SessionStoreError::SessionClosed {
                session_id: session_id.clone(),
            }));
        }
        Ok(())
    }

    fn load_active_associated_task(
        &self,
        task_id: &TaskId,
    ) -> Result<(Task, SessionId), RuntimeError> {
        let task = self
            .tasks
            .load(task_id)
            .map_err(RuntimeError::Task)?
            .ok_or_else(|| {
                RuntimeError::Task(TaskStoreError::NotFound {
                    task_id: task_id.clone(),
                })
            })?;
        match task.status() {
            TaskStatus::Active => {}
            TaskStatus::Completed => {
                return Err(RuntimeError::Task(TaskStoreError::AlreadyCompleted {
                    task_id: task_id.clone(),
                }));
            }
            TaskStatus::Cancelled => {
                return Err(RuntimeError::Task(TaskStoreError::AlreadyCancelled {
                    task_id: task_id.clone(),
                }));
            }
            TaskStatus::Failed => {
                return Err(RuntimeError::Task(TaskStoreError::AlreadyFailed {
                    task_id: task_id.clone(),
                }));
            }
        }
        let session_id =
            task.session_id()
                .cloned()
                .ok_or_else(|| RuntimeError::TaskNotAssociated {
                    task_id: task_id.clone(),
                })?;
        Ok((task, session_id))
    }
}
