use std::{error::Error, fmt, path::Path};

use serde_json::Value;

use crate::session::{
    Session, SessionId, SessionStatus, SessionStore, SessionStoreError, SessionTurn,
    SessionTurnContent, SessionTurnRole,
};
use crate::task::{
    Task, TaskCancellation, TaskFailure, TaskId, TaskObservationId, TaskObservationKind,
    TaskObservationText, TaskObservationTextError, TaskOutput, TaskOutputError, TaskStatus,
    TaskStore, TaskStoreError,
};
use crate::tool::{
    DurableToolInvocationError, DurableToolRegistryInvocationError, ToolAuthorizer, ToolId,
    ToolInvocationId, ToolInvocationStore, ToolInvocationStoreError, ToolMetadata, ToolRegistry,
};

/// A synchronous, provider-neutral source for one assistant response.
pub trait AssistantProvider {
    /// Produces one assistant turn from the complete durable conversation.
    fn complete(&mut self, transcript: &[SessionTurn])
    -> Result<SessionTurnContent, ProviderError>;
}

/// A synchronous provider that may request at most one tool invocation.
///
/// Registry metadata is descriptive only. A provider cannot authorize an invocation or choose
/// its trusted durable identity.
pub trait ToolAssistantProvider {
    fn complete_with_tools(
        &mut self,
        transcript: &[SessionTurn],
        tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError>;
}

/// A synchronous provider continuation after one successful in-memory tool result.
///
/// This additive boundary does not grant permission, choose durable invocation identity, or
/// persist the exact tool result.
pub trait ToolAssistantContinuationProvider {
    fn complete_after_tool(
        &mut self,
        continuation: ProviderToolContinuation<'_>,
        tools: &[ToolMetadata],
    ) -> Result<ProviderToolResponse, ProviderError>;
}

/// One provider response at the bounded tool-step boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProviderToolResponse {
    Final(SessionTurnContent),
    ToolRequest { tool_id: ToolId, input: Value },
}

/// Whether the caller has a final response or must explicitly continue with the provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolStepContinuation {
    Complete,
    ProviderRequired,
}

/// The in-memory result of one successful provider tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProviderToolStepOutcome {
    Final {
        content: SessionTurnContent,
        continuation: ToolStepContinuation,
    },
    ToolCompleted {
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
    },
}

impl ProviderToolStepOutcome {
    /// Borrows the exact in-memory result needed for an explicit provider continuation.
    pub fn tool_result(&self) -> Option<ProviderToolResult<'_>> {
        match self {
            Self::ToolCompleted {
                tool_id,
                input,
                output,
                ..
            } => Some(ProviderToolResult {
                tool_id,
                input,
                output,
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed exact tool result supplied only to an explicit provider continuation.
#[derive(Clone, Copy, Debug)]
pub struct ProviderToolResult<'a> {
    tool_id: &'a ToolId,
    input: &'a Value,
    output: &'a Value,
}

impl<'a> ProviderToolResult<'a> {
    pub fn tool_id(self) -> &'a ToolId {
        self.tool_id
    }

    pub fn input(self) -> &'a Value {
        self.input
    }

    pub fn output(self) -> &'a Value {
        self.output
    }
}

/// Caller-supplied provider context for one explicit continuation.
#[derive(Clone, Copy, Debug)]
pub struct ProviderToolContinuation<'a> {
    transcript: &'a [SessionTurn],
    prior_result: ProviderToolResult<'a>,
}

impl<'a> ProviderToolContinuation<'a> {
    pub fn new(transcript: &'a [SessionTurn], prior_result: ProviderToolResult<'a>) -> Self {
        Self {
            transcript,
            prior_result,
        }
    }

    pub fn transcript(self) -> &'a [SessionTurn] {
        self.transcript
    }

    pub fn prior_result(self) -> ProviderToolResult<'a> {
        self.prior_result
    }
}

/// A provider failure or the existing durable registry invocation failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProviderToolStepError {
    Provider(ProviderError),
    Invocation(DurableToolRegistryInvocationError),
}

impl fmt::Display for ProviderToolStepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider(error) => write!(formatter, "assistant provider error: {error}"),
            Self::Invocation(error) => write!(formatter, "assistant tool step error: {error}"),
        }
    }
}

impl Error for ProviderToolStepError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Provider(error) => Some(error),
            Self::Invocation(error) => Some(error),
        }
    }
}

/// Calls a tool-capable provider once and dispatches at most one durable task tool invocation.
///
/// This operation never persists provider content, exact tool input/output, or a continuation
/// transcript. It never retries or calls the provider a second time. The caller owns the trusted
/// invocation identity, task identity, registry, durable store, and one-invocation authorizer.
pub fn execute_provider_tool_step<P: ToolAssistantProvider, A: ToolAuthorizer>(
    provider: &mut P,
    transcript: &[SessionTurn],
    registry: &mut ToolRegistry,
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    authorizer: &mut A,
) -> Result<ProviderToolStepOutcome, ProviderToolStepError> {
    let response = provider
        .complete_with_tools(transcript, &registry.metadata())
        .map_err(ProviderToolStepError::Provider)?;

    dispatch_provider_tool_response(
        response,
        registry,
        store,
        task_id,
        invocation_id,
        authorizer,
    )
}

/// Calls a continuation provider once with one prior tool result and dispatches at most one new
/// durable task tool invocation.
///
/// Every continuation is caller-owned and explicit. This operation does not persist the prior
/// result, retry, or call the provider a second time.
pub fn continue_provider_tool_step<P: ToolAssistantContinuationProvider, A: ToolAuthorizer>(
    provider: &mut P,
    continuation: ProviderToolContinuation<'_>,
    registry: &mut ToolRegistry,
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    authorizer: &mut A,
) -> Result<ProviderToolStepOutcome, ProviderToolStepError> {
    let response = provider
        .complete_after_tool(continuation, &registry.metadata())
        .map_err(ProviderToolStepError::Provider)?;

    dispatch_provider_tool_response(
        response,
        registry,
        store,
        task_id,
        invocation_id,
        authorizer,
    )
}

fn dispatch_provider_tool_response<A: ToolAuthorizer>(
    response: ProviderToolResponse,
    registry: &mut ToolRegistry,
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    authorizer: &mut A,
) -> Result<ProviderToolStepOutcome, ProviderToolStepError> {
    match response {
        ProviderToolResponse::Final(content) => Ok(ProviderToolStepOutcome::Final {
            content,
            continuation: ToolStepContinuation::Complete,
        }),
        ProviderToolResponse::ToolRequest { tool_id, input } => {
            let output = registry
                .invoke_for_task_durable(
                    store,
                    task_id,
                    invocation_id,
                    &tool_id,
                    authorizer,
                    &input,
                )
                .map_err(ProviderToolStepError::Invocation)?;
            Ok(ProviderToolStepOutcome::ToolCompleted {
                tool_id,
                input,
                output,
                continuation: ToolStepContinuation::ProviderRequired,
            })
        }
    }
}

/// The durable result of one initial tool-capable task turn.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskTurnOutcome {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
    },
}

impl ToolTaskTurnOutcome {
    /// Borrows the exact in-memory result needed for an explicit provider continuation.
    pub fn tool_result(&self) -> Option<ProviderToolResult<'_>> {
        match self {
            Self::ToolCompleted {
                tool_id,
                input,
                output,
                ..
            } => Some(ProviderToolResult {
                tool_id,
                input,
                output,
            }),
            Self::Final { .. } => None,
        }
    }

    /// Borrows the durable transcript and exact result for an explicit continuation.
    pub fn continuation(&self) -> Option<ToolTaskContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                ..
            } => Some(ToolTaskContinuation {
                task_id,
                provider: ProviderToolContinuation::new(
                    session.turns(),
                    ProviderToolResult {
                        tool_id,
                        input,
                        output,
                    },
                ),
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed continuation bound to the task that owns the initial tool step.
#[derive(Clone, Copy, Debug)]
pub struct ToolTaskContinuation<'a> {
    task_id: &'a TaskId,
    provider: ProviderToolContinuation<'a>,
}

impl ToolTaskContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }
}

/// A failure while bridging one initial provider-tool step into a durable task turn.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskRuntimeError {
    Session(SessionStoreError),
    Task(TaskStoreError),
    InvocationStore(ToolInvocationStoreError),
    TaskNotAssociated { task_id: TaskId },
    InvalidAttemptText(TaskObservationTextError),
    Provider(ProviderError),
    Invocation(DurableToolRegistryInvocationError),
}

impl fmt::Display for ToolTaskRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "tool task runtime session error: {error}"),
            Self::Task(error) => write!(formatter, "tool task runtime task error: {error}"),
            Self::InvocationStore(error) => {
                write!(formatter, "tool invocation store error: {error}")
            }
            Self::TaskNotAssociated { task_id } => {
                write!(formatter, "task {task_id} is not associated with a session")
            }
            Self::InvalidAttemptText(error) => {
                write!(formatter, "tool task attempt observation error: {error}")
            }
            Self::Provider(error) => write!(formatter, "assistant provider error: {error}"),
            Self::Invocation(error) => write!(formatter, "assistant tool step error: {error}"),
        }
    }
}

impl Error for ToolTaskRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::InvocationStore(error) => Some(error),
            Self::InvalidAttemptText(error) => Some(error),
            Self::Provider(error) => Some(error),
            Self::Invocation(error) => Some(error),
            Self::TaskNotAssociated { .. } => None,
        }
    }
}

/// Synchronous orchestration for one initial tool-capable task turn.
pub struct ToolAssistantRuntime<P> {
    sessions: SessionStore,
    tasks: TaskStore,
    invocations: ToolInvocationStore,
    provider: P,
}

impl<P: ToolAssistantProvider> ToolAssistantRuntime<P> {
    pub fn open(path: impl AsRef<Path>, provider: P) -> Result<Self, ToolTaskRuntimeError> {
        let path = path.as_ref();
        Ok(Self {
            sessions: SessionStore::open(path).map_err(ToolTaskRuntimeError::Session)?,
            tasks: TaskStore::open(path).map_err(ToolTaskRuntimeError::Task)?,
            invocations: ToolInvocationStore::open(path)
                .map_err(ToolTaskRuntimeError::InvocationStore)?,
            provider,
        })
    }

    /// Appends one human turn and executes exactly one provider/tool step.
    ///
    /// Final content is persisted as an assistant turn and Attempt. A completed tool request
    /// remains in memory and requires an explicit caller-owned provider continuation.
    pub fn execute_task_turn<A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskTurnOutcome, ToolTaskRuntimeError> {
        let task = self.load_active_task(task_id)?;
        let session_id =
            task.session_id()
                .cloned()
                .ok_or_else(|| ToolTaskRuntimeError::TaskNotAssociated {
                    task_id: task_id.clone(),
                })?;
        task.validate_observation_append(
            &attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
        )
        .map_err(ToolTaskRuntimeError::Task)?;
        self.ensure_session_writable(&session_id)?;
        self.ensure_invocation_available(&invocation_id)?;

        let session = self
            .sessions
            .append_turn(&session_id, SessionTurnRole::Human, human_content)
            .map_err(ToolTaskRuntimeError::Session)?;
        let step = execute_provider_tool_step(
            &mut self.provider,
            session.turns(),
            registry,
            &mut self.invocations,
            task_id,
            invocation_id,
            authorizer,
        )
        .map_err(|error| match error {
            ProviderToolStepError::Provider(error) => ToolTaskRuntimeError::Provider(error),
            ProviderToolStepError::Invocation(error) => ToolTaskRuntimeError::Invocation(error),
        })?;

        match step {
            ProviderToolStepOutcome::Final { content, .. } => {
                let attempt_text = content.as_str().to_owned();
                let session = self
                    .sessions
                    .append_turn(&session_id, SessionTurnRole::Assistant, content)
                    .map_err(ToolTaskRuntimeError::Session)?;
                let attempt_text = TaskObservationText::new(attempt_text)
                    .map_err(ToolTaskRuntimeError::InvalidAttemptText)?;
                let task = self
                    .tasks
                    .append_observation(
                        task_id,
                        attempt_observation_id,
                        TaskObservationKind::Attempt,
                        attempt_text,
                    )
                    .map_err(ToolTaskRuntimeError::Task)?;
                Ok(ToolTaskTurnOutcome::Final { session, task })
            }
            ProviderToolStepOutcome::ToolCompleted {
                tool_id,
                input,
                output,
                continuation,
            } => Ok(ToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id: task_id.clone(),
                tool_id,
                input,
                output,
                continuation,
            }),
        }
    }

    fn load_active_task(&self, task_id: &TaskId) -> Result<Task, ToolTaskRuntimeError> {
        let task = self
            .tasks
            .load(task_id)
            .map_err(ToolTaskRuntimeError::Task)?
            .ok_or_else(|| {
                ToolTaskRuntimeError::Task(TaskStoreError::NotFound {
                    task_id: task_id.clone(),
                })
            })?;
        match task.status() {
            TaskStatus::Active => Ok(task),
            TaskStatus::Completed => Err(ToolTaskRuntimeError::Task(
                TaskStoreError::AlreadyCompleted {
                    task_id: task_id.clone(),
                },
            )),
            TaskStatus::Cancelled => Err(ToolTaskRuntimeError::Task(
                TaskStoreError::AlreadyCancelled {
                    task_id: task_id.clone(),
                },
            )),
            TaskStatus::Failed => Err(ToolTaskRuntimeError::Task(TaskStoreError::AlreadyFailed {
                task_id: task_id.clone(),
            })),
        }
    }

    fn ensure_session_writable(&self, session_id: &SessionId) -> Result<(), ToolTaskRuntimeError> {
        let session = self
            .sessions
            .load(session_id)
            .map_err(ToolTaskRuntimeError::Session)?
            .ok_or_else(|| {
                ToolTaskRuntimeError::Session(SessionStoreError::NotFound {
                    session_id: session_id.clone(),
                })
            })?;
        if session.status() == SessionStatus::Closed {
            return Err(ToolTaskRuntimeError::Session(
                SessionStoreError::SessionClosed {
                    session_id: session_id.clone(),
                },
            ));
        }
        Ok(())
    }

    fn ensure_invocation_available(
        &self,
        invocation_id: &ToolInvocationId,
    ) -> Result<(), ToolTaskRuntimeError> {
        if self
            .invocations
            .load(invocation_id)
            .map_err(ToolTaskRuntimeError::InvocationStore)?
            .is_some()
        {
            return Err(ToolTaskRuntimeError::Invocation(
                DurableToolRegistryInvocationError::Invocation(DurableToolInvocationError::Store(
                    ToolInvocationStoreError::AlreadyExists {
                        invocation_id: invocation_id.clone(),
                    },
                )),
            ));
        }
        Ok(())
    }
}

impl<P: ToolAssistantProvider + ToolAssistantContinuationProvider> ToolAssistantRuntime<P> {
    /// Explicitly continues with the same provider instance without persisting a session turn.
    pub fn continue_provider_step<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskContinuation<'_>,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ProviderToolStepOutcome, ToolTaskRuntimeError> {
        self.load_active_task(continuation.task_id)?;
        self.ensure_invocation_available(&invocation_id)?;
        continue_provider_tool_step(
            &mut self.provider,
            continuation.provider,
            registry,
            &mut self.invocations,
            continuation.task_id,
            invocation_id,
            authorizer,
        )
        .map_err(|error| match error {
            ProviderToolStepError::Provider(error) => ToolTaskRuntimeError::Provider(error),
            ProviderToolStepError::Invocation(error) => ToolTaskRuntimeError::Invocation(error),
        })
    }
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

    /// Executes a caller-requested final turn, records its response as an attempt, and fails the
    /// task with the caller-owned diagnostic.
    ///
    /// Deterministic task, observation, diagnostic, and session constraints are checked before
    /// the provider call. Transcript turns, the attempt, and failure then commit in that order, so
    /// a later failure preserves every earlier commit. Provider output is attempt evidence only.
    pub fn fail_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        failure: TaskFailure,
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
            .fail(task_id, failure)
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes a caller-requested final turn, records its response as an attempt, and cancels the
    /// task with the caller-owned reason.
    ///
    /// This terminal transition does not interrupt in-flight work. Deterministic task,
    /// observation, and session constraints are checked before the provider call. Transcript
    /// turns, the attempt, and cancellation then commit in that order, preserving earlier commits
    /// if a later operation fails.
    pub fn cancel_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        cancellation: TaskCancellation,
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
            .cancel(task_id, cancellation)
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
