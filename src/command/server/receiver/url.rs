//! `brain receiver url`: the exact webhook URLs a provider portal needs.
//!
//! Purely informational, so it reads the selected workspace's public base URL
//! and its portable ingress UUID and prints; it never consults receiver intent
//! or a running server. You configure a provider portal *before* ingress is
//! live, so requiring either would make the command useless exactly when it is
//! needed.

use anyhow::Result;

use crate::server::IngressId;
use crate::server::receiver::Channel;
use crate::theme::Theme;
use crate::workspace::CommandContext;

/// The brain-env value that supplies the public origin of the webhook URLs.
const PUBLIC_URL_VAR: &str = "brain_receiver_public_url";

/// Every channel a provider portal can be pointed at.
pub(super) const ALL_CHANNELS: [Channel; 2] = [Channel::Sms, Channel::Email];

/// Which channels a request names; no flag means every channel. Pure.
#[must_use]
pub(crate) fn selected_channels(sms: bool, email: bool) -> Vec<Channel> {
    if sms == email {
        return ALL_CHANNELS.to_vec();
    }
    if sms {
        vec![Channel::Sms]
    } else {
        vec![Channel::Email]
    }
}

/// The provider whose portal owns a channel's webhook.
const fn provider_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Sms => "Twilio (SMS)",
        Channel::Email => "Resend (email)",
    }
}

/// The webhook rows for one workspace, one line per channel. Pure.
#[must_use]
pub(crate) fn webhook_rows(
    public_base_url: &str,
    ingress: IngressId,
    channels: &[Channel],
    theme: Theme,
) -> String {
    let width = channels
        .iter()
        .map(|channel| provider_label(*channel).len())
        .max()
        .unwrap_or_default();
    channels
        .iter()
        .map(|channel| {
            // Pad before styling: color escapes have no display width, so a
            // format-width applied to a styled string misaligns the column.
            let label = format!("{:width$}", provider_label(*channel));
            format!(
                "  {}  {}",
                theme.muted(&label),
                theme.value(&crate::server::receiver::http::receiver_webhook_url(
                    public_base_url,
                    ingress,
                    *channel,
                )),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The full `brain receiver url` block: a heading, the rows, and the one
/// non-obvious rule about pasting them. Pure.
#[must_use]
pub(crate) fn webhook_block(
    workspace: &str,
    public_base_url: &str,
    ingress: IngressId,
    channels: &[Channel],
    theme: Theme,
) -> String {
    format!(
        "{}\n{}\n  {}",
        theme.heading(&format!("Receiver webhook URLs  {workspace}")),
        webhook_rows(public_base_url, ingress, channels, theme),
        theme.muted(
            "Paste exactly: providers sign the literal URL, so a trailing slash or a different host breaks verification."
        ),
    )
}

/// What to say when this machine has no public base URL for the workspace.
///
/// Names the variable and both ways to set it, since the value is machine-local
/// and a peer machine having it does not help here. Pure.
#[must_use]
pub(crate) fn missing_public_url(workspace: &str) -> String {
    format!(
        "{PUBLIC_URL_VAR} is unset for workspace {workspace}, so its webhook URLs have no origin yet.\n  \
         fix: brain receiver setup -w {workspace}\n  \
         or:  brain env set -w {workspace} {PUBLIC_URL_VAR}=https://<public-host>"
    )
}

/// This machine's public base URL for a workspace, if set.
pub(super) fn public_base_url(context: &CommandContext) -> Option<String> {
    crate::env::get(context, PUBLIC_URL_VAR).and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

/// `brain receiver url [--sms] [--email]`.
pub(super) fn run(args: &crate::cli::ReceiverUrlArgs, context: &CommandContext) -> Result<()> {
    let workspace = context.workspace.name().as_str().to_owned();
    let Some(public) = public_base_url(context) else {
        anyhow::bail!(missing_public_url(&workspace));
    };
    println!(
        "{}",
        webhook_block(
            &workspace,
            &public,
            crate::server::workspace_ingress(&context.workspace)?,
            &selected_channels(args.sms, args.email),
            Theme::active(),
        )
    );
    Ok(())
}

/// The webhook rows appended to `brain receiver status`, or a muted pointer at
/// setup when this machine has no public URL for the workspace.
pub(super) fn status_block(context: &CommandContext) -> String {
    let theme = Theme::active();
    let workspace = context.workspace.name().as_str().to_owned();
    let Some(public) = public_base_url(context) else {
        return theme.muted(&format!(
            "Webhook URLs  unset ({PUBLIC_URL_VAR} is not set on this machine; run `brain receiver setup -w {workspace}`)"
        ));
    };
    let Ok(ingress) = crate::server::workspace_ingress(&context.workspace) else {
        return theme.muted("Webhook URLs  unavailable (workspace manifest is unreadable)");
    };
    format!(
        "{}\n{}",
        theme.muted("Webhook URLs"),
        webhook_rows(&public, ingress, &ALL_CHANNELS, theme),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ingress() -> IngressId {
        IngressId::parse("8f670650-0c97-4cf2-aade-1b5bb51aa1b3").expect("ingress id")
    }

    #[test]
    fn no_channel_flag_prints_every_channel() {
        assert_eq!(
            selected_channels(false, false),
            [Channel::Sms, Channel::Email]
        );
    }

    #[test]
    fn one_channel_flag_narrows_to_that_channel() {
        assert_eq!(selected_channels(true, false), [Channel::Sms]);
        assert_eq!(selected_channels(false, true), [Channel::Email]);
    }

    #[test]
    fn asking_for_both_channels_is_the_same_as_asking_for_neither() {
        // `--sms --email` is a redundant way to say "all", not a conflict.
        assert_eq!(
            selected_channels(true, true),
            [Channel::Sms, Channel::Email]
        );
    }

    #[test]
    fn each_row_pairs_a_provider_with_its_exact_ingress_scoped_url() {
        let rows = webhook_rows(
            "https://brain.example.test",
            ingress(),
            &[Channel::Sms, Channel::Email],
            Theme::dark(false),
        );

        assert!(
            rows.contains("https://brain.example.test/w/8f670650-0c97-4cf2-aade-1b5bb51aa1b3/sms"),
            "{rows}"
        );
        assert!(
            rows.contains(
                "https://brain.example.test/w/8f670650-0c97-4cf2-aade-1b5bb51aa1b3/email"
            ),
            "{rows}"
        );
        assert!(rows.contains("Twilio (SMS)"), "{rows}");
        assert!(rows.contains("Resend (email)"), "{rows}");
        assert_eq!(rows.lines().count(), 2, "{rows}");
    }

    #[test]
    fn a_trailing_slash_on_the_public_url_never_doubles_in_the_webhook() {
        // Providers sign the literal URL, so `//w/` would be a different string.
        let rows = webhook_rows(
            "https://brain.example.test/",
            ingress(),
            &[Channel::Sms],
            Theme::dark(false),
        );

        assert!(rows.contains("https://brain.example.test/w/"), "{rows}");
        assert!(!rows.contains("//w/"), "{rows}");
    }

    #[test]
    fn the_block_names_the_workspace_and_the_paste_exactly_rule() {
        let block = webhook_block(
            "family",
            "https://brain.example.test",
            ingress(),
            &[Channel::Sms],
            Theme::dark(false),
        );

        assert!(
            block.starts_with("Receiver webhook URLs  family"),
            "{block}"
        );
        assert!(block.contains("Paste exactly"), "{block}");
    }

    #[test]
    fn a_missing_public_url_names_the_variable_and_both_ways_to_set_it() {
        let message = missing_public_url("family");

        assert!(
            message.contains("brain_receiver_public_url is unset"),
            "{message}"
        );
        assert!(
            message.contains("brain receiver setup -w family"),
            "{message}"
        );
        assert!(
            message.contains("brain env set -w family brain_receiver_public_url="),
            "{message}"
        );
    }
}
