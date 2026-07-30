use std::{cell::RefCell, error::Error, fmt, path::PathBuf, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        AssistantRuntime, ComposedAssistantProvider, ComposedAssistantRequest, DeveloperPolicy,
        ProviderError, RuntimeError, SystemPolicy,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole},
    skill::{RegisteredSkill, SkillId, SkillRegistry},
    task::{
        TaskCancellation, TaskFailure, TaskGoal, TaskId, TaskObservationId, TaskObservationKind,
        TaskObservationText, TaskStore, TaskStoreError,
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
fn records_a_selected_workflow_phase_response_as_task_correction_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-correction-session").unwrap();
    let task_id = TaskId::new("phase-correction-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let parent_attempt_id = TaskObservationId::new("parent-attempt").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .append_observation(
            &task_id,
            parent_attempt_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("original answer").unwrap(),
        )
        .unwrap();
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
        .execute_workflow_phase_task_correction_turn(
            &task_id,
            &parent_attempt_id,
            SessionTurnContent::new("correct this").unwrap(),
            TaskObservationId::new("phase-correction-1").unwrap(),
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
    assert_eq!(outcome.task().observations().len(), 2);
    let correction = &outcome.task().observations()[1];
    assert_eq!(correction.id().as_str(), "phase-correction-1");
    assert_eq!(correction.kind(), TaskObservationKind::Correction);
    assert_eq!(correction.text().as_str(), "phase answer");
    assert_eq!(correction.parent_attempt_id(), Some(&parent_attempt_id));
}

#[test]
fn completes_a_task_with_a_selected_workflow_phase_response() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-completion-session").unwrap();
    let task_id = TaskId::new("phase-completion-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::clone(&calls),
        },
    )
    .unwrap();
    let phase = RegisteredWorkflowPhase::new("finalize", false, vec![])
        .with_skills(["zeta.skill", "alpha.skill"]);

    let outcome = runtime
        .complete_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("finalize this").unwrap(),
            TaskObservationId::new("phase-final-attempt").unwrap(),
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
    assert_eq!(
        outcome.session().turns()[1].role(),
        SessionTurnRole::Assistant
    );
    assert_eq!(
        outcome.session().turns()[1].content().as_str(),
        "phase answer"
    );
    assert_eq!(
        outcome.task().status(),
        vela_kernel::task::TaskStatus::Completed
    );
    assert_eq!(outcome.task().output().unwrap().as_str(), "phase answer");
    assert_eq!(outcome.task().observations().len(), 1);
    let attempt = &outcome.task().observations()[0];
    assert_eq!(attempt.id().as_str(), "phase-final-attempt");
    assert_eq!(attempt.kind(), TaskObservationKind::Attempt);
    assert_eq!(attempt.text().as_str(), "phase answer");
}

#[test]
fn fails_a_task_with_a_selected_workflow_phase_response_and_caller_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-failure-session").unwrap();
    let task_id = TaskId::new("phase-failure-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::clone(&calls),
        },
    )
    .unwrap();
    let phase = RegisteredWorkflowPhase::new("diagnose", false, vec![])
        .with_skills(["zeta.skill", "alpha.skill"]);

    let outcome = runtime
        .fail_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("diagnose this failure").unwrap(),
            TaskObservationId::new("phase-failure-attempt").unwrap(),
            TaskFailure::new("caller-owned diagnostic").unwrap(),
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
    assert_eq!(
        outcome.session().turns()[1].role(),
        SessionTurnRole::Assistant
    );
    assert_eq!(
        outcome.session().turns()[1].content().as_str(),
        "phase answer"
    );
    assert_eq!(
        outcome.task().status(),
        vela_kernel::task::TaskStatus::Failed
    );
    assert_eq!(
        outcome.task().failure().unwrap().as_str(),
        "caller-owned diagnostic"
    );
    assert_eq!(outcome.task().observations().len(), 1);
    let attempt = &outcome.task().observations()[0];
    assert_eq!(attempt.id().as_str(), "phase-failure-attempt");
    assert_eq!(attempt.kind(), TaskObservationKind::Attempt);
    assert_eq!(attempt.text().as_str(), "phase answer");
}

#[test]
fn phase_failure_preflight_precedes_transcript_and_provider_effects() {
    for case in ["duplicate-attempt", "missing-skill"] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new(format!("{case}-failure-session")).unwrap();
        let task_id = TaskId::new(format!("{case}-failure-task")).unwrap();
        create_associated_task(&path, &task_id, &session_id);
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
        let skill_id = if case == "missing-skill" {
            "missing.skill"
        } else {
            "present.skill"
        };

        let error = runtime
            .fail_workflow_phase_task_turn(
                &task_id,
                SessionTurnContent::new("must not persist").unwrap(),
                attempt_id,
                TaskFailure::new("caller diagnostic").unwrap(),
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry(&["present.skill"]),
                &RegisteredWorkflowPhase::new("diagnose", false, vec![]).with_skills([skill_id]),
            )
            .unwrap_err();

        if case == "duplicate-attempt" {
            assert!(matches!(
                error,
                RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
            ));
        } else {
            assert!(matches!(error, RuntimeError::WorkflowPhaseSkills(_)));
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

#[test]
fn phase_completion_preflight_precedes_transcript_and_provider_effects() {
    for case in ["duplicate-attempt", "missing-skill"] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new(format!("{case}-completion-session")).unwrap();
        let task_id = TaskId::new(format!("{case}-completion-task")).unwrap();
        create_associated_task(&path, &task_id, &session_id);
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
        let skill_id = if case == "missing-skill" {
            "missing.skill"
        } else {
            "present.skill"
        };

        let error = runtime
            .complete_workflow_phase_task_turn(
                &task_id,
                SessionTurnContent::new("must not persist").unwrap(),
                attempt_id,
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry(&["present.skill"]),
                &RegisteredWorkflowPhase::new("finalize", false, vec![]).with_skills([skill_id]),
            )
            .unwrap_err();

        if case == "duplicate-attempt" {
            assert!(matches!(
                error,
                RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
            ));
        } else {
            assert!(matches!(error, RuntimeError::WorkflowPhaseSkills(_)));
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

#[test]
fn correction_lineage_and_phase_preflight_precede_transcript_and_provider_effects() {
    for case in [
        "missing-parent",
        "non-attempt-parent",
        "duplicate-correction",
        "missing-skill",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new(format!("{case}-correction-session")).unwrap();
        let task_id = TaskId::new(format!("{case}-correction-task")).unwrap();
        create_associated_task(&path, &task_id, &session_id);
        let parent_attempt_id = TaskObservationId::new("parent-attempt").unwrap();
        let requested_parent_id = if case == "non-attempt-parent" {
            TaskObservationId::new("earlier-correction").unwrap()
        } else {
            parent_attempt_id.clone()
        };
        if case != "missing-parent" {
            let mut tasks = TaskStore::open(&path).unwrap();
            tasks
                .append_observation(
                    &task_id,
                    parent_attempt_id.clone(),
                    TaskObservationKind::Attempt,
                    TaskObservationText::new("original answer").unwrap(),
                )
                .unwrap();
            if case == "non-attempt-parent" {
                tasks
                    .append_observation_for_attempt(
                        &task_id,
                        requested_parent_id.clone(),
                        TaskObservationKind::Correction,
                        TaskObservationText::new("earlier correction").unwrap(),
                        parent_attempt_id.clone(),
                    )
                    .unwrap();
            }
            if case == "duplicate-correction" {
                tasks
                    .append_observation_for_attempt(
                        &task_id,
                        TaskObservationId::new("correction-1").unwrap(),
                        TaskObservationKind::Correction,
                        TaskObservationText::new("existing correction").unwrap(),
                        parent_attempt_id.clone(),
                    )
                    .unwrap();
            }
        }
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = AssistantRuntime::open(
            &path,
            RecordingProvider {
                calls: Rc::clone(&calls),
            },
        )
        .unwrap();
        let phase = RegisteredWorkflowPhase::new("review", false, vec![]).with_skills([
            if case == "missing-skill" {
                "missing.skill"
            } else {
                "present.skill"
            },
        ]);

        let error = runtime
            .execute_workflow_phase_task_correction_turn(
                &task_id,
                &requested_parent_id,
                SessionTurnContent::new("must not persist").unwrap(),
                TaskObservationId::new("correction-1").unwrap(),
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry(&["present.skill"]),
                &phase,
            )
            .unwrap_err();

        match case {
            "missing-skill" => assert!(matches!(error, RuntimeError::WorkflowPhaseSkills(_))),
            _ => assert!(matches!(error, RuntimeError::Task(_))),
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

struct RacingAttemptProvider {
    path: PathBuf,
    task_id: TaskId,
    attempt_id: TaskObservationId,
}

impl ComposedAssistantProvider for RacingAttemptProvider {
    fn complete_composed(
        &mut self,
        _request: ComposedAssistantRequest<'_>,
    ) -> Result<SessionTurnContent, ProviderError> {
        TaskStore::open(&self.path)
            .unwrap()
            .append_observation(
                &self.task_id,
                self.attempt_id.clone(),
                TaskObservationKind::Attempt,
                TaskObservationText::new("racing attempt").unwrap(),
            )
            .unwrap();
        Ok(SessionTurnContent::new("orphan-prone answer").unwrap())
    }
}

#[test]
fn racing_attempt_rejection_does_not_persist_an_orphan_assistant_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("racing-phase-task-session").unwrap();
    let task_id = TaskId::new("racing-phase-task").unwrap();
    let attempt_id = TaskObservationId::new("attempt-1").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let mut runtime = AssistantRuntime::open(
        &path,
        RacingAttemptProvider {
            path: path.clone(),
            task_id: task_id.clone(),
            attempt_id: attempt_id.clone(),
        },
    )
    .unwrap();

    let error = runtime
        .execute_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("persist this question").unwrap(),
            attempt_id,
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["review.skill"]),
            &RegisteredWorkflowPhase::new("review", false, vec![]).with_skills(["review.skill"]),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
    ));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "racing attempt");
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
fn blank_workflow_phase_task_correction_response_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("blank-phase-correction-session").unwrap();
    let task_id = TaskId::new("blank-phase-correction-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let parent_attempt_id = TaskObservationId::new("parent-attempt").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .append_observation(
            &task_id,
            parent_attempt_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("original answer").unwrap(),
        )
        .unwrap();
    let mut runtime = AssistantRuntime::open(&path, BlankProvider).unwrap();

    let error = runtime
        .execute_workflow_phase_task_correction_turn(
            &task_id,
            &parent_attempt_id,
            SessionTurnContent::new("persist this correction request").unwrap(),
            TaskObservationId::new("correction-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["review.skill"]),
            &RegisteredWorkflowPhase::new("review", false, vec![]).with_skills(["review.skill"]),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::InvalidCorrectionText(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].id(), &parent_attempt_id);
}

#[test]
fn workflow_phase_task_correction_provider_failure_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("failed-phase-correction-session").unwrap();
    let task_id = TaskId::new("failed-phase-correction-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let parent_attempt_id = TaskObservationId::new("parent-attempt").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .append_observation(
            &task_id,
            parent_attempt_id.clone(),
            TaskObservationKind::Attempt,
            TaskObservationText::new("original answer").unwrap(),
        )
        .unwrap();
    let mut runtime = AssistantRuntime::open(&path, FailingProvider).unwrap();

    let error = runtime
        .execute_workflow_phase_task_correction_turn(
            &task_id,
            &parent_attempt_id,
            SessionTurnContent::new("persist this correction request").unwrap(),
            TaskObservationId::new("correction-1").unwrap(),
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
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].id(), &parent_attempt_id);
}

#[test]
fn workflow_phase_task_failure_provider_failure_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("failed-phase-failure-session").unwrap();
    let task_id = TaskId::new("failed-phase-failure-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let mut runtime = AssistantRuntime::open(&path, FailingProvider).unwrap();

    let error = runtime
        .fail_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("persist this failure request").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            TaskFailure::new("caller diagnostic").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["diagnose.skill"]),
            &RegisteredWorkflowPhase::new("diagnose", false, vec![])
                .with_skills(["diagnose.skill"]),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert!(task.failure().is_none());
}

#[test]
fn workflow_phase_task_completion_provider_failure_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("failed-phase-completion-session").unwrap();
    let task_id = TaskId::new("failed-phase-completion-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let mut runtime = AssistantRuntime::open(&path, FailingProvider).unwrap();

    let error = runtime
        .complete_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("persist this final request").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["finalize.skill"]),
            &RegisteredWorkflowPhase::new("finalize", false, vec![])
                .with_skills(["finalize.skill"]),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert!(task.output().is_none());
}

#[test]
fn workflow_phase_failure_race_preserves_attempt_and_winning_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-failure-race-session").unwrap();
    let task_id = TaskId::new("phase-failure-race").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER complete_after_phase_failure_attempt
             AFTER INSERT ON events
             WHEN NEW.stream_id = 'task:phase-failure-race'
              AND NEW.event_type = 'task.observation_appended'
             BEGIN
               INSERT INTO events
                 (stream_id, stream_version, event_type, payload_version, payload)
               VALUES
                 (NEW.stream_id, NEW.stream_version + 1, 'task.completed', 2,
                  CAST('{\"output\":\"winning output\"}' AS BLOB));
             END;",
        )
        .unwrap();
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
        },
    )
    .unwrap();

    let error = runtime
        .fail_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("diagnose this").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            TaskFailure::new("losing diagnostic").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["diagnose.skill"]),
            &RegisteredWorkflowPhase::new("diagnose", false, vec![])
                .with_skills(["diagnose.skill"]),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    ));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].role(), SessionTurnRole::Assistant);
    assert_eq!(session.turns()[1].content().as_str(), "phase answer");
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "phase answer");
    assert_eq!(task.output().unwrap().as_str(), "winning output");
    assert!(task.failure().is_none());
}

#[test]
fn workflow_phase_completion_race_preserves_attempt_and_winning_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-completion-race-session").unwrap();
    let task_id = TaskId::new("phase-completion-race").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER complete_after_phase_attempt
             AFTER INSERT ON events
             WHEN NEW.stream_id = 'task:phase-completion-race'
              AND NEW.event_type = 'task.observation_appended'
             BEGIN
               INSERT INTO events
                 (stream_id, stream_version, event_type, payload_version, payload)
               VALUES
                 (NEW.stream_id, NEW.stream_version + 1, 'task.completed', 2,
                  CAST('{\"output\":\"winning output\"}' AS BLOB));
             END;",
        )
        .unwrap();
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
        },
    )
    .unwrap();

    let error = runtime
        .complete_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("finalize this").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["finalize.skill"]),
            &RegisteredWorkflowPhase::new("finalize", false, vec![])
                .with_skills(["finalize.skill"]),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    ));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].role(), SessionTurnRole::Assistant);
    assert_eq!(session.turns()[1].content().as_str(), "phase answer");
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "phase answer");
    assert_eq!(task.output().unwrap().as_str(), "winning output");
}

#[test]
fn cancels_a_task_with_a_selected_workflow_phase_response_and_caller_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-cancellation-session").unwrap();
    let task_id = TaskId::new("phase-cancellation-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::clone(&calls),
        },
    )
    .unwrap();
    let cancellation = TaskCancellation::new("caller-owned reason").unwrap();

    let outcome = runtime
        .cancel_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("prepare to stop").unwrap(),
            TaskObservationId::new("phase-cancellation-attempt").unwrap(),
            cancellation.clone(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["zeta.skill", "unused.skill", "alpha.skill"]),
            &RegisteredWorkflowPhase::new("stop", false, vec![])
                .with_skills(["zeta.skill", "alpha.skill"]),
        )
        .unwrap();

    assert_eq!(
        calls.borrow().as_slice(),
        &[vec!["alpha.skill", "zeta.skill"]]
    );
    assert_eq!(outcome.session().turns().len(), 2);
    assert_eq!(
        outcome.task().status(),
        vela_kernel::task::TaskStatus::Cancelled
    );
    assert_eq!(outcome.task().cancellation(), Some(&cancellation));
    assert_eq!(outcome.task().observations().len(), 1);
    let attempt = &outcome.task().observations()[0];
    assert_eq!(attempt.id().as_str(), "phase-cancellation-attempt");
    assert_eq!(attempt.kind(), TaskObservationKind::Attempt);
    assert_eq!(attempt.text().as_str(), "phase answer");
}

#[test]
fn phase_cancellation_preflight_precedes_transcript_and_provider_effects() {
    for case in ["duplicate-attempt", "missing-skill"] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new(format!("{case}-cancellation-session")).unwrap();
        let task_id = TaskId::new(format!("{case}-cancellation-task")).unwrap();
        create_associated_task(&path, &task_id, &session_id);
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
        let skill_id = if case == "missing-skill" {
            "missing.skill"
        } else {
            "present.skill"
        };

        let error = runtime
            .cancel_workflow_phase_task_turn(
                &task_id,
                SessionTurnContent::new("must not persist").unwrap(),
                attempt_id,
                TaskCancellation::new("caller reason").unwrap(),
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry(&["present.skill"]),
                &RegisteredWorkflowPhase::new("stop", false, vec![]).with_skills([skill_id]),
            )
            .unwrap_err();

        if case == "duplicate-attempt" {
            assert!(matches!(
                error,
                RuntimeError::Task(TaskStoreError::DuplicateObservation { .. })
            ));
        } else {
            assert!(matches!(error, RuntimeError::WorkflowPhaseSkills(_)));
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

#[test]
fn workflow_phase_task_cancellation_provider_failure_preserves_only_the_human_turn() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("failed-phase-cancellation-session").unwrap();
    let task_id = TaskId::new("failed-phase-cancellation-task").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    let mut runtime = AssistantRuntime::open(&path, FailingProvider).unwrap();

    let error = runtime
        .cancel_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("persist this cancellation request").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            TaskCancellation::new("caller reason").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["stop.skill"]),
            &RegisteredWorkflowPhase::new("stop", false, vec![]).with_skills(["stop.skill"]),
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 1);
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert!(task.observations().is_empty());
    assert_eq!(task.status(), vela_kernel::task::TaskStatus::Active);
    assert!(task.cancellation().is_none());
}

#[test]
fn workflow_phase_cancellation_race_preserves_attempt_and_winning_state() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("phase-cancellation-race-session").unwrap();
    let task_id = TaskId::new("phase-cancellation-race").unwrap();
    create_associated_task(&path, &task_id, &session_id);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER complete_after_phase_cancellation_attempt
             AFTER INSERT ON events
             WHEN NEW.stream_id = 'task:phase-cancellation-race'
              AND NEW.event_type = 'task.observation_appended'
             BEGIN
               INSERT INTO events
                 (stream_id, stream_version, event_type, payload_version, payload)
               VALUES
                 (NEW.stream_id, NEW.stream_version + 1, 'task.completed', 2,
                  CAST('{\"output\":\"winning output\"}' AS BLOB));
             END;",
        )
        .unwrap();
    let mut runtime = AssistantRuntime::open(
        &path,
        RecordingProvider {
            calls: Rc::new(RefCell::new(Vec::new())),
        },
    )
    .unwrap();

    let error = runtime
        .cancel_workflow_phase_task_turn(
            &task_id,
            SessionTurnContent::new("prepare to stop").unwrap(),
            TaskObservationId::new("attempt-1").unwrap(),
            TaskCancellation::new("losing reason").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry(&["stop.skill"]),
            &RegisteredWorkflowPhase::new("stop", false, vec![]).with_skills(["stop.skill"]),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeError::Task(TaskStoreError::AlreadyCompleted { .. })
    ));
    let session = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].content().as_str(), "phase answer");
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.observations().len(), 1);
    assert_eq!(task.observations()[0].text().as_str(), "phase answer");
    assert_eq!(task.output().unwrap().as_str(), "winning output");
    assert!(task.cancellation().is_none());
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
