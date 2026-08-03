use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;
use vela_kernel::{
    scheduler::{
        OccurrenceCount, RecurrenceId, RecurrenceStore, ScheduleCancellation, ScheduleId,
        ScheduleInstant, ScheduleInterval, ScheduleRelease, ScheduleStore,
    },
    task::{TaskGoal, TaskId, TaskStore},
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
fn creates_one_durable_schedule_as_deterministic_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "create",
            database.to_str().expect("UTF-8 database path"),
            "intent\n42",
            "preserve \"exact\" goal",
            "123",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"intent\\n42\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"due_at_unix_millis\":123,\"status\":\"pending\",\"revision\":1,",
            "\"cancellation\":null,\"latest_release\":null,\"task_id\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).expect("read-only schedule store");
    let scheduled = store
        .load(&ScheduleId::new("intent\n42").unwrap())
        .unwrap()
        .expect("persisted schedule");
    assert_eq!(scheduled.goal().as_str(), "preserve \"exact\" goal");
    assert_eq!(scheduled.due_at().unix_millis(), 123);
    assert_eq!(scheduled.revision(), 1);
}

#[test]
fn schedule_creation_rejects_duplicates_without_rewriting_the_original() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "create",
            database.to_str().expect("UTF-8 database path"),
            "same-id",
            "original",
            "10",
        ])
        .assert()
        .success();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "create",
            database.to_str().expect("UTF-8 database path"),
            "same-id",
            "replacement",
            "20",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_creation_failed:"));

    let store = ScheduleStore::open_read_only(&database).expect("read-only schedule store");
    let scheduled = store
        .load(&ScheduleId::new("same-id").unwrap())
        .unwrap()
        .expect("original schedule");
    assert_eq!(scheduled.goal().as_str(), "original");
    assert_eq!(scheduled.due_at().unix_millis(), 10);
    assert_eq!(scheduled.revision(), 1);
}

#[test]
fn schedule_creation_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("schedule database directory");

    for (name, id, goal, expected_error) in [
        ("invalid-id.sqlite3", " ", "goal", "invalid_schedule_id"),
        ("invalid-goal.sqlite3", "id", "", "invalid_task_goal"),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "create",
                database.to_str().expect("UTF-8 database path"),
                id,
                goal,
                "1",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }

    let invalid_due = directory.path().join("invalid-due.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "create",
            invalid_due.to_str().expect("UTF-8 database path"),
            "id",
            "goal",
            "not-a-millisecond",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "invalid value 'not-a-millisecond'",
        ));
    assert!(!invalid_due.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "create",
            directory.path().to_str().expect("UTF-8 database path"),
            "id",
            "goal",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_creation_failed:"));
}

#[test]
fn cancels_one_pending_schedule_at_the_exact_revision() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("intent\n42").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("preserve exact goal").unwrap(),
            ScheduleInstant::from_unix_millis(123),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "cancel",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            "operator\t\"request\"",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"intent\\n42\",\"goal\":\"preserve exact goal\",",
            "\"due_at_unix_millis\":123,\"status\":\"cancelled\",\"revision\":2,",
            "\"cancellation\":\"operator\\t\\\"request\\\"\",",
            "\"latest_release\":null,\"task_id\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).unwrap();
    let cancelled = store.load(&id).unwrap().expect("cancelled schedule");
    assert_eq!(cancelled.revision(), 2);
    assert_eq!(
        cancelled.cancellation().unwrap().as_str(),
        "operator\t\"request\""
    );
}

#[test]
fn schedule_cancellation_rejects_stale_missing_and_claimed_intent_without_append() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let pending_id = ScheduleId::new("pending").unwrap();
    let claimed_id = ScheduleId::new("claimed").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            pending_id.clone(),
            TaskGoal::new("pending goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let claimed = store
        .schedule(
            claimed_id.clone(),
            TaskGoal::new("claimed goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .claim(
            &claimed_id,
            claimed.revision(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    drop(store);

    for (id, revision) in [
        (pending_id.as_str(), "0"),
        ("missing", "1"),
        (claimed_id.as_str(), "2"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "cancel",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                "operator request",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: schedule_cancellation_failed:",
            ));
    }

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(store.load(&pending_id).unwrap().unwrap().revision(), 1);
    assert_eq!(store.load(&claimed_id).unwrap().unwrap().revision(), 2);
}

#[test]
fn schedule_cancellation_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("schedule database directory");

    let invalid_revision = directory.path().join("invalid-revision.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "cancel",
            invalid_revision.to_str().expect("UTF-8 database path"),
            "id",
            "not-a-revision",
            "reason",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value 'not-a-revision'"));
    assert!(!invalid_revision.exists());

    for (name, id, reason, expected_error) in [
        ("invalid-id.sqlite3", " ", "reason", "invalid_schedule_id"),
        (
            "invalid-reason.sqlite3",
            "id",
            "\t",
            "invalid_schedule_cancellation",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "cancel",
                database.to_str().expect("UTF-8 database path"),
                id,
                "1",
                reason,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "cancel",
            directory.path().to_str().expect("UTF-8 database path"),
            "id",
            "1",
            "reason",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_cancellation_failed:",
        ));
}

#[test]
fn claims_one_exact_due_schedule_revision_as_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("intent\n42").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(123),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "claim",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            "123",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"intent\\n42\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"due_at_unix_millis\":123,\"status\":\"claimed\",\"revision\":2,",
            "\"cancellation\":null,\"latest_release\":null,\"task_id\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).unwrap();
    let claimed = store.load(&id).unwrap().expect("claimed schedule");
    assert_eq!(claimed.revision(), 2);
}

#[test]
fn schedule_claim_rejects_future_stale_missing_and_terminal_intent_without_append() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let future_id = ScheduleId::new("future").unwrap();
    let claimed_id = ScheduleId::new("claimed").unwrap();
    let cancelled_id = ScheduleId::new("cancelled").unwrap();
    let materialized_id = ScheduleId::new("materialized").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            future_id.clone(),
            TaskGoal::new("future goal").unwrap(),
            ScheduleInstant::from_unix_millis(2),
        )
        .unwrap();
    let claimed = store
        .schedule(
            claimed_id.clone(),
            TaskGoal::new("claimed goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .claim(
            &claimed_id,
            claimed.revision(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let cancelled = store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("cancelled goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .cancel(
            &cancelled_id,
            cancelled.revision(),
            ScheduleCancellation::new("operator request").unwrap(),
        )
        .unwrap();
    let materialized = store
        .schedule(
            materialized_id.clone(),
            TaskGoal::new("materialized goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let materialized = store
        .claim(
            &materialized_id,
            materialized.revision(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .materialize(
            &materialized_id,
            materialized.revision(),
            TaskId::new("materialized-task").unwrap(),
        )
        .unwrap();
    drop(store);

    for (id, revision, cutoff) in [
        (future_id.as_str(), "1", "1"),
        (future_id.as_str(), "0", "2"),
        ("missing", "1", "1"),
        (claimed_id.as_str(), "2", "1"),
        (cancelled_id.as_str(), "2", "1"),
        (materialized_id.as_str(), "3", "1"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "claim",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                cutoff,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with("$: schedule_claim_failed:"));
    }

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(store.load(&future_id).unwrap().unwrap().revision(), 1);
    assert_eq!(store.load(&claimed_id).unwrap().unwrap().revision(), 2);
    assert_eq!(store.load(&cancelled_id).unwrap().unwrap().revision(), 2);
    assert_eq!(store.load(&materialized_id).unwrap().unwrap().revision(), 3);
}

#[test]
fn schedule_claim_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("schedule database directory");

    for (name, id, revision, cutoff, code, expected_error) in [
        (
            "invalid-id.sqlite3",
            " ",
            "1",
            "1",
            1,
            "invalid_schedule_id",
        ),
        (
            "invalid-revision.sqlite3",
            "id",
            "not-a-revision",
            "1",
            2,
            "invalid value 'not-a-revision'",
        ),
        (
            "invalid-cutoff.sqlite3",
            "id",
            "1",
            "not-a-cutoff",
            2,
            "invalid value 'not-a-cutoff'",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "claim",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                cutoff,
            ])
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(expected_error));
        assert!(!database.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "claim",
            directory.path().to_str().expect("UTF-8 database path"),
            "id",
            "1",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_claim_failed:"));
}

#[test]
fn claims_next_due_schedule_in_deterministic_order_as_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).unwrap();
    for (id, goal, due_at) in [
        ("zeta", "second", 5),
        ("alpha", "preserve \"exact\" goal", 5),
        ("future", "later", 6),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(due_at),
            )
            .unwrap();
    }
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "claim-next",
            database.to_str().expect("UTF-8 database path"),
            "5",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"schedule\":{\"id\":\"alpha\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"due_at_unix_millis\":5,\"status\":\"claimed\",\"revision\":2,",
            "\"cancellation\":null,\"latest_release\":null,\"task_id\":null}}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(
        store
            .load(&ScheduleId::new("alpha").unwrap())
            .unwrap()
            .unwrap()
            .revision(),
        2
    );
    assert_eq!(
        store
            .load(&ScheduleId::new("zeta").unwrap())
            .unwrap()
            .unwrap()
            .revision(),
        1
    );
}

#[test]
fn schedule_claim_next_returns_null_without_eligible_work() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            ScheduleId::new("future").unwrap(),
            TaskGoal::new("later").unwrap(),
            ScheduleInstant::from_unix_millis(6),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "claim-next",
            database.to_str().expect("UTF-8 database path"),
            "5",
        ])
        .assert()
        .success()
        .stdout("{\"schedule\":null}\n")
        .stderr(predicate::str::is_empty());

    assert_eq!(
        ScheduleStore::open_read_only(&database)
            .unwrap()
            .load(&ScheduleId::new("future").unwrap())
            .unwrap()
            .unwrap()
            .revision(),
        1
    );
}

#[test]
fn schedule_claim_next_fails_closed_on_corrupt_history() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            ScheduleId::new("corrupt").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'schedule:corrupt'",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "claim-next",
            database.to_str().expect("UTF-8 database path"),
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_claim_failed:"));
}

#[test]
fn materializes_next_due_schedule_in_deterministic_order_as_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).unwrap();
    for (id, goal, due_at) in [
        ("zeta", "second", 5),
        ("alpha", "preserve \"exact\" goal", 5),
        ("future", "later", 6),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(due_at),
            )
            .unwrap();
    }
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "materialize-next",
            database.to_str().expect("UTF-8 database path"),
            "5",
            "task\n42",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"schedule\":{\"id\":\"alpha\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"due_at_unix_millis\":5,\"status\":\"materialized\",\"revision\":2,",
            "\"cancellation\":null,\"latest_release\":null,\"task_id\":\"task\\n42\"}}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(
        store
            .find_by_task_id(&TaskId::new("task\n42").unwrap())
            .unwrap()
            .unwrap()
            .id()
            .as_str(),
        "alpha"
    );
    assert_eq!(
        store
            .load(&ScheduleId::new("zeta").unwrap())
            .unwrap()
            .unwrap()
            .revision(),
        1
    );
}

#[test]
fn schedule_materialize_next_returns_null_without_eligible_work() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            ScheduleId::new("future").unwrap(),
            TaskGoal::new("later").unwrap(),
            ScheduleInstant::from_unix_millis(6),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "materialize-next",
            database.to_str().expect("UTF-8 database path"),
            "5",
            "unused-task",
        ])
        .assert()
        .success()
        .stdout("{\"schedule\":null}\n")
        .stderr(predicate::str::is_empty());

    assert_eq!(
        ScheduleStore::open_read_only(&database)
            .unwrap()
            .load(&ScheduleId::new("future").unwrap())
            .unwrap()
            .unwrap()
            .revision(),
        1
    );
}

#[test]
fn schedule_materialize_next_rejects_task_collision_without_consuming_schedule() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let occupied_id = ScheduleId::new("occupied").unwrap();
    let candidate_id = ScheduleId::new("candidate").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    let occupied = store
        .schedule(
            occupied_id.clone(),
            TaskGoal::new("occupied goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let occupied = store
        .claim(
            &occupied_id,
            occupied.revision(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .materialize(
            &occupied_id,
            occupied.revision(),
            TaskId::new("same-task").unwrap(),
        )
        .unwrap();
    store
        .schedule(
            candidate_id.clone(),
            TaskGoal::new("candidate goal").unwrap(),
            ScheduleInstant::from_unix_millis(2),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "materialize-next",
            database.to_str().expect("UTF-8 database path"),
            "2",
            "same-task",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_materialization_failed:",
        ));

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(store.load(&candidate_id).unwrap().unwrap().revision(), 1);
    assert_eq!(
        store
            .find_by_task_id(&TaskId::new("same-task").unwrap())
            .unwrap()
            .unwrap()
            .id(),
        &occupied_id
    );
}

#[test]
fn schedule_materialize_next_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("schedule database directory");

    for (name, cutoff, task_id, code, expected_error) in [
        (
            "invalid-cutoff.sqlite3",
            "not-a-cutoff",
            "task",
            2,
            "invalid value 'not-a-cutoff'",
        ),
        ("invalid-task.sqlite3", "1", "", 1, "invalid_task_id"),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "materialize-next",
                database.to_str().expect("UTF-8 database path"),
                cutoff,
                task_id,
            ])
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(expected_error));
        assert!(!database.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "materialize-next",
            directory.path().to_str().expect("UTF-8 database path"),
            "1",
            "task",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_materialization_failed:",
        ));
}

#[test]
fn releases_one_exact_claimed_schedule_revision_as_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("intent\n42").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    let scheduled = store
        .schedule(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(123),
        )
        .unwrap();
    store
        .claim(
            &id,
            scheduled.revision(),
            ScheduleInstant::from_unix_millis(123),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "release",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "2",
            "worker\t\"recovery\"",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"intent\\n42\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"due_at_unix_millis\":123,\"status\":\"pending\",\"revision\":3,",
            "\"cancellation\":null,\"latest_release\":\"worker\\t\\\"recovery\\\"\",",
            "\"task_id\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).unwrap();
    let released = store.load(&id).unwrap().expect("released schedule");
    assert_eq!(released.revision(), 3);
    assert_eq!(
        released.latest_release().unwrap().as_str(),
        "worker\t\"recovery\""
    );
}

#[test]
fn schedule_release_rejects_stale_missing_and_wrong_state_without_append() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let pending_id = ScheduleId::new("pending").unwrap();
    let claimed_id = ScheduleId::new("claimed").unwrap();
    let cancelled_id = ScheduleId::new("cancelled").unwrap();
    let materialized_id = ScheduleId::new("materialized").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            pending_id.clone(),
            TaskGoal::new("pending goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let claimed = store
        .schedule(
            claimed_id.clone(),
            TaskGoal::new("claimed goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .claim(
            &claimed_id,
            claimed.revision(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let cancelled = store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("cancelled goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .cancel(
            &cancelled_id,
            cancelled.revision(),
            ScheduleCancellation::new("operator request").unwrap(),
        )
        .unwrap();
    let materialized = store
        .schedule(
            materialized_id.clone(),
            TaskGoal::new("materialized goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    let materialized = store
        .claim(
            &materialized_id,
            materialized.revision(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .materialize(
            &materialized_id,
            materialized.revision(),
            TaskId::new("materialized-task").unwrap(),
        )
        .unwrap();
    drop(store);

    for (id, revision) in [
        (claimed_id.as_str(), "1"),
        ("missing", "1"),
        (pending_id.as_str(), "1"),
        (cancelled_id.as_str(), "2"),
        (materialized_id.as_str(), "3"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "release",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                "worker recovery",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with("$: schedule_release_failed:"));
    }

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(store.load(&pending_id).unwrap().unwrap().revision(), 1);
    assert_eq!(store.load(&claimed_id).unwrap().unwrap().revision(), 2);
    assert_eq!(store.load(&cancelled_id).unwrap().unwrap().revision(), 2);
    assert_eq!(store.load(&materialized_id).unwrap().unwrap().revision(), 3);
}

#[test]
fn schedule_release_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("schedule database directory");

    for (name, id, revision, reason, code, expected_error) in [
        (
            "invalid-id.sqlite3",
            " ",
            "1",
            "reason",
            1,
            "invalid_schedule_id",
        ),
        (
            "invalid-revision.sqlite3",
            "id",
            "not-a-revision",
            "reason",
            2,
            "invalid value 'not-a-revision'",
        ),
        (
            "invalid-reason.sqlite3",
            "id",
            "1",
            "\t",
            1,
            "invalid_schedule_release_reason",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "release",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                reason,
            ])
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(expected_error));
        assert!(!database.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "release",
            directory.path().to_str().expect("UTF-8 database path"),
            "id",
            "1",
            "reason",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_release_failed:"));
}

#[test]
fn materializes_one_exact_claimed_schedule_revision_as_complete_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("intent\n42").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    let scheduled = store
        .schedule(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(123),
        )
        .unwrap();
    store
        .claim(
            &id,
            scheduled.revision(),
            ScheduleInstant::from_unix_millis(123),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "materialize",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "2",
            "task\n42",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"intent\\n42\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"due_at_unix_millis\":123,\"status\":\"materialized\",\"revision\":3,",
            "\"cancellation\":null,\"latest_release\":null,\"task_id\":\"task\\n42\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = ScheduleStore::open_read_only(&database).unwrap();
    let materialized = store.load(&id).unwrap().expect("materialized schedule");
    assert_eq!(materialized.revision(), 3);
    assert_eq!(materialized.task_id().unwrap().as_str(), "task\n42");
}

#[test]
fn schedule_materialize_rejects_stale_missing_wrong_state_and_task_collision_atomically() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let pending_id = ScheduleId::new("pending").unwrap();
    let claimed_id = ScheduleId::new("claimed").unwrap();
    let cancelled_id = ScheduleId::new("cancelled").unwrap();
    let materialized_id = ScheduleId::new("materialized").unwrap();
    let collision_id = ScheduleId::new("collision").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    store
        .schedule(
            pending_id.clone(),
            TaskGoal::new("pending goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    for (id, goal) in [
        (&claimed_id, "claimed goal"),
        (&materialized_id, "materialized goal"),
        (&collision_id, "collision goal"),
    ] {
        let scheduled = store
            .schedule(
                id.clone(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
        store
            .claim(
                id,
                scheduled.revision(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
    }
    let cancelled = store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("cancelled goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
        )
        .unwrap();
    store
        .cancel(
            &cancelled_id,
            cancelled.revision(),
            ScheduleCancellation::new("operator request").unwrap(),
        )
        .unwrap();
    store
        .materialize(&materialized_id, 2, TaskId::new("existing-task").unwrap())
        .unwrap();
    drop(store);

    for (id, revision, task_id) in [
        (claimed_id.as_str(), "1", "stale-task"),
        ("missing", "1", "missing-task"),
        (pending_id.as_str(), "1", "pending-task"),
        (cancelled_id.as_str(), "2", "cancelled-task"),
        (materialized_id.as_str(), "3", "second-task"),
        (collision_id.as_str(), "2", "existing-task"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "materialize",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: schedule_materialization_failed:",
            ));
    }

    let store = ScheduleStore::open_read_only(&database).unwrap();
    assert_eq!(store.load(&pending_id).unwrap().unwrap().revision(), 1);
    assert_eq!(store.load(&claimed_id).unwrap().unwrap().revision(), 2);
    assert_eq!(store.load(&cancelled_id).unwrap().unwrap().revision(), 2);
    assert_eq!(store.load(&materialized_id).unwrap().unwrap().revision(), 3);
    assert_eq!(store.load(&collision_id).unwrap().unwrap().revision(), 2);
}

#[test]
fn schedule_materialize_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("schedule database directory");

    for (name, id, revision, task_id, code, expected_error) in [
        (
            "invalid-id.sqlite3",
            " ",
            "1",
            "task",
            1,
            "invalid_schedule_id",
        ),
        (
            "invalid-revision.sqlite3",
            "id",
            "not-a-revision",
            "task",
            2,
            "invalid value 'not-a-revision'",
        ),
        (
            "invalid-task-id.sqlite3",
            "id",
            "1",
            "",
            1,
            "invalid_task_id",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "materialize",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                task_id,
            ])
            .assert()
            .code(code)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(expected_error));
        assert!(!database.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "materialize",
            directory.path().to_str().expect("UTF-8 database path"),
            "id",
            "1",
            "task",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_materialization_failed:",
        ));
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
fn inspects_due_schedules_at_an_inclusive_caller_owned_cutoff() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");

    for (id, goal, due_at) in [
        ("same-z", "second at cutoff", 20),
        ("earlier", "first by due instant", 10),
        ("same-a", "first at cutoff", 20),
        ("future", "not due", 21),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(due_at),
            )
            .unwrap();
    }

    let claimed_id = ScheduleId::new("claimed").unwrap();
    let claimed = store
        .schedule(
            claimed_id.clone(),
            TaskGoal::new("not pending").unwrap(),
            ScheduleInstant::from_unix_millis(5),
        )
        .unwrap();
    store
        .claim(
            &claimed_id,
            claimed.revision(),
            ScheduleInstant::from_unix_millis(5),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due",
            database.to_str().expect("UTF-8 database path"),
            "20",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"schedules\":[",
            "{\"id\":\"earlier\",\"goal\":\"first by due instant\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null},",
            "{\"id\":\"same-a\",\"goal\":\"first at cutoff\",\"due_at_unix_millis\":20,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null},",
            "{\"id\":\"same-z\",\"goal\":\"second at cutoff\",\"due_at_unix_millis\":20,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn due_schedule_inspection_fails_closed_without_creating_storage() {
    let directory = tempdir().expect("schedule database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(ScheduleStore::open(&empty).expect("empty schedule store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due",
            empty.to_str().expect("UTF-8 database path"),
            "0",
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
            "due",
            missing.to_str().expect("UTF-8 database path"),
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_inspection_failed:",
        ));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due",
            missing.to_str().expect("UTF-8 database path"),
            "not-a-millisecond",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "invalid value 'not-a-millisecond'",
        ));
    assert!(!missing.exists());
}

#[test]
fn inspects_schedules_by_each_exact_lifecycle_status() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");

    for id in ["pending-z", "pending-a"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                ScheduleInstant::from_unix_millis(10),
            )
            .unwrap();
    }
    let cancelled_id = ScheduleId::new("cancelled").unwrap();
    let cancelled = store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("cancelled goal").unwrap(),
            ScheduleInstant::from_unix_millis(20),
        )
        .unwrap();
    store
        .cancel(
            &cancelled_id,
            cancelled.revision(),
            ScheduleCancellation::new("not needed").unwrap(),
        )
        .unwrap();
    let claimed_id = ScheduleId::new("claimed").unwrap();
    let claimed = store
        .schedule(
            claimed_id.clone(),
            TaskGoal::new("claimed goal").unwrap(),
            ScheduleInstant::from_unix_millis(30),
        )
        .unwrap();
    store
        .claim(
            &claimed_id,
            claimed.revision(),
            ScheduleInstant::from_unix_millis(30),
        )
        .unwrap();
    let materialized_id = ScheduleId::new("materialized").unwrap();
    let materialized = store
        .schedule(
            materialized_id.clone(),
            TaskGoal::new("materialized goal").unwrap(),
            ScheduleInstant::from_unix_millis(40),
        )
        .unwrap();
    let materialized = store
        .claim(
            &materialized_id,
            materialized.revision(),
            ScheduleInstant::from_unix_millis(40),
        )
        .unwrap();
    store
        .materialize(
            &materialized_id,
            materialized.revision(),
            TaskId::new("task-1").unwrap(),
        )
        .unwrap();
    drop(store);

    let cases = [
        (
            "pending",
            concat!(
                "{\"schedules\":[",
                "{\"id\":\"pending-a\",\"goal\":\"goal-pending-a\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null},",
                "{\"id\":\"pending-z\",\"goal\":\"goal-pending-z\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}]}\n"
            ),
        ),
        (
            "cancelled",
            "{\"schedules\":[{\"id\":\"cancelled\",\"goal\":\"cancelled goal\",\"due_at_unix_millis\":20,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"not needed\",\"latest_release\":null,\"task_id\":null}]}\n",
        ),
        (
            "claimed",
            "{\"schedules\":[{\"id\":\"claimed\",\"goal\":\"claimed goal\",\"due_at_unix_millis\":30,\"status\":\"claimed\",\"revision\":2,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}]}\n",
        ),
        (
            "materialized",
            "{\"schedules\":[{\"id\":\"materialized\",\"goal\":\"materialized goal\",\"due_at_unix_millis\":40,\"status\":\"materialized\",\"revision\":3,\"cancellation\":null,\"latest_release\":null,\"task_id\":\"task-1\"}]}\n",
        ),
    ];
    for (status, expected) in cases {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(["schedule", "status", database.to_str().unwrap(), status])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn schedule_status_inspection_rejects_invalid_input_before_storage() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    drop(ScheduleStore::open(&database).expect("empty schedule store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "status", database.to_str().unwrap(), "pending"])
        .assert()
        .success()
        .stdout("{\"schedules\":[]}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "status", missing.to_str().unwrap(), "PENDING"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_schedule_status:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "status", missing.to_str().unwrap(), "pending"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_status_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn schedule_status_inspection_fails_closed_on_invalid_history() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    drop(ScheduleStore::open(&database).expect("writable schedule store"));
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:broken', 1, 'schedule.created', 1, '{}')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "status", database.to_str().unwrap(), "pending"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_status_inspection_failed:",
        ));
}

#[test]
fn gets_one_exact_schedule_projection_as_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    let id = ScheduleId::new("exact\nschedule").unwrap();
    let scheduled = store
        .schedule(
            id.clone(),
            TaskGoal::new("run \"carefully\"").unwrap(),
            ScheduleInstant::from_unix_millis(44),
        )
        .unwrap();
    let claimed = store
        .claim(
            &id,
            scheduled.revision(),
            ScheduleInstant::from_unix_millis(44),
        )
        .unwrap();
    store
        .release(
            &id,
            claimed.revision(),
            ScheduleRelease::new("worker\trecovery").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "get",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"exact\\nschedule\",\"schedule\":{",
            "\"id\":\"exact\\nschedule\",\"goal\":\"run \\\"carefully\\\"\",",
            "\"due_at_unix_millis\":44,\"status\":\"pending\",\"revision\":3,",
            "\"cancellation\":null,\"latest_release\":\"worker\\trecovery\",\"task_id\":null}}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn schedule_get_preserves_absence_and_rejects_invalid_input_before_storage() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    drop(ScheduleStore::open(&database).expect("empty schedule store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "get", database.to_str().unwrap(), "missing"])
        .assert()
        .success()
        .stdout("{\"id\":\"missing\",\"schedule\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "get", missing.to_str().unwrap(), "   "])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_schedule_id:"));
    assert!(!missing.exists());
}

#[test]
fn schedule_get_fails_closed_on_malformed_durable_state() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    drop(ScheduleStore::open(&database).expect("writable schedule store"));
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:broken', 1, 'schedule.created', 1, '{}')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "get", database.to_str().unwrap(), "broken"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_lookup_failed:"));
}

#[test]
fn inspects_exact_schedule_history_as_revision_ordered_json() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    let id = ScheduleId::new("history\nintent").unwrap();
    let scheduled = store
        .schedule(
            id.clone(),
            TaskGoal::new("run \"carefully\"").unwrap(),
            ScheduleInstant::from_unix_millis(10),
        )
        .unwrap();
    let claimed = store
        .claim(
            &id,
            scheduled.revision(),
            ScheduleInstant::from_unix_millis(10),
        )
        .unwrap();
    let released = store
        .release(
            &id,
            claimed.revision(),
            ScheduleRelease::new("worker\trecovery").unwrap(),
        )
        .unwrap();
    let reclaimed = store
        .claim(
            &id,
            released.revision(),
            ScheduleInstant::from_unix_millis(10),
        )
        .unwrap();
    store
        .materialize(
            &id,
            reclaimed.revision(),
            TaskId::new("task\nbound").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"history\\nintent\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"run \\\"carefully\\\"\",\"due_at_unix_millis\":10},",
            "{\"revision\":2,\"type\":\"claimed\"},",
            "{\"revision\":3,\"type\":\"released\",\"reason\":\"worker\\trecovery\"},",
            "{\"revision\":4,\"type\":\"claimed\"},",
            "{\"revision\":5,\"type\":\"materialized\",\"task_id\":\"task\\nbound\"}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn schedule_history_preserves_cancellation_and_missing_stream_semantics() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    let id = ScheduleId::new("cancelled").unwrap();
    let scheduled = store
        .schedule(
            id.clone(),
            TaskGoal::new("stop later").unwrap(),
            ScheduleInstant::from_unix_millis(30),
        )
        .unwrap();
    store
        .cancel(
            &id,
            scheduled.revision(),
            ScheduleCancellation::new("operator request").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"cancelled\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"stop later\",\"due_at_unix_millis\":30},",
            "{\"revision\":2,\"type\":\"cancelled\",\"reason\":\"operator request\"}",
            "]}\n"
        ));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "history",
            database.to_str().expect("UTF-8 database path"),
            "missing",
        ])
        .assert()
        .success()
        .stdout("{\"id\":\"missing\",\"history\":null}\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn schedule_history_rejects_invalid_input_before_storage_and_never_creates_storage() {
    let directory = tempdir().expect("schedule database directory");
    let missing = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "history",
            missing.to_str().expect("UTF-8 database path"),
            "   ",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_schedule_id:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "history",
            missing.to_str().expect("UTF-8 database path"),
            "valid",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: schedule_history_failed:"));
    assert!(!missing.exists());
}

#[test]
fn resolves_materialized_schedule_by_exact_task_identity() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    let id = ScheduleId::new("bound\nschedule").unwrap();
    let task_id = TaskId::new("task\tidentity").unwrap();
    let scheduled = store
        .schedule(
            id.clone(),
            TaskGoal::new("run \"exactly\"").unwrap(),
            ScheduleInstant::from_unix_millis(44),
        )
        .unwrap();
    let claimed = store
        .claim(
            &id,
            scheduled.revision(),
            ScheduleInstant::from_unix_millis(44),
        )
        .unwrap();
    store
        .materialize(&id, claimed.revision(), task_id.clone())
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "task",
            database.to_str().expect("UTF-8 database path"),
            task_id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"task_id\":\"task\\tidentity\",\"schedule\":{",
            "\"id\":\"bound\\nschedule\",\"goal\":\"run \\\"exactly\\\"\",",
            "\"due_at_unix_millis\":44,\"status\":\"materialized\",\"revision\":3,",
            "\"cancellation\":null,\"latest_release\":null,\"task_id\":\"task\\tidentity\"}}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn schedule_task_lookup_preserves_absence_and_fails_closed() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    drop(ScheduleStore::open(&database).expect("empty schedule store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "task", database.to_str().unwrap(), "unbound"])
        .assert()
        .success()
        .stdout("{\"task_id\":\"unbound\",\"schedule\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "task", missing.to_str().unwrap(), ""])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_task_id:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "task", missing.to_str().unwrap(), "valid"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_task_lookup_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn schedule_task_lookup_reports_ambiguous_corrupted_bindings() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("duplicate-task").unwrap();
    let mut store = ScheduleStore::open(&database).unwrap();
    for raw_id in ["first", "second"] {
        let id = ScheduleId::new(raw_id).unwrap();
        let scheduled = store
            .schedule(
                id.clone(),
                TaskGoal::new(format!("goal-{raw_id}")).unwrap(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
        store
            .claim(
                &id,
                scheduled.revision(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
    }
    store
        .materialize(&ScheduleId::new("first").unwrap(), 2, task_id.clone())
        .unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:second', 3, 'schedule.materialized', 1, ?1)",
            [br#"{"task_id":"duplicate-task"}"#.as_slice()],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "task",
            database.to_str().unwrap(),
            task_id.as_str(),
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            "$: schedule_task_lookup_failed: \"task duplicate-task is bound to 2 schedules\"\n",
        );
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

#[test]
fn gets_one_finite_recurrence_as_deterministic_complete_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact\nid").unwrap(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store
        .create(
            RecurrenceId::new("unrelated").unwrap(),
            TaskGoal::new("unrelated goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:unrelated'",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "get",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,",
            "\"final_occurrence_unix_millis\":20,\"revision\":1}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn pages_exact_finite_recurrence_occurrences_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact\nid").unwrap(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    store
        .create(
            RecurrenceId::new("unrelated").unwrap(),
            TaskGoal::new("unrelated goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:unrelated'",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrences",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "1",
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":1,\"unix_millis\":15,\"definition_revision\":1},",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":2,\"unix_millis\":20,\"definition_revision\":1}",
            "],\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrences",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "3",
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":3,\"unix_millis\":25,\"definition_revision\":1}",
            "],\"next_offset\":null}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn persists_exact_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact\nid").unwrap(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persist",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "1",
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":2,\"unix_millis\":20,\"definition_revision\":1}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    let occurrence = store
        .load_occurrence(&RecurrenceId::new("exact\nid").unwrap(), 2)
        .unwrap()
        .expect("persisted occurrence");
    assert_eq!(occurrence.instant().unix_millis(), 20);
}

#[test]
fn inspects_exact_persisted_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact\nid").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 2).unwrap();
    let unrelated_id = RecurrenceId::new("unrelated").unwrap();
    store
        .create(
            unrelated_id.clone(),
            TaskGoal::new("unrelated goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&unrelated_id, 1, 0).unwrap();
    drop(store);
    let changed = rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' \
             WHERE event_type = 'recurrence.occurrence_persisted' \
             AND CAST(payload AS TEXT) LIKE '%\"recurrence_id\":\"unrelated\"%'",
            [],
        )
        .unwrap();
    assert_eq!(changed, 1);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":2,\"unix_millis\":20,\"definition_revision\":1}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_occurrence_lookup_validates_id_before_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence",
            database.to_str().expect("UTF-8 database path"),
            "",
            "0",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!database.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence",
            database.to_str().expect("UTF-8 database path"),
            "valid",
            "0",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_lookup_failed:",
        ));
    assert!(!database.exists());
}

#[test]
fn recurrence_occurrence_lookup_distinguishes_absence_and_selected_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    drop(store);

    let command = |offset: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "occurrence",
            database.to_str().expect("UTF-8 database path"),
            "exact",
            offset,
        ]);
        command
    };
    command("1")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_not_found:",
        ));

    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    command("0")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn recurrence_occurrence_persistence_validates_id_before_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persist",
            database.to_str().expect("UTF-8 database path"),
            "",
            "1",
            "0",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!database.exists());
}

#[test]
fn recurrence_occurrence_persistence_fails_closed_for_invalid_coordinates() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);

    for (id, revision, offset) in [
        ("absent", "1", "0"),
        ("exact", "2", "0"),
        ("exact", "1", "2"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "persist",
                database.to_str().expect("UTF-8 database path"),
                id,
                revision,
                offset,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_persistence_failed:",
            ));
    }

    let command = || {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "persist",
                database.to_str().expect("UTF-8 database path"),
                "exact",
                "1",
                "0",
            ])
            .assert()
    };
    command().success();
    command()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_persistence_failed:",
        ));

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    let occurrence = store
        .load_occurrence(&RecurrenceId::new("exact").unwrap(), 0)
        .unwrap()
        .expect("original persisted occurrence");
    assert_eq!(occurrence.instant().unix_millis(), 10);
}

#[test]
fn materializes_exact_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
    let task_id = TaskId::new("task\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 2).unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "2",
            "1",
            task_id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":2,\"unix_millis\":20,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task\\nid\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    let materialized = store
        .load_materialized_occurrence(&id, 2)
        .unwrap()
        .expect("materialized occurrence");
    assert_eq!(materialized.revision(), 2);
    assert_eq!(materialized.task_id(), &task_id);
    let tasks = TaskStore::open(&database).expect("reopened task store");
    let task = tasks.load(&task_id).unwrap().expect("materialized task");
    assert_eq!(task.goal().as_str(), "preserve \"exact\" goal");
}

#[test]
fn resolves_materialized_recurrence_by_exact_task_identity() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("bound\nrecurrence").unwrap();
    let task_id = TaskId::new("task\tidentity").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("run \"exactly\"").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    store
        .materialize_occurrence(&id, 1, 1, task_id.clone())
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "task",
            database.to_str().expect("UTF-8 database path"),
            task_id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"task_id\":\"task\\tidentity\",\"occurrence\":{",
            "\"recurrence_id\":\"bound\\nrecurrence\",\"goal\":\"run \\\"exactly\\\"\",",
            "\"offset\":1,\"unix_millis\":15,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task\\tidentity\"}}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_task_lookup_preserves_absence_and_fails_closed() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    drop(RecurrenceStore::open(&database).expect("empty recurrence store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["recurrence", "task", database.to_str().unwrap(), "unbound"])
        .assert()
        .success()
        .stdout("{\"task_id\":\"unbound\",\"occurrence\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["recurrence", "task", missing.to_str().unwrap(), ""])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_task_id:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["recurrence", "task", missing.to_str().unwrap(), "valid"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_task_lookup_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn recurrence_task_lookup_reports_ambiguous_corrupted_bindings() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let duplicate_task_id = TaskId::new("duplicate-task").unwrap();
    let mut store = RecurrenceStore::open(&database).unwrap();
    for (raw_id, task_id) in [
        ("first", duplicate_task_id.clone()),
        ("second", TaskId::new("second-task").unwrap()),
    ] {
        let id = RecurrenceId::new(raw_id).unwrap();
        store
            .create(
                id.clone(),
                TaskGoal::new(format!("goal-{raw_id}")).unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
        store.persist_occurrence(&id, 1, 0).unwrap();
        store.materialize_occurrence(&id, 0, 1, task_id).unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = CAST(json_object('task_id', ?1) AS BLOB)
             WHERE event_type = 'recurrence.occurrence_materialized'
               AND json_extract(payload, '$.task_id') = 'second-task'",
            [duplicate_task_id.as_str()],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "task",
            database.to_str().unwrap(),
            duplicate_task_id.as_str(),
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(
            "$: recurrence_task_lookup_failed: \"task duplicate-task is bound to 2 recurrence occurrences\"\n",
        );

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE events SET payload = CAST(json_object('task_id', 'second-task') AS BLOB)
                 WHERE event_type = 'recurrence.occurrence_materialized'
                   AND stream_id LIKE 'recurrence-occurrence:6:second:%'",
                [],
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .execute(
                "UPDATE events SET payload = X'7B7D'
                 WHERE event_type = 'recurrence.occurrence_persisted'
                   AND stream_id LIKE 'recurrence-occurrence:5:first:%'",
                [],
            )
            .unwrap(),
        1
    );
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "task",
            database.to_str().unwrap(),
            duplicate_task_id.as_str(),
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_task_lookup_failed: \"recurrence replay error:",
        ));
}

#[test]
fn recurrence_occurrence_materialization_validates_ids_before_storage_access() {
    let directory = tempdir().expect("recurrence database directory");

    for (name, id, task_id, expected_error) in [
        (
            "invalid-recurrence.sqlite3",
            "",
            "task",
            "invalid_recurrence_id",
        ),
        ("invalid-task.sqlite3", "valid", "", "invalid_task_id"),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize",
                database.to_str().expect("UTF-8 database path"),
                id,
                "0",
                "1",
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize",
            directory.path().to_str().expect("UTF-8 directory path"),
            "valid",
            "0",
            "1",
            "task",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_materialization_failed:",
        ));
}

#[test]
fn recurrence_occurrence_materialization_fails_closed_for_invalid_state() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    for offset in 0..=2 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    drop(store);
    let occupied = TaskId::new("occupied").unwrap();
    TaskStore::open(&database)
        .unwrap()
        .start(occupied.clone(), TaskGoal::new("occupied goal").unwrap())
        .unwrap();

    let assert_failure = |id: &str, offset: &str, revision: &str, task_id: &str| {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize",
                database.to_str().expect("UTF-8 database path"),
                id,
                offset,
                revision,
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_materialization_failed:",
            ));
    };

    assert_failure("exact", "3", "1", "missing-occurrence");
    assert_failure("exact", "0", "2", "stale");
    assert_failure("exact", "1", "1", occupied.as_str());

    let first_task = TaskId::new("first-binding").unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize",
            database.to_str().expect("UTF-8 database path"),
            "exact",
            "2",
            "1",
            first_task.as_str(),
        ])
        .assert()
        .success();
    assert_failure("exact", "2", "2", "replacement");

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    assert!(
        store
            .load_materialized_occurrence(&id, 0)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .load_materialized_occurrence(&id, 1)
            .unwrap()
            .is_none()
    );
    let materialized = store
        .load_materialized_occurrence(&id, 2)
        .unwrap()
        .expect("original materialization");
    assert_eq!(materialized.task_id(), &first_task);

    let tasks = TaskStore::open(&database).expect("reopened task store");
    for absent in ["missing-occurrence", "stale", "replacement"] {
        assert!(tasks.load(&TaskId::new(absent).unwrap()).unwrap().is_none());
    }
    assert_eq!(
        tasks
            .load(&occupied)
            .unwrap()
            .expect("original occupied task")
            .goal()
            .as_str(),
        "occupied goal"
    );
    assert!(tasks.load(&first_task).unwrap().is_some());
}

#[test]
fn recurrence_occurrence_paging_validates_inputs_before_storage_access() {
    let directory = tempdir().expect("recurrence database directory");

    for (name, id, page_size, expected_error) in [
        ("invalid-id.sqlite3", "", "1", "invalid_recurrence_id"),
        (
            "zero-size.sqlite3",
            "valid",
            "0",
            "invalid_occurrence_page_size",
        ),
        (
            "oversized.sqlite3",
            "valid",
            "1025",
            "invalid_occurrence_page_size",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "occurrences",
                database.to_str().expect("UTF-8 database path"),
                id,
                "0",
                page_size,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }
}

#[test]
fn recurrence_occurrence_paging_fails_closed_for_missing_invalid_or_corrupt_evidence() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrences",
            missing.to_str().expect("UTF-8 database path"),
            "absent",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "1", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "occurrences",
                database.to_str().expect("UTF-8 database path"),
                id,
                start,
                "1",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }

    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:exact'",
            [],
        )
        .unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrences",
            database.to_str().expect("UTF-8 database path"),
            "exact",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn pages_sparse_persisted_recurrence_occurrences_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact\nid").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    store.persist_occurrence(&id, 1, 3).unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persisted",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "0",
            "3",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":1,\"unix_millis\":15,\"definition_revision\":1}",
            "],\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persisted",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "2",
            "1",
        ])
        .assert()
        .success()
        .stdout("{\"occurrences\":[],\"next_offset\":3}\n")
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persisted",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "3",
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":3,\"unix_millis\":25,\"definition_revision\":1}",
            "],\"next_offset\":null}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn persisted_recurrence_occurrence_paging_isolates_unselected_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let exact = RecurrenceId::new("exact").unwrap();
    store
        .create(
            exact.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&exact, 1, 0).unwrap();
    store.persist_occurrence(&exact, 1, 2).unwrap();
    let unrelated = RecurrenceId::new("unrelated").unwrap();
    store
        .create(
            unrelated.clone(),
            TaskGoal::new("other").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&unrelated, 1, 0).unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&database).unwrap();
    let changed = connection
        .execute(
            "UPDATE events SET payload = X'7B7D' \
             WHERE event_type = 'recurrence.occurrence_persisted' \
             AND (CAST(payload AS TEXT) LIKE '%\"recurrence_id\":\"unrelated\"%' \
                  OR CAST(payload AS TEXT) LIKE '%\"offset\":2%')",
            [],
        )
        .unwrap();
    assert_eq!(changed, 2);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persisted",
            database.to_str().expect("UTF-8 database path"),
            "exact",
            "0",
            "1",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":0,",
            "\"unix_millis\":10,\"definition_revision\":1}",
            "],\"next_offset\":1}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn persisted_recurrence_occurrence_paging_validates_inputs_before_storage_access() {
    let directory = tempdir().expect("recurrence database directory");

    for (name, id, page_size, expected_error) in [
        ("invalid-id.sqlite3", "", "1", "invalid_recurrence_id"),
        (
            "zero-size.sqlite3",
            "valid",
            "0",
            "invalid_occurrence_page_size",
        ),
        (
            "oversized.sqlite3",
            "valid",
            "1025",
            "invalid_occurrence_page_size",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "persisted",
                database.to_str().expect("UTF-8 database path"),
                id,
                "0",
                page_size,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }
}

#[test]
fn persisted_recurrence_occurrence_paging_fails_closed_for_missing_or_invalid_evidence() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persisted",
            missing.to_str().expect("UTF-8 database path"),
            "absent",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: persisted_recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    drop(store);

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "2", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "persisted",
                database.to_str().expect("UTF-8 database path"),
                id,
                start,
                "1",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }

    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persisted",
            database.to_str().expect("UTF-8 database path"),
            "exact",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: persisted_recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn recurrence_lookup_validates_id_and_reports_missing_evidence_without_creation() {
    let directory = tempdir().expect("recurrence database directory");
    let missing_database = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "get",
            missing_database.to_str().expect("UTF-8 database path"),
            "",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!missing_database.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "get",
            missing_database.to_str().expect("UTF-8 database path"),
            "absent",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: recurrence_lookup_failed:"));
    assert!(!missing_database.exists());

    let database = directory.path().join("events.sqlite3");
    drop(RecurrenceStore::open(&database).expect("empty recurrence store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "get",
            database.to_str().expect("UTF-8 database path"),
            "absent",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: recurrence_not_found:"));
}

#[test]
fn recurrence_lookup_fails_closed_on_corrupt_exact_stream() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("corrupt").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:corrupt'",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "get",
            database.to_str().expect("UTF-8 database path"),
            "corrupt",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: recurrence_lookup_failed:"));
}

#[test]
fn inspects_finite_recurrences_as_deterministic_complete_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("z-last").unwrap(),
            TaskGoal::new("later").unwrap(),
            ScheduleInstant::from_unix_millis(40),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store
        .create(
            RecurrenceId::new("a\nfirst").unwrap(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "inspect",
            database.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"a\\nfirst\",\"goal\":\"preserve \\\"exact\\\" goal\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,\"final_occurrence_unix_millis\":20,\"revision\":1},",
            "{\"id\":\"z-last\",\"goal\":\"later\",\"anchor_unix_millis\":40,\"interval_millis\":2,\"occurrence_count\":1,\"final_occurrence_unix_millis\":40,\"revision\":1}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_inspection_reports_empty_and_missing_storage_without_creation() {
    let directory = tempdir().expect("recurrence database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(RecurrenceStore::open(&empty).expect("empty recurrence store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "inspect",
            empty.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .success()
        .stdout("{\"recurrences\":[]}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "inspect",
            missing.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn recurrence_inspection_fails_closed_on_corrupt_inventory() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("corrupt").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:corrupt'",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "inspect",
            database.to_str().expect("UTF-8 database path"),
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_inspection_failed:",
        ));
}

#[test]
fn creates_one_finite_recurrence_as_deterministic_complete_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "create",
            database.to_str().expect("UTF-8 database path"),
            "exact\nid",
            "preserve \"exact\" goal",
            "10",
            "5",
            "3",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,",
            "\"final_occurrence_unix_millis\":20,\"revision\":1}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    let recurrence = store
        .load(&RecurrenceId::new("exact\nid").unwrap())
        .unwrap()
        .expect("persisted recurrence");
    assert_eq!(recurrence.goal().as_str(), "preserve \"exact\" goal");
    assert_eq!(recurrence.final_occurrence().unix_millis(), 20);
    assert_eq!(recurrence.revision(), 1);
}

#[test]
fn recurrence_creation_validates_before_opening_storage() {
    let directory = tempdir().expect("recurrence database directory");

    for (name, id, goal, interval, count, expected_error) in [
        (
            "invalid-id.sqlite3",
            " ",
            "goal",
            "1",
            "1",
            "invalid_recurrence_id",
        ),
        (
            "invalid-goal.sqlite3",
            "id",
            "",
            "1",
            "1",
            "invalid_task_goal",
        ),
        (
            "zero-interval.sqlite3",
            "id",
            "goal",
            "0",
            "1",
            "invalid_recurrence_interval",
        ),
        (
            "zero-count.sqlite3",
            "id",
            "goal",
            "1",
            "0",
            "invalid_occurrence_count",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "create",
                database.to_str().expect("UTF-8 database path"),
                id,
                goal,
                "1",
                interval,
                count,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }
}

#[test]
fn recurrence_creation_rejects_overflow_and_preserves_duplicates() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "create",
            database.to_str().expect("UTF-8 database path"),
            "same-id",
            "original",
            "1",
            "2",
            "2",
        ])
        .assert()
        .success();

    let max_anchor = u64::MAX.to_string();
    for (id, goal, anchor, interval, count) in [
        ("overflow", "goal", max_anchor.as_str(), "1", "2"),
        ("same-id", "replacement", "5", "3", "4"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "create",
                database.to_str().expect("UTF-8 database path"),
                id,
                goal,
                anchor,
                interval,
                count,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_creation_failed:",
            ));
    }

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    assert!(
        store
            .load(&RecurrenceId::new("overflow").unwrap())
            .unwrap()
            .is_none()
    );
    let original = store
        .load(&RecurrenceId::new("same-id").unwrap())
        .unwrap()
        .expect("original recurrence");
    assert_eq!(original.goal().as_str(), "original");
    assert_eq!(original.anchor().unix_millis(), 1);
    assert_eq!(original.interval().millis(), 2);
    assert_eq!(original.occurrence_count().get(), 2);
}
