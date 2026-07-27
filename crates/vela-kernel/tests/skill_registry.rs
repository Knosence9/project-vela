use vela_kernel::skill::{RegisteredSkill, SkillId, SkillRegistry, SkillRegistryError};

fn skill(id: &str, instructions: &str) -> RegisteredSkill {
    RegisteredSkill::new(SkillId::new(id).unwrap(), instructions)
}

#[test]
fn registry_preserves_exact_instructions_and_lists_skills_in_id_order() {
    let mut registry = SkillRegistry::new();
    registry
        .register_all([
            skill("zeta.skill", "  keep zeta whitespace\n"),
            skill("alpha.skill", "Use café.\n"),
        ])
        .unwrap();

    assert_eq!(
        registry
            .skills()
            .map(|skill| (skill.id().as_str(), skill.instructions()))
            .collect::<Vec<_>>(),
        vec![
            ("alpha.skill", "Use café.\n"),
            ("zeta.skill", "  keep zeta whitespace\n"),
        ]
    );
    assert_eq!(
        registry
            .get(&SkillId::new("zeta.skill").unwrap())
            .unwrap()
            .instructions(),
        "  keep zeta whitespace\n"
    );
}

#[test]
fn batch_registration_rejects_internal_and_existing_collisions_atomically() {
    for batch in [
        vec![
            skill("alpha.skill", "first"),
            skill("alpha.skill", "duplicate"),
        ],
        vec![skill("new.skill", "new"), skill("keep.skill", "collision")],
    ] {
        let mut registry = SkillRegistry::new();
        registry
            .register_all([skill("keep.skill", "original")])
            .unwrap();

        let error = registry.register_all(batch).unwrap_err();

        assert!(matches!(error, SkillRegistryError::DuplicateId { .. }));
        assert_eq!(
            registry
                .skills()
                .map(|skill| (skill.id().as_str(), skill.instructions()))
                .collect::<Vec<_>>(),
            vec![("keep.skill", "original")]
        );
    }
}

#[test]
fn debug_output_redacts_instruction_bodies() {
    let registered = skill("review.skill", "secret instruction body");
    let mut registry = SkillRegistry::new();
    registry.register_all([registered.clone()]).unwrap();

    let registered_debug = format!("{registered:?}");
    let registry_debug = format!("{registry:?}");

    assert!(registered_debug.contains("review.skill"));
    assert!(registered_debug.contains("instructions_len"));
    assert!(!registered_debug.contains("secret instruction body"));
    assert!(!registry_debug.contains("secret instruction body"));
}
