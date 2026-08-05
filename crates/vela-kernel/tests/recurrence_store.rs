use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    scheduler::{
        OccurrenceCount, OccurrencePageSize, OccurrencePageSizeError, RecurrenceId,
        RecurrenceOccurrenceLookupError, RecurrenceStore, RecurrenceStoreError, ScheduleInstant,
        ScheduleInterval,
    },
    task::{TaskGoal, TaskId, TaskStore},
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

#[test]
fn pages_due_occurrences_with_an_inclusive_caller_owned_cutoff_and_resume_cursor() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("due-page").unwrap();
    let goal = TaskGoal::new("Select bounded due work").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            goal.clone(),
            instant(5),
            ScheduleInterval::from_millis(7).unwrap(),
            OccurrenceCount::new(5).unwrap(),
        )
        .unwrap();

    let before_horizon = store
        .due_occurrences_page(&id, 0, OccurrencePageSize::new(3).unwrap(), instant(4))
        .unwrap();
    assert!(before_horizon.occurrences().is_empty());
    assert_eq!(before_horizon.next_offset(), Some(0));

    let bounded = store
        .due_occurrences_page(&id, 0, OccurrencePageSize::new(2).unwrap(), instant(19))
        .unwrap();
    assert_eq!(
        bounded
            .occurrences()
            .iter()
            .map(|occurrence| (occurrence.offset(), occurrence.instant()))
            .collect::<Vec<_>>(),
        vec![(0, instant(5)), (1, instant(12))]
    );
    assert_eq!(bounded.next_offset(), Some(2));

    let cutoff_page = store
        .due_occurrences_page(&id, 2, OccurrencePageSize::new(3).unwrap(), instant(19))
        .unwrap();
    assert_eq!(cutoff_page.occurrences().len(), 1);
    assert_eq!(cutoff_page.occurrences()[0].recurrence_id(), &id);
    assert_eq!(cutoff_page.occurrences()[0].goal(), &goal);
    assert_eq!(cutoff_page.occurrences()[0].offset(), 2);
    assert_eq!(cutoff_page.occurrences()[0].instant(), instant(19));
    assert_eq!(cutoff_page.occurrences()[0].recurrence_revision(), 1);
    assert_eq!(cutoff_page.next_offset(), Some(3));

    drop(store);
    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    let resumed = reopened
        .due_occurrences_page(
            &id,
            cutoff_page.next_offset().unwrap(),
            OccurrencePageSize::new(3).unwrap(),
            instant(33),
        )
        .unwrap();
    assert_eq!(
        resumed
            .occurrences()
            .iter()
            .map(|occurrence| (occurrence.offset(), occurrence.instant()))
            .collect::<Vec<_>>(),
        vec![(3, instant(26)), (4, instant(33))]
    );
    assert_eq!(resumed.next_offset(), None);
}

#[test]
fn due_occurrence_pages_preserve_exact_boundaries_and_typed_lookup_failures() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = RecurrenceId::new("due-boundary").unwrap();
    let count = OccurrenceCount::new(2).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Reach the due boundary").unwrap(),
            instant(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            count,
        )
        .unwrap();

    let page = store
        .due_occurrences_page(
            &id,
            0,
            OccurrencePageSize::new(1024).unwrap(),
            instant(u64::MAX),
        )
        .unwrap();
    assert_eq!(
        page.occurrences()
            .iter()
            .map(|occurrence| (occurrence.offset(), occurrence.instant()))
            .collect::<Vec<_>>(),
        vec![(0, instant(u64::MAX - 1)), (1, instant(u64::MAX))]
    );
    assert_eq!(page.next_offset(), None);

    assert!(matches!(
        store
            .due_occurrences_page(
                &id,
                2,
                OccurrencePageSize::new(1).unwrap(),
                instant(u64::MAX),
            )
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id,
            requested_offset: 2,
            occurrence_count,
        } if recurrence_id == id && occurrence_count == count
    ));

    let missing = RecurrenceId::new("missing").unwrap();
    assert!(matches!(
        store
            .due_occurrences_page(
                &missing,
                0,
                OccurrencePageSize::new(1).unwrap(),
                instant(u64::MAX),
            )
            .unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == missing
    ));

    drop(store);
    rusqlite::Connection::open(directory.path().join("events.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'recurrence:due-boundary'",
            [br#"{"goal":"Invalid","anchor_unix_millis":1,"interval_millis":0,"occurrence_count":1}"#.as_slice()],
        )
        .unwrap();
    let error = RecurrenceStore::open_read_only(directory.path().join("events.sqlite3"))
        .unwrap()
        .due_occurrences_page(
            &id,
            0,
            OccurrencePageSize::new(1).unwrap(),
            instant(u64::MAX),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        RecurrenceStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
}

#[test]
fn selects_the_latest_due_occurrence_from_an_explicit_start_without_enumerating_backlog() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("latest-due").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Collapse an explicit backlog").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(8).unwrap(),
        )
        .unwrap();
    drop(store);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    let exact = store.latest_due_occurrence(&id, 2, instant(30)).unwrap();
    let occurrence = exact.occurrence().unwrap();
    assert_eq!(occurrence.recurrence_id(), &id);
    assert_eq!(occurrence.goal().as_str(), "Collapse an explicit backlog");
    assert_eq!(occurrence.offset(), 4);
    assert_eq!(occurrence.instant(), instant(30));
    assert_eq!(occurrence.recurrence_revision(), 1);
    assert_eq!(exact.next_offset(), Some(5));

    let between = store.latest_due_occurrence(&id, 2, instant(33)).unwrap();
    assert_eq!(between.occurrence().unwrap().offset(), 4);
    assert_eq!(between.next_offset(), Some(5));
}

#[test]
fn latest_due_selection_preserves_future_resume_and_finite_boundary_semantics() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("latest-due-boundaries").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Preserve latest-only boundaries").unwrap(),
            instant(u64::MAX - 4),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    drop(store);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    let future = store
        .latest_due_occurrence(&id, 1, instant(u64::MAX - 3))
        .unwrap();
    assert_eq!(future.occurrence(), None);
    assert_eq!(future.next_offset(), Some(1));

    let complete = store
        .latest_due_occurrence(&id, 1, instant(u64::MAX))
        .unwrap();
    assert_eq!(complete.occurrence().unwrap().offset(), 2);
    assert_eq!(complete.occurrence().unwrap().instant(), instant(u64::MAX));
    assert_eq!(complete.next_offset(), None);
}

#[test]
fn latest_due_selection_handles_huge_backlogs_and_typed_lookup_failures() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("huge-latest-due").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    assert!(matches!(
        store
            .latest_due_occurrence(&id, 0, instant(0))
            .unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == id
    ));
    store
        .create(
            id.clone(),
            TaskGoal::new("Select in constant space").unwrap(),
            instant(0),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(u64::MAX).unwrap(),
        )
        .unwrap();

    let selected = store
        .latest_due_occurrence(&id, 7, instant(u64::MAX))
        .unwrap();
    assert_eq!(selected.occurrence().unwrap().offset(), u64::MAX - 1);
    assert_eq!(
        selected.occurrence().unwrap().instant(),
        instant(u64::MAX - 1)
    );
    assert_eq!(selected.next_offset(), None);
    assert!(matches!(
        store
            .latest_due_occurrence(&id, u64::MAX, instant(u64::MAX))
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id,
            requested_offset,
            ..
        } if recurrence_id == id && requested_offset == u64::MAX
    ));
}

#[test]
fn atomically_persists_only_the_latest_due_occurrence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("persist-latest-due").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Persist only the explicit latest choice").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(8).unwrap(),
        )
        .unwrap();

    let selected = store
        .persist_latest_due_occurrence(&id, recurrence.revision(), 2, instant(33))
        .unwrap();
    let occurrence = selected.occurrence().unwrap();
    assert_eq!(occurrence.offset(), 4);
    assert_eq!(occurrence.instant(), instant(30));
    assert_eq!(occurrence.recurrence_revision(), recurrence.revision());
    assert_eq!(selected.next_offset(), Some(5));
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    for skipped in [0, 1, 2, 3] {
        assert_eq!(reopened.load_occurrence(&id, skipped).unwrap(), None);
    }
    assert_eq!(
        reopened.load_occurrence(&id, 4).unwrap().as_ref(),
        selected.occurrence()
    );
}

#[test]
fn latest_due_persistence_preserves_future_and_finite_boundary_semantics() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("persist-latest-boundaries").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Preserve writable latest-only boundaries").unwrap(),
            instant(u64::MAX - 4),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();

    let future = store
        .persist_latest_due_occurrence(&id, recurrence.revision(), 1, instant(u64::MAX - 3))
        .unwrap();
    assert_eq!(future.occurrence(), None);
    assert_eq!(future.next_offset(), Some(1));
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);

    let complete = store
        .persist_latest_due_occurrence(&id, recurrence.revision(), 1, instant(u64::MAX))
        .unwrap();
    assert_eq!(complete.occurrence().unwrap().offset(), 2);
    assert_eq!(complete.occurrence().unwrap().instant(), instant(u64::MAX));
    assert_eq!(complete.next_offset(), None);
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);
    assert_eq!(
        store.load_occurrence(&id, 2).unwrap().as_ref(),
        complete.occurrence()
    );
}

#[test]
fn latest_due_persistence_preserves_typed_preflight_and_duplicate_failures() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = RecurrenceId::new("persist-latest-preflight").unwrap();
    assert!(matches!(
        store
            .persist_latest_due_occurrence(&id, 1, 0, instant(1))
            .unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == id
    ));
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Reject invalid latest persistence").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store
            .persist_latest_due_occurrence(&id, 2, 0, instant(3))
            .unwrap_err(),
        RecurrenceStoreError::ConcurrentModification {
            recurrence_id,
            expected_revision: 2,
            current_revision: 1,
        } if recurrence_id == id
    ));
    assert!(matches!(
        store
            .persist_latest_due_occurrence(&id, recurrence.revision(), 3, instant(3))
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id,
            requested_offset: 3,
            ..
        } if recurrence_id == id
    ));
    store
        .persist_occurrence(&id, recurrence.revision(), 2)
        .unwrap();
    assert!(matches!(
        store
            .persist_latest_due_occurrence(&id, recurrence.revision(), 0, instant(3))
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceAlreadyPersisted {
            recurrence_id,
            offset: 2,
        } if recurrence_id == id
    ));
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);
}

#[test]
fn latest_due_persistence_isolates_skipped_corruption_and_rejects_selected_corruption() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = RecurrenceStore::open(&path).unwrap();
    let skipped_id = RecurrenceId::new("corrupt-skipped-latest").unwrap();
    let selected_id = RecurrenceId::new("corrupt-selected-latest").unwrap();
    for id in [&skipped_id, &selected_id] {
        store
            .create(
                id.clone(),
                TaskGoal::new("Keep exact latest corruption boundaries").unwrap(),
                instant(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(3).unwrap(),
            )
            .unwrap();
    }
    store.persist_occurrence(&skipped_id, 1, 0).unwrap();
    store.persist_occurrence(&selected_id, 1, 2).unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id LIKE 'recurrence-occurrence:%'",
            [br#"{}"#.as_slice()],
        )
        .unwrap();
    drop(connection);

    let mut store = RecurrenceStore::open(&path).unwrap();
    let selected = store
        .persist_latest_due_occurrence(&skipped_id, 1, 1, instant(3))
        .unwrap();
    assert_eq!(selected.occurrence().unwrap().offset(), 2);
    let error = store
        .persist_latest_due_occurrence(&selected_id, 1, 0, instant(3))
        .unwrap_err();
    assert!(
        matches!(
            error,
            RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn atomically_materializes_only_the_latest_due_occurrence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize-latest-due").unwrap();
    let goal = TaskGoal::new("Materialize only the explicit latest choice").unwrap();
    let task_id = TaskId::new("latest-due-task").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            goal.clone(),
            instant(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(8).unwrap(),
        )
        .unwrap();

    let selected = store
        .materialize_latest_due_occurrence(
            &id,
            recurrence.revision(),
            2,
            instant(33),
            task_id.clone(),
        )
        .unwrap();
    let materialized = selected.occurrence().unwrap();
    assert_eq!(materialized.occurrence().offset(), 4);
    assert_eq!(materialized.occurrence().instant(), instant(30));
    assert_eq!(materialized.revision(), 2);
    assert_eq!(materialized.task_id(), &task_id);
    assert_eq!(selected.next_offset(), Some(5));
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    for skipped in [0, 1, 2, 3] {
        assert_eq!(reopened.load_occurrence(&id, skipped).unwrap(), None);
    }
    assert_eq!(
        reopened
            .load_materialized_occurrence(&id, 4)
            .unwrap()
            .as_ref(),
        selected.occurrence()
    );
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .goal(),
        &goal
    );
}

#[test]
fn latest_due_materialization_preserves_write_free_future_and_finite_boundaries() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize-latest-boundaries").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Preserve latest materialization boundaries").unwrap(),
            instant(u64::MAX - 4),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let future_task = TaskId::new("future-latest-task").unwrap();

    let future = store
        .materialize_latest_due_occurrence(
            &id,
            recurrence.revision(),
            1,
            instant(u64::MAX - 3),
            future_task.clone(),
        )
        .unwrap();
    assert_eq!(future.occurrence(), None);
    assert_eq!(future.next_offset(), Some(1));
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&future_task).unwrap(),
        None
    );

    let final_task = TaskId::new("final-latest-task").unwrap();
    let complete = store
        .materialize_latest_due_occurrence(
            &id,
            recurrence.revision(),
            1,
            instant(u64::MAX),
            final_task,
        )
        .unwrap();
    assert_eq!(complete.occurrence().unwrap().occurrence().offset(), 2);
    assert_eq!(
        complete.occurrence().unwrap().occurrence().instant(),
        instant(u64::MAX)
    );
    assert_eq!(complete.next_offset(), None);
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);
}

#[test]
fn latest_due_materialization_failures_leave_occurrence_and_task_absent() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize-latest-failures").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    assert!(matches!(
        store
            .materialize_latest_due_occurrence(
                &id,
                1,
                0,
                instant(1),
                TaskId::new("missing-definition-task").unwrap(),
            )
            .unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == id
    ));
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Reject partial latest materialization").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let existing_task = TaskId::new("existing-latest-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(existing_task.clone(), TaskGoal::new("Existing").unwrap())
        .unwrap();

    assert!(matches!(
        store
            .materialize_latest_due_occurrence(
                &id,
                recurrence.revision() + 1,
                0,
                instant(3),
                TaskId::new("stale-definition-task").unwrap(),
            )
            .unwrap_err(),
        RecurrenceStoreError::ConcurrentModification { .. }
    ));
    assert!(matches!(
        store
            .materialize_latest_due_occurrence(
                &id,
                recurrence.revision(),
                3,
                instant(3),
                TaskId::new("out-of-range-task").unwrap(),
            )
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            requested_offset: 3,
            ..
        }
    ));
    assert!(matches!(
        store
            .materialize_latest_due_occurrence(
                &id,
                recurrence.revision(),
                0,
                instant(3),
                existing_task.clone(),
            )
            .unwrap_err(),
        RecurrenceStoreError::TaskAlreadyExists { task_id } if task_id == existing_task
    ));
    assert_eq!(store.load_occurrence(&id, 2).unwrap(), None);

    store
        .persist_occurrence(&id, recurrence.revision(), 2)
        .unwrap();
    let duplicate_task = TaskId::new("duplicate-latest-task").unwrap();
    assert!(matches!(
        store
            .materialize_latest_due_occurrence(
                &id,
                recurrence.revision(),
                0,
                instant(3),
                duplicate_task.clone(),
            )
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceAlreadyPersisted { offset: 2, .. }
    ));
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&duplicate_task)
            .unwrap(),
        None
    );

    let bound_task = TaskId::new("bound-latest-task").unwrap();
    store.materialize_occurrence(&id, 2, 1, bound_task).unwrap();
    let replacement_task = TaskId::new("replacement-latest-task").unwrap();
    assert!(matches!(
        store
            .materialize_latest_due_occurrence(
                &id,
                recurrence.revision(),
                0,
                instant(3),
                replacement_task.clone(),
            )
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceAlreadyPersisted { offset: 2, .. }
    ));
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&replacement_task)
            .unwrap(),
        None
    );
}

#[test]
fn latest_due_materialization_rejects_selected_corruption_without_creating_a_task() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("corrupt-latest-materialization").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Reject corrupt selected provenance").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 2).unwrap();
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'00' WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();

    let task_id = TaskId::new("corrupt-selected-task").unwrap();
    let error = RecurrenceStore::open(&path)
        .unwrap()
        .materialize_latest_due_occurrence(&id, 1, 0, instant(3), task_id.clone())
        .unwrap_err();
    assert!(
        matches!(
            error,
            RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&task_id).unwrap(),
        None
    );
}

#[test]
fn atomically_materializes_one_bounded_due_occurrence_page() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize-due-page").unwrap();
    let goal = TaskGoal::new("Materialize bounded due work").unwrap();
    let task_ids = [
        TaskId::new("due-page-task-0").unwrap(),
        TaskId::new("due-page-task-1").unwrap(),
    ];
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            goal.clone(),
            instant(5),
            ScheduleInterval::from_millis(7).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();

    let page = store
        .materialize_due_occurrences_page(
            &id,
            recurrence.revision(),
            0,
            OccurrencePageSize::new(2).unwrap(),
            instant(19),
            task_ids.to_vec(),
        )
        .unwrap();
    assert_eq!(
        page.occurrences()
            .iter()
            .map(|binding| (
                binding.occurrence().offset(),
                binding.occurrence().instant(),
                binding.revision(),
                binding.task_id().clone(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, instant(5), 2, task_ids[0].clone()),
            (1, instant(12), 2, task_ids[1].clone()),
        ]
    );
    assert_eq!(page.next_offset(), Some(2));
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    assert_eq!(
        reopened
            .materialized_occurrences_page(&id, 0, OccurrencePageSize::new(2).unwrap())
            .unwrap(),
        page
    );
    for (offset, task_id) in task_ids.iter().enumerate() {
        assert_eq!(
            reopened
                .find_materialized_by_task_id(task_id)
                .unwrap()
                .unwrap()
                .occurrence()
                .offset(),
            offset as u64
        );
        assert_eq!(
            TaskStore::open(&path)
                .unwrap()
                .load(task_id)
                .unwrap()
                .unwrap()
                .goal(),
            &goal
        );
    }
}

#[test]
fn due_page_materialization_rejects_task_shape_without_writing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize-due-page-shape").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Reject malformed task assignment").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let duplicate = TaskId::new("duplicate-page-task").unwrap();

    assert!(matches!(
        store
            .materialize_due_occurrences_page(
                &id,
                recurrence.revision(),
                0,
                OccurrencePageSize::new(2).unwrap(),
                instant(2),
                vec![duplicate.clone()],
            )
            .unwrap_err(),
        RecurrenceStoreError::TaskCountMismatch {
            occurrence_count: 2,
            task_count: 1,
        }
    ));
    assert!(matches!(
        store
            .materialize_due_occurrences_page(
                &id,
                recurrence.revision(),
                0,
                OccurrencePageSize::new(2).unwrap(),
                instant(2),
                vec![duplicate.clone(), duplicate.clone()],
            )
            .unwrap_err(),
        RecurrenceStoreError::DuplicateTaskId { task_id } if task_id == duplicate
    ));
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);
    assert_eq!(
        TaskStore::open(&path).unwrap().load(&duplicate).unwrap(),
        None
    );

    let future = store
        .materialize_due_occurrences_page(
            &id,
            recurrence.revision(),
            2,
            OccurrencePageSize::new(1).unwrap(),
            instant(2),
            Vec::new(),
        )
        .unwrap();
    assert!(future.occurrences().is_empty());
    assert_eq!(future.next_offset(), Some(2));
}

#[test]
fn due_page_materialization_rejects_any_selected_or_task_collision_atomically() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize-due-page-collision").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Reject every partial page").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store
        .persist_occurrence(&id, recurrence.revision(), 1)
        .unwrap();
    let selected_tasks = (0..3)
        .map(|offset| TaskId::new(format!("selected-collision-task-{offset}")).unwrap())
        .collect::<Vec<_>>();

    assert!(matches!(
        store
            .materialize_due_occurrences_page(
                &id,
                recurrence.revision(),
                0,
                OccurrencePageSize::new(3).unwrap(),
                instant(3),
                selected_tasks.clone(),
            )
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceAlreadyPersisted { offset: 1, .. }
    ));
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 2).unwrap(), None);
    for task_id in &selected_tasks {
        assert_eq!(TaskStore::open(&path).unwrap().load(task_id).unwrap(), None);
    }

    let task_collision_id = RecurrenceId::new("materialize-task-collision").unwrap();
    let task_collision = TaskId::new("existing-page-task").unwrap();
    let collision_recurrence = store
        .create(
            task_collision_id.clone(),
            TaskGoal::new("Reject an existing task").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            task_collision.clone(),
            TaskGoal::new("Existing task").unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store
            .materialize_due_occurrences_page(
                &task_collision_id,
                collision_recurrence.revision(),
                0,
                OccurrencePageSize::new(2).unwrap(),
                instant(2),
                vec![
                    TaskId::new("new-page-task").unwrap(),
                    task_collision.clone(),
                ],
            )
            .unwrap_err(),
        RecurrenceStoreError::TaskAlreadyExists { task_id } if task_id == task_collision
    ));
    assert_eq!(store.load_occurrence(&task_collision_id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&task_collision_id, 1).unwrap(), None);
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&TaskId::new("new-page-task").unwrap())
            .unwrap(),
        None
    );
}

#[test]
fn racing_due_page_materialization_commits_one_complete_page() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("racing-due-page-materialization").unwrap();
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Materialize one complete racing page").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|actor| {
            let path = path.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let task_ids = (0..3)
                    .map(|offset| {
                        TaskId::new(format!("racing-page-task-{actor}-{offset}")).unwrap()
                    })
                    .collect::<Vec<_>>();
                let mut store = RecurrenceStore::open(path).unwrap();
                barrier.wait();
                (
                    actor,
                    task_ids.clone(),
                    store.materialize_due_occurrences_page(
                        &id,
                        1,
                        0,
                        OccurrencePageSize::new(3).unwrap(),
                        instant(3),
                        task_ids,
                    ),
                )
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|(_, _, result)| result.is_ok())
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|(_, _, result)| matches!(
                result,
                Err(RecurrenceStoreError::OccurrenceAlreadyPersisted { .. })
            ))
            .count(),
        1
    );

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    let winner = results
        .iter()
        .find(|(_, _, result)| result.is_ok())
        .unwrap()
        .0;
    for (actor, task_ids, _) in results {
        for task_id in task_ids {
            assert_eq!(
                TaskStore::open(&path)
                    .unwrap()
                    .load(&task_id)
                    .unwrap()
                    .is_some(),
                actor == winner
            );
        }
    }
    assert_eq!(
        reopened
            .materialized_occurrences_page(&id, 0, OccurrencePageSize::new(3).unwrap())
            .unwrap()
            .occurrences()
            .len(),
        3
    );
}

#[test]
fn atomically_persists_one_bounded_due_occurrence_page() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("due-persistence").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Persist bounded due work").unwrap(),
            instant(5),
            ScheduleInterval::from_millis(7).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();

    let first = store
        .persist_due_occurrences_page(
            &id,
            recurrence.revision(),
            0,
            OccurrencePageSize::new(2).unwrap(),
            instant(19),
        )
        .unwrap();
    assert_eq!(
        first
            .occurrences()
            .iter()
            .map(|occurrence| (occurrence.offset(), occurrence.instant()))
            .collect::<Vec<_>>(),
        vec![(0, instant(5)), (1, instant(12))]
    );
    assert_eq!(first.next_offset(), Some(2));

    let final_page = store
        .persist_due_occurrences_page(
            &id,
            recurrence.revision(),
            first.next_offset().unwrap(),
            OccurrencePageSize::new(4).unwrap(),
            instant(26),
        )
        .unwrap();
    assert_eq!(
        final_page
            .occurrences()
            .iter()
            .map(|occurrence| (occurrence.offset(), occurrence.instant()))
            .collect::<Vec<_>>(),
        vec![(2, instant(19)), (3, instant(26))]
    );
    assert_eq!(final_page.next_offset(), None);
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    for offset in 0..4 {
        let occurrence = reopened.load_occurrence(&id, offset).unwrap().unwrap();
        assert_eq!(occurrence.offset(), offset);
        assert_eq!(occurrence.recurrence_revision(), recurrence.revision());
    }
}

#[test]
fn due_page_persistence_rejects_existing_coordinates_atomically() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new("atomic-due-persistence").unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Persist all or nothing").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    store
        .persist_occurrence(&id, recurrence.revision(), 1)
        .unwrap();

    let error = store
        .persist_due_occurrences_page(
            &id,
            recurrence.revision(),
            0,
            OccurrencePageSize::new(3).unwrap(),
            instant(3),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        RecurrenceStoreError::OccurrenceAlreadyPersisted {
            recurrence_id,
            offset: 1,
        } if recurrence_id == id
    ));
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 2).unwrap(), None);
}

#[test]
fn due_page_persistence_rejects_selected_corruption_without_a_partial_append() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("corrupt-due-persistence").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Fail closed before the batch").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store
        .persist_occurrence(&id, recurrence.revision(), 1)
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = ?1 WHERE event_type = 'recurrence.occurrence_persisted'",
            [br#"{}"#.as_slice()],
        )
        .unwrap();

    let error = store
        .persist_due_occurrences_page(
            &id,
            recurrence.revision(),
            0,
            OccurrencePageSize::new(2).unwrap(),
            instant(2),
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
}

#[test]
fn due_page_persistence_preserves_preflight_failures_and_future_horizons() {
    let directory = tempdir().unwrap();
    let id = RecurrenceId::new("due-preflight").unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            TaskGoal::new("Keep policy caller owned").unwrap(),
            instant(u64::MAX - 1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();

    let future = store
        .persist_due_occurrences_page(
            &id,
            recurrence.revision(),
            0,
            OccurrencePageSize::new(2).unwrap(),
            instant(u64::MAX - 2),
        )
        .unwrap();
    assert!(future.occurrences().is_empty());
    assert_eq!(future.next_offset(), Some(0));

    assert!(matches!(
        store
            .persist_due_occurrences_page(
                &id,
                recurrence.revision() + 1,
                0,
                OccurrencePageSize::new(2).unwrap(),
                instant(u64::MAX),
            )
            .unwrap_err(),
        RecurrenceStoreError::ConcurrentModification {
            recurrence_id,
            expected_revision: 2,
            current_revision: 1,
        } if recurrence_id == id
    ));
    assert!(matches!(
        store
            .persist_due_occurrences_page(
                &id,
                recurrence.revision(),
                2,
                OccurrencePageSize::new(1).unwrap(),
                instant(u64::MAX),
            )
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id,
            requested_offset: 2,
            ..
        } if recurrence_id == id
    ));
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 1).unwrap(), None);

    let boundary = store
        .persist_due_occurrences_page(
            &id,
            recurrence.revision(),
            0,
            OccurrencePageSize::new(2).unwrap(),
            instant(u64::MAX),
        )
        .unwrap();
    assert_eq!(
        boundary
            .occurrences()
            .iter()
            .map(|occurrence| occurrence.instant())
            .collect::<Vec<_>>(),
        vec![instant(u64::MAX - 1), instant(u64::MAX)]
    );
    assert_eq!(boundary.next_offset(), None);
}

#[test]
fn persists_and_reopens_exact_recurrence_occurrence_provenance() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new(" interval:東京 ").unwrap();
    let goal = TaskGoal::new("Preserve exact durable provenance").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    let recurrence = store
        .create(
            id.clone(),
            goal.clone(),
            instant(u64::MAX - 2),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();

    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 2).unwrap(), None);
    let first = store
        .persist_occurrence(&id, recurrence.revision(), 0)
        .unwrap();
    let persisted = store
        .persist_occurrence(&id, recurrence.revision(), 2)
        .unwrap();
    assert_eq!(first.offset(), 0);
    assert_eq!(first.instant(), instant(u64::MAX - 2));
    assert_eq!(persisted.recurrence_id(), &id);
    assert_eq!(persisted.goal(), &goal);
    assert_eq!(persisted.offset(), 2);
    assert_eq!(persisted.instant(), instant(u64::MAX));
    assert_eq!(persisted.recurrence_revision(), recurrence.revision());
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    assert_eq!(reopened.load_occurrence(&id, 0).unwrap(), Some(first));
    assert_eq!(reopened.load_occurrence(&id, 2).unwrap(), Some(persisted));
}

#[test]
fn atomically_materializes_one_persisted_occurrence_as_an_inert_task() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialize:東京").unwrap();
    let goal = TaskGoal::new("Run the exact recurring task").unwrap();
    let task_id = TaskId::new("recurrence-task").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            goal.clone(),
            instant(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    let occurrence = store.persist_occurrence(&id, 1, 1).unwrap();

    let materialized = store
        .materialize_occurrence(&id, 1, 1, task_id.clone())
        .unwrap();

    assert_eq!(materialized.occurrence(), &occurrence);
    assert_eq!(materialized.revision(), 2);
    assert_eq!(materialized.task_id(), &task_id);
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    assert_eq!(reopened.load_occurrence(&id, 1).unwrap(), Some(occurrence));
    assert_eq!(
        reopened.load_materialized_occurrence(&id, 1).unwrap(),
        Some(materialized)
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.goal(), &goal);
}

#[test]
fn resolves_materialized_recurrence_provenance_by_exact_task_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("task-provenance:東京").unwrap();
    let task_id = TaskId::new("exact recurrence task").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Resolve exact provenance").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(5).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    let materialized = store
        .materialize_occurrence(&id, 1, 1, task_id.clone())
        .unwrap();
    drop(store);

    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    assert_eq!(
        reopened.find_materialized_by_task_id(&task_id).unwrap(),
        Some(materialized)
    );
    assert_eq!(
        reopened
            .find_materialized_by_task_id(&TaskId::new("unrelated").unwrap())
            .unwrap(),
        None
    );
}

#[test]
fn recurrence_task_provenance_rejects_ambiguous_durable_bindings() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let duplicate_task_id = TaskId::new("duplicate-binding").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    for (id, task_id) in [
        ("first", duplicate_task_id.clone()),
        ("second", TaskId::new("second-binding").unwrap()),
    ] {
        let id = RecurrenceId::new(id).unwrap();
        store
            .create(
                id.clone(),
                TaskGoal::new("Detect ambiguity").unwrap(),
                instant(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
        store.persist_occurrence(&id, 1, 0).unwrap();
        store.materialize_occurrence(&id, 0, 1, task_id).unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = CAST(json_object('task_id', ?1) AS BLOB)
             WHERE event_type = 'recurrence.occurrence_materialized' AND json_extract(payload, '$.task_id') = 'second-binding'",
            [duplicate_task_id.as_str()],
        )
        .unwrap();

    let error = RecurrenceStore::open_read_only(&path)
        .unwrap()
        .find_materialized_by_task_id(&duplicate_task_id)
        .unwrap_err();
    assert!(
        matches!(
            error,
            RecurrenceStoreError::AmbiguousTaskBinding {
                ref task_id,
                occurrence_count: 2
            } if task_id == &duplicate_task_id
        ),
        "{error:?}"
    );
}

#[test]
fn recurrence_task_provenance_validates_candidates_and_isolates_unrelated_corruption() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let selected_id = RecurrenceId::new("selected").unwrap();
    let selected_task_id = TaskId::new("selected-task").unwrap();
    let unrelated_id = RecurrenceId::new("unrelated").unwrap();
    let unrelated_task_id = TaskId::new("unrelated-task").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    for (id, task_id) in [
        (&selected_id, selected_task_id.clone()),
        (&unrelated_id, unrelated_task_id),
    ] {
        store
            .create(
                id.clone(),
                TaskGoal::new(format!("Goal for {id}")).unwrap(),
                instant(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(1).unwrap(),
            )
            .unwrap();
        store.persist_occurrence(id, 1, 0).unwrap();
        store.materialize_occurrence(id, 0, 1, task_id).unwrap();
    }
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = X'00'
             WHERE event_type = 'recurrence.occurrence_materialized' AND json_extract(payload, '$.task_id') = 'unrelated-task'",
            [],
        )
        .unwrap();

    assert!(
        RecurrenceStore::open_read_only(&path)
            .unwrap()
            .find_materialized_by_task_id(&selected_task_id)
            .unwrap()
            .is_some()
    );

    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(payload, '$.goal', 'Divergent goal') AS BLOB)
             WHERE event_type = 'recurrence.occurrence_persisted' AND json_extract(payload, '$.recurrence_id') = 'selected'",
            [],
        )
        .unwrap();
    assert!(matches!(
        RecurrenceStore::open_read_only(&path)
            .unwrap()
            .find_materialized_by_task_id(&selected_task_id)
            .unwrap_err(),
        RecurrenceStoreError::InvalidOccurrenceHistory { event_count: 2, .. }
    ));
}

#[test]
fn occurrence_materialization_failures_leave_both_streams_unchanged() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialization-failures").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Materialize once").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    let existing_task = TaskId::new("existing-task").unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(existing_task.clone(), TaskGoal::new("Existing").unwrap())
        .unwrap();

    assert!(matches!(
        store
            .materialize_occurrence(&id, 1, 1, TaskId::new("missing-task").unwrap())
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceNotFound { recurrence_id, offset: 1 }
            if recurrence_id == id
    ));
    assert!(matches!(
        store
            .materialize_occurrence(&id, 0, 0, TaskId::new("stale-task").unwrap())
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceConcurrentModification {
            recurrence_id, offset: 0, expected_revision: 0, current_revision: 1,
        } if recurrence_id == id
    ));
    assert!(matches!(
        store
            .materialize_occurrence(&id, 0, 1, existing_task.clone())
            .unwrap_err(),
        RecurrenceStoreError::TaskAlreadyExists { task_id } if task_id == existing_task
    ));
    assert_eq!(store.load_materialized_occurrence(&id, 0).unwrap(), None);

    let task_id = TaskId::new("first-binding").unwrap();
    store
        .materialize_occurrence(&id, 0, 1, task_id.clone())
        .unwrap();
    assert!(matches!(
        store
            .materialize_occurrence(&id, 0, 2, TaskId::new("replacement").unwrap())
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceAlreadyMaterialized {
            recurrence_id, offset: 0, task_id: bound,
        } if recurrence_id == id && bound == task_id
    ));
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&TaskId::new("replacement").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn materialized_occurrence_replay_rejects_invalid_task_binding_payloads() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("corrupt-materialization").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Reject corrupt binding").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 0).unwrap();
    store
        .materialize_occurrence(&id, 0, 1, TaskId::new("bound-task").unwrap())
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(CAST(payload AS TEXT), '$.goal', 'Corrupt') AS BLOB)
             WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    let error = RecurrenceStore::open_read_only(&path)
        .unwrap()
        .load_materialized_occurrence(&id, 0)
        .unwrap_err();
    assert!(
        matches!(
            error,
            RecurrenceStoreError::InvalidOccurrenceHistory { event_count: 2, .. }
        ),
        "{error:?}"
    );
    connection
        .execute(
            "UPDATE events SET payload = CAST(json_set(CAST(payload AS TEXT), '$.goal', 'Reject corrupt binding') AS BLOB)
             WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE event_type = 'recurrence.occurrence_materialized'",
            [br#"{"task_id":""}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        RecurrenceStore::open_read_only(&path)
            .unwrap()
            .load_materialized_occurrence(&id, 0)
            .unwrap_err(),
        RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
}

#[test]
fn pages_sparse_persisted_occurrences_by_authored_offset_window() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("sparse-page").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Inspect sparse durable provenance").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(7).unwrap(),
        )
        .unwrap();
    for offset in [0, 2, 5, 6] {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    drop(store);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    let first = store
        .persisted_occurrences_page(&id, 0, OccurrencePageSize::new(3).unwrap())
        .unwrap();
    assert_eq!(
        first
            .occurrences()
            .iter()
            .map(|occurrence| occurrence.offset())
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(first.next_offset(), Some(3));

    let gap = store
        .persisted_occurrences_page(&id, 3, OccurrencePageSize::new(2).unwrap())
        .unwrap();
    assert!(gap.occurrences().is_empty());
    assert_eq!(gap.next_offset(), Some(5));

    let final_page = store
        .persisted_occurrences_page(&id, 5, OccurrencePageSize::new(3).unwrap())
        .unwrap();
    assert_eq!(
        final_page
            .occurrences()
            .iter()
            .map(|occurrence| occurrence.offset())
            .collect::<Vec<_>>(),
        vec![5, 6]
    );
    assert_eq!(final_page.next_offset(), None);
}

#[test]
fn persisted_occurrence_pages_reject_missing_and_out_of_range_starts() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let missing = RecurrenceId::new("missing-page").unwrap();
    assert!(matches!(
        store
            .persisted_occurrences_page(
                &missing,
                0,
                OccurrencePageSize::new(1).unwrap(),
            )
            .unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == missing
    ));

    let id = RecurrenceId::new("bounded-page").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Reject invalid starts").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store
            .persisted_occurrences_page(&id, 2, OccurrencePageSize::new(1).unwrap())
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id,
            requested_offset: 2,
            ..
        } if recurrence_id == id
    ));
}

#[test]
fn persisted_occurrence_pages_fail_closed_only_for_the_selected_window() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("page-corruption").unwrap();
    let unrelated_id = RecurrenceId::new("unrelated-page-corruption").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    for recurrence_id in [&id, &unrelated_id] {
        store
            .create(
                recurrence_id.clone(),
                TaskGoal::new(format!("Validate {recurrence_id}")).unwrap(),
                instant(10),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(4).unwrap(),
            )
            .unwrap();
    }
    for offset in 0..4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    store.persist_occurrence(&unrelated_id, 1, 0).unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    for (recurrence_id, offset) in [(&id, 3_u64), (&unrelated_id, 0_u64)] {
        connection
            .execute(
                "UPDATE events SET payload_version = 2
                 WHERE event_type = 'recurrence.occurrence_persisted'
                   AND json_extract(CAST(payload AS TEXT), '$.recurrence_id') = ?1
                   AND json_extract(CAST(payload AS TEXT), '$.offset') = ?2",
                rusqlite::params![recurrence_id.as_str(), offset],
            )
            .unwrap();
    }
    drop(connection);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    let isolated = store
        .persisted_occurrences_page(&id, 0, OccurrencePageSize::new(3).unwrap())
        .unwrap();
    assert_eq!(isolated.occurrences().len(), 3);
    assert_eq!(isolated.next_offset(), Some(3));
    assert!(matches!(
        store
            .persisted_occurrences_page(&id, 3, OccurrencePageSize::new(1).unwrap())
            .unwrap_err(),
        RecurrenceStoreError::Replay(ReplayError::UnsupportedEvent { .. })
    ));
}

#[test]
fn pages_sparse_materialized_occurrences_by_authored_offset_window() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("sparse-materialized-page").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Inspect sparse materialized provenance").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(7).unwrap(),
        )
        .unwrap();
    for offset in [0, 1, 2, 5, 6] {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    for (offset, task_id) in [
        (0, "task-zero"),
        (2, "task-two"),
        (5, "task-five"),
        (6, "task-six"),
    ] {
        store
            .materialize_occurrence(&id, offset, 1, TaskId::new(task_id).unwrap())
            .unwrap();
    }
    drop(store);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    let first = store
        .materialized_occurrences_page(&id, 0, OccurrencePageSize::new(3).unwrap())
        .unwrap();
    assert_eq!(
        first
            .occurrences()
            .iter()
            .map(|materialized| (
                materialized.occurrence().offset(),
                materialized.revision(),
                materialized.task_id().as_str(),
            ))
            .collect::<Vec<_>>(),
        vec![(0, 2, "task-zero"), (2, 2, "task-two")]
    );
    assert_eq!(first.next_offset(), Some(3));

    let gap = store
        .materialized_occurrences_page(&id, 3, OccurrencePageSize::new(2).unwrap())
        .unwrap();
    assert!(gap.occurrences().is_empty());
    assert_eq!(gap.next_offset(), Some(5));

    let final_page = store
        .materialized_occurrences_page(&id, 5, OccurrencePageSize::new(1024).unwrap())
        .unwrap();
    assert_eq!(
        final_page
            .occurrences()
            .iter()
            .map(|materialized| materialized.occurrence().offset())
            .collect::<Vec<_>>(),
        vec![5, 6]
    );
    assert_eq!(final_page.next_offset(), None);
}

#[test]
fn materialized_occurrence_pages_reject_missing_and_out_of_range_starts() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let missing = RecurrenceId::new("missing-materialized-page").unwrap();
    assert!(matches!(
        store
            .materialized_occurrences_page(
                &missing,
                0,
                OccurrencePageSize::new(1).unwrap(),
            )
            .unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == missing
    ));

    let id = RecurrenceId::new("bounded-materialized-page").unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Reject invalid materialized starts").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store
            .materialized_occurrences_page(&id, 2, OccurrencePageSize::new(1).unwrap())
            .unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id,
            requested_offset: 2,
            ..
        } if recurrence_id == id
    ));
}

#[test]
fn materialized_occurrence_pages_fail_closed_only_for_the_selected_window() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("materialized-page-corruption").unwrap();
    let unrelated_id = RecurrenceId::new("unrelated-materialized-page-corruption").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    for recurrence_id in [&id, &unrelated_id] {
        store
            .create(
                recurrence_id.clone(),
                TaskGoal::new(format!("Validate {recurrence_id}")).unwrap(),
                instant(10),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(4).unwrap(),
            )
            .unwrap();
    }
    for offset in 0..4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
        store
            .materialize_occurrence(
                &id,
                offset,
                1,
                TaskId::new(format!("selected-task-{offset}")).unwrap(),
            )
            .unwrap();
    }
    store.persist_occurrence(&unrelated_id, 1, 0).unwrap();
    store
        .materialize_occurrence(&unrelated_id, 0, 1, TaskId::new("unrelated-task").unwrap())
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    for task_id in ["selected-task-3", "unrelated-task"] {
        assert_eq!(
            connection
                .execute(
                    "UPDATE events SET payload_version = 2
                     WHERE event_type = 'recurrence.occurrence_materialized'
                       AND json_extract(CAST(payload AS TEXT), '$.task_id') = ?1",
                    [task_id],
                )
                .unwrap(),
            1
        );
    }
    drop(connection);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    let isolated = store
        .materialized_occurrences_page(&id, 0, OccurrencePageSize::new(3).unwrap())
        .unwrap();
    assert_eq!(isolated.occurrences().len(), 3);
    assert_eq!(isolated.next_offset(), Some(3));
    assert!(matches!(
        store
            .materialized_occurrences_page(&id, 3, OccurrencePageSize::new(1).unwrap())
            .unwrap_err(),
        RecurrenceStoreError::Replay(ReplayError::UnsupportedEvent { .. })
    ));
}

#[test]
fn occurrence_coordinates_are_collision_free_and_duplicates_are_preserved() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let first_id = RecurrenceId::new("a:1").unwrap();
    let second_id = RecurrenceId::new("a").unwrap();
    for id in [&first_id, &second_id] {
        store
            .create(
                id.clone(),
                TaskGoal::new(format!("goal {id}")).unwrap(),
                instant(1),
                ScheduleInterval::from_millis(1).unwrap(),
                OccurrenceCount::new(2).unwrap(),
            )
            .unwrap();
    }

    let first = store.persist_occurrence(&first_id, 1, 0).unwrap();
    let second = store.persist_occurrence(&second_id, 1, 1).unwrap();
    assert_ne!(first, second);
    let connection = rusqlite::Connection::open(directory.path().join("events.sqlite3")).unwrap();
    let mut key_statement = connection
        .prepare(
            "SELECT stream_id FROM events
             WHERE event_type = 'recurrence.occurrence_persisted' ORDER BY stream_id",
        )
        .unwrap();
    let keys = key_statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        keys,
        vec![
            "recurrence-occurrence:1:a:1".to_owned(),
            "recurrence-occurrence:3:a:1:0".to_owned(),
        ]
    );
    drop(key_statement);
    drop(connection);
    assert!(matches!(
        store.persist_occurrence(&first_id, 1, 0).unwrap_err(),
        RecurrenceStoreError::OccurrenceAlreadyPersisted { recurrence_id, offset: 0 }
            if recurrence_id == first_id
    ));
    assert_eq!(store.load_occurrence(&first_id, 0).unwrap(), Some(first));
    assert_eq!(store.load_occurrence(&second_id, 1).unwrap(), Some(second));
}

#[test]
fn racing_occurrence_persistence_records_one_exact_coordinate() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("racing-occurrence").unwrap();
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Persist once").unwrap(),
            instant(4),
            ScheduleInterval::from_millis(3).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = RecurrenceStore::open(path).unwrap();
                barrier.wait();
                store.persist_occurrence(&id, 1, 1)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(RecurrenceStoreError::OccurrenceAlreadyPersisted {
                    recurrence_id,
                    offset: 1,
                }) if recurrence_id == &id
            ))
            .count(),
        1
    );
    assert_eq!(
        RecurrenceStore::open_read_only(&path)
            .unwrap()
            .load_occurrence(&id, 1)
            .unwrap()
            .unwrap()
            .instant(),
        instant(7)
    );
}

#[test]
fn racing_due_page_persistence_commits_one_complete_page() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("racing-due-page").unwrap();
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Persist one complete page").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = RecurrenceStore::open(path).unwrap();
                barrier.wait();
                store.persist_due_occurrences_page(
                    &id,
                    1,
                    0,
                    OccurrencePageSize::new(3).unwrap(),
                    instant(3),
                )
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(RecurrenceStoreError::OccurrenceAlreadyPersisted {
                    recurrence_id,
                    offset: 0,
                }) if recurrence_id == &id
            ))
            .count(),
        1
    );
    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    for offset in 0..3 {
        assert_eq!(
            reopened
                .load_occurrence(&id, offset)
                .unwrap()
                .unwrap()
                .offset(),
            offset
        );
    }
}

#[test]
fn racing_latest_due_persistence_records_one_selected_coordinate() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("racing-latest-due").unwrap();
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Persist one latest choice").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = RecurrenceStore::open(path).unwrap();
                barrier.wait();
                store.persist_latest_due_occurrence(&id, 1, 0, instant(3))
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(RecurrenceStoreError::OccurrenceAlreadyPersisted {
                    recurrence_id,
                    offset: 2,
                }) if recurrence_id == &id
            ))
            .count(),
        1
    );
    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    assert_eq!(reopened.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(reopened.load_occurrence(&id, 1).unwrap(), None);
    assert_eq!(
        reopened.load_occurrence(&id, 2).unwrap().unwrap().instant(),
        instant(3)
    );
}

#[test]
fn racing_latest_due_materializations_commit_one_complete_binding() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("racing-latest-materialization").unwrap();
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Materialize one latest choice").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(3).unwrap(),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = ["latest-race-task-a", "latest-race-task-b"].map(|raw_task_id| {
        let path = path.clone();
        let id = id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut store = RecurrenceStore::open(path).unwrap();
            barrier.wait();
            store.materialize_latest_due_occurrence(
                &id,
                1,
                0,
                instant(3),
                TaskId::new(raw_task_id).unwrap(),
            )
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(RecurrenceStoreError::OccurrenceAlreadyPersisted {
                    recurrence_id,
                    offset: 2,
                }) if recurrence_id == &id
            ))
            .count(),
        1
    );
    let materialized = RecurrenceStore::open_read_only(&path)
        .unwrap()
        .load_materialized_occurrence(&id, 2)
        .unwrap()
        .unwrap();
    assert_eq!(materialized.revision(), 2);
    for raw_task_id in ["latest-race-task-a", "latest-race-task-b"] {
        let task_id = TaskId::new(raw_task_id).unwrap();
        assert_eq!(
            TaskStore::open(&path)
                .unwrap()
                .load(&task_id)
                .unwrap()
                .is_some(),
            materialized.task_id() == &task_id
        );
    }
}

#[test]
fn racing_occurrence_materializations_commit_one_complete_binding() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("racing-materialization").unwrap();
    let mut setup = RecurrenceStore::open(&path).unwrap();
    setup
        .create(
            id.clone(),
            TaskGoal::new("Materialize exactly once").unwrap(),
            instant(4),
            ScheduleInterval::from_millis(3).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();
    setup.persist_occurrence(&id, 1, 0).unwrap();
    drop(setup);

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = ["race-task-a", "race-task-b"].map(|raw_task_id| {
        let path = path.clone();
        let id = id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut store = RecurrenceStore::open(path).unwrap();
            barrier.wait();
            store.materialize_occurrence(&id, 0, 1, TaskId::new(raw_task_id).unwrap())
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let winner = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .unwrap();
    let reopened = RecurrenceStore::open_read_only(&path).unwrap();
    assert_eq!(
        reopened
            .load_materialized_occurrence(&id, 0)
            .unwrap()
            .unwrap(),
        *winner
    );
    for raw_task_id in ["race-task-a", "race-task-b"] {
        let task_id = TaskId::new(raw_task_id).unwrap();
        assert_eq!(
            TaskStore::open(&path)
                .unwrap()
                .load(&task_id)
                .unwrap()
                .is_some(),
            winner.task_id() == &task_id
        );
    }
}

#[test]
fn rejects_missing_stale_and_out_of_range_occurrences_before_persistence() {
    let directory = tempdir().unwrap();
    let mut store = RecurrenceStore::open(directory.path().join("events.sqlite3")).unwrap();
    let id = RecurrenceId::new("bounded-persistence").unwrap();

    assert!(matches!(
        store.persist_occurrence(&id, 1, 0).unwrap_err(),
        RecurrenceStoreError::NotFound { recurrence_id } if recurrence_id == id
    ));
    store
        .create(
            id.clone(),
            TaskGoal::new("Persist only authored coordinates").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store.persist_occurrence(&id, 0, 0).unwrap_err(),
        RecurrenceStoreError::ConcurrentModification {
            recurrence_id, expected_revision: 0, current_revision: 1,
        } if recurrence_id == id
    ));
    assert!(matches!(
        store.persist_occurrence(&id, 1, 2).unwrap_err(),
        RecurrenceStoreError::OccurrenceOutOfRange {
            recurrence_id, requested_offset: 2, ..
        } if recurrence_id == id
    ));
    assert_eq!(store.load_occurrence(&id, 0).unwrap(), None);
    assert_eq!(store.load_occurrence(&id, 2).unwrap(), None);
}

#[test]
fn read_only_recurrence_store_cannot_persist_occurrences() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("read-only-occurrence").unwrap();
    RecurrenceStore::open(&path)
        .unwrap()
        .create(
            id.clone(),
            TaskGoal::new("Retain read-only authority").unwrap(),
            instant(1),
            ScheduleInterval::from_millis(1).unwrap(),
            OccurrenceCount::new(1).unwrap(),
        )
        .unwrap();

    let mut read_only = RecurrenceStore::open_read_only(&path).unwrap();
    assert!(matches!(
        read_only.persist_occurrence(&id, 1, 0).unwrap_err(),
        RecurrenceStoreError::EventLog(_)
    ));
    assert_eq!(read_only.load_occurrence(&id, 0).unwrap(), None);
}

#[test]
fn exact_occurrence_replay_rejects_multiple_event_histories() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("corrupt-occurrence").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Strict replay").unwrap(),
            instant(3),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(2).unwrap(),
        )
        .unwrap();
    store.persist_occurrence(&id, 1, 1).unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let stream_id: String = connection
        .query_row(
            "SELECT stream_id FROM events WHERE event_type = 'recurrence.occurrence_persisted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    connection.execute(
        "INSERT INTO events (stream_id, stream_version, event_type, payload_version, payload)
         VALUES (?1, 2, 'recurrence.occurrence_persisted', 1, ?2)",
        rusqlite::params![
            stream_id,
            br#"{"recurrence_id":"corrupt-occurrence","recurrence_revision":1,"offset":1,"goal":"Strict replay","unix_millis":5}"#.as_slice()
        ],
    ).unwrap();

    assert!(matches!(
        RecurrenceStore::open_read_only(&path)
            .unwrap()
            .load_occurrence(&id, 1)
            .unwrap_err(),
        RecurrenceStoreError::InvalidOccurrenceHistory {
            recurrence_id,
            offset: 1,
            event_count: 2,
        } if recurrence_id == id
    ));
}

#[test]
fn exact_occurrence_replay_rejects_event_and_payload_corruption() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = RecurrenceId::new("strict-occurrences").unwrap();
    let mut store = RecurrenceStore::open(&path).unwrap();
    store
        .create(
            id.clone(),
            TaskGoal::new("Validate every field").unwrap(),
            instant(10),
            ScheduleInterval::from_millis(2).unwrap(),
            OccurrenceCount::new(4).unwrap(),
        )
        .unwrap();
    for offset in 0..4 {
        store.persist_occurrence(&id, 1, offset).unwrap();
    }
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let mut streams = connection
        .prepare(
            "SELECT stream_id FROM events WHERE event_type = 'recurrence.occurrence_persisted'
             ORDER BY json_extract(CAST(payload AS TEXT), '$.offset')",
        )
        .unwrap();
    let stream_ids = streams
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    drop(streams);
    connection
        .execute(
            "UPDATE events SET event_type = 'recurrence.unknown' WHERE stream_id = ?1",
            [&stream_ids[0]],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = ?1",
            [&stream_ids[1]],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = ?2",
            rusqlite::params![
                br#"{"recurrence_id":"strict-occurrences","recurrence_revision":1,"offset":2,"goal":"Validate every field","unix_millis":14,"unexpected":true}"#.as_slice(),
                &stream_ids[2],
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = ?2",
            rusqlite::params![
                br#"{"recurrence_id":"strict-occurrences","recurrence_revision":1,"offset":99,"goal":"Validate every field","unix_millis":16}"#.as_slice(),
                &stream_ids[3],
            ],
        )
        .unwrap();
    drop(connection);

    let store = RecurrenceStore::open_read_only(&path).unwrap();
    for offset in [0, 1] {
        assert!(matches!(
            store.load_occurrence(&id, offset).unwrap_err(),
            RecurrenceStoreError::Replay(ReplayError::UnsupportedEvent { .. })
        ));
    }
    assert!(matches!(
        store.load_occurrence(&id, 2).unwrap_err(),
        RecurrenceStoreError::Replay(ReplayError::MalformedPayload { .. })
    ));
    assert!(matches!(
        store.load_occurrence(&id, 3).unwrap_err(),
        RecurrenceStoreError::InvalidOccurrenceHistory {
            recurrence_id,
            offset: 3,
            event_count: 1,
        } if recurrence_id == id
    ));
}
