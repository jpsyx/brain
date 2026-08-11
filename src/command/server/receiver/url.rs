//! `brain receiver url`: the exact webhook URLs a provider portal needs.
//!
//! There is one URL per channel for the whole machine. Nothing in it identifies
//! a workspace: brain selects the workspace from the phone number or email
//! address a message arrived at, so every workspace's Twilio and Resend portals
//! are pointed at the same two URLs.
//!
//! Purely informational, so it reads this machine's public base URL and prints;
//! it never consults receiver intent or a running server. You configure a
//! provider portal *before* ingress is live, so requiring either would make the
//! command useless exactly when it is needed.

use anyhow::Result;

use crate::server::receiver::Channel;
use crate::theme::Theme;
use crate::workspace::CommandContext;

/// The brain-env value that supplies the public origin of the webhook URLs.
///
/// Machine-global: one machine serves one origin, and the URL a provider signs
/// must be identical for every workspace registered here.
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

/// How brain picks the workspace now that no URL names one.
pub(super) const ROUTING_RULE: &str = "One URL per channel for this whole machine: brain routes each message by the number or address it arrived at, so every workspace's portal gets the same URL.";

/// The one non-obvious rule about pasting a webhook URL into a portal.
const PASTE_RULE: &str = "Paste exactly: providers sign the literal URL, so a trailing slash or a different host breaks verification.";

/// The label column width the provider rows need. Pure.
#[must_use]
pub(super) fn label_width(channels: &[Channel]) -> usize {
    channels
        .iter()
        .map(|channel| provider_label(*channel).len())
        .max()
        .unwrap_or_default()
}

/// The webhook rows for this machine, one line per channel. Pure.
#[must_use]
pub(crate) fn webhook_rows(public_base_url: &str, channels: &[Channel], theme: Theme) -> String {
    webhook_rows_at(public_base_url, channels, label_width(channels), theme)
}

/// The webhook rows in a label column of a caller's chosen width, so a listing
/// can align them with rows of its own. Pure.
#[must_use]
pub(super) fn webhook_rows_at(
    public_base_url: &str,
    channels: &[Channel],
    width: usize,
    theme: Theme,
) -> String {
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
                    *channel,
                )),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The full `brain receiver url` block: a heading, the rows, how routing picks a
/// workspace, and the one non-obvious rule about pasting them. Pure.
#[must_use]
pub(crate) fn webhook_block(public_base_url: &str, channels: &[Channel], theme: Theme) -> String {
    format!(
        "{}\n{}\n  {}\n  {}",
        theme.heading("Receiver webhook URLs"),
        webhook_rows(public_base_url, channels, theme),
        theme.muted(ROUTING_RULE),
        theme.muted(PASTE_RULE),
    )
}

/// What to say when this machine has no public base URL.
///
/// Names the variable and both ways to set it, since the value is machine-local
/// and a peer machine having it does not help here. The direct env write comes
/// first because it is the exact fix for the exact missing value and needs no
/// selector; guided setup carries one, because it *also* collects that
/// workspace's provider credentials and would otherwise configure whichever
/// workspace happens to be the default. Pure.
#[must_use]
pub(crate) fn missing_public_url(workspace: &str) -> String {
    format!(
        "{PUBLIC_URL_VAR} is unset on this machine, so its webhook URLs have no origin yet.\n  \
         fix: brain env set {PUBLIC_URL_VAR}=https://<public-host>\n  \
         or:  brain receiver setup -w {workspace} (guided; also collects {workspace}'s provider credentials)"
    )
}

/// This machine's public base URL, if set.
pub(super) fn public_base_url(context: &CommandContext) -> Option<String> {
    crate::env::get(context, PUBLIC_URL_VAR).and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

/// `brain receiver url [--sms] [--email]`.
pub(super) fn run(args: &crate::cli::ReceiverUrlArgs, context: &CommandContext) -> Result<()> {
    let Some(public) = public_base_url(context) else {
        anyhow::bail!(missing_public_url(context.workspace.name().as_str()));
    };
    println!(
        "{}",
        webhook_block(
            &public,
            &selected_channels(args.sms, args.email),
            Theme::active(),
        )
    );
    Ok(())
}

/// The webhook rows appended to `brain receiver status`, or a muted pointer at
/// setup when this machine has no public URL.
pub(super) fn status_block(context: &CommandContext) -> String {
    let theme = Theme::active();
    let Some(public) = public_base_url(context) else {
        return theme.muted(&format!(
            "Webhook URLs  unset ({PUBLIC_URL_VAR} is not set on this machine; set it with `brain env set {PUBLIC_URL_VAR}=https://<public-host>`)"
        ));
    };
    format!(
        "{}\n{}",
        theme.muted("Webhook URLs"),
        webhook_rows(&public, &ALL_CHANNELS, theme),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn each_row_pairs_a_provider_with_its_one_machine_wide_url() {
        let rows = webhook_rows(
            "https://brain.example.test",
            &[Channel::Sms, Channel::Email],
            Theme::dark(false),
        );

        assert!(rows.contains("https://brain.example.test/sms"), "{rows}");
        assert!(rows.contains("https://brain.example.test/email"), "{rows}");
        // Nothing in a webhook URL identifies a workspace any more.
        assert!(!rows.contains("/w/"), "{rows}");
        assert!(rows.contains("Twilio (SMS)"), "{rows}");
        assert!(rows.contains("Resend (email)"), "{rows}");
        assert_eq!(rows.lines().count(), 2, "{rows}");
    }

    #[test]
    fn a_trailing_slash_on_the_public_url_never_doubles_in_the_webhook() {
        // Providers sign the literal URL, so `//sms` would be a different string.
        let rows = webhook_rows(
            "https://brain.example.test/",
            &[Channel::Sms],
            Theme::dark(false),
        );

        assert!(rows.contains("https://brain.example.test/sms"), "{rows}");
        assert!(!rows.contains("//sms"), "{rows}");
    }

    #[test]
    fn the_block_explains_both_the_routing_rule_and_the_paste_rule() {
        let block = webhook_block(
            "https://brain.example.test",
            &[Channel::Sms],
            Theme::dark(false),
        );

        assert!(block.starts_with("Receiver webhook URLs"), "{block}");
        // A user pasting one URL into two portals needs to know why that works.
        assert!(
            block.contains("routes each message by the number"),
            "{block}"
        );
        assert!(block.contains("Paste exactly"), "{block}");
    }

    #[test]
    fn a_missing_public_url_names_the_variable_and_both_ways_to_set_it() {
        let message = missing_public_url("family");

        assert!(
            message.contains("brain_receiver_public_url is unset on this machine"),
            "{message}"
        );
        // The machine-wide write is the exact fix and carries no selector.
        assert!(
            message.contains("brain env set brain_receiver_public_url="),
            "{message}"
        );
        assert!(
            !message.contains("brain env set -w"),
            "a machine-global write must not imply a workspace: {message}"
        );
        // Guided setup does carry one: it would otherwise collect the *default*
        // workspace's provider credentials for someone working in `family`.
        assert!(
            message.contains("brain receiver setup -w family"),
            "{message}"
        );
    }
}
