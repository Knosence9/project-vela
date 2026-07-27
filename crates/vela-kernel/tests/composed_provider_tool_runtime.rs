use std::{cell::Cell, error::Error, fmt, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        ComposedProviderToolStepError, ComposedProviderToolStepOutcome,
        ComposedToolAssistantContinuationProvider, ComposedToolAssistantContinuationRequest,
        ComposedToolAssistantProvider, ComposedToolAssistantRequest, DeveloperPolicy,
        ProviderError, ProviderToolResponse, ProviderToolStepError, SystemPolicy,
        ToolStepContinuation, continue_composed_provider_tool_step,
        execute_composed_provider_tool_step,
    },
    session::{
        Session, SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole,
    },
    skill::{RegisteredSkill, SkillId, SkillRegistry, SkillSelectionError},
    task::{TaskGoal, TaskId, TaskStore},
    tool::{
        PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationId,
        ToolInvocationStatus, ToolInvocationStore, ToolRegistry, ToolRequest,
    },
};

#[derive(Debug)]
struct FakeFailure(&'static str);

impl fmt::Display for FakeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FakeFailure {}

#[derive(Debug, Eq, PartialEq)]
struct ObservedRequest {
    system: String,
    developer: String,
    skills: Vec<(String, String)>,
    transcript: Vec<(SessionTurnRole, String)>,
    prior_result: Option<(String, Value, Value)>,
    tools: Vec<(String, ToolEffect)>,
}

struct Provider {
    calls: Rc<Cell<usize>>,
    responses: Vec<Result<ProviderToolResponse, ProviderError>>,
    observed: Vec<ObservedRequest>,
}

impl Provider {
    fn new(
        responses: Vec<Result<ProviderToolResponse, ProviderError>>,
        calls: Rc<Cell<usize>>,
    ) -> Self {
        Self {
            calls,
            responses: responses.into_iter().rev().collect(),
            observed: Vec::new(),
        }
    }

    fn record(
        &mut self,
        request: ComposedToolAssistantRequest<'_>,
        prior_result: Option<(String, Value, Value)>,
    ) {
        self.calls.set(self.calls.get() + 1);
        self.observed.push(ObservedRequest {
            system: request.system_policy().as_str().to_owned(),
            developer: request.developer_policy().as_str().to_owned(),
            skills: request
                .skills()
                .map(|skill| {
                    (
                        skill.id().as_str().to_owned(),
                        skill.instructions().to_owned(),
                    )
                })
                .collect(),
            transcript: request
                .transcript()
                .iter()
                .map(|turn| (turn.role(), turn.content().as_str().to_owned()))
                .collect(),
            prior_result,
            tools: request
                .tools()
                .iter()
                .map(|tool| (tool.id().as_str().to_owned(), tool.effect()))
                .collect(),
        });
    }

    fn respond(&mut self) -> Result<ProviderToolResponse, ProviderError> {
        self.responses
            .pop()
            .expect("provider called too many times")
    }
}

impl ComposedToolAssistantProvider for Provider {
    fn complete_composed_with_tools(
        &mut self,
        request: ComposedToolAssistantRequest<'_>,
    ) -> Result<ProviderToolResponse, ProviderError> {
        self.record(request, None);
        self.respond()
    }
}

impl ComposedToolAssistantContinuationProvider for Provider {
    fn complete_composed_after_tool(
        &mut self,
        request: ComposedToolAssistantContinuationRequest<'_>,
    ) -> Result<ProviderToolResponse, ProviderError> {
        let prior = request.prior_result();
        let observed_prior = Some((
            prior.tool_id().as_str().to_owned(),
            prior.input().clone(),
            prior.output().clone(),
        ));
        self.record(request.request(), observed_prior);
        self.respond()
    }
}

struct FakeTool {
    id: ToolId,
    calls: Rc<Cell<usize>>,
}

impl Tool for FakeTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }

    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        self.calls.set(self.calls.get() + 1);
        Ok(json!({"echo": input}))
    }
}

struct Authorizer {
    calls: usize,
    decision: PermissionDecision,
}

impl ToolAuthorizer for Authorizer {
    fn authorize(&mut self, _request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        self.decision
    }
}

fn authorizer(decision: PermissionDecision) -> Authorizer {
    Authorizer { calls: 0, decision }
}

fn skill_registry() -> SkillRegistry {
    let mut registry = SkillRegistry::new();
    registry
        .register_all([
            RegisteredSkill::new(SkillId::new("skill.zeta").unwrap(), "exact zeta"),
            RegisteredSkill::new(SkillId::new("skill.alpha").unwrap(), "exact alpha"),
            RegisteredSkill::new(SkillId::new("skill.unselected").unwrap(), "must stay inert"),
        ])
        .unwrap();
    registry
}

fn active_task(path: &std::path::Path) -> TaskId {
    let task_id = TaskId::new("task-1").unwrap();
    TaskStore::open(path)
        .unwrap()
        .start(
            task_id.clone(),
            TaskGoal::new("use composed tools").unwrap(),
        )
        .unwrap();
    task_id
}

fn session_with_human_turn(path: &std::path::Path, content: &str) -> Session {
    let mut store = SessionStore::open(path).unwrap();
    let session_id = SessionId::new("provider-context").unwrap();
    store
        .create(
            session_id.clone(),
            SessionTitle::new("Provider context").unwrap(),
        )
        .unwrap();
    store
        .append_turn(
            &session_id,
            SessionTurnRole::Human,
            SessionTurnContent::new(content).unwrap(),
        )
        .unwrap()
}

#[test]
fn skill_selection_failures_precede_provider_authorization_and_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = Provider::new(
        vec![Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("unused").unwrap(),
        ))],
        calls.clone(),
    );
    let skills = skill_registry();
    let duplicate = SkillId::new("skill.alpha").unwrap();
    let mut tools = ToolRegistry::new();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("not-written").unwrap();
    let mut authorizer = authorizer(PermissionDecision::Allow);

    let error = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("system"),
        DeveloperPolicy::new("developer"),
        &skills,
        &[duplicate.clone(), duplicate.clone()],
        &[],
        &mut tools,
        &mut store,
        &TaskId::new("not-consulted").unwrap(),
        invocation_id.clone(),
        &mut authorizer,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ComposedProviderToolStepError::SkillSelection(SkillSelectionError::DuplicateId {
            skill_id
        }) if skill_id == duplicate
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(authorizer.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());

    let missing = SkillId::new("skill.missing").unwrap();
    let missing_invocation_id = ToolInvocationId::new("also-not-written").unwrap();
    let error = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("system"),
        DeveloperPolicy::new("developer"),
        &skills,
        std::slice::from_ref(&missing),
        &[],
        &mut tools,
        &mut store,
        &TaskId::new("not-consulted").unwrap(),
        missing_invocation_id.clone(),
        &mut authorizer,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ComposedProviderToolStepError::SkillSelection(SkillSelectionError::MissingId {
            skill_id
        }) if skill_id == missing
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(authorizer.calls, 0);
    assert!(store.load(&missing_invocation_id).unwrap().is_none());
}

#[test]
fn initial_final_request_keeps_typed_fields_distinct_and_skips_tool_dispatch() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = Provider::new(
        vec![Ok(ProviderToolResponse::Final(
            SessionTurnContent::new("finished").unwrap(),
        ))],
        calls.clone(),
    );
    let skills = skill_registry();
    let selected = [
        SkillId::new("skill.zeta").unwrap(),
        SkillId::new("skill.alpha").unwrap(),
    ];
    let session = session_with_human_turn(&path, "hello");
    let transcript = session.turns();
    let mut tools = ToolRegistry::new();
    let tool_calls = Rc::new(Cell::new(0));
    tools
        .register(FakeTool {
            id: ToolId::new("tool.echo").unwrap(),
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("unused").unwrap();
    let mut authorizer = authorizer(PermissionDecision::Allow);

    let outcome = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("system policy"),
        DeveloperPolicy::new("developer policy"),
        &skills,
        &selected,
        transcript,
        &mut tools,
        &mut store,
        &TaskId::new("not-consulted").unwrap(),
        invocation_id.clone(),
        &mut authorizer,
    )
    .unwrap();

    assert!(matches!(
        outcome,
        ComposedProviderToolStepOutcome::Final {
            ref content,
            continuation: ToolStepContinuation::Complete,
        } if content.as_str() == "finished"
    ));
    assert_eq!(calls.get(), 1);
    assert_eq!(tool_calls.get(), 0);
    assert_eq!(authorizer.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());
    assert_eq!(
        provider.observed,
        vec![ObservedRequest {
            system: "system policy".into(),
            developer: "developer policy".into(),
            skills: vec![
                ("skill.alpha".into(), "exact alpha".into()),
                ("skill.zeta".into(), "exact zeta".into()),
            ],
            transcript: vec![(SessionTurnRole::Human, "hello".into())],
            prior_result: None,
            tools: vec![("tool.echo".into(), ToolEffect::Pure)],
        }]
    );
}

#[test]
fn continuation_retains_composition_and_requires_fresh_tool_authorization() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = active_task(&path);
    let calls = Rc::new(Cell::new(0));
    let tool_id = ToolId::new("tool.echo").unwrap();
    let mut provider = Provider::new(
        vec![
            Ok(ProviderToolResponse::ToolRequest {
                tool_id: tool_id.clone(),
                input: json!({"step": 1}),
            }),
            Ok(ProviderToolResponse::ToolRequest {
                tool_id: tool_id.clone(),
                input: json!({"step": 2}),
            }),
        ],
        calls.clone(),
    );
    let skills = skill_registry();
    let selected = [SkillId::new("skill.alpha").unwrap()];
    let session = session_with_human_turn(&path, "chain");
    let transcript = session.turns();
    let tool_calls = Rc::new(Cell::new(0));
    let mut tools = ToolRegistry::new();
    tools
        .register(FakeTool {
            id: tool_id.clone(),
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let first_id = ToolInvocationId::new("first").unwrap();
    let second_id = ToolInvocationId::new("second").unwrap();
    let mut first_authorizer = authorizer(PermissionDecision::Allow);

    let first = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("fixed system"),
        DeveloperPolicy::new("fixed developer"),
        &skills,
        &selected,
        transcript,
        &mut tools,
        &mut store,
        &task_id,
        first_id.clone(),
        &mut first_authorizer,
    )
    .unwrap();
    let continuation = first.continuation().expect("tool continuation");
    tools
        .register(FakeTool {
            id: ToolId::new("tool.added").unwrap(),
            calls: Rc::new(Cell::new(0)),
        })
        .unwrap();
    let mut second_authorizer = authorizer(PermissionDecision::Allow);
    let second = continue_composed_provider_tool_step(
        &mut provider,
        continuation,
        &mut tools,
        &mut store,
        &task_id,
        second_id.clone(),
        &mut second_authorizer,
    )
    .unwrap();

    assert!(matches!(
        second,
        ComposedProviderToolStepOutcome::ToolCompleted(ref completed)
            if completed.tool_id() == &tool_id
                && completed.input() == &json!({"step": 2})
                && completed.output() == &json!({"echo": {"step": 2}})
                && completed.continuation() == ToolStepContinuation::ProviderRequired
    ));
    assert_eq!(calls.get(), 2);
    assert_eq!(tool_calls.get(), 2);
    assert_eq!(first_authorizer.calls, 1);
    assert_eq!(second_authorizer.calls, 1);
    let first_invocation = store.load(&first_id).unwrap().unwrap();
    assert_eq!(first_invocation.status(), ToolInvocationStatus::Succeeded);
    assert_eq!(first_invocation.task_id(), Some(&task_id));
    let second_invocation = store.load(&second_id).unwrap().unwrap();
    assert_eq!(second_invocation.status(), ToolInvocationStatus::Succeeded);
    assert_eq!(second_invocation.task_id(), Some(&task_id));
    assert_eq!(provider.observed[0].system, "fixed system");
    assert_eq!(provider.observed[1].system, "fixed system");
    assert_eq!(provider.observed[0].developer, "fixed developer");
    assert_eq!(provider.observed[1].developer, "fixed developer");
    assert_eq!(provider.observed[0].skills, provider.observed[1].skills);
    assert_eq!(
        provider.observed[0].transcript,
        provider.observed[1].transcript
    );
    assert_eq!(
        provider.observed[0].tools,
        vec![("tool.echo".into(), ToolEffect::Pure)]
    );
    assert_eq!(
        provider.observed[1].tools,
        vec![
            ("tool.added".into(), ToolEffect::Pure),
            ("tool.echo".into(), ToolEffect::Pure),
        ]
    );
    assert_eq!(
        provider.observed[1].prior_result,
        Some((
            "tool.echo".into(),
            json!({"step": 1}),
            json!({"echo": {"step": 1}}),
        ))
    );
}

#[test]
fn denial_blocks_composed_tool_invocation_and_records_no_success() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = active_task(&path);
    let provider_calls = Rc::new(Cell::new(0));
    let tool_id = ToolId::new("tool.echo").unwrap();
    let mut provider = Provider::new(
        vec![Ok(ProviderToolResponse::ToolRequest {
            tool_id: tool_id.clone(),
            input: json!({"denied": true}),
        })],
        provider_calls.clone(),
    );
    let skills = skill_registry();
    let tool_calls = Rc::new(Cell::new(0));
    let mut tools = ToolRegistry::new();
    tools
        .register(FakeTool {
            id: tool_id,
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("denied-composed").unwrap();
    let mut deny = authorizer(PermissionDecision::Deny);

    let error = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("system"),
        DeveloperPolicy::new("developer"),
        &skills,
        &[SkillId::new("skill.alpha").unwrap()],
        &[],
        &mut tools,
        &mut store,
        &task_id,
        invocation_id.clone(),
        &mut deny,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ComposedProviderToolStepError::Step(ProviderToolStepError::Invocation(_))
    ));
    assert_eq!(provider_calls.get(), 1);
    assert_eq!(deny.calls, 1);
    assert_eq!(tool_calls.get(), 0);
    assert_eq!(
        store.load(&invocation_id).unwrap().unwrap().status(),
        ToolInvocationStatus::Denied
    );
}

#[test]
fn continuation_provider_failure_preserves_source_and_skips_next_dispatch() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let task_id = active_task(&path);
    let calls = Rc::new(Cell::new(0));
    let tool_id = ToolId::new("tool.echo").unwrap();
    let mut provider = Provider::new(
        vec![
            Ok(ProviderToolResponse::ToolRequest {
                tool_id: tool_id.clone(),
                input: json!({"step": 1}),
            }),
            Err(ProviderError::new(FakeFailure("continuation failed"))),
        ],
        calls.clone(),
    );
    let skills = skill_registry();
    let tool_calls = Rc::new(Cell::new(0));
    let mut tools = ToolRegistry::new();
    tools
        .register(FakeTool {
            id: tool_id,
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let first_id = ToolInvocationId::new("completed-first").unwrap();
    let next_id = ToolInvocationId::new("not-written-next").unwrap();
    let mut first_authorizer = authorizer(PermissionDecision::Allow);
    let first = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("system"),
        DeveloperPolicy::new("developer"),
        &skills,
        &[SkillId::new("skill.alpha").unwrap()],
        &[],
        &mut tools,
        &mut store,
        &task_id,
        first_id.clone(),
        &mut first_authorizer,
    )
    .unwrap();
    let mut next_authorizer = authorizer(PermissionDecision::Allow);

    let error = continue_composed_provider_tool_step(
        &mut provider,
        first.continuation().unwrap(),
        &mut tools,
        &mut store,
        &task_id,
        next_id.clone(),
        &mut next_authorizer,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ComposedProviderToolStepError::Step(ProviderToolStepError::Provider(_))
    ));
    assert_eq!(
        error.source().unwrap().source().unwrap().to_string(),
        "continuation failed"
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(tool_calls.get(), 1);
    assert_eq!(first_authorizer.calls, 1);
    assert_eq!(next_authorizer.calls, 0);
    assert_eq!(
        store.load(&first_id).unwrap().unwrap().status(),
        ToolInvocationStatus::Succeeded
    );
    assert!(store.load(&next_id).unwrap().is_none());
}

#[test]
fn composed_provider_failure_preserves_source_and_skips_dispatch() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let calls = Rc::new(Cell::new(0));
    let mut provider = Provider::new(
        vec![Err(ProviderError::new(FakeFailure("provider failed")))],
        calls.clone(),
    );
    let skills = skill_registry();
    let mut tools = ToolRegistry::new();
    let tool_calls = Rc::new(Cell::new(0));
    tools
        .register(FakeTool {
            id: ToolId::new("tool.echo").unwrap(),
            calls: tool_calls.clone(),
        })
        .unwrap();
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let invocation_id = ToolInvocationId::new("not-written").unwrap();
    let mut authorizer = authorizer(PermissionDecision::Allow);

    let error = execute_composed_provider_tool_step(
        &mut provider,
        SystemPolicy::new("system"),
        DeveloperPolicy::new("developer"),
        &skills,
        &[],
        &[],
        &mut tools,
        &mut store,
        &TaskId::new("not-consulted").unwrap(),
        invocation_id.clone(),
        &mut authorizer,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ComposedProviderToolStepError::Step(ProviderToolStepError::Provider(_))
    ));
    assert_eq!(
        error.source().unwrap().source().unwrap().to_string(),
        "provider failed"
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(tool_calls.get(), 0);
    assert_eq!(authorizer.calls, 0);
    assert!(store.load(&invocation_id).unwrap().is_none());
}
