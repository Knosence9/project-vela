use std::{fmt, path::Path};

use serde::Serialize;

use crate::event_log::{
    DecodeError, Event, EventLog, EventLogError, ExpectedVersion, ReplayError, StreamId,
};

const SESSION_CREATED_EVENT_TYPE: &str = "session.created";
const SESSION_RENAMED_EVENT_TYPE: &str = "session.renamed";
const SESSION_SUMMARIZED_EVENT_TYPE: &str = "session.summarized";
const SESSION_CLOSED_EVENT_TYPE: &str = "session.closed";
const SESSION_REOPENED_EVENT_TYPE: &str = "session.reopened";
const SESSION_EVENT_PAYLOAD_VERSION: u32 = 1;
const SESSION_CLOSED_PAYLOAD_VERSION: u32 = 2;
const SESSION_REOPENED_PAYLOAD_VERSION: u32 = 2;

/// An opaque, non-empty identifier for one session.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
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

/// A non-empty persisted summary of a session's relevant context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionSummary(String);

impl SessionSummary {
    pub fn new(value: impl Into<String>) -> Result<Self, SessionSummaryError> {
        let value = value.into();
        if value.is_empty() {
            Err(SessionSummaryError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSummaryError;

impl fmt::Display for SessionSummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session summary must not be empty")
    }
}

impl std::error::Error for SessionSummaryError {}

/// The non-empty reason recorded when a session closes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionClosure(String);

impl SessionClosure {
    pub fn new(value: impl Into<String>) -> Result<Self, SessionClosureError> {
        let value = value.into();
        if value.is_empty() {
            Err(SessionClosureError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionClosureError;

impl fmt::Display for SessionClosureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session closure reason must not be empty")
    }
}

impl std::error::Error for SessionClosureError {}

/// The non-empty reason recorded when a closed session reopens.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionReopenReason(String);

impl SessionReopenReason {
    pub fn new(value: impl Into<String>) -> Result<Self, SessionReopenReasonError> {
        let value = value.into();
        if value.is_empty() {
            Err(SessionReopenReasonError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionReopenReasonError;

impl fmt::Display for SessionReopenReasonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session reopen reason must not be empty")
    }
}

impl std::error::Error for SessionReopenReasonError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Open,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    id: SessionId,
    title: SessionTitle,
    summary: Option<SessionSummary>,
    status: SessionStatus,
    closure: Option<SessionClosure>,
    reopen_reason: Option<SessionReopenReason>,
}

impl Session {
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn title(&self) -> &SessionTitle {
        &self.title
    }

    pub fn summary(&self) -> Option<&SessionSummary> {
        self.summary.as_ref()
    }

    pub fn status(&self) -> SessionStatus {
        self.status
    }

    pub fn closure(&self) -> Option<&SessionClosure> {
        self.closure.as_ref()
    }

    pub fn reopen_reason(&self) -> Option<&SessionReopenReason> {
        self.reopen_reason.as_ref()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SessionStoreError {
    EventLog(EventLogError),
    Replay(ReplayError),
    AlreadyExists { session_id: SessionId },
    NotFound { session_id: SessionId },
    AlreadyClosed { session_id: SessionId },
    AlreadyOpen { session_id: SessionId },
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
            Self::NotFound { session_id } => {
                write!(formatter, "session {session_id} was not found")
            }
            Self::AlreadyClosed { session_id } => {
                write!(formatter, "session {session_id} is already closed")
            }
            Self::AlreadyOpen { session_id } => {
                write!(formatter, "session {session_id} is already open")
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
            Self::AlreadyExists { .. }
            | Self::NotFound { .. }
            | Self::AlreadyClosed { .. }
            | Self::AlreadyOpen { .. }
            | Self::InvalidHistory { .. } => None,
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
                summary: None,
                status: SessionStatus::Open,
                closure: None,
                reopen_reason: None,
            }),
            Err(EventLogError::WrongExpectedVersion {
                expected: ExpectedVersion::NoStream,
                current: Some(_),
            }) => Err(SessionStoreError::AlreadyExists { session_id: id }),
            Err(error) => Err(SessionStoreError::EventLog(error)),
        }
    }

    pub fn rename(
        &mut self,
        id: &SessionId,
        title: SessionTitle,
    ) -> Result<Session, SessionStoreError> {
        loop {
            let Some(loaded) = self.load_versioned(id)? else {
                return Err(SessionStoreError::NotFound {
                    session_id: id.clone(),
                });
            };
            match self.event_log.append(
                &session_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                &SessionEvent::Renamed {
                    title: title.clone(),
                },
            ) {
                Ok(_) => {
                    return Ok(Session {
                        title,
                        ..loaded.session
                    });
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(SessionStoreError::EventLog(error)),
            }
        }
    }

    pub fn summarize(
        &mut self,
        id: &SessionId,
        summary: SessionSummary,
    ) -> Result<Session, SessionStoreError> {
        loop {
            let Some(loaded) = self.load_versioned(id)? else {
                return Err(SessionStoreError::NotFound {
                    session_id: id.clone(),
                });
            };
            match self.event_log.append(
                &session_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                &SessionEvent::Summarized {
                    summary: summary.clone(),
                },
            ) {
                Ok(_) => {
                    return Ok(Session {
                        summary: Some(summary),
                        ..loaded.session
                    });
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(SessionStoreError::EventLog(error)),
            }
        }
    }

    pub fn close(
        &mut self,
        id: &SessionId,
        closure: SessionClosure,
    ) -> Result<Session, SessionStoreError> {
        self.transition(
            id,
            SessionStatus::Open,
            &SessionEvent::Closed {
                reason: Some(closure),
            },
            |session_id| SessionStoreError::AlreadyClosed { session_id },
        )
    }

    pub fn reopen(
        &mut self,
        id: &SessionId,
        reason: SessionReopenReason,
    ) -> Result<Session, SessionStoreError> {
        self.transition(
            id,
            SessionStatus::Closed,
            &SessionEvent::Reopened {
                reason: Some(reason),
            },
            |session_id| SessionStoreError::AlreadyOpen { session_id },
        )
    }

    fn transition(
        &mut self,
        id: &SessionId,
        required_status: SessionStatus,
        event: &SessionEvent,
        already_transitioned: impl Fn(SessionId) -> SessionStoreError,
    ) -> Result<Session, SessionStoreError> {
        loop {
            let loaded = match self.load_versioned(id)? {
                Some(loaded) if loaded.session.status == required_status => loaded,
                Some(_) => return Err(already_transitioned(id.clone())),
                None => {
                    return Err(SessionStoreError::NotFound {
                        session_id: id.clone(),
                    });
                }
            };
            match self.event_log.append(
                &session_stream(id),
                ExpectedVersion::Exact(loaded.stream_version),
                event,
            ) {
                Ok(_) => {
                    let (status, closure, reopen_reason) = match event {
                        SessionEvent::Closed { reason } => {
                            (SessionStatus::Closed, reason.clone(), None)
                        }
                        SessionEvent::Reopened { reason } => {
                            (SessionStatus::Open, None, reason.clone())
                        }
                        SessionEvent::Created { .. }
                        | SessionEvent::Renamed { .. }
                        | SessionEvent::Summarized { .. } => {
                            unreachable!(
                                "creation, rename, and summary do not use status transitions"
                            )
                        }
                    };
                    return Ok(Session {
                        status,
                        closure,
                        reopen_reason,
                        ..loaded.session
                    });
                }
                Err(EventLogError::WrongExpectedVersion { .. }) => continue,
                Err(error) => return Err(SessionStoreError::EventLog(error)),
            }
        }
    }

    pub fn load(&self, id: &SessionId) -> Result<Option<Session>, SessionStoreError> {
        self.load_versioned(id)
            .map(|loaded| loaded.map(|loaded| loaded.session))
    }

    pub(crate) fn load_with_version(
        &self,
        id: &SessionId,
    ) -> Result<Option<(Session, u64)>, SessionStoreError> {
        self.load_versioned(id)
            .map(|loaded| loaded.map(|loaded| (loaded.session, loaded.stream_version)))
    }

    fn load_versioned(
        &self,
        id: &SessionId,
    ) -> Result<Option<VersionedSession>, SessionStoreError> {
        let events = self
            .event_log
            .replay::<SessionEvent>(&session_stream(id))
            .map_err(SessionStoreError::Replay)?;
        let Some(SessionEvent::Created { title }) = events.first() else {
            return if events.is_empty() {
                Ok(None)
            } else {
                Err(SessionStoreError::InvalidHistory {
                    event_count: events.len(),
                })
            };
        };
        let mut title = title.clone();
        let mut summary = None;
        let mut status = SessionStatus::Open;
        let mut closure = None;
        let mut reopen_reason = None;
        for event in &events[1..] {
            status = match (status, event) {
                (status, SessionEvent::Renamed { title: new_title }) => {
                    title = new_title.clone();
                    status
                }
                (
                    status,
                    SessionEvent::Summarized {
                        summary: new_summary,
                    },
                ) => {
                    summary = Some(new_summary.clone());
                    status
                }
                (SessionStatus::Open, SessionEvent::Closed { reason }) => {
                    closure = reason.clone();
                    reopen_reason = None;
                    SessionStatus::Closed
                }
                (SessionStatus::Closed, SessionEvent::Reopened { reason }) => {
                    closure = None;
                    reopen_reason = reason.clone();
                    SessionStatus::Open
                }
                _ => {
                    return Err(SessionStoreError::InvalidHistory {
                        event_count: events.len(),
                    });
                }
            };
        }
        Ok(Some(VersionedSession {
            session: Session {
                id: id.clone(),
                title,
                summary,
                status,
                closure,
                reopen_reason,
            },
            stream_version: u64::try_from(events.len()).map_err(|_| {
                SessionStoreError::InvalidHistory {
                    event_count: events.len(),
                }
            })?,
        }))
    }
}

struct VersionedSession {
    session: Session,
    stream_version: u64,
}

pub(crate) fn session_stream(id: &SessionId) -> StreamId {
    StreamId::new(format!("session:{id}")).expect("a prefixed session stream is never empty")
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum SessionEvent {
    Created { title: SessionTitle },
    Renamed { title: SessionTitle },
    Summarized { summary: SessionSummary },
    Closed { reason: Option<SessionClosure> },
    Reopened { reason: Option<SessionReopenReason> },
}

impl Event for SessionEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => SESSION_CREATED_EVENT_TYPE,
            Self::Renamed { .. } => SESSION_RENAMED_EVENT_TYPE,
            Self::Summarized { .. } => SESSION_SUMMARIZED_EVENT_TYPE,
            Self::Closed { .. } => SESSION_CLOSED_EVENT_TYPE,
            Self::Reopened { .. } => SESSION_REOPENED_EVENT_TYPE,
        }
    }

    fn payload_version(&self) -> u32 {
        match self {
            Self::Closed { reason: Some(_) } => SESSION_CLOSED_PAYLOAD_VERSION,
            Self::Reopened { reason: Some(_) } => SESSION_REOPENED_PAYLOAD_VERSION,
            Self::Created { .. }
            | Self::Renamed { .. }
            | Self::Summarized { .. }
            | Self::Closed { reason: None }
            | Self::Reopened { reason: None } => SESSION_EVENT_PAYLOAD_VERSION,
        }
    }

    fn decode(event_type: &str, payload_version: u32, payload: &[u8]) -> Result<Self, DecodeError> {
        let supported = match event_type {
            SESSION_CREATED_EVENT_TYPE
            | SESSION_RENAMED_EVENT_TYPE
            | SESSION_SUMMARIZED_EVENT_TYPE => payload_version == SESSION_EVENT_PAYLOAD_VERSION,
            SESSION_CLOSED_EVENT_TYPE => matches!(payload_version, 1 | 2),
            SESSION_REOPENED_EVENT_TYPE => matches!(payload_version, 1 | 2),
            _ => false,
        };
        if !supported {
            return Err(DecodeError::UnsupportedEvent {
                event_type: event_type.to_owned(),
                payload_version,
            });
        }

        if matches!(
            (event_type, payload_version),
            (SESSION_CLOSED_EVENT_TYPE, SESSION_CLOSED_PAYLOAD_VERSION)
                | (
                    SESSION_REOPENED_EVENT_TYPE,
                    SESSION_REOPENED_PAYLOAD_VERSION
                )
        ) {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {
                reason: String,
            }

            let payload: Payload =
                serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            return if event_type == SESSION_CLOSED_EVENT_TYPE {
                let reason = SessionClosure::new(payload.reason).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Closed {
                    reason: Some(reason),
                })
            } else {
                let reason = SessionReopenReason::new(payload.reason).map_err(|error| {
                    DecodeError::MalformedPayload {
                        message: error.to_string(),
                    }
                })?;
                Ok(Self::Reopened {
                    reason: Some(reason),
                })
            };
        }

        if matches!(
            event_type,
            SESSION_CLOSED_EVENT_TYPE | SESSION_REOPENED_EVENT_TYPE
        ) {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Payload {}

            serde_json::from_slice::<Payload>(payload).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(if event_type == SESSION_CLOSED_EVENT_TYPE {
                Self::Closed { reason: None }
            } else {
                Self::Reopened { reason: None }
            });
        }

        if event_type == SESSION_SUMMARIZED_EVENT_TYPE {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct SummaryPayload {
                summary: String,
            }

            let payload: SummaryPayload =
                serde_json::from_slice(payload).map_err(|error| DecodeError::MalformedPayload {
                    message: error.to_string(),
                })?;
            let summary = SessionSummary::new(payload.summary).map_err(|error| {
                DecodeError::MalformedPayload {
                    message: error.to_string(),
                }
            })?;
            return Ok(Self::Summarized { summary });
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
        Ok(if event_type == SESSION_CREATED_EVENT_TYPE {
            Self::Created { title }
        } else {
            Self::Renamed { title }
        })
    }
}
