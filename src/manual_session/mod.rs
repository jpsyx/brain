//! Durable user-managed brain-panel session identity and ordering.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::agent::AgentSession;

/// The permanent title of the primary manual session.
pub const MAIN_SESSION_TITLE: &str = "Brain";

/// Stable identity for one durable manual session.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ManualSessionId(String);

impl ManualSessionId {
    /// Create a fresh manual-session identity.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Parse a persisted non-blank identity.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()).then(|| Self(value.to_owned()))
    }

    /// Borrow the stable database representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ManualSessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// A normalized title for one open manual session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualSessionName(String);

impl ManualSessionName {
    /// Trim and validate a title against the other open titles.
    ///
    /// # Errors
    ///
    /// Returns [`ManualSessionNameError::Blank`] for an empty normalized
    /// title and [`ManualSessionNameError::Duplicate`] for an ASCII
    /// case-insensitive duplicate.
    pub fn parse(value: &str, open_titles: &[String]) -> Result<Self, ManualSessionNameError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(ManualSessionNameError::Blank);
        }
        if open_titles
            .iter()
            .any(|title| value.eq_ignore_ascii_case(title))
        {
            return Err(ManualSessionNameError::Duplicate);
        }
        Ok(Self(value.to_owned()))
    }

    /// The invariant title of the primary manual session.
    #[must_use]
    pub fn main() -> Self {
        Self(MAIN_SESSION_TITLE.to_owned())
    }

    /// Borrow the normalized title.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a proposed manual-session title is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualSessionNameError {
    /// The normalized title is empty.
    Blank,
    /// Another open manual session has the same ASCII case-insensitive title.
    Duplicate,
}

impl Display for ManualSessionNameError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Blank => "session name cannot be blank",
            Self::Duplicate => "a session with this name is already open",
        })
    }
}

impl Error for ManualSessionNameError {}

/// Whether a manual session is the primary tab or an additional named tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualSessionRole {
    /// The permanent primary session.
    Main,
    /// A user-created named session.
    Additional,
}

impl ManualSessionRole {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Additional => "additional",
        }
    }

    /// Parse a stable database representation.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "main" => Some(Self::Main),
            "additional" => Some(Self::Additional),
            _ => None,
        }
    }
}

/// Durable metadata linking one manual tab to a native frontend session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualSessionRecord {
    /// Stable Brain-owned manual-session identity.
    pub id: ManualSessionId,
    /// Current native frontend session identity.
    pub agent_session: AgentSession,
    /// User-visible normalized title.
    pub name: ManualSessionName,
    /// Stable display position, with Main fixed at zero.
    pub position: u32,
    /// Main or additional invariant role.
    pub role: ManualSessionRole,
}

impl ManualSessionRecord {
    /// Construct the primary manual-session record.
    #[must_use]
    pub fn main(id: ManualSessionId, agent_session: AgentSession) -> Self {
        Self {
            id,
            agent_session,
            name: ManualSessionName::main(),
            position: 0,
            role: ManualSessionRole::Main,
        }
    }

    /// Construct an additional named manual-session record.
    #[must_use]
    pub fn additional(
        id: ManualSessionId,
        agent_session: AgentSession,
        name: ManualSessionName,
        position: u32,
    ) -> Self {
        Self {
            id,
            agent_session,
            name,
            position,
            role: ManualSessionRole::Additional,
        }
    }

    /// Stable Brain-owned identity.
    #[must_use]
    pub const fn id(&self) -> &ManualSessionId {
        &self.id
    }

    /// Current native frontend session identity.
    #[must_use]
    pub const fn agent_session(&self) -> &AgentSession {
        &self.agent_session
    }

    /// User-visible title.
    #[must_use]
    pub const fn name(&self) -> &ManualSessionName {
        &self.name
    }

    /// Display position.
    #[must_use]
    pub const fn position(&self) -> u32 {
        self.position
    }

    /// Main or additional role.
    #[must_use]
    pub const fn role(&self) -> ManualSessionRole {
        self.role
    }
}

#[cfg(test)]
mod tests;
