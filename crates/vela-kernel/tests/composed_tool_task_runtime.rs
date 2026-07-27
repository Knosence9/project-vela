use std::{cell::RefCell, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        ComposedToolAssistantContinuationProvider, ComposedToolAssistantContinuationRequest,
        ComposedToolAssistantProvider, ComposedToolAssistantRequest, ComposedToolTaskTurnOutcome,
        DeveloperPolicy, ProviderError, ProviderToolResponse, SystemPolicy, ToolAssistantRuntime,
        ToolTaskRuntimeError,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole},
    skill::{RegisteredSkill, SkillId, SkillRegistry, SkillSelectionError},
    task::{TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskStore},
    tool::{
        PermissionDecision, Tool, ToolAuthorizer, ToolEffect, ToolError, ToolId, ToolInvocationId,
        ToolInvocationStatus, ToolInvocationStore, ToolRegistry, ToolRequest,
    },
};

#[derive(Debug, Eq, PartialEq)]
struct Observed {
    system: String,
    developer: String,
    skills: Vec<(String, String)>,
    transcript: Vec<(SessionTurnRole, String)>,
    tools: Vec<(String, ToolEffect)>,
    prior: Option<(String, Value, Value)>,
}

struct Provider {
    observations: Rc<RefCell<Vec<Observed>>>,
    responses: Vec<ProviderToolResponse>,
}

impl Provider {
    fn record(
        &mut self,
        request: ComposedToolAssistantRequest<'_>,
        prior: Option<(String, Value, Value)>,
    ) -> Result<ProviderToolResponse, ProviderError> {
        self.observations.borrow_mut().push(Observed {
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
            tools: request
                .tools()
                .iter()
                .map(|tool| (tool.id().as_str().to_owned(), tool.effect()))
                .collect(),
            prior,
        });
        Ok(self.responses.remove(0))
    }
}

impl ComposedToolAssistantProvider for Provider {
    fn complete_composed_with_tools(
        &mut self,
        request: ComposedToolAssistantRequest<'_>,
    ) -> Result<ProviderToolResponse, ProviderError> {
        self.record(request, None)
    }
}

impl ComposedToolAssistantContinuationProvider for Provider {
    fn complete_composed_after_tool(
        &mut self,
        request: ComposedToolAssistantContinuationRequest<'_>,
    ) -> Result<ProviderToolResponse, ProviderError> {
        let prior = request.prior_result();
        self.record(
            request.request(),
            Some((
                prior.tool_id().as_str().to_owned(),
                prior.input().clone(),
                prior.output().clone(),
            )),
        )
    }
}

struct EchoTool;
impl Tool for EchoTool {
    fn id(&self) -> &ToolId {
        static ID: std::sync::OnceLock<ToolId> = std::sync::OnceLock::new();
        ID.get_or_init(|| ToolId::new("tool.echo").unwrap())
    }
    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }
    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        Ok(json!({"echo": input}))
    }
}

struct Allow(usize);
impl ToolAuthorizer for Allow {
    fn authorize(&mut self, _request: ToolRequest<'_>) -> PermissionDecision {
        self.0 += 1;
        PermissionDecision::Allow
    }
}

fn setup(path: &std::path::Path) -> (SessionId, TaskId, SkillRegistry) {
    let session_id = SessionId::new("session-1").unwrap();
    let task_id = TaskId::new("task-1").unwrap();
    SessionStore::open(path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Composed task").unwrap(),
        )
        .unwrap();
    let mut tasks = TaskStore::open(path).unwrap();
    tasks
        .start(
            task_id.clone(),
            TaskGoal::new("use skills and tools").unwrap(),
        )
        .unwrap();
    tasks.associate_session(&task_id, &session_id).unwrap();
    let mut skills = SkillRegistry::new();
    skills
        .register_all([
            RegisteredSkill::new(SkillId::new("skill.zeta").unwrap(), "zeta instructions"),
            RegisteredSkill::new(SkillId::new("skill.alpha").unwrap(), "alpha instructions"),
            RegisteredSkill::new(SkillId::new("skill.inert").unwrap(), "must stay absent"),
        ])
        .unwrap();
    (session_id, task_id, skills)
}

#[test]
fn selection_failure_precedes_durable_or_provider_side_effects() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id, skills) = setup(&path);
    let observations = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = ToolAssistantRuntime::open(
        &path,
        Provider {
            observations: observations.clone(),
            responses: vec![],
        },
    )
    .unwrap();
    let duplicate = SkillId::new("skill.alpha").unwrap();

    let error = runtime
        .execute_composed_task_turn(
            &task_id,
            SessionTurnContent::new("must not persist").unwrap(),
            TaskObservationId::new("attempt-unused").unwrap(),
            ToolInvocationId::new("invocation-unused").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &skills,
            &[duplicate.clone(), duplicate.clone()],
            &mut ToolRegistry::new(),
            &mut Allow(0),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ToolTaskRuntimeError::SkillSelection(SkillSelectionError::DuplicateId { skill_id })
            if skill_id == duplicate
    ));

    let missing = SkillId::new("skill.missing").unwrap();
    let error = runtime
        .execute_composed_task_turn(
            &task_id,
            SessionTurnContent::new("also must not persist").unwrap(),
            TaskObservationId::new("attempt-also-unused").unwrap(),
            ToolInvocationId::new("invocation-also-unused").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &skills,
            std::slice::from_ref(&missing),
            &mut ToolRegistry::new(),
            &mut Allow(0),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ToolTaskRuntimeError::SkillSelection(SkillSelectionError::MissingId { skill_id })
            if skill_id == missing
    ));
    assert!(observations.borrow().is_empty());
    assert!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap()
            .turns()
            .is_empty()
    );
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("invocation-unused").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn composed_task_turn_retains_authority_through_tool_continuation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id, skills) = setup(&path);
    let observations = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        observations: observations.clone(),
        responses: vec![
            ProviderToolResponse::ToolRequest {
                tool_id: ToolId::new("tool.echo").unwrap(),
                input: json!({"step": 1}),
            },
            ProviderToolResponse::ToolRequest {
                tool_id: ToolId::new("tool.echo").unwrap(),
                input: json!({"step": 2}),
            },
            ProviderToolResponse::Final(SessionTurnContent::new("final answer").unwrap()),
        ],
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool).unwrap();
    let mut first_authorizer = Allow(0);
    let selected = [
        SkillId::new("skill.zeta").unwrap(),
        SkillId::new("skill.alpha").unwrap(),
    ];

    let first = runtime
        .execute_composed_task_turn(
            &task_id,
            SessionTurnContent::new("question").unwrap(),
            TaskObservationId::new("attempt-final").unwrap(),
            ToolInvocationId::new("invocation-1").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &skills,
            &selected,
            &mut tools,
            &mut first_authorizer,
        )
        .unwrap();
    assert_eq!(first_authorizer.0, 1);
    assert!(matches!(
        first,
        ComposedToolTaskTurnOutcome::ToolCompleted { .. }
    ));
    let continuation = first.continuation().unwrap();
    let mut second_authorizer = Allow(0);
    let second = runtime
        .continue_composed_task_turn(
            continuation,
            TaskObservationId::new("attempt-final").unwrap(),
            ToolInvocationId::new("invocation-2").unwrap(),
            &mut tools,
            &mut second_authorizer,
        )
        .unwrap();
    assert_eq!(second_authorizer.0, 1);
    let continuation = second.continuation().unwrap();
    let mut final_authorizer = Allow(0);
    let final_outcome = runtime
        .continue_composed_task_turn(
            continuation,
            TaskObservationId::new("attempt-final").unwrap(),
            ToolInvocationId::new("invocation-3-unused").unwrap(),
            &mut tools,
            &mut final_authorizer,
        )
        .unwrap();

    let ComposedToolTaskTurnOutcome::Final { session, task } = final_outcome else {
        panic!("expected final composed task outcome")
    };
    assert_eq!(final_authorizer.0, 0);
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].content().as_str(), "final answer");
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].kind(), TaskObservationKind::Attempt);
    assert_eq!(task.observations()[0].text().as_str(), "final answer");
    assert_eq!(
        observations.borrow().as_slice(),
        &[
            Observed {
                system: "system policy".into(),
                developer: "developer policy".into(),
                skills: vec![
                    ("skill.alpha".into(), "alpha instructions".into()),
                    ("skill.zeta".into(), "zeta instructions".into()),
                ],
                transcript: vec![(SessionTurnRole::Human, "question".into())],
                tools: vec![("tool.echo".into(), ToolEffect::Pure)],
                prior: None,
            },
            Observed {
                system: "system policy".into(),
                developer: "developer policy".into(),
                skills: vec![
                    ("skill.alpha".into(), "alpha instructions".into()),
                    ("skill.zeta".into(), "zeta instructions".into()),
                ],
                transcript: vec![(SessionTurnRole::Human, "question".into())],
                tools: vec![("tool.echo".into(), ToolEffect::Pure)],
                prior: Some((
                    "tool.echo".into(),
                    json!({"step": 1}),
                    json!({"echo": {"step": 1}}),
                )),
            },
            Observed {
                system: "system policy".into(),
                developer: "developer policy".into(),
                skills: vec![
                    ("skill.alpha".into(), "alpha instructions".into()),
                    ("skill.zeta".into(), "zeta instructions".into()),
                ],
                transcript: vec![(SessionTurnRole::Human, "question".into())],
                tools: vec![("tool.echo".into(), ToolEffect::Pure)],
                prior: Some((
                    "tool.echo".into(),
                    json!({"step": 2}),
                    json!({"echo": {"step": 2}}),
                )),
            },
        ]
    );
    assert_eq!(
        SessionStore::open(&path)
            .unwrap()
            .load(&session_id)
            .unwrap()
            .unwrap(),
        session
    );
    assert_eq!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("invocation-1").unwrap())
            .unwrap()
            .unwrap()
            .status(),
        ToolInvocationStatus::Succeeded
    );
    assert_eq!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("invocation-2").unwrap())
            .unwrap()
            .unwrap()
            .status(),
        ToolInvocationStatus::Succeeded
    );
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("invocation-3-unused").unwrap())
            .unwrap()
            .is_none()
    );
}
