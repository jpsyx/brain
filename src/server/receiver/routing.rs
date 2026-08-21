//! Pure selection of one workspace from the destination a provider names.
//!
//! Every workspace's provider portal points at the same machine-wide `/sms` and
//! `/email` URL, so the workspace is chosen by the number or address the
//! message arrived at rather than by anything in the path.
//!
//! The payload is still unverified here, and that is safe: the destination only
//! decides *whose* signing credential the request is checked against. A request
//! whose signature does not match the selected workspace's own credential is
//! rejected exactly as before, so naming another workspace's address buys an
//! attacker nothing.

use super::Channel;
use crate::workspace::{MachineRegistry, WorkspaceId};

/// The env variable holding the address one channel's receiver answers on.
///
/// It is both the sender of outbound replies and, now, the routing key inbound
/// traffic is matched against, so there is exactly one value to keep correct.
#[must_use]
pub(crate) const fn address_var(channel: Channel) -> &'static str {
    match channel {
        Channel::Sms => "twilio_from_number",
        Channel::Email => "resend_from_email",
    }
}

/// Reduce a raw provider-supplied address to the form addresses compare in.
///
/// Providers send an E.164 number or an RFC 5322 mailbox; a configured value is
/// whatever a human typed into setup. Both sides pass through here so the match
/// never depends on case or on a display name.
#[must_use]
pub(crate) fn normalize_address(channel: Channel, raw: &str) -> Option<String> {
    match channel {
        Channel::Sms => crate::users::normalize_phone(raw).ok(),
        Channel::Email => crate::users::normalize_mailbox(raw).ok(),
    }
}

/// Which workspace an inbound message's destination belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverRoute {
    /// Exactly one registered workspace publishes one of the named addresses.
    Workspace(WorkspaceId),
    /// Several workspaces publish the same address, so no answer is safe:
    /// delivering to either would hand one workspace another's message.
    Ambiguous,
    /// No registered workspace publishes any of the named addresses.
    Unknown,
}

/// Select the workspace that published one of `destinations`. Pure.
///
/// Receiver intent is deliberately not consulted: a disabled workspace must
/// still be *found*, so the request gets that workspace's unavailable answer
/// instead of looking like a URL nobody serves.
#[must_use]
pub(crate) fn select_workspace(
    registry: &MachineRegistry,
    channel: Channel,
    destinations: &[String],
) -> ReceiverRoute {
    let wanted = destinations
        .iter()
        .filter_map(|destination| normalize_address(channel, destination))
        .collect::<Vec<_>>();
    if wanted.is_empty() {
        return ReceiverRoute::Unknown;
    }
    let mut matched = registry
        .workspaces
        .values()
        .filter(|record| {
            record
                .env
                .get(address_var(channel))
                .and_then(serde_json::Value::as_str)
                .and_then(|published| normalize_address(channel, published))
                .is_some_and(|published| wanted.contains(&published))
        })
        .map(|record| record.workspace_id);
    let Some(first) = matched.next() else {
        return ReceiverRoute::Unknown;
    };
    // Two records claiming one address is a misconfiguration, not a preference.
    if matched.any(|other| other != first) {
        return ReceiverRoute::Ambiguous;
    }
    ReceiverRoute::Workspace(first)
}

/// Longest an untrusted destination may be before it is cut short in the log.
const LOGGED_ADDRESS_LIMIT: usize = 120;

/// Render one untrusted address safely for a single log line.
fn loggable(raw: &str) -> String {
    let mut safe = raw
        .chars()
        .take(LOGGED_ADDRESS_LIMIT)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if raw.chars().count() > LOGGED_ADDRESS_LIMIT {
        safe.push('…');
    }
    safe
}

/// Why a message routed nowhere, phrased for the machine owner's log.
///
/// The provider only ever gets an empty 404, so a prober cannot learn which
/// addresses this machine serves. That makes the local log the one place the
/// owner can find out what went wrong, and a bare "not found" leaves them
/// guessing at a value they cannot see. Naming the address that arrived beside
/// the ones actually configured turns it into a one-line fix.
#[must_use]
pub(crate) fn unrouted_explanation(
    registry: &MachineRegistry,
    channel: Channel,
    destinations: &[String],
) -> String {
    let variable = address_var(channel);
    let named = destinations
        .iter()
        .map(|destination| {
            normalize_address(channel, destination).unwrap_or_else(|| loggable(destination))
        })
        .collect::<Vec<_>>();
    if named.is_empty() {
        return format!("request carried no destination address to route on ({variable})");
    }
    let configured = registry
        .workspaces
        .iter()
        .map(|(name, record)| {
            let published = record
                .env
                .get(variable)
                .and_then(serde_json::Value::as_str)
                .and_then(|published| normalize_address(channel, published));
            published.map_or_else(
                || format!("{name}=<unset>"),
                |address| format!("{name}={address}"),
            )
        })
        .collect::<Vec<_>>();
    format!(
        "no workspace publishes {} as its {variable}; this machine has [{}]. \
         Point the provider at a configured address, or run \
         `brain env set -w <workspace> {variable}=<address>`",
        named.join(", "),
        configured.join(", "),
    )
}

#[cfg(test)]
mod tests;
