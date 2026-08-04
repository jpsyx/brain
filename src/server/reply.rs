//! Channel-specific final-response shaping.

use serde::Serialize;

pub const SMS_LIMIT: usize = 480;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplyEnvelope {
    pub channel: &'static str,
    pub text: String,
    pub long_form_available: bool,
}

#[must_use]
pub fn sms(text: &str) -> ReplyEnvelope {
    let clean = text.trim();
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
pub fn email_html(text: &str) -> String {
    let body = text
        .trim()
        .split("\n\n")
        .map(|paragraph| format!("<p>{}</p>", escape_html(paragraph).replace('\n', "<br>")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<!doctype html><html><body style=\"margin:0;background:#f6f4ef;padding:32px;font-family:ui-sans-serif,system-ui,sans-serif;color:#252525\"><main style=\"max-width:680px;margin:auto;background:#fff;padding:32px;border-radius:16px;box-shadow:0 8px 30px #00000012\">{body}</main></body></html>"
    )
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[must_use]
pub fn processing_notice(channel: &'static str) -> ReplyEnvelope {
    ReplyEnvelope {
        channel,
        text: "Your message was received and is still being processed. I’ll send the full response when it’s ready.".to_owned(),
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
