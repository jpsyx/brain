//! Provider authentication after an ingress has selected one live workspace.

mod email;
mod sms;

use anyhow::{Context as _, Result};

use super::{AttachmentRef, Channel};

pub(super) const WEBHOOK_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) struct ProviderConfig {
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
    ) -> Result<Self> {
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
            twilio_auth_token: get("twilio_auth_token"),
            public_base_url: get("brain_receiver_public_url"),
            resend_signing_secret: get("resend_webhook_signing_secret"),
            resend_api_key: get("resend_api_key"),
            resend_from_email: get("resend_from_email"),
            ingress_id: route.lease().ingress_id,
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct AuthenticatedInbound {
    pub channel: Channel,
    pub sender: String,
    pub prompt: String,
    pub participants: Vec<String>,
    pub attachments: Vec<AttachmentRef>,
    pub receiving_address: String,
    pub provider_id: Option<String>,
}

pub(super) fn authenticate(
    request: &mut crate::server::http::Request,
    config: &ProviderConfig,
    channel: Channel,
) -> Result<AuthenticatedInbound> {
    let body = request
        .read_body(WEBHOOK_BODY_LIMIT)
        .map_err(|error| match error {
            crate::server::http::BodyError::TooLarge => {
                anyhow::anyhow!("webhook body is too large")
            }
            crate::server::http::BodyError::Io(error) => {
                anyhow::anyhow!("could not read webhook body: {error}")
            }
            crate::server::http::BodyError::Malformed => {
                anyhow::anyhow!("webhook body is malformed")
            }
        })?;
    match channel {
        Channel::Sms => sms::authenticate(request, &body, config),
        Channel::Email => email::authenticate(request, &body, config),
    }
}
