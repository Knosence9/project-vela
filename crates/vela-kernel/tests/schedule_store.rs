use std::sync::{Arc, Barrier};

use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    scheduler::{
        ScheduleCancellation, ScheduleId, ScheduleInstant, ScheduleStatus, ScheduleStore,
        ScheduleStoreError,
    },
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
            store.cancel(&id, ScheduleCancellation::new(reason).unwrap())
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ScheduleStoreError::AlreadyCancelled { .. })))
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

    let claimed = store.claim(&id, instant(10)).unwrap();

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
            ScheduleCancellation::new("Withdrawn").unwrap(),
        )
        .unwrap();

    assert!(matches!(
        store.claim(&pending_id, instant(10)).unwrap_err(),
        ScheduleStoreError::NotDue {
            schedule_id,
            due_at,
            cutoff,
        } if schedule_id == pending_id && due_at == instant(11) && cutoff == instant(10)
    ));
    assert_eq!(store.load(&pending_id).unwrap(), Some(pending));
    assert!(matches!(
        store.claim(&missing_id, instant(u64::MAX)).unwrap_err(),
        ScheduleStoreError::NotFound { schedule_id } if schedule_id == missing_id
    ));
    assert!(matches!(
        store
            .claim(&cancelled_id, instant(u64::MAX))
            .unwrap_err(),
        ScheduleStoreError::AlreadyCancelled { schedule_id } if schedule_id == cancelled_id
    ));
    assert_eq!(store.load(&cancelled_id).unwrap(), Some(cancelled));

    let claimed = store.claim(&pending_id, instant(11)).unwrap();
    assert!(matches!(
        store.claim(&pending_id, instant(11)).unwrap_err(),
        ScheduleStoreError::AlreadyClaimed { schedule_id } if schedule_id == pending_id
    ));
    assert!(matches!(
        store
            .cancel(
                &pending_id,
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
            store.claim(&id, instant(1))
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ScheduleStoreError::AlreadyClaimed { .. })))
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
            store.claim(&id, instant(1))
        })
    };
    let cancel = {
        let path = path.clone();
        let id = id.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let mut store = ScheduleStore::open(path).unwrap();
            barrier.wait();
            store.cancel(&id, ScheduleCancellation::new("operator withdrew").unwrap())
        })
    };
    let results = [claim.join().unwrap(), cancel.join().unwrap()];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let loaded = ScheduleStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    match loaded.status() {
        ScheduleStatus::Claimed => assert!(
            results
                .iter()
                .any(|result| matches!(result, Err(ScheduleStoreError::AlreadyClaimed { .. })))
        ),
        ScheduleStatus::Cancelled => assert!(
            results
                .iter()
                .any(|result| matches!(result, Err(ScheduleStoreError::AlreadyCancelled { .. })))
        ),
        ScheduleStatus::Pending => panic!("the schedule must be terminal"),
    }
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
            .cancel(&missing, ScheduleCancellation::new("No intent").unwrap())
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
        .cancel(&id, ScheduleCancellation::new("First reason").unwrap())
        .unwrap();

    assert!(matches!(
        store
            .cancel(&id, ScheduleCancellation::new("Replacement").unwrap())
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
