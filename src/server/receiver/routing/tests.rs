use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::workspace::{MachineRegistry, REGISTRY_SCHEMA_VERSION, WorkspaceName, WorkspaceRecord};

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

/// A provider gets an empty 404 so a prober learns nothing, which leaves
/// the local log as the only place the owner can find out *why*. Naming the
/// address that arrived beside the addresses actually configured turns a
/// bare 404 into an obvious one-line fix.
#[test]
fn an_unrouted_address_is_explained_against_what_is_configured() {
    let registry = two_workspaces();
    let explanation = unrouted_explanation(
        &registry,
        Channel::Email,
        &["brain@old-domain.test".to_owned()],
    );

    assert!(
        explanation.contains("brain@old-domain.test"),
        "must name the address that arrived: {explanation}"
    );
    assert!(
        explanation.contains("personal@example.test")
            && explanation.contains("family@example.test"),
        "must name every configured address so the mismatch is visible: {explanation}"
    );
    assert!(
        explanation.contains("resend_from_email"),
        "must name the variable to change: {explanation}"
    );
}

/// A payload carrying no destination at all is a different fault from one
/// naming an address nobody serves, and must not be reported as a mismatch.
#[test]
fn a_payload_with_no_destination_says_so_rather_than_blaming_configuration() {
    let explanation = unrouted_explanation(&two_workspaces(), Channel::Sms, &[]);
    assert!(
        explanation.contains("no destination"),
        "must say the payload named nothing: {explanation}"
    );
}

/// The destination is attacker-supplied and unverified at this point, so it
/// must not be able to forge log lines or flood the log.
#[test]
fn an_untrusted_destination_cannot_forge_or_flood_log_lines() {
    let explanation = unrouted_explanation(
        &two_workspaces(),
        Channel::Email,
        &[format!("a@b.test\nreceiver forged line{}", "x".repeat(500))],
    );
    assert!(
        !explanation.contains('\n'),
        "log line was broken: {explanation}"
    );
    assert!(explanation.len() < 600, "unbounded log line: {explanation}");
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
