//! The `/triage/done` route: the ephemeral daily-triage session's
//! completion signal.
//!
//! Thin controller. The `/triage` skill POSTs `{"token": "<one-time-token>"}`
//! once a daily-triage pass truly finishes (PDF written, Morning Triage habit
//! marked). We record the token to the on-disk signal file
//! ([`crate::triage_signal`]); the running tasks view polls it and, when the
//! token matches the triage tab it opened, auto-closes that tab. The token
//! parsing is pure ([`crate::triage_signal::parse_token`]); this handler only
//! maps success/failure to an HTTP `(status, json)` pair, mirroring the
//! `/habits/done` shape.

use serde_json::json;

/// Handle a `POST /triage/done` body, returning the HTTP `(status, json_body)`
/// pair. A valid token is recorded (200); a missing/invalid token is a 400; a
/// write failure is a 500.
#[must_use]
pub fn done(body: &str) -> (u16, String) {
    let Some(token) = crate::triage_signal::parse_token(body) else {
        return (400, json!({ "error": "missing token" }).to_string());
    };
    match crate::triage_signal::record_done(&token) {
        Ok(()) => (200, json!({ "ok": true }).to_string()),
        Err(e) => (500, json!({ "error": e.to_string() }).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_token_is_a_400() {
        let (status, body) = done("{}");
        assert_eq!(status, 400);
        assert!(body.contains(r#""error":"missing token""#));
    }

    #[test]
    fn invalid_json_is_a_400() {
        let (status, _) = done("not json");
        assert_eq!(status, 400);
    }
}
