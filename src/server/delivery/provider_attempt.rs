use serde::Deserialize;

use crate::state::{
    ReceiverDeliveryAmbiguity, ReceiverDeliveryEnvelope, ReceiverDeliveryErrorCategory,
    ReceiverDeliveryId, ReceiverProviderCapability, ReceiverProviderReference,
    ReceiverProviderResultClass,
};

#[cfg(not(test))]
use super::super::provider::CurlCancellation;
use super::super::provider::CurlRequest;

pub(crate) const PROVIDER_RESPONSE_LIMIT: usize = 16 * 1024;
const HTTP_STATUS_MARKER: &str = "\n__brain_http_status__:";

/// Content-free process boundary failure for one provider attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverProviderProcessFailure {
    Preflight,
    Credentials,
    Spawn,
    Timeout,
    ProcessExit,
    ResponseTooLarge,
    Cancelled,
    LostResultChannel,
}

#[derive(Deserialize)]
struct ResendSuccess {
    id: String,
}

#[derive(Deserialize)]
struct ResendError {
    name: String,
}

#[derive(Deserialize)]
struct TwilioSuccess {
    sid: String,
}

pub(crate) fn classify_provider_http_response(
    provider: ReceiverProviderCapability,
    status: u16,
    body: &[u8],
) -> ReceiverProviderResultClass {
    if body.len() > PROVIDER_RESPONSE_LIMIT {
        return malformed_acknowledgement();
    }
    if status == 429 {
        return ReceiverProviderResultClass::DefinitelyNotAccepted(
            ReceiverDeliveryErrorCategory::TransportUnavailable,
        );
    }
    if status >= 500 {
        return ReceiverProviderResultClass::Ambiguous(
            ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
        );
    }
    if provider == ReceiverProviderCapability::Resend && status == 409 {
        return classify_resend_conflict(body);
    }
    if !(200..300).contains(&status) {
        return ReceiverProviderResultClass::PermanentlyRejected(
            ReceiverDeliveryErrorCategory::ProviderRejected,
        );
    }
    let reference = match provider {
        ReceiverProviderCapability::Resend => serde_json::from_slice::<ResendSuccess>(body)
            .ok()
            .and_then(|success| parse_resend_id(&success.id)),
        ReceiverProviderCapability::Twilio => serde_json::from_slice::<TwilioSuccess>(body)
            .ok()
            .and_then(|success| parse_twilio_sid(&success.sid)),
    };
    reference.map_or_else(
        malformed_acknowledgement,
        ReceiverProviderResultClass::Acknowledged,
    )
}

fn classify_resend_conflict(body: &[u8]) -> ReceiverProviderResultClass {
    let concurrent = serde_json::from_slice::<ResendError>(body)
        .is_ok_and(|error| error.name == "concurrent_idempotent_requests");
    if concurrent {
        ReceiverProviderResultClass::Ambiguous(ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown)
    } else {
        ReceiverProviderResultClass::PermanentlyRejected(
            ReceiverDeliveryErrorCategory::ProviderRejected,
        )
    }
}

pub(crate) fn classify_provider_process_failure(
    failure: ReceiverProviderProcessFailure,
) -> ReceiverProviderResultClass {
    match failure {
        ReceiverProviderProcessFailure::Preflight => {
            ReceiverProviderResultClass::PermanentlyRejected(
                ReceiverDeliveryErrorCategory::InvalidRequest,
            )
        }
        ReceiverProviderProcessFailure::Credentials => {
            ReceiverProviderResultClass::PermanentlyRejected(
                ReceiverDeliveryErrorCategory::Credentials,
            )
        }
        ReceiverProviderProcessFailure::Spawn => {
            ReceiverProviderResultClass::DefinitelyNotAccepted(
                ReceiverDeliveryErrorCategory::TransportUnavailable,
            )
        }
        ReceiverProviderProcessFailure::Timeout
        | ReceiverProviderProcessFailure::ProcessExit
        | ReceiverProviderProcessFailure::Cancelled
        | ReceiverProviderProcessFailure::LostResultChannel => {
            ReceiverProviderResultClass::Ambiguous(
                ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
            )
        }
        ReceiverProviderProcessFailure::ResponseTooLarge => malformed_acknowledgement(),
    }
}

fn malformed_acknowledgement() -> ReceiverProviderResultClass {
    ReceiverProviderResultClass::Ambiguous(
        ReceiverDeliveryAmbiguity::ProviderAcknowledgementMalformed,
    )
}

fn parse_resend_id(value: &str) -> Option<ReceiverProviderReference> {
    uuid::Uuid::parse_str(value).ok()?;
    ReceiverProviderReference::parse(value).ok()
}

fn parse_twilio_sid(value: &str) -> Option<ReceiverProviderReference> {
    let hex = value.strip_prefix("SM")?;
    if hex.len() != 32 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    ReceiverProviderReference::parse(value).ok()
}

#[cfg(not(test))]
pub(crate) fn deliver_receiver_claim(
    command: &crate::workspace::CommandContext,
    claim: &crate::state::ReceiverDeliveryClaim,
    cancellation: &CurlCancellation,
) -> ReceiverProviderResultClass {
    match claim.envelope() {
        ReceiverDeliveryEnvelope::Sms { value } => deliver_sms(command, value, cancellation),
        ReceiverDeliveryEnvelope::Email { value } => {
            deliver_email(command, claim.delivery_id(), value, cancellation)
        }
    }
}

#[cfg(not(test))]
fn deliver_sms(
    command: &crate::workspace::CommandContext,
    envelope: &crate::state::ReceiverSmsEnvelope,
    cancellation: &CurlCancellation,
) -> ReceiverProviderResultClass {
    let Some(account) = super::super::provider::get(command, "twilio_account_sid") else {
        return classify_provider_process_failure(ReceiverProviderProcessFailure::Credentials);
    };
    let Some(token) = super::super::provider::get(command, "twilio_auth_token") else {
        return classify_provider_process_failure(ReceiverProviderProcessFailure::Credentials);
    };
    let endpoint = format!("https://api.twilio.com/2010-04-01/Accounts/{account}/Messages.json");
    let request = CurlRequest::new()
        .flag("silent")
        .option("connect-timeout", "10")
        .option("max-time", "30")
        .option("user", &format!("{account}:{token}"))
        .option("request", "POST")
        .option("url", &endpoint)
        .option("data-urlencode", &format!("To={}", envelope.recipient()))
        .option("data-urlencode", &format!("From={}", envelope.sender()))
        .option("data-urlencode", &format!("Body={}", envelope.body()))
        .option("write-out", &format!("{HTTP_STATUS_MARKER}%{{http_code}}"));
    run_provider_request(ReceiverProviderCapability::Twilio, request, cancellation)
}

#[cfg(not(test))]
fn deliver_email(
    command: &crate::workspace::CommandContext,
    delivery_id: ReceiverDeliveryId,
    envelope: &crate::state::ReceiverEmailEnvelope,
    cancellation: &CurlCancellation,
) -> ReceiverProviderResultClass {
    let Some(key) = super::super::provider::get(command, "resend_sending_api_key") else {
        return classify_provider_process_failure(ReceiverProviderProcessFailure::Credentials);
    };
    let Ok(request) = resend_request(&key, delivery_id, envelope) else {
        return classify_provider_process_failure(ReceiverProviderProcessFailure::Preflight);
    };
    run_provider_request(ReceiverProviderCapability::Resend, request, cancellation)
}

fn resend_request(
    key: &str,
    delivery_id: ReceiverDeliveryId,
    envelope: &crate::state::ReceiverEmailEnvelope,
) -> anyhow::Result<CurlRequest> {
    anyhow::ensure!(
        crate::users::normalize_mailbox(envelope.sender()).is_ok()
            && envelope.sender().trim() == envelope.sender(),
        "receiver email sender is invalid"
    );
    let payload = super::email_payload(
        envelope.sender(),
        envelope.recipients(),
        envelope.subject(),
        envelope.text(),
        envelope.html(),
        envelope.in_reply_to(),
    );
    Ok(CurlRequest::new()
        .flag("silent")
        .option("connect-timeout", "10")
        .option("max-time", "30")
        .option("request", "POST")
        .option("url", "https://api.resend.com/emails")
        .option("header", &format!("Authorization: Bearer {key}"))
        .option("header", "Content-Type: application/json")
        .option("header", &format!("Idempotency-Key: {delivery_id}"))
        .option("data", &payload.to_string())
        .option("write-out", &format!("{HTTP_STATUS_MARKER}%{{http_code}}")))
}

#[cfg(not(test))]
fn run_provider_request(
    provider: ReceiverProviderCapability,
    request: CurlRequest,
    cancellation: &CurlCancellation,
) -> ReceiverProviderResultClass {
    let output_limit = PROVIDER_RESPONSE_LIMIT.saturating_add(HTTP_STATUS_MARKER.len() + 3);
    match request.output_limited_cancellable(output_limit, cancellation) {
        Ok(output) => classify_provider_process_output(
            provider,
            output.status.success(),
            output.status.code(),
            &output.stdout,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            classify_provider_process_failure(ReceiverProviderProcessFailure::Spawn)
        }
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            classify_provider_process_failure(ReceiverProviderProcessFailure::Cancelled)
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            classify_provider_process_failure(ReceiverProviderProcessFailure::ResponseTooLarge)
        }
        Err(_) => classify_provider_process_failure(ReceiverProviderProcessFailure::ProcessExit),
    }
}

pub(crate) fn classify_provider_process_output(
    provider: ReceiverProviderCapability,
    process_success: bool,
    exit_code: Option<i32>,
    output: &[u8],
) -> ReceiverProviderResultClass {
    if !process_success {
        let failure = if exit_code == Some(28) {
            ReceiverProviderProcessFailure::Timeout
        } else {
            ReceiverProviderProcessFailure::ProcessExit
        };
        return classify_provider_process_failure(failure);
    }
    split_http_output(output).map_or_else(
        || classify_provider_process_failure(ReceiverProviderProcessFailure::ProcessExit),
        |(body, status)| classify_provider_http_response(provider, status, body),
    )
}

fn split_http_output(output: &[u8]) -> Option<(&[u8], u16)> {
    let marker = HTTP_STATUS_MARKER.as_bytes();
    let start = output
        .windows(marker.len())
        .rposition(|window| window == marker)?;
    let status_bytes = output.get(start + marker.len()..)?;
    if status_bytes.len() != 3 || !status_bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let status = std::str::from_utf8(status_bytes).ok()?.parse().ok()?;
    Some((&output[..start], status))
}

#[cfg(test)]
#[derive(PartialEq, Eq)]
pub(super) struct ResendRequestProof {
    digest: [u8; 32],
    exact_delivery_key: bool,
    idempotency_header_count: usize,
    frozen_sender: bool,
    authorization_header_count: usize,
}

#[cfg(test)]
impl ResendRequestProof {
    pub(super) const fn has_exact_delivery_key(&self) -> bool {
        self.exact_delivery_key
    }

    pub(super) const fn idempotency_header_count(&self) -> usize {
        self.idempotency_header_count
    }

    pub(super) const fn uses_frozen_sender(&self) -> bool {
        self.frozen_sender
    }

    pub(super) const fn has_one_authorization_header(&self) -> bool {
        self.authorization_header_count == 1
    }
}

#[cfg(test)]
impl std::fmt::Debug for ResendRequestProof {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResendRequestProof(<redacted>)")
    }
}

#[cfg(test)]
pub(super) fn resend_request_for_test(
    key: &str,
    delivery_id: ReceiverDeliveryId,
    envelope: &ReceiverDeliveryEnvelope,
) -> anyhow::Result<ResendRequestProof> {
    let ReceiverDeliveryEnvelope::Email { value } = envelope else {
        anyhow::bail!("receiver delivery is not email")
    };
    let request = resend_request(key, delivery_id, value)?;
    let delivery_key = format!("Idempotency-Key: {delivery_id}");
    let payload = super::email_payload(
        value.sender(),
        value.recipients(),
        value.subject(),
        value.text(),
        value.html(),
        value.in_reply_to(),
    )
    .to_string();
    Ok(ResendRequestProof {
        digest: request.redacted_digest_for_test(),
        exact_delivery_key: request.has_exact_option_for_test("header", &delivery_key),
        idempotency_header_count: request
            .option_prefix_count_for_test("header", "Idempotency-Key:"),
        frozen_sender: request.has_exact_option_for_test("data", &payload),
        authorization_header_count: request
            .option_prefix_count_for_test("header", "Authorization: Bearer "),
    })
}
