use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{
    ExtensionKind, ExtensionRegistry, WorkflowRegistrationError, register_workflow_selection,
};
use vela_kernel::workflow::{
    RegisteredWorkflow, RegisteredWorkflowPhase, WorkflowId, WorkflowRegistry,
};

#[test]
fn registers_prepared_workflows_atomically_with_exact_topology() {
    let root = tempdir().unwrap();
    write_extension(
        root.path(),
        "zeta",
        "zeta.workflow",
        "workflow",
        workflow_yaml("plan"),
    );
    write_extension(
        root.path(),
        "alpha",
        "alpha.workflow",
        "workflow",
        workflow_yaml("start"),
    );
    let extensions = ExtensionRegistry::discover(root.path()).unwrap();
    let selection = extensions
        .select_kind(ExtensionKind::Workflow, ["zeta.workflow", "alpha.workflow"])
        .unwrap();
    let mut workflows = WorkflowRegistry::new();

    register_workflow_selection(root.path(), &selection, &mut workflows).unwrap();

    assert_eq!(
        workflows
            .workflows()
            .map(|workflow| workflow.id().as_str())
            .collect::<Vec<_>>(),
        vec!["alpha.workflow", "zeta.workflow"]
    );
    let zeta = workflows
        .get(&WorkflowId::new("zeta.workflow").unwrap())
        .unwrap();
    assert_eq!(zeta.start(), "plan");
    assert_eq!(zeta.phases()[0].id(), "plan");
    assert_eq!(
        zeta.phases()[0].skills(),
        ["research.skill", "review.skill"]
    );
    assert_eq!(zeta.phases()[0].transitions()[0].target(), "done");
    assert_eq!(zeta.phases()[0].transitions()[0].gate(), Some("approved"));
    assert!(zeta.phases()[1].is_terminal());
}

#[test]
fn rejects_first_non_workflow_before_filesystem_access() {
    let root = tempdir().unwrap();
    write_extension(root.path(), "zeta", "zeta.tool", "tool", "bytes");
    write_extension(root.path(), "alpha", "alpha.skill", "skill", "text");
    let extensions = ExtensionRegistry::discover(root.path()).unwrap();
    let selection = extensions.select(["zeta.tool", "alpha.skill"]).unwrap();
    fs::remove_dir_all(root.path()).unwrap();
    let mut workflows = WorkflowRegistry::new();

    let error = register_workflow_selection(root.path(), &selection, &mut workflows).unwrap_err();

    assert!(matches!(
        error,
        WorkflowRegistrationError::WrongKind {
            ref id,
            actual: ExtensionKind::Skill,
        } if id == "alpha.skill"
    ));
    assert!(error.source().is_none());
    assert_eq!(workflows.workflows().count(), 0);
}

#[test]
fn preparation_and_registry_failures_leave_existing_workflows_unchanged() {
    for failure in ["preparation", "collision"] {
        let root = tempdir().unwrap();
        write_extension(
            root.path(),
            "alpha",
            "alpha.workflow",
            "workflow",
            workflow_yaml("start"),
        );
        write_extension(
            root.path(),
            "zeta",
            "zeta.workflow",
            "workflow",
            workflow_yaml("plan"),
        );
        let extensions = ExtensionRegistry::discover(root.path()).unwrap();
        let selection = extensions
            .select_kind(ExtensionKind::Workflow, ["alpha.workflow", "zeta.workflow"])
            .unwrap();
        let mut workflows = WorkflowRegistry::new();
        workflows
            .register_all([RegisteredWorkflow::new(
                WorkflowId::new(if failure == "collision" {
                    "zeta.workflow"
                } else {
                    "keep.workflow"
                })
                .unwrap(),
                "original",
                vec![RegisteredWorkflowPhase::new("original", true, vec![])],
            )])
            .unwrap();
        if failure == "preparation" {
            fs::remove_file(root.path().join("zeta/WORKFLOW.org")).unwrap();
        }

        let error =
            register_workflow_selection(root.path(), &selection, &mut workflows).unwrap_err();

        assert!(error.source().is_some());
        assert!(matches!(
            (failure, &error),
            ("preparation", WorkflowRegistrationError::Preparation { .. })
                | ("collision", WorkflowRegistrationError::Registry { .. })
        ));
        assert_eq!(workflows.workflows().count(), 1);
        assert_eq!(workflows.workflows().next().unwrap().start(), "original");
    }
}

#[test]
fn empty_selection_needs_no_filesystem_and_does_not_mutate_registry() {
    let root = tempdir().unwrap();
    let extensions = ExtensionRegistry::discover(root.path()).unwrap();
    let selection = extensions
        .select_kind(ExtensionKind::Workflow, std::iter::empty::<&str>())
        .unwrap();
    fs::remove_dir(root.path()).unwrap();
    let mut workflows = WorkflowRegistry::new();

    register_workflow_selection(root.path(), &selection, &mut workflows).unwrap();

    assert_eq!(workflows.workflows().count(), 0);
}

fn workflow_yaml(start: &str) -> &'static str {
    match start {
        "plan" => {
            "workflow_version: 2\nstart: plan\nphases:\n  - id: plan\n    skills:\n      - research.skill\n      - review.skill\n    transitions:\n      - to: done\n        gate: approved\n  - id: done\n    terminal: true\n"
        }
        _ => {
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n    transitions:\n      - to: done\n        gate: approved\n  - id: done\n    terminal: true\n"
        }
    }
}

fn write_extension(root: &std::path::Path, package: &str, id: &str, kind: &str, content: &str) {
    let package = root.join(package);
    fs::create_dir(&package).unwrap();
    fs::write(
        package.join("extension.yaml"),
        format!("manifest_version: 1\nid: {id}\nkind: {kind}\nentrypoint: WORKFLOW.org\n"),
    )
    .unwrap();
    fs::write(package.join("WORKFLOW.org"), content).unwrap();
}
