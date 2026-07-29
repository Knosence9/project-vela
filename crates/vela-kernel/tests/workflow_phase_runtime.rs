use std::{cell::RefCell, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        AssistantRuntime, ComposedAssistantProvider, ComposedAssistantRequest, DeveloperPolicy,
        ProviderError, RuntimeError, SystemPolicy,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurnContent},
    skill::{RegisteredSkill, SkillId, SkillRegistry},
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
