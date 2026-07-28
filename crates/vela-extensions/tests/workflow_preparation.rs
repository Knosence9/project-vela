use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{
    ExtensionKind, ExtensionRegistry, MAX_WORKFLOW_DEFINITION_BYTES, WorkflowDefinitionError,
    WorkflowPreparationError, prepare_workflow_definitions,
};

const VALID: &str = r#"workflow_version: 1
start: plan
phases:
  - id: plan
    transitions:
      - to: review
        gate: plan.approved
  - id: review
    transitions:
      - to: plan
      - to: done
  - id: done
    terminal: true
"#;

#[test]
fn prepares_exact_workflow_topology_in_extension_id_order() {
    let root = tempdir().expect("temporary extension root");
    write_extension(root.path(), "zeta", "zeta.workflow", "workflow", VALID);
    write_extension(root.path(), "alpha", "alpha.workflow", "workflow", VALID);
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Workflow, ["zeta.workflow", "alpha.workflow"])
        .expect("workflow selection");

    let workflows =
        prepare_workflow_definitions(root.path(), &selection).expect("prepared workflows");

    assert_eq!(
        workflows
            .iter()
            .map(|workflow| workflow.id())
            .collect::<Vec<_>>(),
        vec!["alpha.workflow", "zeta.workflow"]
    );
    let workflow = &workflows[0];
    assert_eq!(workflow.start(), "plan");
    assert_eq!(
        workflow
            .phases()
            .iter()
            .map(|phase| phase.id())
            .collect::<Vec<_>>(),
        vec!["plan", "review", "done"]
    );
    assert!(!workflow.phases()[0].is_terminal());
    assert_eq!(workflow.phases()[0].transitions()[0].target(), "review");
    assert_eq!(
        workflow.phases()[0].transitions()[0].gate(),
        Some("plan.approved")
    );
    assert!(workflow.phases()[2].is_terminal());
    assert!(workflow.phases()[2].transitions().is_empty());
    assert_eq!(
        format!("{workflow:?}"),
        "PreparedWorkflowDefinition { id: \"alpha.workflow\", start: \"plan\", phases_len: 3 }"
    );
}

#[test]
fn rejects_first_non_workflow_before_filesystem_access() {
    let root = tempdir().expect("temporary extension root");
    write_extension(root.path(), "alpha", "alpha.skill", "skill", VALID);
    write_extension(root.path(), "zeta", "zeta.tool", "tool", VALID);
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select(["zeta.tool", "alpha.skill"])
        .expect("generic selection");
    fs::remove_dir_all(root.path()).expect("remove root");

    let error = prepare_workflow_definitions(root.path(), &selection)
        .expect_err("wrong kind must precede root access");

    assert!(matches!(
        error,
        WorkflowPreparationError::WrongKind {
            ref id,
            actual: ExtensionKind::Skill,
        } if id == "alpha.skill"
    ));
    assert!(error.source().is_none());
}

#[test]
fn rejects_encoding_yaml_version_and_unknown_fields_with_sources_where_applicable() {
    type ErrorCase = (
        &'static str,
        &'static [u8],
        fn(&WorkflowPreparationError) -> bool,
    );
    let cases: &[ErrorCase] = &[
        ("utf8", &[0xff, 0xfe], |error| {
            matches!(error, WorkflowPreparationError::InvalidUtf8 { .. }) && error.source().is_some()
        }),
        ("yaml", b"workflow_version: [", |error| {
            matches!(error, WorkflowPreparationError::Parse { .. }) && error.source().is_some()
        }),
        ("version", b"workflow_version: 2\nstart: done\nphases:\n  - id: done\n    terminal: true\n", |error| {
            matches!(error, WorkflowPreparationError::Definition { source: WorkflowDefinitionError::UnsupportedVersion { version: 2 }, .. })
        }),
        ("unknown", b"workflow_version: 1\nstart: done\nunknown: true\nphases:\n  - id: done\n    terminal: true\n", |error| {
            matches!(error, WorkflowPreparationError::Parse { .. }) && error.source().is_some()
        }),
    ];

    for (name, bytes, matches_error) in cases {
        let root = tempdir().expect("temporary extension root");
        write_extension(root.path(), "flow", "review.workflow", "workflow", VALID);
        let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let selection = registry
            .select_kind(ExtensionKind::Workflow, ["review.workflow"])
            .expect("workflow selection");
        fs::write(root.path().join("flow/WORKFLOW.yaml"), bytes).expect("replace definition");

        let error = prepare_workflow_definitions(root.path(), &selection)
            .expect_err("invalid definition must fail");
        assert!(matches_error(&error), "unexpected {name} error: {error:?}");
    }
}

#[test]
fn rejects_invalid_graph_invariants_deterministically() {
    let cases = [
        (
            "blank start",
            "workflow_version: 1\nstart: '  '\nphases:\n  - id: done\n    terminal: true\n",
            WorkflowDefinitionError::BlankStart,
        ),
        (
            "empty phases",
            "workflow_version: 1\nstart: start\nphases: []\n",
            WorkflowDefinitionError::EmptyPhases,
        ),
        (
            "missing terminal",
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n    transitions:\n      - to: start\n",
            WorkflowDefinitionError::NoTerminalPhase,
        ),
        (
            "blank phase",
            "workflow_version: 1\nstart: start\nphases:\n  - id: ' '\n    terminal: true\n",
            WorkflowDefinitionError::BlankPhaseId { index: 0 },
        ),
        (
            "duplicate phase",
            "workflow_version: 1\nstart: same\nphases:\n  - id: same\n    terminal: true\n  - id: same\n    terminal: true\n",
            WorkflowDefinitionError::DuplicatePhaseId {
                phase_id: "same".into(),
            },
        ),
        (
            "missing start",
            "workflow_version: 1\nstart: absent\nphases:\n  - id: done\n    terminal: true\n",
            WorkflowDefinitionError::StartNotFound {
                phase_id: "absent".into(),
            },
        ),
        (
            "terminal transition",
            "workflow_version: 1\nstart: done\nphases:\n  - id: done\n    terminal: true\n    transitions:\n      - to: done\n",
            WorkflowDefinitionError::TerminalHasTransitions {
                phase_id: "done".into(),
            },
        ),
        (
            "nonterminal without transition",
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n",
            WorkflowDefinitionError::NonTerminalHasNoTransitions {
                phase_id: "start".into(),
            },
        ),
        (
            "blank target",
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n    transitions:\n      - to: ' '\n",
            WorkflowDefinitionError::BlankTransitionTarget {
                phase_id: "start".into(),
                index: 0,
            },
        ),
        (
            "missing target",
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n    transitions:\n      - to: absent\n",
            WorkflowDefinitionError::TransitionTargetNotFound {
                phase_id: "start".into(),
                target: "absent".into(),
            },
        ),
        (
            "blank gate",
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n    transitions:\n      - to: done\n        gate: ' '\n  - id: done\n    terminal: true\n",
            WorkflowDefinitionError::BlankGate {
                phase_id: "start".into(),
                index: 0,
            },
        ),
        (
            "unreachable",
            "workflow_version: 1\nstart: start\nphases:\n  - id: start\n    transitions:\n      - to: done\n  - id: hidden\n    terminal: true\n  - id: done\n    terminal: true\n",
            WorkflowDefinitionError::UnreachablePhase {
                phase_id: "hidden".into(),
            },
        ),
    ];

    for (name, definition, expected) in cases {
        let error = prepare_one(definition).expect_err("invalid graph must fail");
        assert!(matches!(error, WorkflowPreparationError::Definition { .. }));
        let WorkflowPreparationError::Definition { source, .. } = error else {
            unreachable!()
        };
        assert_eq!(source, expected, "wrong invariant for {name}");
    }
}

#[test]
fn enforces_size_revalidation_empty_and_all_or_nothing_boundaries() {
    for (size, accepted) in [
        (MAX_WORKFLOW_DEFINITION_BYTES as usize, true),
        (MAX_WORKFLOW_DEFINITION_BYTES as usize + 1, false),
    ] {
        let padding = size - VALID.len() - 2;
        let definition = format!("{VALID}# {}", "x".repeat(padding));
        let result = prepare_one(&definition);
        assert_eq!(result.is_ok(), accepted, "size {size}");
    }

    let empty_root = tempdir().expect("temporary extension root");
    let registry = ExtensionRegistry::discover(empty_root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Workflow, std::iter::empty::<&str>())
        .expect("empty selection");
    fs::remove_dir(empty_root.path()).expect("remove unused root");
    assert!(
        prepare_workflow_definitions(empty_root.path(), &selection)
            .expect("empty preparation")
            .is_empty()
    );

    for changed in ["manifest", "package", "entrypoint"] {
        let root = tempdir().expect("temporary extension root");
        write_extension(root.path(), "alpha", "alpha.workflow", "workflow", VALID);
        write_extension(root.path(), "zeta", "zeta.workflow", "workflow", VALID);
        let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let selection = registry
            .select_kind(ExtensionKind::Workflow, ["alpha.workflow", "zeta.workflow"])
            .expect("selection");
        match changed {
            "manifest" => fs::write(
                root.path().join("zeta/extension.yaml"),
                manifest("changed.workflow", "workflow"),
            )
            .expect("change manifest"),
            "package" => {
                fs::rename(root.path().join("zeta"), root.path().join("old-zeta")).expect("move");
                fs::create_dir(root.path().join("zeta")).expect("replace");
            }
            "entrypoint" => {
                fs::remove_file(root.path().join("zeta/WORKFLOW.yaml")).expect("remove entrypoint")
            }
            _ => unreachable!(),
        }
        assert!(matches!(
            prepare_workflow_definitions(root.path(), &selection),
            Err(WorkflowPreparationError::Preparation { .. })
        ));
    }
}

fn prepare_one(
    definition: &str,
) -> Result<Vec<vela_extensions::PreparedWorkflowDefinition>, WorkflowPreparationError> {
    let root = tempdir().expect("temporary extension root");
    write_extension(
        root.path(),
        "flow",
        "review.workflow",
        "workflow",
        definition,
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Workflow, ["review.workflow"])
        .expect("selection");
    prepare_workflow_definitions(root.path(), &selection)
}

fn write_extension(root: &std::path::Path, package: &str, id: &str, kind: &str, content: &str) {
    let package = root.join(package);
    fs::create_dir(&package).expect("create package");
    fs::write(package.join("extension.yaml"), manifest(id, kind)).expect("write manifest");
    fs::write(package.join("WORKFLOW.yaml"), content).expect("write entrypoint");
}

fn manifest(id: &str, kind: &str) -> String {
    format!("manifest_version: 1\nid: {id}\nkind: {kind}\nentrypoint: WORKFLOW.yaml\n")
}
