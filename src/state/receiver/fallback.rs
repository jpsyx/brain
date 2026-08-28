use super::ReceiverProviderCapability;

const SAFE_NOTICE: &str =
    "I couldn’t deliver the full response on the original channel. Please try again there.";

/// One alternate destination whose authority was frozen with the inbound job.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverFallbackDestination {
    provider: ReceiverProviderCapability,
    recipient: String,
}

impl ReceiverFallbackDestination {
    #[must_use]
    pub fn new(provider: ReceiverProviderCapability, recipient: impl Into<String>) -> Self {
        Self {
            provider,
            recipient: recipient.into(),
        }
    }

    #[must_use]
    pub const fn provider(&self) -> ReceiverProviderCapability {
        self.provider
    }

    #[must_use]
    pub fn recipient(&self) -> &str {
        &self.recipient
    }
}

impl std::fmt::Debug for ReceiverFallbackDestination {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverFallbackDestination(<redacted>)")
    }
}

/// At most one conservative notice on a frozen alternate channel.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverFallbackPlan {
    destination: ReceiverFallbackDestination,
}

impl ReceiverFallbackPlan {
    #[must_use]
    pub const fn destination(&self) -> &ReceiverFallbackDestination {
        &self.destination
    }

    #[must_use]
    pub const fn notice(&self) -> &'static str {
        SAFE_NOTICE
    }
}

impl std::fmt::Debug for ReceiverFallbackPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverFallbackPlan(<redacted>)")
    }
}

/// Select only acceptance-frozen authority, never mutable users or configuration.
#[must_use]
pub fn plan_receiver_fallback(
    failed_provider: ReceiverProviderCapability,
    attempted_recipients: &[&str],
    frozen_alternates: &[ReceiverFallbackDestination],
) -> Option<ReceiverFallbackPlan> {
    frozen_alternates
        .iter()
        .find(|candidate| {
            candidate.provider != failed_provider
                && !attempted_recipients.contains(&candidate.recipient.as_str())
        })
        .cloned()
        .map(|destination| ReceiverFallbackPlan { destination })
}
