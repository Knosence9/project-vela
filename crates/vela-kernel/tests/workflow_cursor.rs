use vela_kernel::workflow::{
    RegisteredWorkflow, RegisteredWorkflowPhase, RegisteredWorkflowTransition, WorkflowCursor,
    WorkflowCursorError, WorkflowId,
};

fn phase(
    id: &str,
    terminal: bool,
    transitions: Vec<RegisteredWorkflowTransition>,
) -> RegisteredWorkflowPhase {
    RegisteredWorkflowPhase::new(id, terminal, transitions)
}

fn transition(target: &str, gate: Option<&str>) -> RegisteredWorkflowTransition {
    RegisteredWorkflowTransition::new(target, gate.map(str::to_owned))
}

fn workflow(start: &str, phases: Vec<RegisteredWorkflowPhase>) -> RegisteredWorkflow {
    RegisteredWorkflow::new(WorkflowId::new("release.workflow").unwrap(), start, phases)
}

#[test]
fn begins_at_the_exact_start_without_advancing_or_copying_topology() {
    let workflow = workflow(
        "  plan ",
        vec![
            phase("  plan ", false, vec![transition("done", None)]),
            phase("done", true, vec![]),
        ],
    );

    let cursor = WorkflowCursor::new(&workflow).unwrap();

    assert!(std::ptr::eq(cursor.workflow(), &workflow));
    assert_eq!(cursor.current_phase().id(), "  plan ");
    assert!(!cursor.is_terminal());
    assert_eq!(cursor.current_phase().transitions()[0].target(), "done");
}

#[test]
fn authored_index_selects_duplicate_edges_and_exact_gate_acknowledgement() {
    let workflow = workflow(
        "review",
        vec![
            phase(
                "review",
                false,
                vec![
                    transition("done", Some("first.approved")),
                    transition("done", Some("second.approved")),
                ],
            ),
            phase("done", true, vec![]),
        ],
    );
    let mut cursor = WorkflowCursor::new(&workflow).unwrap();

    let error = cursor.advance(1, Some("first.approved")).unwrap_err();
    assert_eq!(
        error,
        WorkflowCursorError::GateAcknowledgementMismatch {
            phase_id: "review".to_owned(),
            transition_index: 1,
            expected_gate_id: "second.approved".to_owned(),
            actual_gate_id: "first.approved".to_owned(),
        }
    );
    assert_eq!(cursor.current_phase().id(), "review");

    cursor.advance(1, Some("second.approved")).unwrap();
    assert_eq!(cursor.current_phase().id(), "done");
    assert!(cursor.is_terminal());
}

#[test]
fn gated_and_ungated_edges_require_exact_acknowledgement_shapes() {
    let gated = workflow(
        "start",
        vec![
            phase("start", false, vec![transition("done", Some("approved"))]),
            phase("done", true, vec![]),
        ],
    );
    let mut gated_cursor = WorkflowCursor::new(&gated).unwrap();
    assert_eq!(
        gated_cursor.advance(0, None).unwrap_err(),
        WorkflowCursorError::GateAcknowledgementMissing {
            phase_id: "start".to_owned(),
            transition_index: 0,
            gate_id: "approved".to_owned(),
        }
    );
    assert_eq!(gated_cursor.current_phase().id(), "start");

    let ungated = workflow(
        "start",
        vec![
            phase("start", false, vec![transition("done", None)]),
            phase("done", true, vec![]),
        ],
    );
    let mut ungated_cursor = WorkflowCursor::new(&ungated).unwrap();
    assert_eq!(
        ungated_cursor.advance(0, Some("unneeded")).unwrap_err(),
        WorkflowCursorError::GateAcknowledgementUnexpected {
            phase_id: "start".to_owned(),
            transition_index: 0,
            gate_id: "unneeded".to_owned(),
        }
    );
    assert_eq!(ungated_cursor.current_phase().id(), "start");
    ungated_cursor.advance(0, None).unwrap();
    assert_eq!(ungated_cursor.current_phase().id(), "done");
}

#[test]
fn invalid_transition_attempts_are_typed_and_atomic() {
    let workflow = workflow(
        "start",
        vec![
            phase("start", false, vec![transition("done", None)]),
            phase("done", true, vec![]),
        ],
    );
    let mut cursor = WorkflowCursor::new(&workflow).unwrap();

    assert_eq!(
        cursor.advance(7, None).unwrap_err(),
        WorkflowCursorError::TransitionNotFound {
            phase_id: "start".to_owned(),
            transition_index: 7,
        }
    );
    assert_eq!(cursor.current_phase().id(), "start");

    cursor.advance(0, None).unwrap();
    assert_eq!(
        cursor.advance(0, None).unwrap_err(),
        WorkflowCursorError::TerminalPhase {
            phase_id: "done".to_owned(),
        }
    );
    assert_eq!(cursor.current_phase().id(), "done");
}

#[test]
fn missing_and_ambiguous_exact_phases_fail_closed() {
    let missing_start = workflow("missing", vec![phase("done", true, vec![])]);
    assert_eq!(
        WorkflowCursor::new(&missing_start).unwrap_err(),
        WorkflowCursorError::StartNotFound {
            phase_id: "missing".to_owned(),
        }
    );

    let ambiguous_start = workflow(
        "start",
        vec![
            phase("start", false, vec![transition("done", None)]),
            phase("start", false, vec![transition("done", None)]),
            phase("done", true, vec![]),
        ],
    );
    assert_eq!(
        WorkflowCursor::new(&ambiguous_start).unwrap_err(),
        WorkflowCursorError::StartAmbiguous {
            phase_id: "start".to_owned(),
        }
    );

    let missing_target = workflow(
        "start",
        vec![phase("start", false, vec![transition("missing", None)])],
    );
    let mut missing_cursor = WorkflowCursor::new(&missing_target).unwrap();
    assert_eq!(
        missing_cursor.advance(0, None).unwrap_err(),
        WorkflowCursorError::TargetNotFound {
            phase_id: "start".to_owned(),
            transition_index: 0,
            target_phase_id: "missing".to_owned(),
        }
    );
    assert_eq!(missing_cursor.current_phase().id(), "start");

    let ambiguous_target = workflow(
        "start",
        vec![
            phase("start", false, vec![transition("done", None)]),
            phase("done", true, vec![]),
            phase("done", true, vec![]),
        ],
    );
    let mut ambiguous_cursor = WorkflowCursor::new(&ambiguous_target).unwrap();
    assert_eq!(
        ambiguous_cursor.advance(0, None).unwrap_err(),
        WorkflowCursorError::TargetAmbiguous {
            phase_id: "start".to_owned(),
            transition_index: 0,
            target_phase_id: "done".to_owned(),
        }
    );
    assert_eq!(ambiguous_cursor.current_phase().id(), "start");
}

#[test]
fn cycles_can_revisit_phases_without_automatic_stop_or_side_effects() {
    let workflow = workflow(
        "alpha",
        vec![
            phase(
                "alpha",
                false,
                vec![transition("beta", None), transition("done", None)],
            ),
            phase("beta", false, vec![transition("alpha", None)]),
            phase("done", true, vec![]),
        ],
    );
    let mut cursor = WorkflowCursor::new(&workflow).unwrap();

    cursor.advance(0, None).unwrap();
    assert_eq!(cursor.current_phase().id(), "beta");
    cursor.advance(0, None).unwrap();
    assert_eq!(cursor.current_phase().id(), "alpha");
    assert!(!cursor.is_terminal());
}
