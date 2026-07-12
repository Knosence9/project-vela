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
    let unsupported = VALID.replace("\"schema_version\": 1", "\"schema_version\": 2");
    assert!(serde_json::from_str::<DevelopmentRecord>(&unsupported).is_err());

    let invalid_enum = VALID.replace("\"trust\":\"curated\"", "\"trust\":\"perfect\"");
    assert!(serde_json::from_str::<DevelopmentRecord>(&invalid_enum).is_err());
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
