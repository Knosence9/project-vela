use std::{fmt, path::Path};

use serde::Serialize;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};

const SESSION_CREATED_EVENT_TYPE: &str = "session.created";
const SESSION_EVENT_PAYLOAD_VERSION: u32 = 1;

/// An opaque, non-empty identifier for one session.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, SessionIdError> {
        let value = value.into();
        if value.is_empty() {
            Err(SessionIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionIdError;

impl fmt::Display for SessionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session id must not be empty")
    }
}

impl std::error::Error for SessionIdError {}

/// The non-empty, human-readable title recorded when a session is created.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionTitle(String);

impl SessionTitle {
    pub fn new(value: impl Into<String>) -> Result<Self, SessionTitleError> {
        let value = value.into();
        if value.is_empty() {
            Err(SessionTitleError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionTitleError;

impl fmt::Display for SessionTitleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session title must not be empty")
    }
}

impl std::error::Error for SessionTitleError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Open,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    id: SessionId,
    title: SessionTitle,
    status: SessionStatus,
}

impl Session {
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn title(&self) -> &SessionTitle {
        &self.title
    }

    pub fn status(&self) -> SessionStatus {
        self.status
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SessionStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists { session_id: SessionId },
    InvalidHistory { event_count: usize },
}

impl fmt::Display for SessionStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLog(error) => write!(formatter, "session event-log error: {error}"),
            Self::Replay(error) => write!(formatter, "session replay error: {error}"),
            Self::AlreadyExists { session_id } => {
                write!(formatter, "session {session_id} already exists")
            }
            Self::InvalidHistory { event_count } => write!(
                formatter,
                "invalid session history with {event_count} lifecycle events"
            ),
        }
    }
}

impl std::error::Error for SessionStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EventLog(error) => Some(error),
            Self::Replay(error) => Some(error),
            Self::AlreadyExists { .. } | Self::InvalidHistory { .. } => None,
        }
    }
}

/// A synchronous session lifecycle store backed by the typed event log.
pub struct SessionStore {
    event_log: EventLog,
}

impl SessionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionStoreError> {
        EventLog::open(path)
            .map(|event_log| Self { event_log })
            .map_err(SessionStoreError::EventLog)
    }

    pub fn create(
        &mut self,
        id: SessionId,
        title: SessionTitle,
    ) -> Result<Session, SessionStoreError> {
        let event = SessionEvent::Created {
            title: title.clone(),
        };
        match self
            .event_log
            .append(&session_stream(&id), ExpectedVersion::NoStream, &event)
        {
            Ok(_) => Ok(Session {
                id,
                title,
                status: SessionStatus::Open,
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(SessionStoreError::AlreadyExists { session_id: id }),
            Err(error) => Err(SessionStoreError::EventLog(error)),
        }
    }

    pub fn load(&self, id: &SessionId) -> Result<Option<Session>, SessionStoreError> {
        let events = self
            .event_log
            .replay::<SessionEvent>(&session_stream(id))
            .map_err(SessionStoreError::Replay)?;
        match events.as_slice() {
            [] => Ok(None),
            [SessionEvent::Created { title }] => Ok(Some(Session {
                id: id.clone(),
                title: title.clone(),
                status: SessionStatus::Open,
            })),
            _ => Err(SessionStoreError::InvalidHistory {
                event_count: events.len(),
            }),
        }
    }
}

fn session_stream(id: &SessionId) -> StreamId {
    StreamId::new(format!("session:{id}")).expect("a prefixed session stream is never empty")
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum SessionEvent {
    Created { title: SessionTitle },
}

impl Event for SessionEvent {
    fn event_type(&self) -> &'static str {
        SESSION_CREATED_EVENT_TYPE
    }

    fn payload_version(&self) -> u32 {
        SESSION_EVENT_PAYLOAD_VERSION
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        if event_type != SESSION_CREATED_EVENT_TYPE
            || payload_version != SESSION_EVENT_PAYLOAD_VERSION
        {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }

        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Payload {
            title: String,
        }

        let payload: Payload =
            serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                message: error.to_string(),
            })?;
        let title =
            SessionTitle::new(payload.title).map_err(|error| DecodeError::MalformedPayload {
                message: error.to_string(),
            })?;
        Ok(Self::Created { title })
    }
}
