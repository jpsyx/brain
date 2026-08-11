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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::workspace::{
        MachineRegistry, REGISTRY_SCHEMA_VERSION, WorkspaceName, WorkspaceRecord,
    };

    const PERSONAL: &str = "3f1b9a2c-0d4e-4a6b-8c1d-2e3f4a5b6c7d";
    const FAMILY: &str = "5a2c8d3e-1f4b-4c7d-9e8f-0a1b2c3d4e5f";

    fn workspace_id(raw: &str) -> WorkspaceId {
        WorkspaceId::parse(raw).expect("workspace id fixture")
    }

    fn record(id: &str, env: serde_json::Map<String, serde_json::Value>) -> WorkspaceRecord {
        WorkspaceRecord {
            workspace_id: workspace_id(id),
            root: std::path::PathBuf::from("/tmp/brain"),
            aliases: BTreeSet::new(),
            local_user_id: "member".to_owned(),
            receiver_enabled: true,
            env,
        }
    }

    fn registry(records: &[(&str, &str, serde_json::Value)]) -> MachineRegistry {
        MachineRegistry {
            schema_version: REGISTRY_SCHEMA_VERSION,
            default_workspace: WorkspaceName::parse(records[0].0).expect("default name"),
            workspaces: records
                .iter()
                .map(|(name, id, env)| {
                    (
                        WorkspaceName::parse(name).expect("workspace name"),
                        record(id, env.as_object().cloned().expect("env object fixture")),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            env: serde_json::Map::new(),
        }
    }

    fn two_workspaces() -> MachineRegistry {
        registry(&[
            (
                "personal",
                PERSONAL,
                serde_json::json!({
                    "twilio_from_number": "+12125550100",
                    "resend_from_email": "personal@example.test",
                }),
            ),
            (
                "family",
                FAMILY,
                serde_json::json!({
                    "twilio_from_number": "+13105550111",
                    "resend_from_email": "Family Brain <FAMILY@example.test>",
                }),
            ),
        ])
    }

    #[test]
    fn each_channel_routes_on_the_address_its_receiver_publishes() {
        let registry = two_workspaces();

        assert_eq!(
            select_workspace(&registry, Channel::Sms, &["+13105550111".to_owned()]),
            ReceiverRoute::Workspace(workspace_id(FAMILY))
        );
        assert_eq!(
            select_workspace(
                &registry,
                Channel::Email,
                &["personal@example.test".to_owned()]
            ),
            ReceiverRoute::Workspace(workspace_id(PERSONAL))
        );
    }

    #[test]
    fn a_destination_matches_however_the_two_sides_were_written() {
        // Twilio sends E.164 and mail headers carry a display name; a human
        // typed the configured value. Both sides normalize before comparing.
        let registry = two_workspaces();

        assert_eq!(
            select_workspace(&registry, Channel::Sms, &["(212) 555-0100".to_owned()]),
            ReceiverRoute::Workspace(workspace_id(PERSONAL))
        );
        assert_eq!(
            select_workspace(
                &registry,
                Channel::Email,
                &["Someone <family@example.test>".to_owned()]
            ),
            ReceiverRoute::Workspace(workspace_id(FAMILY))
        );
    }

    #[test]
    fn any_named_destination_can_carry_the_route() {
        // An inbound email names several recipients; only one of them is a
        // workspace brain answers on.
        let registry = two_workspaces();

        assert_eq!(
            select_workspace(
                &registry,
                Channel::Email,
                &[
                    "someone-else@example.test".to_owned(),
                    "family@example.test".to_owned(),
                ]
            ),
            ReceiverRoute::Workspace(workspace_id(FAMILY))
        );
    }

    #[test]
    fn an_address_no_workspace_publishes_routes_nowhere() {
        let registry = two_workspaces();

        for destinations in [
            vec!["+14155550199".to_owned()],
            vec![String::new()],
            Vec::new(),
        ] {
            assert_eq!(
                select_workspace(&registry, Channel::Sms, &destinations),
                ReceiverRoute::Unknown,
                "{destinations:?}"
            );
        }
    }

    #[test]
    fn a_channel_routes_only_on_its_own_address() {
        // A phone number is never an email destination, however it is spelled.
        let registry = two_workspaces();

        assert_eq!(
            select_workspace(&registry, Channel::Email, &["+12125550100".to_owned()]),
            ReceiverRoute::Unknown
        );
    }

    #[test]
    fn two_workspaces_claiming_one_address_route_nowhere() {
        // Guessing here would hand one workspace another's private message.
        let registry = registry(&[
            (
                "personal",
                PERSONAL,
                serde_json::json!({"twilio_from_number": "+12125550100"}),
            ),
            (
                "family",
                FAMILY,
                serde_json::json!({"twilio_from_number": "(212) 555-0100"}),
            ),
        ]);

        assert_eq!(
            select_workspace(&registry, Channel::Sms, &["+12125550100".to_owned()]),
            ReceiverRoute::Ambiguous
        );
    }

    #[test]
    fn a_workspace_with_no_configured_address_is_never_the_fallback() {
        let registry = registry(&[
            ("personal", PERSONAL, serde_json::json!({})),
            (
                "family",
                FAMILY,
                serde_json::json!({"twilio_from_number": "   "}),
            ),
        ]);

        assert_eq!(
            select_workspace(&registry, Channel::Sms, &["+12125550100".to_owned()]),
            ReceiverRoute::Unknown
        );
    }
}
