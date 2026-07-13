use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use vela_kernel::event_log::{Event, EventLog, ExpectedVersion, ReplayError, StreamId};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum AccountEvent {
    Opened { owner: String },
    Credited { cents: u64 },
}

impl Event for AccountEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "account.opened",
            Self::Credited { .. } => "account.credited",
        }
    }

    fn payload_version(&self) -> u32 {
        1
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, ReplayError> {
        if payload_version != 1 || !matches!(event_type, "account.opened" | "account.credited") {
            return Err(ReplayError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }
        serde_json::from_slice(payload).map_err(|error| ReplayError::MalformedPayload {
            stream_version: 0,
            message: error.to_string(),
        })
    }
}

#[test]
fn appends_and_replays_typed_events_in_order_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let stream = StreamId::new("account-42").unwrap();

    {
        let mut log = EventLog::open(&path).unwrap();
        assert_eq!(
            log.append(
                &stream,
                ExpectedVersion::NoStream,
                &AccountEvent::Opened {
                    owner: "Ada".into(),
                },
            )
            .unwrap(),
            1
        );
        assert_eq!(
            log.append(
                &stream,
                ExpectedVersion::Exact(1),
                &AccountEvent::Credited { cents: 500 },
            )
            .unwrap(),
            2
        );
    }

    let log = EventLog::open(&path).unwrap();
    assert_eq!(
        log.replay::<AccountEvent>(&stream).unwrap(),
        vec![
            AccountEvent::Opened {
                owner: "Ada".into()
            },
            AccountEvent::Credited { cents: 500 },
        ]
    );
}

#[test]
fn rejects_stale_expected_versions_without_writing() {
    let directory = tempdir().unwrap();
    let mut log = EventLog::open(directory.path().join("events.sqlite3")).unwrap();
    let stream = StreamId::new("account-42").unwrap();
    let event = AccountEvent::Opened {
        owner: "Ada".into(),
    };

    log.append(&stream, ExpectedVersion::NoStream, &event)
        .unwrap();
    let error = log
        .append(&stream, ExpectedVersion::NoStream, &event)
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "wrong expected version: expected no stream, current version is 1"
    );
    assert_eq!(log.replay::<AccountEvent>(&stream).unwrap(), vec![event]);
}

#[test]
fn missing_stream_replays_as_empty() {
    let directory = tempdir().unwrap();
    let log = EventLog::open(directory.path().join("events.sqlite3")).unwrap();

    assert!(
        log.replay::<AccountEvent>(&StreamId::new("missing").unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stream_ids_must_not_be_empty() {
    assert_eq!(
        StreamId::new("").unwrap_err().to_string(),
        "stream id must not be empty"
    );
}

#[test]
fn rejects_unknown_persisted_event_types() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let stream = StreamId::new("account-42").unwrap();
    let mut log = EventLog::open(&path).unwrap();
    log.append(
        &stream,
        ExpectedVersion::NoStream,
        &AccountEvent::Opened {
            owner: "Ada".into(),
        },
    )
    .unwrap();
    drop(log);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET event_type = 'account.renamed' WHERE stream_id = ?1",
            [stream.as_str()],
        )
        .unwrap();

    let log = EventLog::open(&path).unwrap();
    assert_eq!(
        log.replay::<AccountEvent>(&stream).unwrap_err(),
        ReplayError::UnsupportedEvent {
            event_type: "account.renamed".into(),
            payload_version: 1,
        }
    );
}

#[test]
fn rejects_unknown_payload_versions() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let stream = StreamId::new("account-42").unwrap();
    let mut log = EventLog::open(&path).unwrap();
    log.append(
        &stream,
        ExpectedVersion::NoStream,
        &AccountEvent::Opened {
            owner: "Ada".into(),
        },
    )
    .unwrap();
    drop(log);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload_version = 2 WHERE stream_id = ?1",
            [stream.as_str()],
        )
        .unwrap();

    let log = EventLog::open(&path).unwrap();
    assert_eq!(
        log.replay::<AccountEvent>(&stream).unwrap_err(),
        ReplayError::UnsupportedEvent {
            event_type: "account.opened".into(),
            payload_version: 2,
        }
    );
}

#[test]
fn reports_the_version_of_a_malformed_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let stream = StreamId::new("account-42").unwrap();
    let mut log = EventLog::open(&path).unwrap();
    log.append(
        &stream,
        ExpectedVersion::NoStream,
        &AccountEvent::Opened {
            owner: "Ada".into(),
        },
    )
    .unwrap();
    drop(log);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE events SET payload = X'00' WHERE stream_id = ?1",
            [stream.as_str()],
        )
        .unwrap();

    let log = EventLog::open(&path).unwrap();
    assert!(matches!(
        log.replay::<AccountEvent>(&stream).unwrap_err(),
        ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        }
    ));
}

#[test]
fn rejects_a_stored_version_gap() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let stream = StreamId::new("account-42").unwrap();
    let log = EventLog::open(&path).unwrap();
    drop(log);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES (?1, 2, 'account.credited', 1, ?2)",
            rusqlite::params![
                stream.as_str(),
                serde_json::to_vec(&AccountEvent::Credited { cents: 500 }).unwrap()
            ],
        )
        .unwrap();

    let log = EventLog::open(&path).unwrap();
    assert_eq!(
        log.replay::<AccountEvent>(&stream).unwrap_err(),
        ReplayError::VersionGap {
            expected: 1,
            found: 2,
        }
    );
}
