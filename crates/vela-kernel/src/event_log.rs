use std::{fmt, path::Path};

use rusqlite::{Connection, TransactionBehavior, params};
use serde::Serialize;

/// An opaque, non-empty identifier for one event stream.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StreamId(String);

impl StreamId {
    pub fn new(value: impl Into<String>) -> Result<Self, StreamIdError> {
        let value = value.into();
        if value.is_empty() {
            Err(StreamIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamIdError;

impl fmt::Display for StreamIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("stream id must not be empty")
    }
}

impl std::error::Error for StreamIdError {}

/// The stream state a caller requires before an append may succeed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedVersion {
    NoStream,
    Exact(u64),
}

/// A typed event decoder can reject an unsupported discriminator or malformed bytes.
#[derive(Debug, Eq, PartialEq)]
pub enum DecodeError {
    UnsupportedEvent {
        event_type: String,
        payload_version: u32,
    },
    MalformedPayload {
        message: String,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedEvent {
                event_type,
                payload_version,
            } => write!(
                formatter,
                "unsupported event {event_type} at payload version {payload_version}"
            ),
            Self::MalformedPayload { message } => {
                write!(formatter, "malformed event payload: {message}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// A typed event family controls its stable persistence discriminator and decoding.
pub trait Event: Serialize + Sized {
    fn event_type(&self) -> &'static str;
    fn payload_version(&self) -> u32;
    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError>;
}

#[derive(Debug)]
#[non_exhaustive]
pub enum EventLogError {
    Storage(rusqlite::Error),
    Encode(serde_json::Error),
    UnsupportedJournalMode(String),
    InvalidEventType,
    InvalidPayloadVersion(u32),
    WrongExpectedVersion {
        expected: ExpectedVersion,
        current: Option<u64>,
    },
    VersionOutOfRange(u64),
}

impl fmt::Display for EventLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "event-log storage error: {error}"),
            Self::Encode(error) => write!(formatter, "event payload encoding failed: {error}"),
            Self::UnsupportedJournalMode(mode) => {
                write!(
                    formatter,
                    "event log requires WAL journal mode, found {mode}"
                )
            }
            Self::InvalidEventType => formatter.write_str("event type must not be empty"),
            Self::InvalidPayloadVersion(version) => {
                write!(formatter, "invalid event payload version {version}")
            }
            Self::WrongExpectedVersion { expected, current } => {
                formatter.write_str("wrong expected version: expected ")?;
                match expected {
                    ExpectedVersion::NoStream => formatter.write_str("no stream")?,
                    ExpectedVersion::Exact(version) => write!(formatter, "version {version}")?,
                }
                match current {
                    Some(version) => write!(formatter, ", current version is {version}"),
                    None => formatter.write_str(", stream does not exist"),
                }
            }
            Self::VersionOutOfRange(version) => {
                write!(
                    formatter,
                    "stream version {version} cannot be stored by SQLite"
                )
            }
        }
    }
}

impl std::error::Error for EventLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::UnsupportedJournalMode(_)
            | Self::InvalidEventType
            | Self::InvalidPayloadVersion(_)
            | Self::WrongExpectedVersion { .. }
            | Self::VersionOutOfRange(_) => None,
        }
    }
}

impl From<rusqlite::Error> for EventLogError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<serde_json::Error> for EventLogError {
    fn from(error: serde_json::Error) -> Self {
        Self::Encode(error)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReplayError {
    Storage(rusqlite::Error),
    UnsupportedEvent {
        event_type: String,
        payload_version: u32,
    },
    MalformedPayload {
        stream_version: u64,
        message: String,
    },
    VersionGap {
        expected: u64,
        found: u64,
    },
    InvalidStoredVersion(i64),
    InvalidStoredPayloadVersion(i64),
}

impl PartialEq for ReplayError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Storage(left), Self::Storage(right)) => left.to_string() == right.to_string(),
            (
                Self::UnsupportedEvent {
                    event_type: left_type,
                    payload_version: left_version,
                },
                Self::UnsupportedEvent {
                    event_type: right_type,
                    payload_version: right_version,
                },
            ) => left_type == right_type && left_version == right_version,
            (
                Self::MalformedPayload {
                    stream_version: left_version,
                    message: left_message,
                },
                Self::MalformedPayload {
                    stream_version: right_version,
                    message: right_message,
                },
            ) => left_version == right_version && left_message == right_message,
            (
                Self::VersionGap {
                    expected: left_expected,
                    found: left_found,
                },
                Self::VersionGap {
                    expected: right_expected,
                    found: right_found,
                },
            ) => left_expected == right_expected && left_found == right_found,
            (Self::InvalidStoredVersion(left), Self::InvalidStoredVersion(right)) => left == right,
            (Self::InvalidStoredPayloadVersion(left), Self::InvalidStoredPayloadVersion(right)) => {
                left == right
            }
            _ => false,
        }
    }
}

impl Eq for ReplayError {}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "event-log storage error: {error}"),
            Self::UnsupportedEvent {
                event_type,
                payload_version,
            } => write!(
                formatter,
                "unsupported event {event_type} payload version {payload_version}"
            ),
            Self::MalformedPayload {
                stream_version,
                message,
            } => write!(
                formatter,
                "malformed payload at stream version {stream_version}: {message}"
            ),
            Self::VersionGap { expected, found } => write!(
                formatter,
                "stream version gap: expected version {expected}, found {found}"
            ),
            Self::InvalidStoredVersion(version) => {
                write!(formatter, "invalid stored stream version {version}")
            }
            Self::InvalidStoredPayloadVersion(version) => {
                write!(formatter, "invalid stored payload version {version}")
            }
        }
    }
}

impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::UnsupportedEvent { .. }
            | Self::MalformedPayload { .. }
            | Self::VersionGap { .. }
            | Self::InvalidStoredVersion(_)
            | Self::InvalidStoredPayloadVersion(_) => None,
        }
    }
}

/// A synchronous, single-node SQLite append-only event log.
pub struct EventLog {
    connection: Connection,
}

impl EventLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EventLogError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let journal_mode: String =
            connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(EventLogError::UnsupportedJournalMode(journal_mode));
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                stream_id TEXT NOT NULL,
                stream_version INTEGER NOT NULL CHECK (stream_version >= 1),
                event_type TEXT NOT NULL,
                payload_version INTEGER NOT NULL CHECK (payload_version >= 1),
                payload BLOB NOT NULL,
                PRIMARY KEY (stream_id, stream_version)
            ) WITHOUT ROWID;",
        )?;
        Ok(Self { connection })
    }

    pub fn append<E: Event>(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        event: &E,
    ) -> Result<u64, EventLogError> {
        let event_type = event.event_type();
        if event_type.is_empty() {
            return Err(EventLogError::InvalidEventType);
        }
        let payload_version = event.payload_version();
        if payload_version == 0 {
            return Err(EventLogError::InvalidPayloadVersion(payload_version));
        }
        let payload = serde_json::to_vec(event)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = transaction
            .query_row(
                "SELECT MAX(stream_version) FROM events WHERE stream_id = ?1",
                [stream.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )?
            .map(stored_version_to_u64)
            .transpose()?;

        let matches = match (expected, current) {
            (ExpectedVersion::NoStream, None) => true,
            (ExpectedVersion::Exact(expected), Some(current)) => expected == current,
            _ => false,
        };
        if !matches {
            return Err(EventLogError::WrongExpectedVersion { expected, current });
        }

        let version = current.unwrap_or(0) + 1;
        let stored_version =
            i64::try_from(version).map_err(|_| EventLogError::VersionOutOfRange(version))?;
        transaction.execute(
            "INSERT INTO events
             (stream_id, stream_version, event_type, payload_version, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                stream.as_str(),
                stored_version,
                event_type,
                payload_version,
                payload
            ],
        )?;
        transaction.commit()?;
        Ok(version)
    }

    pub fn replay<E: Event>(&self, stream: &StreamId) -> Result<Vec<E>, ReplayError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT stream_version, event_type, payload_version, payload
                 FROM events WHERE stream_id = ?1 ORDER BY stream_version ASC",
            )
            .map_err(storage_replay_error)?;
        let mut rows = statement
            .query([stream.as_str()])
            .map_err(storage_replay_error)?;
        let mut expected = 1_u64;
        let mut events = Vec::new();

        while let Some(row) = rows.next().map_err(storage_replay_error)? {
            let stored_version: i64 = row.get(0).map_err(storage_replay_error)?;
            let version = u64::try_from(stored_version)
                .map_err(|_| ReplayError::InvalidStoredVersion(stored_version))?;
            if version != expected {
                return Err(ReplayError::VersionGap {
                    expected,
                    found: version,
                });
            }
            let event_type: String = row.get(1).map_err(storage_replay_error)?;
            let stored_payload_version: i64 = row.get(2).map_err(storage_replay_error)?;
            let payload_version = u32::try_from(stored_payload_version)
                .map_err(|_| ReplayError::InvalidStoredPayloadVersion(stored_payload_version))?;
            let payload: Vec<u8> = row.get(3).map_err(storage_replay_error)?;
            let event =
                E::decode(&event_type, payload_version, &payload).map_err(|error| match error {
                    DecodeError::UnsupportedEvent { .. } => ReplayError::UnsupportedEvent {
                        event_type,
                        payload_version,
                    },
                    DecodeError::MalformedPayload { message } => ReplayError::MalformedPayload {
                        stream_version: version,
                        message,
                    },
                })?;
            events.push(event);
            expected += 1;
        }

        Ok(events)
    }
}

fn stored_version_to_u64(version: i64) -> Result<u64, EventLogError> {
    u64::try_from(version)
        .map_err(|_| EventLogError::Storage(rusqlite::Error::IntegralValueOutOfRange(0, version)))
}

fn storage_replay_error(error: rusqlite::Error) -> ReplayError {
    ReplayError::Storage(error)
}
