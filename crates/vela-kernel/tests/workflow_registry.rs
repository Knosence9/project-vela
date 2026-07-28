use vela_kernel::workflow::{
    RegisteredWorkflow, RegisteredWorkflowPhase, RegisteredWorkflowTransition, WorkflowId,
    WorkflowRegistry, WorkflowRegistryError,
};

fn workflow(id: &str, start: &str) -> RegisteredWorkflow {
    RegisteredWorkflow::new(
        WorkflowId::new(id).unwrap(),
        start,
        vec![
            RegisteredWorkflowPhase::new(
                start,
                false,
                vec![RegisteredWorkflowTransition::new(
                    "done",
                    Some("approved".to_owned()),
                )],
            ),
            RegisteredWorkflowPhase::new("done", true, vec![]),
        ],
    )
}

#[test]
fn registry_preserves_exact_topology_and_lists_workflows_in_id_order() {
    let mut registry = WorkflowRegistry::new();
    registry
        .register_all([
            workflow("zeta.workflow", "  plan "),
            workflow("alpha.workflow", "start"),
        ])
        .unwrap();

    assert_eq!(
        registry
            .workflows()
            .map(|workflow| workflow.id().as_str())
            .collect::<Vec<_>>(),
        vec!["alpha.workflow", "zeta.workflow"]
    );
    let registered = registry
        .get(&WorkflowId::new("zeta.workflow").unwrap())
        .unwrap();
    assert_eq!(registered.start(), "  plan ");
    assert_eq!(registered.phases()[0].id(), "  plan ");
    assert!(!registered.phases()[0].is_terminal());
    assert_eq!(registered.phases()[0].transitions()[0].target(), "done");
    assert_eq!(
        registered.phases()[0].transitions()[0].gate(),
        Some("approved")
    );
    assert!(registered.phases()[1].is_terminal());
}

#[test]
fn batch_registration_reports_first_collision_and_is_atomic() {
    let mut registry = WorkflowRegistry::new();
    registry
        .register_all([workflow("keep.workflow", "start")])
        .unwrap();

    let error = registry
        .register_all([
            workflow("zeta.workflow", "start"),
            workflow("zeta.workflow", "other"),
            workflow("keep.workflow", "replacement"),
        ])
        .unwrap_err();

    assert_eq!(
        error,
        WorkflowRegistryError::DuplicateId {
            workflow_id: WorkflowId::new("keep.workflow").unwrap(),
        }
    );
    assert_eq!(registry.workflows().count(), 1);
    assert_eq!(registry.workflows().next().unwrap().start(), "start");
}

#[test]
fn internal_batch_collisions_are_rejected_atomically() {
    let mut registry = WorkflowRegistry::new();

    let error = registry
        .register_all([
            workflow("zeta.workflow", "start"),
            workflow("alpha.workflow", "start"),
            workflow("zeta.workflow", "other"),
            workflow("alpha.workflow", "other"),
        ])
        .unwrap_err();

    assert_eq!(
        error,
        WorkflowRegistryError::DuplicateId {
            workflow_id: WorkflowId::new("alpha.workflow").unwrap(),
        }
    );
    assert_eq!(registry.workflows().count(), 0);
}

#[test]
fn workflow_ids_reject_blank_values_and_debug_summarizes_topology() {
    assert!(WorkflowId::new(" \n").is_err());
    let registered = workflow("review.workflow", "start");
    let debug = format!("{registered:?}");
    assert!(debug.contains("review.workflow"));
    assert!(debug.contains("phases_len"));
    assert!(!debug.contains("approved"));
}
