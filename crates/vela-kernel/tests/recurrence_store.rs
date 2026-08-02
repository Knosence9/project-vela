use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    scheduler::{
        OccurrenceCount, OccurrencePageSize, OccurrencePageSizeError, RecurrenceId,
        RecurrenceOccurrenceLookupError, RecurrenceStore, RecurrenceStoreError, ScheduleInstant,
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
fn lists_complete_recurrence_definitions_in_exact_id_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&path).unwrap();
    let later = store
        .create(
            RecurrenceId::new("zeta").unwrap(),
            TaskGoal::new("Later exact goal").unwrap(),
            instant(11),
            ScheduleInterval::from_millis(7).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let earlier = store
        .create(
            RecurrenceId::new(" alpha ").unwrap(),
            TaskGoal::new("Earlier exact goal").unwrap(),
            instant(5),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('unrelated', 1, 'other.created', 1, '{}')",
            [],
        )
        .unwrap();
    drop(connection);

    let listed = RecurrenceStore::open_read_only(&path)
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(listed, vec![earlier, later]);
}

#[test]
fn read_only_recurrence_inventory_is_empty_and_never_creates_storage() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");

    assert!(matches!(
        RecurrenceStore::open_read_only(&path),
        Err(RecurrenceStoreError::EventLog(_))
    ));
    assert!(!path.exists());

    RecurrenceStore::open(&path).unwrap();
    let mut read_only = RecurrenceStore::open_read_only(&path).unwrap();
    assert!(read_only.list().unwrap().is_empty());
    assert!(matches!(
        read_only.create(
            RecurrenceId::new("blocked").unwrap(),
            TaskGoal::new("Must remain inert").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        ),
        Err(RecurrenceStoreError::EventLog(_))
    ));
    assert!(read_only.list().unwrap().is_empty());
}

#[test]
fn recurrence_inventory_rejects_malformed_owning_stream_ids() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            RecurrenceId::new("valid-first").unwrap(),
            TaskGoal::new("Must fail closed").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET stream_id = 'recurrence:' WHERE stream_id = 'recurrence:valid-first'",
            [],
        )
        .unwrap();

    assert!(matches!(
        RecurrenceStore::open_read_only(&path).unwrap().list().unwrap_err(),
        RecurrenceStoreError::InvalidStreamId { ref stream_id } if stream_id == "recurrence:"
    ));
}

#[test]
fn recurrence_inventory_rejects_invalid_discovered_histories() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            RecurrenceId::new("multiple").unwrap(),
            TaskGoal::new("Only definition").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('recurrence:multiple', 2, 'recurrence.fixed_interval_created', 1, ?1)",
            [br#"{"goal":"Second","anchor_unix_millis":2,"interval_millis":1,"occurrence_count":1}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        RecurrenceStore::open_read_only(&path)
            .unwrap()
            .list()
            .unwrap_err(),
        RecurrenceStoreError::InvalidHistory { event_count: 2 }
    ));
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
        assert!(matches!(
            store.list().unwrap_err(),
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

#[test]
fn projects_exact_zero_interior_and_final_occurrences() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new(" projected ").unwrap();
    let goal = TaskGoal::new("Preserve exact projection evidence").unwrap();
    let recurrence = RecurrenceStore::open(directory.path().join("events.sqlite3"))
        .unwrap()
        .create(
            id.clone(),
            goal.clone(),
            instant(5),
            ScheduleInterval::from_millis(7).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();

    for (offset, expected_instant) in [(0, 5), (1, 12), (2, 19)] {
        let occurrence = recurrence.occurrence_at(offset).unwrap();
        assert_eq!(occurrence.recurrence_id(), &id);
        assert_eq!(occurrence.goal(), &goal);
        assert_eq!(occurrence.offset(), offset);
        assert_eq!(occurrence.instant(), instant(expected_instant));
        assert_eq!(occurrence.recurrence_revision(), 1);
    }
}

#[test]
fn projects_the_maximum_instant_and_rejects_offsets_outside_the_finite_range() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new("bounded").unwrap();
    let count = OccurrenceCount::new(2).unwrap();
    let recurrence = RecurrenceStore::open(directory.path().join("events.sqlite3"))
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Reach the exact boundary").unwrap(),
            instant(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            count,
        )
        .unwrap();

    assert_eq!(
        recurrence.occurrence_at(1).unwrap().instant(),
        instant(u64::MAX)
    );
    for offset in [2, u64::MAX] {
        match recurrence.occurrence_at(offset).unwrap_err() {
            RecurrenceOccurrenceLookupError::OutOfRange {
                recurrence_id,
                requested_offset,
                occurrence_count,
            } => {
                assert_eq!(recurrence_id, id);
                assert_eq!(requested_offset, offset);
                assert_eq!(occurrence_count, count);
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}

#[test]
fn pages_occurrences_in_order_with_complete_provenance_and_a_stable_cursor() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new(" paged ").unwrap();
    let goal = TaskGoal::new("Preserve paged evidence").unwrap();
    let recurrence = RecurrenceStore::open(directory.path().join("events.sqlite3"))
        .unwrap()
        .create(
            id.clone(),
            goal.clone(),
            instant(5),
            ScheduleInterval::from_millis(7).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();

    let first_page = recurrence
        .occurrences_page(0, OccurrencePageSize::new(1).unwrap())
        .unwrap();
    assert_eq!(first_page.next_offset(), Some(1));
    assert_eq!(first_page.occurrences().len(), 1);
    assert_eq!(first_page.occurrences()[0].offset(), 0);
    assert_eq!(first_page.occurrences()[0].instant(), instant(5));

    let page = recurrence
        .occurrences_page(1, OccurrencePageSize::new(2).unwrap())
        .unwrap();

    assert_eq!(page.next_offset(), Some(3));
    assert_eq!(page.occurrences().len(), 2);
    for (occurrence, expected_offset, expected_instant) in [
        (page.occurrences()[0].clone(), 1, 12),
        (page.occurrences()[1].clone(), 2, 19),
    ] {
        assert_eq!(occurrence.recurrence_id(), &id);
        assert_eq!(occurrence.goal(), &goal);
        assert_eq!(occurrence.offset(), expected_offset);
        assert_eq!(occurrence.instant(), instant(expected_instant));
        assert_eq!(occurrence.recurrence_revision(), 1);
    }
}

#[test]
fn truncates_the_final_page_and_rejects_invalid_start_offsets() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new("final-page").unwrap();
    let count = OccurrenceCount::new(3).unwrap();
    let recurrence = RecurrenceStore::open(directory.path().join("events.sqlite3"))
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Finish paging").unwrap(),
            instant(u64::MAX - 2),
            ScheduleInterval::from_millis(1).unwrap(),
            count,
        )
        .unwrap();

    let page = recurrence
        .occurrences_page(1, OccurrencePageSize::new(1024).unwrap())
        .unwrap();
    assert_eq!(
        page.occurrences()
            .iter()
            .map(|occurrence| (occurrence.offset(), occurrence.instant()))
            .collect::<Vec<_>>(),
        vec![(1, instant(u64::MAX - 1)), (2, instant(u64::MAX))]
    );
    assert_eq!(page.next_offset(), None);

    let arithmetic_boundary =
        RecurrenceStore::open(directory.path().join("boundary-events.sqlite3"))
            .unwrap()
            .create(
                RecurrenceId::new("offset-boundary").unwrap(),
                TaskGoal::new("Bound page arithmetic").unwrap(),
                instant(0),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(u64::MAX).unwrap(),
            )
            .unwrap();
    let boundary_page = arithmetic_boundary
        .occurrences_page(u64::MAX - 2, OccurrencePageSize::new(1024).unwrap())
        .unwrap();
    assert_eq!(boundary_page.occurrences().len(), 2);
    assert_eq!(boundary_page.occurrences()[0].offset(), u64::MAX - 2);
    assert_eq!(
        boundary_page.occurrences()[0].instant(),
        instant(u64::MAX - 2)
    );
    assert_eq!(boundary_page.occurrences()[1].offset(), u64::MAX - 1);
    assert_eq!(
        boundary_page.occurrences()[1].instant(),
        instant(u64::MAX - 1)
    );
    assert_eq!(boundary_page.next_offset(), None);

    for start_offset in [3, u64::MAX] {
        assert!(matches!(
            recurrence
                .occurrences_page(start_offset, OccurrencePageSize::new(1).unwrap())
                .unwrap_err(),
            RecurrenceOccurrenceLookupError::OutOfRange {
                recurrence_id,
                requested_offset,
                occurrence_count,
            } if recurrence_id == id
                && requested_offset == start_offset
                && occurrence_count == count
        ));
    }
}

#[test]
fn page_sizes_are_positive_and_bounded_before_allocation() {
    assert_eq!(OccurrencePageSize::MAX, 1024);
    assert_eq!(
        OccurrencePageSize::new(0).unwrap_err(),
        OccurrencePageSizeError::Zero
    );
    assert_eq!(
        OccurrencePageSize::new(1025).unwrap_err(),
        OccurrencePageSizeError::TooLarge {
            requested: 1025,
            maximum: 1024,
        }
    );
    assert_eq!(OccurrencePageSize::new(1024).unwrap().get(), 1024);
}
