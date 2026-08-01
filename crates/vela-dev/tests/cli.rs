use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;
use vela_kernel::{
    scheduler::{
        ScheduleCancellation, ScheduleId, ScheduleInstant, ScheduleRelease, ScheduleStore,
    },
    task::{TaskGoal, TaskId},
};

const ECHO_COMPONENT: &str = r#"
(component
  (core module $guest
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 64))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $next
      global.get $next
      local.get 3
      i32.add
      global.set $next)
    (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
      i32.const 4
      local.get $ptr
      i32.store
      i32.const 8
      local.get $len
      i32.store
      i32.const 0))
  (core instance $guest (instantiate $guest))
  (type $outcome (result string (error string)))
  (type $invoke (func (param "input" string) (result $outcome)))
  (func $invoke (type $invoke)
    (canon lift (core func $guest "invoke")
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
  (export "invoke" (func $invoke)))
"#;

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
fn inspects_durable_schedules_as_deterministic_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");

    let cancelled_id = ScheduleId::new("cancelled\nintent").unwrap();
    let cancelled = store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("cancel \"safely\"").unwrap(),
            ScheduleInstant::from_unix_millis(30),
        )
        .unwrap();
    store
        .cancel(
            &cancelled_id,
            cancelled.revision(),
            ScheduleCancellation::new("operator\trequest").unwrap(),
        )
        .unwrap();

    let claimed_id = ScheduleId::new("claimed").unwrap();
    let claimed = store
        .schedule(
            claimed_id.clone(),
            TaskGoal::new("reserved work").unwrap(),
            ScheduleInstant::from_unix_millis(15),
        )
        .unwrap();
    store
        .claim(
            &claimed_id,
            claimed.revision(),
            ScheduleInstant::from_unix_millis(15),
        )
        .unwrap();

    let materialized_id = ScheduleId::new("materialized").unwrap();
    let materialized = store
        .schedule(
            materialized_id.clone(),
            TaskGoal::new("create task").unwrap(),
            ScheduleInstant::from_unix_millis(10),
        )
        .unwrap();
    let claimed = store
        .claim(
            &materialized_id,
            materialized.revision(),
            ScheduleInstant::from_unix_millis(10),
        )
        .unwrap();
    store
        .materialize(
            &materialized_id,
            claimed.revision(),
            TaskId::new("task\n42").unwrap(),
        )
        .unwrap();

    let pending_id = ScheduleId::new("pending").unwrap();
    let pending = store
        .schedule(
            pending_id.clone(),
            TaskGoal::new("retry later").unwrap(),
            ScheduleInstant::from_unix_millis(20),
        )
        .unwrap();
    let claimed = store
        .claim(
            &pending_id,
            pending.revision(),
            ScheduleInstant::from_unix_millis(20),
        )
        .unwrap();
    store
        .release(
            &pending_id,
            claimed.revision(),
            ScheduleRelease::new("worker\rrecovery").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "inspect",
            database.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"schedules\":[",
            "{\"id\":\"cancelled\\nintent\",\"goal\":\"cancel \\\"safely\\\"\",\"due_at_unix_millis\":30,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator\\trequest\",\"latest_release\":null,\"task_id\":null},",
            "{\"id\":\"claimed\",\"goal\":\"reserved work\",\"due_at_unix_millis\":15,\"status\":\"claimed\",\"revision\":2,\"cancellation\":null,\"latest_release\":null,\"task_id\":null},",
            "{\"id\":\"materialized\",\"goal\":\"create task\",\"due_at_unix_millis\":10,\"status\":\"materialized\",\"revision\":3,\"cancellation\":null,\"latest_release\":null,\"task_id\":\"task\\n42\"},",
            "{\"id\":\"pending\",\"goal\":\"retry later\",\"due_at_unix_millis\":20,\"status\":\"pending\",\"revision\":3,\"cancellation\":null,\"latest_release\":\"worker\\rrecovery\",\"task_id\":null}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn schedule_inspection_reports_empty_and_missing_storage_without_creation() {
    let directory = tempdir().expect("schedule database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(ScheduleStore::open(&empty).expect("empty schedule store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "inspect",
            empty.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .success()
        .stdout("{\"schedules\":[]}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "inspect",
            missing.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_inspection_failed:",
        ));
    assert!(!missing.exists());
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
fn invokes_one_exact_tool_with_json_through_the_cli_permission_boundary() {
    let root = tempdir().expect("extension root");
    write_extension(root.path(), "echo", "echo.tool", "tool", ECHO_COMPONENT);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "extension",
            "invoke",
            root.path().to_str().expect("UTF-8 temporary root"),
            "echo.tool",
            r#"{"nested":[true,null,3]}"#,
        ])
        .assert()
        .success()
        .stdout("{\"nested\":[true,null,3]}\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn extension_invocation_rejects_input_before_filesystem_access_and_fails_closed_on_kind() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "extension",
            "invoke",
            "tests/fixtures/missing-extensions",
            "echo.tool",
            "not-json",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_tool_input:"));

    let root = tempdir().expect("extension root");
    write_extension(root.path(), "skill", "alpha.skill", "skill", "not wasm");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "extension",
            "invoke",
            root.path().to_str().expect("UTF-8 temporary root"),
            "alpha.skill",
            "null",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_tool_selection:"));
}

fn write_extension(root: &std::path::Path, package: &str, id: &str, kind: &str, body: &str) {
    let package = root.join(package);
    fs::create_dir(&package).expect("package directory");
    fs::write(
        package.join("extension.yaml"),
        format!("manifest_version: 1\nid: {id}\nkind: {kind}\nentrypoint: run.wasm\n"),
    )
    .expect("manifest");
    let bytes = if kind == "tool" {
        wat::parse_str(body).expect("valid test component")
    } else {
        body.as_bytes().to_vec()
    };
    fs::write(package.join("run.wasm"), bytes).expect("entrypoint");
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
