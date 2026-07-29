//! Provider delivery decisions and narrow recipient authorization.

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
pub fn send_sms(to: &str, body: &str) -> anyhow::Result<()> {
    let account = std::env::var("TWILIO_ACCOUNT_SID")?;
    let token = std::env::var("TWILIO_AUTH_TOKEN")?;
    let from = std::env::var("TWILIO_FROM_NUMBER")?;
    let endpoint = format!("https://api.twilio.com/2010-04-01/Accounts/{account}/Messages.json");
    let status = std::process::Command::new("curl")
        .args([
            "-fsS",
            "-u",
            &format!("{account}:{token}"),
            "-X",
            "POST",
            &endpoint,
            "--data-urlencode",
            &format!("To={to}"),
            "--data-urlencode",
            &format!("From={from}"),
            "--data-urlencode",
            &format!("Body={body}"),
        ])
        .status()?;
    anyhow::ensure!(status.success(), "Twilio rejected the outbound SMS");
    Ok(())
}

/// Send a threaded email through Resend. The caller has already applied the
/// participant/allowlist intersection before invoking this function.
pub fn send_email(to: &[String], subject: &str, text: &str, html: &str) -> anyhow::Result<()> {
    let key = std::env::var("RESEND_API_KEY")?;
    let from = std::env::var("RESEND_FROM_EMAIL")?;
    let payload = serde_json::json!({
        "from": from,
        "to": to,
        "subject": subject,
        "text": text,
        "html": html,
    });
    let status = std::process::Command::new("curl")
        .args([
            "-fsS",
            "-X",
            "POST",
            "https://api.resend.com/emails",
            "-H",
            &format!("Authorization: Bearer {key}"),
            "-H",
            "Content-Type: application/json",
            "--data",
            &payload.to_string(),
        ])
        .status()?;
    anyhow::ensure!(status.success(), "Resend rejected the outbound email");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
