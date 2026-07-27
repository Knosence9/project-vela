use std::{cell::RefCell, error::Error, fmt, rc::Rc};

use tempfile::tempdir;
use vela_kernel::{
    runtime::{
        AssistantProvider, AssistantRuntime, ComposedAssistantProvider, ComposedAssistantRequest,
        DeveloperPolicy, ProviderError, RuntimeError, SystemPolicy,
    },
    session::{SessionId, SessionStore, SessionTitle, SessionTurnContent, SessionTurnRole},
    skill::{RegisteredSkill, SkillId, SkillRegistry, SkillSelectionError},
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordedComposedRequest {
    system_policy: String,
    developer_policy: String,
    skills: Vec<(String, String)>,
    transcript: Vec<(SessionTurnRole, String)>,
}

struct RecordingProvider {
    plain_calls: Rc<RefCell<usize>>,
    composed_calls: Rc<RefCell<Vec<RecordedComposedRequest>>>,
    composed_result: Result<SessionTurnContent, FakeProviderFailure>,
}

impl AssistantProvider for RecordingProvider {
    fn complete(
        &mut self,
        _transcript: &[vela_kernel::session::SessionTurn],
    ) -> Result<SessionTurnContent, ProviderError> {
        *self.plain_calls.borrow_mut() += 1;
        Ok(SessionTurnContent::new("plain answer").unwrap())
    }
}

impl ComposedAssistantProvider for RecordingProvider {
    fn complete_composed(
        &mut self,
        request: ComposedAssistantRequest<'_>,
    ) -> Result<SessionTurnContent, ProviderError> {
        self.composed_calls
            .borrow_mut()
            .push(RecordedComposedRequest {
                system_policy: request.system_policy().as_str().to_owned(),
                developer_policy: request.developer_policy().as_str().to_owned(),
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
            });
        self.composed_result.clone().map_err(ProviderError::new)
    }
}

#[derive(Clone, Debug)]
struct FakeProviderFailure;

impl fmt::Display for FakeProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("composed provider unavailable")
    }
}

impl Error for FakeProviderFailure {}

fn registry() -> SkillRegistry {
    let mut registry = SkillRegistry::new();
    registry
        .register_all([
            RegisteredSkill::new(SkillId::new("zeta.skill").unwrap(), "  exact zeta\n"),
            RegisteredSkill::new(SkillId::new("alpha.skill").unwrap(), "Exact café.\n"),
            RegisteredSkill::new(SkillId::new("unused.skill").unwrap(), "excluded"),
        ])
        .unwrap();
    registry
}

fn create_session(path: &std::path::Path, session_id: &SessionId) {
    SessionStore::open(path)
        .unwrap()
        .create(
            session_id.clone(),
            SessionTitle::new("Composed turn").unwrap(),
        )
        .unwrap();
}

#[test]
fn composed_turn_preserves_typed_authority_fields_and_durable_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("composed").unwrap();
    create_session(&path, &session_id);
    let plain_calls = Rc::new(RefCell::new(0));
    let composed_calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingProvider {
        plain_calls: Rc::clone(&plain_calls),
        composed_calls: Rc::clone(&composed_calls),
        composed_result: Ok(SessionTurnContent::new("composed answer").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();
    let registry = registry();

    let session = runtime
        .execute_composed_turn(
            &session_id,
            SessionTurnContent::new("question").unwrap(),
            SystemPolicy::new("system policy"),
            DeveloperPolicy::new("developer policy"),
            &registry,
            &[
                SkillId::new("zeta.skill").unwrap(),
                SkillId::new("alpha.skill").unwrap(),
            ],
        )
        .unwrap();

    assert_eq!(*plain_calls.borrow(), 0);
    assert_eq!(
        composed_calls.borrow().as_slice(),
        &[RecordedComposedRequest {
            system_policy: "system policy".to_owned(),
            developer_policy: "developer policy".to_owned(),
            skills: vec![
                ("alpha.skill".to_owned(), "Exact café.\n".to_owned()),
                ("zeta.skill".to_owned(), "  exact zeta\n".to_owned()),
            ],
            transcript: vec![(SessionTurnRole::Human, "question".to_owned())],
        }]
    );
    assert_eq!(session.turns().len(), 2);
    assert_eq!(session.turns()[1].content().as_str(), "composed answer");
}

#[test]
fn existing_turn_stays_skill_free_with_registered_skills() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("plain").unwrap();
    create_session(&path, &session_id);
    let plain_calls = Rc::new(RefCell::new(0));
    let composed_calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingProvider {
        plain_calls: Rc::clone(&plain_calls),
        composed_calls: Rc::clone(&composed_calls),
        composed_result: Ok(SessionTurnContent::new("unused").unwrap()),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();
    let _registered_but_not_passed = registry();

    runtime
        .execute_turn(
            &session_id,
            SessionTurnContent::new("plain question").unwrap(),
        )
        .unwrap();

    assert_eq!(*plain_calls.borrow(), 1);
    assert!(composed_calls.borrow().is_empty());
}

#[test]
fn selection_failures_precede_transcript_persistence_and_provider_invocation() {
    for selected_ids in [
        vec![
            SkillId::new("alpha.skill").unwrap(),
            SkillId::new("alpha.skill").unwrap(),
        ],
        vec![SkillId::new("missing.skill").unwrap()],
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("vela.sqlite3");
        let session_id = SessionId::new("selection-failure").unwrap();
        create_session(&path, &session_id);
        let composed_calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingProvider {
            plain_calls: Rc::new(RefCell::new(0)),
            composed_calls: Rc::clone(&composed_calls),
            composed_result: Ok(SessionTurnContent::new("unused").unwrap()),
        };
        let mut runtime = AssistantRuntime::open(&path, provider).unwrap();
        let registry = registry();

        let error = runtime
            .execute_composed_turn(
                &session_id,
                SessionTurnContent::new("must not persist").unwrap(),
                SystemPolicy::new("system"),
                DeveloperPolicy::new("developer"),
                &registry,
                &selected_ids,
            )
            .unwrap_err();

        assert!(matches!(
            error,
            RuntimeError::SkillSelection(
                SkillSelectionError::DuplicateId { .. } | SkillSelectionError::MissingId { .. }
            )
        ));
        assert!(composed_calls.borrow().is_empty());
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
fn composed_provider_failure_preserves_human_turn_and_error_source() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("vela.sqlite3");
    let session_id = SessionId::new("provider-failure").unwrap();
    create_session(&path, &session_id);
    let provider = RecordingProvider {
        plain_calls: Rc::new(RefCell::new(0)),
        composed_calls: Rc::new(RefCell::new(Vec::new())),
        composed_result: Err(FakeProviderFailure),
    };
    let mut runtime = AssistantRuntime::open(&path, provider).unwrap();
    let registry = registry();

    let error = runtime
        .execute_composed_turn(
            &session_id,
            SessionTurnContent::new("durable question").unwrap(),
            SystemPolicy::new("system"),
            DeveloperPolicy::new("developer"),
            &registry,
            &[SkillId::new("alpha.skill").unwrap()],
        )
        .unwrap_err();

    assert!(matches!(error, RuntimeError::Provider(_)));
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<FakeProviderFailure>()
            .is_some()
    );
    let persisted = SessionStore::open(&path)
        .unwrap()
        .load(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.turns().len(), 1);
    assert_eq!(persisted.turns()[0].role(), SessionTurnRole::Human);
    assert_eq!(persisted.turns()[0].content().as_str(), "durable question");
}
