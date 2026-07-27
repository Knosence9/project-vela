use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

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
fn inspects_validated_extensions_in_manifest_path_order() {
    let root = format!(
        "{}/tests/fixtures/extensions/mixed",
        env!("CARGO_MANIFEST_DIR")
    );

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["extension", "inspect", &root])
        .assert()
        .success()
        .stdout(predicate::eq(
            "\"alpha.skill\"\tskill\t\"SKILL.md\"\t\"alpha/extension.yaml\"\n\
             \"beta.workflow\"\tworkflow\t\"WORKFLOW.md\"\t\"beta/extension.yaml\"\n\
             \"zeta.tool\"\ttool\t\"bin/zeta.wasm\"\t\"zeta/extension.yaml\"\n\
             inspected 3 extensions\n",
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn extension_inspection_reports_empty_and_invalid_roots_without_partial_output() {
    let empty = tempdir().expect("empty extension root");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "extension",
            "inspect",
            empty.path().to_str().expect("UTF-8 temporary path"),
        ])
        .assert()
        .success()
        .stdout("inspected 0 extensions\n")
        .stderr(predicate::str::is_empty());

    let invalid = format!(
        "{}/tests/fixtures/extensions/invalid",
        env!("CARGO_MANIFEST_DIR")
    );
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["extension", "inspect", &invalid])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_extension_root:"));
}

#[test]
fn extension_inspection_escapes_untrusted_record_fields() {
    let root = tempdir().expect("extension root");
    let package = root.path().join("package\nforged");
    fs::create_dir(&package).expect("package directory");
    fs::write(
        package.join("extension.yaml"),
        "manifest_version: 1\nid: \"unsafe\\tid\\n\\u001b[31m\"\nkind: tool\nentrypoint: run.wasm\n",
    )
    .expect("manifest");
    fs::write(package.join("run.wasm"), "placeholder").expect("entrypoint");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "extension",
            "inspect",
            root.path().to_str().expect("UTF-8 temporary root"),
        ])
        .assert()
        .success()
        .stdout(predicate::eq(
            "\"unsafe\\tid\\n\\u{1b}[31m\"\ttool\t\"run.wasm\"\t\"package\\nforged/extension.yaml\"\n\
             inspected 1 extensions\n",
        ))
        .stderr(predicate::str::is_empty());

    fs::remove_file(package.join("run.wasm")).expect("invalidate entrypoint");
    let output = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "extension",
            "inspect",
            root.path().to_str().expect("UTF-8 temporary root"),
        ])
        .output()
        .expect("invalid inspection output");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 escaped diagnostic");
    assert_eq!(stderr.lines().count(), 1);
    assert!(stderr.starts_with("$: invalid_extension_root: \""));
    assert!(stderr.contains("package\\nforged/extension.yaml"));
    assert!(!stderr.contains('\u{1b}'));
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
