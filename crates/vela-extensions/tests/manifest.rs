use std::{error::Error, fs};

use tempfile::tempdir;
use vela_extensions::{ExtensionKind, ExtensionManifest, ExtensionManifestError};

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

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
