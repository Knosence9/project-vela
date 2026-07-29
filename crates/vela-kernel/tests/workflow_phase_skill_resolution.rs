use tempfile::tempdir;
use vela_kernel::{
    skill::{RegisteredSkill, SkillId, SkillRegistry, SkillSelectionError},
    workflow::{
        RegisteredWorkflow, RegisteredWorkflowPhase, RegisteredWorkflowTransition, WorkflowCursor,
        WorkflowId, WorkflowPhaseSkillResolutionError, WorkflowRunId, WorkflowRunStore,
    },
};

fn skill(id: &str) -> RegisteredSkill {
    RegisteredSkill::new(SkillId::new(id).unwrap(), format!("instructions for {id}"))
}

fn registry(ids: &[&str]) -> SkillRegistry {
    let mut registry = SkillRegistry::new();
    registry
        .register_all(ids.iter().map(|id| skill(id)))
        .unwrap();
    registry
}

fn workflow(skills: &[&str]) -> RegisteredWorkflow {
    RegisteredWorkflow::new(
        WorkflowId::new("review.workflow").unwrap(),
        "review",
        vec![
            RegisteredWorkflowPhase::new(
                "review",
                false,
                vec![RegisteredWorkflowTransition::new("done", None)],
            )
            .with_skills(skills.iter().copied()),
            RegisteredWorkflowPhase::new("done", true, vec![]),
        ],
    )
}

#[test]
fn explicitly_resolves_cursor_current_phase_through_the_skill_registry() {
    let workflow = workflow(&["zeta.skill", "alpha.skill"]);
    let cursor = WorkflowCursor::new(&workflow).unwrap();
    let registry = registry(&["unused.skill", "zeta.skill", "alpha.skill"]);

    let selected = cursor.current_phase().resolve_skills(&registry).unwrap();

    assert_eq!(
        selected
            .skills()
            .map(|skill| skill.id().as_str())
            .collect::<Vec<_>>(),
        ["alpha.skill", "zeta.skill"]
    );
    assert_eq!(
        cursor.current_phase().skills(),
        ["zeta.skill", "alpha.skill"]
    );
}

#[test]
fn resolves_replayed_run_phase_and_fails_closed_when_a_binding_is_missing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let run_id = WorkflowRunId::new("review-run").unwrap();
    let workflow = workflow(&["present.skill", "missing.skill"]);
    WorkflowRunStore::open(&path)
        .unwrap()
        .start(run_id.clone(), &workflow)
        .unwrap();
    let replayed = WorkflowRunStore::open(&path)
        .unwrap()
        .load(&run_id)
        .unwrap()
        .unwrap();
    let registry = registry(&["present.skill", "unrelated.skill"]);

    let error = replayed
        .current_phase()
        .resolve_skills(&registry)
        .unwrap_err();

    assert_eq!(
        error,
        WorkflowPhaseSkillResolutionError::Selection(SkillSelectionError::MissingId {
            skill_id: SkillId::new("missing.skill").unwrap(),
        })
    );
}

#[test]
fn malformed_direct_phase_bindings_fail_before_registry_selection() {
    let phase = RegisteredWorkflowPhase::new("manual", false, vec![]).with_skills(["  "]);
    let registry = registry(&["unrelated.skill"]);

    let error = phase.resolve_skills(&registry).unwrap_err();

    assert!(matches!(
        error,
        WorkflowPhaseSkillResolutionError::InvalidId {
            ref phase_id,
            index: 0,
            ..
        } if phase_id == "manual"
    ));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn malformed_direct_terminal_bindings_fail_closed() {
    let phase =
        RegisteredWorkflowPhase::new("done", true, vec![]).with_skills(["registered.skill"]);
    let registry = registry(&["registered.skill"]);

    assert_eq!(
        phase.resolve_skills(&registry).unwrap_err(),
        WorkflowPhaseSkillResolutionError::TerminalHasBindings {
            phase_id: "done".to_owned(),
        }
    );
}

#[test]
fn empty_phase_bindings_resolve_without_selecting_registered_skills() {
    let workflow = workflow(&[]);
    let cursor = WorkflowCursor::new(&workflow).unwrap();
    let registry = registry(&["unrelated.skill"]);

    assert_eq!(
        cursor
            .current_phase()
            .resolve_skills(&registry)
            .unwrap()
            .skills()
            .count(),
        0
    );
}
