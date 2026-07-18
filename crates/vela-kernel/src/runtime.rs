use std::{error::Error, fmt, path::Path};

use crate::session::{
    Session, SessionId, SessionStore, SessionStoreError, SessionTurn, SessionTurnContent,
    SessionTurnRole,
};
use crate::task::{
    Task, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText,
    TaskObservationTextError, TaskStatus, TaskStore, TaskStoreError,
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
}
