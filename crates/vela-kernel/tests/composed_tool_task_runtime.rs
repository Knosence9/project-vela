use std::{cell::RefCell, rc::Rc};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        ComposedToolAssistantContinuationProvider, ComposedToolAssistantContinuationRequest,
        ComposedToolAssistantProvider, ComposedToolAssistantRequest,
        ComposedToolTaskCompletionOutcome, ComposedToolTaskCorrectionOutcome,
        ComposedToolTaskFailureOutcome, ComposedToolTaskTurnOutcome, DeveloperPolicy,
        ProviderError, ProviderToolResponse, SystemPolicy, ToolAssistantRuntime,
        ToolTaskRuntimeError,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole},
    skill::{RegisteredSkill, SkillId, SkillRegistry, SkillSelectionError},
    task::{
        TaskFailure, TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskStatus,
        TaskStore,
    },
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

#[test]
fn composed_completion_can_finish_on_the_initial_provider_response() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (_, task_id, skills) = setup(&path);
    let observations = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = ToolAssistantRuntime::open(
        &path,
        Provider {
            observations: observations.clone(),
            responses: vec![ProviderToolResponse::Final(
                SessionTurnContent::new("initial completion").unwrap(),
            )],
        },
    )
    .unwrap();

    let outcome = runtime
        .complete_composed_task_turn(
            &task_id,
            SessionTurnContent::new("finish now").unwrap(),
            TaskObservationId::new("initial-completion-attempt").unwrap(),
            ToolInvocationId::new("initial-completion-unused").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &skills,
            &[SkillId::new("skill.alpha").unwrap()],
            &mut ToolRegistry::new(),
            &mut Allow(0),
        )
        .unwrap();

    let ComposedToolTaskCompletionOutcome::Final { task, .. } = outcome else {
        panic!("expected initial composed completion")
    };
    assert_eq!(task.status(), TaskStatus::Completed);
    assert_eq!(task.output().unwrap().as_str(), "initial completion");
    assert_eq!(task.observations()[0].text().as_str(), "initial completion");
    assert_eq!(observations.borrow().len(), 1);
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("initial-completion-unused").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn composed_completion_retains_authority_and_completes_with_exact_output() {
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
            ProviderToolResponse::Final(SessionTurnContent::new("completed exactly").unwrap()),
        ],
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool).unwrap();
    let selected = [
        SkillId::new("skill.zeta").unwrap(),
        SkillId::new("skill.alpha").unwrap(),
    ];

    let first = runtime
        .complete_composed_task_turn(
            &task_id,
            SessionTurnContent::new("finish this").unwrap(),
            TaskObservationId::new("completion-attempt").unwrap(),
            ToolInvocationId::new("completion-invocation-1").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &skills,
            &selected,
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    let second = runtime
        .continue_composed_completion_task_turn(
            first.continuation().unwrap(),
            TaskObservationId::new("completion-attempt").unwrap(),
            ToolInvocationId::new("completion-invocation-2").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    let final_outcome = runtime
        .continue_composed_completion_task_turn(
            second.continuation().unwrap(),
            TaskObservationId::new("completion-attempt").unwrap(),
            ToolInvocationId::new("completion-invocation-unused").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();

    let ComposedToolTaskCompletionOutcome::Final { session, task } = final_outcome else {
        panic!("expected final composed completion outcome")
    };
    assert_eq!(session.id(), &session_id);
    assert_eq!(session.turns()[1].content().as_str(), "completed exactly");
    assert_eq!(task.status(), TaskStatus::Completed);
    assert_eq!(task.output().unwrap().as_str(), "completed exactly");
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].kind(), TaskObservationKind::Attempt);
    assert_eq!(task.observations()[0].text().as_str(), "completed exactly");
    assert_eq!(observations.borrow().len(), 3);
    assert!(observations.borrow().iter().all(|observed| {
        observed.system == "system policy"
            && observed.developer == "developer policy"
            && observed.skills
                == vec![
                    ("skill.alpha".into(), "alpha instructions".into()),
                    ("skill.zeta".into(), "zeta instructions".into()),
                ]
            && observed.transcript == vec![(SessionTurnRole::Human, "finish this".into())]
    }));
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        task
    );
}

#[test]
fn composed_failure_can_finish_initially_with_the_exact_caller_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (_, task_id, skills) = setup(&path);
    let observations = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = ToolAssistantRuntime::open(
        &path,
        Provider {
            observations: observations.clone(),
            responses: vec![ProviderToolResponse::Final(
                SessionTurnContent::new("model attempt only").unwrap(),
            )],
        },
    )
    .unwrap();
    let failure = TaskFailure::new(" caller-owned diagnostic ").unwrap();

    let outcome = runtime
        .fail_composed_task_turn(
            &task_id,
            SessionTurnContent::new("make a final attempt").unwrap(),
            TaskObservationId::new("initial-failure-attempt").unwrap(),
            failure.clone(),
            ToolInvocationId::new("initial-failure-unused").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &skills,
            &[SkillId::new("skill.alpha").unwrap()],
            &mut ToolRegistry::new(),
            &mut Allow(0),
        )
        .unwrap();

    let ComposedToolTaskFailureOutcome::Final { task, .. } = outcome else {
        panic!("expected initial composed failure")
    };
    assert_eq!(task.status(), TaskStatus::Failed);
    assert_eq!(task.failure(), Some(&failure));
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].kind(), TaskObservationKind::Attempt);
    assert_eq!(task.observations()[0].text().as_str(), "model attempt only");
    assert!(task.output().is_none());
    assert_eq!(observations.borrow().len(), 1);
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("initial-failure-unused").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn composed_failure_retains_authority_and_diagnostic_across_multiple_tools() {
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
            ProviderToolResponse::Final(SessionTurnContent::new("attempt evidence").unwrap()),
        ],
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool).unwrap();
    let selected = [
        SkillId::new("skill.zeta").unwrap(),
        SkillId::new("skill.alpha").unwrap(),
    ];
    let failure = TaskFailure::new("dependency stayed unavailable").unwrap();

    let first = runtime
        .fail_composed_task_turn(
            &task_id,
            SessionTurnContent::new("try before failing").unwrap(),
            TaskObservationId::new("failure-attempt").unwrap(),
            failure.clone(),
            ToolInvocationId::new("failure-invocation-1").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &skills,
            &selected,
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    assert_eq!(first.continuation().unwrap().failure(), &failure);
    let second = runtime
        .continue_composed_failure_task_turn(
            first.continuation().unwrap(),
            TaskObservationId::new("failure-attempt").unwrap(),
            ToolInvocationId::new("failure-invocation-2").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    assert_eq!(second.continuation().unwrap().failure(), &failure);
    let final_outcome = runtime
        .continue_composed_failure_task_turn(
            second.continuation().unwrap(),
            TaskObservationId::new("failure-attempt").unwrap(),
            ToolInvocationId::new("failure-invocation-unused").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();

    let ComposedToolTaskFailureOutcome::Final { session, task } = final_outcome else {
        panic!("expected final composed failure outcome")
    };
    assert_eq!(session.id(), &session_id);
    assert_eq!(session.turns()[1].content().as_str(), "attempt evidence");
    assert_eq!(task.status(), TaskStatus::Failed);
    assert_eq!(task.failure(), Some(&failure));
    assert!(task.output().is_none());
    assert_eq!(task.observations()[0].text().as_str(), "attempt evidence");
    assert_eq!(observations.borrow().len(), 3);
    assert!(observations.borrow().iter().all(|observed| {
        observed.system == "system policy"
            && observed.developer == "developer policy"
            && observed.skills
                == vec![
                    ("skill.alpha".into(), "alpha instructions".into()),
                    ("skill.zeta".into(), "zeta instructions".into()),
                ]
            && observed.transcript == vec![(SessionTurnRole::Human, "try before failing".into())]
    }));
    assert_eq!(observations.borrow()[0].prior, None);
    assert_eq!(
        observations.borrow()[1].prior,
        Some((
            "tool.echo".into(),
            json!({"step": 1}),
            json!({"echo": {"step": 1}}),
        ))
    );
    assert_eq!(
        observations.borrow()[2].prior,
        Some((
            "tool.echo".into(),
            json!({"step": 2}),
            json!({"echo": {"step": 2}}),
        ))
    );
}

#[test]
fn composed_failure_rejects_selection_and_stale_or_terminal_continuations_before_side_effects() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id, skills) = setup(&path);
    let observations = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = ToolAssistantRuntime::open(
        &path,
        Provider {
            observations: observations.clone(),
            responses: vec![ProviderToolResponse::ToolRequest {
                tool_id: ToolId::new("tool.echo").unwrap(),
                input: json!({"probe": true}),
            }],
        },
    )
    .unwrap();
    assert!(TaskFailure::new("").is_err());
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
    let failure = TaskFailure::new("validated diagnostic").unwrap();
    let duplicate = SkillId::new("skill.alpha").unwrap();

    let error = runtime
        .fail_composed_task_turn(
            &task_id,
            SessionTurnContent::new("must not persist").unwrap(),
            TaskObservationId::new("failure-unused").unwrap(),
            failure.clone(),
            ToolInvocationId::new("failure-selection-unused").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &skills,
            &[duplicate.clone(), duplicate],
            &mut ToolRegistry::new(),
            &mut Allow(0),
        )
        .unwrap_err();
    assert!(matches!(error, ToolTaskRuntimeError::SkillSelection(_)));
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

    let mut tools = ToolRegistry::new();
    tools.register(EchoTool).unwrap();
    let first = runtime
        .fail_composed_task_turn(
            &task_id,
            SessionTurnContent::new("persist once").unwrap(),
            TaskObservationId::new("failure-final").unwrap(),
            failure,
            ToolInvocationId::new("failure-invocation-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &skills,
            &[SkillId::new("skill.alpha").unwrap()],
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    SessionStore::open(&path)
        .unwrap()
        .append_turn(
            &session_id,
            SessionTurnRole::Human,
            SessionTurnContent::new("racing turn").unwrap(),
        )
        .unwrap();

    let stale_error = runtime
        .continue_composed_failure_task_turn(
            first.continuation().unwrap(),
            TaskObservationId::new("failure-final").unwrap(),
            ToolInvocationId::new("failure-stale-unused").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap_err();
    assert!(matches!(
        stale_error,
        ToolTaskRuntimeError::StaleContinuationTranscript { task_id: stale } if stale == task_id
    ));
    assert_eq!(observations.borrow().len(), 1);
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("failure-stale-unused").unwrap())
            .unwrap()
            .is_none()
    );

    TaskStore::open(&path)
        .unwrap()
        .fail(&task_id, TaskFailure::new("racing failure").unwrap())
        .unwrap();

    let error = runtime
        .continue_composed_failure_task_turn(
            first.continuation().unwrap(),
            TaskObservationId::new("failure-final").unwrap(),
            ToolInvocationId::new("failure-terminal-unused").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap_err();
    assert!(matches!(error, ToolTaskRuntimeError::Task(_)));
    assert_eq!(observations.borrow().len(), 1);
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("failure-terminal-unused").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn composed_correction_retains_authority_and_parent_through_tool_continuation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id, skills) = setup(&path);
    let parent_attempt_id = TaskObservationId::new("attempt-parent").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .append_observation(
            &task_id,
            parent_attempt_id.clone(),
            TaskObservationKind::Attempt,
            vela_kernel::task::TaskObservationText::new("original answer").unwrap(),
        )
        .unwrap();
    let observations = Rc::new(RefCell::new(Vec::new()));
    let provider = Provider {
        observations: observations.clone(),
        responses: vec![
            ProviderToolResponse::ToolRequest {
                tool_id: ToolId::new("tool.echo").unwrap(),
                input: json!({"fact": "check"}),
            },
            ProviderToolResponse::Final(SessionTurnContent::new("corrected answer").unwrap()),
        ],
    };
    let mut runtime = ToolAssistantRuntime::open(&path, provider).unwrap();
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool).unwrap();
    let selected = [
        SkillId::new("skill.zeta").unwrap(),
        SkillId::new("skill.alpha").unwrap(),
    ];

    let first = runtime
        .execute_composed_task_correction_turn(
            &task_id,
            &parent_attempt_id,
            SessionTurnContent::new("correct this").unwrap(),
            TaskObservationId::new("correction-final").unwrap(),
            ToolInvocationId::new("correction-invocation-1").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &skills,
            &selected,
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    let continuation = first.continuation().unwrap();
    assert_eq!(continuation.parent_attempt_id(), &parent_attempt_id);
    let final_outcome = runtime
        .continue_composed_correction_task_turn(
            continuation,
            TaskObservationId::new("correction-final").unwrap(),
            ToolInvocationId::new("correction-invocation-unused").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();

    let ComposedToolTaskCorrectionOutcome::Final { session, task } = final_outcome else {
        panic!("expected final composed correction outcome")
    };
    assert_eq!(session.id(), &session_id);
    assert_eq!(task.observations().len(), 2);
    let correction = &task.observations()[1];
    assert_eq!(correction.kind(), TaskObservationKind::Correction);
    assert_eq!(correction.text().as_str(), "corrected answer");
    assert_eq!(correction.parent_attempt_id(), Some(&parent_attempt_id));
    assert_eq!(observations.borrow().len(), 2);
    assert!(observations.borrow().iter().all(|observed| {
        observed.system == "system policy"
            && observed.developer == "developer policy"
            && observed.skills
                == vec![
                    ("skill.alpha".into(), "alpha instructions".into()),
                    ("skill.zeta".into(), "zeta instructions".into()),
                ]
            && observed.transcript == vec![(SessionTurnRole::Human, "correct this".into())]
            && observed.tools == vec![("tool.echo".into(), ToolEffect::Pure)]
    }));
    assert_eq!(observations.borrow()[0].prior, None);
    assert_eq!(
        observations.borrow()[1].prior,
        Some((
            "tool.echo".into(),
            json!({"fact": "check"}),
            json!({"echo": {"fact": "check"}}),
        ))
    );
    assert_eq!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("correction-invocation-1").unwrap())
            .unwrap()
            .unwrap()
            .status(),
        ToolInvocationStatus::Succeeded
    );
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("correction-invocation-unused").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn composed_correction_rejects_selection_and_lineage_before_side_effects() {
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
        .execute_composed_task_correction_turn(
            &task_id,
            &TaskObservationId::new("missing-parent").unwrap(),
            SessionTurnContent::new("must not persist").unwrap(),
            TaskObservationId::new("correction-unused").unwrap(),
            ToolInvocationId::new("correction-invocation-unused").unwrap(),
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

    let error = runtime
        .execute_composed_task_correction_turn(
            &task_id,
            &TaskObservationId::new("missing-parent").unwrap(),
            SessionTurnContent::new("still must not persist").unwrap(),
            TaskObservationId::new("correction-unused").unwrap(),
            ToolInvocationId::new("correction-invocation-unused").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &skills,
            std::slice::from_ref(&duplicate),
            &mut ToolRegistry::new(),
            &mut Allow(0),
        )
        .unwrap_err();
    assert!(matches!(error, ToolTaskRuntimeError::Task(_)));
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
            .load(&ToolInvocationId::new("correction-invocation-unused").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn composed_correction_rejects_a_stale_transcript_before_continuation_provider_work() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let (session_id, task_id, skills) = setup(&path);
    let parent_attempt_id = TaskObservationId::new("attempt-parent").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .append_observation(
            &task_id,
            parent_attempt_id.clone(),
            TaskObservationKind::Attempt,
            vela_kernel::task::TaskObservationText::new("original answer").unwrap(),
        )
        .unwrap();
    let observations = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = ToolAssistantRuntime::open(
        &path,
        Provider {
            observations: observations.clone(),
            responses: vec![ProviderToolResponse::ToolRequest {
                tool_id: ToolId::new("tool.echo").unwrap(),
                input: json!({"fact": "check"}),
            }],
        },
    )
    .unwrap();
    let mut tools = ToolRegistry::new();
    tools.register(EchoTool).unwrap();
    let first = runtime
        .execute_composed_task_correction_turn(
            &task_id,
            &parent_attempt_id,
            SessionTurnContent::new("correct this").unwrap(),
            TaskObservationId::new("correction-final").unwrap(),
            ToolInvocationId::new("correction-invocation-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &skills,
            &[SkillId::new("skill.alpha").unwrap()],
            &mut tools,
            &mut Allow(0),
        )
        .unwrap();
    SessionStore::open(&path)
        .unwrap()
        .append_turn(
            &session_id,
            SessionTurnRole::Human,
            SessionTurnContent::new("racing turn").unwrap(),
        )
        .unwrap();

    let error = runtime
        .continue_composed_correction_task_turn(
            first.continuation().unwrap(),
            TaskObservationId::new("correction-final").unwrap(),
            ToolInvocationId::new("correction-invocation-2").unwrap(),
            &mut tools,
            &mut Allow(0),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ToolTaskRuntimeError::StaleContinuationTranscript { task_id: stale } if stale == task_id
    ));
    assert_eq!(observations.borrow().len(), 1);
    assert!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&ToolInvocationId::new("correction-invocation-2").unwrap())
            .unwrap()
            .is_none()
    );
}
