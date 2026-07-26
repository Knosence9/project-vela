use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{
    ExtensionDiscoveryError, ExtensionKind, ExtensionManifest, ExtensionManifestError,
    ExtensionPreparationError, ExtensionRegistry, ExtensionRegistryChange, ExtensionSelectionError,
    MAX_ENTRYPOINT_BYTES, MAX_MANIFEST_BYTES, discover_extensions, prepare_tool_artifacts,
};

#[test]
fn loads_a_valid_version_one_manifest() {
    let manifest = ExtensionManifest::load(fixture("valid.yaml")).expect("valid manifest");

    assert_eq!(manifest.manifest_version(), 1);
    assert_eq!(manifest.id(), "local.search");
    assert_eq!(manifest.kind(), ExtensionKind::Tool);
    assert_eq!(manifest.entrypoint(), "local-search");
    assert_eq!(manifest.description(), Some("Searches local project files"));
}

#[test]
fn accepts_each_supported_kind() {
    for (kind, expected) in [
        ("tool", ExtensionKind::Tool),
        ("skill", ExtensionKind::Skill),
        ("workflow", ExtensionKind::Workflow),
    ] {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("extension.yaml");
        fs::write(
            &path,
            format!("manifest_version: 1\nid: test.{kind}\nkind: {kind}\nentrypoint: run\n"),
        )
        .expect("write manifest");

        let manifest = ExtensionManifest::load(path).expect("supported kind");
        assert_eq!(manifest.kind(), expected);
    }
}

#[test]
fn accepts_portable_relative_entrypoints() {
    for entrypoint in ["SKILL.md", "bin/search.wasm", "workflows/review.yaml"] {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("extension.yaml");
        fs::write(
            &path,
            format!(
                "manifest_version: 1\nid: test.entrypoint\nkind: tool\nentrypoint: {entrypoint}\n"
            ),
        )
        .expect("write manifest");

        let manifest = ExtensionManifest::load(path).expect("portable relative entrypoint");
        assert_eq!(manifest.entrypoint(), entrypoint);
    }
}

#[test]
fn rejects_non_portable_entrypoints() {
    for entrypoint in [
        "/usr/bin/search",
        "C:/tools/search.exe",
        "bin//search",
        "./search",
        "bin/../search",
        "bin/ /search",
        "bin/.. /search",
        "bin/. ./search",
        "bin/search:prod",
        "bin/search*beta",
        "bin/search?beta",
        "bin/search.",
        "bin/search ",
        "bin/CON.txt",
        "bin/CON .txt",
        "bin/LPT1 .md",
        "bin/conout$.log",
        "bin/COM¹.txt",
        r"bin\search",
        "bin/search\0hidden",
    ] {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("extension.yaml");
        let yaml_entrypoint = serde_norway::to_string(entrypoint).expect("encode entrypoint");
        fs::write(
            &path,
            format!(
                "manifest_version: 1\nid: test.entrypoint\nkind: tool\nentrypoint: {yaml_entrypoint}"
            ),
        )
        .expect("write manifest");

        let error = ExtensionManifest::load(path).expect_err("invalid entrypoint");
        assert!(
            matches!(error, ExtensionManifestError::InvalidEntrypoint),
            "unexpected error for {entrypoint:?}: {error}"
        );
        assert_eq!(
            error.to_string(),
            "extension manifest field entrypoint must be a portable relative path"
        );
        assert!(error.source().is_none());
    }
}

#[test]
fn rejects_unsupported_versions() {
    let error = ExtensionManifest::load(fixture("unsupported-version.yaml"))
        .expect_err("unsupported version");

    assert!(matches!(
        error,
        ExtensionManifestError::UnsupportedVersion { version: 2 }
    ));
    assert!(error.source().is_none());
}

#[test]
fn rejects_blank_required_strings() {
    for (fixture_name, field) in [
        ("blank-id.yaml", "id"),
        ("blank-entrypoint.yaml", "entrypoint"),
    ] {
        let error = ExtensionManifest::load(fixture(fixture_name)).expect_err("blank field");
        assert!(matches!(
            error,
            ExtensionManifestError::BlankField { field: actual } if actual == field
        ));
    }
}

#[test]
fn rejects_unsupported_kinds() {
    let error =
        ExtensionManifest::load(fixture("unsupported-kind.yaml")).expect_err("unsupported kind");

    assert!(matches!(
        error,
        ExtensionManifestError::UnsupportedKind { ref kind } if kind == "service"
    ));
}

#[test]
fn preserves_yaml_parser_errors_as_sources() {
    let error = ExtensionManifest::load(fixture("malformed.txt")).expect_err("malformed YAML");

    assert!(matches!(error, ExtensionManifestError::Parse { .. }));
    assert!(error.source().is_some());
}

#[test]
fn bounds_manifest_input_before_parsing() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("extension.yaml");
    let prefix =
        "manifest_version: 1\nid: test.boundary\nkind: tool\nentrypoint: run\ndescription: ";
    let maximum = usize::try_from(MAX_MANIFEST_BYTES).expect("manifest limit fits usize");
    let padding = "x".repeat(maximum - prefix.len());

    fs::write(&path, format!("{prefix}{padding}")).expect("write maximum-size manifest");
    ExtensionManifest::load(&path).expect("maximum-size manifest is accepted");

    fs::write(&path, format!("{prefix}{padding}x")).expect("write oversized manifest");
    let error = ExtensionManifest::load(&path).expect_err("first oversized byte is rejected");
    assert!(matches!(
        error,
        ExtensionManifestError::TooLarge { max_bytes } if max_bytes == MAX_MANIFEST_BYTES
    ));
    assert!(error.source().is_none());

    fs::write(&path, format!("{prefix}{padding}é"))
        .expect("write oversized manifest crossing a UTF-8 boundary");
    let error = ExtensionManifest::load(path).expect_err("UTF-8 boundary oversize is rejected");
    assert!(matches!(error, ExtensionManifestError::TooLarge { .. }));
}

#[cfg(unix)]
#[test]
fn discovers_extension_manifests_in_sorted_path_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("zeta/extension.yaml"), "zeta.tool");
    write_manifest(root.path().join("alpha/extension.yaml"), "alpha.tool");

    let discovered = discover_extensions(root.path()).expect("discover extensions");

    assert_eq!(
        discovered
            .iter()
            .map(|extension| extension.manifest().id())
            .collect::<Vec<_>>(),
        ["alpha.tool", "zeta.tool"]
    );
    assert_eq!(
        discovered[0].path(),
        root.path().join("alpha/extension.yaml")
    );
    assert_eq!(
        discovered[1].path(),
        root.path().join("zeta/extension.yaml")
    );
}

#[cfg(unix)]
#[test]
fn discovery_accepts_a_nested_regular_file_entrypoint_without_reading_it() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().expect("temporary extension root");
    let manifest_path = root.path().join("alpha/extension.yaml");
    let entrypoint_path = root.path().join("alpha/bin/run");
    write_manifest_with_entrypoint(&manifest_path, "alpha.tool", "bin/run");
    fs::create_dir_all(root.path().join("alpha/bin")).expect("create entrypoint directory");
    fs::write(&entrypoint_path, "not executed").expect("write entrypoint");
    fs::set_permissions(&entrypoint_path, fs::Permissions::from_mode(0o111))
        .expect("remove entrypoint read permission");

    let discovered = discover_extensions(root.path()).expect("regular entrypoint target");

    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest().entrypoint(), "bin/run");
}

#[cfg(unix)]
#[test]
fn discovery_rejects_missing_symlinked_and_non_regular_entrypoint_targets() {
    use std::os::unix::fs::symlink;

    for (case, entrypoint) in [
        ("missing", "missing"),
        ("symlink", "linked"),
        ("intermediate-symlink", "linked-directory/run"),
        ("directory", "directory"),
    ] {
        let root = tempdir().expect("temporary extension root");
        let manifest_path = root.path().join("alpha/extension.yaml");
        write_manifest_with_entrypoint(&manifest_path, "alpha.tool", entrypoint);
        match case {
            "symlink" => {
                fs::write(root.path().join("alpha/outside"), "target").expect("write target");
                symlink("outside", root.path().join("alpha/linked")).expect("link entrypoint");
            }
            "intermediate-symlink" => {
                fs::create_dir(root.path().join("alpha/real-directory"))
                    .expect("create real directory");
                fs::write(root.path().join("alpha/real-directory/run"), "target")
                    .expect("write nested target");
                symlink("real-directory", root.path().join("alpha/linked-directory"))
                    .expect("link entrypoint directory");
            }
            "directory" => {
                fs::create_dir(root.path().join("alpha/directory"))
                    .expect("create entrypoint directory");
            }
            _ => {}
        }

        let error = discover_extensions(root.path()).expect_err("unsafe entrypoint target");

        assert!(matches!(
            error,
            ExtensionDiscoveryError::Entrypoint {
                ref path,
                entrypoint: ref actual_entrypoint,
                ..
            } if path == &manifest_path && actual_entrypoint == entrypoint
        ));
        assert!(error.source().is_some());
    }
}

#[cfg(unix)]
#[test]
fn discovery_rejects_the_first_exact_duplicate_id_with_deterministic_paths() {
    let root = tempdir().expect("temporary extension root");
    let first_path = root.path().join("alpha/extension.yaml");
    let duplicate_path = root.path().join("middle/extension.yaml");
    write_manifest(root.path().join("zeta/extension.yaml"), "shared.tool");
    write_manifest(duplicate_path.clone(), "shared.tool");
    write_manifest(first_path.clone(), "shared.tool");

    let error = discover_extensions(root.path()).expect_err("duplicate ID is rejected");

    assert!(matches!(
        error,
        ExtensionDiscoveryError::DuplicateId {
            ref id,
            first_path: ref actual_first_path,
            duplicate_path: ref actual_duplicate_path,
        } if id == "shared.tool"
            && actual_first_path == &first_path
            && actual_duplicate_path == &duplicate_path
    ));
    assert!(error.source().is_none());
}

#[cfg(unix)]
#[test]
fn duplicate_id_diagnostics_escape_untrusted_id_content() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(
        root.path().join("alpha/extension.yaml"),
        "\"shared\\n.tool\"",
    );
    write_manifest(
        root.path().join("zeta/extension.yaml"),
        "\"shared\\n.tool\"",
    );

    let error = discover_extensions(root.path()).expect_err("duplicate ID is rejected");

    assert!(
        error
            .to_string()
            .starts_with("duplicate extension ID \"shared\\n.tool\" in ")
    );
}

#[cfg(unix)]
#[test]
fn discovery_preserves_case_and_whitespace_when_comparing_ids() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("alpha/extension.yaml"), "shared.tool");
    write_manifest(root.path().join("middle/extension.yaml"), "Shared.Tool");
    write_manifest(root.path().join("zeta/extension.yaml"), "'shared.tool '");

    let discovered = discover_extensions(root.path()).expect("distinct exact IDs");

    assert_eq!(
        discovered
            .iter()
            .map(|extension| extension.manifest().id())
            .collect::<Vec<_>>(),
        ["shared.tool", "Shared.Tool", "shared.tool "]
    );
}

#[cfg(unix)]
#[test]
fn discovery_is_shallow_and_ignores_unrelated_entries() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("valid/extension.yaml"), "valid.tool");
    write_manifest(root.path().join("extension.yaml"), "root.tool");
    write_manifest(
        root.path().join("nested/deeper/extension.yaml"),
        "nested.tool",
    );
    fs::write(root.path().join("notes.txt"), "not a manifest").expect("write unrelated file");
    fs::create_dir(root.path().join("empty")).expect("create empty extension directory");

    let discovered = discover_extensions(root.path()).expect("discover extensions");

    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest().id(), "valid.tool");
}

#[cfg(unix)]
#[test]
fn discovery_reports_the_first_sorted_invalid_manifest_with_its_path() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("zeta/extension.yaml"), "zeta.tool");
    let invalid_path = root.path().join("alpha/extension.yaml");
    fs::create_dir_all(invalid_path.parent().expect("manifest parent"))
        .expect("create extension directory");
    fs::write(&invalid_path, "manifest_version: nope").expect("write invalid manifest");

    let error = discover_extensions(root.path()).expect_err("invalid discovery");

    assert!(matches!(
        error,
        ExtensionDiscoveryError::Manifest { ref path, .. } if path == &invalid_path
    ));
    assert!(error.source().is_some());
}

#[cfg(unix)]
#[test]
fn discovery_rejects_symlinked_candidates_without_following_targets() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("temporary extension root");
    let outside = tempdir().expect("external directory");
    let external_manifest = outside.path().join("external.yaml");
    write_manifest(external_manifest.clone(), "external.tool");
    let linked_candidate = root.path().join("alpha/extension.yaml");
    fs::create_dir_all(linked_candidate.parent().expect("manifest parent"))
        .expect("create extension directory");
    symlink(&external_manifest, &linked_candidate).expect("link external manifest");

    let error = discover_extensions(root.path()).expect_err("external symlink is rejected");

    assert!(matches!(
        error,
        ExtensionDiscoveryError::Manifest { ref path, .. } if path == &linked_candidate
    ));
    assert!(error.source().is_some());

    let dangling_root = tempdir().expect("temporary extension root");
    let dangling_candidate = dangling_root.path().join("alpha/extension.yaml");
    fs::create_dir_all(dangling_candidate.parent().expect("manifest parent"))
        .expect("create extension directory");
    symlink("missing.yaml", &dangling_candidate).expect("create dangling symlink");

    let error =
        discover_extensions(dangling_root.path()).expect_err("dangling symlink is rejected");

    assert!(matches!(
        error,
        ExtensionDiscoveryError::Manifest { ref path, .. } if path == &dangling_candidate
    ));
    assert!(error.source().is_some());
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
#[test]
fn discovery_ignores_fifo_manifest_candidates_without_blocking() {
    use rustix::fs::{Mode, mkfifoat};

    let root = tempdir().expect("temporary extension root");
    let child = root.path().join("fifo");
    fs::create_dir(&child).expect("create extension directory");
    let child = fs::File::open(child).expect("open extension directory");
    mkfifoat(&child, c"extension.yaml", Mode::RUSR | Mode::WUSR).expect("create manifest fifo");

    let discovered = discover_extensions(root.path()).expect("ignore non-file candidate");

    assert!(discovered.is_empty());
}

#[cfg(unix)]
#[test]
fn discovery_reports_an_unreadable_child_directory_path() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().expect("temporary extension root");
    let child = root.path().join("blocked");
    fs::create_dir(&child).expect("create blocked child");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o000)).expect("block child reads");

    let result = discover_extensions(root.path());

    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).expect("restore child access");
    let error = result.expect_err("unreadable child is reported");
    assert!(matches!(
        error,
        ExtensionDiscoveryError::ReadRoot { ref path, .. } if path == &child
    ));
    assert!(error.source().is_some());
}

#[test]
fn discovery_preserves_root_enumeration_errors() {
    let root = tempdir().expect("temporary extension root");
    let missing = root.path().join("missing");

    let error = discover_extensions(&missing).expect_err("missing extension root");

    assert!(matches!(
        error,
        ExtensionDiscoveryError::ReadRoot { ref path, .. } if path == &missing
    ));
    assert!(error.source().is_some());
}

#[cfg(unix)]
#[test]
fn registry_resolves_exact_ids_and_enumerates_in_manifest_path_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("zeta/extension.yaml"), "alpha.id");
    write_manifest(root.path().join("alpha/extension.yaml"), "zeta.id");

    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");

    assert_eq!(
        registry
            .extensions()
            .map(|extension| extension.manifest().id())
            .collect::<Vec<_>>(),
        ["zeta.id", "alpha.id"]
    );
    assert_eq!(
        registry.get("alpha.id").expect("exact ID").path(),
        root.path().join("zeta/extension.yaml")
    );
    assert!(registry.get("Alpha.Id").is_none());
    assert!(registry.get(" alpha.id").is_none());
}

#[cfg(unix)]
#[test]
fn registry_preserves_discovery_failures() {
    let root = tempdir().expect("temporary extension root");
    let invalid_path = root.path().join("invalid/extension.yaml");
    fs::create_dir_all(invalid_path.parent().expect("manifest parent"))
        .expect("create extension directory");
    fs::write(&invalid_path, "manifest_version: nope").expect("write invalid manifest");

    let error = ExtensionRegistry::discover(root.path()).expect_err("invalid registry");

    assert!(matches!(
        error,
        ExtensionDiscoveryError::Manifest { ref path, .. } if path == &invalid_path
    ));
}

#[cfg(unix)]
#[test]
fn registry_is_an_explicit_immutable_snapshot() {
    let root = tempdir().expect("temporary extension root");
    let registry = ExtensionRegistry::discover(root.path()).expect("empty registry");
    assert_eq!(registry.extensions().count(), 0);

    write_manifest(root.path().join("later/extension.yaml"), "later.tool");

    assert!(registry.get("later.tool").is_none());
    assert_eq!(registry.extensions().count(), 0);

    let refreshed = ExtensionRegistry::discover(root.path()).expect("refreshed registry");
    assert_eq!(refreshed.extensions().count(), 1);
    assert!(refreshed.get("later.tool").is_some());
}

#[cfg(unix)]
#[test]
fn registry_compares_snapshots_by_exact_id_in_deterministic_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("unchanged/extension.yaml"), "unchanged.id");
    write_manifest(root.path().join("changed/extension.yaml"), "changed.id");
    write_manifest(root.path().join("removed/extension.yaml"), "removed.id");
    let previous = ExtensionRegistry::discover(root.path()).expect("previous registry");

    fs::remove_dir_all(root.path().join("removed")).expect("remove extension");
    write_manifest(root.path().join("added/extension.yaml"), "added.id");
    fs::write(
        root.path().join("changed/extension.yaml"),
        "manifest_version: 1\nid: changed.id\nkind: tool\nentrypoint: run\ndescription: changed\n",
    )
    .expect("change manifest metadata");
    let current = ExtensionRegistry::discover(root.path()).expect("current registry");

    let changes = current.changes_from(&previous);

    assert_eq!(changes.len(), 3);
    assert!(matches!(
        changes[0],
        ExtensionRegistryChange::Added(extension)
            if extension.manifest().id() == "added.id"
    ));
    assert!(matches!(
        changes[1],
        ExtensionRegistryChange::Changed { previous, current }
            if previous.manifest().description().is_none()
                && current.manifest().description() == Some("changed")
    ));
    assert!(matches!(
        changes[2],
        ExtensionRegistryChange::Removed(extension)
            if extension.manifest().id() == "removed.id"
    ));
    assert!(current.changes_from(&current).is_empty());
}

#[cfg(unix)]
#[test]
fn registry_comparison_reports_source_path_changes() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("before/extension.yaml"), "moved.id");
    let previous = ExtensionRegistry::discover(root.path()).expect("previous registry");

    fs::rename(root.path().join("before"), root.path().join("after"))
        .expect("move extension directory");
    let current = ExtensionRegistry::discover(root.path()).expect("current registry");

    let changes = current.changes_from(&previous);

    assert!(matches!(
        changes.as_slice(),
        [ExtensionRegistryChange::Changed { previous, current }]
            if previous.path() == root.path().join("before/extension.yaml")
                && current.path() == root.path().join("after/extension.yaml")
    ));
}

#[cfg(unix)]
#[test]
fn registry_comparison_preserves_exact_id_semantics() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("lower/extension.yaml"), "shared.id");
    let previous = ExtensionRegistry::discover(root.path()).expect("previous registry");

    write_manifest(root.path().join("upper/extension.yaml"), "Shared.id");
    write_manifest(root.path().join("spaced/extension.yaml"), "'shared.id '");
    let current = ExtensionRegistry::discover(root.path()).expect("current registry");

    let added_ids = current
        .changes_from(&previous)
        .into_iter()
        .map(|change| match change {
            ExtensionRegistryChange::Added(extension) => extension.manifest().id(),
            other => panic!("expected added extension, got {other:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(added_ids, ["Shared.id", "shared.id "]);
}

#[cfg(unix)]
#[test]
fn registry_selects_exact_ids_in_deterministic_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("zeta/extension.yaml"), "zeta.id");
    write_manifest(root.path().join("upper/extension.yaml"), "Alpha.id");
    write_manifest(root.path().join("spaced/extension.yaml"), "'alpha.id '");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");

    let selected = registry
        .select(["zeta.id", "alpha.id ", "Alpha.id"])
        .expect("valid exact-ID selection");

    assert_eq!(selected.len(), 3);
    assert!(!selected.is_empty());
    assert_eq!(
        selected
            .extensions()
            .map(|extension| extension.manifest().id())
            .collect::<Vec<_>>(),
        ["Alpha.id", "alpha.id ", "zeta.id"]
    );
    assert_eq!(
        selected
            .get("alpha.id ")
            .expect("selected exact ID")
            .manifest()
            .id(),
        "alpha.id "
    );
    assert!(selected.get("alpha.id").is_none());

    let empty = registry
        .select(std::iter::empty::<&str>())
        .expect("empty selection");
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.extensions().next().is_none());
    assert!(empty.get("Alpha.id").is_none());
}

#[cfg(unix)]
#[test]
fn registry_selection_fails_closed_for_duplicate_or_unknown_ids() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("known/extension.yaml"), "known.id");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");

    let duplicate = registry
        .select(["known.id", "known.id"])
        .expect_err("duplicate request");
    assert!(matches!(
        duplicate,
        ExtensionSelectionError::DuplicateId { ref id } if id == "known.id"
    ));
    assert_eq!(
        duplicate.to_string(),
        "extension selection contains duplicate ID \"known.id\""
    );
    assert!(duplicate.source().is_none());

    let unknown = registry
        .select(["known.id", "Known.id"])
        .expect_err("unknown exact ID");
    assert!(matches!(
        unknown,
        ExtensionSelectionError::NotFound { ref id } if id == "Known.id"
    ));
    assert_eq!(
        unknown.to_string(),
        "extension ID \"Known.id\" was not found"
    );
    assert!(unknown.source().is_none());
}

#[cfg(unix)]
#[test]
fn registry_selects_one_expected_kind_in_exact_id_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest_with_kind(root.path().join("zeta/extension.yaml"), "zeta.tool", "tool");
    write_manifest_with_kind(
        root.path().join("alpha/extension.yaml"),
        "alpha.tool",
        "tool",
    );
    write_manifest_with_kind(
        root.path().join("skill/extension.yaml"),
        "review.skill",
        "skill",
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");

    let tools = registry
        .select_kind(ExtensionKind::Tool, ["zeta.tool", "alpha.tool"])
        .expect("kind-constrained selection");

    assert_eq!(
        tools
            .extensions()
            .map(|extension| extension.manifest().id())
            .collect::<Vec<_>>(),
        ["alpha.tool", "zeta.tool"]
    );
    assert!(
        registry
            .select_kind(ExtensionKind::Workflow, std::iter::empty::<&str>())
            .expect("empty kind-constrained selection")
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn kind_constrained_selection_fails_closed_with_deterministic_errors() {
    let root = tempdir().expect("temporary extension root");
    write_manifest_with_kind(root.path().join("tool/extension.yaml"), "zeta.tool", "tool");
    write_manifest_with_kind(
        root.path().join("skill/extension.yaml"),
        "alpha.skill",
        "skill",
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");

    let duplicate = registry
        .select_kind(
            ExtensionKind::Tool,
            ["zeta.tool", "zeta.tool", "alpha.skill", "alpha.skill"],
        )
        .expect_err("first duplicate takes precedence");
    assert!(matches!(
        duplicate,
        ExtensionSelectionError::DuplicateId { ref id } if id == "alpha.skill"
    ));

    let mismatch = registry
        .select_kind(ExtensionKind::Tool, ["zeta.tool", "alpha.skill"])
        .expect_err("wrong kind");
    assert!(matches!(
        mismatch,
        ExtensionSelectionError::KindMismatch {
            ref id,
            expected: ExtensionKind::Tool,
            actual: ExtensionKind::Skill,
        } if id == "alpha.skill"
    ));
    assert_eq!(
        mismatch.to_string(),
        "extension ID \"alpha.skill\" has kind Skill, expected Tool"
    );
    assert!(mismatch.source().is_none());

    let unknown = registry
        .select_kind(
            ExtensionKind::Tool,
            ["zeta.tool", "Alpha.missing", "alpha.skill"],
        )
        .expect_err("first exact ID lookup error");
    assert!(matches!(
        unknown,
        ExtensionSelectionError::NotFound { ref id } if id == "Alpha.missing"
    ));
}

#[cfg(unix)]
#[test]
fn selection_projects_capabilities_by_kind_in_exact_id_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest_with_kind(root.path().join("zeta/extension.yaml"), "zeta.tool", "tool");
    write_manifest_with_kind(
        root.path().join("alpha/extension.yaml"),
        "alpha.skill",
        "skill",
    );
    write_manifest_with_kind(
        root.path().join("middle/extension.yaml"),
        "middle.tool",
        "tool",
    );
    write_manifest_with_kind(
        root.path().join("workflow/extension.yaml"),
        "review.workflow",
        "workflow",
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select(["zeta.tool", "alpha.skill", "middle.tool", "review.workflow"])
        .expect("mixed selection");

    let tools = selection.of_kind(ExtensionKind::Tool);

    assert_eq!(tools.len(), 2);
    assert!(!tools.is_empty());
    assert_eq!(
        tools
            .extensions()
            .map(|extension| extension.manifest().id())
            .collect::<Vec<_>>(),
        ["middle.tool", "zeta.tool"]
    );
    assert_eq!(
        tools
            .get("zeta.tool")
            .expect("selected tool")
            .manifest()
            .kind(),
        ExtensionKind::Tool
    );
    assert!(tools.get("alpha.skill").is_none());
    assert!(tools.get("missing.tool").is_none());
    assert_eq!(
        selection
            .of_kind(ExtensionKind::Workflow)
            .extensions()
            .next()
            .map(|extension| extension.manifest().id()),
        Some("review.workflow")
    );
}

#[cfg(unix)]
#[test]
fn selection_kind_projection_can_be_empty() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("known/extension.yaml"), "known.tool");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry.select(["known.tool"]).expect("tool selection");

    let workflows = selection.of_kind(ExtensionKind::Workflow);

    assert!(workflows.is_empty());
    assert_eq!(workflows.len(), 0);
    assert!(workflows.extensions().next().is_none());
    assert!(workflows.get("known.tool").is_none());
}

#[cfg(unix)]
#[test]
fn preparation_reacquires_owned_tool_artifacts_in_exact_id_order() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("first/extension.yaml"), "zeta.tool");
    write_manifest(root.path().join("second/extension.yaml"), "alpha.tool");
    fs::write(root.path().join("second/run"), b"alpha component").expect("write alpha bytes");
    fs::write(root.path().join("first/run"), b"zeta component").expect("write zeta bytes");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Tool, ["zeta.tool", "alpha.tool"])
        .expect("tool selection");

    let artifacts =
        prepare_tool_artifacts(root.path(), &selection).expect("prepared tool artifacts");

    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0].id(), "alpha.tool");
    assert_eq!(artifacts[0].bytes(), b"alpha component");
    assert_eq!(artifacts[1].id(), "zeta.tool");
    assert_eq!(artifacts[1].bytes(), b"zeta component");
}

#[cfg(unix)]
#[test]
fn preparation_accepts_equivalent_root_paths() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().expect("temporary parent");
    let real_parent = parent.path().join("real");
    let root = real_parent.join("extensions");
    write_manifest(root.join("tool/extension.yaml"), "equivalent.tool");
    let registry = ExtensionRegistry::discover(&root).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Tool, ["equivalent.tool"])
        .expect("tool selection");
    fs::create_dir(parent.path().join("detour")).expect("create path detour");
    let lexical_root = parent.path().join("detour/../real/extensions");

    let lexical_artifacts = prepare_tool_artifacts(&lexical_root, &selection)
        .expect("lexically equivalent root must prepare artifacts");

    assert_eq!(lexical_artifacts.len(), 1);
    assert_eq!(lexical_artifacts[0].id(), "equivalent.tool");

    symlink(&real_parent, parent.path().join("alias")).expect("create parent alias");
    let alias_root = parent.path().join("alias/extensions");
    let alias_artifacts = prepare_tool_artifacts(&alias_root, &selection)
        .expect("aliased parent path must prepare artifacts");

    assert_eq!(alias_artifacts.len(), 1);
    assert_eq!(alias_artifacts[0].id(), "equivalent.tool");
}

#[cfg(unix)]
#[test]
fn preparation_fails_closed_for_wrong_kind_root_and_changed_manifest() {
    let root = tempdir().expect("temporary extension root");
    write_manifest_with_kind(
        root.path().join("skill/extension.yaml"),
        "review.skill",
        "skill",
    );
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select(["review.skill"])
        .expect("generic selection");

    let wrong_kind =
        prepare_tool_artifacts(root.path(), &selection).expect_err("wrong kind must fail");
    assert!(matches!(
        wrong_kind,
        ExtensionPreparationError::KindMismatch {
            ref id,
            actual: ExtensionKind::Skill,
        } if id == "review.skill"
    ));

    let other_root = tempdir().expect("different extension root");
    let wrong_root =
        prepare_tool_artifacts(other_root.path(), &selection).expect_err("wrong root must fail");
    assert!(matches!(
        wrong_root,
        ExtensionPreparationError::SourceMismatch { ref id, .. } if id == "review.skill"
    ));

    write_manifest(root.path().join("tool/extension.yaml"), "changed.tool");
    let changed_registry = ExtensionRegistry::discover(root.path()).expect("changed registry");
    let changed_selection = changed_registry
        .select_kind(ExtensionKind::Tool, ["changed.tool"])
        .expect("tool selection");
    fs::write(
        root.path().join("tool/extension.yaml"),
        "manifest_version: 1\nid: changed.tool\nkind: tool\nentrypoint: run\ndescription: changed\n",
    )
    .expect("change manifest");

    let changed = prepare_tool_artifacts(root.path(), &changed_selection)
        .expect_err("changed manifest must fail");
    assert!(matches!(
        changed,
        ExtensionPreparationError::ManifestChanged { ref id, .. } if id == "changed.tool"
    ));
}

#[cfg(unix)]
#[test]
fn preparation_rejects_replaced_moved_and_unsafe_selected_targets() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("tool/extension.yaml"), "stable.tool");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Tool, ["stable.tool"])
        .expect("tool selection");
    fs::rename(root.path().join("tool"), root.path().join("moved")).expect("move selected package");
    write_manifest(root.path().join("tool/extension.yaml"), "stable.tool");
    assert!(matches!(
        prepare_tool_artifacts(root.path(), &selection),
        Err(ExtensionPreparationError::PackageChanged { ref id, .. }) if id == "stable.tool"
    ));
    fs::remove_dir_all(root.path().join("tool")).expect("remove replacement");
    assert!(matches!(
        prepare_tool_artifacts(root.path(), &selection),
        Err(ExtensionPreparationError::Package { ref id, .. }) if id == "stable.tool"
    ));

    let unsafe_root = tempdir().expect("temporary extension root");
    write_manifest(
        unsafe_root.path().join("tool/extension.yaml"),
        "unsafe.tool",
    );
    let unsafe_registry =
        ExtensionRegistry::discover(unsafe_root.path()).expect("extension registry");
    let unsafe_selection = unsafe_registry
        .select_kind(ExtensionKind::Tool, ["unsafe.tool"])
        .expect("tool selection");
    fs::remove_file(unsafe_root.path().join("tool/run")).expect("remove entrypoint");
    fs::write(unsafe_root.path().join("tool/outside"), "component").expect("write target");
    symlink("outside", unsafe_root.path().join("tool/run")).expect("link target");
    let error = prepare_tool_artifacts(unsafe_root.path(), &unsafe_selection)
        .expect_err("symlink must fail");
    assert!(matches!(
        error,
        ExtensionPreparationError::Entrypoint { ref id, .. } if id == "unsafe.tool"
    ));
    assert!(error.source().is_some());
}

#[cfg(unix)]
#[test]
fn preparation_rejects_missing_non_regular_and_intermediate_symlink_targets() {
    use std::os::unix::fs::symlink;

    for case in ["missing", "directory", "intermediate-symlink"] {
        let root = tempdir().expect("temporary extension root");
        let entrypoint = if case == "intermediate-symlink" {
            "nested/run"
        } else {
            "run"
        };
        write_manifest_with_entrypoint(
            &root.path().join("tool/extension.yaml"),
            "unsafe.tool",
            entrypoint,
        );
        let target = root.path().join("tool").join(entrypoint);
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
        fs::write(&target, "component").expect("write initial target");
        let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
        let selection = registry
            .select_kind(ExtensionKind::Tool, ["unsafe.tool"])
            .expect("tool selection");

        if case == "intermediate-symlink" {
            fs::remove_dir_all(root.path().join("tool/nested")).expect("remove nested target");
            fs::create_dir(root.path().join("tool/real")).expect("create real target directory");
            fs::write(root.path().join("tool/real/run"), "component").expect("write target");
            symlink("real", root.path().join("tool/nested")).expect("link intermediate directory");
        } else {
            fs::remove_file(root.path().join("tool/run")).expect("remove target");
            if case == "directory" {
                fs::create_dir(root.path().join("tool/run")).expect("create target directory");
            }
        }

        assert!(
            matches!(
                prepare_tool_artifacts(root.path(), &selection),
                Err(ExtensionPreparationError::Entrypoint { ref id, .. }) if id == "unsafe.tool"
            ),
            "case {case} must fail with an entrypoint error"
        );
    }
}

#[cfg(unix)]
#[test]
fn preparation_rejects_symlinks_at_root_package_and_manifest_boundaries() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().expect("temporary parent");
    let root = parent.path().join("extensions");
    write_manifest(root.join("tool/extension.yaml"), "root.tool");
    let registry = ExtensionRegistry::discover(&root).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Tool, ["root.tool"])
        .expect("tool selection");
    fs::rename(&root, parent.path().join("real-root")).expect("move root");
    symlink("real-root", &root).expect("link root");
    assert!(matches!(
        prepare_tool_artifacts(&root, &selection),
        Err(ExtensionPreparationError::ReadRoot { .. })
    ));

    let package_root = tempdir().expect("temporary extension root");
    write_manifest(
        package_root.path().join("tool/extension.yaml"),
        "package.tool",
    );
    let package_registry =
        ExtensionRegistry::discover(package_root.path()).expect("extension registry");
    let package_selection = package_registry
        .select_kind(ExtensionKind::Tool, ["package.tool"])
        .expect("tool selection");
    fs::rename(
        package_root.path().join("tool"),
        package_root.path().join("real-tool"),
    )
    .expect("move package");
    symlink("real-tool", package_root.path().join("tool")).expect("link package");
    assert!(matches!(
        prepare_tool_artifacts(package_root.path(), &package_selection),
        Err(ExtensionPreparationError::Package { .. })
    ));

    let manifest_root = tempdir().expect("temporary extension root");
    write_manifest(
        manifest_root.path().join("tool/extension.yaml"),
        "manifest.tool",
    );
    let manifest_registry =
        ExtensionRegistry::discover(manifest_root.path()).expect("extension registry");
    let manifest_selection = manifest_registry
        .select_kind(ExtensionKind::Tool, ["manifest.tool"])
        .expect("tool selection");
    fs::rename(
        manifest_root.path().join("tool/extension.yaml"),
        manifest_root.path().join("tool/real.yaml"),
    )
    .expect("move manifest");
    symlink(
        "real.yaml",
        manifest_root.path().join("tool/extension.yaml"),
    )
    .expect("link manifest");
    assert!(matches!(
        prepare_tool_artifacts(manifest_root.path(), &manifest_selection),
        Err(ExtensionPreparationError::Manifest { .. })
    ));
}

#[cfg(unix)]
#[test]
fn preparation_rejects_oversized_targets_without_returning_a_prefix() {
    let root = tempdir().expect("temporary extension root");
    write_manifest(root.path().join("alpha/extension.yaml"), "alpha.tool");
    write_manifest(root.path().join("zeta/extension.yaml"), "zeta.tool");
    let registry = ExtensionRegistry::discover(root.path()).expect("extension registry");
    let selection = registry
        .select_kind(ExtensionKind::Tool, ["alpha.tool", "zeta.tool"])
        .expect("tool selection");
    fs::write(
        root.path().join("zeta/run"),
        vec![0_u8; MAX_ENTRYPOINT_BYTES as usize + 1],
    )
    .expect("write oversized target");

    let error =
        prepare_tool_artifacts(root.path(), &selection).expect_err("oversized target must fail");

    assert!(matches!(
        error,
        ExtensionPreparationError::EntrypointTooLarge {
            ref id,
            max_bytes: MAX_ENTRYPOINT_BYTES,
            ..
        } if id == "zeta.tool"
    ));
    assert!(error.source().is_none());
}

#[cfg(not(unix))]
#[test]
fn discovery_fails_closed_when_descriptor_anchored_traversal_is_unavailable() {
    let root = tempdir().expect("temporary extension root");

    let error = discover_extensions(root.path()).expect_err("unsupported secure discovery");

    assert!(matches!(
        &error,
        ExtensionDiscoveryError::ReadRoot { path, .. } if path == root.path()
    ));
    let source = error
        .source()
        .and_then(|source| source.downcast_ref::<std::io::Error>());
    assert_eq!(
        source.map(std::io::Error::kind),
        Some(std::io::ErrorKind::Unsupported)
    );
}

fn write_manifest(path: std::path::PathBuf, id: &str) {
    write_manifest_with_entrypoint(&path, id, "run");
    fs::write(
        path.parent().expect("manifest parent").join("run"),
        "not executed",
    )
    .expect("write entrypoint");
}

fn write_manifest_with_kind(path: std::path::PathBuf, id: &str, kind: &str) {
    fs::create_dir_all(path.parent().expect("manifest parent")).expect("create manifest directory");
    fs::write(
        &path,
        format!("manifest_version: 1\nid: {id}\nkind: {kind}\nentrypoint: run\n"),
    )
    .expect("write manifest");
    fs::write(
        path.parent().expect("manifest parent").join("run"),
        "not executed",
    )
    .expect("write entrypoint");
}

fn write_manifest_with_entrypoint(path: &std::path::Path, id: &str, entrypoint: &str) {
    fs::create_dir_all(path.parent().expect("manifest parent"))
        .expect("create extension directory");
    fs::write(
        path,
        format!("manifest_version: 1\nid: {id}\nkind: tool\nentrypoint: {entrypoint}\n"),
    )
    .expect("write manifest");
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
