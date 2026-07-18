use std::{error::Error, fmt, path::Path};

use crate::session::{
    Session, SessionId, SessionStore, SessionStoreError, SessionTurn, SessionTurnContent,
    SessionTurnRole,
};

/// A synchronous, provider-neutral source for one assistant response.
pub trait AssistantProvider {
    /// Produces one assistant turn from the complete durable conversation.
    fn complete(&mut self, transcript: &[SessionTurn])
    -> Result<SessionTurnContent, ProviderError>;
}

/// A provider failure that preserves the provider-specific error as its source.
#[derive(Debug)]
pub struct ProviderError {
    source: Box<dyn Error>,
}

impl ProviderError {
    pub fn new(error: impl Error + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for ProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// A failure before, during, or after one provider invocation.
#[derive(Debug)]
#[non_exhaustive]
pub enum RuntimeError {
    Session(SessionStoreError),
    Provider(ProviderError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "assistant runtime session error: {error}"),
            Self::Provider(error) => write!(formatter, "assistant provider error: {error}"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Provider(error) => Some(error),
        }
    }
}

/// Synchronous orchestration for one tool-free assistant turn.
pub struct AssistantRuntime<P> {
    sessions: SessionStore,
    provider: P,
}

impl<P: AssistantProvider> AssistantRuntime<P> {
    pub fn open(path: impl AsRef<Path>, provider: P) -> Result<Self, RuntimeError> {
        let sessions = SessionStore::open(path).map_err(RuntimeError::Session)?;
        Ok(Self { sessions, provider })
    }

    /// Durably appends the human turn, invokes the provider, then durably appends its response.
    ///
    /// Provider failure leaves the human turn committed and appends no assistant turn. The
    /// runtime does not retry provider calls because they may have external effects.
    pub fn execute_turn(
        &mut self,
        session_id: &SessionId,
        human_content: SessionTurnContent,
    ) -> Result<Session, RuntimeError> {
        let session = self
            .sessions
            .append_turn(session_id, SessionTurnRole::Human, human_content)
            .map_err(RuntimeError::Session)?;
        let assistant_content = self
            .provider
            .complete(session.turns())
            .map_err(RuntimeError::Provider)?;
        self.sessions
            .append_turn(session_id, SessionTurnRole::Assistant, assistant_content)
            .map_err(RuntimeError::Session)
    }
}
