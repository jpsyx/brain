//! Provider delivery decisions and narrow recipient authorization.

mod executor;
mod provider_attempt;

#[cfg(not(test))]
use executor::DeliveryExecutorPermit;
pub(crate) use executor::{BoundedDeliveryExecutor, DeliveryExecutorPoll};
#[cfg(not(test))]
use provider_attempt::deliver_receiver_claim;
#[cfg(test)]
use provider_attempt::{
    PROVIDER_RESPONSE_LIMIT, classify_provider_http_response, classify_provider_process_output,
    resend_request_for_test,
};
pub(crate) use provider_attempt::{
    ReceiverProviderProcessFailure, classify_provider_process_failure,
};

/// Publication handle for work already admitted by the bounded provider executor.
pub(crate) trait ReceiverDeliveryStart: Send {
    fn attempt_kind(&self) -> ReceiverDeliveryAttemptKind;

    fn start(self: Box<Self>) -> anyhow::Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverDeliveryAttemptKind {
    ProviderIo,
    NoProviderIo,
}

/// Nonblocking provider result returned to the application event loop.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReceiverDeliveryExecutionPoll {
    Pending,
    Ready {
        claim: Box<crate::state::ReceiverDeliveryClaim>,
        result: crate::state::ReceiverProviderResultClass,
        attempt_kind: ReceiverDeliveryAttemptKind,
    },
    Disconnected,
}

/// Injectable provider execution boundary used by the semantic-response coordinator.
pub(crate) trait ReceiverDeliveryExecution: Send {
    fn reserve(
        &mut self,
        command: crate::workspace::CommandContext,
        claim: crate::state::ReceiverDeliveryClaim,
    ) -> Result<Box<dyn ReceiverDeliveryStart>, Box<crate::state::ReceiverDeliveryClaim>>;

    fn poll(&self) -> ReceiverDeliveryExecutionPoll;

    fn cancel(&mut self);
}

#[cfg(not(test))]
pub(crate) struct SystemReceiverDeliveryExecution {
    executor: BoundedDeliveryExecutor<
        crate::state::ReceiverDeliveryClaim,
        crate::state::ReceiverProviderResultClass,
    >,
    cancellation: super::provider::CurlCancellation,
}

#[cfg(not(test))]
impl SystemReceiverDeliveryExecution {
    pub(crate) fn new() -> std::io::Result<Self> {
        Ok(Self {
            executor: BoundedDeliveryExecutor::new(1, "brain-receiver-delivery")?,
            cancellation: super::provider::CurlCancellation::default(),
        })
    }
}

#[cfg(not(test))]
struct SystemReceiverDeliveryStart(DeliveryExecutorPermit);

#[cfg(not(test))]
impl ReceiverDeliveryStart for SystemReceiverDeliveryStart {
    fn attempt_kind(&self) -> ReceiverDeliveryAttemptKind {
        ReceiverDeliveryAttemptKind::ProviderIo
    }

    fn start(self: Box<Self>) -> anyhow::Result<()> {
        self.0
            .start()
            .map_err(|_| anyhow::anyhow!("provider delivery worker disconnected before start"))
    }
}

#[cfg(not(test))]
impl ReceiverDeliveryExecution for SystemReceiverDeliveryExecution {
    fn reserve(
        &mut self,
        command: crate::workspace::CommandContext,
        claim: crate::state::ReceiverDeliveryClaim,
    ) -> Result<Box<dyn ReceiverDeliveryStart>, Box<crate::state::ReceiverDeliveryClaim>> {
        let operation_claim = claim.clone();
        let cancellation = self.cancellation.clone();
        self.executor
            .reserve(claim, move || {
                deliver_receiver_claim(&command, &operation_claim, &cancellation)
            })
            .map(|permit| Box::new(SystemReceiverDeliveryStart(permit)) as Box<_>)
            .map_err(|full| Box::new(full.into_input()))
    }

    fn poll(&self) -> ReceiverDeliveryExecutionPoll {
        match self.executor.poll() {
            DeliveryExecutorPoll::Pending => ReceiverDeliveryExecutionPoll::Pending,
            DeliveryExecutorPoll::Ready(result) => ReceiverDeliveryExecutionPoll::Ready {
                claim: Box::new(result.input),
                result: result.output,
                attempt_kind: ReceiverDeliveryAttemptKind::ProviderIo,
            },
            DeliveryExecutorPoll::Disconnected => ReceiverDeliveryExecutionPoll::Disconnected,
        }
    }

    fn cancel(&mut self) {
        self.cancellation.cancel();
    }
}

/// Keep only addresses that appear in the inbound thread and are explicitly
/// allowlisted. The receiving address itself is never echoed back.
///
/// Every side is reduced to a bare address first. The receiving address comes
/// from free-form env (`resend_from_email`), so a perfectly valid
/// `Brain <brain@example.com>` there must not defeat the self-echo guard and
/// let brain answer its own mail. A value that is not an address at all — an
/// SMS sender, a malformed header — matches nothing and is dropped.
#[must_use]
pub fn allowed_thread_recipients(
    participants: &[String],
    allowed: &[String],
    receiving_address: &str,
) -> Vec<String> {
    let receiving = crate::users::normalize_mailbox(receiving_address).unwrap_or_default();
    let allowed = allowed
        .iter()
        .filter_map(|item| crate::users::normalize_mailbox(item).ok())
        .collect::<Vec<_>>();
    participants
        .iter()
        .filter_map(|participant| crate::users::normalize_mailbox(participant).ok())
        .filter(|participant| *participant != receiving)
        .filter(|participant| allowed.contains(participant))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Restrict a reply to enabled email identities owned by the initiating actor.
#[must_use]
pub fn actor_thread_recipients(
    participants: &[String],
    users: &crate::users::Users,
    actor: &crate::actor::ActorContext,
    receiving_address: &str,
) -> Vec<String> {
    let allowed = users
        .user(actor.user_id())
        .map(|user| {
            user.emails
                .iter()
                .filter(|email| email.inbound_allowed)
                .map(|email| email.value.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    allowed_thread_recipients(participants, &allowed, receiving_address)
}

/// Use only acceptance-time trusted email destinations for a remote turn.
#[must_use]
pub fn trusted_response_recipients(
    response_email: Option<&str>,
    allowed_thread_participants: &[String],
) -> Vec<String> {
    response_email
        .into_iter()
        .chain(allowed_thread_participants.iter().map(String::as_str))
        .filter_map(|address| crate::users::normalize_mailbox(address).ok())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[must_use]
pub fn reply_subject(reply: Option<&crate::server::receiver::EmailReplyContext>) -> String {
    let Some(subject) = reply
        .map(|context| context.subject.trim())
        .filter(|value| !value.is_empty())
    else {
        return "Brain response".to_owned();
    };
    if subject
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
    {
        subject.to_owned()
    } else {
        format!("Re: {subject}")
    }
}

#[must_use]
pub fn email_payload(
    from: &str,
    to: &[String],
    subject: &str,
    text: &str,
    html: &str,
    in_reply_to: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "from": from,
        "to": to,
        "subject": subject,
        "text": text,
        "html": html,
    });
    if let Some(message_id) = in_reply_to {
        payload["headers"] = serde_json::json!({
            "In-Reply-To": message_id,
            "References": message_id,
        });
    }
    payload
}

#[cfg(test)]
mod tests;
