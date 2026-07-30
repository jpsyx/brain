//! Signature and sender authorization helpers for receiver webhooks.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;

type TwilioMac = Hmac<Sha1>;
type SvixMac = Hmac<Sha256>;

/// Compute Twilio's signature for an exact public URL and form body.
#[cfg(test)]
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
    let Ok(provided) = STANDARD.decode(provided.trim()) else {
        return false;
    };
    let mut message = public_url.to_owned();
    for (key, value) in fields {
        message.push_str(key);
        message.push_str(value);
    }
    let mut mac = TwilioMac::new_from_slice(auth_token.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of every length"));
    mac.update(message.as_bytes());
    mac.verify_slice(&provided).is_ok()
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
    const TIMESTAMP_TOLERANCE_SECS: u64 = 5 * 60;

    let Ok(timestamp_secs) = timestamp.parse::<u64>() else {
        return false;
    };
    let Ok(now_secs) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    if now_secs.as_secs().abs_diff(timestamp_secs) > TIMESTAMP_TOLERANCE_SECS {
        return false;
    }
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
    provided.split_whitespace().any(|value| {
        let candidate = value.strip_prefix("v1,");
        candidate
            .and_then(|candidate| STANDARD.decode(candidate).ok())
            .is_some_and(|candidate| mac.clone().verify_slice(&candidate).is_ok())
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

    fn resend_signature(key: &[u8], webhook_id: &str, timestamp: &str, body: &[u8]) -> String {
        let mut mac = SvixMac::new_from_slice(key).unwrap();
        mac.update(webhook_id.as_bytes());
        mac.update(b".");
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body);
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
    }

    fn now_unix() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    }

    #[test]
    fn twilio_signature_uses_sorted_form_fields() {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("Body".to_owned(), "hello".to_owned());
        fields.insert("From".to_owned(), "+1555".to_owned());
        let signature = twilio_signature("token", "https://example.test/sms", &fields);
        assert_eq!(signature, "CDaLvjkKwtQostjstDZguA1+V2s=");
    }

    #[test]
    fn twilio_verification_rejects_modified_content() {
        let mut fields = std::collections::BTreeMap::from([
            ("Body".to_owned(), "hello".to_owned()),
            ("From".to_owned(), "+1555".to_owned()),
        ]);
        let url = "https://example.test/sms";
        let signature = twilio_signature("token", url, &fields);

        assert!(verify_twilio("token", url, &fields, &signature));
        fields.insert("Body".to_owned(), "modified".to_owned());
        assert!(!verify_twilio("token", url, &fields, &signature));
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

    #[test]
    fn resend_rejects_a_correctly_signed_stale_replay() {
        let key = b"test signing secret";
        let secret = format!("whsec_{}", STANDARD.encode(key));
        let timestamp = "1";
        let body = br#"{"type":"email.received"}"#;
        let signature = resend_signature(key, "message-id", timestamp, body);

        assert!(!verify_resend(
            &secret,
            "message-id",
            timestamp,
            body,
            &signature
        ));
    }

    #[test]
    fn resend_accepts_the_official_comma_signature_format() {
        let key = b"test signing secret";
        let secret = format!("whsec_{}", STANDARD.encode(key));
        let timestamp = now_unix();
        let body = br#"{"type":"email.received"}"#;
        let signature = resend_signature(key, "message-id", &timestamp, body);

        assert!(verify_resend(
            &secret,
            "message-id",
            &timestamp,
            body,
            &signature
        ));
    }
}
