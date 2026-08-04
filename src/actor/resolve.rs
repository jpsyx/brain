use std::error::Error;
use std::fmt::{Display, Formatter};

use super::{ActorContext, Channel};
use crate::users::{UserId, Users};

/// Identity evidence available after request authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestIdentity<'a> {
    Local,
    Sms { from: &'a str },
    Email { from: &'a str },
}

/// Effective-actor resolution failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorError {
    LocalUserNotFound,
    UnknownOrDisallowedSender,
}

impl Display for ActorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalUserNotFound => formatter.write_str("local user is not a workspace member"),
            Self::UnknownOrDisallowedSender => {
                formatter.write_str("inbound sender is unknown or not allowed")
            }
        }
    }
}

impl Error for ActorError {}

/// Resolve the effective actor once, after provider authentication.
pub fn resolve_actor(
    local_user_id: &UserId,
    request: RequestIdentity<'_>,
    users: &Users,
) -> Result<ActorContext, ActorError> {
    let (user, channel) = match request {
        RequestIdentity::Local => (
            users
                .user(local_user_id)
                .ok_or(ActorError::LocalUserNotFound)?,
            Channel::Interactive,
        ),
        RequestIdentity::Sms { from } => (
            users
                .resolve_phone(from)
                .ok_or(ActorError::UnknownOrDisallowedSender)?,
            Channel::Sms,
        ),
        RequestIdentity::Email { from } => (
            users
                .resolve_email(from)
                .ok_or(ActorError::UnknownOrDisallowedSender)?,
            Channel::Email,
        ),
    };
    Ok(ActorContext::new(
        user.id.clone(),
        user.name.clone(),
        channel,
    ))
}
