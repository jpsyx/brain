use std::path::Path;

use super::debug_impl::{item_automatically_derives_debug, manual_debug_delegates_content};

#[test]
fn content_bearing_receiver_types_cannot_derive_debug() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (relative, types) in [
        (
            "src/server/receiver/job.rs",
            &["AttachmentRef", "EmailReplyContext", "InboundJob"] as &[&str],
        ),
        ("src/server/receiver/attachments.rs", &["StagedAttachment"]),
        ("src/server/receiver/admission.rs", &["ReceiverAdmission"]),
        ("src/server/receiver/control.rs", &["RestartPlan"]),
        (
            "src/server/receiver/http/mod.rs",
            &["ProviderConfig", "AuthenticatedInbound"],
        ),
        ("src/server/receiver/dispatch.rs", &["DispatchHttpError"]),
        ("src/server/receiver/routing.rs", &["ReceiverRoute"]),
        (
            "src/state/receiver/identity.rs",
            &["EmailLineage", "ReceiverConversationIdentity"],
        ),
        (
            "src/state/receiver/model/claim.rs",
            &["ReceiverRunClaim", "ReceiverClaim"],
        ),
        (
            "src/state/receiver/model/conversation.rs",
            &[
                "ReceiverSessionBinding",
                "ReceiverSessionPlan",
                "ReceiverConversation",
            ],
        ),
        (
            "src/state/receiver/model/effect.rs",
            &["ReceiverReconciliationEffect"],
        ),
        (
            "src/state/receiver/model/identity.rs",
            &["ReceiverSessionAttribution"],
        ),
        ("src/state/receiver/model/job.rs", &["ReceiverJob"]),
        (
            "src/state/receiver/model/observation.rs",
            &[
                "ReceiverLaunchObservation",
                "ReceiverObservation",
                "ReceiverCompletionRequest",
            ],
        ),
    ] {
        let source = std::fs::read_to_string(root.join(relative)).expect("receiver source");
        for type_name in types {
            assert!(
                !item_automatically_derives_debug(&source, type_name),
                "{type_name} must use a content-free manual Debug implementation"
            );
        }
    }
}

#[test]
fn adjacent_receiver_reachable_types_have_content_free_debug_implementations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (relative, types) in [
        (
            "src/agent/observation.rs",
            &["AgentObservationResult"] as &[&str],
        ),
        (
            "src/agent/session.rs",
            &["AgentSession", "SessionScope", "SessionPlan"],
        ),
        ("src/agent/frontend.rs", &["LaunchRequest"]),
        ("src/server/reply/mod.rs", &["ReplyEnvelope"]),
    ] {
        let source = std::fs::read_to_string(root.join(relative)).expect("adjacent source");
        for type_name in types {
            assert!(
                !item_automatically_derives_debug(&source, type_name),
                "adjacent receiver type must not derive Debug"
            );
            assert!(
                !manual_debug_delegates_content(&source, type_name),
                "adjacent receiver type delegates content through Debug"
            );
        }
    }
}

#[test]
fn debug_policy_rejects_multiline_derives_and_nested_delegation() {
    let multiline = r"
        #[derive(
            Clone,
            Debug,
            PartialEq,
        )]
        struct AgentSession(String);
    ";
    let delegated = r#"
        struct AgentSession(String);
        impl std::fmt::Debug for AgentSession {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.debug_tuple("AgentSession").field(&self.0).finish()
            }
        }
    "#;

    assert!(
        item_automatically_derives_debug(multiline, "AgentSession"),
        "multiline Debug derive mutation was accepted"
    );
    assert!(
        manual_debug_delegates_content(delegated, "AgentSession"),
        "nested Debug delegation mutation was accepted"
    );
}
