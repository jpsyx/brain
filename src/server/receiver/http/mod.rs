//! Provider authentication after an ingress has selected one live workspace.

mod email;
mod sms;

use anyhow::Context as _;

use super::{AttachmentRef, Channel};

pub(in crate::server::receiver) use email::refresh_attachment_access;

pub(super) const WEBHOOK_BODY_LIMIT: usize = 1024 * 1024;
pub(in crate::server) const RECEIVER_HANDLER_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
pub(in crate::server) const RECEIVER_JOB_HANDOFF_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(2);
pub(in crate::server) const RECEIVER_RESPONSE_RESERVE: std::time::Duration =
    std::time::Duration::from_secs(5);
pub(super) const RESEND_FETCH_TIMEOUT_SECONDS: u64 = 10;
const MAX_RESEND_FETCHES: u64 = 2;
const _: () = assert!(
    RESEND_FETCH_TIMEOUT_SECONDS * MAX_RESEND_FETCHES
        + RECEIVER_JOB_HANDOFF_TIMEOUT.as_secs()
        + RECEIVER_RESPONSE_RESERVE.as_secs()
        < RECEIVER_HANDLER_TIMEOUT.as_secs()
);

/// The one machine-wide webhook URL a provider portal posts a channel to.
///
/// It carries no workspace identity: the destination number or address inside
/// the payload selects the workspace (see
/// [`crate::server::receiver::routing`]), so every workspace on a machine
/// shares one URL per channel.
pub(crate) fn receiver_webhook_url(public_base_url: &str, channel: Channel) -> String {
    format!(
        "{}{}",
        public_base_url.trim_end_matches('/'),
        webhook_path(channel)
    )
}

/// The path component of one channel's webhook. Pure.
pub(crate) const fn webhook_path(channel: Channel) -> &'static str {
    match channel {
        Channel::Sms => "/sms",
        Channel::Email => "/email",
    }
}

/// Every destination address a provider payload names, before any signature has
/// been checked. Pure.
///
/// Used only to select which workspace's signing credential the request is then
/// verified against; see [`crate::server::receiver::routing`].
pub(in crate::server) fn destinations(channel: Channel, body: &[u8]) -> Vec<String> {
    match channel {
        Channel::Sms => sms::destinations(body),
        Channel::Email => email::destinations(body),
    }
}

#[derive(Debug, Clone)]
pub(in crate::server) struct ProviderConfig {
    pub workspace_id: crate::workspace::WorkspaceId,
    pub twilio_auth_token: String,
    pub twilio_from_number: String,
    pub public_base_url: String,
    pub resend_signing_secret: String,
    pub resend_full_access_api_key: String,
    pub resend_from_email: String,
}

impl ProviderConfig {
    pub(super) fn load(
        route: &crate::server::workspace_route::ResolvedWorkspaceRoute,
    ) -> anyhow::Result<Self> {
        let registry = crate::workspace::RegistryStore::load_from(route.registry_store().path())
            .context("loading selected workspace provider configuration")?;
        let selected = registry
            .select(Some(route.context().name().as_str()))
            .context("selecting provider configuration workspace")?;
        anyhow::ensure!(
            selected.record().workspace_id == route.context().id(),
            "provider configuration workspace changed after routing"
        );
        Ok(Self::from_records(
            route.context().id(),
            &selected.record().env,
            &registry.env,
        ))
    }

    pub(in crate::server) fn load_for_workspace(
        store: &crate::workspace::RegistryStore,
        workspace_id: crate::workspace::WorkspaceId,
    ) -> anyhow::Result<Self> {
        let registry = crate::workspace::RegistryStore::load_from(store.path())
            .context("loading routed workspace provider configuration")?;
        let record = registry
            .workspaces
            .values()
            .find(|record| record.workspace_id == workspace_id)
            .context("routed receiver workspace no longer exists")?;
        Ok(Self::from_records(workspace_id, &record.env, &registry.env))
    }

    /// Provider credentials from one workspace record plus the machine-global
    /// values every workspace on this machine shares. Pure.
    fn from_records(
        workspace_id: crate::workspace::WorkspaceId,
        record_env: &serde_json::Map<String, serde_json::Value>,
        machine_env: &serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        let string_of = |env: &serde_json::Map<String, serde_json::Value>, name: &str| {
            env.get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Self {
            workspace_id,
            twilio_auth_token: string_of(record_env, "twilio_auth_token"),
            twilio_from_number: string_of(record_env, "twilio_from_number"),
            // One machine serves one public origin, so this is machine-global:
            // the URL Twilio signs is identical for every workspace here.
            public_base_url: string_of(machine_env, "brain_receiver_public_url"),
            resend_signing_secret: string_of(record_env, "resend_webhook_signing_secret"),
            resend_full_access_api_key: string_of(record_env, "resend_full_access_api_key"),
            resend_from_email: string_of(record_env, "resend_from_email"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::server) enum ProviderError {
    BodyTooLarge,
    MalformedBody,
    NotConfigured(&'static str),
    InvalidSignature(&'static str),
    SenderNotAllowed(&'static str),
    InvalidRequest(&'static str),
    IgnoredEvent,
    Upstream(&'static str),
    Deadline,
}

impl ProviderError {
    pub(in crate::server) const fn status(self) -> u16 {
        match self {
            Self::IgnoredEvent => 202,
            Self::InvalidSignature(_) => 401,
            Self::SenderNotAllowed(_) => 403,
            Self::BodyTooLarge => 413,
            Self::Upstream(_) => 502,
            Self::NotConfigured(_) | Self::Deadline => 503,
            Self::MalformedBody | Self::InvalidRequest(_) => 400,
        }
    }

    pub(in crate::server) const fn unavailable(self) -> bool {
        matches!(self, Self::NotConfigured(_) | Self::Deadline)
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::BodyTooLarge => "webhook body is too large",
            Self::MalformedBody => "webhook body is malformed",
            Self::NotConfigured(message)
            | Self::InvalidSignature(message)
            | Self::SenderNotAllowed(message)
            | Self::InvalidRequest(message)
            | Self::Upstream(message) => message,
            Self::IgnoredEvent => "event ignored",
            Self::Deadline => "receiver handler deadline elapsed",
        })
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Clone)]
pub(super) struct AuthenticatedInbound {
    pub channel: Channel,
    pub sender: String,
    pub prompt: String,
    pub participants: Vec<String>,
    pub attachments: Vec<AttachmentRef>,
    pub receiving_address: String,
    pub provider_id: Option<String>,
    pub email_reply: Option<super::EmailReplyContext>,
}

/// Authenticate one already-read provider body against the routed workspace.
///
/// The body arrives read: the shared boundary needs it before this point to
/// learn which workspace the message was addressed to, and a request body can
/// only be consumed once.
pub(super) fn authenticate(
    request: &mut crate::server::http::Request,
    body: &[u8],
    config: &ProviderConfig,
    channel: Channel,
) -> Result<AuthenticatedInbound, ProviderError> {
    let pending_email = match channel {
        Channel::Sms => {
            let inbound = sms::authenticate(request, body, config)?;
            confirm_destination(config, channel, body)?;
            request
                .begin_handler_phase()
                .map_err(|_| ProviderError::Deadline)?;
            return Ok(inbound);
        }
        Channel::Email => {
            let verified = email::verify(request, body, config)?;
            confirm_destination(config, channel, body)?;
            verified
        }
    };
    if crate::server::receiver::dispatch::provider_delivery_was_discarded(
        config.workspace_id,
        Channel::Email,
        pending_email.webhook_id(),
    ) {
        return Err(ProviderError::IgnoredEvent);
    }
    request
        .begin_handler_phase()
        .map_err(|_| ProviderError::Deadline)?;
    email::fetch(pending_email, config)
}

/// Re-check, on now-authenticated bytes, that the message really was addressed
/// to the workspace it was routed to.
///
/// Routing reads the destination before any signature is verified, which is
/// safe on its own (it only picks whose credential to check). This closes the
/// loop for the case where several workspaces share one provider account and so
/// share one signing credential: the signature alone could not tell them apart,
/// but the address the provider signed can.
fn confirm_destination(
    config: &ProviderConfig,
    channel: Channel,
    body: &[u8],
) -> Result<(), ProviderError> {
    let published = match channel {
        Channel::Sms => &config.twilio_from_number,
        Channel::Email => &config.resend_from_email,
    };
    let Some(published) = crate::server::receiver::routing::normalize_address(channel, published)
    else {
        return Err(ProviderError::NotConfigured(
            "workspace receiver has no configured address",
        ));
    };
    let addressed = destinations(channel, body)
        .iter()
        .filter_map(|destination| {
            crate::server::receiver::routing::normalize_address(channel, destination)
        })
        .any(|destination| destination == published);
    if addressed {
        return Ok(());
    }
    Err(ProviderError::InvalidRequest(
        "webhook destination is not this workspace's receiver address",
    ))
}

pub(in crate::server) fn verify_unavailable_email(
    request: &crate::server::http::Request,
    body: &[u8],
    config: &ProviderConfig,
) -> Result<String, ProviderError> {
    email::verify(request, body, config).map(|verified| verified.webhook_id().to_owned())
}

/// Read one provider webhook body under its fixed limit.
///
/// The shared boundary calls this before routing, since the destination that
/// selects the workspace lives inside the body.
pub(in crate::server) fn read_webhook_body(
    request: &mut crate::server::http::Request,
) -> Result<Vec<u8>, ProviderError> {
    request
        .read_body(WEBHOOK_BODY_LIMIT)
        .map_err(|error| match error {
            crate::server::http::BodyError::TooLarge => ProviderError::BodyTooLarge,
            crate::server::http::BodyError::Io(_) | crate::server::http::BodyError::Malformed => {
                ProviderError::MalformedBody
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{Channel, ProviderConfig, ProviderError, confirm_destination, destinations};

    fn config() -> ProviderConfig {
        ProviderConfig {
            workspace_id: crate::workspace::WorkspaceId::new(),
            twilio_auth_token: "token".to_owned(),
            twilio_from_number: "+13105550111".to_owned(),
            public_base_url: "https://brain.example.test".to_owned(),
            resend_signing_secret: "secret".to_owned(),
            resend_full_access_api_key: String::new(),
            resend_from_email: "family@example.test".to_owned(),
        }
    }

    #[test]
    fn the_url_is_the_same_for_every_workspace_on_the_machine() {
        assert_eq!(
            super::receiver_webhook_url("https://brain.example.test", Channel::Sms),
            "https://brain.example.test/sms"
        );
        assert_eq!(
            super::receiver_webhook_url("https://brain.example.test", Channel::Email),
            "https://brain.example.test/email"
        );
    }

    #[test]
    fn an_authenticated_body_must_name_the_address_it_was_routed_to() {
        assert!(
            confirm_destination(
                &config(),
                Channel::Sms,
                b"Body=hi&From=%2B12125550100&To=%2B13105550111"
            )
            .is_ok()
        );
        assert!(
            confirm_destination(
                &config(),
                Channel::Email,
                br#"{"type":"email.received","data":{"to":["Family <FAMILY@example.test>"]}}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn a_body_addressed_elsewhere_is_refused_after_its_signature_verified() {
        // Reached only when the registry changed under a live route, or when two
        // workspaces share one provider credential. Either way the verified
        // destination, not the routing guess, has the last word.
        let rejected = confirm_destination(
            &config(),
            Channel::Sms,
            b"Body=hi&From=%2B12125550100&To=%2B19995550000",
        )
        .expect_err("a peer's destination must not pass");

        assert!(matches!(rejected, ProviderError::InvalidRequest(_)));
    }

    #[test]
    fn a_workspace_with_no_published_address_can_confirm_nothing() {
        let unconfigured = ProviderConfig {
            twilio_from_number: String::new(),
            ..config()
        };

        let rejected = confirm_destination(
            &unconfigured,
            Channel::Sms,
            b"Body=hi&From=%2B12125550100&To=%2B13105550111",
        )
        .expect_err("an unset receiver address cannot confirm a destination");

        assert!(matches!(rejected, ProviderError::NotConfigured(_)));
    }

    #[test]
    fn a_body_that_names_no_destination_yields_no_candidate() {
        assert!(destinations(Channel::Sms, b"Body=hi").is_empty());
        assert!(destinations(Channel::Email, b"not json").is_empty());
        assert_eq!(
            destinations(
                Channel::Email,
                br#"{"data":{"to":"one@example.test","cc":["two@example.test"]}}"#
            ),
            ["one@example.test", "two@example.test"]
        );
    }
}
