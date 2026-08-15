use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;
use vela_kernel::{
    scheduler::{
        OccurrenceCount, OccurrencePageSize, RecurrenceCancellation, RecurrenceId,
        RecurrenceOccurrenceRelease, RecurrenceStore, ScheduleCancellation, ScheduleId,
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

fn insert_recurrence_cancellation(
    database: &std::path::Path,
    recurrence_id: &str,
    stream_version: u64,
    reason: &str,
) {
    rusqlite::Connection::open(database)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES (?1, ?2, 'recurrence.cancelled', 1, ?3)",
            rusqlite::params![
                format!("recurrence:{recurrence_id}"),
                stream_version,
                format!(r#"{{"reason":{reason:?}}}"#).into_bytes(),
            ],
        )
        .unwrap();
}

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
fn pages_durable_schedules_with_exact_keyset_continuation() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for (id, goal) in [
        ("z-last", "later"),
        ("a\nfirst", "preserve \"exact\" goal"),
        ("middle", "middle goal"),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
            )
            .unwrap();
    }
    let middle = ScheduleId::new("middle").unwrap();
    store
        .cancel(
            &middle,
            1,
            ScheduleCancellation::new("operator\trequest").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"schedules\":[",
            "{\"id\":\"a\\nfirst\",\"goal\":\"preserve \\\"exact\\\" goal\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null},",
            "{\"id\":\"middle\",\"goal\":\"middle goal\",\"due_at_unix_millis\":10,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator\\trequest\",\"latest_release\":null,\"task_id\":null}",
            "],\"next_after\":\"middle\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    for (after, expected) in [
        (
            "middle",
            concat!(
                "{\"schedules\":[",
                "{\"id\":\"z-last\",\"goal\":\"later\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}",
                "],\"next_after\":null}\n"
            ),
        ),
        (
            "n-nonexistent",
            concat!(
                "{\"schedules\":[",
                "{\"id\":\"z-last\",\"goal\":\"later\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}",
                "],\"next_after\":null}\n"
            ),
        ),
        ("zz-nonexistent", "{\"schedules\":[],\"next_after\":null}\n"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "page",
                database.to_str().expect("UTF-8 database path"),
                "2",
                after,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn schedule_paging_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("schedule database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(ScheduleStore::open(&empty).expect("empty schedule store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "page",
            empty.to_str().expect("UTF-8 database path"),
            "1",
        ])
        .assert()
        .success()
        .stdout("{\"schedules\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    for (page_size, after, diagnostic) in [
        ("0", None, "$: invalid_schedule_page_size:"),
        ("1025", None, "$: invalid_schedule_page_size:"),
        ("1", Some("   "), "$: invalid_schedule_id:"),
    ] {
        let mut arguments = vec![
            "schedule",
            "page",
            missing.to_str().expect("UTF-8 database path"),
            page_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "page",
            missing.to_str().expect("UTF-8 database path"),
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_page_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn schedule_paging_isolates_corruption_outside_the_selected_window() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for id in ["a-corrupt", "b-valid", "c-valid", "d-corrupt"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id IN ('schedule:a-corrupt', 'schedule:d-corrupt')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "1",
            "a-corrupt",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"schedules\":[",
            "{\"id\":\"b-valid\",\"goal\":\"goal\",\"due_at_unix_millis\":1,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}",
            "],\"next_after\":\"b-valid\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "1",
            "b-valid",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_page_inspection_failed:",
        ));
}

#[test]
fn pages_schedules_sparsely_by_status_with_scan_cursor_progress() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for (id, goal) in [
        ("alpha", "pending alpha"),
        ("bravo", "pending bravo"),
        ("charlie", "cancel \"exactly\""),
        ("delta", "cancel delta"),
        ("echo", "cancel later"),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
            )
            .unwrap();
    }
    for id in ["charlie", "delta", "echo"] {
        store
            .cancel(
                &ScheduleId::new(id).unwrap(),
                1,
                ScheduleCancellation::new("operator request").unwrap(),
            )
            .unwrap();
    }
    drop(store);

    let cases = [
        (None, "{\"schedules\":[],\"next_after\":\"bravo\"}\n"),
        (
            Some("bravo"),
            concat!(
                "{\"schedules\":[",
                "{\"id\":\"charlie\",\"goal\":\"cancel \\\"exactly\\\"\",\"due_at_unix_millis\":10,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator request\",\"latest_release\":null,\"task_id\":null},",
                "{\"id\":\"delta\",\"goal\":\"cancel delta\",\"due_at_unix_millis\":10,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator request\",\"latest_release\":null,\"task_id\":null}",
                "],\"next_after\":\"delta\"}\n"
            ),
        ),
        (
            Some("delta"),
            "{\"schedules\":[{\"id\":\"echo\",\"goal\":\"cancel later\",\"due_at_unix_millis\":10,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator request\",\"latest_release\":null,\"task_id\":null}],\"next_after\":null}\n",
        ),
        (
            Some("coconut"),
            concat!(
                "{\"schedules\":[",
                "{\"id\":\"delta\",\"goal\":\"cancel delta\",\"due_at_unix_millis\":10,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator request\",\"latest_release\":null,\"task_id\":null},",
                "{\"id\":\"echo\",\"goal\":\"cancel later\",\"due_at_unix_millis\":10,\"status\":\"cancelled\",\"revision\":2,\"cancellation\":\"operator request\",\"latest_release\":null,\"task_id\":null}",
                "],\"next_after\":null}\n"
            ),
        ),
        (Some("zzzz"), "{\"schedules\":[],\"next_after\":null}\n"),
    ];
    for (after, expected) in cases {
        let mut arguments = vec![
            "schedule",
            "status-page",
            database.to_str().expect("UTF-8 database path"),
            "cancelled",
            "2",
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn schedule_status_paging_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("schedule database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(ScheduleStore::open(&empty).expect("empty schedule store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "status-page",
            empty.to_str().expect("UTF-8 database path"),
            "pending",
            "1",
        ])
        .assert()
        .success()
        .stdout("{\"schedules\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    for (status, scan_size, after, diagnostic) in [
        ("PENDING", "1", None, "$: invalid_schedule_status:"),
        ("pending", "0", None, "$: invalid_schedule_page_size:"),
        ("pending", "1025", None, "$: invalid_schedule_page_size:"),
        ("pending", "1", Some("   "), "$: invalid_schedule_id:"),
    ] {
        let mut arguments = vec![
            "schedule",
            "status-page",
            missing.to_str().expect("UTF-8 database path"),
            status,
            scan_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "status-page",
            missing.to_str().expect("UTF-8 database path"),
            "pending",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_status_page_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn schedule_status_paging_fails_on_selected_lookahead_and_isolates_other_corruption() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for id in ["a-corrupt", "b-valid", "c-valid", "d-corrupt"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id IN ('schedule:a-corrupt', 'schedule:d-corrupt')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "status-page",
            database.to_str().expect("UTF-8 database path"),
            "pending",
            "1",
            "a-corrupt",
        ])
        .assert()
        .success()
        .stdout("{\"schedules\":[{\"id\":\"b-valid\",\"goal\":\"goal\",\"due_at_unix_millis\":1,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}],\"next_after\":\"b-valid\"}\n")
        .stderr(predicate::str::is_empty());

    for after in [None, Some("b-valid")] {
        let mut arguments = vec![
            "schedule",
            "status-page",
            database.to_str().expect("UTF-8 database path"),
            "pending",
            "1",
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: schedule_status_page_inspection_failed:",
            ));
    }
}

#[test]
fn pages_due_schedules_sparsely_with_scan_cursor_progress() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for (id, goal, due_at) in [
        ("alpha", "future alpha", 30),
        ("bravo", "cancelled bravo", 5),
        ("charlie", "due at \"cutoff\"", 20),
        ("delta", "due earlier", 10),
        ("echo", "future echo", 21),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(due_at),
            )
            .unwrap();
    }
    store
        .cancel(
            &ScheduleId::new("bravo").unwrap(),
            1,
            ScheduleCancellation::new("operator request").unwrap(),
        )
        .unwrap();
    drop(store);

    let cases = [
        (None, "{\"schedules\":[],\"next_after\":\"bravo\"}\n"),
        (
            Some("bravo"),
            concat!(
                "{\"schedules\":[",
                "{\"id\":\"charlie\",\"goal\":\"due at \\\"cutoff\\\"\",\"due_at_unix_millis\":20,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null},",
                "{\"id\":\"delta\",\"goal\":\"due earlier\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}",
                "],\"next_after\":\"delta\"}\n"
            ),
        ),
        (Some("delta"), "{\"schedules\":[],\"next_after\":null}\n"),
        (
            Some("coconut"),
            "{\"schedules\":[{\"id\":\"delta\",\"goal\":\"due earlier\",\"due_at_unix_millis\":10,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}],\"next_after\":null}\n",
        ),
        (Some("zzzz"), "{\"schedules\":[],\"next_after\":null}\n"),
    ];
    for (after, expected) in cases {
        let mut arguments = vec![
            "schedule",
            "due-page",
            database.to_str().expect("UTF-8 database path"),
            "20",
            "2",
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn schedule_due_paging_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("schedule database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(ScheduleStore::open(&empty).expect("empty schedule store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due-page",
            empty.to_str().expect("UTF-8 database path"),
            "0",
            "1",
        ])
        .assert()
        .success()
        .stdout("{\"schedules\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    for (scan_size, after, diagnostic) in [
        ("0", None, "$: invalid_schedule_page_size:"),
        ("1025", None, "$: invalid_schedule_page_size:"),
        ("1", Some("   "), "$: invalid_schedule_id:"),
    ] {
        let mut arguments = vec![
            "schedule",
            "due-page",
            missing.to_str().expect("UTF-8 database path"),
            "20",
            scan_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due-page",
            missing.to_str().expect("UTF-8 database path"),
            "20",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_due_page_inspection_failed:",
        ));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due-page",
            missing.to_str().expect("UTF-8 database path"),
            "not-a-millisecond",
            "1",
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
fn schedule_due_paging_fails_on_selected_lookahead_and_isolates_other_corruption() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for id in ["a-corrupt", "b-valid", "c-valid", "d-corrupt"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id IN ('schedule:a-corrupt', 'schedule:d-corrupt')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "due-page",
            database.to_str().expect("UTF-8 database path"),
            "1",
            "1",
            "a-corrupt",
        ])
        .assert()
        .success()
        .stdout("{\"schedules\":[{\"id\":\"b-valid\",\"goal\":\"goal\",\"due_at_unix_millis\":1,\"status\":\"pending\",\"revision\":1,\"cancellation\":null,\"latest_release\":null,\"task_id\":null}],\"next_after\":\"b-valid\"}\n")
        .stderr(predicate::str::is_empty());

    for after in [None, Some("b-valid")] {
        let mut arguments = vec![
            "schedule",
            "due-page",
            database.to_str().expect("UTF-8 database path"),
            "1",
            "1",
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: schedule_due_page_inspection_failed:",
            ));
    }
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
fn pages_complete_schedule_histories_with_exact_keyset_continuation() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for (id, goal) in [
        ("z-last", "last"),
        ("a\nfirst", "run \"carefully\""),
        ("middle", "stop later"),
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
            )
            .unwrap();
    }
    let first = ScheduleId::new("a\nfirst").unwrap();
    let claimed = store
        .claim(&first, 1, ScheduleInstant::from_unix_millis(10))
        .unwrap();
    let released = store
        .release(
            &first,
            claimed.revision(),
            ScheduleRelease::new("worker\trecovery").unwrap(),
        )
        .unwrap();
    let reclaimed = store
        .claim(
            &first,
            released.revision(),
            ScheduleInstant::from_unix_millis(10),
        )
        .unwrap();
    store
        .materialize(
            &first,
            reclaimed.revision(),
            TaskId::new("task\nbound").unwrap(),
        )
        .unwrap();
    let middle = ScheduleId::new("middle").unwrap();
    store
        .cancel(
            &middle,
            1,
            ScheduleCancellation::new("operator request").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "histories",
            database.to_str().expect("UTF-8 database path"),
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"histories\":[",
            "{\"id\":\"a\\nfirst\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"run \\\"carefully\\\"\",\"due_at_unix_millis\":10},",
            "{\"revision\":2,\"type\":\"claimed\"},",
            "{\"revision\":3,\"type\":\"released\",\"reason\":\"worker\\trecovery\"},",
            "{\"revision\":4,\"type\":\"claimed\"},",
            "{\"revision\":5,\"type\":\"materialized\",\"task_id\":\"task\\nbound\"}]},",
            "{\"id\":\"middle\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"stop later\",\"due_at_unix_millis\":10},",
            "{\"revision\":2,\"type\":\"cancelled\",\"reason\":\"operator request\"}]}",
            "],\"next_after\":\"middle\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    for (after, expected) in [
        (
            "middle",
            "{\"histories\":[{\"id\":\"z-last\",\"history\":[{\"revision\":1,\"type\":\"created\",\"goal\":\"last\",\"due_at_unix_millis\":10}]}],\"next_after\":null}\n",
        ),
        (
            "n-nonexistent",
            "{\"histories\":[{\"id\":\"z-last\",\"history\":[{\"revision\":1,\"type\":\"created\",\"goal\":\"last\",\"due_at_unix_millis\":10}]}],\"next_after\":null}\n",
        ),
        ("zz", "{\"histories\":[],\"next_after\":null}\n"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "schedule",
                "histories",
                database.to_str().expect("UTF-8 database path"),
                "2",
                after,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn schedule_history_paging_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("schedule database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(ScheduleStore::open(&empty).expect("empty schedule store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "histories", empty.to_str().unwrap(), "1"])
        .assert()
        .success()
        .stdout("{\"histories\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    for (page_size, after, diagnostic) in [
        ("0", None, "$: invalid_schedule_page_size:"),
        ("1025", None, "$: invalid_schedule_page_size:"),
        ("1", Some("   "), "$: invalid_schedule_id:"),
    ] {
        let mut arguments = vec![
            "schedule",
            "histories",
            missing.to_str().unwrap(),
            page_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["schedule", "histories", missing.to_str().unwrap(), "1"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: schedule_history_page_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn schedule_history_paging_isolates_corruption_outside_the_bounded_window() {
    let directory = tempdir().expect("schedule database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&database).expect("writable schedule store");
    for id in ["a-corrupt", "b-valid", "c-valid", "d-corrupt"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id IN ('schedule:a-corrupt', 'schedule:d-corrupt')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "schedule",
            "histories",
            database.to_str().unwrap(),
            "1",
            "a-corrupt",
        ])
        .assert()
        .success()
        .stdout("{\"histories\":[{\"id\":\"b-valid\",\"history\":[{\"revision\":1,\"type\":\"created\",\"goal\":\"goal\",\"due_at_unix_millis\":1}]}],\"next_after\":\"b-valid\"}\n")
        .stderr(predicate::str::is_empty());

    for after in [None, Some("b-valid")] {
        let mut arguments = vec!["schedule", "histories", database.to_str().unwrap(), "1"];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: schedule_history_page_inspection_failed:",
            ));
    }
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
            "\"nested/first.json\": valid\n\"second.json\": valid",
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
        .stderr(predicate::str::contains(
            "\"malformed.json\": malformed_record",
        ))
        .stderr(predicate::str::contains(
            "\"semantic.json\": task.title: required",
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

#[cfg(unix)]
#[test]
fn corpus_inspection_escapes_untrusted_record_paths() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("inspection corpus");
    fs::copy(source, corpus.path().join("forged\n\r\t\u{1b}[31m.json"))
        .expect("valid record with untrusted path");

    let output = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "inspect",
            corpus.path().to_str().expect("UTF-8 corpus path"),
        ])
        .output()
        .expect("inspection output");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 escaped output");
    assert_eq!(stdout.lines().count(), 2);
    assert!(stdout.starts_with("\"forged\\n\\r\\t\\u{1b}[31m.json\": valid\n"));
    assert!(!stdout.contains('\r'));
    assert!(!stdout.contains('\t'));
    assert!(!stdout.contains('\u{1b}'));
}

#[cfg(unix)]
#[test]
fn corpus_inspection_preserves_non_utf8_record_path_identity() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("inspection corpus");
    for filename in [b"forged-\xff.json", b"forged-\xfe.json"] {
        fs::copy(
            &source,
            corpus.path().join(OsString::from_vec(filename.to_vec())),
        )
        .expect("valid record with non-UTF-8 path");
    }

    let output = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "inspect",
            corpus.path().to_str().expect("UTF-8 corpus path"),
        ])
        .output()
        .expect("inspection output");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 escaped output");
    assert!(stdout.contains("\"forged-\\xFF.json\": valid"));
    assert!(stdout.contains("\"forged-\\xFE.json\": valid"));
}

#[cfg(unix)]
#[test]
fn corpus_sampling_escapes_untrusted_record_paths_in_diagnostics() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("forged\n\r\t\u{1b}[31m.json"), "{}")
        .expect("malformed record with untrusted path");

    let output = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
        ])
        .output()
        .expect("sampling output");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 escaped diagnostic");
    assert_eq!(stderr.lines().count(), 1);
    assert!(stderr.starts_with("\"forged\\n\\r\\t\\u{1b}[31m.json\": malformed_record:"));
    assert!(!stderr.contains('\r'));
    assert!(!stderr.contains('\t'));
    assert!(!stderr.contains('\u{1b}'));
}

#[test]
fn corpus_sampling_is_deterministic_and_bounded() {
    let corpus = format!("{}/tests/fixtures/corpus/valid", env!("CARGO_MANIFEST_DIR"));

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["corpus", "sample", &corpus, "1"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");

    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "nested/first.json");
    assert_eq!(sample[0]["record"]["task"]["title"], "First");
}

#[test]
fn corpus_sampling_filters_by_exact_trust_and_allows_empty_matches() {
    let source = format!("{}/tests/fixtures/corpus/valid", env!("CARGO_MANIFEST_DIR"));
    let corpus = tempdir().expect("sample corpus");
    fs::create_dir(corpus.path().join("nested")).expect("nested sample directory");
    fs::copy(
        format!("{source}/nested/first.json"),
        corpus.path().join("nested/first.json"),
    )
    .expect("reviewed record");
    let curated = fs::read_to_string(format!("{source}/second.json"))
        .expect("curated record source")
        .replace("\"reviewed\"", "\"curated\"");
    fs::write(corpus.path().join("second.json"), curated).expect("curated record");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--trust",
            "curated",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "second.json");
    assert_eq!(sample[0]["record"]["trust"], "curated");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--trust",
            "untrusted",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_filters_by_example_type_and_composes_with_trust() {
    let source = format!("{}/tests/fixtures/corpus/valid", env!("CARGO_MANIFEST_DIR"));
    let corpus = tempdir().expect("sample corpus");
    fs::create_dir(corpus.path().join("nested")).expect("nested sample directory");
    fs::copy(
        format!("{source}/nested/first.json"),
        corpus.path().join("nested/first.json"),
    )
    .expect("positive reviewed record");
    let negative = fs::read_to_string(format!("{source}/second.json"))
        .expect("negative record source")
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        );
    fs::write(corpus.path().join("second.json"), &negative).expect("negative reviewed record");
    fs::write(
        corpus.path().join("third.json"),
        negative
            .replace("\"Second\"", "\"Third\"")
            .replace("\"reviewed\"", "\"curated\""),
    )
    .expect("negative curated record");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--example",
            "positive",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "nested/first.json");
    assert_eq!(sample[0]["record"]["example"]["type"], "positive");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--example",
            "negative",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "second.json");
    assert_eq!(sample[0]["record"]["example"]["type"], "negative");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--trust",
            "curated",
            "--example",
            "negative",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "third.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--trust",
            "curated",
            "--example",
            "positive",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_filters_by_attempt_outcome_after_composing_other_filters() {
    let source = format!("{}/tests/fixtures/corpus/valid", env!("CARGO_MANIFEST_DIR"));
    let corpus = tempdir().expect("sample corpus");
    fs::create_dir(corpus.path().join("nested")).expect("nested sample directory");
    fs::copy(
        format!("{source}/nested/first.json"),
        corpus.path().join("nested/first.json"),
    )
    .expect("record without attempts");
    let source_record = fs::read_to_string(format!("{source}/second.json")).expect("record source");
    let failure = source_record.replace(
        "\"attempts\": []",
        "\"attempts\": [\
         {\"summary\":\"first try\",\"outcome\":\"success\",\"diagnostic\":null,\"patch\":\"second.rs\"},\
         {\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
    );
    fs::write(corpus.path().join("second.json"), &failure).expect("mixed outcome record");
    let curated_negative = failure
        .replace("\"Second\"", "\"Third\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        );
    fs::write(corpus.path().join("third.json"), curated_negative)
        .expect("curated negative failure");
    let blocked = source_record
        .replace("\"Second\"", "\"Fourth\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"waiting\",\"outcome\":\"blocked\",\"diagnostic\":\"dependency unavailable\",\"patch\":\"second.rs\"}]",
        );
    fs::write(corpus.path().join("fourth.json"), blocked).expect("blocked record");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--attempt-outcome",
            "failure",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "second.json");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "third.json");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--attempt-outcome",
            "blocked",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "fourth.json");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--attempt-outcome",
            "success",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 2);
    assert_eq!(sample[0]["path"], "second.json");
    assert_eq!(sample[1]["path"], "third.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--attempt-outcome",
            "success",
            "--trust",
            "untrusted",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_filters_by_verification_status_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-empty.json"),
        source_record.replace("\"Second\"", "\"Empty\"").replace(
            "\"verified\":true,\"verification\":[{\"command\":\"test\",\"status\":\"passed\"}]",
            "\"verified\":false,\"verification\":[]",
        ),
    )
    .expect("record without verification");
    fs::write(
        corpus.path().join("b-mixed.json"),
        source_record.replace(
            "[{\"command\":\"test\",\"status\":\"passed\"}]",
            "[{\"command\":\"first\",\"status\":\"failed\"},{\"command\":\"test\",\"status\":\"passed\"}]",
        ),
    )
    .expect("mixed verification record");
    fs::write(
        corpus.path().join("c-not-run.json"),
        source_record.replace("\"Second\"", "\"Not run\"").replace(
            "\"verified\":true,\"verification\":[{\"command\":\"test\",\"status\":\"passed\"}]",
            "\"verified\":false,\"verification\":[{\"command\":\"test\",\"status\":\"not_run\"}]",
        ),
    )
    .expect("not-run verification record");
    let composed = source_record
        .replace("\"Second\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
        )
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        )
        .replace(
            "[{\"command\":\"test\",\"status\":\"passed\"}]",
            "[{\"command\":\"first\",\"status\":\"failed\"},{\"command\":\"test\",\"status\":\"passed\"}]",
        );
    fs::write(corpus.path().join("d-composed.json"), composed)
        .expect("record matching every filter");

    for (status, expected_path) in [
        ("failed", "b-mixed.json"),
        ("passed", "b-mixed.json"),
        ("not-run", "c-not-run.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--verification-status",
                status,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "failed",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "d-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--verification-status",
            "failed",
            "--trust",
            "untrusted",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_unknown_verification_status_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--verification-status",
            "skipped",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_filters_by_verified_outcome_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-unverified.json"),
        source_record
            .replace("\"Second\"", "\"Unverified\"")
            .replace("\"verified\":true", "\"verified\":false"),
    )
    .expect("unverified record");
    fs::write(corpus.path().join("b-verified.json"), &source_record).expect("verified record");
    let composed = source_record
        .replace("\"Second\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
        )
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        );
    fs::write(corpus.path().join("c-composed.json"), composed)
        .expect("record matching every filter");

    for (verified, expected_path) in [("false", "a-unverified.json"), ("true", "b-verified.json")] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--verified",
                verified,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "passed",
            "--verified",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--verified",
            "false",
            "--trust",
            "untrusted",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_verified_value_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--verified",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_filters_by_sanitation_outcome_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-unsanitized.json"),
        source_record
            .replace("\"Second\"", "\"Unsanitized\"")
            .replace(
                "\"passed\":true,\"blockers\":[]",
                "\"passed\":false,\"blockers\":[]",
            ),
    )
    .expect("unsanitized record without blockers");
    fs::write(corpus.path().join("b-sanitized.json"), &source_record).expect("sanitized record");
    let composed = source_record
        .replace("\"Second\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
        )
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        );
    fs::write(corpus.path().join("c-composed.json"), composed)
        .expect("record matching every filter");

    for (sanitation_passed, expected_path) in [
        ("false", "a-unsanitized.json"),
        ("true", "b-sanitized.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--sanitation-passed",
                sanitation_passed,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "passed",
            "--verified",
            "true",
            "--sanitation-passed",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--sanitation-passed",
            "false",
            "--trust",
            "untrusted",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_filters_by_sanitation_blocker_presence_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    let unsanitized = source_record
        .replace("\"Second\"", "\"Unsanitized\"")
        .replace(
            "\"passed\":true,\"blockers\":[]",
            "\"passed\":false,\"blockers\":[]",
        );
    fs::write(corpus.path().join("a-unblocked.json"), &unsanitized)
        .expect("unsanitized record without blockers");
    let blocked = unsanitized
        .replace("\"Unsanitized\"", "\"Blocked\"")
        .replace("\"blockers\":[]", "\"blockers\":[\"license review\"]");
    fs::write(corpus.path().join("b-blocked.json"), blocked).expect("blocked record");
    let composed = unsanitized
        .replace("\"Unsanitized\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
        )
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        )
        .replace("\"blockers\":[]", "\"blockers\":[\"review finding\"]");
    fs::write(corpus.path().join("c-composed.json"), composed)
        .expect("record matching every filter");

    for (has_blockers, expected_path) in [("false", "a-unblocked.json"), ("true", "b-blocked.json")]
    {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--has-sanitation-blockers",
                has_blockers,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "passed",
            "--verified",
            "true",
            "--sanitation-passed",
            "false",
            "--has-sanitation-blockers",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--sanitation-passed",
            "true",
            "--has-sanitation-blockers",
            "true",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_sanitation_blocker_value_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--has-sanitation-blockers",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_filters_by_attempt_diagnostic_presence_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-no-attempts.json"),
        source_record.replace("\"Second\"", "\"No attempts\""),
    )
    .expect("record without attempts");
    fs::write(
        corpus.path().join("b-null-diagnostic.json"),
        source_record
            .replace("\"Second\"", "\"Null diagnostic\"")
            .replace(
                "\"attempts\": []",
                "\"attempts\": [{\"summary\":\"clean pass\",\"outcome\":\"success\",\"diagnostic\":null,\"patch\":\"second.rs\"}]",
            ),
    )
    .expect("record with a null diagnostic");
    fs::write(
        corpus.path().join("c-omitted-diagnostic.json"),
        source_record
            .replace("\"Second\"", "\"Omitted diagnostic\"")
            .replace(
                "\"attempts\": []",
                "\"attempts\": [{\"summary\":\"clean pass\",\"outcome\":\"success\",\"patch\":\"second.rs\"}]",
            ),
    )
    .expect("record with an omitted diagnostic");
    fs::write(
        corpus.path().join("c-present-diagnostic.json"),
        source_record
            .replace("\"Second\"", "\"Present diagnostic\"")
            .replace(
                "\"attempts\": []",
                "\"attempts\": [{\"summary\":\"clean pass\",\"outcome\":\"success\",\"diagnostic\":null,\"patch\":\"second.rs\"},{\"summary\":\"observed evidence\",\"outcome\":\"success\",\"diagnostic\":\"\",\"patch\":\"second.rs\"}]",
            ),
    )
    .expect("record with null and present diagnostics");
    let composed = source_record
        .replace("\"Second\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
        )
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        )
        .replace(
            "\"passed\":true,\"blockers\":[]",
            "\"passed\":false,\"blockers\":[\"review finding\"]",
        )
        .replace("\"lessons\": []", "\"lessons\": [\"correct the boundary\"]");
    fs::write(corpus.path().join("d-composed.json"), composed)
        .expect("record matching every filter");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--has-attempt-diagnostic",
            "false",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    let paths: Vec<_> = sample
        .as_array()
        .expect("sample array")
        .iter()
        .map(|entry| entry["path"].as_str().expect("sample path"))
        .collect();
    assert_eq!(
        paths,
        [
            "a-no-attempts.json",
            "b-null-diagnostic.json",
            "c-omitted-diagnostic.json"
        ]
    );

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--has-attempt-diagnostic",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-present-diagnostic.json");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "passed",
            "--verified",
            "true",
            "--sanitation-passed",
            "false",
            "--has-sanitation-blockers",
            "true",
            "--has-lessons",
            "true",
            "--has-attempt-diagnostic",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "d-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--has-attempt-diagnostic",
            "false",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_attempt_diagnostic_value_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--has-attempt-diagnostic",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_filters_by_rejection_rationale_presence_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-null.json"),
        source_record.replace("\"Second\"", "\"Null rationale\""),
    )
    .expect("record with null rationale");
    fs::write(
        corpus.path().join("a-omitted.json"),
        source_record
            .replace("\"Second\"", "\"Omitted rationale\"")
            .replace(",\"rejection_rationale\":null", ""),
    )
    .expect("record with omitted rationale");
    fs::write(
        corpus.path().join("b-empty.json"),
        source_record
            .replace("\"Second\"", "\"Empty rationale\"")
            .replace(
                "\"rejection_rationale\":null",
                "\"rejection_rationale\":\"\"",
            ),
    )
    .expect("record with empty present rationale");
    fs::write(
        corpus.path().join("c-present.json"),
        source_record
            .replace("\"Second\"", "\"Present rationale\"")
            .replace(
                "\"type\":\"positive\",\"rejection_rationale\":null",
                "\"type\":\"negative\",\"rejection_rationale\":\"review rejected this example\"",
            ),
    )
    .expect("record with present rationale");
    fs::write(
        corpus.path().join("d-curated.json"),
        source_record
            .replace("\"Second\"", "\"Curated rationale\"")
            .replace("\"reviewed\"", "\"curated\"")
            .replace(
                "\"type\":\"positive\",\"rejection_rationale\":null",
                "\"type\":\"negative\",\"rejection_rationale\":\"curator rejected this example\"",
            ),
    )
    .expect("curated record with present rationale");

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--has-rejection-rationale",
            "false",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    let paths: Vec<_> = sample
        .as_array()
        .expect("sample array")
        .iter()
        .map(|entry| entry["path"].as_str().expect("sample path"))
        .collect();
    assert_eq!(paths, ["a-null.json", "a-omitted.json"]);

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "2",
            "--has-rejection-rationale",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    let paths: Vec<_> = sample
        .as_array()
        .expect("sample array")
        .iter()
        .map(|entry| entry["path"].as_str().expect("sample path"))
        .collect();
    assert_eq!(paths, ["b-empty.json", "c-present.json"]);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--has-rejection-rationale",
            "true",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"path\":\"d-curated.json\""))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--has-rejection-rationale",
            "false",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_rejection_rationale_value_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--has-rejection-rationale",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_rejection_rationale_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--has-rejection-rationale",
            "false",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_attempt_presence_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-no-attempts.json"),
        source_record.replace("\"Second\"", "\"No attempts\""),
    )
    .expect("record without attempts");
    fs::write(
        corpus.path().join("b-one-attempt.json"),
        source_record
            .replace("\"Second\"", "\"One attempt\"")
            .replace(
                "\"attempts\": []",
                "\"attempts\": [{\"summary\":\"implemented\",\"outcome\":\"success\",\"diagnostic\":null,\"patch\":\"second.rs\"}]",
            ),
    )
    .expect("record with one attempt");
    let composed = source_record
        .replace("\"Second\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"first try\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"first.rs\"},{\"summary\":\"fixed\",\"outcome\":\"success\",\"diagnostic\":null,\"patch\":\"second.rs\"}]",
        );
    fs::write(corpus.path().join("c-composed.json"), composed)
        .expect("record with multiple attempts matching every filter");

    for (has_attempts, expected_path) in [
        ("false", "a-no-attempts.json"),
        ("true", "b-one-attempt.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--has-attempts",
                has_attempts,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--attempt-outcome",
            "failure",
            "--has-attempts",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--has-attempts",
            "false",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_attempt_presence_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--has-attempts",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_attempt_presence_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--has-attempts",
            "false",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_repository_path_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, title, repository_path, trust) in [
        ("a-case.json", "Case", "Second.rs", "reviewed"),
        ("b-exact.json", "Exact", "second.rs", "reviewed"),
        ("c-prefixed.json", "Prefixed", "./second.rs", "reviewed"),
        ("d-composed.json", "Composed", "second.rs", "curated"),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"Second\"", &format!("\"{title}\""))
                .replace(
                    "\"repository_path\":\"second.rs\"",
                    &format!("\"repository_path\":\"{repository_path}\""),
                )
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("repository-path record");
    }

    for (repository_path, expected_path) in [
        ("second.rs", "b-exact.json"),
        ("Second.rs", "a-case.json"),
        ("./second.rs", "c-prefixed.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--repository-path",
                repository_path,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--repository-path",
            "second.rs",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "d-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--repository-path",
            "missing.rs",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_repository_path_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--repository-path",
            "second.rs",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_provenance_url_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, title, provenance_url, trust) in [
        (
            "a-case.json",
            "Case",
            "https://example.com/Second",
            "reviewed",
        ),
        (
            "b-exact.json",
            "Exact",
            "https://example.com/second",
            "reviewed",
        ),
        (
            "c-trailing.json",
            "Trailing",
            "https://example.com/second/",
            "reviewed",
        ),
        (
            "d-whitespace.json",
            "Whitespace",
            "https://example.com/second ",
            "reviewed",
        ),
        (
            "e-composed.json",
            "Composed",
            "https://example.com/second",
            "curated",
        ),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"Second\"", &format!("\"{title}\""))
                .replace(
                    "\"url\":\"https://example.com/second\"",
                    &format!("\"url\":\"{provenance_url}\""),
                )
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("provenance-URL record");
    }

    for (provenance_url, expected_path) in [
        ("https://example.com/second", "b-exact.json"),
        ("https://example.com/Second", "a-case.json"),
        ("https://example.com/second/", "c-trailing.json"),
        ("https://example.com/second ", "d-whitespace.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--provenance-url",
                provenance_url,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--trust",
            "curated",
            "--provenance-url",
            "https://example.com/second",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "e-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "4",
            "--provenance-url",
            "https://example.com/missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_provenance_url_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--provenance-url",
            "https://example.com/second",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_task_title_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, title, trust) in [
        ("a-case.json", "exact", "reviewed"),
        ("b-exact.json", "Exact", "reviewed"),
        ("c-leading.json", " Exact", "reviewed"),
        ("d-trailing.json", "Exact ", "reviewed"),
        ("e-composed.json", "Exact", "curated"),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"Second\"", &format!("\"{title}\""))
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("task-title record");
    }

    for (task_title, expected_path) in [
        ("Exact", "b-exact.json"),
        ("exact", "a-case.json"),
        (" Exact", "c-leading.json"),
        ("Exact ", "d-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--task-title",
                task_title,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--trust",
            "curated",
            "--task-title",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "e-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--task-title",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_task_title_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--task-title",
            "Exact",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_task_objective_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, objective, trust) in [
        ("a-case.json", "exact", "reviewed"),
        ("b-exact.json", "Exact", "reviewed"),
        ("c-leading.json", " Exact", "reviewed"),
        ("d-trailing.json", "Exact ", "reviewed"),
        ("e-composed.json", "Exact", "curated"),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"Inspect second\"", &format!("\"{objective}\""))
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("task-objective record");
    }

    for (task_objective, expected_path) in [
        ("Exact", "b-exact.json"),
        ("exact", "a-case.json"),
        (" Exact", "c-leading.json"),
        ("Exact ", "d-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--task-objective",
                task_objective,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--trust",
            "curated",
            "--task-objective",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "e-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--task-objective",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_task_objective_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--task-objective",
            "Exact",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_acceptance_criterion_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, criteria, trust) in [
        ("a-case.json", "[\"exact\"]", "reviewed"),
        ("b-exact.json", "[\"other\",\"Exact\"]", "reviewed"),
        ("c-leading.json", "[\" Exact\"]", "reviewed"),
        ("d-trailing.json", "[\"Exact \"]", "reviewed"),
        ("e-composed.json", "[\"Exact\"]", "curated"),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("[\"passes\"]", criteria)
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("acceptance-criterion record");
    }

    for (criterion, expected_path) in [
        ("Exact", "b-exact.json"),
        ("exact", "a-case.json"),
        (" Exact", "c-leading.json"),
        ("Exact ", "d-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--acceptance-criterion",
                criterion,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--trust",
            "curated",
            "--acceptance-criterion",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "e-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--acceptance-criterion",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_acceptance_criterion_filter_still_rejects_malformed_records() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    fs::copy(source, corpus.path().join("a-match.json")).expect("early matching record");
    fs::write(corpus.path().join("z-malformed.json"), "{").expect("late malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--acceptance-criterion",
            "passes",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_attempt_summary_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, attempts, trust) in [
        ("a-empty.json", "[]", "reviewed"),
        (
            "b-case.json",
            r#"[{"summary":"exact","outcome":"success","diagnostic":null,"patch":"case.rs"}]"#,
            "reviewed",
        ),
        (
            "c-exact.json",
            r#"[{"summary":"Other","outcome":"success","diagnostic":null,"patch":"other.rs"},{"summary":"Exact","outcome":"success","diagnostic":null,"patch":"exact.rs"}]"#,
            "reviewed",
        ),
        (
            "d-leading.json",
            r#"[{"summary":" Exact","outcome":"success","diagnostic":null,"patch":"leading.rs"}]"#,
            "reviewed",
        ),
        (
            "e-trailing.json",
            r#"[{"summary":"Exact ","outcome":"success","diagnostic":null,"patch":"trailing.rs"}]"#,
            "reviewed",
        ),
        (
            "f-composed.json",
            r#"[{"summary":"Exact","outcome":"success","diagnostic":null,"patch":"composed.rs"}]"#,
            "curated",
        ),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"attempts\": []", &format!("\"attempts\": {attempts}"))
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("attempt-summary record");
    }

    for (attempt_summary, expected_path) in [
        ("Exact", "c-exact.json"),
        ("exact", "b-case.json"),
        (" Exact", "d-leading.json"),
        ("Exact ", "e-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--attempt-summary",
                attempt_summary,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "6",
            "--trust",
            "curated",
            "--attempt-summary",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "f-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "6",
            "--attempt-summary",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_attempt_summary_filter_still_rejects_malformed_records() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let matching = fs::read_to_string(source)
        .expect("record source")
        .replace(
            "\"attempts\": []",
            r#""attempts": [{"summary":"Exact","outcome":"success","diagnostic":null,"patch":"exact.rs"}]"#,
        );
    fs::write(corpus.path().join("a-match.json"), matching).expect("early matching record");
    fs::write(corpus.path().join("z-malformed.json"), "{").expect("late malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--attempt-summary",
            "Exact",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_outcome_summary_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, summary, trust) in [
        ("a-case.json", "exact", "reviewed"),
        ("b-exact.json", "Exact", "reviewed"),
        ("c-leading.json", " Exact", "reviewed"),
        ("d-trailing.json", "Exact ", "reviewed"),
        ("e-composed.json", "Exact", "curated"),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace(
                    "\"summary\":\"Done\"",
                    &format!("\"summary\":\"{summary}\""),
                )
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("outcome-summary record");
    }
    fs::write(
        corpus.path().join("a-attempt-collision.json"),
        source_record
            .replace("\"summary\":\"Done\"", "\"summary\":\"Other\"")
            .replace(
                "\"attempts\": []",
                r#""attempts": [{"summary":"Exact","outcome":"success","diagnostic":null,"patch":"attempt.rs"}]"#,
            ),
    )
    .expect("attempt-summary collision record");

    for (outcome_summary, expected_path) in [
        ("Exact", "b-exact.json"),
        ("exact", "a-case.json"),
        (" Exact", "c-leading.json"),
        ("Exact ", "d-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--outcome-summary",
                outcome_summary,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--trust",
            "curated",
            "--outcome-summary",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "e-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "5",
            "--outcome-summary",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_outcome_summary_filter_still_rejects_malformed_records() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let matching = fs::read_to_string(source)
        .expect("record source")
        .replace("\"summary\":\"Done\"", "\"summary\":\"Exact\"");
    fs::write(corpus.path().join("a-match.json"), matching).expect("early matching record");
    fs::write(corpus.path().join("z-malformed.json"), "{").expect("late malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--outcome-summary",
            "Exact",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_verification_command_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, verification, trust) in [
        ("a-empty.json", "[]", "reviewed"),
        (
            "b-case.json",
            r#"[{"command":"exact","status":"passed"}]"#,
            "reviewed",
        ),
        (
            "c-exact.json",
            r#"[{"command":"other","status":"passed"},{"command":"Exact","status":"passed"}]"#,
            "reviewed",
        ),
        (
            "d-leading.json",
            r#"[{"command":" Exact","status":"passed"}]"#,
            "reviewed",
        ),
        (
            "e-trailing.json",
            r#"[{"command":"Exact ","status":"passed"}]"#,
            "reviewed",
        ),
        (
            "f-composed.json",
            r#"[{"command":"Exact","status":"passed"}]"#,
            "curated",
        ),
    ] {
        let record = source_record
            .replace(r#"[{"command":"test","status":"passed"}]"#, verification)
            .replace("\"reviewed\"", &format!("\"{trust}\""));
        let record = if verification == "[]" {
            record.replace("\"verified\":true", "\"verified\":false")
        } else {
            record
        };
        fs::write(corpus.path().join(name), record).expect("verification-command record");
    }

    for (verification_command, expected_path) in [
        ("Exact", "c-exact.json"),
        ("exact", "b-case.json"),
        (" Exact", "d-leading.json"),
        ("Exact ", "e-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--verification-command",
                verification_command,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "6",
            "--trust",
            "curated",
            "--verification-command",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "f-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "6",
            "--verification-command",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_verification_command_filter_still_rejects_malformed_records() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let matching = fs::read_to_string(source)
        .expect("record source")
        .replace("\"command\":\"test\"", "\"command\":\"Exact\"");
    fs::write(corpus.path().join("a-match.json"), matching).expect("early matching record");
    fs::write(corpus.path().join("z-malformed.json"), "{").expect("late malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--verification-command",
            "Exact",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_attempt_diagnostic_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, attempts, trust) in [
        (
            "a-collision.json",
            r#"[{"summary":"Exact","outcome":"failure","diagnostic":"Other","patch":"Exact"}]"#,
            "reviewed",
        ),
        ("a-empty.json", "[]", "reviewed"),
        (
            "b-null.json",
            r#"[{"summary":"none","outcome":"success","diagnostic":null,"patch":"a.rs"}]"#,
            "reviewed",
        ),
        (
            "c-case.json",
            r#"[{"summary":"case","outcome":"failure","diagnostic":"exact","patch":"b.rs"}]"#,
            "reviewed",
        ),
        (
            "d-exact.json",
            r#"[{"summary":"none","outcome":"success","diagnostic":null,"patch":"c.rs"},{"summary":"match","outcome":"failure","diagnostic":"Exact","patch":"c.rs"}]"#,
            "reviewed",
        ),
        (
            "e-leading.json",
            r#"[{"summary":"leading","outcome":"failure","diagnostic":" Exact","patch":"d.rs"}]"#,
            "reviewed",
        ),
        (
            "f-trailing.json",
            r#"[{"summary":"trailing","outcome":"failure","diagnostic":"Exact ","patch":"e.rs"}]"#,
            "reviewed",
        ),
        (
            "g-composed.json",
            r#"[{"summary":"composed","outcome":"failure","diagnostic":"Exact","patch":"f.rs"}]"#,
            "curated",
        ),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"attempts\": []", &format!("\"attempts\": {attempts}"))
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("attempt-diagnostic record");
    }

    for (attempt_diagnostic, expected_path) in [
        ("Exact", "d-exact.json"),
        ("exact", "c-case.json"),
        (" Exact", "e-leading.json"),
        ("Exact ", "f-trailing.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--attempt-diagnostic",
                attempt_diagnostic,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "7",
            "--trust",
            "curated",
            "--attempt-diagnostic",
            "Exact",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "g-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "7",
            "--attempt-diagnostic",
            "Missing",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_attempt_diagnostic_filter_still_rejects_malformed_records() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let matching = fs::read_to_string(source)
        .expect("record source")
        .replace(
            "\"attempts\": []",
            r#""attempts": [{"summary":"match","outcome":"failure","diagnostic":"Exact","patch":"a.rs"}]"#,
        );
    fs::write(corpus.path().join("a-match.json"), matching).expect("early matching record");
    fs::write(corpus.path().join("z-malformed.json"), "{").expect("late malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--attempt-diagnostic",
            "Exact",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_exact_attempt_patch_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    for (name, attempts, trust) in [
        ("a-empty.json", "[]", "reviewed"),
        (
            "b-collision.json",
            r#"[{"summary":"Exact.rs","outcome":"failure","diagnostic":"Exact.rs","patch":"other.rs"}]"#,
            "reviewed",
        ),
        (
            "c-case.json",
            r#"[{"summary":"case","outcome":"success","diagnostic":null,"patch":"exact.rs"}]"#,
            "reviewed",
        ),
        (
            "d-exact.json",
            r#"[{"summary":"other","outcome":"success","diagnostic":null,"patch":"other.rs"},{"summary":"match","outcome":"success","diagnostic":null,"patch":"Exact.rs"}]"#,
            "reviewed",
        ),
        (
            "e-leading.json",
            r#"[{"summary":"leading","outcome":"success","diagnostic":null,"patch":" Exact.rs"}]"#,
            "reviewed",
        ),
        (
            "f-composed.json",
            r#"[{"summary":"composed","outcome":"success","diagnostic":null,"patch":"Exact.rs"}]"#,
            "curated",
        ),
    ] {
        fs::write(
            corpus.path().join(name),
            source_record
                .replace("\"attempts\": []", &format!("\"attempts\": {attempts}"))
                .replace("\"reviewed\"", &format!("\"{trust}\"")),
        )
        .expect("attempt-patch record");
    }

    for (attempt_patch, expected_path) in [
        ("Exact.rs", "d-exact.json"),
        ("exact.rs", "c-case.json"),
        (" Exact.rs", "e-leading.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--attempt-patch",
                attempt_patch,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "6",
            "--trust",
            "curated",
            "--attempt-patch",
            "Exact.rs",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "f-composed.json");
}

#[test]
fn corpus_sampling_attempt_patch_filter_still_rejects_malformed_records() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let matching = fs::read_to_string(source)
        .expect("record source")
        .replace(
            "\"attempts\": []",
            r#""attempts": [{"summary":"match","outcome":"success","diagnostic":null,"patch":"Exact.rs"}]"#,
        );
    fs::write(corpus.path().join("a-match.json"), matching).expect("early matching record");
    fs::write(corpus.path().join("z-malformed.json"), "{").expect("late malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--attempt-patch",
            "Exact.rs",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_verification_presence_before_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-no-verification.json"),
        source_record
            .replace("\"Second\"", "\"No verification\"")
            .replace(
                "\"verified\":true,\"verification\":[{\"command\":\"test\",\"status\":\"passed\"}]",
                "\"verified\":false,\"verification\":[]",
            ),
    )
    .expect("record without verification checks");
    fs::write(
        corpus.path().join("b-verification.json"),
        source_record.replace("\"Second\"", "\"Verification\""),
    )
    .expect("record with a verification check");
    fs::write(
        corpus.path().join("c-composed.json"),
        source_record
            .replace("\"Second\"", "\"Composed\"")
            .replace("\"reviewed\"", "\"curated\""),
    )
    .expect("record matching composed filters");

    for (has_verification, expected_path) in [
        ("false", "a-no-verification.json"),
        ("true", "b-verification.json"),
    ] {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--has-verification",
                has_verification,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--verified",
            "true",
            "--has-verification",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--has-verification",
            "false",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_verification_presence_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--has-verification",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_verification_presence_filter_still_rejects_malformed_records() {
    let corpus = tempdir().expect("sample corpus");
    fs::write(corpus.path().join("malformed.json"), "{").expect("malformed record");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
            "--has-verification",
            "false",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("malformed_record"));
}

#[test]
fn corpus_sampling_filters_by_lesson_presence_after_filtering_and_limiting() {
    let source = format!(
        "{}/tests/fixtures/corpus/valid/second.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let corpus = tempdir().expect("sample corpus");
    let source_record = fs::read_to_string(source).expect("record source");
    fs::write(
        corpus.path().join("a-no-lessons.json"),
        source_record.replace("\"Second\"", "\"No lessons\""),
    )
    .expect("record without lessons");
    fs::write(
        corpus.path().join("b-lessons.json"),
        source_record
            .replace("\"Second\"", "\"Lessons\"")
            .replace("\"lessons\": []", "\"lessons\": [\"reuse exact evidence\"]"),
    )
    .expect("record with lessons");
    let composed = source_record
        .replace("\"Second\"", "\"Composed\"")
        .replace("\"reviewed\"", "\"curated\"")
        .replace(
            "\"attempts\": []",
            "\"attempts\": [{\"summary\":\"regression\",\"outcome\":\"failure\",\"diagnostic\":\"failed check\",\"patch\":\"second.rs\"}]",
        )
        .replace(
            "\"type\":\"positive\",\"rejection_rationale\":null",
            "\"type\":\"negative\",\"rejection_rationale\":\"rejected example\"",
        )
        .replace(
            "\"passed\":true,\"blockers\":[]",
            "\"passed\":false,\"blockers\":[\"review finding\"]",
        )
        .replace("\"lessons\": []", "\"lessons\": [\"correct the boundary\"]");
    fs::write(corpus.path().join("c-composed.json"), composed)
        .expect("record matching every filter");

    for (has_lessons, expected_path) in [("false", "a-no-lessons.json"), ("true", "b-lessons.json")]
    {
        let assertion = Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "corpus",
                "sample",
                corpus.path().to_str().expect("UTF-8 corpus path"),
                "1",
                "--has-lessons",
                has_lessons,
            ])
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
        let sample: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
        assert_eq!(sample.as_array().expect("sample array").len(), 1);
        assert_eq!(sample[0]["path"], expected_path);
    }

    let assertion = Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--example",
            "negative",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "passed",
            "--verified",
            "true",
            "--sanitation-passed",
            "false",
            "--has-sanitation-blockers",
            "true",
            "--has-lessons",
            "true",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let sample: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("sample JSON");
    assert_eq!(sample.as_array().expect("sample array").len(), 1);
    assert_eq!(sample[0]["path"], "c-composed.json");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "3",
            "--trust",
            "curated",
            "--has-lessons",
            "false",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn corpus_sampling_rejects_non_boolean_lesson_value_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--has-lessons",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_rejects_non_boolean_sanitation_value_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--sanitation-passed",
            "yes",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_rejects_unknown_attempt_outcomes_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--attempt-outcome",
            "partial",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_rejects_unknown_example_types_before_storage_access() {
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            "tests/fixtures/missing-corpus",
            "1",
            "--example",
            "mixed",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("unreadable_corpus").not());
}

#[test]
fn corpus_sampling_rejects_out_of_range_limits_before_storage_access() {
    for limit in ["0", "1025"] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(["corpus", "sample", "tests/fixtures/missing-corpus", limit])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("invalid value"))
            .stderr(predicate::str::contains("unreadable_corpus").not());
    }
}

#[test]
fn corpus_sampling_rejects_invalid_records_without_partial_output() {
    let corpus = format!(
        "{}/tests/fixtures/corpus/invalid",
        env!("CARGO_MANIFEST_DIR")
    );

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            &corpus,
            "2",
            "--attempt-outcome",
            "failure",
            "--verification-status",
            "failed",
            "--verified",
            "false",
            "--sanitation-passed",
            "false",
            "--has-sanitation-blockers",
            "true",
            "--has-lessons",
            "true",
            "--has-attempt-diagnostic",
            "true",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "\"malformed.json\": malformed_record",
        ))
        .stderr(predicate::str::contains(
            "\"semantic.json\": task.title: required",
        ));
}

#[cfg(unix)]
#[test]
fn corpus_sampling_ignores_non_regular_json_entries_without_blocking() {
    use rustix::fs::{Mode, mkfifoat};
    use std::os::unix::fs::symlink;

    let corpus = tempdir().expect("sample corpus");
    mkfifoat(
        rustix::fs::CWD,
        corpus.path().join("blocked.json"),
        Mode::RUSR | Mode::WUSR,
    )
    .expect("JSON FIFO");
    symlink("blocked.json", corpus.path().join("linked.json")).expect("JSON symlink");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "corpus",
            "sample",
            corpus.path().to_str().expect("UTF-8 corpus path"),
            "1",
        ])
        .assert()
        .success()
        .stdout("[]\n")
        .stderr(predicate::str::is_empty());
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
            "\"alpha.skill\"\tskill\t\"SKILL.org\"\t\"alpha/extension.yaml\"\n\
             \"beta.workflow\"\tworkflow\t\"WORKFLOW.org\"\t\"beta/extension.yaml\"\n\
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
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,\"status\":\"active\",",
            "\"final_occurrence_unix_millis\":20,\"definition_revision\":1,",
            "\"aggregate_revision\":1,\"cancellation\":null}\n"
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
fn claims_exact_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
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
    store.persist_occurrence(&id, 1, 1).unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "claim",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            "1",
            "15",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":1,\"unix_millis\":15,\"definition_revision\":1,",
            "\"occurrence_revision\":2}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    let claimed = store
        .load_claimed_occurrence(&id, 1)
        .unwrap()
        .expect("durable claimed occurrence");
    assert_eq!(claimed.revision(), 2);
}

#[test]
fn recurrence_occurrence_claim_validates_and_fails_closed() {
    let directory = tempdir().expect("recurrence database directory");
    let missing_database = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "claim",
            missing_database.to_str().unwrap(),
            "",
            "0",
            "1",
            "10",
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
            "claim",
            missing_database.to_str().unwrap(),
            "valid",
            "0",
            "1",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_claim_failed:",
        ));

    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
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
    store.persist_occurrence(&id, 1, 1).unwrap();
    store
        .materialize_occurrence(&id, 1, 1, TaskId::new("task").unwrap())
        .unwrap();
    drop(store);

    let claim = |offset: &str, revision: &str, cutoff: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "claim",
            database.to_str().unwrap(),
            id.as_str(),
            offset,
            revision,
            cutoff,
        ]);
        command
    };
    for (offset, revision, cutoff) in [
        ("2", "1", "100"),
        ("0", "0", "100"),
        ("0", "1", "9"),
        ("1", "2", "100"),
    ] {
        claim(offset, revision, cutoff)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_claim_failed:",
            ));
    }

    claim("0", "1", "10").assert().success();
    claim("0", "2", "10")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_claim_failed:",
        ));

    let mut store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    store
        .release_occurrence(
            &id,
            0,
            2,
            RecurrenceOccurrenceRelease::new("retry elsewhere").unwrap(),
        )
        .unwrap();
    drop(store);
    claim("0", "2", "10")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_claim_failed:",
        ));
    claim("0", "3", "10")
        .assert()
        .success()
        .stdout(predicate::str::ends_with("\"occurrence_revision\":4}\n"));

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    let claimed = store
        .load_claimed_occurrence(&id, 0)
        .unwrap()
        .expect("released occurrence reclaimed at its exact revision");
    assert_eq!(claimed.revision(), 4);
}

#[test]
fn releases_exact_recurrence_occurrence_as_deterministic_json_and_allows_reclaim() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "release",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            "2",
            "retry \"elsewhere\"\nnow",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":1,\"unix_millis\":15,\"definition_revision\":1,",
            "\"occurrence_revision\":3,\"latest_release\":\"retry \\\"elsewhere\\\"\\nnow\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    let mut store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    let released = store
        .load_released_occurrence(&id, 1)
        .unwrap()
        .expect("durable released occurrence");
    assert_eq!(released.revision(), 3);
    assert_eq!(
        released.latest_release().as_str(),
        "retry \"elsewhere\"\nnow"
    );
    store
        .claim_occurrence(&id, 1, 3, ScheduleInstant::from_unix_millis(15))
        .expect("released occurrence can be reclaimed at exact revision");
}

#[test]
fn recurrence_occurrence_release_validates_and_fails_closed() {
    let directory = tempdir().expect("recurrence database directory");
    let missing_database = directory.path().join("missing.sqlite3");

    for (id, reason, category) in [
        ("", "reason", "invalid_recurrence_id"),
        ("valid", " \n\t", "invalid_recurrence_occurrence_release"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "release",
                missing_database.to_str().unwrap(),
                id,
                "0",
                "2",
                reason,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {category}:")));
        assert!(!missing_database.exists());
    }

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
    store.persist_occurrence(&id, 1, 1).unwrap();
    store.persist_occurrence(&id, 1, 2).unwrap();
    store
        .claim_occurrence(&id, 2, 1, ScheduleInstant::from_unix_millis(20))
        .unwrap();
    store.persist_occurrence(&id, 1, 3).unwrap();
    let task_id = TaskId::new("task").unwrap();
    store
        .materialize_occurrence(&id, 3, 1, task_id.clone())
        .unwrap();
    drop(store);

    let release = |offset: &str, revision: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "release",
            database.to_str().unwrap(),
            id.as_str(),
            offset,
            revision,
            "recovery",
        ]);
        command
    };
    for (offset, revision) in [("0", "1"), ("1", "1"), ("2", "1"), ("3", "2")] {
        release(offset, revision)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_release_failed:",
            ));
    }

    let mut store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    assert!(store.load_occurrence(&id, 0).unwrap().is_none());
    let claimed = store
        .load_claimed_occurrence(&id, 2)
        .unwrap()
        .expect("claimed occurrence remains claimed");
    assert_eq!(claimed.revision(), 2);
    let materialized = store
        .load_materialized_occurrence(&id, 3)
        .unwrap()
        .expect("materialized occurrence remains materialized");
    assert_eq!(materialized.revision(), 2);
    assert_eq!(materialized.task_id(), &task_id);
    let available = store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .expect("available occurrence remains available at revision one");
    assert_eq!(available.revision(), 2);
}

#[test]
fn persists_due_recurrence_pages_atomically_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("due\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"due\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    drop(store);

    let command = |start: &str, size: &str, cutoff: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "persist-due",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            start,
            size,
            cutoff,
        ]);
        command
    };

    command("0", "2", "100")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"due\\nid\",\"goal\":\"preserve \\\"due\\\" goal\",\"offset\":0,\"unix_millis\":10,\"definition_revision\":1},",
            "{\"recurrence_id\":\"due\\nid\",\"goal\":\"preserve \\\"due\\\" goal\",\"offset\":1,\"unix_millis\":15,\"definition_revision\":1}",
            "],\"next_offset\":2}\n"
        ))
        .stderr(predicate::str::is_empty());
    command("2", "10", "20")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"due\\nid\",\"goal\":\"preserve \\\"due\\\" goal\",\"offset\":2,\"unix_millis\":20,\"definition_revision\":1}",
            "],\"next_offset\":3}\n"
        ));
    command("3", "10", "20")
        .assert()
        .success()
        .stdout("{\"occurrences\":[],\"next_offset\":3}\n");
    command("3", "10", "30")
        .assert()
        .success()
        .stdout(predicate::str::ends_with(
            "\"offset\":4,\"unix_millis\":30,\"definition_revision\":1}],\"next_offset\":null}\n",
        ));

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    for offset in 0..5 {
        assert!(store.load_occurrence(&id, offset).unwrap().is_some());
    }
}

#[test]
fn due_recurrence_page_persistence_validates_before_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("missing.sqlite3");

    for (id, page_size, diagnostic) in [
        ("", "1", "invalid_recurrence_id"),
        ("valid", "0", "invalid_occurrence_page_size"),
        ("valid", "1025", "invalid_occurrence_page_size"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "persist-due",
                database.to_str().expect("UTF-8 database path"),
                id,
                "1",
                "0",
                page_size,
                "10",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {diagnostic}:")));
        assert!(!database.exists());
    }
}

#[test]
fn due_recurrence_page_persistence_fails_without_partial_provenance() {
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
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    drop(store);

    let run = |id: &str, revision: &str, start: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "persist-due",
            database.to_str().expect("UTF-8 database path"),
            id,
            revision,
            start,
            "3",
            "100",
        ]);
        command
    };
    for (selected_id, revision, start) in [
        ("absent", "1", "0"),
        ("exact", "2", "0"),
        ("exact", "1", "3"),
    ] {
        run(selected_id, revision, start)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: due_recurrence_occurrence_persistence_failed:",
            ));
    }
    run("exact", "1", "0")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: due_recurrence_occurrence_persistence_failed:",
        ));

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    assert!(store.load_occurrence(&id, 0).unwrap().is_none());
    assert!(store.load_occurrence(&id, 1).unwrap().is_some());
    assert!(store.load_occurrence(&id, 2).unwrap().is_none());
    drop(store);

    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    run("exact", "1", "0")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: due_recurrence_occurrence_persistence_failed:",
        ));
    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    assert!(store.load_occurrence(&id, 0).unwrap().is_none());
    assert!(store.load_occurrence(&id, 2).unwrap().is_none());
}

#[test]
fn persists_due_recurrence_page_at_maximum_instant() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("boundary").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persist-due",
            database.to_str().expect("UTF-8 database path"),
            "boundary",
            "1",
            "0",
            "2",
            &u64::MAX.to_string(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::ends_with(format!(
            "\"offset\":1,\"unix_millis\":{},\"definition_revision\":1}}],\"next_offset\":null}}\n",
            u64::MAX
        )));
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
fn materializes_claimed_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("claimed\nid").unwrap();
    let task_id = TaskId::new("claimed\ntask").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"claimed\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 2).unwrap();
    store
        .claim_occurrence(&id, 2, 1, ScheduleInstant::from_unix_millis(20))
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize-claimed",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "2",
            "2",
            task_id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"claimed\\nid\",\"goal\":\"preserve \\\"claimed\\\" goal\",",
            "\"offset\":2,\"unix_millis\":20,\"definition_revision\":1,",
            "\"occurrence_revision\":3,\"task_id\":\"claimed\\ntask\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    assert!(store.load_claimed_occurrence(&id, 2).unwrap().is_none());
    let materialized = store
        .load_materialized_occurrence(&id, 2)
        .unwrap()
        .expect("materialized occurrence");
    assert_eq!(materialized.revision(), 3);
    assert_eq!(materialized.task_id(), &task_id);
    assert_eq!(
        store
            .find_materialized_by_task_id(&task_id)
            .unwrap()
            .expect("reverse task binding")
            .task_id(),
        &task_id
    );
}

#[test]
fn claimed_recurrence_materialization_validates_ids_before_storage_access() {
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
                "materialize-claimed",
                database.to_str().expect("UTF-8 database path"),
                id,
                "0",
                "2",
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }
}

#[test]
fn claimed_recurrence_materialization_fails_closed_for_invalid_state() {
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
            OccurrenceCount::new(6).unwrap(),
        )
        .unwrap();
    for offset in 0..=4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .release_occurrence(
            &id,
            1,
            2,
            RecurrenceOccurrenceRelease::new("recovered").unwrap(),
        )
        .unwrap();
    for offset in 2..=4 {
        store
            .claim_occurrence(&id, offset, 1, ScheduleInstant::from_unix_millis(30))
            .unwrap();
    }
    drop(store);
    let occupied = TaskId::new("occupied").unwrap();
    TaskStore::open(&database)
        .unwrap()
        .start(occupied.clone(), TaskGoal::new("occupied goal").unwrap())
        .unwrap();

    let assert_failure = |offset: &str, revision: &str, task_id: &str| {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize-claimed",
                database.to_str().expect("UTF-8 database path"),
                id.as_str(),
                offset,
                revision,
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_claimed_occurrence_materialization_failed:",
            ));
    };

    assert_failure("5", "1", "missing");
    assert_failure("0", "1", "available");
    assert_failure("1", "3", "released");
    assert_failure("2", "1", "stale");
    assert_failure("4", "2", occupied.as_str());

    let first_task = TaskId::new("first-binding").unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize-claimed",
            database.to_str().unwrap(),
            id.as_str(),
            "3",
            "2",
            first_task.as_str(),
        ])
        .assert()
        .success();
    assert_failure("3", "3", "replacement");

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    assert!(store.load_claimed_occurrence(&id, 2).unwrap().is_some());
    assert!(store.load_claimed_occurrence(&id, 4).unwrap().is_some());
    let tasks = TaskStore::open(&database).expect("reopened task store");
    for absent in ["missing", "available", "released", "stale", "replacement"] {
        assert!(tasks.load(&TaskId::new(absent).unwrap()).unwrap().is_none());
    }
    assert!(tasks.load(&first_task).unwrap().is_some());
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
fn pages_current_claimed_recurrence_occurrences_as_deterministic_json() {
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
    for offset in 1..5 {
        store.persist_occurrence(&id, 1, offset).unwrap();
        store
            .claim_occurrence(&id, offset, 1, ScheduleInstant::from_unix_millis(30))
            .unwrap();
    }
    store
        .release_occurrence(
            &id,
            2,
            2,
            vela_kernel::scheduler::RecurrenceOccurrenceRelease::new("retry later").unwrap(),
        )
        .unwrap();
    store
        .materialize_claimed_occurrence(&id, 3, 2, TaskId::new("task-three").unwrap())
        .unwrap();
    drop(store);

    for (start, size, expected) in [
        (
            "0",
            "3",
            concat!(
                "{\"occurrences\":[{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":1,",
                "\"unix_millis\":15,\"definition_revision\":1,",
                "\"occurrence_revision\":2}],\"next_offset\":3}\n"
            ),
        ),
        ("2", "1", "{\"occurrences\":[],\"next_offset\":3}\n"),
        (
            "3",
            "2",
            concat!(
                "{\"occurrences\":[{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":4,",
                "\"unix_millis\":30,\"definition_revision\":1,",
                "\"occurrence_revision\":2}],\"next_offset\":null}\n"
            ),
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "claimed",
                database.to_str().unwrap(),
                id.as_str(),
                start,
                size,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn claimed_recurrence_occurrence_paging_validates_before_storage_access() {
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
                "claimed",
                database.to_str().unwrap(),
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
fn claimed_recurrence_occurrence_paging_categorizes_bounds_and_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "claimed",
            missing.to_str().unwrap(),
            "exact",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: claimed_recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    for offset in 0..4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
        store
            .claim_occurrence(&id, offset, 1, ScheduleInstant::from_unix_millis(20))
            .unwrap();
    }
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
    store
        .claim_occurrence(&unrelated, 0, 1, ScheduleInstant::from_unix_millis(1))
        .unwrap();
    drop(store);

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "4", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "claimed",
                database.to_str().unwrap(),
                id,
                start,
                "1",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }

    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload_version = 2
                 WHERE event_type = 'recurrence.occurrence_claimed'
                   AND stream_id IN (
                     SELECT stream_id FROM events
                     WHERE event_type = 'recurrence.occurrence_persisted'
                       AND (json_extract(CAST(payload AS TEXT), '$.offset') = 3
                            OR json_extract(CAST(payload AS TEXT), '$.recurrence_id') = 'unrelated')
                   )",
                [],
            )
            .unwrap(),
        2
    );
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "claimed",
            database.to_str().unwrap(),
            "exact",
            "0",
            "3",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"next_offset\":3"))
        .stderr(predicate::str::is_empty());
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "claimed",
            database.to_str().unwrap(),
            "exact",
            "3",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: claimed_recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn pages_current_available_recurrence_occurrences_as_deterministic_json() {
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
    for offset in 0..5 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    for offset in 1..5 {
        store
            .claim_occurrence(&id, offset, 1, ScheduleInstant::from_unix_millis(30))
            .unwrap();
    }
    store
        .release_occurrence(
            &id,
            2,
            2,
            vela_kernel::scheduler::RecurrenceOccurrenceRelease::new("retry later").unwrap(),
        )
        .unwrap();
    store
        .materialize_claimed_occurrence(&id, 3, 2, TaskId::new("task-three").unwrap())
        .unwrap();
    let released = store
        .release_occurrence(
            &id,
            4,
            2,
            vela_kernel::scheduler::RecurrenceOccurrenceRelease::new("first reason").unwrap(),
        )
        .unwrap();
    store
        .claim_occurrence(
            &id,
            4,
            released.revision(),
            ScheduleInstant::from_unix_millis(30),
        )
        .unwrap();
    store
        .release_occurrence(
            &id,
            4,
            4,
            vela_kernel::scheduler::RecurrenceOccurrenceRelease::new("retry\nagain").unwrap(),
        )
        .unwrap();
    drop(store);

    for (start, size, expected) in [
        (
            "0",
            "3",
            concat!(
                "{\"occurrences\":[{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":0,",
                "\"unix_millis\":10,\"definition_revision\":1,",
                "\"occurrence_revision\":1,\"latest_release\":null},",
                "{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":2,",
                "\"unix_millis\":20,\"definition_revision\":1,",
                "\"occurrence_revision\":3,\"latest_release\":\"retry later\"}],",
                "\"next_offset\":3}\n"
            ),
        ),
        ("1", "1", "{\"occurrences\":[],\"next_offset\":2}\n"),
        (
            "3",
            "2",
            concat!(
                "{\"occurrences\":[{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":4,",
                "\"unix_millis\":30,\"definition_revision\":1,",
                "\"occurrence_revision\":5,\"latest_release\":\"retry\\nagain\"}],",
                "\"next_offset\":null}\n"
            ),
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "available",
                database.to_str().unwrap(),
                id.as_str(),
                start,
                size,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn available_recurrence_occurrence_paging_validates_before_storage_access() {
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
                "available",
                database.to_str().unwrap(),
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
fn available_recurrence_occurrence_paging_categorizes_bounds_and_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "available",
            missing.to_str().unwrap(),
            "exact",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: available_recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    for offset in 0..4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
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

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "4", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "available",
                database.to_str().unwrap(),
                id,
                start,
                "1",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }

    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload_version = 2
                 WHERE event_type = 'recurrence.occurrence_persisted'
                   AND (json_extract(CAST(payload AS TEXT), '$.offset') = 3
                        OR json_extract(CAST(payload AS TEXT), '$.recurrence_id') = 'unrelated')",
                [],
            )
            .unwrap(),
        2
    );
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "available",
            database.to_str().unwrap(),
            "exact",
            "0",
            "3",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"next_offset\":3"))
        .stderr(predicate::str::is_empty());
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "available",
            database.to_str().unwrap(),
            "exact",
            "3",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: available_recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn pages_sparse_materialized_recurrence_occurrences_as_deterministic_json() {
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
    for offset in [1, 2, 3] {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    store
        .materialize_occurrence(&id, 1, 1, TaskId::new("task\none").unwrap())
        .unwrap();
    store
        .materialize_occurrence(&id, 3, 1, TaskId::new("task-three").unwrap())
        .unwrap();
    drop(store);

    for (start, size, expected) in [
        (
            "0",
            "3",
            concat!(
                "{\"occurrences\":[{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":1,",
                "\"unix_millis\":15,\"definition_revision\":1,",
                "\"occurrence_revision\":2,\"task_id\":\"task\\none\"}],",
                "\"next_offset\":3}\n"
            ),
        ),
        ("2", "1", "{\"occurrences\":[],\"next_offset\":3}\n"),
        (
            "3",
            "2",
            concat!(
                "{\"occurrences\":[{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":3,",
                "\"unix_millis\":25,\"definition_revision\":1,",
                "\"occurrence_revision\":2,\"task_id\":\"task-three\"}],",
                "\"next_offset\":null}\n"
            ),
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialized",
                database.to_str().expect("UTF-8 database path"),
                "exact\nid",
                start,
                size,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn materialized_recurrence_occurrence_paging_validates_before_storage_access() {
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
                "materialized",
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
fn materialized_recurrence_occurrence_paging_categorizes_bounds_and_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized",
            missing.to_str().unwrap(),
            "exact",
            "0",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: materialized_recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let id = RecurrenceId::new("exact").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    for offset in 0..4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
        store
            .materialize_occurrence(
                &id,
                offset,
                1,
                TaskId::new(format!("task-{offset}")).unwrap(),
            )
            .unwrap();
    }
    let unrelated = RecurrenceId::new("unrelated").unwrap();
    store
        .create(
            unrelated.clone(),
            TaskGoal::new("unrelated goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&unrelated, 1, 0).unwrap();
    store
        .materialize_occurrence(&unrelated, 0, 1, TaskId::new("unrelated-task").unwrap())
        .unwrap();
    drop(store);

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "4", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialized",
                database.to_str().unwrap(),
                id,
                start,
                "1",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }

    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload_version = 2
                 WHERE event_type = 'recurrence.occurrence_materialized'
                   AND json_extract(CAST(payload AS TEXT), '$.task_id')
                       IN ('task-3', 'unrelated-task')",
                [],
            )
            .unwrap(),
        2
    );
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized",
            database.to_str().unwrap(),
            "exact",
            "0",
            "3",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":0,",
            "\"unix_millis\":10,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task-0\"},",
            "{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":1,",
            "\"unix_millis\":11,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task-1\"},",
            "{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":2,",
            "\"unix_millis\":12,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task-2\"}],",
            "\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized",
            database.to_str().unwrap(),
            "exact",
            "3",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: materialized_recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn pages_global_materialized_recurrence_bindings_with_a_round_trip_cursor() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let short_id = RecurrenceId::new("a").unwrap();
    let separator_id = RecurrenceId::new("bbb:scope").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in [&short_id, &separator_id] {
        store
            .create(
                id.clone(),
                TaskGoal::new(format!("Audit binding {id}")).unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(11).unwrap(),
            )
            .unwrap();
    }
    for (id, offset, task_id) in [
        (&short_id, 2, "short-two"),
        (&short_id, 10, "short-ten"),
        (&separator_id, 0, "separator-zero"),
    ] {
        store.persist_occurrence(id, 1, offset).unwrap();
        store
            .materialize_occurrence(id, offset, 1, TaskId::new(task_id).unwrap())
            .unwrap();
    }
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized-page",
            database.to_str().unwrap(),
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"a\",\"goal\":\"Audit binding a\",\"offset\":10,",
            "\"unix_millis\":11,\"definition_revision\":1,\"occurrence_revision\":2,",
            "\"task_id\":\"short-ten\"},",
            "{\"recurrence_id\":\"a\",\"goal\":\"Audit binding a\",\"offset\":2,",
            "\"unix_millis\":3,\"definition_revision\":1,\"occurrence_revision\":2,",
            "\"task_id\":\"short-two\"}],",
            "\"next_after\":{\"recurrence_id\":\"a\",\"offset\":2}}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized-page",
            database.to_str().unwrap(),
            "2",
            "--after-recurrence-id",
            "a",
            "--after-offset",
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[{\"recurrence_id\":\"bbb:scope\",",
            "\"goal\":\"Audit binding bbb:scope\",\"offset\":0,\"unix_millis\":1,",
            "\"definition_revision\":1,\"occurrence_revision\":2,",
            "\"task_id\":\"separator-zero\"}],\"next_after\":null}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn global_materialized_recurrence_paging_validates_the_complete_cursor_before_storage() {
    let directory = tempdir().expect("recurrence database directory");
    for (name, extra, expected_error) in [
        ("zero.sqlite3", vec!["0"], "invalid_occurrence_page_size"),
        (
            "partial-id.sqlite3",
            vec!["1", "--after-recurrence-id", "valid"],
            "invalid_materialized_recurrence_occurrence_cursor",
        ),
        (
            "partial-offset.sqlite3",
            vec!["1", "--after-offset", "0"],
            "invalid_materialized_recurrence_occurrence_cursor",
        ),
        (
            "invalid-id.sqlite3",
            vec!["1", "--after-recurrence-id", "", "--after-offset", "0"],
            "invalid_recurrence_id",
        ),
    ] {
        let database = directory.path().join(name);
        let mut args = vec![
            "recurrence",
            "materialized-page",
            database.to_str().expect("UTF-8 database path"),
        ];
        args.extend(extra);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(args)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }
}

#[test]
fn global_materialized_recurrence_paging_fails_closed_on_lookahead_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    for offset in 0..3 {
        store.persist_occurrence(&id, 1, offset).unwrap();
        store
            .materialize_occurrence(
                &id,
                offset,
                1,
                TaskId::new(format!("task-{offset}")).unwrap(),
            )
            .unwrap();
    }
    drop(store);
    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload_version = 2
                 WHERE event_type = 'recurrence.occurrence_materialized'
                   AND json_extract(CAST(payload AS TEXT), '$.task_id') = 'task-1'",
                [],
            )
            .unwrap(),
        1
    );

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized-page",
            database.to_str().unwrap(),
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: global_materialized_recurrence_occurrence_lookup_failed:",
        ));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialized-page",
            database.to_str().unwrap(),
            "1",
            "--after-recurrence-id",
            "exact",
            "--after-offset",
            "1",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[{\"recurrence_id\":\"exact\",\"goal\":\"goal\",",
            "\"offset\":2,\"unix_millis\":12,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task-2\"}],",
            "\"next_after\":null}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn materializes_due_recurrence_page_as_ordered_resumable_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize-due",
            database.to_str().unwrap(),
            id.as_str(),
            "1",
            "1",
            "2",
            "23",
            "task\none",
            "task-two",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":1,\"unix_millis\":15,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task\\none\"},",
            "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"offset\":2,\"unix_millis\":20,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task-two\"}],",
            "\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize-due",
            database.to_str().unwrap(),
            id.as_str(),
            "1",
            "3",
            "2",
            "24",
        ])
        .assert()
        .success()
        .stdout("{\"occurrences\":[],\"next_offset\":3}\n")
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    for (offset, task_id) in [(1, "task\none"), (2, "task-two")] {
        let materialized = store
            .load_materialized_occurrence(&id, offset)
            .unwrap()
            .expect("materialized due occurrence");
        assert_eq!(materialized.revision(), 2);
        assert_eq!(materialized.task_id().as_str(), task_id);
    }
    for offset in [0, 3, 4] {
        assert!(store.load_occurrence(&id, offset).unwrap().is_none());
    }
    drop(store);
    let tasks = TaskStore::open(&database).expect("reopened task store");
    for task_id in ["task\none", "task-two"] {
        assert_eq!(
            tasks
                .load(&TaskId::new(task_id).unwrap())
                .unwrap()
                .expect("authoritative task")
                .goal()
                .as_str(),
            "preserve \"exact\" goal"
        );
    }
}

#[test]
fn due_recurrence_page_materialization_validates_and_fails_closed() {
    let directory = tempdir().expect("recurrence database directory");
    for (name, id, page_size, task_id, expected_error) in [
        (
            "invalid-id.sqlite3",
            "",
            "1",
            "task",
            "invalid_recurrence_id",
        ),
        (
            "invalid-page.sqlite3",
            "valid",
            "0",
            "task",
            "invalid_occurrence_page_size",
        ),
        (
            "oversized-page.sqlite3",
            "valid",
            "1025",
            "task",
            "invalid_occurrence_page_size",
        ),
        ("invalid-task.sqlite3", "valid", "1", "", "invalid_task_id"),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize-due",
                database.to_str().unwrap(),
                id,
                "1",
                "0",
                page_size,
                "10",
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }
    let invalid_number_database = directory.path().join("invalid-number.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize-due",
            invalid_number_database.to_str().unwrap(),
            "valid",
            "not-a-revision",
            "0",
            "1",
            "10",
            "task",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
    assert!(!invalid_number_database.exists());

    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);
    TaskStore::open(&database)
        .unwrap()
        .start(
            TaskId::new("occupied").unwrap(),
            TaskGoal::new("occupied goal").unwrap(),
        )
        .unwrap();

    let run = |selected_id: &str, revision: &str, start: &str, tasks: &[&str]| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "materialize-due",
            database.to_str().unwrap(),
            selected_id,
            revision,
            start,
            "2",
            &u64::MAX.to_string(),
        ]);
        command.args(tasks);
        command
    };
    for (selected_id, revision, start, tasks) in [
        ("missing", "1", "0", vec!["missing-0", "missing-1"]),
        (id.as_str(), "2", "0", vec!["stale-0", "stale-1"]),
        (id.as_str(), "1", "2", vec!["range-0", "range-1"]),
        (id.as_str(), "1", "0", vec!["short"]),
        (id.as_str(), "1", "0", vec!["duplicate", "duplicate"]),
        (id.as_str(), "1", "0", vec!["new", "occupied"]),
    ] {
        run(selected_id, revision, start, &tasks)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: due_recurrence_occurrence_materialization_failed:",
            ));
    }
    let store = RecurrenceStore::open(&database).unwrap();
    for offset in 0..2 {
        assert!(store.load_occurrence(&id, offset).unwrap().is_none());
    }
    drop(store);
    for task_id in [
        "missing-0",
        "missing-1",
        "stale-0",
        "stale-1",
        "range-0",
        "range-1",
        "short",
        "duplicate",
        "new",
    ] {
        assert!(
            TaskStore::open(&database)
                .unwrap()
                .load(&TaskId::new(task_id).unwrap())
                .unwrap()
                .is_none()
        );
    }

    let conflict_id = RecurrenceId::new("conflict").unwrap();
    let mut store = RecurrenceStore::open(&database).unwrap();
    store
        .create(
            conflict_id.clone(),
            TaskGoal::new("conflict goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&conflict_id, 1, 0).unwrap();
    drop(store);
    run(
        conflict_id.as_str(),
        "1",
        "0",
        &["existing-0", "existing-1"],
    )
    .assert()
    .code(1)
    .stdout(predicate::str::is_empty())
    .stderr(predicate::str::starts_with(
        "$: due_recurrence_occurrence_materialization_failed:",
    ));
    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload = X'7B7D'
                 WHERE event_type = 'recurrence.occurrence_persisted'
                   AND stream_id LIKE 'recurrence-occurrence:8:conflict:%'",
                [],
            )
            .unwrap(),
        1
    );
    run(conflict_id.as_str(), "1", "0", &["corrupt-0", "corrupt-1"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: due_recurrence_occurrence_materialization_failed:",
        ));
    assert!(
        RecurrenceStore::open(&database)
            .unwrap()
            .load_occurrence(&conflict_id, 1)
            .unwrap()
            .is_none()
    );
    for task_id in ["existing-0", "existing-1", "corrupt-0", "corrupt-1"] {
        assert!(
            TaskStore::open(&database)
                .unwrap()
                .load(&TaskId::new(task_id).unwrap())
                .unwrap()
                .is_none()
        );
    }
    run(id.as_str(), "1", "0", &["max-0", "max-1"])
        .assert()
        .success()
        .stdout(format!(
            "{{\"occurrences\":[{{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":0,\"unix_millis\":{},\"definition_revision\":1,\"occurrence_revision\":2,\"task_id\":\"max-0\"}},{{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":1,\"unix_millis\":{},\"definition_revision\":1,\"occurrence_revision\":2,\"task_id\":\"max-1\"}}],\"next_offset\":null}}\n",
            u64::MAX - 1,
            u64::MAX
        ))
        .stderr(predicate::str::is_empty());
    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    for (offset, task_id) in [(0, "max-0"), (1, "max-1")] {
        let materialized = store
            .load_materialized_occurrence(&id, offset)
            .unwrap()
            .expect("materialized due occurrence");
        assert_eq!(materialized.task_id().as_str(), task_id);
    }
}

#[test]
fn materializes_latest_due_recurrence_occurrence_as_resumable_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    drop(store);

    let run = |start: &str, cutoff: &str, task_id: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "materialize-latest-due",
            database.to_str().unwrap(),
            id.as_str(),
            "1",
            start,
            cutoff,
            task_id,
        ]);
        command
    };
    run("0", "23", "task\none")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
            "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":2,",
            "\"unix_millis\":20,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task\\none\"},",
            "\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());
    run("3", "24", "future-task")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":3}\n")
        .stderr(predicate::str::is_empty());
    run("3", "30", "final-task")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
            "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":4,",
            "\"unix_millis\":30,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"final-task\"},",
            "\"next_offset\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    for offset in [0, 1, 3] {
        assert!(store.load_occurrence(&id, offset).unwrap().is_none());
    }
    for (offset, task_id) in [(2, "task\none"), (4, "final-task")] {
        let materialized = store
            .load_materialized_occurrence(&id, offset)
            .unwrap()
            .expect("materialized latest-due occurrence");
        assert_eq!(materialized.revision(), 2);
        assert_eq!(materialized.task_id().as_str(), task_id);
    }
    drop(store);
    let tasks = TaskStore::open(&database).expect("reopened task store");
    assert_eq!(
        tasks
            .load(&TaskId::new("task\none").unwrap())
            .unwrap()
            .expect("authoritative task")
            .goal()
            .as_str(),
        "preserve \"exact\" goal"
    );
    assert!(
        tasks
            .load(&TaskId::new("future-task").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn latest_due_recurrence_materialization_validates_and_fails_closed() {
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
                "materialize-latest-due",
                database.to_str().unwrap(),
                id,
                "1",
                "0",
                "10",
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
            "materialize-latest-due",
            directory.path().to_str().unwrap(),
            "valid",
            "1",
            "0",
            "10",
            "task",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: latest_due_recurrence_occurrence_materialization_failed:",
        ));

    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);
    let occupied = TaskId::new("occupied").unwrap();
    TaskStore::open(&database)
        .unwrap()
        .start(occupied.clone(), TaskGoal::new("occupied goal").unwrap())
        .unwrap();

    let run = |selected_id: &str, revision: &str, start: &str, task_id: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "materialize-latest-due",
            database.to_str().unwrap(),
            selected_id,
            revision,
            start,
            &u64::MAX.to_string(),
            task_id,
        ]);
        command
    };
    for (selected_id, revision, start, task_id) in [
        ("absent", "1", "0", "missing"),
        ("exact", "2", "0", "stale"),
        ("exact", "1", "2", "out-of-range"),
        ("exact", "1", "0", occupied.as_str()),
    ] {
        run(selected_id, revision, start, task_id)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: latest_due_recurrence_occurrence_materialization_failed:",
            ));
    }
    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    for offset in 0..2 {
        assert!(store.load_occurrence(&id, offset).unwrap().is_none());
    }
    drop(store);

    run("exact", "1", "0", "max-task")
        .assert()
        .success()
        .stdout(format!(
            "{{\"occurrence\":{{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":1,\"unix_millis\":{},\"definition_revision\":1,\"occurrence_revision\":2,\"task_id\":\"max-task\"}},\"next_offset\":null}}\n",
            u64::MAX
        ));
    run("exact", "1", "0", "duplicate")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: latest_due_recurrence_occurrence_materialization_failed:",
        ));
    assert!(
        TaskStore::open(&database)
            .unwrap()
            .load(&TaskId::new("duplicate").unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload = X'7B7D' \
                 WHERE event_type = 'recurrence.occurrence_persisted'",
                [],
            )
            .unwrap(),
        1
    );
    run("exact", "1", "0", "corrupt")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: latest_due_recurrence_occurrence_materialization_failed:",
        ));
    assert!(
        TaskStore::open(&database)
            .unwrap()
            .load(&TaskId::new("corrupt").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn persists_latest_due_recurrence_occurrence_as_resumable_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    drop(store);

    let run = |start: &str, cutoff: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "persist-latest-due",
            database.to_str().unwrap(),
            id.as_str(),
            "1",
            start,
            cutoff,
        ]);
        command
    };
    run("0", "23")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
            "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":2,",
            "\"unix_millis\":20,\"definition_revision\":1},\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());
    run("3", "24")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":3}\n")
        .stderr(predicate::str::is_empty());
    run("3", "30")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
            "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":4,",
            "\"unix_millis\":30,\"definition_revision\":1},\"next_offset\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    for offset in [0, 1, 3] {
        assert!(store.load_occurrence(&id, offset).unwrap().is_none());
    }
    for offset in [2, 4] {
        assert!(store.load_occurrence(&id, offset).unwrap().is_some());
    }
}

#[test]
fn latest_due_recurrence_persistence_validates_and_fails_closed() {
    let directory = tempdir().expect("recurrence database directory");
    let missing_database = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "persist-latest-due",
            missing_database.to_str().unwrap(),
            "",
            "1",
            "0",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!missing_database.exists());

    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);

    let run = |selected_id: &str, revision: &str, start: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "persist-latest-due",
            database.to_str().unwrap(),
            selected_id,
            revision,
            start,
            &u64::MAX.to_string(),
        ]);
        command
    };
    run("exact", "1", "0")
        .assert()
        .success()
        .stdout(format!(
            "{{\"occurrence\":{{\"recurrence_id\":\"exact\",\"goal\":\"goal\",\"offset\":1,\"unix_millis\":{},\"definition_revision\":1}},\"next_offset\":null}}\n",
            u64::MAX
        ));
    for (selected_id, revision, start) in [
        ("absent", "1", "0"),
        ("exact", "2", "0"),
        ("exact", "1", "2"),
        ("exact", "1", "0"),
    ] {
        run(selected_id, revision, start)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: latest_due_recurrence_occurrence_persistence_failed:",
            ));
    }

    assert_eq!(
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE events SET payload = X'7B7D' \
                 WHERE event_type = 'recurrence.occurrence_persisted'",
                [],
            )
            .unwrap(),
        1
    );
    run("exact", "1", "0")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: latest_due_recurrence_occurrence_persistence_failed:",
        ));
}

#[test]
fn selects_latest_due_recurrence_occurrence_as_resumable_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact\nid").unwrap(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    drop(store);

    for (start, cutoff, expected) in [
        (
            "0",
            "10",
            concat!(
                "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":0,",
                "\"unix_millis\":10,\"definition_revision\":1},\"next_offset\":1}\n"
            ),
        ),
        (
            "0",
            "23",
            concat!(
                "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":2,",
                "\"unix_millis\":20,\"definition_revision\":1},\"next_offset\":3}\n"
            ),
        ),
        ("3", "24", "{\"occurrence\":null,\"next_offset\":3}\n"),
        (
            "3",
            "30",
            concat!(
                "{\"occurrence\":{\"recurrence_id\":\"exact\\nid\",",
                "\"goal\":\"preserve \\\"exact\\\" goal\",\"offset\":4,",
                "\"unix_millis\":30,\"definition_revision\":1},\"next_offset\":null}\n"
            ),
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "latest-due",
                database.to_str().unwrap(),
                "exact\nid",
                start,
                cutoff,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn latest_due_recurrence_selection_validates_and_categorizes_inputs() {
    let directory = tempdir().expect("recurrence database directory");
    let invalid_database = directory.path().join("invalid.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "latest-due",
            invalid_database.to_str().unwrap(),
            "",
            "0",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!invalid_database.exists());

    let missing_database = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "latest-due",
            missing_database.to_str().unwrap(),
            "valid",
            "0",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: latest_due_recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing_database.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(u64::MAX),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "latest-due",
            database.to_str().unwrap(),
            "exact",
            "0",
            "18446744073709551615",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"exact\",\"goal\":\"goal\",",
            "\"offset\":0,\"unix_millis\":18446744073709551615,",
            "\"definition_revision\":1},\"next_offset\":null}\n"
        ));

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        (
            "exact",
            "18446744073709551615",
            "recurrence_occurrence_out_of_range",
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "latest-due",
                database.to_str().unwrap(),
                id,
                start,
                "18446744073709551615",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }
}

#[test]
fn latest_due_recurrence_selection_isolates_unrelated_corruption_and_fails_selected() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in ["exact", "unrelated"] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(10),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
    }
    drop(store);

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:unrelated'",
                [],
            )
            .unwrap(),
        1
    );
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "latest-due",
            database.to_str().unwrap(),
            "exact",
            "0",
            "10",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"exact\",\"goal\":\"goal\",",
            "\"offset\":0,\"unix_millis\":10,\"definition_revision\":1},",
            "\"next_offset\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:exact'",
            [],
        )
        .unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "latest-due",
            database.to_str().unwrap(),
            "exact",
            "0",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: latest_due_recurrence_occurrence_lookup_failed:",
        ));
}

#[test]
fn pages_due_recurrence_occurrences_with_resumable_cutoff_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact\nid").unwrap(),
            TaskGoal::new("preserve \"exact\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();
    drop(store);

    for (start, size, cutoff, expected) in [
        (
            "0",
            "2",
            "15",
            concat!(
                "{\"occurrences\":[",
                "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"offset\":0,\"unix_millis\":10,\"definition_revision\":1},",
                "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"offset\":1,\"unix_millis\":15,\"definition_revision\":1}],",
                "\"next_offset\":2}\n"
            ),
        ),
        ("2", "2", "19", "{\"occurrences\":[],\"next_offset\":2}\n"),
        (
            "2",
            "2",
            "25",
            concat!(
                "{\"occurrences\":[",
                "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"offset\":2,\"unix_millis\":20,\"definition_revision\":1},",
                "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"offset\":3,\"unix_millis\":25,\"definition_revision\":1}],",
                "\"next_offset\":4}\n"
            ),
        ),
        (
            "4",
            "2",
            "30",
            concat!(
                "{\"occurrences\":[",
                "{\"recurrence_id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"offset\":4,\"unix_millis\":30,\"definition_revision\":1}],",
                "\"next_offset\":null}\n"
            ),
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "due",
                database.to_str().unwrap(),
                "exact\nid",
                start,
                size,
                cutoff,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn due_recurrence_occurrence_paging_validates_and_categorizes_inputs() {
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
                "due",
                database.to_str().unwrap(),
                id,
                "0",
                page_size,
                "10",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }

    let missing = directory.path().join("missing.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "due",
            missing.to_str().unwrap(),
            "valid",
            "0",
            "1",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: due_recurrence_occurrence_lookup_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(u64::MAX),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "due",
            database.to_str().unwrap(),
            "exact",
            "0",
            "1",
            "18446744073709551615",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrences\":[{\"recurrence_id\":\"exact\",\"goal\":\"goal\",",
            "\"offset\":0,\"unix_millis\":18446744073709551615,",
            "\"definition_revision\":1}],\"next_offset\":null}\n"
        ));

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "1", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "due",
                database.to_str().unwrap(),
                id,
                start,
                "1",
                "18446744073709551615",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }
}

#[test]
fn due_recurrence_occurrence_paging_isolates_unrelated_corruption_and_fails_selected() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in ["exact", "unrelated"] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(10),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
    }
    drop(store);

    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:unrelated'",
            [],
        )
        .unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "due",
            database.to_str().unwrap(),
            "exact",
            "0",
            "1",
            "10",
        ])
        .assert()
        .success();

    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'recurrence:exact'",
            [],
        )
        .unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "due",
            database.to_str().unwrap(),
            "exact",
            "0",
            "1",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: due_recurrence_occurrence_lookup_failed:",
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
fn recurrence_lookup_and_inventory_report_cancellation_and_distinct_revisions() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("cancelled").unwrap(),
            TaskGoal::new("preserve cancellation truth").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);
    insert_recurrence_cancellation(&database, "cancelled", 2, "operator request");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "get",
            database.to_str().expect("UTF-8 database path"),
            "cancelled",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"cancelled\",\"goal\":\"preserve cancellation truth\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,",
            "\"status\":\"cancelled\",",
            "\"final_occurrence_unix_millis\":15,\"definition_revision\":1,",
            "\"aggregate_revision\":2,\"cancellation\":\"operator request\"}\n"
        ))
        .stderr(predicate::str::is_empty());

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
            "{\"id\":\"cancelled\",\"goal\":\"preserve cancellation truth\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",",
            "\"final_occurrence_unix_millis\":15,\"definition_revision\":1,",
            "\"aggregate_revision\":2,\"cancellation\":\"operator request\"}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());
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
            "{\"id\":\"a\\nfirst\",\"goal\":\"preserve \\\"exact\\\" goal\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,\"status\":\"active\",\"final_occurrence_unix_millis\":20,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null},",
            "{\"id\":\"z-last\",\"goal\":\"later\",\"anchor_unix_millis\":40,\"interval_millis\":2,\"occurrence_count\":1,\"status\":\"active\",\"final_occurrence_unix_millis\":40,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}",
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
fn pages_finite_recurrences_with_exact_keyset_continuation() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for (id, goal) in [
        ("z-last", "later"),
        ("a\nfirst", "preserve \"exact\" goal"),
        ("middle", "middle goal"),
    ] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
                ScheduleInterval::from_millis(5).unwrap(),
                OccurrenceCount::new(2).unwrap(),
            )
            .unwrap();
    }
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"a\\nfirst\",\"goal\":\"preserve \\\"exact\\\" goal\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"active\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null},",
            "{\"id\":\"middle\",\"goal\":\"middle goal\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"active\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}",
            "],\"next_after\":\"middle\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "2",
            "middle",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"z-last\",\"goal\":\"later\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"active\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}",
            "],\"next_after\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "2",
            "n-nonexistent",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"z-last\",\"goal\":\"later\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"active\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}",
            "],\"next_after\":null}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "2",
            "zz-nonexistent",
        ])
        .assert()
        .success()
        .stdout("{\"recurrences\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_paging_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(RecurrenceStore::open(&empty).expect("empty recurrence store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            empty.to_str().expect("UTF-8 database path"),
            "1",
        ])
        .assert()
        .success()
        .stdout("{\"recurrences\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");

    for (page_size, after, diagnostic) in [
        ("0", None, "$: invalid_recurrence_page_size:"),
        ("1025", None, "$: invalid_recurrence_page_size:"),
        ("1", Some("   "), "$: invalid_recurrence_id:"),
    ] {
        let mut arguments = vec![
            "recurrence",
            "page",
            missing.to_str().expect("UTF-8 database path"),
            page_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            missing.to_str().expect("UTF-8 database path"),
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_page_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn recurrence_paging_isolates_corruption_outside_the_selected_window() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in ["a-corrupt", "b-valid", "c-valid", "d-corrupt"] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
    }
    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id IN ('recurrence:a-corrupt', 'recurrence:d-corrupt')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "1",
            "a-corrupt",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"b-valid\",\"goal\":\"goal\",\"anchor_unix_millis\":1,\"interval_millis\":1,\"occurrence_count\":1,\"status\":\"active\",\"final_occurrence_unix_millis\":1,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}",
            "],\"next_after\":\"b-valid\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "page",
            database.to_str().expect("UTF-8 database path"),
            "1",
            "b-valid",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_page_inspection_failed:",
        ));
}

#[test]
fn pages_finite_recurrences_sparsely_by_status_with_scan_cursor_progress() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for (id, goal) in [
        ("alpha", "active alpha"),
        ("bravo", "active bravo"),
        ("charlie", "cancel \"exactly\""),
        ("delta", "cancel delta"),
        ("echo", "cancel later"),
    ] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
                ScheduleInterval::from_millis(5).unwrap(),
                OccurrenceCount::new(2).unwrap(),
            )
            .unwrap();
    }
    for id in ["charlie", "delta", "echo"] {
        store
            .cancel(
                &RecurrenceId::new(id).unwrap(),
                1,
                RecurrenceCancellation::new("operator request").unwrap(),
            )
            .unwrap();
    }
    drop(store);

    let cases = [
        (None, "{\"recurrences\":[],\"next_after\":\"bravo\"}\n"),
        (
            Some("bravo"),
            concat!(
                "{\"recurrences\":[",
                "{\"id\":\"charlie\",\"goal\":\"cancel \\\"exactly\\\"\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":2,\"cancellation\":\"operator request\"},",
                "{\"id\":\"delta\",\"goal\":\"cancel delta\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":2,\"cancellation\":\"operator request\"}",
                "],\"next_after\":\"delta\"}\n"
            ),
        ),
        (
            Some("delta"),
            "{\"recurrences\":[{\"id\":\"echo\",\"goal\":\"cancel later\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":2,\"cancellation\":\"operator request\"}],\"next_after\":null}\n",
        ),
        (
            Some("coconut"),
            concat!(
                "{\"recurrences\":[",
                "{\"id\":\"delta\",\"goal\":\"cancel delta\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":2,\"cancellation\":\"operator request\"},",
                "{\"id\":\"echo\",\"goal\":\"cancel later\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":2,\"cancellation\":\"operator request\"}",
                "],\"next_after\":null}\n"
            ),
        ),
        (Some("zzzz"), "{\"recurrences\":[],\"next_after\":null}\n"),
    ];
    for (after, expected) in cases {
        let mut arguments = vec![
            "recurrence",
            "status-page",
            database.to_str().expect("UTF-8 database path"),
            "cancelled",
            "2",
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn recurrence_status_paging_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(RecurrenceStore::open(&empty).expect("empty recurrence store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status-page",
            empty.to_str().expect("UTF-8 database path"),
            "active",
            "1",
        ])
        .assert()
        .success()
        .stdout("{\"recurrences\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    for (status, scan_size, after, diagnostic) in [
        ("ACTIVE", "1", None, "$: invalid_recurrence_status:"),
        ("active", "0", None, "$: invalid_recurrence_page_size:"),
        ("active", "1025", None, "$: invalid_recurrence_page_size:"),
        ("active", "1", Some("   "), "$: invalid_recurrence_id:"),
    ] {
        let mut arguments = vec![
            "recurrence",
            "status-page",
            missing.to_str().expect("UTF-8 database path"),
            status,
            scan_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status-page",
            missing.to_str().expect("UTF-8 database path"),
            "active",
            "1",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_status_page_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn recurrence_status_paging_fails_on_selected_lookahead_and_isolates_other_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in ["a-corrupt", "b-valid", "c-valid", "d-corrupt"] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id IN ('recurrence:a-corrupt', 'recurrence:d-corrupt')",
            [],
        )
        .unwrap();

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status-page",
            database.to_str().expect("UTF-8 database path"),
            "active",
            "1",
            "a-corrupt",
        ])
        .assert()
        .success()
        .stdout("{\"recurrences\":[{\"id\":\"b-valid\",\"goal\":\"goal\",\"anchor_unix_millis\":1,\"interval_millis\":1,\"occurrence_count\":1,\"status\":\"active\",\"final_occurrence_unix_millis\":1,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}],\"next_after\":\"b-valid\"}\n")
        .stderr(predicate::str::is_empty());

    for after in [None, Some("b-valid")] {
        let mut arguments = vec![
            "recurrence",
            "status-page",
            database.to_str().expect("UTF-8 database path"),
            "active",
            "1",
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_status_page_inspection_failed:",
            ));
    }
}

#[test]
fn filters_finite_recurrences_by_exact_lifecycle_status() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for (id, goal) in [
        ("z-active", "later"),
        ("a\nactive", "preserve \"exact\" goal"),
        ("cancelled", "cancelled goal"),
    ] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
                ScheduleInterval::from_millis(5).unwrap(),
                OccurrenceCount::new(2).unwrap(),
            )
            .unwrap();
    }
    let cancelled_id = RecurrenceId::new("cancelled").unwrap();
    store
        .cancel(
            &cancelled_id,
            1,
            RecurrenceCancellation::new("operator\trequest").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status",
            database.to_str().expect("UTF-8 database path"),
            "active",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"a\\nactive\",\"goal\":\"preserve \\\"exact\\\" goal\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"active\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null},",
            "{\"id\":\"z-active\",\"goal\":\"later\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"active\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":1,\"cancellation\":null}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status",
            database.to_str().expect("UTF-8 database path"),
            "cancelled",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrences\":[",
            "{\"id\":\"cancelled\",\"goal\":\"cancelled goal\",\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":2,\"status\":\"cancelled\",\"final_occurrence_unix_millis\":15,\"definition_revision\":1,\"aggregate_revision\":2,\"cancellation\":\"operator\\trequest\"}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_status_inspection_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(RecurrenceStore::open(&empty).expect("empty recurrence store"));

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status",
            empty.to_str().expect("UTF-8 database path"),
            "active",
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
            "status",
            missing.to_str().expect("UTF-8 database path"),
            "ACTIVE",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_status:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "status",
            missing.to_str().expect("UTF-8 database path"),
            "cancelled",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_status_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn recurrence_status_inspection_fails_closed_on_nonmatching_corrupt_history() {
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
            "status",
            database.to_str().expect("UTF-8 database path"),
            "cancelled",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_status_inspection_failed:",
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
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,\"status\":\"active\",",
            "\"final_occurrence_unix_millis\":20,\"definition_revision\":1,",
            "\"aggregate_revision\":1,\"cancellation\":null}\n"
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
fn cancels_one_finite_recurrence_at_the_exact_revision() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact\nid").unwrap();
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
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "cancel",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            "operator\t\"request\"",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"exact\\nid\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3,\"status\":\"cancelled\",",
            "\"final_occurrence_unix_millis\":20,\"definition_revision\":1,",
            "\"aggregate_revision\":2,\"cancellation\":\"operator\\t\\\"request\\\"\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    let recurrence = store.load(&id).unwrap().expect("cancelled recurrence");
    assert_eq!(recurrence.definition_revision(), 1);
    assert_eq!(recurrence.revision(), 2);
    assert_eq!(
        recurrence.cancellation().unwrap().as_str(),
        "operator\t\"request\""
    );
}

#[test]
fn inspects_exact_recurrence_history_as_revision_ordered_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("history\nrecurrence").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    let recurrence = store
        .create(
            id.clone(),
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
            "history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"history\\nrecurrence\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());

    let mut store = RecurrenceStore::open(&database).expect("reopened recurrence store");
    store
        .cancel(
            &id,
            recurrence.revision(),
            RecurrenceCancellation::new("operator\trequest").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"id\":\"history\\nrecurrence\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3},",
            "{\"revision\":2,\"type\":\"cancelled\",\"reason\":\"operator\\trequest\"}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
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
fn inspects_exact_recurrence_occurrence_history_as_revision_ordered_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("history\noccurrence").unwrap();
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
    store.persist_occurrence(&id, 1, 1).unwrap();
    store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .release_occurrence(
            &id,
            1,
            2,
            RecurrenceOccurrenceRelease::new("retry \"elsewhere\"\nnow").unwrap(),
        )
        .unwrap();
    store
        .claim_occurrence(&id, 1, 3, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .materialize_claimed_occurrence(&id, 1, 4, TaskId::new("task\ttrace").unwrap())
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence-history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"recurrence_id\":\"history\\noccurrence\",\"offset\":1,\"history\":[",
            "{\"revision\":1,\"type\":\"persisted\",\"goal\":\"preserve \\\"exact\\\" goal\",",
            "\"unix_millis\":15,\"definition_revision\":1},",
            "{\"revision\":2,\"type\":\"claimed\"},",
            "{\"revision\":3,\"type\":\"released\",\"reason\":\"retry \\\"elsewhere\\\"\\nnow\"},",
            "{\"revision\":4,\"type\":\"claimed\"},",
            "{\"revision\":5,\"type\":\"materialized\",\"task_id\":\"task\\ttrace\"}",
            "]}\n"
        ))
        .stderr(predicate::str::is_empty());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence-history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "2",
        ])
        .assert()
        .success()
        .stdout("{\"recurrence_id\":\"history\\noccurrence\",\"offset\":2,\"history\":null}\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_occurrence_history_validates_before_storage_and_isolates_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence-history",
            missing.to_str().expect("UTF-8 database path"),
            "   ",
            "0",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "occurrence-history",
            missing.to_str().expect("UTF-8 database path"),
            "valid",
            "0",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_history_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("corrupt.sqlite3");
    let selected_id = RecurrenceId::new("selected").unwrap();
    let unrelated_id = RecurrenceId::new("unrelated").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in [&selected_id, &unrelated_id] {
        store
            .create(
                id.clone(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(2).unwrap(),
            )
            .unwrap();
        store.persist_occurrence(id, 1, 0).unwrap();
    }
    store.persist_occurrence(&selected_id, 1, 1).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE events SET payload = X'7B7D'
                 WHERE event_type = 'recurrence.occurrence_persisted'
                   AND CAST(payload AS TEXT) LIKE '%unrelated%'",
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
                   AND CAST(payload AS TEXT) LIKE '%selected%'
                   AND CAST(payload AS TEXT) LIKE '%\"offset\":1%'",
                [],
            )
            .unwrap(),
        1
    );

    let inspect = |id: &RecurrenceId, offset: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "occurrence-history",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            offset,
        ]);
        command
    };
    inspect(&selected_id, "0")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"type\":\"persisted\""))
        .stderr(predicate::str::is_empty());
    inspect(&selected_id, "1")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_history_failed:",
        ));
    inspect(&unrelated_id, "0")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_history_failed:",
        ));
}

#[test]
fn pages_sparse_recurrence_occurrence_histories_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("history\npage").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
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
    store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .release_occurrence(
            &id,
            1,
            2,
            RecurrenceOccurrenceRelease::new("retry \"elsewhere\"\nnow").unwrap(),
        )
        .unwrap();
    store
        .claim_occurrence(&id, 1, 3, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .materialize_claimed_occurrence(&id, 1, 4, TaskId::new("task\ttrace").unwrap())
        .unwrap();
    store.persist_occurrence(&id, 1, 4).unwrap();
    drop(store);

    for (start, size, expected) in [
        (
            "0",
            "3",
            concat!(
                "{\"histories\":[{\"recurrence_id\":\"history\\npage\",\"offset\":1,\"history\":[",
                "{\"revision\":1,\"type\":\"persisted\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"unix_millis\":15,\"definition_revision\":1},",
                "{\"revision\":2,\"type\":\"claimed\"},",
                "{\"revision\":3,\"type\":\"released\",\"reason\":\"retry \\\"elsewhere\\\"\\nnow\"},",
                "{\"revision\":4,\"type\":\"claimed\"},",
                "{\"revision\":5,\"type\":\"materialized\",\"task_id\":\"task\\ttrace\"}",
                "]}],\"next_offset\":3}\n"
            ),
        ),
        ("2", "2", "{\"histories\":[],\"next_offset\":4}\n"),
        (
            "4",
            "1",
            concat!(
                "{\"histories\":[{\"recurrence_id\":\"history\\npage\",\"offset\":4,\"history\":[",
                "{\"revision\":1,\"type\":\"persisted\",\"goal\":\"preserve \\\"exact\\\" goal\",",
                "\"unix_millis\":30,\"definition_revision\":1}]}",
                "],\"next_offset\":null}\n"
            ),
        ),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "occurrence-histories",
                database.to_str().expect("UTF-8 database path"),
                id.as_str(),
                start,
                size,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn recurrence_occurrence_history_paging_validates_before_storage_access() {
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
                "occurrence-histories",
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
fn recurrence_occurrence_history_paging_categorizes_bounds_and_isolates_corruption() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");
    let run = |database: &std::path::Path, id: &str, start: &str, size: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "occurrence-histories",
            database.to_str().expect("UTF-8 database path"),
            id,
            start,
            size,
        ]);
        command
    };
    run(&missing, "selected", "0", "1")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_histories_failed:",
        ));
    assert!(!missing.exists());

    let database = directory.path().join("events.sqlite3");
    let selected = RecurrenceId::new("selected").unwrap();
    let unrelated = RecurrenceId::new("unrelated\ntrace").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in [&selected, &unrelated] {
        store
            .create(
                id.clone(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(3).unwrap(),
            )
            .unwrap();
        store.persist_occurrence(id, 1, 0).unwrap();
    }
    store.persist_occurrence(&selected, 1, 2).unwrap();
    drop(store);

    for (id, start) in [("missing", "0"), (selected.as_str(), "3")] {
        run(&database, id, start, "1")
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_histories_failed:",
            ));
    }

    let connection = rusqlite::Connection::open(&database).unwrap();
    for (id, offset) in [(unrelated.as_str(), 0), (selected.as_str(), 2)] {
        let stream_id = format!("recurrence-occurrence:{}:{id}:{offset}", id.len());
        assert_eq!(
            connection
                .execute(
                    "UPDATE events SET payload = X'7B7D'
                     WHERE event_type = 'recurrence.occurrence_persisted'
                       AND stream_id = ?1",
                    [stream_id],
                )
                .unwrap(),
            1
        );
    }
    drop(connection);

    run(&database, selected.as_str(), "0", "2")
        .assert()
        .success()
        .stdout(concat!(
            "{\"histories\":[{\"recurrence_id\":\"selected\",\"offset\":0,\"history\":[",
            "{\"revision\":1,\"type\":\"persisted\",\"goal\":\"goal\",",
            "\"unix_millis\":1,\"definition_revision\":1}]}],\"next_offset\":2}\n"
        ))
        .stderr(predicate::str::is_empty());
    for (id, start) in [(selected.as_str(), "1"), (unrelated.as_str(), "0")] {
        run(&database, id, start, "2")
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_histories_failed:",
            ));
    }
}

#[test]
fn recurrence_history_validates_before_storage_and_fails_closed() {
    let directory = tempdir().expect("recurrence database directory");
    let missing = directory.path().join("missing.sqlite3");

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "history",
            missing.to_str().expect("UTF-8 database path"),
            "   ",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: invalid_recurrence_id:"));
    assert!(!missing.exists());

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "history",
            missing.to_str().expect("UTF-8 database path"),
            "valid",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: recurrence_history_failed:"));
    assert!(!missing.exists());

    let database = directory.path().join("corrupt.sqlite3");
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
            "history",
            database.to_str().expect("UTF-8 database path"),
            "corrupt",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with("$: recurrence_history_failed:"));
}

#[test]
fn pages_complete_recurrence_histories_with_exact_keyset_continuation() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for (id, goal) in [
        ("z-last", "last"),
        ("a\nfirst", "run \"carefully\""),
        ("middle", "stop later"),
    ] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new(goal).unwrap(),
                ScheduleInstant::from_unix_millis(10),
                ScheduleInterval::from_millis(5).unwrap(),
                OccurrenceCount::new(3).unwrap(),
            )
            .unwrap();
    }
    store
        .cancel(
            &RecurrenceId::new("middle").unwrap(),
            1,
            RecurrenceCancellation::new("operator\trequest").unwrap(),
        )
        .unwrap();
    drop(store);

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "history-page",
            database.to_str().expect("UTF-8 database path"),
            "2",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "{\"histories\":[",
            "{\"id\":\"a\\nfirst\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"run \\\"carefully\\\"\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3}]},",
            "{\"id\":\"middle\",\"history\":[",
            "{\"revision\":1,\"type\":\"created\",\"goal\":\"stop later\",",
            "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3},",
            "{\"revision\":2,\"type\":\"cancelled\",\"reason\":\"operator\\trequest\"}]}",
            "],\"next_after\":\"middle\"}\n"
        ))
        .stderr(predicate::str::is_empty());

    for (after, expected) in [
        (
            "middle",
            concat!(
                "{\"histories\":[{\"id\":\"z-last\",\"history\":[",
                "{\"revision\":1,\"type\":\"created\",\"goal\":\"last\",",
                "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3}",
                "]}],\"next_after\":null}\n"
            ),
        ),
        (
            "n-nonexistent",
            concat!(
                "{\"histories\":[{\"id\":\"z-last\",\"history\":[",
                "{\"revision\":1,\"type\":\"created\",\"goal\":\"last\",",
                "\"anchor_unix_millis\":10,\"interval_millis\":5,\"occurrence_count\":3}",
                "]}],\"next_after\":null}\n"
            ),
        ),
        ("zz", "{\"histories\":[],\"next_after\":null}\n"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "history-page",
                database.to_str().expect("UTF-8 database path"),
                "2",
                after,
            ])
            .assert()
            .success()
            .stdout(expected)
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn recurrence_history_page_validates_before_read_only_storage_access() {
    let directory = tempdir().expect("recurrence database directory");
    let empty = directory.path().join("empty.sqlite3");
    drop(RecurrenceStore::open(&empty).expect("empty recurrence store"));
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["recurrence", "history-page", empty.to_str().unwrap(), "1"])
        .assert()
        .success()
        .stdout("{\"histories\":[],\"next_after\":null}\n")
        .stderr(predicate::str::is_empty());

    let missing = directory.path().join("missing.sqlite3");
    for (page_size, after, diagnostic) in [
        ("0", None, "$: invalid_recurrence_page_size:"),
        ("1025", None, "$: invalid_recurrence_page_size:"),
        ("1", Some("   "), "$: invalid_recurrence_id:"),
    ] {
        let mut arguments = vec![
            "recurrence",
            "history-page",
            missing.to_str().unwrap(),
            page_size,
        ];
        arguments.extend(after);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args(arguments)
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(diagnostic));
        assert!(!missing.exists());
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args(["recurrence", "history-page", missing.to_str().unwrap(), "1"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_history_page_inspection_failed:",
        ));
    assert!(!missing.exists());
}

#[test]
fn recurrence_history_page_fails_on_selected_or_lookahead_corruption_only() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    for id in ["a-before", "b-selected", "c-lookahead", "d-after"] {
        store
            .create(
                RecurrenceId::new(id).unwrap(),
                TaskGoal::new("goal").unwrap(),
                ScheduleInstant::from_unix_millis(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
    }
    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    let corrupt = |id: &str| {
        assert_eq!(
            connection
                .execute(
                    "UPDATE events SET payload = X'7B7D' WHERE stream_id = ?1",
                    [format!("recurrence:{id}")],
                )
                .unwrap(),
            1
        );
    };
    corrupt("a-before");
    corrupt("d-after");

    let run = || {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "history-page",
            database.to_str().unwrap(),
            "1",
            "a-before",
        ]);
        command
    };
    run()
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\":\"b-selected\""))
        .stderr(predicate::str::is_empty());

    corrupt("c-lookahead");
    for after in ["a-before", "b-selected"] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "history-page",
                database.to_str().unwrap(),
                "1",
                after,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_history_page_inspection_failed:",
            ));
    }
}

#[test]
fn recurrence_cancellation_rejects_missing_stale_and_cancelled_intent_without_append() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("recurrence").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    drop(store);

    for (selected_id, revision) in [(id.as_str(), "0"), ("missing", "1")] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "cancel",
                database.to_str().expect("UTF-8 database path"),
                selected_id,
                revision,
                "operator request",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_cancellation_failed:",
            ));
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "cancel",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "1",
            "operator request",
        ])
        .assert()
        .success();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "cancel",
            database.to_str().expect("UTF-8 database path"),
            id.as_str(),
            "2",
            "second request",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_cancellation_failed:",
        ));

    let store = RecurrenceStore::open_read_only(&database).expect("read-only recurrence store");
    let recurrence = store.load(&id).unwrap().expect("cancelled recurrence");
    assert_eq!(recurrence.revision(), 2);
    assert_eq!(
        recurrence.cancellation().unwrap().as_str(),
        "operator request"
    );
}

#[test]
fn recurrence_cancellation_validates_before_storage_and_reports_storage_failures() {
    let directory = tempdir().expect("recurrence database directory");

    let invalid_revision = directory.path().join("invalid-revision.sqlite3");
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
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
        ("invalid-id.sqlite3", " ", "reason", "invalid_recurrence_id"),
        (
            "invalid-reason.sqlite3",
            "id",
            "\t",
            "invalid_recurrence_cancellation",
        ),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
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
            "recurrence",
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
            "$: recurrence_cancellation_failed:",
        ));
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

#[test]
fn claims_next_available_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("next\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"next\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(6).unwrap(),
        )
        .unwrap();
    for offset in 1..6 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .claim_occurrence(&id, 2, 1, ScheduleInstant::from_unix_millis(20))
        .unwrap();
    store
        .release_occurrence(
            &id,
            2,
            2,
            RecurrenceOccurrenceRelease::new("retry\ncarefully").unwrap(),
        )
        .unwrap();
    store
        .materialize_occurrence(&id, 3, 1, TaskId::new("occupied").unwrap())
        .unwrap();
    drop(store);

    let claim_next = |start: &str, size: &str, cutoff: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "claim-next",
            database.to_str().unwrap(),
            id.as_str(),
            start,
            size,
            cutoff,
        ]);
        command
    };

    claim_next("0", "5", "20")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"next\\nid\",",
            "\"goal\":\"preserve \\\"next\\\" goal\",\"offset\":2,",
            "\"unix_millis\":20,\"definition_revision\":1,",
            "\"occurrence_revision\":4,\"latest_release\":\"retry\\ncarefully\"},",
            "\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());
    claim_next("0", "5", "25")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":4}\n")
        .stderr(predicate::str::is_empty());
    claim_next("4", "2", "30")
        .assert()
        .success()
        .stdout(predicate::str::ends_with(concat!(
            "\"offset\":4,\"unix_millis\":30,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"latest_release\":null},\"next_offset\":5}\n"
        )))
        .stderr(predicate::str::is_empty());
    claim_next("0", "5", "100")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":5}\n");
    claim_next("5", "1", "100")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"offset\":5"));
    claim_next("0", "6", "100")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":null}\n")
        .stderr(predicate::str::is_empty());
}

#[test]
fn recurrence_claim_next_validates_before_storage_access_and_categorizes_failures() {
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
                "claim-next",
                database.to_str().unwrap(),
                id,
                "0",
                page_size,
                "10",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }

    let database = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            RecurrenceId::new("exact").unwrap(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    drop(store);

    for (id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "2", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "claim-next",
                database.to_str().unwrap(),
                id,
                start,
                "1",
                "10",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }
}

#[test]
fn recurrence_claim_next_fails_closed_on_selected_corruption_and_read_only_storage() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    drop(store);

    let claim = || {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "claim-next",
            database.to_str().unwrap(),
            id.as_str(),
            "0",
            "2",
            "20",
        ]);
        command
    };
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload_version = 2 WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    claim()
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_claim_next_failed:",
        ));

    let read_only_database = directory.path().join("read-only.sqlite3");
    let read_only_id = RecurrenceId::new("read-only").unwrap();
    let mut store = RecurrenceStore::open(&read_only_database).unwrap();
    store
        .create(
            read_only_id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&read_only_id, 1, 0).unwrap();
    drop(store);
    let mut permissions = fs::metadata(&read_only_database).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&read_only_database, permissions).unwrap();
    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "claim-next",
            read_only_database.to_str().unwrap(),
            read_only_id.as_str(),
            "0",
            "1",
            "10",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_claim_next_failed:",
        ));
}

#[test]
fn materializes_next_available_recurrence_occurrence_as_deterministic_json() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("next\nid").unwrap();
    let mut store = RecurrenceStore::open(&database).expect("writable recurrence store");
    store
        .create(
            id.clone(),
            TaskGoal::new("preserve \"next\" goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(6).unwrap(),
        )
        .unwrap();
    for offset in 1..6 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    store
        .claim_occurrence(&id, 1, 1, ScheduleInstant::from_unix_millis(15))
        .unwrap();
    store
        .claim_occurrence(&id, 2, 1, ScheduleInstant::from_unix_millis(20))
        .unwrap();
    store
        .release_occurrence(
            &id,
            2,
            2,
            RecurrenceOccurrenceRelease::new("retry carefully").unwrap(),
        )
        .unwrap();
    store
        .materialize_occurrence(&id, 3, 1, TaskId::new("occupied").unwrap())
        .unwrap();
    drop(store);

    let materialize_next = |start: &str, size: &str, cutoff: &str, task_id: &str| {
        let mut command = Command::cargo_bin("vela-dev").expect("vela-dev binary");
        command.args([
            "recurrence",
            "materialize-next",
            database.to_str().unwrap(),
            id.as_str(),
            start,
            size,
            cutoff,
            task_id,
        ]);
        command
    };

    materialize_next("0", "5", "20", "task\n2")
        .assert()
        .success()
        .stdout(concat!(
            "{\"occurrence\":{\"recurrence_id\":\"next\\nid\",",
            "\"goal\":\"preserve \\\"next\\\" goal\",\"offset\":2,",
            "\"unix_millis\":20,\"definition_revision\":1,",
            "\"occurrence_revision\":4,\"task_id\":\"task\\n2\"},",
            "\"next_offset\":3}\n"
        ))
        .stderr(predicate::str::is_empty());
    materialize_next("0", "5", "25", "future-task")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":4}\n");
    materialize_next("4", "2", "30", "task-4")
        .assert()
        .success()
        .stdout(predicate::str::ends_with(concat!(
            "\"offset\":4,\"unix_millis\":30,\"definition_revision\":1,",
            "\"occurrence_revision\":2,\"task_id\":\"task-4\"},",
            "\"next_offset\":5}\n"
        )));
    materialize_next("0", "5", "100", "gap-task")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":5}\n");
    materialize_next("5", "1", "100", "task-5")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"offset\":5"));
    materialize_next("0", "6", "100", "complete-task")
        .assert()
        .success()
        .stdout("{\"occurrence\":null,\"next_offset\":null}\n");
}

#[test]
fn recurrence_materialize_next_validates_and_preserves_failure_atomicity() {
    let directory = tempdir().expect("recurrence database directory");
    for (name, id, page_size, task_id, expected_error) in [
        (
            "invalid-id.sqlite3",
            "",
            "1",
            "task",
            "invalid_recurrence_id",
        ),
        (
            "zero-size.sqlite3",
            "valid",
            "0",
            "task",
            "invalid_occurrence_page_size",
        ),
        (
            "oversized.sqlite3",
            "valid",
            "1025",
            "task",
            "invalid_occurrence_page_size",
        ),
        ("invalid-task.sqlite3", "valid", "1", "", "invalid_task_id"),
    ] {
        let database = directory.path().join(name);
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize-next",
                database.to_str().unwrap(),
                id,
                "0",
                page_size,
                "10",
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
        assert!(!database.exists());
    }

    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    store
        .materialize_occurrence(&id, 0, 1, TaskId::new("occupied").unwrap())
        .unwrap();
    drop(store);

    for (raw_id, start, expected_error) in [
        ("absent", "0", "recurrence_not_found"),
        ("exact", "2", "recurrence_occurrence_out_of_range"),
    ] {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize-next",
                database.to_str().unwrap(),
                raw_id,
                start,
                "1",
                "20",
                "new-task",
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(format!("$: {expected_error}:")));
    }

    Command::cargo_bin("vela-dev")
        .expect("vela-dev binary")
        .args([
            "recurrence",
            "materialize-next",
            database.to_str().unwrap(),
            id.as_str(),
            "0",
            "2",
            "20",
            "occupied",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::starts_with(
            "$: recurrence_occurrence_materialize_next_failed:",
        ));
    let store = RecurrenceStore::open_read_only(&database).unwrap();
    let available = store
        .available_occurrences_page(&id, 1, OccurrencePageSize::new(1).unwrap())
        .unwrap();
    assert_eq!(available.occurrences()[0].occurrence().offset(), 1);
}

#[test]
fn recurrence_materialize_next_fails_closed_on_corruption_and_read_only_storage() {
    let directory = tempdir().expect("recurrence database directory");
    let database = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("exact").unwrap();
    let mut store = RecurrenceStore::open(&database).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    drop(store);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "UPDATE events SET payload_version = 2 WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    let assert_failure = |database: &std::path::Path, id: &RecurrenceId, task_id: &str| {
        Command::cargo_bin("vela-dev")
            .expect("vela-dev binary")
            .args([
                "recurrence",
                "materialize-next",
                database.to_str().unwrap(),
                id.as_str(),
                "0",
                "1",
                "10",
                task_id,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::starts_with(
                "$: recurrence_occurrence_materialize_next_failed:",
            ));
    };
    assert_failure(&database, &id, "corrupt-task");
    assert!(
        TaskStore::open(&database)
            .unwrap()
            .load(&TaskId::new("corrupt-task").unwrap())
            .unwrap()
            .is_none()
    );

    let read_only_database = directory.path().join("read-only.sqlite3");
    let read_only_id = RecurrenceId::new("read-only").unwrap();
    let mut store = RecurrenceStore::open(&read_only_database).unwrap();
    store
        .create(
            read_only_id.clone(),
            TaskGoal::new("goal").unwrap(),
            ScheduleInstant::from_unix_millis(10),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&read_only_id, 1, 0).unwrap();
    drop(store);
    let mut permissions = fs::metadata(&read_only_database).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&read_only_database, permissions).unwrap();
    assert_failure(&read_only_database, &read_only_id, "read-only-task");
}
