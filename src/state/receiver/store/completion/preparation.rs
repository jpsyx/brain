use anyhow::{Context as _, Result};

use crate::state::{
    ReceiverDeliveryErrorCategory, ReceiverDeliveryRenderError, ReceiverResponseKind,
    render_receiver_delivery,
};

const AUTHORIZATION_TERMINAL_JSON: &str = r#"{"terminal":"authorization"}"#;
const INVALID_REQUEST_TERMINAL_JSON: &str = r#"{"terminal":"invalid-request"}"#;

pub(super) enum PreparedDelivery {
    Ready {
        envelope_json: String,
    },
    Terminal {
        envelope_json: &'static str,
        category: ReceiverDeliveryErrorCategory,
    },
}

impl PreparedDelivery {
    pub(super) fn envelope_json(&self) -> &str {
        match self {
            Self::Ready { envelope_json } => envelope_json,
            Self::Terminal { envelope_json, .. } => envelope_json,
        }
    }

    pub(super) const fn state(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "ready",
            Self::Terminal { .. } => "failed",
        }
    }

    pub(super) const fn error_category(&self) -> Option<&'static str> {
        match self {
            Self::Ready { .. } => None,
            Self::Terminal { category, .. } => Some(category.as_str()),
        }
    }

    pub(super) const fn job_state(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "answer-ready",
            Self::Terminal { .. } => "failed",
        }
    }

    pub(super) const fn job_error(&self) -> Option<&'static str> {
        match self {
            Self::Ready { .. } => None,
            Self::Terminal { category, .. } => Some(category.as_str()),
        }
    }
}

pub(super) fn prepare(
    inbound: &crate::server::receiver::InboundJob,
    answer: &str,
) -> Result<PreparedDelivery> {
    match render_receiver_delivery(
        inbound,
        ReceiverResponseKind::FinalAnswer,
        &inbound.response_sender,
        answer,
    ) {
        Ok(envelope) => Ok(PreparedDelivery::Ready {
            envelope_json: serde_json::to_string(&envelope)
                .context("serialize durable receiver delivery envelope")?,
        }),
        Err(ReceiverDeliveryRenderError::NoTrustedEmailRecipients) => {
            Ok(PreparedDelivery::Terminal {
                envelope_json: AUTHORIZATION_TERMINAL_JSON,
                category: ReceiverDeliveryErrorCategory::Authorization,
            })
        }
        Err(ReceiverDeliveryRenderError::InvalidOutboundSender)
            if inbound.response_sender.is_empty() =>
        {
            Ok(PreparedDelivery::Terminal {
                envelope_json: INVALID_REQUEST_TERMINAL_JSON,
                category: ReceiverDeliveryErrorCategory::InvalidRequest,
            })
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn terminal_record_is_valid(
    state: &str,
    category: Option<&str>,
    envelope_json: &str,
) -> bool {
    matches!(
        (state, category, envelope_json),
        ("failed", Some("authorization"), AUTHORIZATION_TERMINAL_JSON)
            | (
                "failed",
                Some("invalid-request"),
                INVALID_REQUEST_TERMINAL_JSON
            )
    )
}
