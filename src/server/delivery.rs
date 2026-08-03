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
) {
    if let Err(error) = dispatch_background(action, move || {
        send_email(&command, &to, &subject, &text, &html)
    }) {
        crate::logging::log(format!(
            "receiver delivery could not start action={action} error={error:#}"
        ));
    }
}

/// Keep only addresses that appear in the inbound thread and are explicitly
/// allowlisted. The receiving address itself is never echoed back.
#[must_use]
pub fn allowed_thread_recipients(
    participants: &[String],
    allowed: &[String],
    receiving_address: &str,
) -> Vec<String> {
    let receiving = receiving_address.trim().to_ascii_lowercase();
    participants
        .iter()
        .map(|participant| participant.trim().to_ascii_lowercase())
        .filter(|participant| !participant.is_empty() && *participant != receiving)
        .filter(|participant| {
            allowed
                .iter()
                .any(|item| item.trim().eq_ignore_ascii_case(participant))
        })
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
) -> anyhow::Result<()> {
    let key = super::provider::get(command, "resend_api_key")
        .ok_or_else(|| anyhow::anyhow!("RESEND_API_KEY is not configured"))?;
    let from = super::provider::get(command, "resend_from_email")
        .ok_or_else(|| anyhow::anyhow!("RESEND_FROM_EMAIL is not configured"))?;
    let payload = serde_json::json!({
        "from": from,
        "to": to,
        "subject": subject,
        "text": text,
        "html": html,
    });
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
mod tests {
    use super::*;

    #[test]
    fn provider_delivery_runs_off_the_tui_thread() {
        let started = std::time::Instant::now();
        dispatch_background("test delivery", || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            Ok(())
        })
        .unwrap();

        assert!(
            started.elapsed() < std::time::Duration::from_millis(250),
            "dispatch waited for the provider request"
        );
    }

    #[test]
    fn thread_delivery_intersects_participants_and_allowlist() {
        let recipients = allowed_thread_recipients(
            &[
                "Me@Example.com".to_owned(),
                "other@example.com".to_owned(),
                "new@example.com".to_owned(),
            ],
            &["me@example.com".to_owned(), "other@example.com".to_owned()],
            "me@example.com",
        );
        assert_eq!(recipients, vec!["other@example.com"]);
    }
}
