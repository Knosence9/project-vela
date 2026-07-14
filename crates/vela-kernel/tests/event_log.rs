use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use vela_kernel::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};

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

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if payload_version != 1 || !matches!(event_type, "account.opened" | "account.credited") {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }
        serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
            message: error.to_string(),
        })
    }
}

#[derive(Serialize)]
struct InvalidVersionEvent;

impl Event for InvalidVersionEvent {
    fn event_type(&self) -> &'static str {
        "invalid.version"
    }

    fn payload_version(&self) -> u32 {
        0
    }

    fn decode(_: &str, _: u32, _: &[u8]) -> Result<Self, DecodeError> {
        unreachable!("an invalid event must never be persisted")
    }
}

#[derive(Serialize)]
struct EmptyEventType;

impl Event for EmptyEventType {
    fn event_type(&self) -> &'static str {
        ""
    }

    fn payload_version(&self) -> u32 {
        1
    }

    fn decode(_: &str, _: u32, _: &[u8]) -> Result<Self, DecodeError> {
        unreachable!("an event without a discriminator must never be persisted")
    }
}

#[derive(Debug, Serialize)]
struct DivergentDiscriminatorEvent;

impl Event for DivergentDiscriminatorEvent {
    fn event_type(&self) -> &'static str {
        unreachable!("this decoder-only event must never be persisted")
    }

    fn payload_version(&self) -> u32 {
        unreachable!("this decoder-only event must never be persisted")
    }

    fn decode(_: &str, _: u32, _: &[u8]) -> Result<Self, DecodeError> {
        Err(DecodeError::UnsupportedEvent {
            event_type: "decoder.fabricated".into(),
            payload_version: 99,
        })
    }
}

#[test]
fn decode_errors_are_standard_errors_with_stable_context() {
    fn assert_standard_error(_: &dyn std::error::Error) {}

    let unsupported = DecodeError::UnsupportedEvent {
        event_type: "account.renamed".into(),
        payload_version: 2,
    };
    let malformed = DecodeError::MalformedPayload {
        message: "expected value".into(),
    };

    assert_standard_error(&unsupported);
    assert_standard_error(&malformed);
    assert_eq!(
        unsupported.to_string(),
        "unsupported event account.renamed at payload version 2"
    );
    assert_eq!(
        malformed.to_string(),
        "malformed event payload: expected value"
    );
}

#[test]
fn event_log_errors_expose_only_wrapped_error_sources() {
    use std::error::Error;

    let storage = EventLogError::Storage(rusqlite::Error::InvalidQuery);
    let encode = EventLogError::Encode(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
    let concurrency = EventLogError::WrongExpectedVersion {
        expected: ExpectedVersion::NoStream,
        current: Some(1),
    };
    let range = EventLogError::VersionOutOfRange(u64::MAX);
    let journal_mode = EventLogError::UnsupportedJournalMode("memory".into());
    let invalid_event_type = EventLogError::InvalidEventType;
    let invalid_payload_version = EventLogError::InvalidPayloadVersion(0);

    assert!(storage.source().unwrap().is::<rusqlite::Error>());
    assert!(encode.source().unwrap().is::<serde_json::Error>());
    assert!(concurrency.source().is_none());
    assert!(range.source().is_none());
    assert!(journal_mode.source().is_none());
    assert!(invalid_event_type.source().is_none());
    assert!(invalid_payload_version.source().is_none());
}

#[test]
fn rejects_zero_payload_version_without_writing() {
    let directory = tempdir().unwrap();
    let mut log = EventLog::open(directory.path().join("events.sqlite3")).unwrap();
    let stream = StreamId::new("invalid-1").unwrap();

    let error = log
        .append(&stream, ExpectedVersion::NoStream, &InvalidVersionEvent)
        .unwrap_err();

    assert_eq!(error.to_string(), "invalid event payload version 0");
    assert!(matches!(error, EventLogError::InvalidPayloadVersion(0)));
    assert!(log.replay::<AccountEvent>(&stream).unwrap().is_empty());
}

#[test]
fn rejects_empty_event_type_without_writing() {
    let directory = tempdir().unwrap();
    let mut log = EventLog::open(directory.path().join("events.sqlite3")).unwrap();
    let stream = StreamId::new("invalid-type-1").unwrap();

    let error = log
        .append(&stream, ExpectedVersion::NoStream, &EmptyEventType)
        .unwrap_err();

    assert_eq!(error.to_string(), "event type must not be empty");
    assert!(matches!(error, EventLogError::InvalidEventType));
    assert!(log.replay::<AccountEvent>(&stream).unwrap().is_empty());
}

#[test]
fn replay_errors_expose_only_storage_error_sources() {
    use std::error::Error;

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
            "UPDATE events SET payload = 1 WHERE stream_id = ?1",
            [stream.as_str()],
        )
        .unwrap();

    let log = EventLog::open(&path).unwrap();
    let storage = log.replay::<AccountEvent>(&stream).unwrap_err();
    let unsupported = ReplayError::UnsupportedEvent {
        event_type: "account.renamed".into(),
        payload_version: 2,
    };
    let malformed = ReplayError::MalformedPayload {
        stream_version: 1,
        message: "expected value".into(),
    };
    let gap = ReplayError::VersionGap {
        expected: 1,
        found: 2,
    };
    let invalid_version = ReplayError::InvalidStoredVersion(-1);
    let invalid_payload_version = ReplayError::InvalidStoredPayloadVersion(-1);

    assert!(matches!(&storage, ReplayError::Storage(_)));
    assert!(storage.source().unwrap().is::<rusqlite::Error>());
    assert_eq!(
        ReplayError::Storage(rusqlite::Error::InvalidQuery),
        ReplayError::Storage(rusqlite::Error::InvalidQuery)
    );
    assert!(unsupported.source().is_none());
    assert!(malformed.source().is_none());
    assert!(gap.source().is_none());
    assert!(invalid_version.source().is_none());
    assert!(invalid_payload_version.source().is_none());
}

#[test]
fn rejects_an_event_log_without_wal_journaling() {
    let error = match EventLog::open(":memory:") {
        Ok(_) => panic!("an event log without WAL journaling must be rejected"),
        Err(error) => error,
    };

    assert_eq!(
        error.to_string(),
        "event log requires WAL journal mode, found memory"
    );
    assert!(matches!(
        error,
        EventLogError::UnsupportedJournalMode(mode) if mode == "memory"
    ));
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
fn unsupported_replay_reports_the_persisted_discriminator() {
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

    assert_eq!(
        log.replay::<DivergentDiscriminatorEvent>(&stream)
            .unwrap_err(),
        ReplayError::UnsupportedEvent {
            event_type: "account.opened".into(),
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
fn reports_an_out_of_range_stored_payload_version() {
    use std::error::Error;

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
            "UPDATE events SET payload_version = 4294967296 WHERE stream_id = ?1",
            [stream.as_str()],
        )
        .unwrap();

    let log = EventLog::open(&path).unwrap();
    let error = log.replay::<AccountEvent>(&stream).unwrap_err();

    assert_eq!(
        error.to_string(),
        "invalid stored payload version 4294967296"
    );
    assert_eq!(
        error,
        ReplayError::InvalidStoredPayloadVersion(4_294_967_296)
    );
    assert!(error.source().is_none());
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
