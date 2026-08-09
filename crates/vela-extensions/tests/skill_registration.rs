use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{
    ExtensionKind, ExtensionRegistry, SkillRegistrationError, register_skill_selection,
};
use vela_kernel::skill::{RegisteredSkill, SkillId, SkillRegistry};

#[test]
fn registers_prepared_skills_atomically_in_exact_id_order() {
    let root = tempdir().unwrap();
    write_extension(root.path(), "zeta", "zeta.skill", "skill", "Zeta.\n");
    write_extension(root.path(), "alpha", "alpha.skill", "skill", "Use café.\n");
    let extensions = ExtensionRegistry::discover(root.path()).unwrap();
    let selection = extensions
        .select_kind(ExtensionKind::Skill, ["zeta.skill", "alpha.skill"])
        .unwrap();
    let mut skills = SkillRegistry::new();

    register_skill_selection(root.path(), &selection, &mut skills).unwrap();

    assert_eq!(
        skills
            .skills()
            .map(|skill| (skill.id().as_str(), skill.instructions()))
            .collect::<Vec<_>>(),
        vec![("alpha.skill", "Use café.\n"), ("zeta.skill", "Zeta.\n")]
    );
}

#[test]
fn rejects_first_non_skill_before_filesystem_access() {
    let root = tempdir().unwrap();
    write_extension(root.path(), "zeta", "zeta.tool", "tool", "bytes");
    write_extension(root.path(), "alpha", "alpha.workflow", "workflow", "text");
    let extensions = ExtensionRegistry::discover(root.path()).unwrap();
    let selection = extensions.select(["zeta.tool", "alpha.workflow"]).unwrap();
    fs::remove_dir_all(root.path()).unwrap();
    let mut skills = SkillRegistry::new();

    let error = register_skill_selection(root.path(), &selection, &mut skills).unwrap_err();

    assert!(matches!(
        error,
        SkillRegistrationError::WrongKind {
            ref id,
            actual: ExtensionKind::Workflow,
        } if id == "alpha.workflow"
    ));
    assert!(error.source().is_none());
    assert_eq!(skills.skills().count(), 0);
}

#[test]
fn preparation_and_registry_failures_leave_existing_skills_unchanged() {
    for failure in ["preparation", "collision"] {
        let root = tempdir().unwrap();
        write_extension(root.path(), "alpha", "alpha.skill", "skill", "Alpha.\n");
        write_extension(root.path(), "zeta", "zeta.skill", "skill", "Zeta.\n");
        let extensions = ExtensionRegistry::discover(root.path()).unwrap();
        let selection = extensions
            .select_kind(ExtensionKind::Skill, ["alpha.skill", "zeta.skill"])
            .unwrap();
        let mut skills = SkillRegistry::new();
        skills
            .register_all([RegisteredSkill::new(
                SkillId::new(if failure == "collision" {
                    "zeta.skill"
                } else {
                    "keep.skill"
                })
                .unwrap(),
                "original",
            )])
            .unwrap();
        if failure == "preparation" {
            fs::remove_file(root.path().join("zeta/SKILL.org")).unwrap();
        }

        let error = register_skill_selection(root.path(), &selection, &mut skills).unwrap_err();

        assert!(matches!(
            (failure, &error),
            ("preparation", SkillRegistrationError::Preparation { .. })
                | ("collision", SkillRegistrationError::Registry { .. })
        ));
        assert_eq!(skills.skills().count(), 1);
        assert_eq!(skills.skills().next().unwrap().instructions(), "original");
    }
}

#[test]
fn empty_selection_needs_no_filesystem_and_does_not_mutate_registry() {
    let root = tempdir().unwrap();
    let extensions = ExtensionRegistry::discover(root.path()).unwrap();
    let selection = extensions
        .select_kind(ExtensionKind::Skill, std::iter::empty::<&str>())
        .unwrap();
    fs::remove_dir(root.path()).unwrap();
    let mut skills = SkillRegistry::new();
    skills
        .register_all([RegisteredSkill::new(
            SkillId::new("keep.skill").unwrap(),
            "keep",
        )])
        .unwrap();

    register_skill_selection(root.path(), &selection, &mut skills).unwrap();

    assert_eq!(skills.skills().count(), 1);
}

fn write_extension(root: &std::path::Path, package: &str, id: &str, kind: &str, content: &str) {
    let package = root.join(package);
    fs::create_dir(&package).unwrap();
    fs::write(
        package.join("extension.yaml"),
        format!("manifest_version: 1\nid: {id}\nkind: {kind}\nentrypoint: SKILL.org\n"),
    )
    .unwrap();
    fs::write(package.join("SKILL.org"), content).unwrap();
}
