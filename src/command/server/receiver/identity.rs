//! `brain receiver email` / `brain receiver phone`: the address a workspace's
//! receiver answers on.
//!
//! Both print the bare configured value on stdout so a script or an agent can
//! read it without parsing a status block. `brain receiver status` still
//! reports presence only; asking for the address is the explicit request that
//! makes printing it the right answer.

use anyhow::Result;

use crate::server::receiver::Channel;
use crate::workspace::CommandContext;

/// The brain-env variable that holds one channel's configured address. Pure.
pub(crate) const fn address_var(channel: Channel) -> &'static str {
    match channel {
        Channel::Sms => "twilio_from_number",
        Channel::Email => "resend_from_email",
    }
}

/// What one channel's address is called in user-facing output. Pure.
pub(crate) const fn address_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Sms => "Phone",
        Channel::Email => "Email",
    }
}

/// A stored env value reduced to a real address, or `None`. Pure.
///
/// A blank value is stored whenever setup skips a channel, so an empty string
/// means "unset" exactly as a missing key does.
#[must_use]
pub(crate) fn normalize_address(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The configured address for one channel of a workspace's receiver.
#[must_use]
pub(crate) fn address(context: &CommandContext, channel: Channel) -> Option<String> {
    normalize_address(crate::env::get(context, address_var(channel)))
}

/// What to say when a channel has no configured address.
///
/// Names the variable and both ways to set it, since the value is machine-local
/// and a peer machine having it does not help here. Pure.
#[must_use]
pub(crate) fn missing_address(workspace: &str, channel: Channel) -> String {
    let variable = address_var(channel);
    let noun = address_label(channel).to_lowercase();
    format!(
        "{variable} is unset for workspace {workspace}, so its receiver has no {noun} yet.\n  \
         fix: brain receiver setup -w {workspace}\n  \
         or:  brain env set -w {workspace} {variable}=<{noun}>"
    )
}

/// `brain receiver email` / `brain receiver phone`.
pub(super) fn run(context: &CommandContext, channel: Channel) -> Result<()> {
    let workspace = context.workspace.name().as_str().to_owned();
    let Some(address) = address(context, channel) else {
        anyhow::bail!(missing_address(&workspace, channel));
    };
    // Bare value, unstyled: the point of naming a channel is to pipe the answer.
    println!("{address}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Channel, address_label, address_var, missing_address};

    #[test]
    fn each_channel_reads_its_own_provider_address_variable() {
        assert_eq!(address_var(Channel::Sms), "twilio_from_number");
        assert_eq!(address_var(Channel::Email), "resend_from_email");
        assert_eq!(address_label(Channel::Sms), "Phone");
        assert_eq!(address_label(Channel::Email), "Email");
    }

    #[test]
    fn a_blank_stored_value_reads_as_unset() {
        // Setup writes an empty string for a channel it skipped, so a present
        // key is not the same as a configured address.
        assert_eq!(super::normalize_address(Some("   ".to_owned())), None);
        assert_eq!(super::normalize_address(None), None);
        assert_eq!(
            super::normalize_address(Some("  brain@example.test\n".to_owned())),
            Some("brain@example.test".to_owned())
        );
    }

    #[test]
    fn a_missing_address_names_the_variable_and_both_ways_to_set_it() {
        let message = missing_address("family", Channel::Email);

        assert!(
            message.contains("resend_from_email is unset for workspace family"),
            "{message}"
        );
        assert!(
            message.contains("brain receiver setup -w family"),
            "{message}"
        );
        assert!(
            message.contains("brain env set -w family resend_from_email="),
            "{message}"
        );
    }
}
