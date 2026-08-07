use std::sync::{Arc, Barrier};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    scheduler::{
        ScheduleCancellation, ScheduleHistoryEvent, ScheduleId, ScheduleInstant, ScheduleInterval,
        SchedulePageSize, SchedulePageSizeError, ScheduleRelease, ScheduleStatus, ScheduleStore,
        ScheduleStoreError,
    },
    task::{TaskGoal, TaskId, TaskStore},
};

fn instant(unix_millis: u64) -> ScheduleInstant {
    ScheduleInstant::from_unix_millis(unix_millis)
}

#[test]
fn fixed_intervals_advance_instants_exactly_and_fail_closed_on_overflow() {
    assert_eq!(
        ScheduleInterval::from_millis(0).unwrap_err().to_string(),
        "schedule interval must be greater than zero milliseconds"
    );
    let interval = ScheduleInterval::from_millis(7).unwrap();
    assert_eq!(interval.millis(), 7);
    assert_eq!(instant(5).checked_advance(interval).unwrap(), instant(12));

    let maximum_boundary = ScheduleInterval::from_millis(u64::MAX - 5).unwrap();
    assert_eq!(
        instant(5).checked_advance(maximum_boundary).unwrap(),
        instant(u64::MAX)
    );

    let overflow_interval = ScheduleInterval::from_millis(2).unwrap();
    let error = instant(u64::MAX)
        .checked_advance(overflow_interval)
        .unwrap_err();
    assert_eq!(error.instant(), instant(u64::MAX));
    assert_eq!(error.interval(), overflow_interval);
    assert_eq!(
        error.to_string(),
        "schedule instant 18446744073709551615 cannot advance by 2 milliseconds"
    );
}

#[test]
fn indexed_fixed_intervals_derive_exact_instants_and_preserve_overflow_evidence() {
    let interval = ScheduleInterval::from_millis(7).unwrap();
    assert_eq!(
        instant(5).checked_advance_by(interval, 0).unwrap(),
        instant(5)
    );
    assert_eq!(
        instant(5).checked_advance_by(interval, 1).unwrap(),
        instant(5).checked_advance(interval).unwrap()
    );
    assert_eq!(
        instant(5).checked_advance_by(interval, 3).unwrap(),
        instant(26)
    );

    let maximum_offset = (u64::MAX - 5) / 2;
    assert_eq!(
        instant(5)
            .checked_advance_by(ScheduleInterval::from_millis(2).unwrap(), maximum_offset)
            .unwrap(),
        instant(u64::MAX)
    );

    let multiplication_interval = ScheduleInterval::from_millis(u64::MAX).unwrap();
    let multiplication_error = instant(0)
        .checked_advance_by(multiplication_interval, 2)
        .unwrap_err();
    assert_eq!(multiplication_error.instant(), instant(0));
    assert_eq!(multiplication_error.interval(), multiplication_interval);
    assert_eq!(multiplication_error.offset(), 2);

    let addition_interval = ScheduleInterval::from_millis(1).unwrap();
    let addition_error = instant(u64::MAX)
        .checked_advance_by(addition_interval, 1)
        .unwrap_err();
    assert_eq!(addition_error.instant(), instant(u64::MAX));
    assert_eq!(addition_error.interval(), addition_interval);
    assert_eq!(addition_error.offset(), 1);
    assert_eq!(
        addition_error.to_string(),
        "schedule instant 18446744073709551615 cannot advance by interval 1 milliseconds at offset 1"
    );
}

#[test]
fn schedule_ids_require_content_without_normalizing_exact_values() {
    assert!(ScheduleId::new(" \t").is_err());
    let exact = ScheduleId::new(" Morning ").unwrap();
    assert_eq!(exact.as_str(), " Morning ");
}

#[test]
fn read_only_store_projects_existing_schedules_and_rejects_mutation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("read-only-schedule").unwrap();
    let goal = TaskGoal::new("Inspect without lifecycle authority").unwrap();
    let mut writer = ScheduleStore::open(&path).unwrap();
    let scheduled = writer.schedule(id.clone(), goal, instant(42)).unwrap();
    drop(writer);

    let mut reader = ScheduleStore::open_read_only(&path).unwrap();
    assert_eq!(reader.load(&id).unwrap(), Some(scheduled.clone()));
    assert_eq!(reader.list().unwrap(), std::slice::from_ref(&scheduled));
    assert_eq!(reader.history(&id).unwrap().unwrap().len(), 1);
    assert!(matches!(
        reader
            .cancel(
                &id,
                scheduled.revision(),
                ScheduleCancellation::new("must not persist").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::EventLog(vela_kernel::event_log::EventLogError::Storage(_))
    ));
    drop(reader);

    let reopened = ScheduleStore::open(&path).unwrap();
    assert_eq!(reopened.load(&id).unwrap(), Some(scheduled));
    assert_eq!(reopened.history(&id).unwrap().unwrap().len(), 1);
}

#[test]
fn queries_complete_typed_schedule_history_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("history").unwrap();
    let goal = TaskGoal::new("Preserve every transition").unwrap();
    let release = ScheduleRelease::new(" first worker stopped ").unwrap();
    let task_id = TaskId::new("history-task").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(id.clone(), goal.clone(), instant(42))
        .unwrap();
    store.claim(&id, 1, instant(42)).unwrap();
    store.release(&id, 2, release.clone()).unwrap();
    store.claim(&id, 3, instant(42)).unwrap();
    store.materialize(&id, 4, task_id.clone()).unwrap();
    drop(store);

    let history = ScheduleStore::open(&path)
        .unwrap()
        .history(&id)
        .unwrap()
        .unwrap();

    assert_eq!(
        history
            .iter()
            .map(|entry| entry.revision())
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(
        history
            .iter()
            .map(|entry| entry.event().clone())
            .collect::<Vec<_>>(),
        [
            ScheduleHistoryEvent::Created {
                goal,
                due_at: instant(42),
            },
            ScheduleHistoryEvent::Claimed,
            ScheduleHistoryEvent::Released { reason: release },
            ScheduleHistoryEvent::Claimed,
            ScheduleHistoryEvent::Materialized { task_id },
        ]
    );
}

#[test]
fn schedule_history_preserves_cancellation_and_returns_none_for_missing_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("cancelled-history").unwrap();
    let missing = ScheduleId::new("missing-history").unwrap();
    let goal = TaskGoal::new("Withdraw this schedule").unwrap();
    let reason = ScheduleCancellation::new(" exact withdrawal ").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(id.clone(), goal.clone(), instant(7))
        .unwrap();
    store.cancel(&id, 1, reason.clone()).unwrap();

    assert_eq!(
        store
            .history(&id)
            .unwrap()
            .unwrap()
            .iter()
            .map(|entry| (entry.revision(), entry.event().clone()))
            .collect::<Vec<_>>(),
        [
            (
                1,
                ScheduleHistoryEvent::Created {
                    goal,
                    due_at: instant(7),
                },
            ),
            (2, ScheduleHistoryEvent::Cancelled { reason }),
        ]
    );
    assert_eq!(store.history(&missing).unwrap(), None);
}

#[test]
fn finds_materialized_schedule_by_exact_task_id_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let schedule_id = ScheduleId::new("task-provenance").unwrap();
    let task_id = TaskId::new("materialized-task").unwrap();
    let unrelated_task_id = TaskId::new("unrelated-task").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            schedule_id.clone(),
            TaskGoal::new("Preserve schedule provenance").unwrap(),
            instant(42),
        )
        .unwrap();
    store.claim(&schedule_id, 1, instant(42)).unwrap();
    let materialized = store.materialize(&schedule_id, 2, task_id.clone()).unwrap();
    drop(store);

    let reopened = ScheduleStore::open(&path).unwrap();
    assert_eq!(
        reopened.find_by_task_id(&task_id).unwrap(),
        Some(materialized)
    );
    assert_eq!(reopened.find_by_task_id(&unrelated_task_id).unwrap(), None);
}

#[test]
fn task_lookup_fails_closed_for_malformed_history_and_duplicate_bindings() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("duplicate-task-binding").unwrap();
    let first_id = ScheduleId::new("first-binding").unwrap();
    let second_id = ScheduleId::new("second-binding").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in [&first_id, &second_id] {
        store
            .schedule(
                id.clone(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(1),
            )
            .unwrap();
        store.claim(id, 1, instant(1)).unwrap();
    }
    store.materialize(&first_id, 2, task_id.clone()).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:second-binding', 3, 'schedule.materialized', 1, ?1)",
            [br#"{"task_id":"duplicate-task-binding"}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        store.find_by_task_id(&task_id).unwrap_err(),
        ScheduleStoreError::AmbiguousTaskBinding {
            task_id: ref duplicate,
            schedule_count: 2,
        } if duplicate == &task_id
    ));

    connection
        .execute(
            "UPDATE events SET payload = X'7B7D'
             WHERE stream_id = 'schedule:second-binding' AND stream_version = 3",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.find_by_task_id(&task_id).unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));
}

#[test]
fn schedule_history_rejects_invalid_lifecycle_without_returning_a_prefix() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("invalid-history").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Reject partial evidence").unwrap(),
            instant(1),
        )
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:invalid-history', 2, 'schedule.released', 1, ?1)",
            [br#"{"reason":"release before claim"}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        store.history(&id).unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 2 }
    ));

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D'
             WHERE stream_id = 'schedule:invalid-history' AND stream_version = 2",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.history(&id).unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));
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
    assert_eq!(scheduled.status(), ScheduleStatus::Pending);
    assert_eq!(scheduled.cancellation(), None);
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
fn cancels_pending_schedule_with_exact_reason_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("cancel-me").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Withdraw this intent").unwrap(),
            instant(10),
        )
        .unwrap();

    let cancelled = store
        .cancel(
            &id,
            1,
            ScheduleCancellation::new(" Superseded by operator ").unwrap(),
        )
        .unwrap();

    assert_eq!(cancelled.status(), ScheduleStatus::Cancelled);
    assert_eq!(
        cancelled.cancellation().unwrap().as_str(),
        " Superseded by operator "
    );
    assert_eq!(
        ScheduleStore::open(&path).unwrap().load(&id).unwrap(),
        Some(cancelled)
    );
}

#[test]
fn racing_cancellations_persist_exactly_one_terminal_reason() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("cancel-race").unwrap();
    ScheduleStore::open(&path)
        .unwrap()
        .schedule(
            id.clone(),
            TaskGoal::new("Choose one cancellation").unwrap(),
            instant(10),
        )
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let handles = ["first", "second"].map(|reason| {
        let path = path.clone();
        let id = id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.cancel(&id, 1, ScheduleCancellation::new(reason).unwrap())
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(ScheduleStoreError::ConcurrentModification {
                        expected_revision: 1,
                        current_revision: 2,
                        ..
                    })
                )
            })
            .count(),
        1
    );
    let loaded = ScheduleStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status(), ScheduleStatus::Cancelled);
    assert!(matches!(
        loaded.cancellation().unwrap().as_str(),
        "first" | "second"
    ));
}

#[test]
fn claims_due_schedule_and_excludes_it_from_due_work_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("claim-me").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Reserve this intent").unwrap(),
            instant(10),
        )
        .unwrap();

    let claimed = store.claim(&id, 1, instant(10)).unwrap();

    assert_eq!(claimed.status(), ScheduleStatus::Claimed);
    assert_eq!(
        ScheduleStore::open(&path).unwrap().load(&id).unwrap(),
        Some(claimed)
    );
    assert!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(u64::MAX))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_claiming_future_missing_or_terminal_schedules_without_rewriting_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let pending_id = ScheduleId::new("future").unwrap();
    let cancelled_id = ScheduleId::new("cancelled").unwrap();
    let missing_id = ScheduleId::new("missing").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    let pending = store
        .schedule(
            pending_id.clone(),
            TaskGoal::new("Wait until due").unwrap(),
            instant(11),
        )
        .unwrap();
    store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("Never claim").unwrap(),
            instant(1),
        )
        .unwrap();
    let cancelled = store
        .cancel(
            &cancelled_id,
            1,
            ScheduleCancellation::new("Withdrawn").unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store.claim(&pending_id, 1, instant(10)).unwrap_err(),
        ScheduleStoreError::NotDue {
            schedule_id,
            due_at,
            cutoff,
        } if schedule_id == pending_id && due_at == instant(11) && cutoff == instant(10)
    ));
    assert_eq!(store.load(&pending_id).unwrap(), Some(pending));
    assert!(matches!(
        store.claim(&missing_id, 1, instant(u64::MAX)).unwrap_err(),
        ScheduleStoreError::NotFound { schedule_id } if schedule_id == missing_id
    ));
    assert!(matches!(
        store
            .claim(&cancelled_id, 1, instant(u64::MAX))
            .unwrap_err(),
        ScheduleStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 2,
            ..
        }
    ));
    assert!(matches!(
        store
            .claim(&cancelled_id, 2, instant(u64::MAX))
            .unwrap_err(),
        ScheduleStoreError::AlreadyCancelled { schedule_id } if schedule_id == cancelled_id
    ));
    assert_eq!(store.load(&cancelled_id).unwrap(), Some(cancelled));

    let claimed = store.claim(&pending_id, 1, instant(11)).unwrap();
    assert!(matches!(
        store
            .cancel(
                &pending_id,
                1,
                ScheduleCancellation::new("stale withdrawal").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 2,
            ..
        }
    ));
    assert!(matches!(
        store.claim(&pending_id, 2, instant(11)).unwrap_err(),
        ScheduleStoreError::AlreadyClaimed { schedule_id } if schedule_id == pending_id
    ));
    assert!(matches!(
        store
            .cancel(
                &pending_id,
                2,
                ScheduleCancellation::new("too late").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::AlreadyClaimed { schedule_id } if schedule_id == pending_id
    ));
    assert_eq!(store.load(&pending_id).unwrap(), Some(claimed));
}

#[test]
fn racing_claims_persist_exactly_one_claim() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("claim-race").unwrap();
    ScheduleStore::open(&path)
        .unwrap()
        .schedule(id.clone(), TaskGoal::new("Claim once").unwrap(), instant(1))
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = [(), ()].map(|()| {
        let path = path.clone();
        let id = id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.claim(&id, 1, instant(1))
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(ScheduleStoreError::ConcurrentModification {
                        expected_revision: 1,
                        current_revision: 2,
                        ..
                    })
                )
            })
            .count(),
        1
    );
    assert_eq!(
        ScheduleStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap()
            .unwrap()
            .status(),
        ScheduleStatus::Claimed
    );
}

#[test]
fn racing_claim_and_cancellation_commit_one_terminal_transition() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("terminal-race").unwrap();
    ScheduleStore::open(&path)
        .unwrap()
        .schedule(
            id.clone(),
            TaskGoal::new("Choose one terminal state").unwrap(),
            instant(1),
        )
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let claim = {
        let path = path.clone();
        let id = id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.claim(&id, 1, instant(1))
        })
    };
    let cancel = {
        let path = path.clone();
        let id = id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.cancel(
                &id,
                1,
                ScheduleCancellation::new("operator withdrew").unwrap(),
            )
        })
    };
    let results = [claim.join().unwrap(), cancel.join().unwrap()];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let loaded = ScheduleStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert!(matches!(
        loaded.status(),
        ScheduleStatus::Claimed | ScheduleStatus::Cancelled
    ));
    assert!(results.iter().any(|result| matches!(
        result,
        Err(ScheduleStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 2,
            ..
        })
    )));
}

#[test]
fn replay_rejects_malformed_claim_payload_and_transition_after_claim() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("malformed-claim").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Reject corrupt claim").unwrap(),
            instant(1),
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:malformed-claim', 2, 'schedule.claimed', 1, ?1)",
            [br#"{"unexpected":true}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        store.load(&id).unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));

    connection
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'schedule:malformed-claim' AND stream_version = 2",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:malformed-claim', 3, 'schedule.cancelled', 1, ?1)",
            [br#"{"reason":"too late"}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store.load(&id).unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 3 }
    ));
}

#[test]
fn cancellation_requires_content_and_reports_missing_or_terminal_schedules() {
    assert!(ScheduleCancellation::new(" \n").is_err());
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("terminal").unwrap();
    let missing = ScheduleId::new("missing").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();

    assert!(matches!(
        store
            .cancel(
                &missing,
                1,
                ScheduleCancellation::new("No intent").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::NotFound { schedule_id } if schedule_id == missing
    ));
    store
        .schedule(
            id.clone(),
            TaskGoal::new("One terminal transition").unwrap(),
            instant(10),
        )
        .unwrap();
    let original = store
        .cancel(&id, 1, ScheduleCancellation::new("First reason").unwrap())
        .unwrap();

    assert!(matches!(
        store
            .cancel(
                &id,
                2,
                ScheduleCancellation::new("Replacement").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::AlreadyCancelled { schedule_id } if schedule_id == id
    ));
    assert_eq!(store.load(&id).unwrap(), Some(original));

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:terminal', 3, 'schedule.cancelled', 1, ?1)",
            [br#"{"reason":"impossible duplicate"}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store.load(&id).unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 3 }
    ));
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
    store
        .cancel(
            &ScheduleId::new("beta").unwrap(),
            1,
            ScheduleCancellation::new("Do not run").unwrap(),
        )
        .unwrap();
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
        ["early", "zeta"]
    );
}

#[test]
fn empty_store_has_no_due_intents() {
    let directory = tempdir().unwrap();
    let store = ScheduleStore::open(directory.path().join("events.sqlite3")).unwrap();

    assert!(store.list_due(instant(u64::MAX)).unwrap().is_empty());
}

#[test]
fn lists_every_schedule_in_exact_id_order_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in ["zeta", "alpha", "middle"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(10),
            )
            .unwrap();
    }
    store
        .cancel(
            &ScheduleId::new("zeta").unwrap(),
            1,
            ScheduleCancellation::new(" exact cancellation ").unwrap(),
        )
        .unwrap();
    store
        .claim(&ScheduleId::new("middle").unwrap(), 1, instant(10))
        .unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            TaskId::new("not-a-schedule").unwrap(),
            TaskGoal::new("Ignore this stream").unwrap(),
        )
        .unwrap();

    let schedules = ScheduleStore::open(&path).unwrap().list().unwrap();

    assert_eq!(
        schedules
            .iter()
            .map(|schedule| (schedule.id().as_str(), schedule.status()))
            .collect::<Vec<_>>(),
        [
            ("alpha", ScheduleStatus::Pending),
            ("middle", ScheduleStatus::Claimed),
            ("zeta", ScheduleStatus::Cancelled),
        ]
    );
    assert_eq!(
        schedules[2].cancellation().unwrap().as_str(),
        " exact cancellation "
    );
}

#[test]
fn schedule_page_sizes_are_positive_and_bounded() {
    assert_eq!(
        SchedulePageSize::new(0).unwrap_err(),
        SchedulePageSizeError::Zero
    );
    assert_eq!(SchedulePageSize::new(1).unwrap().get(), 1);
    assert_eq!(
        SchedulePageSize::new(SchedulePageSize::MAX).unwrap().get(),
        SchedulePageSize::MAX
    );
    assert_eq!(
        SchedulePageSize::new(SchedulePageSize::MAX + 1).unwrap_err(),
        SchedulePageSizeError::TooLarge {
            requested: SchedulePageSize::MAX + 1,
            maximum: SchedulePageSize::MAX,
        }
    );
}

#[test]
fn pages_complete_schedules_by_exclusive_exact_id_cursor() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    let mut authored = Vec::new();
    for id in ["zeta", "alpha", "middle"] {
        let schedule = store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("Goal for {id}")).unwrap(),
                instant(10),
            )
            .unwrap();
        authored.push(if id == "middle" {
            store
                .cancel(
                    schedule.id(),
                    schedule.revision(),
                    ScheduleCancellation::new("paged cancellation").unwrap(),
                )
                .unwrap()
        } else {
            schedule
        });
    }
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('unrelated', 1, 'other.created', 1, '{}')",
            [],
        )
        .unwrap();

    authored.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
    let store = ScheduleStore::open_read_only(&path).unwrap();
    let first = store
        .list_page(None, SchedulePageSize::new(2).unwrap())
        .unwrap();
    assert_eq!(first.schedules(), &authored[..2]);
    assert_eq!(first.next_after(), Some(authored[1].id()));

    let second = store
        .list_page(first.next_after(), SchedulePageSize::new(2).unwrap())
        .unwrap();
    assert_eq!(second.schedules(), &authored[2..]);
    assert_eq!(second.next_after(), None);

    let between = ScheduleId::new("bravo").unwrap();
    let from_nonexistent = store
        .list_page(Some(&between), SchedulePageSize::new(1).unwrap())
        .unwrap();
    assert_eq!(from_nonexistent.schedules(), &authored[1..2]);
    assert_eq!(from_nonexistent.next_after(), Some(authored[1].id()));

    let beyond = ScheduleId::new("zzzz").unwrap();
    let terminal = store
        .list_page(Some(&beyond), SchedulePageSize::new(1).unwrap())
        .unwrap();
    assert!(terminal.schedules().is_empty());
    assert_eq!(terminal.next_after(), None);
}

#[test]
fn schedule_pages_fail_closed_only_for_the_bounded_selected_window() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in ["alpha", "bravo", "charlie", "delta"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("Goal for {id}")).unwrap(),
                instant(10),
            )
            .unwrap();
    }
    drop(store);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:delta', 2, 'schedule.created', 1, ?1)",
            [br#"{"goal":"Duplicate","due_at_unix_millis":2}"#.as_slice()],
        )
        .unwrap();
    let store = ScheduleStore::open_read_only(&path).unwrap();
    let first = store
        .list_page(None, SchedulePageSize::new(1).unwrap())
        .unwrap();
    assert_eq!(first.schedules()[0].id().as_str(), "alpha");
    assert_eq!(first.next_after().unwrap().as_str(), "alpha");

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:alpha', 2, 'schedule.created', 1, ?1)",
            [br#"{"goal":"Duplicate","due_at_unix_millis":2}"#.as_slice()],
        )
        .unwrap();
    let after_corrupt_prefix = store
        .list_page(first.next_after(), SchedulePageSize::new(1).unwrap())
        .unwrap();
    assert_eq!(after_corrupt_prefix.schedules()[0].id().as_str(), "bravo");

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:charlie', 2, 'schedule.created', 1, ?1)",
            [br#"{"goal":"Duplicate","due_at_unix_millis":2}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store
            .list_page(first.next_after(), SchedulePageSize::new(1).unwrap())
            .unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 2 }
    ));
}

#[test]
fn filters_schedules_by_exact_status_in_id_order_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in [
        "pending-z",
        "cancelled",
        "claimed",
        "pending-a",
        "materialized",
    ] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(10),
            )
            .unwrap();
    }
    store
        .cancel(
            &ScheduleId::new("cancelled").unwrap(),
            1,
            ScheduleCancellation::new("withdrawn").unwrap(),
        )
        .unwrap();
    store
        .claim(&ScheduleId::new("claimed").unwrap(), 1, instant(10))
        .unwrap();
    store
        .claim(&ScheduleId::new("materialized").unwrap(), 1, instant(10))
        .unwrap();
    store
        .materialize(
            &ScheduleId::new("materialized").unwrap(),
            2,
            TaskId::new("scheduled-task").unwrap(),
        )
        .unwrap();
    drop(store);

    let store = ScheduleStore::open(&path).unwrap();
    let ids_for = |status| {
        store
            .list_by_status(status)
            .unwrap()
            .iter()
            .map(|schedule| schedule.id().as_str().to_owned())
            .collect::<Vec<_>>()
    };

    assert_eq!(ids_for(ScheduleStatus::Pending), ["pending-a", "pending-z"]);
    assert_eq!(ids_for(ScheduleStatus::Claimed), ["claimed"]);
    assert_eq!(ids_for(ScheduleStatus::Cancelled), ["cancelled"]);
    assert_eq!(ids_for(ScheduleStatus::Materialized), ["materialized"]);
}

#[test]
fn status_filter_returns_empty_when_no_schedules_match() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ScheduleStore::open(&path)
        .unwrap()
        .schedule(
            ScheduleId::new("pending").unwrap(),
            TaskGoal::new("still pending").unwrap(),
            instant(10),
        )
        .unwrap();

    assert!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_by_status(ScheduleStatus::Cancelled)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn empty_store_has_no_schedules() {
    let directory = tempdir().unwrap();
    let store = ScheduleStore::open(directory.path().join("events.sqlite3")).unwrap();

    assert!(store.list().unwrap().is_empty());
    let page = store
        .list_page(None, SchedulePageSize::new(1).unwrap())
        .unwrap();
    assert!(page.schedules().is_empty());
    assert_eq!(page.next_after(), None);
}

#[test]
fn schedule_discovery_rejects_malformed_creation_payload_and_owning_stream_id() {
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
        ScheduleStore::open(&path).unwrap().list().unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_by_status(ScheduleStatus::Cancelled)
            .unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
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
        ScheduleStore::open(&path).unwrap().list().unwrap_err(),
        ScheduleStoreError::InvalidStreamId { ref stream_id } if stream_id == "schedule:"
    ));
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_by_status(ScheduleStatus::Pending)
            .unwrap_err(),
        ScheduleStoreError::InvalidStreamId { ref stream_id } if stream_id == "schedule:"
    ));
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(7))
            .unwrap_err(),
        ScheduleStoreError::InvalidStreamId { ref stream_id } if stream_id == "schedule:"
    ));
}

#[test]
fn schedule_discovery_rejects_duplicate_creation_history() {
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
        ScheduleStore::open(&path).unwrap().list().unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 2 }
    ));
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(u64::MAX))
            .unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 2 }
    ));
}

#[test]
fn replay_rejects_cancellation_before_creation_and_blank_reasons() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ScheduleStore::open(&path).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:cancelled-first', 1, 'schedule.cancelled', 1, ?1)",
            [br#"{"reason":"impossible"}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .load(&ScheduleId::new("cancelled-first").unwrap())
            .unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 1 }
    ));

    let mut store = ScheduleStore::open(&path).unwrap();
    let blank = ScheduleId::new("blank-reason").unwrap();
    store
        .schedule(
            blank.clone(),
            TaskGoal::new("Reject malformed reason").unwrap(),
            instant(1),
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:blank-reason', 2, 'schedule.cancelled', 1, ?1)",
            [br#"{"reason":"  "}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .load(&blank)
            .unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 2,
            ..
        })
    ));
}

#[test]
fn releases_claimed_schedule_back_to_due_work_with_exact_recovery_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("recover-me").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Recover abandoned reservation").unwrap(),
            instant(10),
        )
        .unwrap();
    store.claim(&id, 1, instant(10)).unwrap();

    let released = store
        .release(
            &id,
            2,
            ScheduleRelease::new(" worker stopped before dispatch ").unwrap(),
        )
        .unwrap();

    assert_eq!(released.status(), ScheduleStatus::Pending);
    assert_eq!(
        released.latest_release().unwrap().as_str(),
        " worker stopped before dispatch "
    );
    let reopened = ScheduleStore::open(&path).unwrap();
    assert_eq!(reopened.load(&id).unwrap(), Some(released.clone()));
    assert_eq!(reopened.list().unwrap(), vec![released.clone()]);
    assert_eq!(reopened.list_due(instant(10)).unwrap(), vec![released]);
}

#[test]
fn released_schedule_can_be_claimed_again_without_changing_intent() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("retry-claim").unwrap();
    let goal = TaskGoal::new("Keep immutable intent").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(id.clone(), goal.clone(), instant(7))
        .unwrap();
    store.claim(&id, 1, instant(7)).unwrap();
    store
        .release(&id, 2, ScheduleRelease::new("retry reservation").unwrap())
        .unwrap();

    let reclaimed = store.claim(&id, 3, instant(7)).unwrap();

    assert_eq!(reclaimed.status(), ScheduleStatus::Claimed);
    assert_eq!(reclaimed.goal(), &goal);
    assert_eq!(reclaimed.due_at(), instant(7));
    assert_eq!(
        reclaimed.latest_release().unwrap().as_str(),
        "retry reservation"
    );
    assert_eq!(
        ScheduleStore::open(&path).unwrap().load(&id).unwrap(),
        Some(reclaimed)
    );

    let released_again = store
        .release(&id, 4, ScheduleRelease::new("second recovery").unwrap())
        .unwrap();
    let cancelled = store
        .cancel(
            &id,
            5,
            ScheduleCancellation::new("stop after recovery").unwrap(),
        )
        .unwrap();
    assert_eq!(cancelled.status(), ScheduleStatus::Cancelled);
    assert_eq!(
        cancelled.latest_release().unwrap().as_str(),
        "second recovery"
    );
    assert_eq!(cancelled.goal(), released_again.goal());
    assert_eq!(cancelled.due_at(), released_again.due_at());
}

#[test]
fn stale_pending_observer_cannot_cancel_after_claim_release_cycle() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("stale-pending-cancel").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    let pending = store
        .schedule(
            id.clone(),
            TaskGoal::new("Preserve newer recovery state").unwrap(),
            instant(1),
        )
        .unwrap();
    let claimed = store.claim(&id, pending.revision(), instant(1)).unwrap();
    let released = store
        .release(
            &id,
            claimed.revision(),
            ScheduleRelease::new("newer recovery").unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store
            .cancel(
                &id,
                pending.revision(),
                ScheduleCancellation::new("stale withdrawal").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 3,
            ..
        }
    ));
    assert_eq!(store.load(&id).unwrap(), Some(released));
}

#[test]
fn stale_pending_observer_cannot_claim_after_claim_release_cycle() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("stale-pending-claim").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    let pending = store
        .schedule(
            id.clone(),
            TaskGoal::new("Preserve newer claim eligibility").unwrap(),
            instant(1),
        )
        .unwrap();
    let claimed = store.claim(&id, pending.revision(), instant(1)).unwrap();
    let released = store
        .release(
            &id,
            claimed.revision(),
            ScheduleRelease::new("newer recovery").unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store
            .claim(&id, pending.revision(), instant(0))
            .unwrap_err(),
        ScheduleStoreError::ConcurrentModification {
            expected_revision: 1,
            current_revision: 3,
            ..
        }
    ));
    assert_eq!(store.load(&id).unwrap(), Some(released));
}

#[test]
fn release_rejects_blank_reasons_and_non_claimed_schedules_without_appending() {
    assert!(ScheduleRelease::new(" \n").is_err());
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let pending_id = ScheduleId::new("pending-release").unwrap();
    let cancelled_id = ScheduleId::new("cancelled-release").unwrap();
    let missing_id = ScheduleId::new("missing-release").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    let pending = store
        .schedule(
            pending_id.clone(),
            TaskGoal::new("Remain pending").unwrap(),
            instant(1),
        )
        .unwrap();
    store
        .schedule(
            cancelled_id.clone(),
            TaskGoal::new("Remain cancelled").unwrap(),
            instant(1),
        )
        .unwrap();
    let cancelled = store
        .cancel(
            &cancelled_id,
            1,
            ScheduleCancellation::new("withdrawn").unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store
            .release(
                &missing_id,
                1,
                ScheduleRelease::new("recover").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::NotFound { schedule_id } if schedule_id == missing_id
    ));
    assert!(matches!(
        store
            .release(
                &pending_id,
                1,
                ScheduleRelease::new("recover").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::NotClaimed { schedule_id } if schedule_id == pending_id
    ));
    assert!(matches!(
        store
            .release(
                &cancelled_id,
                2,
                ScheduleRelease::new("recover").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::AlreadyCancelled { schedule_id } if schedule_id == cancelled_id
    ));
    assert_eq!(store.load(&pending_id).unwrap(), Some(pending));
    assert_eq!(store.load(&cancelled_id).unwrap(), Some(cancelled));
}

#[test]
fn racing_releases_append_once_and_impossible_release_history_fails_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("release-race").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Release once").unwrap(),
            instant(1),
        )
        .unwrap();
    store.claim(&id, 1, instant(1)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["first recovery", "second recovery"].map(|reason| {
        let path = path.clone();
        let id = id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.release(&id, 2, ScheduleRelease::new(reason).unwrap())
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(ScheduleStoreError::ConcurrentModification {
                    expected_revision: 2,
                    current_revision: 3,
                    ..
                })
            ))
            .count(),
        1
    );
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:release-race', 4, 'schedule.released', 1, ?1)",
            [br#"{"reason":"impossible second release"}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        ScheduleStore::open(&path).unwrap().list().unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 4 }
    ));
}

#[test]
fn replay_rejects_malformed_release_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("malformed-release").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Reject corrupt recovery evidence").unwrap(),
            instant(1),
        )
        .unwrap();
    store.claim(&id, 1, instant(1)).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:malformed-release', 3, 'schedule.released', 1, ?1)",
            [br#"{"reason":"  "}"#.as_slice()],
        )
        .unwrap();

    assert!(matches!(
        ScheduleStore::open(&path).unwrap().list().unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));
    assert!(matches!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(1))
            .unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));
}

#[test]
fn materializes_claimed_schedule_and_task_atomically_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let schedule_id = ScheduleId::new("materialize-me").unwrap();
    let task_id = TaskId::new("scheduled-task").unwrap();
    let goal = TaskGoal::new("Perform the scheduled work").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(schedule_id.clone(), goal.clone(), instant(10))
        .unwrap();
    store.claim(&schedule_id, 1, instant(10)).unwrap();
    store
        .release(
            &schedule_id,
            2,
            ScheduleRelease::new("recover before materialization").unwrap(),
        )
        .unwrap();
    store.claim(&schedule_id, 3, instant(10)).unwrap();

    let materialized = store.materialize(&schedule_id, 4, task_id.clone()).unwrap();

    assert_eq!(materialized.status(), ScheduleStatus::Materialized);
    assert_eq!(materialized.task_id(), Some(&task_id));
    assert_eq!(
        materialized.latest_release().unwrap().as_str(),
        "recover before materialization"
    );
    assert_eq!(materialized.goal(), &goal);
    assert_eq!(
        ScheduleStore::open(&path)
            .unwrap()
            .load(&schedule_id)
            .unwrap(),
        Some(materialized.clone())
    );
    assert_eq!(
        ScheduleStore::open(&path).unwrap().list().unwrap(),
        vec![materialized.clone()]
    );
    let task = TaskStore::open(&path)
        .unwrap()
        .load(&task_id)
        .unwrap()
        .unwrap();
    assert_eq!(task.id(), &task_id);
    assert_eq!(task.goal(), &goal);
    assert!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(u64::MAX))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn materialization_rejects_invalid_states_and_task_collisions_without_writes() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let pending_id = ScheduleId::new("pending-materialization").unwrap();
    let cancelled_id = ScheduleId::new("cancelled-materialization").unwrap();
    let claimed_id = ScheduleId::new("claimed-materialization").unwrap();
    let missing_id = ScheduleId::new("missing-materialization").unwrap();
    let existing_task_id = TaskId::new("existing-task").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in [&pending_id, &cancelled_id, &claimed_id] {
        store
            .schedule(
                id.clone(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(1),
            )
            .unwrap();
    }
    store
        .cancel(
            &cancelled_id,
            1,
            ScheduleCancellation::new("withdrawn").unwrap(),
        )
        .unwrap();
    let claimed = store.claim(&claimed_id, 1, instant(1)).unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(
            existing_task_id.clone(),
            TaskGoal::new("Existing goal").unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store
            .materialize(&missing_id, 1, TaskId::new("missing-task").unwrap())
            .unwrap_err(),
        ScheduleStoreError::NotFound { schedule_id } if schedule_id == missing_id
    ));
    assert!(matches!(
        store
            .materialize(&pending_id, 1, TaskId::new("pending-task").unwrap())
            .unwrap_err(),
        ScheduleStoreError::NotClaimed { schedule_id } if schedule_id == pending_id
    ));
    assert!(matches!(
        store
            .materialize(
                &cancelled_id,
                2,
                TaskId::new("cancelled-task").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::AlreadyCancelled { schedule_id } if schedule_id == cancelled_id
    ));
    assert!(matches!(
        store
            .materialize(&claimed_id, 2, existing_task_id.clone())
            .unwrap_err(),
        ScheduleStoreError::TaskAlreadyExists { task_id } if task_id == existing_task_id
    ));
    assert_eq!(store.load(&claimed_id).unwrap(), Some(claimed));

    let new_task_id = TaskId::new("new-task").unwrap();
    let materialized = store
        .materialize(&claimed_id, 2, new_task_id.clone())
        .unwrap();
    assert!(matches!(
        store
            .materialize(
                &claimed_id,
                3,
                TaskId::new("replacement-task").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::AlreadyMaterialized {
            schedule_id,
            task_id,
        } if schedule_id == claimed_id && task_id == new_task_id
    ));
    assert_eq!(store.load(&claimed_id).unwrap(), Some(materialized));
}

#[test]
fn racing_materializations_commit_exactly_one_schedule_task_pair() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let schedule_id = ScheduleId::new("materialization-race").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            schedule_id.clone(),
            TaskGoal::new("Create one task").unwrap(),
            instant(1),
        )
        .unwrap();
    store.claim(&schedule_id, 1, instant(1)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["first-task", "second-task"].map(|task_id| {
        let path = path.clone();
        let schedule_id = schedule_id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.materialize(&schedule_id, 2, TaskId::new(task_id).unwrap())
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(ScheduleStoreError::ConcurrentModification {
                    expected_revision: 2,
                    current_revision: 3,
                    ..
                })
            ))
            .count(),
        1
    );
    let loaded = ScheduleStore::open(&path)
        .unwrap()
        .load(&schedule_id)
        .unwrap()
        .unwrap();
    let winner = loaded.task_id().unwrap();
    assert!(matches!(winner.as_str(), "first-task" | "second-task"));
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(winner)
            .unwrap()
            .is_some()
    );
    let loser = if winner.as_str() == "first-task" {
        TaskId::new("second-task").unwrap()
    } else {
        TaskId::new("first-task").unwrap()
    };
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&loser)
            .unwrap()
            .is_none()
    );
}

#[test]
fn replay_rejects_malformed_and_impossible_materialization_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("bad-materialization").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Reject invalid materialization").unwrap(),
            instant(1),
        )
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:bad-materialization', 2, 'schedule.materialized', 1, ?1)",
            [br#"{"task_id":"first-task"}"#.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES ('schedule:bad-materialization', 3, 'schedule.materialized', 1, ?1)",
            [br#"{"task_id":"duplicate-task"}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store.load(&id).unwrap_err(),
        ScheduleStoreError::InvalidHistory { event_count: 3 }
    ));

    connection
        .execute(
            "UPDATE events SET payload = ?1 WHERE stream_id = 'schedule:bad-materialization' AND stream_version = 3",
            [br#"{"task_id":""}"#.as_slice()],
        )
        .unwrap();
    assert!(matches!(
        store.load(&id).unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 3,
            ..
        })
    ));
}

#[test]
fn stale_claim_revision_cannot_release_a_later_claim() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("stale-release").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Preserve the latest claimant").unwrap(),
            instant(1),
        )
        .unwrap();
    let first_claim = store.claim(&id, 1, instant(1)).unwrap();
    store
        .release(
            &id,
            first_claim.revision(),
            ScheduleRelease::new("first claimant stopped").unwrap(),
        )
        .unwrap();
    let second_claim = store.claim(&id, 3, instant(1)).unwrap();

    assert!(matches!(
        store
            .release(
                &id,
                first_claim.revision(),
                ScheduleRelease::new("stale recovery").unwrap(),
            )
            .unwrap_err(),
        ScheduleStoreError::ConcurrentModification {
            schedule_id,
            expected_revision: 2,
            current_revision: 4,
        } if schedule_id == id
    ));
    assert_eq!(store.load(&id).unwrap(), Some(second_claim));
}

#[test]
fn stale_claim_revision_cannot_materialize_a_later_claim() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = ScheduleId::new("stale-materialization").unwrap();
    let task_id = TaskId::new("stale-task").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    store
        .schedule(
            id.clone(),
            TaskGoal::new("Do not consume a newer claim").unwrap(),
            instant(1),
        )
        .unwrap();
    let first_claim = store.claim(&id, 1, instant(1)).unwrap();
    store
        .release(
            &id,
            first_claim.revision(),
            ScheduleRelease::new("handoff").unwrap(),
        )
        .unwrap();
    let second_claim = store.claim(&id, 3, instant(1)).unwrap();

    assert!(matches!(
        store
            .materialize(&id, first_claim.revision(), task_id.clone())
            .unwrap_err(),
        ScheduleStoreError::ConcurrentModification {
            schedule_id,
            expected_revision: 2,
            current_revision: 4,
        } if schedule_id == id
    ));
    assert_eq!(store.load(&id).unwrap(), Some(second_claim));
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn claims_next_due_schedule_in_due_then_exact_id_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    let cancelled_id = ScheduleId::new("cancelled").unwrap();
    let first_id = ScheduleId::new("alpha").unwrap();
    let second_id = ScheduleId::new("zeta").unwrap();
    let later_id = ScheduleId::new("later").unwrap();
    for (id, due_at) in [
        (&cancelled_id, 1),
        (&second_id, 5),
        (&first_id, 5),
        (&later_id, 6),
    ] {
        store
            .schedule(
                id.clone(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(due_at),
            )
            .unwrap();
    }
    store
        .cancel(
            &cancelled_id,
            1,
            ScheduleCancellation::new("withdrawn").unwrap(),
        )
        .unwrap();

    assert_eq!(store.claim_next_due(instant(4)).unwrap(), None);
    let first = store.claim_next_due(instant(5)).unwrap().unwrap();
    assert_eq!(first.id(), &first_id);
    assert_eq!(first.status(), ScheduleStatus::Claimed);
    assert_eq!(first.revision(), 2);
    let second = store.claim_next_due(instant(5)).unwrap().unwrap();
    assert_eq!(second.id(), &second_id);
    assert_eq!(store.claim_next_due(instant(5)).unwrap(), None);

    drop(store);
    let reopened = ScheduleStore::open(&path).unwrap();
    assert_eq!(reopened.load(&first_id).unwrap(), Some(first));
    assert_eq!(reopened.load(&second_id).unwrap(), Some(second));
    assert_eq!(reopened.list_due(instant(6)).unwrap()[0].id(), &later_id);
}

#[test]
fn racing_next_due_claims_reserve_distinct_schedules() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in ["first", "second"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(1),
            )
            .unwrap();
    }
    drop(store);
    let barrier = Arc::new(Barrier::new(2));
    let handles = [(), ()].map(|()| {
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.claim_next_due(instant(1)).unwrap().unwrap()
        })
    });
    let claimed = handles.map(|handle| handle.join().unwrap());

    assert_ne!(claimed[0].id(), claimed[1].id());
    assert!(
        claimed
            .iter()
            .all(|item| item.status() == ScheduleStatus::Claimed)
    );
    assert!(
        ScheduleStore::open(&path)
            .unwrap()
            .list_due(instant(1))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn next_due_claim_fails_closed_before_skipping_corrupt_schedule_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in ["corrupt", "valid"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(1),
            )
            .unwrap();
    }
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'schedule:corrupt'",
            [],
        )
        .unwrap();

    assert!(matches!(
        store.claim_next_due(instant(1)).unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
    assert_eq!(
        store
            .load(&ScheduleId::new("valid").unwrap())
            .unwrap()
            .unwrap()
            .status(),
        ScheduleStatus::Pending
    );
}

#[test]
fn materializes_next_due_schedule_atomically_in_deterministic_order() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for (id, due_at) in [("zeta", 5), ("alpha", 5), ("later", 6)] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(due_at),
            )
            .unwrap();
    }

    let too_early_task = TaskId::new("too-early").unwrap();
    assert_eq!(
        store
            .materialize_next_due(instant(4), too_early_task.clone())
            .unwrap(),
        None
    );
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&too_early_task)
            .unwrap()
            .is_none()
    );
    let task_id = TaskId::new("selected-task").unwrap();
    let materialized = store
        .materialize_next_due(instant(5), task_id.clone())
        .unwrap()
        .unwrap();

    assert_eq!(materialized.id().as_str(), "alpha");
    assert_eq!(materialized.status(), ScheduleStatus::Materialized);
    assert_eq!(materialized.revision(), 2);
    assert_eq!(materialized.task_id(), Some(&task_id));
    assert_eq!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .unwrap()
            .goal()
            .as_str(),
        "goal-alpha"
    );
    assert_eq!(
        ScheduleStore::open(&path)
            .unwrap()
            .find_by_task_id(&task_id)
            .unwrap(),
        Some(materialized.clone())
    );
    assert_eq!(
        ScheduleStore::open(&path)
            .unwrap()
            .history(materialized.id())
            .unwrap()
            .unwrap()
            .iter()
            .map(|entry| entry.event().clone())
            .collect::<Vec<_>>(),
        [
            ScheduleHistoryEvent::Created {
                goal: TaskGoal::new("goal-alpha").unwrap(),
                due_at: instant(5),
            },
            ScheduleHistoryEvent::Materialized { task_id },
        ]
    );
}

#[test]
fn next_due_materialization_task_collision_consumes_no_schedule() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = TaskId::new("existing-task").unwrap();
    let mut store = ScheduleStore::open(&path).unwrap();
    let pending = store
        .schedule(
            ScheduleId::new("pending").unwrap(),
            TaskGoal::new("remain pending").unwrap(),
            instant(1),
        )
        .unwrap();
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("existing goal").unwrap())
        .unwrap();

    assert!(matches!(
        store
            .materialize_next_due(instant(1), task_id.clone())
            .unwrap_err(),
        ScheduleStoreError::TaskAlreadyExists { task_id: duplicate } if duplicate == task_id
    ));
    assert_eq!(store.load(pending.id()).unwrap(), Some(pending));
}

#[test]
fn racing_next_due_materializations_consume_distinct_schedules() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in ["first", "second"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(1),
            )
            .unwrap();
    }
    drop(store);
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["task-a", "task-b"].map(|task_id| {
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store
                .materialize_next_due(instant(1), TaskId::new(task_id).unwrap())
                .unwrap()
                .unwrap()
        })
    });
    let materialized = handles.map(|handle| handle.join().unwrap());

    assert_ne!(materialized[0].id(), materialized[1].id());
    assert!(
        materialized
            .iter()
            .all(|schedule| schedule.status() == ScheduleStatus::Materialized)
    );
}

#[test]
fn next_due_materialization_fails_closed_before_skipping_corrupt_history() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ScheduleStore::open(&path).unwrap();
    for id in ["corrupt", "valid"] {
        store
            .schedule(
                ScheduleId::new(id).unwrap(),
                TaskGoal::new(format!("goal-{id}")).unwrap(),
                instant(1),
            )
            .unwrap();
    }
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'7B7D' WHERE stream_id = 'schedule:corrupt'",
            [],
        )
        .unwrap();
    let task_id = TaskId::new("must-not-start").unwrap();

    assert!(matches!(
        store
            .materialize_next_due(instant(1), task_id.clone())
            .unwrap_err(),
        ScheduleStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
    assert!(
        TaskStore::open(&path)
            .unwrap()
            .load(&task_id)
            .unwrap()
            .is_none()
    );
}
