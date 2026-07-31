use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    scheduler::{ScheduleId, ScheduleInstant, ScheduleStore, ScheduleStoreError},
    task::{TaskGoal, TaskId, TaskStore},
};

fn instant(unix_millis: u64) -> ScheduleInstant {
    ScheduleInstant::from_unix_millis(unix_millis)
}

#[test]
fn schedule_ids_require_content_without_normalizing_exact_values() {
    assert!(ScheduleId::new(" \t").is_err());
    let exact = ScheduleId::new(" Morning ").unwrap();
    assert_eq!(exact.as_str(), " Morning ");
}

#[test]
fn schedules_and_loads_exact_one_shot_intent_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("morning-check").unwrap();
    let goal = TaskGoal::new("Check the build queue").unwrap();

    let scheduled = ScheduleStore::open(&path)
        .unwrap()
        .schedule(id.clone(), goal.clone(), instant(1_775_000_000_123))
        .unwrap();

    assert_eq!(scheduled.id(), &id);
    assert_eq!(scheduled.goal(), &goal);
    assert_eq!(scheduled.due_at().unix_millis(), 1_775_000_000_123);
    assert_eq!(
        ScheduleStore::open(&path).unwrap().load(&id).unwrap(),
        Some(scheduled)
    );
}

#[test]
fn rejects_duplicate_schedule_ids_without_rewriting_original_intent() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("same").unwrap();
    let original_goal = TaskGoal::new("Original").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    let original = store
        .schedule(id.clone(), original_goal, instant(10))
        .unwrap();

    let error = store
        .schedule(
            id.clone(),
            TaskGoal::new("Replacement").unwrap(),
            instant(5),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ScheduleStoreError::AlreadyExists { schedule_id } if schedule_id == id
    ));
    assert_eq!(store.load(&id).unwrap(), Some(original));
}

#[test]
fn lists_due_intents_in_due_then_exact_id_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for (id, due) in [("zeta", 20), ("future", 21), ("beta", 20), ("early", 5)] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(due),
            )
            .unwrap();
    }
    TaskStore::open(&path)
        .unwrap()
        .start(
            TaskId::new("not-a-schedule").unwrap(),
            TaskGoal::new("Ignore this stream").unwrap(),
        )
        .unwrap();

    let due = ScheduleStore::open(&path)
        .unwrap()
        .list_due(instant(20))
        .unwrap();

    assert_eq!(
        due.iter()
            .map(|item| item.id().as_str())
            .collect::<Vec<_>>(),
        ["early", "beta", "zeta"]
    );
}

#[test]
fn empty_store_has_no_due_intents() {
    let directory = tempdir().unwrap();
    let store = ScheduleStore::open(directory.path().join("events.sqlite3")).unwrap();

    assert!(store.list_due(instant(u64::MAX)).unwrap().is_empty());
}

#[test]
fn due_listing_rejects_malformed_creation_payload_and_owning_stream_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ScheduleStore::open(&path)
        .unwrap()
        .schedule(
            ScheduleId::new("corrupt").unwrap(),
            TaskGoal::new("Corrupt me").unwrap(),
            instant(7),
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'schedule:corrupt'",
            [],
        )
        .unwrap();
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(7))
            .unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET stream_id = 'schedule:', payload = ?1",
            [br#"{"goal":"Corrupt me","due_at_unix_millis":7}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(7))
            .unwrap_err(),
        ScheduleStoreError::InvalidStreamId { ref stream_id } if stream_id == "schedule:"
    ));
}

#[test]
fn due_listing_rejects_duplicate_creation_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ScheduleStore::open(&path)
        .unwrap()
        .schedule(
            ScheduleId::new("duplicate-history").unwrap(),
            TaskGoal::new("First").unwrap(),
            instant(1),
        )
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:duplicate-history', 2, 'schedule.created', 1, ?1)",
            [br#"{"goal":"Second","due_at_unix_millis":2}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(u64::MAX))
            .unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 2 }
    ));
}
