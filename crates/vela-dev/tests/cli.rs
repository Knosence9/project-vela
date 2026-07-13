use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_identifies_vela_developer_tooling() {
    let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Developer tooling for Project Vela",
        ))
        .stdout(predicate::str::contains("Usage: vela-dev [COMMAND]"));
}

#[test]
fn record_help_describes_development_records() {
    let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");

    command
        .args(["record", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Work with Vela development records",
        ))
        .stdout(predicate::str::contains("Usage: vela-dev record"));
}

#[test]
fn inspects_corpus_in_deterministic_order_and_reports_failures() {
    let corpus = format!("{}/tests/fixtures/corpus", env!("CARGO_MANIFEST_DIR"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["corpus", "inspect", &format!("{corpus}/valid")])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "nested/first.json: valid\nsecond.json: valid",
        ))
        .stdout(predicate::str::contains(
            "inspected 2 records: 2 valid, 0 invalid",
        ));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["corpus", "inspect", &format!("{corpus}/invalid")])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "inspected 2 records: 0 valid, 2 invalid",
        ))
        .stderr(predicate::str::contains("malformed.json: malformed_record"))
        .stderr(predicate::str::contains(
            "semantic.json: task.title: required",
        ));
}

#[test]
fn corpus_inspection_rejects_an_unreadable_root() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["corpus", "inspect", "tests/fixtures/missing-corpus"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("$: unreadable_corpus"));
}

#[test]
fn validates_development_record_files_with_stable_diagnostics() {
    let fixtures = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "record",
            "validate",
            &format!("{fixtures}/valid-record.json"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid development record"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "record",
            "validate",
            &format!("{fixtures}/invalid-record.json"),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("task.title: required"))
        .stderr(predicate::str::contains(
            "outcome.verification: verified_without_pass",
        ));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "record",
            "validate",
            &format!("{fixtures}/malformed-record.json"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("$: malformed_record"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "record",
            "validate",
            &format!("{fixtures}/missing-record.json"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("$: unreadable_record"));
}
