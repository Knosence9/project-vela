use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{
    ExtensionKind, ExtensionPreparationError, ExtensionRegistry, MAX_SKILL_INSTRUCTION_BYTES,
    SkillPreparationError, prepare_skill_artifacts,
};

#[test]
fn prepares_exact_skill_instructions_in_id_order() {
    let root = tempdir().expect("temporary extension root");
    write_extension(
        root.path(),
        "zeta",
        "zeta.skill",
        "skill",
        "# Zeta\n\nDo zeta.\n",
    );
    write_extension(
        root.path(),
        "alpha",
        "alpha.skill",
        "skill",
        "# Alpha\n\nUse café.\n",
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Skill, ["zeta.skill", "alpha.skill"])
        .expect("skill selection");

    let artifacts = prepare_skill_artifacts(root.path(), &selection).expect("prepared skills");

    assert_eq!(
        artifacts
            .iter()
            .map(|artifact| (artifact.id(), artifact.instructions()))
            .collect::<Vec<_>>(),
        vec![
            ("alpha.skill", "# Alpha\n\nUse café.\n"),
            ("zeta.skill", "# Zeta\n\nDo zeta.\n"),
        ]
    );
    assert_eq!(
        format!("{:?}", artifacts[0]),
        "PreparedSkillArtifact { id: \"alpha.skill\", instructions_len: 20 }"
    );
    assert!(!format!("{:?}", artifacts[0]).contains("Use café"));
}

#[test]
fn rejects_first_non_skill_before_filesystem_access() {
    let root = tempdir().expect("temporary extension root");
    write_extension(root.path(), "zeta", "zeta.tool", "tool", "tool bytes");
    write_extension(
        root.path(),
        "alpha",
        "alpha.workflow",
        "workflow",
        "workflow text",
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select(["zeta.tool", "alpha.workflow"])
        .expect("generic selection");
    fs::remove_dir_all(root.path()).expect("remove root before kind preflight");

    let error = prepare_skill_artifacts(root.path(), &selection)
        .expect_err("non-skill selection must fail before root access");

    assert!(matches!(
        error,
        SkillPreparationError::WrongKind {
            ref id,
            actual: ExtensionKind::Workflow,
        } if id == "alpha.workflow"
    ));
    assert!(error.source().is_none());
}

#[test]
fn preparation_kind_mismatch_reports_the_requested_kind() {
    let error = ExtensionPreparationError::ExpectedKindMismatch {
        id: "review.skill".to_owned(),
        expected: ExtensionKind::Skill,
        actual: ExtensionKind::Tool,
    };

    assert_eq!(
        error.to_string(),
        "selected extension \"review.skill\" has kind Tool, expected Skill"
    );
}

#[test]
fn rejects_invalid_utf8_with_the_exact_id_and_source() {
    let root = tempdir().expect("temporary extension root");
    write_extension(root.path(), "skill", "review.skill", "skill", "valid");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Skill, ["review.skill"])
        .expect("skill selection");
    fs::write(root.path().join("skill/SKILL.md"), [0xff, 0xfe]).expect("write invalid UTF-8");

    let error = prepare_skill_artifacts(root.path(), &selection)
        .expect_err("invalid UTF-8 must fail preparation");

    assert!(matches!(
        error,
        SkillPreparationError::InvalidUtf8 { ref id, .. } if id == "review.skill"
    ));
    assert!(error.source().is_some());
}

#[test]
fn accepts_exact_limit_and_rejects_first_byte_beyond_it() {
    for (size, accepted) in [
        (MAX_SKILL_INSTRUCTION_BYTES as usize, true),
        (MAX_SKILL_INSTRUCTION_BYTES as usize + 1, false),
    ] {
        let root = tempdir().expect("temporary extension root");
        write_extension(root.path(), "skill", "bounded.skill", "skill", "seed");
        let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let selection = registry
            .select_kind(ExtensionKind::Skill, ["bounded.skill"])
            .expect("skill selection");
        fs::write(root.path().join("skill/SKILL.md"), vec![b'x'; size])
            .expect("write bounded instructions");

        let result = prepare_skill_artifacts(root.path(), &selection);
        if accepted {
            assert_eq!(
                result.expect("exact limit must be accepted")[0]
                    .instructions()
                    .len(),
                size
            );
        } else {
            assert!(matches!(
                result.expect_err("first byte beyond limit must fail"),
                SkillPreparationError::Preparation {
                    source: ExtensionPreparationError::EntrypointTooLarge {
                        ref id,
                        max_bytes: MAX_SKILL_INSTRUCTION_BYTES,
                        ..
                    }
                } if id == "bounded.skill"
            ));
        }
    }
}

#[test]
fn revalidates_manifest_package_and_entrypoint_and_returns_no_prefix() {
    let cases = ["manifest", "package", "entrypoint"];

    for case in cases {
        let root = tempdir().expect("temporary extension root");
        write_extension(root.path(), "alpha", "alpha.skill", "skill", "alpha");
        write_extension(root.path(), "zeta", "zeta.skill", "skill", "zeta");
        let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let selection = registry
            .select_kind(ExtensionKind::Skill, ["alpha.skill", "zeta.skill"])
            .expect("skill selection");

        match case {
            "manifest" => fs::write(
                root.path().join("zeta/extension.yaml"),
                manifest("changed.skill", "skill"),
            )
            .expect("change manifest"),
            "package" => {
                fs::rename(root.path().join("zeta"), root.path().join("old-zeta"))
                    .expect("move package");
                fs::create_dir(root.path().join("zeta")).expect("replace package");
            }
            "entrypoint" => {
                fs::remove_file(root.path().join("zeta/SKILL.md")).expect("remove entrypoint")
            }
            _ => unreachable!("fixed cases"),
        }

        let error = prepare_skill_artifacts(root.path(), &selection)
            .expect_err("changed selected package must reject the whole batch");
        assert!(matches!(error, SkillPreparationError::Preparation { .. }));
        let source = error.source().expect("preparation source");
        let preparation = source
            .downcast_ref::<ExtensionPreparationError>()
            .expect("typed preparation source");
        match case {
            "manifest" => assert!(matches!(
                preparation,
                ExtensionPreparationError::ManifestChanged { id, .. } if id == "zeta.skill"
            )),
            "package" => assert!(matches!(
                preparation,
                ExtensionPreparationError::PackageChanged { id, .. } if id == "zeta.skill"
            )),
            "entrypoint" => assert!(matches!(
                preparation,
                ExtensionPreparationError::Entrypoint { id, .. } if id == "zeta.skill"
            )),
            _ => unreachable!("fixed cases"),
        }
    }
}

#[cfg(unix)]
#[test]
fn rejects_an_entrypoint_replaced_by_a_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("temporary extension root");
    write_extension(root.path(), "skill", "review.skill", "skill", "safe");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Skill, ["review.skill"])
        .expect("skill selection");
    fs::remove_file(root.path().join("skill/SKILL.md")).expect("remove discovered entrypoint");
    fs::write(root.path().join("outside.md"), "unsafe alias").expect("write outside target");
    symlink("../outside.md", root.path().join("skill/SKILL.md")).expect("replace with symlink");

    let error = prepare_skill_artifacts(root.path(), &selection)
        .expect_err("descriptor-anchored preparation must reject an entrypoint symlink");

    assert!(matches!(
        error,
        SkillPreparationError::Preparation {
            source: ExtensionPreparationError::Entrypoint { ref id, .. }
        } if id == "review.skill"
    ));
    assert!(error.source().is_some());
}

#[test]
fn empty_skill_selection_needs_no_filesystem() {
    let root = tempdir().expect("temporary extension root");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Skill, std::iter::empty::<&str>())
        .expect("empty selection");
    fs::remove_dir(root.path()).expect("remove unused root");

    assert!(
        prepare_skill_artifacts(root.path(), &selection)
            .expect("empty preparation")
            .is_empty()
    );
}

fn write_extension(root: &std::path::Path, package: &str, id: &str, kind: &str, content: &str) {
    let package = root.join(package);
    fs::create_dir(&package).expect("create package");
    fs::write(package.join("extension.yaml"), manifest(id, kind)).expect("write manifest");
    fs::write(package.join("SKILL.md"), content).expect("write entrypoint");
}

fn manifest(id: &str, kind: &str) -> String {
    format!("manifest_version: 1\nid: {id}\nkind: {kind}\nentrypoint: SKILL.md\n")
}
