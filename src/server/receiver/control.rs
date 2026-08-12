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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPlan<T> {
    /// The restart command itself, whose sender is owed an acknowledgement.
    pub command: T,
    /// Work that was waiting and will never be answered.
    pub dropped: Vec<T>,
}

/// Split a queue around the first restart command in it, leaving the survivors.
///
/// Everything queued *ahead* of the restart is dropped: that is the backlog the
/// sender is asking to be rid of. Everything behind it is kept, because it was
/// sent after the decision to restart and dropping it would lose a message
/// nobody asked to abandon. Returns `None` when no restart is queued, leaving
/// the queue untouched.
pub fn take_restart<T>(
    queue: &mut Vec<T>,
    is_restart: impl Fn(&T) -> bool,
) -> Option<RestartPlan<T>> {
    let at = queue.iter().position(is_restart)?;
    let survivors = queue.split_off(at + 1);
    let command = queue.pop().expect("the restart sits at the end of the split");
    let dropped = std::mem::replace(queue, survivors);
    Some(RestartPlan { command, dropped })
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

    /// A sender who restarts is clearing the backlog they are stuck behind.
    /// Anything they sent *after* deciding that is new work they still expect
    /// an answer to, so the restart is a cut through the queue, not a wipe.
    #[test]
    fn a_restart_drops_the_backlog_ahead_of_it_and_keeps_what_came_after() {
        let mut queue = vec!["first", "second", "/restart", "later"];
        let plan = take_restart(&mut queue, |job| *job == "/restart")
            .expect("a queued restart must be found");

        assert_eq!(plan.command, "/restart");
        assert_eq!(
            plan.dropped,
            vec!["first", "second"],
            "the backlog is what the sender is stuck behind"
        );
        assert_eq!(
            queue,
            vec!["later"],
            "work sent after the restart is still owed an answer"
        );
    }

    /// The common case is no restart at all, and it must not disturb the queue.
    #[test]
    fn a_queue_with_no_restart_is_left_exactly_as_it_was() {
        let mut queue = vec!["first", "second"];
        assert!(take_restart(&mut queue, |job| *job == "/restart").is_none());
        assert_eq!(queue, vec!["first", "second"]);
    }

    /// A restart that arrives with nothing waiting still has to acknowledge its
    /// own sender, and must not report phantom casualties.
    #[test]
    fn a_restart_with_an_empty_backlog_drops_nothing() {
        let mut queue = vec!["/restart"];
        let plan = take_restart(&mut queue, |job| *job == "/restart").expect("found");
        assert!(plan.dropped.is_empty());
        assert!(queue.is_empty());
    }

    /// Two restarts in one backlog: the first one wins and the second becomes
    /// ordinary queued work, which the next pass handles.
    #[test]
    fn only_the_first_restart_is_taken_in_one_pass() {
        let mut queue = vec!["a", "/restart", "b", "/restart"];
        let plan = take_restart(&mut queue, |job| *job == "/restart").expect("found");
        assert_eq!(plan.dropped, vec!["a"]);
        assert_eq!(queue, vec!["b", "/restart"]);
    }
}
