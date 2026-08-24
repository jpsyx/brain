//! Provider delivery decisions and narrow recipient authorization.

type DeliveryJob = Box<dyn FnOnce() + Send>;

static DELIVERY_DISPATCHER: std::sync::LazyLock<
    Result<std::sync::mpsc::SyncSender<DeliveryJob>, String>,
> = std::sync::LazyLock::new(|| {
    let (sender, receiver) = std::sync::mpsc::sync_channel::<DeliveryJob>(64);
    std::thread::Builder::new()
        .name("brain-delivery".to_owned())
        .spawn(move || {
            while let Ok(job) = receiver.recv() {
                job();
            }
        })
        .map(|_| sender)
        .map_err(|error| error.to_string())
});

pub fn log_outcome(action: &str, result: anyhow::Result<()>) {
    match result {
        Ok(()) => crate::logging::log(format!("receiver delivery succeeded action={action}")),
        Err(error) => crate::logging::log(format!(
            "receiver delivery failed action={action} error={error:#}"
        )),
    }
}

fn dispatch_background(
    action: &'static str,
    operation: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    let sender = DELIVERY_DISPATCHER
        .as_ref()
        .map_err(|error| anyhow::anyhow!("starting receiver delivery worker: {error}"))?;
    sender
        .try_send(Box::new(move || log_outcome(action, operation())))
        .map_err(|error| anyhow::anyhow!("queuing provider delivery: {error}"))?;
    crate::logging::log(format!("receiver delivery queued action={action}"));
    Ok(())
}

#[cfg(test)]
pub(crate) fn wait_for_background_delivery() {
    let (sent, received) = std::sync::mpsc::sync_channel(0);
    dispatch_background("test delivery barrier", move || {
        sent.send(())
            .map_err(|error| anyhow::anyhow!("signaling delivery barrier: {error}"))
    })
    .expect("queue delivery barrier");
    received
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("background delivery completed before test deadline");
}

pub fn send_sms_background(
    command: crate::workspace::CommandContext,
    action: &'static str,
    to: String,
    body: String,
) {
    if let Err(error) = dispatch_background(action, move || send_sms(&command, &to, &body)) {
        crate::logging::log(format!(
            "receiver delivery could not start action={action} error={error:#}"
        ));
    }
}

pub fn send_email_background(
    command: crate::workspace::CommandContext,
    action: &'static str,
    to: Vec<String>,
    subject: String,
    text: String,
    html: String,
    reply: Option<crate::server::receiver::EmailReplyContext>,
) {
    if let Err(error) = dispatch_background(action, move || {
        send_email(&command, &to, &subject, &text, &html, reply.as_ref())
    }) {
        crate::logging::log(format!(
            "receiver delivery could not start action={action} error={error:#}"
        ));
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

/// Send a final SMS through Twilio. The credentials are read only when a
/// remote job completes, never from the portable brain config.
pub fn send_sms(
    command: &crate::workspace::CommandContext,
    to: &str,
    body: &str,
) -> anyhow::Result<()> {
    let account = super::provider::get(command, "twilio_account_sid")
        .ok_or_else(|| anyhow::anyhow!("TWILIO_ACCOUNT_SID is not configured"))?;
    let token = super::provider::get(command, "twilio_auth_token")
        .ok_or_else(|| anyhow::anyhow!("TWILIO_AUTH_TOKEN is not configured"))?;
    let from = super::provider::get(command, "twilio_from_number")
        .ok_or_else(|| anyhow::anyhow!("TWILIO_FROM_NUMBER is not configured"))?;
    let endpoint = format!("https://api.twilio.com/2010-04-01/Accounts/{account}/Messages.json");
    let output = super::provider::CurlRequest::new()
        .flag("silent")
        .flag("show-error")
        .flag("fail")
        .option("connect-timeout", "10")
        .option("max-time", "30")
        .option("user", &format!("{account}:{token}"))
        .option("request", "POST")
        .option("url", &endpoint)
        .option("data-urlencode", &format!("To={to}"))
        .option("data-urlencode", &format!("From={from}"))
        .option("data-urlencode", &format!("Body={body}"))
        .output()?;
    anyhow::ensure!(output.status.success(), "Twilio rejected the outbound SMS");
    Ok(())
}

/// Send a threaded email through Resend. The caller has already applied the
/// participant/allowlist intersection before invoking this function.
pub fn send_email(
    command: &crate::workspace::CommandContext,
    to: &[String],
    subject: &str,
    text: &str,
    html: &str,
    reply: Option<&crate::server::receiver::EmailReplyContext>,
) -> anyhow::Result<()> {
    let key = super::provider::get(command, "resend_sending_api_key")
        .ok_or_else(|| anyhow::anyhow!("resend_sending_api_key is not configured"))?;
    let from = super::provider::get(command, "resend_from_email")
        .ok_or_else(|| anyhow::anyhow!("RESEND_FROM_EMAIL is not configured"))?;
    let payload = email_payload(
        &from,
        to,
        subject,
        text,
        html,
        reply.and_then(|context| context.message_id.as_deref()),
    );
    send_email_payload(&key, &payload)
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

fn send_email_payload(key: &str, payload: &serde_json::Value) -> anyhow::Result<()> {
    let output = super::provider::CurlRequest::new()
        .flag("silent")
        .flag("show-error")
        .flag("fail")
        .option("connect-timeout", "10")
        .option("max-time", "30")
        .option("request", "POST")
        .option("url", "https://api.resend.com/emails")
        .option("header", &format!("Authorization: Bearer {key}"))
        .option("header", "Content-Type: application/json")
        .option("data", &payload.to_string())
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Resend rejected the outbound email"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
