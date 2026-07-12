use vela_dev::record::{DevelopmentRecord, ValidationIssue};

const VALID: &str = include_str!("fixtures/valid-record.json");

#[test]
fn valid_record_round_trips() {
    let record: DevelopmentRecord =
        serde_json::from_str(VALID).expect("valid fixture deserializes");
    assert!(record.validate().is_empty());
    let encoded = serde_json::to_string(&record).expect("record serializes");
    let decoded: DevelopmentRecord = serde_json::from_str(&encoded).expect("record round-trips");
    assert_eq!(decoded, record);
}

#[test]
fn unsupported_version_and_invalid_enums_are_rejected() {
    let mut unsupported: serde_json::Value = serde_json::from_str(VALID).expect("JSON fixture");
    unsupported["schema_version"] = 2.into();
    assert!(serde_json::from_value::<DevelopmentRecord>(unsupported).is_err());

    let mut invalid_enum: serde_json::Value = serde_json::from_str(VALID).expect("JSON fixture");
    invalid_enum["trust"] = "perfect".into();
    assert!(serde_json::from_value::<DevelopmentRecord>(invalid_enum).is_err());
}

#[test]
fn rejects_repository_traversal_and_windows_home_paths() {
    let mut value: serde_json::Value = serde_json::from_str(VALID).expect("JSON fixture");
    value["provenance"]["repository_path"] = "src/../private.txt".into();
    value["lessons"][0] = r"Read C:\Users\alice\private.txt".into();
    let record: DevelopmentRecord = serde_json::from_value(value).expect("record shape");
    let issues = record.validate();

    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "repository_path_traversal")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "home_path_detected")
    );
}

#[test]
fn semantic_validation_collects_all_issues() {
    let record: DevelopmentRecord =
        serde_json::from_str(include_str!("fixtures/invalid-record.json"))
            .expect("semantic fixture deserializes");
    let issues = record.validate();

    for expected in [
        ValidationIssue::new("required", "task.title", "must not be empty"),
        ValidationIssue::new(
            "verified_without_pass",
            "outcome.verification",
            "verified outcomes require a passing verification",
        ),
        ValidationIssue::new(
            "curated_after_failed_verification",
            "trust",
            "curated trust requires final verification not to fail",
        ),
        ValidationIssue::new(
            "negative_without_diagnostic",
            "example.type",
            "negative examples require a diagnostic or rejection rationale",
        ),
        ValidationIssue::new(
            "absolute_repository_path",
            "provenance.repository_path",
            "repository paths must be relative",
        ),
        ValidationIssue::new(
            "insecure_provenance_url",
            "provenance.url",
            "provenance URLs must use HTTPS",
        ),
        ValidationIssue::new(
            "secret_detected",
            "attempts[0].patch",
            "obvious secret detected",
        ),
        ValidationIssue::new(
            "home_path_detected",
            "lessons[0]",
            "absolute home-directory path detected",
        ),
        ValidationIssue::new(
            "sanitation_blocked",
            "sanitation.passed",
            "sanitation cannot pass while blockers remain",
        ),
    ] {
        assert!(
            issues.contains(&expected),
            "missing {expected:?}; got {issues:#?}"
        );
    }
}
