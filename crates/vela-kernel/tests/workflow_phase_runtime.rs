use std::{cell::RefCell, error::Error, fmt, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        AssistantRuntime, ComposedAssistantProvider, ComposedAssistantRequest, DeveloperPolicy,
        ProviderError, RuntimeError, SystemPolicy,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurnContent},
    skill::{RegisteredSkill, SkillId, SkillRegistry},
    task::{
        TaskGoal, TaskId, TaskObservationId, TaskObservationKind, TaskObservationText, TaskStore,
        TaskStoreError,
    },
    workflow::{RegisteredWorkflowPhase, WorkflowPhaseSkillResolutionError},
};

struct RecordingProvider {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
}

impl ComposedAssistantProvider for RecordingProvider {
    fn complete_composed(
        &mut self,
        request: ComposedAssistantRequest<'_>,
    ) -> Result<SessionTurnContent, ProviderError> {
        self.calls.borrow_mut().push(
            request
                .skills()
                .map(|skill| skill.id().as_str().to_owned())
                .collect(),
        );
        Ok(SessionTurnContent::new("phase answer").unwrap())
    }
}

fn registry(ids: &[&str]) -> SkillRegistry {
    let mut registry = SkillRegistry::new();
    registry
        .register_all(ids.iter().map(|id| {
            RegisteredSkill::new(SkillId::new(*id).unwrap(), format!("instructions for {id}"))
        }))
        .unwrap();
    registry
}

fn create_session(path: &std::path::Path, id: &SessionId) {
    SessionStore::open(path)
        .unwrap()
        .create(id.clone(), SessionTitle::new("Workflow phase").unwrap())
        .unwrap();
}

fn create_associated_task(path: &std::path::Path, task_id: &TaskId, session_id: &SessionId) {
    create_session(path, session_id);
    let mut tasks = TaskStore::open(path).unwrap();
    tasks
        .start(task_id.clone(), TaskGoal::new("review carefully").unwrap())
        .unwrap();
    tasks.associate_session(task_id, session_id).unwrap();
}

#[test]
fn explicitly_executes_one_caller_selected_phase_with_only_its_bound_skills() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-turn").unwrap();
    create_session(&path, &session_id);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::clone(&calls),
        },
    )
    .unwrap();
    let registry = registry(&["unused.skill", "zeta.skill", "alpha.skill"]);
    let phase = RegisteredWorkflowPhase::new("review", false, vec![])
        .with_skills(["zeta.skill", "alpha.skill"]);

    runtime
        .execute_workflow_phase_turn(
            &session_id,
            SessionTurnContent::new("review this").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry,
            &phase,
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec!["alpha.skill", "zeta.skill"]]
    );
    let persisted = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.turns().len(), 2);
    assert_eq!(persisted.turns()[1].content().as_str(), "phase answer");
    assert_eq!(phase.skills(), ["zeta.skill", "alpha.skill"]);
}

#[test]
fn phase_resolution_failures_precede_transcript_and_provider_side_effects() {
    for (case, phase) in [
        (
            "missing",
            RegisteredWorkflowPhase::new("review", false, vec![]).with_skills(["missing.skill"]),
        ),
        (
            "malformed",
            RegisteredWorkflowPhase::new("review", false, vec![]).with_skills(["  "]),
        ),
        (
            "terminal",
            RegisteredWorkflowPhase::new("done", true, vec![]).with_skills(["present.skill"]),
        ),
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new(format!("phase-{case}")).unwrap();
        create_session(&path, &session_id);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = AssistantRuntime::open(
            &path,
            RecordingProvider {
                calls: Rc::clone(&calls),
            },
        )
        .unwrap();

        let error = runtime
            .execute_workflow_phase_turn(
                &session_id,
                SessionTurnContent::new("must not persist").unwrap(),
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry(&["present.skill"]),
                &phase,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            RuntimeError::WorkflowPhaseSkills(
                WorkflowPhaseSkillResolutionError::InvalidId { .. }
                    | WorkflowPhaseSkillResolutionError::Selection(_)
                    | WorkflowPhaseSkillResolutionError::TerminalHasBindings { .. }
            )
        ));
        assert!(calls.borrow().is_empty());
        assert!(
            SessionStore::open(&path)
                .unwrap()
                .load(&session_id)
                .unwrap()
                .unwrap()
                .turns()
                .is_empty()
        );
    }
}

#[test]
fn records_a_selected_workflow_phase_response_as_task_attempt_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-task-session").unwrap();
    let task_id = TaskId::new("phase-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::clone(&calls),
        },
    )
    .unwrap();
    let phase = RegisteredWorkflowPhase::new("review", false, vec![])
        .with_skills(["zeta.skill", "alpha.skill"]);

    let outcome = runtime
        .execute_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("review this").unwrap(),
            TaskObservationId::new("phase-attempt-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["zeta.skill", "unused.skill", "alpha.skill"]),
            &phase,
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec!["alpha.skill", "zeta.skill"]]
    );
    assert_eq!(outcome.session().turns().len(), 2);
    assert_eq!(outcome.task().observations().len(), 1);
    let attempt = &outcome.task().observations()[0];
    assert_eq!(attempt.id().as_str(), "phase-attempt-1");
    assert_eq!(attempt.kind(), TaskObservationKind::Attempt);
    assert_eq!(attempt.text().as_str(), "phase answer");
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap(),
        *outcome.task()
    );
}

#[test]
fn task_attempt_and_phase_preflight_fail_before_transcript_or_provider_side_effects() {
    for case in [
        "unassociated-task",
        "duplicate-attempt",
        "missing-skill",
        "terminal-binding",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new(format!("{case}-session")).unwrap();
        let task_id = TaskId::new(format!("{case}-task")).unwrap();
        if case == "unassociated-task" {
            create_session(&path, &session_id);
            TaskStore::open(&path)
                .unwrap()
                .start(task_id.clone(), TaskGoal::new("unassociated task").unwrap())
                .unwrap();
        } else {
            create_associated_task(&path, &task_id, &session_id);
        }
        let attempt_id = TaskObservationId::new("attempt-1").unwrap();
        if case == "duplicate-attempt" {
            TaskStore::open(&path)
                .unwrap()
                .append_observation(
                    &task_id,
                    attempt_id.clone(),
                    TaskObservationKind::Attempt,
                    TaskObservationText::new("existing attempt").unwrap(),
                )
                .unwrap();
        }
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = AssistantRuntime::open(
            &path,
            RecordingProvider {
                calls: Rc::clone(&calls),
            },
        )
        .unwrap();
        let phase =
            match case {
                "missing-skill" => RegisteredWorkflowPhase::new("review", false, vec![])
                    .with_skills(["missing.skill"]),
                "terminal-binding" => RegisteredWorkflowPhase::new("done", true, vec![])
                    .with_skills(["present.skill"]),
                _ => RegisteredWorkflowPhase::new("review", false, vec![])
                    .with_skills(["present.skill"]),
            };

        let error = runtime
            .execute_workflow_phase_task_turn(
                &task_id,
                SessionTurnContent::new("must not persist").unwrap(),
                attempt_id,
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry(&["present.skill"]),
                &phase,
            )
            .unwrap_err();

        match case {
            "unassociated-task" => {
                assert!(matches!(error, RuntimeError::TaskNotAssociated { .. }))
            }
            "duplicate-attempt" => assert!(matches!(
                error,
                RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
            )),
            _ => assert!(matches!(error, RuntimeError::WorkflowPhaseSkills(_))),
        }
        assert!(calls.borrow().is_empty());
        assert!(
            SessionStore::open(&path)
                .unwrap()
                .load(&session_id)
                .unwrap()
                .unwrap()
                .turns()
                .is_empty()
        );
    }
}

#[derive(Debug)]
struct ProviderUnavailable;

impl fmt::Display for ProviderUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider unavailable")
    }
}

impl Error for ProviderUnavailable {}

struct FailingProvider;

impl ComposedAssistantProvider for FailingProvider {
    fn complete_composed(
        &mut self,
        _request: ComposedAssistantRequest<'_>,
    ) -> Result<SessionTurnContent, ProviderError> {
        Err(ProviderError::new(ProviderUnavailable))
    }
}

struct BlankProvider;

impl ComposedAssistantProvider for BlankProvider {
    fn complete_composed(
        &mut self,
        _request: ComposedAssistantRequest<'_>,
    ) -> Result<SessionTurnContent, ProviderError> {
        Ok(SessionTurnContent::new(" \n ").unwrap())
    }
}

#[test]
fn blank_workflow_phase_task_response_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("blank-phase-task-session").unwrap();
    let task_id = TaskId::new("blank-phase-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let mut runtime = AssistantRuntime::open(&path, BlankProvider).unwrap();

    let error = runtime
        .execute_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("persist this question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["review.skill"]),
            &RegisteredWorkflowPhase::new("review", false, vec![]).with_skills(["review.skill"]),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::InvalidAttemptText(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .observations()
            .is_empty()
    );
}

#[test]
fn workflow_phase_task_provider_failure_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("failed-phase-task-session").unwrap();
    let task_id = TaskId::new("failed-phase-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let mut runtime = AssistantRuntime::open(&path, FailingProvider).unwrap();

    let error = runtime
        .execute_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("persist this question").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["review.skill"]),
            &RegisteredWorkflowPhase::new("review", false, vec![]).with_skills(["review.skill"]),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .observations()
            .is_empty()
    );
}
