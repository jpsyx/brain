//! Receiver messages that steer the session instead of asking a question.

/// A message whose whole content is an instruction to brain itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    /// Abandon the channel's current conversation and start a fresh one.
    NewSession,
    /// Drop everything waiting and tell each sender their message was lost.
    Restart,
}

impl ControlCommand {
    /// The literal a sender types, for help text and diagnostics.
    #[must_use]
    pub const fn literal(self) -> &'static str {
        match self {
            Self::NewSession => "/new",
            Self::Restart => "/restart",
        }
    }
}

/// Read a message as a control command, or `None` if it is ordinary work.
///
/// The whole message must be the command. A command mentioned inside a real
/// question ("what does /new do?", "restart the sync and tell me") is a
/// question, and silently swallowing it as an instruction would be the worst
/// possible reading: the sender is told nothing happened to a message they
/// expected an answer to. Surrounding whitespace is forgiven because a phone
/// keyboard adds a trailing space or newline on its own, and case is forgiven
/// because a phone capitalizes the first letter of a message by default —
/// `/New` is a typo the sender cannot even see themselves make.
#[must_use]
pub fn parse(prompt: &str) -> Option<ControlCommand> {
    let trimmed = prompt.trim();
    [ControlCommand::NewSession, ControlCommand::Restart]
        .into_iter()
        .find(|command| trimmed.eq_ignore_ascii_case(command.literal()))
}

/// What a restart found waiting when it arrived.
#[derive(Clone, PartialEq, Eq)]
pub struct RestartPlan<T> {
    /// The restart command itself, whose sender is owed an acknowledgement.
    pub command: T,
    /// Work that was waiting and will never be answered.
    pub dropped: Vec<T>,
}

impl<T> std::fmt::Debug for RestartPlan<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RestartPlan(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_commands_are_recognized_however_a_phone_capitalizes_them() {
        for text in ["/new", "/NEW", "/NeW", "  /new  ", "/new\n"] {
            assert_eq!(
                parse(text),
                Some(ControlCommand::NewSession),
                "{text:?} must start a new session"
            );
        }
        for text in ["/restart", "/RESTART", "/ReStArT", " /restart\n"] {
            assert_eq!(
                parse(text),
                Some(ControlCommand::Restart),
                "{text:?} must restart"
            );
        }
    }

    /// A command is only a command when it is the entire message. Reading one
    /// out of a sentence would swallow a real question and answer it with
    /// silence, which is the failure this channel can least afford.
    #[test]
    fn a_command_mentioned_inside_a_real_message_is_still_a_real_message() {
        for text in [
            "what does /new do?",
            "/new session please",
            "restart",
            "//new",
            "/newest",
            "run /restart on the server",
            "",
            "   ",
        ] {
            assert_eq!(parse(text), None, "{text:?} must be answered, not obeyed");
        }
    }

    #[test]
    fn each_command_reports_the_literal_a_sender_types() {
        assert_eq!(ControlCommand::NewSession.literal(), "/new");
        assert_eq!(ControlCommand::Restart.literal(), "/restart");
    }
}
