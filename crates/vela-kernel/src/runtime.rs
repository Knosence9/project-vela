use std::{error::Error, fmt, path::Path};

use serde_json::Value;

use crate::session::{
    Session, SessionId, SessionStatus, SessionStore, SessionStoreError, SessionTurn,
    SessionTurnContent, SessionTurnRole,
};
use crate::skill::{RegisteredSkill, SkillId, SkillRegistry, SkillSelection, SkillSelectionError};
use crate::task::{
    Task, TaskCancellation, TaskFailure, TaskId, TaskObservationId, TaskObservationKind,
    TaskObservationText, TaskObservationTextError, TaskOutput, TaskOutputError, TaskStatus,
    TaskStore, TaskStoreError,
};
use crate::tool::{
    DurableToolInvocationError, DurableToolRegistryInvocationError, ToolAuthorizer, ToolId,
    ToolInvocationId, ToolInvocationStore, ToolInvocationStoreError, ToolMetadata, ToolRegistry,
};
use crate::workflow::{RegisteredWorkflowPhase, WorkflowPhaseSkillResolutionError};

/// A synchronous, provider-neutral source for one assistant response.
pub trait AssistantProvider {
    /// Produces one assistant turn from the complete durable conversation.
    fn complete(&mut self, transcript: &[SessionTurn])
    -> Result<SessionTurnContent, ProviderError>;
}

/// Caller-owned highest-authority policy for one explicitly composed request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemPolicy<'a>(&'a str);

impl<'a> SystemPolicy<'a> {
    pub fn new(value: &'a str) -> Self {
        Self(value)
    }

    pub fn as_str(self) -> &'a str {
        self.0
    }
}

/// Caller-owned policy below system authority for one explicitly composed request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeveloperPolicy<'a>(&'a str);

impl<'a> DeveloperPolicy<'a> {
    pub fn new(value: &'a str) -> Self {
        Self(value)
    }

    pub fn as_str(self) -> &'a str {
        self.0
    }
}

/// A provider-neutral tool-free request in descending authority-field order.
#[derive(Debug)]
pub struct ComposedAssistantRequest<'a> {
    system_policy: SystemPolicy<'a>,
    developer_policy: DeveloperPolicy<'a>,
    selected_skills: SkillSelection<'a>,
    transcript: &'a [SessionTurn],
}

impl<'a> ComposedAssistantRequest<'a> {
    pub fn system_policy(&self) -> SystemPolicy<'a> {
        self.system_policy
    }

    pub fn developer_policy(&self) -> DeveloperPolicy<'a> {
        self.developer_policy
    }

    pub fn skills(&self) -> impl Iterator<Item = &'a RegisteredSkill> + '_ {
        self.selected_skills.skills()
    }

    pub fn transcript(&self) -> &'a [SessionTurn] {
        self.transcript
    }
}

/// A synchronous provider for an explicitly composed, tool-free assistant response.
pub trait ComposedAssistantProvider {
    fn complete_composed(
        &mut self,
        request: ComposedAssistantRequest<'_>,
    ) -> Result<SessionTurnContent, ProviderError>;
}

/// Opaque retained policy, skill, and transcript context for one composed tool chain.
#[derive(Clone, Debug)]
struct ComposedToolContext<'a> {
    system_policy: SystemPolicy<'a>,
    developer_policy: DeveloperPolicy<'a>,
    selected_skills: SkillSelection<'a>,
    transcript: &'a [SessionTurn],
}

impl<'a> ComposedToolContext<'a> {
    fn request(&self, tools: Vec<ToolMetadata>) -> ComposedToolAssistantRequest<'a> {
        ComposedToolAssistantRequest {
            system_policy: self.system_policy,
            developer_policy: self.developer_policy,
            selected_skills: self.selected_skills.clone(),
            transcript: self.transcript,
            tools,
        }
    }
}

/// A provider-neutral composed request with capability metadata kept below instruction fields.
#[derive(Debug)]
pub struct ComposedToolAssistantRequest<'a> {
    system_policy: SystemPolicy<'a>,
    developer_policy: DeveloperPolicy<'a>,
    selected_skills: SkillSelection<'a>,
    transcript: &'a [SessionTurn],
    tools: Vec<ToolMetadata>,
}

impl<'a> ComposedToolAssistantRequest<'a> {
    pub fn system_policy(&self) -> SystemPolicy<'a> {
        self.system_policy
    }

    pub fn developer_policy(&self) -> DeveloperPolicy<'a> {
        self.developer_policy
    }

    pub fn skills(&self) -> impl Iterator<Item = &'a RegisteredSkill> + '_ {
        self.selected_skills.skills()
    }

    pub fn transcript(&self) -> &'a [SessionTurn] {
        self.transcript
    }

    pub fn tools(&self) -> &[ToolMetadata] {
        &self.tools
    }
}

/// A synchronous provider for one explicitly composed bounded tool step.
pub trait ComposedToolAssistantProvider {
    fn complete_composed_with_tools(
        &mut self,
        request: ComposedToolAssistantRequest<'_>,
    ) -> Result<ProviderToolResponse, ProviderError>;
}

/// A synchronous provider for one continuation with retained composition and one prior result.
pub trait ComposedToolAssistantContinuationProvider {
    fn complete_composed_after_tool(
        &mut self,
        request: ComposedToolAssistantContinuationRequest<'_>,
    ) -> Result<ProviderToolResponse, ProviderError>;
}

/// Caller-held retained composition and exact prior in-memory tool result.
#[derive(Clone, Debug)]
pub struct ComposedProviderToolContinuation<'a> {
    context: ComposedToolContext<'a>,
    prior_result: ProviderToolResult<'a>,
}

/// A provider request for one continuation with current descriptive tool metadata.
#[derive(Debug)]
pub struct ComposedToolAssistantContinuationRequest<'a> {
    context: ComposedToolContext<'a>,
    prior_result: ProviderToolResult<'a>,
    tools: Vec<ToolMetadata>,
}

impl<'a> ComposedToolAssistantContinuationRequest<'a> {
    pub fn request(&self) -> ComposedToolAssistantRequest<'a> {
        self.context.request(self.tools.clone())
    }

    pub fn prior_result(&self) -> ProviderToolResult<'a> {
        self.prior_result
    }
}

/// One opaque completed composed tool step with exact in-memory values and retained context.
#[derive(Debug)]
pub struct ComposedToolCompleted<'a> {
    tool_id: ToolId,
    input: Value,
    output: Value,
    continuation: ToolStepContinuation,
    context: ComposedToolContext<'a>,
}

impl ComposedToolCompleted<'_> {
    pub fn tool_id(&self) -> &ToolId {
        &self.tool_id
    }

    pub fn input(&self) -> &Value {
        &self.input
    }

    pub fn output(&self) -> &Value {
        &self.output
    }

    pub fn continuation(&self) -> ToolStepContinuation {
        self.continuation
    }
}

/// A composed bounded-step result that retains instruction fields only when continuation is needed.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedProviderToolStepOutcome<'a> {
    Final {
        content: SessionTurnContent,
        continuation: ToolStepContinuation,
    },
    ToolCompleted(ComposedToolCompleted<'a>),
}

impl ComposedProviderToolStepOutcome<'_> {
    /// Borrows an immutable continuation that retains the exact initial composition and result.
    pub fn continuation(&self) -> Option<ComposedProviderToolContinuation<'_>> {
        match self {
            Self::ToolCompleted(completed)
                if completed.continuation == ToolStepContinuation::ProviderRequired =>
            {
                Some(ComposedProviderToolContinuation {
                    context: completed.context.clone(),
                    prior_result: ProviderToolResult {
                        tool_id: &completed.tool_id,
                        input: &completed.input,
                        output: &completed.output,
                    },
                })
            }
            _ => None,
        }
    }
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

/// A deterministic skill-selection failure or an existing provider/tool step failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedProviderToolStepError {
    SkillSelection(SkillSelectionError),
    Step(ProviderToolStepError),
}

impl fmt::Display for ComposedProviderToolStepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SkillSelection(error) => {
                write!(formatter, "assistant skill selection error: {error}")
            }
            Self::Step(error) => error.fmt(formatter),
        }
    }
}

impl Error for ComposedProviderToolStepError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SkillSelection(error) => Some(error),
            Self::Step(error) => Some(error),
        }
    }
}

/// Calls a composed tool-capable provider once and dispatches at most one durable invocation.
///
/// Skill selection is validated before the provider call or any invocation side effect. A tool
/// result retains the exact policies, selected skill blocks, and transcript for continuation.
#[allow(clippy::too_many_arguments)]
pub fn execute_composed_provider_tool_step<'a, P, A>(
    provider: &mut P,
    system_policy: SystemPolicy<'a>,
    developer_policy: DeveloperPolicy<'a>,
    skill_registry: &'a SkillRegistry,
    selected_skill_ids: &[SkillId],
    transcript: &'a [SessionTurn],
    registry: &mut ToolRegistry,
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    authorizer: &mut A,
) -> Result<ComposedProviderToolStepOutcome<'a>, ComposedProviderToolStepError>
where
    P: ComposedToolAssistantProvider,
    A: ToolAuthorizer,
{
    let context = ComposedToolContext {
        system_policy,
        developer_policy,
        selected_skills: skill_registry
            .select(selected_skill_ids)
            .map_err(ComposedProviderToolStepError::SkillSelection)?,
        transcript,
    };
    let response = provider
        .complete_composed_with_tools(context.request(registry.metadata()))
        .map_err(ProviderToolStepError::Provider)
        .map_err(ComposedProviderToolStepError::Step)?;

    dispatch_composed_provider_tool_response(
        response,
        context,
        registry,
        store,
        task_id,
        invocation_id,
        authorizer,
    )
}

/// Calls one explicit composed continuation and dispatches at most one subsequent invocation.
///
/// The provider receives the exact retained policies, skill selection, transcript, and prior tool
/// result. Tool metadata is refreshed from the current registry without changing that composition.
#[allow(clippy::too_many_arguments)]
pub fn continue_composed_provider_tool_step<'a, P, A>(
    provider: &mut P,
    continuation: ComposedProviderToolContinuation<'a>,
    registry: &mut ToolRegistry,
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    authorizer: &mut A,
) -> Result<ComposedProviderToolStepOutcome<'a>, ComposedProviderToolStepError>
where
    P: ComposedToolAssistantContinuationProvider,
    A: ToolAuthorizer,
{
    let context = continuation.context.clone();
    let request = ComposedToolAssistantContinuationRequest {
        context: continuation.context,
        prior_result: continuation.prior_result,
        tools: registry.metadata(),
    };
    let response = provider
        .complete_composed_after_tool(request)
        .map_err(ProviderToolStepError::Provider)
        .map_err(ComposedProviderToolStepError::Step)?;

    dispatch_composed_provider_tool_response(
        response,
        context,
        registry,
        store,
        task_id,
        invocation_id,
        authorizer,
    )
}

fn dispatch_composed_provider_tool_response<'a, A: ToolAuthorizer>(
    response: ProviderToolResponse,
    context: ComposedToolContext<'a>,
    registry: &mut ToolRegistry,
    store: &mut ToolInvocationStore,
    task_id: &TaskId,
    invocation_id: ToolInvocationId,
    authorizer: &mut A,
) -> Result<ComposedProviderToolStepOutcome<'a>, ComposedProviderToolStepError> {
    match dispatch_provider_tool_response(
        response,
        registry,
        store,
        task_id,
        invocation_id,
        authorizer,
    )
    .map_err(ComposedProviderToolStepError::Step)?
    {
        ProviderToolStepOutcome::Final {
            content,
            continuation,
        } => Ok(ComposedProviderToolStepOutcome::Final {
            content,
            continuation,
        }),
        ProviderToolStepOutcome::ToolCompleted {
            tool_id,
            input,
            output,
            continuation,
        } => Ok(ComposedProviderToolStepOutcome::ToolCompleted(
            ComposedToolCompleted {
                tool_id,
                input,
                output,
                continuation,
                context,
            },
        )),
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

/// The durable result of one explicitly composed correction-oriented tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedToolTaskCorrectionOutcome<'a> {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        parent_attempt_id: TaskObservationId,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        selected_skills: SkillSelection<'a>,
    },
}

impl ComposedToolTaskCorrectionOutcome<'_> {
    /// Borrows the exact composition and caller-owned correction lineage for one continuation.
    pub fn continuation(&self) -> Option<ComposedToolTaskCorrectionContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                parent_attempt_id,
                tool_id,
                input,
                output,
                system_policy,
                developer_policy,
                selected_skills,
                ..
            } => Some(ComposedToolTaskCorrectionContinuation {
                task_id,
                parent_attempt_id,
                provider: ComposedProviderToolContinuation {
                    context: ComposedToolContext {
                        system_policy: *system_policy,
                        developer_policy: *developer_policy,
                        selected_skills: selected_skills.clone(),
                        transcript: session.turns(),
                    },
                    prior_result: ProviderToolResult {
                        tool_id,
                        input,
                        output,
                    },
                },
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed composed continuation restricted to correction-producing operations.
#[derive(Clone, Debug)]
pub struct ComposedToolTaskCorrectionContinuation<'a> {
    task_id: &'a TaskId,
    parent_attempt_id: &'a TaskObservationId,
    provider: ComposedProviderToolContinuation<'a>,
}

impl ComposedToolTaskCorrectionContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }

    pub fn parent_attempt_id(&self) -> &TaskObservationId {
        self.parent_attempt_id
    }
}

fn composed_correction_outcome<'a>(
    outcome: ComposedToolTaskTurnOutcome<'a>,
    parent_attempt_id: TaskObservationId,
) -> ComposedToolTaskCorrectionOutcome<'a> {
    match outcome {
        ComposedToolTaskTurnOutcome::Final { session, task } => {
            ComposedToolTaskCorrectionOutcome::Final { session, task }
        }
        ComposedToolTaskTurnOutcome::ToolCompleted {
            session,
            task_id,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        } => ComposedToolTaskCorrectionOutcome::ToolCompleted {
            session,
            task_id,
            parent_attempt_id,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        },
    }
}

/// The durable result of one explicitly composed completion-oriented tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedToolTaskCompletionOutcome<'a> {
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
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        selected_skills: SkillSelection<'a>,
    },
}

impl ComposedToolTaskCompletionOutcome<'_> {
    /// Borrows the exact composition and completion intent for one continuation.
    pub fn continuation(&self) -> Option<ComposedToolTaskCompletionContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                system_policy,
                developer_policy,
                selected_skills,
                ..
            } => Some(ComposedToolTaskCompletionContinuation {
                task_id,
                provider: ComposedProviderToolContinuation {
                    context: ComposedToolContext {
                        system_policy: *system_policy,
                        developer_policy: *developer_policy,
                        selected_skills: selected_skills.clone(),
                        transcript: session.turns(),
                    },
                    prior_result: ProviderToolResult {
                        tool_id,
                        input,
                        output,
                    },
                },
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed composed continuation restricted to completion-producing operations.
#[derive(Clone, Debug)]
pub struct ComposedToolTaskCompletionContinuation<'a> {
    task_id: &'a TaskId,
    provider: ComposedProviderToolContinuation<'a>,
}

impl ComposedToolTaskCompletionContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }
}

fn complete_composed_outcome<'a>(
    tasks: &mut TaskStore,
    outcome: ComposedToolTaskTurnOutcome<'a>,
) -> Result<ComposedToolTaskCompletionOutcome<'a>, ToolTaskRuntimeError> {
    match outcome {
        ComposedToolTaskTurnOutcome::Final { session, task } => {
            let content = session
                .turns()
                .last()
                .expect("a final composed task outcome has an assistant turn")
                .content();
            let output = TaskOutput::new(content.as_str())
                .map_err(ToolTaskRuntimeError::InvalidTaskOutput)?;
            let task = tasks
                .complete(task.id(), output)
                .map_err(ToolTaskRuntimeError::Task)?;
            Ok(ComposedToolTaskCompletionOutcome::Final { session, task })
        }
        ComposedToolTaskTurnOutcome::ToolCompleted {
            session,
            task_id,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        } => Ok(ComposedToolTaskCompletionOutcome::ToolCompleted {
            session,
            task_id,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        }),
    }
}

/// The durable result of one explicitly composed failure-oriented tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedToolTaskFailureOutcome<'a> {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        failure: TaskFailure,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        selected_skills: SkillSelection<'a>,
    },
}

impl ComposedToolTaskFailureOutcome<'_> {
    /// Borrows the exact composition and caller-owned failure intent for one continuation.
    pub fn continuation(&self) -> Option<ComposedToolTaskFailureContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                failure,
                tool_id,
                input,
                output,
                system_policy,
                developer_policy,
                selected_skills,
                ..
            } => Some(ComposedToolTaskFailureContinuation {
                task_id,
                failure,
                provider: ComposedProviderToolContinuation {
                    context: ComposedToolContext {
                        system_policy: *system_policy,
                        developer_policy: *developer_policy,
                        selected_skills: selected_skills.clone(),
                        transcript: session.turns(),
                    },
                    prior_result: ProviderToolResult {
                        tool_id,
                        input,
                        output,
                    },
                },
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed composed continuation restricted to failure-producing operations.
#[derive(Clone, Debug)]
pub struct ComposedToolTaskFailureContinuation<'a> {
    task_id: &'a TaskId,
    failure: &'a TaskFailure,
    provider: ComposedProviderToolContinuation<'a>,
}

impl ComposedToolTaskFailureContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }

    pub fn failure(&self) -> &TaskFailure {
        self.failure
    }
}

fn fail_composed_outcome<'a>(
    tasks: &mut TaskStore,
    outcome: ComposedToolTaskTurnOutcome<'a>,
    failure: TaskFailure,
) -> Result<ComposedToolTaskFailureOutcome<'a>, ToolTaskRuntimeError> {
    match outcome {
        ComposedToolTaskTurnOutcome::Final { session, task } => {
            let task = tasks
                .fail(task.id(), failure)
                .map_err(ToolTaskRuntimeError::Task)?;
            Ok(ComposedToolTaskFailureOutcome::Final { session, task })
        }
        ComposedToolTaskTurnOutcome::ToolCompleted {
            session,
            task_id,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        } => Ok(ComposedToolTaskFailureOutcome::ToolCompleted {
            session,
            task_id,
            failure,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        }),
    }
}

/// The durable result of one explicitly composed cancellation-oriented tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedToolTaskCancellationOutcome<'a> {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        cancellation: TaskCancellation,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        selected_skills: SkillSelection<'a>,
    },
}

impl ComposedToolTaskCancellationOutcome<'_> {
    /// Borrows the exact composition and caller-owned cancellation intent for one continuation.
    pub fn continuation(&self) -> Option<ComposedToolTaskCancellationContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                cancellation,
                tool_id,
                input,
                output,
                system_policy,
                developer_policy,
                selected_skills,
                ..
            } => Some(ComposedToolTaskCancellationContinuation {
                task_id,
                cancellation,
                provider: ComposedProviderToolContinuation {
                    context: ComposedToolContext {
                        system_policy: *system_policy,
                        developer_policy: *developer_policy,
                        selected_skills: selected_skills.clone(),
                        transcript: session.turns(),
                    },
                    prior_result: ProviderToolResult {
                        tool_id,
                        input,
                        output,
                    },
                },
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed composed continuation restricted to cancellation-producing operations.
#[derive(Clone, Debug)]
pub struct ComposedToolTaskCancellationContinuation<'a> {
    task_id: &'a TaskId,
    cancellation: &'a TaskCancellation,
    provider: ComposedProviderToolContinuation<'a>,
}

impl ComposedToolTaskCancellationContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }

    pub fn cancellation(&self) -> &TaskCancellation {
        self.cancellation
    }
}

fn cancel_composed_outcome<'a>(
    tasks: &mut TaskStore,
    outcome: ComposedToolTaskTurnOutcome<'a>,
    cancellation: TaskCancellation,
) -> Result<ComposedToolTaskCancellationOutcome<'a>, ToolTaskRuntimeError> {
    match outcome {
        ComposedToolTaskTurnOutcome::Final { session, task } => {
            let task = tasks
                .cancel(task.id(), cancellation)
                .map_err(ToolTaskRuntimeError::Task)?;
            Ok(ComposedToolTaskCancellationOutcome::Final { session, task })
        }
        ComposedToolTaskTurnOutcome::ToolCompleted {
            session,
            task_id,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        } => Ok(ComposedToolTaskCancellationOutcome::ToolCompleted {
            session,
            task_id,
            cancellation,
            tool_id,
            input,
            output,
            continuation,
            system_policy,
            developer_policy,
            selected_skills,
        }),
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

/// The durable result of one explicitly composed tool-capable task turn.
#[derive(Debug)]
#[non_exhaustive]
pub enum ComposedToolTaskTurnOutcome<'a> {
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
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        selected_skills: SkillSelection<'a>,
    },
}

impl ComposedToolTaskTurnOutcome<'_> {
    /// Borrows the durable lineage, exact composition, and prior result for one continuation.
    pub fn continuation(&self) -> Option<ComposedToolTaskContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                system_policy,
                developer_policy,
                selected_skills,
                ..
            } => Some(ComposedToolTaskContinuation {
                task_id,
                provider: ComposedProviderToolContinuation {
                    context: ComposedToolContext {
                        system_policy: *system_policy,
                        developer_policy: *developer_policy,
                        selected_skills: selected_skills.clone(),
                        transcript: session.turns(),
                    },
                    prior_result: ProviderToolResult {
                        tool_id,
                        input,
                        output,
                    },
                },
            }),
            Self::Final { .. } => None,
        }
    }
}

/// A borrowed continuation bound to one task and one immutable composed request context.
#[derive(Clone, Debug)]
pub struct ComposedToolTaskContinuation<'a> {
    task_id: &'a TaskId,
    provider: ComposedProviderToolContinuation<'a>,
}

impl ComposedToolTaskContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }
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

/// The durable result of one correction-oriented provider/tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskCorrectionOutcome {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        parent_attempt_id: TaskObservationId,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
    },
}

impl ToolTaskCorrectionOutcome {
    /// Borrows a continuation that preserves caller-owned correction lineage.
    pub fn continuation(&self) -> Option<ToolTaskCorrectionContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                parent_attempt_id,
                tool_id,
                input,
                output,
                ..
            } => Some(ToolTaskCorrectionContinuation {
                task_id,
                parent_attempt_id,
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

/// A borrowed provider/tool continuation that can only resume correction-oriented operations.
#[derive(Clone, Copy, Debug)]
pub struct ToolTaskCorrectionContinuation<'a> {
    task_id: &'a TaskId,
    parent_attempt_id: &'a TaskObservationId,
    provider: ProviderToolContinuation<'a>,
}

impl ToolTaskCorrectionContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }

    pub fn parent_attempt_id(&self) -> &TaskObservationId {
        self.parent_attempt_id
    }
}

/// The durable result of one caller-requested completion-oriented provider/tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskCompletionOutcome {
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

impl ToolTaskCompletionOutcome {
    /// Borrows a continuation that preserves caller-requested completion intent.
    pub fn continuation(&self) -> Option<ToolTaskCompletionContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                ..
            } => Some(ToolTaskCompletionContinuation {
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

/// A borrowed provider/tool continuation that can only resume completion-oriented operations.
#[derive(Clone, Copy, Debug)]
pub struct ToolTaskCompletionContinuation<'a> {
    task_id: &'a TaskId,
    provider: ProviderToolContinuation<'a>,
}

impl ToolTaskCompletionContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }
}

/// The durable result of one caller-requested failure-oriented provider/tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskFailureOutcome {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        failure: TaskFailure,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
    },
}

impl ToolTaskFailureOutcome {
    /// Borrows a continuation that preserves caller-requested failure intent and diagnostic.
    pub fn continuation(&self) -> Option<ToolTaskFailureContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                failure,
                tool_id,
                input,
                output,
                ..
            } => Some(ToolTaskFailureContinuation {
                task_id,
                failure,
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

/// A borrowed provider/tool continuation that can only resume failure-oriented operations.
#[derive(Clone, Copy, Debug)]
pub struct ToolTaskFailureContinuation<'a> {
    task_id: &'a TaskId,
    failure: &'a TaskFailure,
    provider: ProviderToolContinuation<'a>,
}

impl ToolTaskFailureContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }

    pub fn failure(&self) -> &TaskFailure {
        self.failure
    }
}

/// The durable result of one caller-requested cancellation-oriented provider/tool step.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskCancellationOutcome {
    Final {
        session: Session,
        task: Task,
    },
    ToolCompleted {
        session: Session,
        task_id: TaskId,
        cancellation: TaskCancellation,
        tool_id: ToolId,
        input: Value,
        output: Value,
        continuation: ToolStepContinuation,
    },
}

impl ToolTaskCancellationOutcome {
    /// Borrows a continuation that preserves caller-requested cancellation intent and reason.
    pub fn continuation(&self) -> Option<ToolTaskCancellationContinuation<'_>> {
        match self {
            Self::ToolCompleted {
                session,
                task_id,
                cancellation,
                tool_id,
                input,
                output,
                ..
            } => Some(ToolTaskCancellationContinuation {
                task_id,
                cancellation,
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

/// A borrowed provider/tool continuation that can only resume cancellation-oriented operations.
#[derive(Clone, Copy, Debug)]
pub struct ToolTaskCancellationContinuation<'a> {
    task_id: &'a TaskId,
    cancellation: &'a TaskCancellation,
    provider: ProviderToolContinuation<'a>,
}

impl ToolTaskCancellationContinuation<'_> {
    pub fn task_id(&self) -> &TaskId {
        self.task_id
    }

    pub fn cancellation(&self) -> &TaskCancellation {
        self.cancellation
    }
}

/// A failure while bridging one initial provider-tool step into a durable task turn.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolTaskRuntimeError {
    SkillSelection(SkillSelectionError),
    Session(SessionStoreError),
    Task(TaskStoreError),
    InvocationStore(ToolInvocationStoreError),
    TaskNotAssociated { task_id: TaskId },
    StaleContinuationTranscript { task_id: TaskId },
    InvalidAttemptText(TaskObservationTextError),
    InvalidCorrectionText(TaskObservationTextError),
    InvalidTaskOutput(TaskOutputError),
    Provider(ProviderError),
    Invocation(DurableToolRegistryInvocationError),
}

impl fmt::Display for ToolTaskRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SkillSelection(error) => {
                write!(formatter, "tool task skill selection error: {error}")
            }
            Self::Session(error) => write!(formatter, "tool task runtime session error: {error}"),
            Self::Task(error) => write!(formatter, "tool task runtime task error: {error}"),
            Self::InvocationStore(error) => {
                write!(formatter, "tool invocation store error: {error}")
            }
            Self::TaskNotAssociated { task_id } => {
                write!(formatter, "task {task_id} is not associated with a session")
            }
            Self::StaleContinuationTranscript { task_id } => write!(
                formatter,
                "task {task_id} continuation transcript is no longer current"
            ),
            Self::InvalidAttemptText(error) => {
                write!(formatter, "tool task attempt observation error: {error}")
            }
            Self::InvalidCorrectionText(error) => {
                write!(formatter, "tool task correction observation error: {error}")
            }
            Self::InvalidTaskOutput(error) => {
                write!(formatter, "tool task output error: {error}")
            }
            Self::Provider(error) => write!(formatter, "assistant provider error: {error}"),
            Self::Invocation(error) => write!(formatter, "assistant tool step error: {error}"),
        }
    }
}

impl Error for ToolTaskRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SkillSelection(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::InvocationStore(error) => Some(error),
            Self::InvalidAttemptText(error) => Some(error),
            Self::InvalidCorrectionText(error) => Some(error),
            Self::InvalidTaskOutput(error) => Some(error),
            Self::Provider(error) => Some(error),
            Self::Invocation(error) => Some(error),
            Self::TaskNotAssociated { .. } | Self::StaleContinuationTranscript { .. } => None,
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

impl<P> ToolAssistantRuntime<P> {
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
}

impl<P: ToolAssistantProvider> ToolAssistantRuntime<P> {
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
        self.execute_observation_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            invocation_id,
            registry,
            authorizer,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the private bridge retains explicit evidence and invocation identities"
    )]
    fn execute_observation_task_turn<A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        observation_id: TaskObservationId,
        observation_kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
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
        task.validate_observation_append(&observation_id, observation_kind, parent_attempt_id)
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
                let observation_text = content.as_str().to_owned();
                let session = self
                    .sessions
                    .append_turn(&session_id, SessionTurnRole::Assistant, content)
                    .map_err(ToolTaskRuntimeError::Session)?;
                let task = self.persist_final_observation(
                    task_id,
                    observation_id,
                    observation_kind,
                    parent_attempt_id,
                    observation_text,
                )?;
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

    /// Executes one correction turn through a bounded provider/tool step.
    ///
    /// Final provider content becomes Correction evidence linked to the caller-owned parent
    /// Attempt. A tool request remains non-terminal and retains that lineage only in memory.
    #[allow(
        clippy::too_many_arguments,
        reason = "correction adds explicit parent and evidence identities to task-turn inputs"
    )]
    pub fn execute_task_correction_turn<A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        parent_attempt_id: &TaskObservationId,
        human_content: SessionTurnContent,
        correction_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskCorrectionOutcome, ToolTaskRuntimeError> {
        let outcome = self.execute_observation_task_turn(
            task_id,
            human_content,
            correction_observation_id,
            TaskObservationKind::Correction,
            Some(parent_attempt_id),
            invocation_id,
            registry,
            authorizer,
        )?;
        Ok(Self::correction_outcome(outcome, parent_attempt_id.clone()))
    }

    fn correction_outcome(
        outcome: ToolTaskTurnOutcome,
        parent_attempt_id: TaskObservationId,
    ) -> ToolTaskCorrectionOutcome {
        match outcome {
            ToolTaskTurnOutcome::Final { session, task } => {
                ToolTaskCorrectionOutcome::Final { session, task }
            }
            ToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                continuation,
            } => ToolTaskCorrectionOutcome::ToolCompleted {
                session,
                task_id,
                parent_attempt_id,
                tool_id,
                input,
                output,
                continuation,
            },
        }
    }

    /// Executes one caller-requested completion turn through a bounded provider/tool step.
    ///
    /// A tool request remains non-terminal and requires an explicit completion continuation. A
    /// final response commits the assistant turn, an Attempt, and completion in that order.
    pub fn complete_task_turn<A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskCompletionOutcome, ToolTaskRuntimeError> {
        let outcome = self.execute_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        self.complete_final_outcome(outcome)
    }

    fn complete_final_outcome(
        &mut self,
        outcome: ToolTaskTurnOutcome,
    ) -> Result<ToolTaskCompletionOutcome, ToolTaskRuntimeError> {
        match outcome {
            ToolTaskTurnOutcome::Final { session, task } => {
                let content = session
                    .turns()
                    .last()
                    .expect("a final tool task outcome has an assistant turn")
                    .content();
                let output = TaskOutput::new(content.as_str())
                    .map_err(ToolTaskRuntimeError::InvalidTaskOutput)?;
                let task = self
                    .tasks
                    .complete(task.id(), output)
                    .map_err(ToolTaskRuntimeError::Task)?;
                Ok(ToolTaskCompletionOutcome::Final { session, task })
            }
            ToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                continuation,
            } => Ok(ToolTaskCompletionOutcome::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                continuation,
            }),
        }
    }

    /// Executes one caller-requested failure turn through a bounded provider/tool step.
    ///
    /// Provider content is Attempt evidence only; the caller-owned diagnostic fails the task
    /// after a final response. A tool request remains non-terminal and requires an explicit
    /// failure continuation.
    #[allow(
        clippy::too_many_arguments,
        reason = "failure adds one caller-owned diagnostic to the symmetric task-turn inputs"
    )]
    pub fn fail_task_turn<A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        failure: TaskFailure,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskFailureOutcome, ToolTaskRuntimeError> {
        let outcome = self.execute_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        self.fail_final_outcome(outcome, failure)
    }

    fn fail_final_outcome(
        &mut self,
        outcome: ToolTaskTurnOutcome,
        failure: TaskFailure,
    ) -> Result<ToolTaskFailureOutcome, ToolTaskRuntimeError> {
        match outcome {
            ToolTaskTurnOutcome::Final { session, task } => {
                let task = self
                    .tasks
                    .fail(task.id(), failure)
                    .map_err(ToolTaskRuntimeError::Task)?;
                Ok(ToolTaskFailureOutcome::Final { session, task })
            }
            ToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                continuation,
            } => Ok(ToolTaskFailureOutcome::ToolCompleted {
                session,
                task_id,
                failure,
                tool_id,
                input,
                output,
                continuation,
            }),
        }
    }

    /// Executes one caller-requested cancellation turn through a bounded provider/tool step.
    ///
    /// Provider content is Attempt evidence only; the caller-owned reason cancels the task after
    /// a final response. A tool request remains non-terminal and requires an explicit
    /// cancellation continuation. This records a decision and does not interrupt in-flight work.
    #[allow(
        clippy::too_many_arguments,
        reason = "cancellation adds one caller-owned reason to the symmetric task-turn inputs"
    )]
    pub fn cancel_task_turn<A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        cancellation: TaskCancellation,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskCancellationOutcome, ToolTaskRuntimeError> {
        let outcome = self.execute_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        self.cancel_final_outcome(outcome, cancellation)
    }

    fn cancel_final_outcome(
        &mut self,
        outcome: ToolTaskTurnOutcome,
        cancellation: TaskCancellation,
    ) -> Result<ToolTaskCancellationOutcome, ToolTaskRuntimeError> {
        match outcome {
            ToolTaskTurnOutcome::Final { session, task } => {
                let task = self
                    .tasks
                    .cancel(task.id(), cancellation)
                    .map_err(ToolTaskRuntimeError::Task)?;
                Ok(ToolTaskCancellationOutcome::Final { session, task })
            }
            ToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id,
                tool_id,
                input,
                output,
                continuation,
            } => Ok(ToolTaskCancellationOutcome::ToolCompleted {
                session,
                task_id,
                cancellation,
                tool_id,
                input,
                output,
                continuation,
            }),
        }
    }
}

impl<P> ToolAssistantRuntime<P> {
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

    fn persist_final_observation(
        &mut self,
        task_id: &TaskId,
        observation_id: TaskObservationId,
        observation_kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
        text: String,
    ) -> Result<Task, ToolTaskRuntimeError> {
        let observation_text =
            TaskObservationText::new(text).map_err(|error| match observation_kind {
                TaskObservationKind::Correction => {
                    ToolTaskRuntimeError::InvalidCorrectionText(error)
                }
                _ => ToolTaskRuntimeError::InvalidAttemptText(error),
            })?;
        match parent_attempt_id {
            Some(parent_attempt_id) => self.tasks.append_observation_for_attempt(
                task_id,
                observation_id,
                observation_kind,
                observation_text,
                parent_attempt_id.clone(),
            ),
            None => self.tasks.append_observation(
                task_id,
                observation_id,
                observation_kind,
                observation_text,
            ),
        }
        .map_err(ToolTaskRuntimeError::Task)
    }
}

impl<P: ComposedToolAssistantProvider> ToolAssistantRuntime<P> {
    /// Appends one human turn and executes one explicitly composed provider/tool step.
    #[allow(
        clippy::too_many_arguments,
        reason = "composed turns add explicit policies, registry, and selection inputs"
    )]
    pub fn execute_composed_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        skill_registry: &'a SkillRegistry,
        selected_skill_ids: &[SkillId],
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskTurnOutcome<'a>, ToolTaskRuntimeError> {
        self.execute_composed_observation_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            invocation_id,
            system_policy,
            developer_policy,
            skill_registry,
            selected_skill_ids,
            registry,
            authorizer,
        )
    }

    /// Executes one explicitly composed completion turn through a bounded provider/tool step.
    #[allow(
        clippy::too_many_arguments,
        reason = "composed completion retains explicit policy, skill, evidence, and tool inputs"
    )]
    pub fn complete_composed_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        skill_registry: &'a SkillRegistry,
        selected_skill_ids: &[SkillId],
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskCompletionOutcome<'a>, ToolTaskRuntimeError> {
        let outcome = self.execute_composed_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            invocation_id,
            system_policy,
            developer_policy,
            skill_registry,
            selected_skill_ids,
            registry,
            authorizer,
        )?;
        complete_composed_outcome(&mut self.tasks, outcome)
    }

    /// Executes one explicitly composed failure turn with a caller-owned diagnostic.
    ///
    /// A tool request remains non-terminal and retains the exact failure intent. A final response
    /// commits the assistant turn and Attempt before failing the task with the supplied diagnostic.
    #[allow(
        clippy::too_many_arguments,
        reason = "composed failure retains explicit policy, skill, evidence, diagnostic, and tool inputs"
    )]
    pub fn fail_composed_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        failure: TaskFailure,
        invocation_id: ToolInvocationId,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        skill_registry: &'a SkillRegistry,
        selected_skill_ids: &[SkillId],
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskFailureOutcome<'a>, ToolTaskRuntimeError> {
        let outcome = self.execute_composed_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            invocation_id,
            system_policy,
            developer_policy,
            skill_registry,
            selected_skill_ids,
            registry,
            authorizer,
        )?;
        fail_composed_outcome(&mut self.tasks, outcome, failure)
    }

    /// Executes one explicitly composed cancellation turn with a caller-owned reason.
    ///
    /// A tool request remains non-terminal and retains the exact cancellation intent. A final
    /// response commits the assistant turn and Attempt before cancelling with the supplied reason.
    #[allow(
        clippy::too_many_arguments,
        reason = "composed cancellation retains explicit policy, skill, evidence, reason, and tool inputs"
    )]
    pub fn cancel_composed_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        cancellation: TaskCancellation,
        invocation_id: ToolInvocationId,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        skill_registry: &'a SkillRegistry,
        selected_skill_ids: &[SkillId],
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskCancellationOutcome<'a>, ToolTaskRuntimeError> {
        let outcome = self.execute_composed_task_turn(
            task_id,
            human_content,
            attempt_observation_id,
            invocation_id,
            system_policy,
            developer_policy,
            skill_registry,
            selected_skill_ids,
            registry,
            authorizer,
        )?;
        cancel_composed_outcome(&mut self.tasks, outcome, cancellation)
    }

    /// Executes an explicitly composed correction turn while retaining caller-owned lineage.
    #[allow(
        clippy::too_many_arguments,
        reason = "composed corrections add explicit policy, skill, parent, and evidence inputs"
    )]
    pub fn execute_composed_task_correction_turn<'a, A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        parent_attempt_id: &TaskObservationId,
        human_content: SessionTurnContent,
        correction_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        skill_registry: &'a SkillRegistry,
        selected_skill_ids: &[SkillId],
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskCorrectionOutcome<'a>, ToolTaskRuntimeError> {
        let outcome = self.execute_composed_observation_task_turn(
            task_id,
            human_content,
            correction_observation_id,
            TaskObservationKind::Correction,
            Some(parent_attempt_id),
            invocation_id,
            system_policy,
            developer_policy,
            skill_registry,
            selected_skill_ids,
            registry,
            authorizer,
        )?;
        Ok(composed_correction_outcome(
            outcome,
            parent_attempt_id.clone(),
        ))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "private composed evidence execution retains all explicit authority inputs"
    )]
    fn execute_composed_observation_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        observation_id: TaskObservationId,
        observation_kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
        invocation_id: ToolInvocationId,
        system_policy: SystemPolicy<'a>,
        developer_policy: DeveloperPolicy<'a>,
        skill_registry: &'a SkillRegistry,
        selected_skill_ids: &[SkillId],
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskTurnOutcome<'a>, ToolTaskRuntimeError> {
        let selected_skills = skill_registry
            .select(selected_skill_ids)
            .map_err(ToolTaskRuntimeError::SkillSelection)?;
        let task = self.load_active_task(task_id)?;
        let session_id =
            task.session_id()
                .cloned()
                .ok_or_else(|| ToolTaskRuntimeError::TaskNotAssociated {
                    task_id: task_id.clone(),
                })?;
        task.validate_observation_append(&observation_id, observation_kind, parent_attempt_id)
            .map_err(ToolTaskRuntimeError::Task)?;
        self.ensure_session_writable(&session_id)?;
        self.ensure_invocation_available(&invocation_id)?;

        let session = self
            .sessions
            .append_turn(&session_id, SessionTurnRole::Human, human_content)
            .map_err(ToolTaskRuntimeError::Session)?;
        let response = self
            .provider
            .complete_composed_with_tools(ComposedToolAssistantRequest {
                system_policy,
                developer_policy,
                selected_skills: selected_skills.clone(),
                transcript: session.turns(),
                tools: registry.metadata(),
            })
            .map_err(ToolTaskRuntimeError::Provider)?;
        let step = dispatch_provider_tool_response(
            response,
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
                let observation_text = content.as_str().to_owned();
                let session = self
                    .sessions
                    .append_turn(&session_id, SessionTurnRole::Assistant, content)
                    .map_err(ToolTaskRuntimeError::Session)?;
                let task = self.persist_final_observation(
                    task_id,
                    observation_id,
                    observation_kind,
                    parent_attempt_id,
                    observation_text,
                )?;
                Ok(ComposedToolTaskTurnOutcome::Final { session, task })
            }
            ProviderToolStepOutcome::ToolCompleted {
                tool_id,
                input,
                output,
                continuation,
            } => Ok(ComposedToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id: task_id.clone(),
                tool_id,
                input,
                output,
                continuation,
                system_policy,
                developer_policy,
                selected_skills,
            }),
        }
    }
}

impl<P: ToolAssistantProvider + ToolAssistantContinuationProvider> ToolAssistantRuntime<P> {
    /// Continues one task-bound provider step and durably bridges its result into the same turn.
    ///
    /// The caller retains control of every step, observation identity, invocation identity,
    /// registry, and authorization decision. A final response commits an assistant turn followed
    /// by an Attempt. Another completed tool request remains exact only in memory and requires a
    /// subsequent explicit continuation.
    pub fn continue_task_turn<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskContinuation<'_>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskTurnOutcome, ToolTaskRuntimeError> {
        self.continue_observation_task_turn(
            continuation,
            attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            invocation_id,
            registry,
            authorizer,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the private continuation retains explicit evidence and invocation identities"
    )]
    fn continue_observation_task_turn<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskContinuation<'_>,
        observation_id: TaskObservationId,
        observation_kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskTurnOutcome, ToolTaskRuntimeError> {
        let task = self.load_active_task(continuation.task_id)?;
        let session_id =
            task.session_id()
                .cloned()
                .ok_or_else(|| ToolTaskRuntimeError::TaskNotAssociated {
                    task_id: continuation.task_id.clone(),
                })?;
        let session = self
            .sessions
            .load(&session_id)
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
        if session.turns() != continuation.provider.transcript() {
            return Err(ToolTaskRuntimeError::StaleContinuationTranscript {
                task_id: continuation.task_id.clone(),
            });
        }
        task.validate_observation_append(&observation_id, observation_kind, parent_attempt_id)
            .map_err(ToolTaskRuntimeError::Task)?;
        self.ensure_invocation_available(&invocation_id)?;

        let response = self
            .provider
            .complete_after_tool(continuation.provider, &registry.metadata())
            .map_err(ToolTaskRuntimeError::Provider)?;
        let current_session = self
            .sessions
            .load(&session_id)
            .map_err(ToolTaskRuntimeError::Session)?
            .ok_or_else(|| {
                ToolTaskRuntimeError::Session(SessionStoreError::NotFound {
                    session_id: session_id.clone(),
                })
            })?;
        if current_session.status() == SessionStatus::Closed {
            return Err(ToolTaskRuntimeError::Session(
                SessionStoreError::SessionClosed {
                    session_id: session_id.clone(),
                },
            ));
        }
        if current_session.turns() != continuation.provider.transcript() {
            return Err(ToolTaskRuntimeError::StaleContinuationTranscript {
                task_id: continuation.task_id.clone(),
            });
        }
        let step = dispatch_provider_tool_response(
            response,
            registry,
            &mut self.invocations,
            continuation.task_id,
            invocation_id,
            authorizer,
        )
        .map_err(|error| match error {
            ProviderToolStepError::Provider(error) => ToolTaskRuntimeError::Provider(error),
            ProviderToolStepError::Invocation(error) => ToolTaskRuntimeError::Invocation(error),
        })?;

        match step {
            ProviderToolStepOutcome::Final { content, .. } => {
                let observation_text = content.as_str().to_owned();
                let session = self
                    .sessions
                    .append_turn_if_current_transcript(
                        &session_id,
                        continuation.provider.transcript(),
                        SessionTurnRole::Assistant,
                        content,
                    )
                    .map_err(ToolTaskRuntimeError::Session)?
                    .ok_or_else(|| ToolTaskRuntimeError::StaleContinuationTranscript {
                        task_id: continuation.task_id.clone(),
                    })?;
                let task = self.persist_final_observation(
                    continuation.task_id,
                    observation_id,
                    observation_kind,
                    parent_attempt_id,
                    observation_text,
                )?;
                Ok(ToolTaskTurnOutcome::Final { session, task })
            }
            ProviderToolStepOutcome::ToolCompleted {
                tool_id,
                input,
                output,
                continuation: next,
            } => Ok(ToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id: continuation.task_id.clone(),
                tool_id,
                input,
                output,
                continuation: next,
            }),
        }
    }

    /// Continues a correction turn while preserving the caller-owned parent Attempt.
    pub fn continue_correction_task_turn<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskCorrectionContinuation<'_>,
        correction_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskCorrectionOutcome, ToolTaskRuntimeError> {
        let parent_attempt_id = continuation.parent_attempt_id.clone();
        let continuation = ToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_observation_task_turn(
            continuation,
            correction_observation_id,
            TaskObservationKind::Correction,
            Some(&parent_attempt_id),
            invocation_id,
            registry,
            authorizer,
        )?;
        Ok(Self::correction_outcome(outcome, parent_attempt_id))
    }

    /// Continues a caller-requested completion turn with the same provider instance.
    ///
    /// Another tool request remains non-terminal. A final response preserves the continuation's
    /// guarded assistant append, then records the Attempt and completes the task.
    pub fn continue_completion_task_turn<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskCompletionContinuation<'_>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskCompletionOutcome, ToolTaskRuntimeError> {
        let continuation = ToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_task_turn(
            continuation,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        self.complete_final_outcome(outcome)
    }

    /// Continues a caller-requested failure turn with the same provider and diagnostic.
    pub fn continue_failure_task_turn<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskFailureContinuation<'_>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskFailureOutcome, ToolTaskRuntimeError> {
        let failure = continuation.failure.clone();
        let continuation = ToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_task_turn(
            continuation,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        self.fail_final_outcome(outcome, failure)
    }

    /// Continues a caller-requested cancellation turn with the same provider and reason.
    pub fn continue_cancellation_task_turn<A: ToolAuthorizer>(
        &mut self,
        continuation: ToolTaskCancellationContinuation<'_>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ToolTaskCancellationOutcome, ToolTaskRuntimeError> {
        let cancellation = continuation.cancellation.clone();
        let continuation = ToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_task_turn(
            continuation,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        self.cancel_final_outcome(outcome, cancellation)
    }

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
    SkillSelection(SkillSelectionError),
    WorkflowPhaseSkills(WorkflowPhaseSkillResolutionError),
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
            Self::SkillSelection(error) => {
                write!(formatter, "assistant skill selection error: {error}")
            }
            Self::WorkflowPhaseSkills(error) => {
                write!(formatter, "assistant workflow phase skill error: {error}")
            }
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
            Self::SkillSelection(error) => Some(error),
            Self::WorkflowPhaseSkills(error) => Some(error),
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

impl<P: ComposedToolAssistantContinuationProvider> ToolAssistantRuntime<P> {
    /// Continues one task-bound composed step without permitting composition substitution.
    pub fn continue_composed_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        continuation: ComposedToolTaskContinuation<'a>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskTurnOutcome<'a>, ToolTaskRuntimeError> {
        self.continue_composed_observation_task_turn(
            continuation,
            attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            invocation_id,
            registry,
            authorizer,
        )
    }

    /// Continues a composed completion while preserving its immutable authority context.
    pub fn continue_composed_completion_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        continuation: ComposedToolTaskCompletionContinuation<'a>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskCompletionOutcome<'a>, ToolTaskRuntimeError> {
        let continuation = ComposedToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_composed_task_turn(
            continuation,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        complete_composed_outcome(&mut self.tasks, outcome)
    }

    /// Continues a composed failure while preserving immutable authority and failure intent.
    pub fn continue_composed_failure_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        continuation: ComposedToolTaskFailureContinuation<'a>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskFailureOutcome<'a>, ToolTaskRuntimeError> {
        let failure = continuation.failure.clone();
        let continuation = ComposedToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_composed_task_turn(
            continuation,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        fail_composed_outcome(&mut self.tasks, outcome, failure)
    }

    /// Continues a composed cancellation while preserving immutable authority and cancellation intent.
    pub fn continue_composed_cancellation_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        continuation: ComposedToolTaskCancellationContinuation<'a>,
        attempt_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskCancellationOutcome<'a>, ToolTaskRuntimeError> {
        let cancellation = continuation.cancellation.clone();
        let continuation = ComposedToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_composed_task_turn(
            continuation,
            attempt_observation_id,
            invocation_id,
            registry,
            authorizer,
        )?;
        cancel_composed_outcome(&mut self.tasks, outcome, cancellation)
    }

    /// Continues a composed correction while preserving policy, skills, and parent Attempt.
    pub fn continue_composed_correction_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        continuation: ComposedToolTaskCorrectionContinuation<'a>,
        correction_observation_id: TaskObservationId,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskCorrectionOutcome<'a>, ToolTaskRuntimeError> {
        let parent_attempt_id = continuation.parent_attempt_id.clone();
        let continuation = ComposedToolTaskContinuation {
            task_id: continuation.task_id,
            provider: continuation.provider,
        };
        let outcome = self.continue_composed_observation_task_turn(
            continuation,
            correction_observation_id,
            TaskObservationKind::Correction,
            Some(&parent_attempt_id),
            invocation_id,
            registry,
            authorizer,
        )?;
        Ok(composed_correction_outcome(outcome, parent_attempt_id))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "private composed continuation retains explicit evidence and invocation inputs"
    )]
    fn continue_composed_observation_task_turn<'a, A: ToolAuthorizer>(
        &mut self,
        continuation: ComposedToolTaskContinuation<'a>,
        observation_id: TaskObservationId,
        observation_kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
        invocation_id: ToolInvocationId,
        registry: &mut ToolRegistry,
        authorizer: &mut A,
    ) -> Result<ComposedToolTaskTurnOutcome<'a>, ToolTaskRuntimeError> {
        let task_id = continuation.task_id;
        let provider_continuation = continuation.provider;
        let context = provider_continuation.context.clone();
        let task = self.load_active_task(task_id)?;
        let session_id =
            task.session_id()
                .cloned()
                .ok_or_else(|| ToolTaskRuntimeError::TaskNotAssociated {
                    task_id: task_id.clone(),
                })?;
        let session = self
            .sessions
            .load(&session_id)
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
        if session.turns() != context.transcript {
            return Err(ToolTaskRuntimeError::StaleContinuationTranscript {
                task_id: task_id.clone(),
            });
        }
        task.validate_observation_append(&observation_id, observation_kind, parent_attempt_id)
            .map_err(ToolTaskRuntimeError::Task)?;
        self.ensure_invocation_available(&invocation_id)?;

        let response = self
            .provider
            .complete_composed_after_tool(ComposedToolAssistantContinuationRequest {
                context: provider_continuation.context,
                prior_result: provider_continuation.prior_result,
                tools: registry.metadata(),
            })
            .map_err(ToolTaskRuntimeError::Provider)?;
        let current_session = self
            .sessions
            .load(&session_id)
            .map_err(ToolTaskRuntimeError::Session)?
            .ok_or_else(|| {
                ToolTaskRuntimeError::Session(SessionStoreError::NotFound {
                    session_id: session_id.clone(),
                })
            })?;
        if current_session.status() == SessionStatus::Closed {
            return Err(ToolTaskRuntimeError::Session(
                SessionStoreError::SessionClosed {
                    session_id: session_id.clone(),
                },
            ));
        }
        if current_session.turns() != context.transcript {
            return Err(ToolTaskRuntimeError::StaleContinuationTranscript {
                task_id: task_id.clone(),
            });
        }
        let step = dispatch_provider_tool_response(
            response,
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
                let observation_text = content.as_str().to_owned();
                let session = self
                    .sessions
                    .append_turn_if_current_transcript(
                        &session_id,
                        context.transcript,
                        SessionTurnRole::Assistant,
                        content,
                    )
                    .map_err(ToolTaskRuntimeError::Session)?
                    .ok_or_else(|| ToolTaskRuntimeError::StaleContinuationTranscript {
                        task_id: task_id.clone(),
                    })?;
                let task = self.persist_final_observation(
                    task_id,
                    observation_id,
                    observation_kind,
                    parent_attempt_id,
                    observation_text,
                )?;
                Ok(ComposedToolTaskTurnOutcome::Final { session, task })
            }
            ProviderToolStepOutcome::ToolCompleted {
                tool_id,
                input,
                output,
                continuation,
            } => Ok(ComposedToolTaskTurnOutcome::ToolCompleted {
                session,
                task_id: task_id.clone(),
                tool_id,
                input,
                output,
                continuation,
                system_policy: context.system_policy,
                developer_policy: context.developer_policy,
                selected_skills: context.selected_skills,
            }),
        }
    }
}

/// Synchronous orchestration for one tool-free assistant turn.
pub struct AssistantRuntime<P> {
    sessions: SessionStore,
    tasks: TaskStore,
    provider: P,
}

impl<P> AssistantRuntime<P> {
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

impl<P: AssistantProvider> AssistantRuntime<P> {
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
}

impl<P: ComposedAssistantProvider> AssistantRuntime<P> {
    fn preflight_workflow_phase_task_turn<'a>(
        &self,
        task_id: &TaskId,
        observation_id: &TaskObservationId,
        observation_kind: TaskObservationKind,
        parent_attempt_id: Option<&TaskObservationId>,
        skill_registry: &'a SkillRegistry,
        phase: &RegisteredWorkflowPhase,
    ) -> Result<(SessionId, SkillSelection<'a>), RuntimeError> {
        let (task, session_id) = self.load_active_associated_task(task_id)?;
        task.validate_observation_append(observation_id, observation_kind, parent_attempt_id)
            .map_err(RuntimeError::Task)?;
        self.ensure_session_writable(&session_id)?;
        let selected_skills = phase
            .resolve_skills(skill_registry)
            .map_err(RuntimeError::WorkflowPhaseSkills)?;

        Ok((session_id, selected_skills))
    }

    /// Executes one tool-free turn with explicit caller policies and selected skill blocks.
    ///
    /// Selection is validated before transcript persistence. After validation, this preserves the
    /// ordinary turn contract: commit the human turn, call the provider once, then commit the
    /// assistant turn. Existing turn and task methods remain skill-free.
    pub fn execute_composed_turn(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        skill_registry: &SkillRegistry,
        selected_skill_ids: &[SkillId],
    ) -> Result<Session, RuntimeError> {
        let selected_skills = skill_registry
            .select(selected_skill_ids)
            .map_err(RuntimeError::SkillSelection)?;
        self.execute_selected_composed_turn(
            session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
        )
    }

    /// Executes one caller-selected workflow phase as a task-associated tool-free composed turn.
    ///
    /// The active associated task, fresh Attempt identity, writable session, and phase bindings are
    /// validated before transcript or provider side effects. A successful provider response is
    /// committed atomically as an assistant turn and Attempt evidence. This operation neither
    /// proves phase/run provenance nor mutates workflow lifecycle state.
    #[allow(
        clippy::too_many_arguments,
        reason = "phase task turns retain explicit task evidence, policies, registry, and phase"
    )]
    pub fn execute_workflow_phase_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        skill_registry: &SkillRegistry,
        phase: &RegisteredWorkflowPhase,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (session_id, selected_skills) = self.preflight_workflow_phase_task_turn(
            task_id,
            &attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            skill_registry,
            phase,
        )?;

        let (assistant_content, attempt_text) = self.complete_selected_composed_turn_validated(
            &session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
            |assistant_content| {
                TaskObservationText::new(assistant_content.as_str())
                    .map_err(RuntimeError::InvalidAttemptText)
            },
        )?;
        let (session, task) = self
            .tasks
            .append_attempt_with_assistant_turn(
                task_id,
                &session_id,
                attempt_observation_id,
                attempt_text,
                assistant_content,
            )
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes one caller-selected workflow phase as a task-associated correction turn.
    ///
    /// The active associated task, Correction lineage, writable session, and phase bindings are
    /// validated before transcript or provider side effects. A successful provider response is
    /// committed atomically as an assistant turn and Correction evidence linked to the exact
    /// caller-selected parent Attempt. This operation neither proves phase/run provenance nor
    /// mutates workflow lifecycle state.
    #[allow(
        clippy::too_many_arguments,
        reason = "phase correction turns retain explicit lineage, policies, registry, and phase"
    )]
    pub fn execute_workflow_phase_task_correction_turn(
        &mut self,
        task_id: &TaskId,
        parent_attempt_id: &TaskObservationId,
        human_content: SessionTurnContent,
        correction_observation_id: TaskObservationId,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        skill_registry: &SkillRegistry,
        phase: &RegisteredWorkflowPhase,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (session_id, selected_skills) = self.preflight_workflow_phase_task_turn(
            task_id,
            &correction_observation_id,
            TaskObservationKind::Correction,
            Some(parent_attempt_id),
            skill_registry,
            phase,
        )?;

        let (assistant_content, correction_text) = self.complete_selected_composed_turn_validated(
            &session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
            |assistant_content| {
                TaskObservationText::new(assistant_content.as_str())
                    .map_err(RuntimeError::InvalidCorrectionText)
            },
        )?;
        let (session, task) = self
            .tasks
            .append_correction_with_assistant_turn(
                task_id,
                &session_id,
                correction_observation_id,
                correction_text,
                parent_attempt_id.clone(),
                assistant_content,
            )
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes one caller-selected workflow phase as a caller-requested final task turn.
    ///
    /// The active associated task, fresh Attempt identity, writable session, and phase bindings are
    /// validated before transcript or provider side effects. A successful provider response is
    /// validated as both Attempt evidence and task output, then the assistant turn and Attempt are
    /// committed atomically before the task completion. This operation does not complete or mutate
    /// workflow lifecycle state and does not classify provider content as Verification evidence.
    #[allow(
        clippy::too_many_arguments,
        reason = "phase completion turns retain explicit task intent, policies, registry, and phase"
    )]
    pub fn complete_workflow_phase_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        skill_registry: &SkillRegistry,
        phase: &RegisteredWorkflowPhase,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (session_id, selected_skills) = self.preflight_workflow_phase_task_turn(
            task_id,
            &attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            skill_registry,
            phase,
        )?;

        let (assistant_content, (attempt_text, output)) = self
            .complete_selected_composed_turn_validated(
                &session_id,
                human_content,
                system_policy,
                developer_policy,
                selected_skills,
                |assistant_content| {
                    let attempt_text = TaskObservationText::new(assistant_content.as_str())
                        .map_err(RuntimeError::InvalidAttemptText)?;
                    let output = TaskOutput::new(assistant_content.as_str())
                        .map_err(RuntimeError::InvalidTaskOutput)?;
                    Ok((attempt_text, output))
                },
            )?;
        let (session, _) = self
            .tasks
            .append_attempt_with_assistant_turn(
                task_id,
                &session_id,
                attempt_observation_id,
                attempt_text,
                assistant_content,
            )
            .map_err(RuntimeError::Task)?;
        let task = self
            .tasks
            .complete(task_id, output)
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes one caller-selected workflow phase as a caller-requested task failure turn.
    ///
    /// The active associated task, fresh Attempt identity, writable session, and phase bindings are
    /// validated before transcript or provider side effects. A successful provider response is
    /// committed atomically as an assistant turn and Attempt before the task is failed with the
    /// caller-owned diagnostic. Provider content is neither Diagnostic nor Verification evidence,
    /// and this operation does not fail or otherwise mutate workflow lifecycle state.
    #[allow(
        clippy::too_many_arguments,
        reason = "phase failure turns retain explicit task intent, diagnostic, policies, registry, and phase"
    )]
    pub fn fail_workflow_phase_task_turn(
        &mut self,
        task_id: &TaskId,
        human_content: SessionTurnContent,
        attempt_observation_id: TaskObservationId,
        failure: TaskFailure,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        skill_registry: &SkillRegistry,
        phase: &RegisteredWorkflowPhase,
    ) -> Result<TaskTurnOutcome, RuntimeError> {
        let (session_id, selected_skills) = self.preflight_workflow_phase_task_turn(
            task_id,
            &attempt_observation_id,
            TaskObservationKind::Attempt,
            None,
            skill_registry,
            phase,
        )?;

        let (assistant_content, attempt_text) = self.complete_selected_composed_turn_validated(
            &session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
            |assistant_content| {
                TaskObservationText::new(assistant_content.as_str())
                    .map_err(RuntimeError::InvalidAttemptText)
            },
        )?;
        let (session, _) = self
            .tasks
            .append_attempt_with_assistant_turn(
                task_id,
                &session_id,
                attempt_observation_id,
                attempt_text,
                assistant_content,
            )
            .map_err(RuntimeError::Task)?;
        let task = self
            .tasks
            .fail(task_id, failure)
            .map_err(RuntimeError::Task)?;

        Ok(TaskTurnOutcome { session, task })
    }

    /// Executes one caller-selected workflow phase as an explicit tool-free composed turn.
    ///
    /// Phase bindings are resolved before transcript or provider side effects. This operation
    /// neither chooses nor mutates workflow lifecycle state.
    pub fn execute_workflow_phase_turn(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        skill_registry: &SkillRegistry,
        phase: &RegisteredWorkflowPhase,
    ) -> Result<Session, RuntimeError> {
        let selected_skills = phase
            .resolve_skills(skill_registry)
            .map_err(RuntimeError::WorkflowPhaseSkills)?;
        self.execute_selected_composed_turn(
            session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
        )
    }

    fn execute_selected_composed_turn(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        selected_skills: SkillSelection<'_>,
    ) -> Result<Session, RuntimeError> {
        self.execute_selected_composed_turn_validated(
            session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
            |_| Ok(()),
        )
        .map(|(session, ())| session)
    }

    fn execute_selected_composed_turn_validated<T>(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        selected_skills: SkillSelection<'_>,
        validate_assistant: impl FnOnce(&SessionTurnContent) -> Result<T, RuntimeError>,
    ) -> Result<(Session, T), RuntimeError> {
        let (assistant_content, validated) = self.complete_selected_composed_turn_validated(
            session_id,
            human_content,
            system_policy,
            developer_policy,
            selected_skills,
            validate_assistant,
        )?;
        let session = self
            .sessions
            .append_turn(session_id, SessionTurnRole::Assistant, assistant_content)
            .map_err(RuntimeError::Session)?;
        Ok((session, validated))
    }

    fn complete_selected_composed_turn_validated<T>(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
        system_policy: SystemPolicy<'_>,
        developer_policy: DeveloperPolicy<'_>,
        selected_skills: SkillSelection<'_>,
        validate_assistant: impl FnOnce(&SessionTurnContent) -> Result<T, RuntimeError>,
    ) -> Result<(SessionTurnContent, T), RuntimeError> {
        let session = self
            .sessions
            .append_turn(session_id, SessionTurnRole::Human, human_content)
            .map_err(RuntimeError::Session)?;
        let assistant_content = self
            .provider
            .complete_composed(ComposedAssistantRequest {
                system_policy,
                developer_policy,
                selected_skills,
                transcript: session.turns(),
            })
            .map_err(RuntimeError::Provider)?;
        let validated = validate_assistant(&assistant_content)?;
        Ok((assistant_content, validated))
    }
}

#[cfg(test)]
mod composed_outcome_tests {
    use super::*;

    #[test]
    fn completed_tool_chain_does_not_expose_another_continuation() {
        let skills = SkillRegistry::new();
        let outcome = ComposedProviderToolStepOutcome::ToolCompleted(ComposedToolCompleted {
            tool_id: ToolId::new("tool.done").unwrap(),
            input: Value::Null,
            output: Value::Null,
            continuation: ToolStepContinuation::Complete,
            context: ComposedToolContext {
                system_policy: SystemPolicy::new("system"),
                developer_policy: DeveloperPolicy::new("developer"),
                selected_skills: skills.select(&[]).unwrap(),
                transcript: &[],
            },
        });

        assert!(outcome.continuation().is_none());
    }
}
