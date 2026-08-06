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

pub(crate) fn receiver_webhook_url(
    public_base_url: &str,
    ingress_id: crate::server::IngressId,
    channel: Channel,
) -> String {
    let channel = match channel {
        Channel::Sms => "sms",
        Channel::Email => "email",
    };
    format!(
        "{}/w/{ingress_id}/{channel}",
        public_base_url.trim_end_matches('/')
    )
}

#[derive(Debug, Clone)]
pub(in crate::server) struct ProviderConfig {
    pub workspace_id: crate::workspace::WorkspaceId,
    pub twilio_auth_token: String,
    pub public_base_url: String,
    pub resend_signing_secret: String,
    pub resend_api_key: String,
    pub resend_from_email: String,
    pub ingress_id: crate::server::IngressId,
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
        let get = |name: &str| {
            selected
                .record()
                .env
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Self {
            workspace_id: route.context().id(),
            twilio_auth_token: get("twilio_auth_token"),
            public_base_url: get("brain_receiver_public_url"),
            resend_signing_secret: get("resend_webhook_signing_secret"),
            resend_api_key: get("resend_api_key"),
            resend_from_email: get("resend_from_email"),
            ingress_id: route.lease().ingress_id,
        })
    }

    pub(in crate::server) fn load_for_workspace(
        store: &crate::workspace::RegistryStore,
        workspace_id: crate::workspace::WorkspaceId,
        ingress_id: crate::server::IngressId,
    ) -> anyhow::Result<Self> {
        let registry = crate::workspace::RegistryStore::load_from(store.path())
            .context("loading routed workspace provider configuration")?;
        let record = registry
            .workspaces
            .values()
            .find(|record| record.workspace_id == workspace_id)
            .context("routed receiver workspace no longer exists")?;
        let get = |name: &str| {
            record
                .env
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Self {
            workspace_id,
            twilio_auth_token: get("twilio_auth_token"),
            public_base_url: get("brain_receiver_public_url"),
            resend_signing_secret: get("resend_webhook_signing_secret"),
            resend_api_key: get("resend_api_key"),
            resend_from_email: get("resend_from_email"),
            ingress_id,
        })
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

    pub(super) const fn unavailable(self) -> bool {
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

pub(super) fn authenticate(
    request: &mut crate::server::http::Request,
    config: &ProviderConfig,
    channel: Channel,
) -> Result<AuthenticatedInbound, ProviderError> {
    let body = request
        .read_body(WEBHOOK_BODY_LIMIT)
        .map_err(|error| match error {
            crate::server::http::BodyError::TooLarge => ProviderError::BodyTooLarge,
            crate::server::http::BodyError::Io(_) | crate::server::http::BodyError::Malformed => {
                ProviderError::MalformedBody
            }
        })?;
    let pending_email = match channel {
        Channel::Sms => {
            let inbound = sms::authenticate(request, &body, config)?;
            request
                .begin_handler_phase()
                .map_err(|_| ProviderError::Deadline)?;
            return Ok(inbound);
        }
        Channel::Email => email::verify(request, &body, config)?,
    };
    if crate::server::receiver::dispatch::provider_delivery_completed(
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

pub(in crate::server) fn verify_unavailable_email(
    request: &mut crate::server::http::Request,
    config: &ProviderConfig,
) -> Result<String, ProviderError> {
    let body = request
        .read_body(WEBHOOK_BODY_LIMIT)
        .map_err(|error| match error {
            crate::server::http::BodyError::TooLarge => ProviderError::BodyTooLarge,
            crate::server::http::BodyError::Io(_) | crate::server::http::BodyError::Malformed => {
                ProviderError::MalformedBody
            }
        })?;
    email::verify(request, &body, config).map(|verified| verified.webhook_id().to_owned())
}
