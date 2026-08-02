use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    scheduler::{
        OccurrenceCount, RecurrenceId, RecurrenceStore, RecurrenceStoreError, ScheduleInstant,
        ScheduleInterval,
    },
    task::TaskGoal,
};

fn instant(unix_millis: u64) -> ScheduleInstant {
    ScheduleInstant::from_unix_millis(unix_millis)
}

#[test]
fn creates_and_reopens_an_exact_finite_fixed_interval_recurrence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new(" morning ").unwrap();
    let goal = TaskGoal::new("Prepare the exact report").unwrap();
    let interval = ScheduleInterval::from_millis(7).unwrap();
    let count = OccurrenceCount::new(3).unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    assert_eq!(
        store.load(&RecurrenceId::new("missing").unwrap()).unwrap(),
        None
    );

    let recurrence = store
        .create(id.clone(), goal.clone(), instant(5), interval, count)
        .unwrap();

    assert_eq!(recurrence.id(), &id);
    assert_eq!(recurrence.goal(), &goal);
    assert_eq!(recurrence.anchor(), instant(5));
    assert_eq!(recurrence.interval(), interval);
    assert_eq!(recurrence.occurrence_count(), count);
    assert_eq!(recurrence.final_occurrence(), instant(19));
    assert_eq!(recurrence.revision(), 1);
    drop(store);

    assert_eq!(
        RecurrenceStore::open(&path).unwrap().load(&id).unwrap(),
        Some(recurrence)
    );
}

#[test]
fn occurrence_count_and_recurrence_ids_validate_without_normalizing() {
    assert_eq!(
        OccurrenceCount::new(0).unwrap_err().to_string(),
        "recurrence occurrence count must be greater than zero"
    );
    assert_eq!(OccurrenceCount::new(u64::MAX).unwrap().get(), u64::MAX);
    assert!(RecurrenceId::new(" \t").is_err());
    assert_eq!(RecurrenceId::new(" exact ").unwrap().as_str(), " exact ");
}

#[test]
fn accepts_an_exact_maximum_final_occurrence() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let interval = ScheduleInterval::from_millis(2).unwrap();
    let count = OccurrenceCount::new((u64::MAX - 5) / 2 + 1).unwrap();

    let recurrence = store
        .create(
            RecurrenceId::new("maximum").unwrap(),
            TaskGoal::new("Reach the boundary").unwrap(),
            instant(5),
            interval,
            count,
        )
        .unwrap();

    assert_eq!(recurrence.final_occurrence(), instant(u64::MAX));
}

#[test]
fn rejects_overflow_before_persisting_any_recurrence_stream() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("overflow").unwrap();
    let interval = ScheduleInterval::from_millis(1).unwrap();
    let count = OccurrenceCount::new(2).unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();

    let error = store
        .create(
            id.clone(),
            TaskGoal::new("Must not wrap").unwrap(),
            instant(u64::MAX),
            interval,
            count,
        )
        .unwrap_err();

    match error {
        RecurrenceStoreError::OccurrenceOverflow {
            recurrence_id,
            source,
        } => {
            assert_eq!(recurrence_id, id);
            assert_eq!(source.instant(), instant(u64::MAX));
            assert_eq!(source.interval(), interval);
            assert_eq!(source.offset(), 1);
        }
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(store.load(&id).unwrap(), None);
}

#[test]
fn duplicate_creation_preserves_the_original_definition() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new("duplicate").unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let original = store
        .create(
            id.clone(),
            TaskGoal::new("Original").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store
            .create(
                id.clone(),
                TaskGoal::new("Replacement").unwrap(),
                instant(20),
                ScheduleInterval::from_millis(9).unwrap(),
                OccurrenceCount::new(4).unwrap(),
            )
            .unwrap_err(),
        RecurrenceStoreError::AlreadyExists { recurrence_id } if recurrence_id == id
    ));
    assert_eq!(store.load(&id).unwrap(), Some(original));
}

#[test]
fn rejects_invalid_persisted_interval_count_and_range_values() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&path).unwrap();

    for (suffix, payload) in [
        (
            "zero-interval",
            br#"{"goal":"Invalid","anchor_unix_millis":1,"interval_millis":0,"occurrence_count":1}"#.as_slice(),
        ),
        (
            "zero-count",
            br#"{"goal":"Invalid","anchor_unix_millis":1,"interval_millis":1,"occurrence_count":0}"#.as_slice(),
        ),
        (
            "overflow",
            br#"{"goal":"Invalid","anchor_unix_millis":18446744073709551615,"interval_millis":1,"occurrence_count":2}"#.as_slice(),
        ),
    ] {
        let id = RecurrenceId::new(suffix).unwrap();
        store
            .create(
                id.clone(),
                TaskGoal::new("Valid first").unwrap(),
                instant(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE events SET payload = ?1 WHERE stream_id = ?2",
                rusqlite::params![payload, format!("recurrence:{suffix}")],
            )
            .unwrap();
        assert!(matches!(
            store.load(&id).unwrap_err(),
            RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
        ));
    }
}

#[test]
fn rejects_unsupported_persisted_event_types_and_versions() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&path).unwrap();

    for (suffix, column, value) in [
        ("unsupported-type", "event_type", "recurrence.unknown"),
        ("unsupported-version", "payload_version", "2"),
    ] {
        let id = RecurrenceId::new(suffix).unwrap();
        store
            .create(
                id.clone(),
                TaskGoal::new("Valid first").unwrap(),
                instant(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute(
                &format!("UPDATE events SET {column} = ?1 WHERE stream_id = ?2"),
                rusqlite::params![value, format!("recurrence:{suffix}")],
            )
            .unwrap();
        assert!(matches!(
            store.load(&id).unwrap_err(),
            RecurrenceStoreError::Replay(ReplayError::UnsupportedEvent { .. })
        ));
    }
}

#[test]
fn rejects_unknown_payload_fields_and_multiple_definition_events() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&path).unwrap();
    let malformed_id = RecurrenceId::new("malformed").unwrap();
    store
        .create(
            malformed_id.clone(),
            TaskGoal::new("Valid first").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'recurrence:malformed'",
            [br#"{"goal":"Valid first","anchor_unix_millis":1,"interval_millis":1,"occurrence_count":1,"unexpected":true}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store.load(&malformed_id).unwrap_err(),
        RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));

    let multiple_id = RecurrenceId::new("multiple").unwrap();
    store
        .create(
            multiple_id.clone(),
            TaskGoal::new("Only definition").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('recurrence:multiple', 2, 'recurrence.fixed_interval_created', 1, ?1)",
            [br#"{"goal":"Second","anchor_unix_millis":2,"interval_millis":1,"occurrence_count":1}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store.load(&multiple_id).unwrap_err(),
        RecurrenceStoreError::InvalidHistory { event_count: 2 }
    ));
}
