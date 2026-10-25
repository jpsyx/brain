//! Shared channel presentation policy.

mod render;

pub use render::{render_instructions, render_workspace_reference};

use crate::actor::Channel;

/// Unicode-character budget after removing Markdown from an SMS reply.
pub const SMS_LIMIT: usize = 480;

/// Presentation selected from the initiating actor's channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponsePolicy {
    pub channel: Channel,
    pub max_characters: Option<usize>,
    pub markdown: bool,
    pub local_paths: bool,
    pub changes_table: bool,
}

#[must_use]
pub const fn policy(channel: Channel) -> ResponsePolicy {
    match channel {
        Channel::Interactive => ResponsePolicy {
            channel,
            max_characters: None,
            markdown: true,
            local_paths: true,
            changes_table: true,
        },
        Channel::Sms => ResponsePolicy {
            channel,
            max_characters: Some(SMS_LIMIT),
            markdown: false,
            local_paths: false,
            changes_table: false,
        },
        Channel::Email => ResponsePolicy {
            channel,
            max_characters: None,
            markdown: true,
            local_paths: false,
            changes_table: false,
        },
    }
}

#[cfg(test)]
mod tests;
