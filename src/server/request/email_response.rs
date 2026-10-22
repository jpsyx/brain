use crate::server::http;

pub(super) fn rejected(code: &str, message: &str) -> http::Response {
    outcome(200, code, message)
}

pub(super) fn unrouted(status: u16) -> http::Response {
    match status {
        403 => rejected(
            "recipient_domain_not_allowed",
            "no email recipient satisfies the configured domain policy",
        ),
        404 => rejected("unknown_recipient", "no receiver accepts this recipient"),
        503 => outcome(
            503,
            "ambiguous_recipient",
            "multiple workspaces match the email recipients",
        ),
        _ => outcome(
            status,
            "receiver_configuration_error",
            "receiver routing configuration could not be loaded",
        ),
    }
}

pub(super) fn failure(status: u16, unavailable: bool) -> http::Response {
    let (code, message) = if unavailable {
        (
            "receiver_unavailable",
            crate::server::receiver::unavailable_message(),
        )
    } else {
        match status {
            202 => ("ignored_event", "event ignored"),
            400 => ("invalid_request", "webhook payload cannot be processed"),
            401 => ("invalid_signature", "webhook signature verification failed"),
            403 => ("sender_not_allowed", "email sender is not allowed"),
            413 => ("body_too_large", "webhook body is too large"),
            502 => ("upstream_error", "received email could not be retrieved"),
            503 => (
                "receiver_not_ready",
                "receiver cannot process this webhook yet",
            ),
            _ => ("internal_error", "receiver could not process this webhook"),
        }
    };
    outcome(
        super::provider_http_status(status, unavailable, crate::server::receiver::Channel::Email),
        code,
        message,
    )
}

fn outcome(status: u16, code: &str, message: &str) -> http::Response {
    let line = format!("receiver email outcome status={status} code={code} error={message}");
    #[cfg(not(test))]
    crate::server::lifecycle::append_event_log(&line);
    crate::logging::log(line);
    http::Response::json(
        status,
        serde_json::json!({
            "ok": false,
            "queued": false,
            "error_code": code,
            "error": message,
        })
        .to_string(),
    )
}
