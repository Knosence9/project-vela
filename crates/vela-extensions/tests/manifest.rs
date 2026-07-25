use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{
    ExtensionDiscoveryError, ExtensionKind, ExtensionManifest, ExtensionManifestError,
    MAX_MANIFEST_BYTES, discover_extensions,
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
