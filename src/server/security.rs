//! Signature and sender authorization helpers for receiver webhooks.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;

type TwilioMac = Hmac<Sha1>;
type SvixMac = Hmac<Sha256>;

/// Compute Twilio's signature for an exact public URL and form body.
#[must_use]
pub fn twilio_signature(
    auth_token: &str,
    public_url: &str,
    fields: &BTreeMap<String, String>,
) -> String {
    let mut message = public_url.to_owned();
    for (key, value) in fields {
        message.push_str(key);
        message.push_str(value);
    }
    let mut mac = TwilioMac::new_from_slice(auth_token.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    mac.update(message.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Check a Twilio signature without exposing the secret to logs.
#[must_use]
pub fn verify_twilio(
    auth_token: &str,
    public_url: &str,
    fields: &BTreeMap<String, String>,
    provided: &str,
) -> bool {
    twilio_signature(auth_token, public_url, fields) == provided.trim()
}

/// Check a Resend/Svix signature. The signing secret is the base64 payload
/// after the `whsec_` prefix.
#[must_use]
pub fn verify_resend(
    signing_secret: &str,
    webhook_id: &str,
    timestamp: &str,
    raw_body: &[u8],
    provided: &str,
) -> bool {
    let secret = signing_secret
        .strip_prefix("whsec_")
        .unwrap_or(signing_secret);
    let Ok(key) = STANDARD.decode(secret) else {
        return false;
    };
    let Ok(mut mac) = SvixMac::new_from_slice(&key) else {
        return false;
    };
    mac.update(webhook_id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(raw_body);
    let expected = STANDARD.encode(mac.finalize().into_bytes());
    provided.split_whitespace().any(|value| {
        value
            .strip_prefix("v1=")
            .is_some_and(|candidate| candidate == expected)
    })
}

/// Exact, case-insensitive allowlist matching for an inbound sender.
#[must_use]
pub fn sender_allowed(sender: &str, allowed: &[String]) -> bool {
    let normalized = sender.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && allowed
            .iter()
            .any(|item| item.trim().eq_ignore_ascii_case(&normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twilio_signature_uses_sorted_form_fields() {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("Body".to_owned(), "hello".to_owned());
        fields.insert("From".to_owned(), "+1555".to_owned());
        let signature = twilio_signature("token", "https://example.test/sms", &fields);
        assert_eq!(signature, "CDaLvjkKwtQostjstDZguA1+V2s=");
    }

    #[test]
    fn allowlist_is_case_insensitive_and_exact() {
        assert!(sender_allowed(
            "Me@Example.com",
            &["me@example.com".to_owned()]
        ));
        assert!(!sender_allowed(
            "other@example.com",
            &["me@example.com".to_owned()]
        ));
    }
}
