//! Channel-specific final-response shaping.

mod html;
mod plain_text;

pub use html::email_html;
pub use plain_text::strip_markdown;
use serde::Serialize;

pub const SMS_LIMIT: usize = 480;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplyEnvelope {
    pub channel: &'static str,
    pub text: String,
    pub long_form_available: bool,
}

/// Shape the final SMS body. Markup is removed *before* the length decision:
/// a phone renders none of it, so it must not consume the budget or trigger a
/// needless "ask for a longer reply".
#[must_use]
pub fn sms(text: &str) -> ReplyEnvelope {
    let plain = strip_markdown(text);
    let clean = plain.as_str();
    if clean.chars().count() <= SMS_LIMIT {
        return ReplyEnvelope {
            channel: "sms",
            text: clean.to_owned(),
            long_form_available: false,
        };
    }
    let mut shortened = clean
        .chars()
        .take(SMS_LIMIT.saturating_sub(78))
        .collect::<String>();
    shortened.push_str("… Ask for a longer reply and I’ll email the full answer.");
    ReplyEnvelope {
        channel: "sms",
        text: shortened,
        long_form_available: true,
    }
}

#[must_use]
pub fn email(text: &str) -> ReplyEnvelope {
    ReplyEnvelope {
        channel: "email",
        text: text.trim().to_owned(),
        long_form_available: false,
    }
}

#[must_use]
pub fn processing_notice(channel: &'static str) -> ReplyEnvelope {
    ReplyEnvelope {
        channel,
        text: "Your message was received and is still being processed. I’ll send the full response when it’s ready.".to_owned(),
        long_form_available: false,
    }
}

/// Told to a sender whose turn was abandoned without ever producing an answer.
///
/// Silence is the worst outcome: the sender has already been promised a reply,
/// so an unanswered turn says so plainly and invites a retry.
#[must_use]
pub fn unanswered_notice(channel: &'static str) -> ReplyEnvelope {
    ReplyEnvelope {
        channel,
        text: "Sorry — I couldn’t process that message. Please try sending it again.".to_owned(),
        long_form_available: false,
    }
}

/// Confirms that the channel's conversation was replaced with a fresh one.
///
/// Starting over is invisible from a phone — the next answer simply stops
/// referring to what came before — so the boundary is stated explicitly. A
/// sender who is told nothing cannot tell a new session from an ignored
/// command.
#[must_use]
pub fn new_session_notice(channel: &'static str) -> ReplyEnvelope {
    ReplyEnvelope {
        channel,
        text: "Started a fresh conversation. I’ve set aside everything from the previous one."
            .to_owned(),
        long_form_available: false,
    }
}

/// Confirms a restart, and says how much waiting work it discarded.
#[must_use]
pub fn restart_notice(channel: &'static str, dropped: usize) -> ReplyEnvelope {
    let text = match dropped {
        0 => "Restarted. Nothing was waiting.".to_owned(),
        1 => "Restarted. 1 waiting message was dropped, and its sender has been told.".to_owned(),
        count => format!(
            "Restarted. {count} waiting messages were dropped, and their senders have been told."
        ),
    };
    ReplyEnvelope {
        channel,
        text,
        long_form_available: false,
    }
}

/// Verify that a completion artifact belongs to the immutable launched actor.
#[must_use]
pub fn completion_matches_actor(
    value: &serde_json::Value,
    actor: &crate::actor::ActorContext,
) -> bool {
    value.get("actor_id").and_then(serde_json::Value::as_str) == Some(actor.user_id().as_str())
        && value.get("channel").and_then(serde_json::Value::as_str)
            == Some(actor.channel().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sms_is_stripped_of_markup_a_phone_cannot_render() {
        let reply =
            sms("## Today\n\n- **Rent** is due\n- see [the invoice](https://example.test/a)");
        assert_eq!(
            reply.text, "Today\n\n- Rent is due\n- see the invoice (https://example.test/a)",
            "SMS carries no markup, so the markers are wasted characters"
        );
    }

    #[test]
    fn markup_is_removed_before_the_limit_is_measured() {
        let padded = format!("**{}**", "x".repeat(SMS_LIMIT));
        let reply = sms(&padded);
        assert!(
            !reply.long_form_available,
            "the four asterisks must not be what pushes a reply over the limit"
        );
        assert_eq!(reply.text.chars().count(), SMS_LIMIT);
    }

    #[test]
    fn sms_stays_within_the_medium_limit() {
        let reply = sms(&"x".repeat(1000));
        assert!(reply.text.chars().count() <= SMS_LIMIT);
        assert!(reply.long_form_available);
    }

    #[test]
    fn short_sms_is_not_rewritten() {
        assert_eq!(sms("Done").text, "Done");
        assert!(!sms("Done").long_form_available);
    }

    #[test]
    fn email_preserves_full_text_and_escapes_html() {
        assert_eq!(email("# Heading\n\nDetails").text, "# Heading\n\nDetails");
        assert!(email_html("<unsafe>").contains("&lt;unsafe&gt;"));
    }

    /// A sender who was promised a reply and then told nothing has no way to
    /// know the message died. The notice has to say it failed and ask for a
    /// retry, on whichever channel the message arrived on.
    #[test]
    fn an_abandoned_message_tells_its_sender_to_try_again() {
        for channel in ["sms", "email"] {
            let notice = unanswered_notice(channel);
            assert_eq!(notice.channel, channel);
            assert!(!notice.long_form_available);
            let text = notice.text.to_lowercase();
            assert!(text.contains("again"), "no retry instruction: {text}");
            assert!(
                notice.text.chars().count() <= SMS_LIMIT,
                "must fit one SMS: {}",
                notice.text
            );
            assert_ne!(
                notice.text,
                processing_notice(channel).text,
                "a failure must not read like the still-working notice"
            );
        }
    }

    /// Every acknowledgement brain sends has to survive the narrowest channel
    /// it can go out on, and has to be distinguishable from the others — a
    /// sender reads these instead of seeing any UI.
    #[test]
    fn control_acknowledgements_fit_one_sms_and_never_read_alike() {
        for channel in ["sms", "email"] {
            let notices = [
                new_session_notice(channel).text,
                restart_notice(channel, 0).text,
                restart_notice(channel, 1).text,
                restart_notice(channel, 4).text,
                processing_notice(channel).text,
                unanswered_notice(channel).text,
            ];
            for notice in &notices {
                assert!(
                    notice.chars().count() <= SMS_LIMIT,
                    "must fit one SMS: {notice}"
                );
            }
            let unique = notices.iter().collect::<std::collections::BTreeSet<_>>();
            assert_eq!(unique.len(), notices.len(), "two notices read the same");
        }
    }

    /// The count is the whole point of the restart acknowledgement: it tells
    /// the sender how many other people were just told to resend.
    #[test]
    fn a_restart_acknowledgement_reports_how_much_it_discarded() {
        assert!(restart_notice("sms", 0).text.contains("Nothing"));
        assert!(restart_notice("sms", 1).text.contains('1'));
        assert!(restart_notice("sms", 7).text.contains('7'));
    }

    #[test]
    fn completion_must_match_the_session_actor_and_channel() {
        let users = crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![crate::users::User {
                id: crate::users::UserId::parse("member").unwrap(),
                name: "Member".to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        };
        let actor = crate::actor::resolve_actor(
            &crate::users::UserId::parse("member").unwrap(),
            crate::actor::RequestIdentity::Local,
            &users,
        )
        .unwrap();
        assert!(completion_matches_actor(
            &serde_json::json!({"actor_id":"member","channel":"interactive"}),
            &actor,
        ));
        assert!(!completion_matches_actor(
            &serde_json::json!({"actor_id":"other","channel":"interactive"}),
            &actor,
        ));
    }
}
