use crate::users::UserId;

/// The immutable effective person for one request lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorContext {
    user_id: UserId,
    display_name: String,
    channel: Channel,
}

/// How the initiating request reached Brain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Interactive,
    Sms,
    Email,
}

impl Channel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Sms => "sms",
            Self::Email => "email",
        }
    }
}

impl ActorContext {
    pub(super) fn new(user_id: UserId, display_name: String, channel: Channel) -> Self {
        Self {
            user_id,
            display_name,
            channel,
        }
    }

    #[must_use]
    pub const fn user_id(&self) -> &UserId {
        &self.user_id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub const fn channel(&self) -> Channel {
        self.channel
    }

    /// Preserve the initiating identity for another turn in the same lineage.
    #[must_use]
    pub fn follow_up(initiating: &Self) -> Self {
        initiating.clone()
    }
}
