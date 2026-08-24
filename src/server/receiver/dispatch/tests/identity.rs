use super::super::pipeline::conversation_identity;
use crate::server::receiver::{Channel, EmailReplyContext, InboundJob};

#[test]
fn sms_ingress_uses_one_stable_workspace_user_conversation() {
    let first = conversation_identity(&job(Channel::Sms, "provider-sms-1"));
    let second = conversation_identity(&job(Channel::Sms, "provider-sms-2"));

    assert_eq!(first, second);
}

#[test]
fn resend_subject_and_individual_message_id_never_merge_email_conversations() {
    let first = conversation_identity(&job(Channel::Email, "provider-email-1"));
    let second = conversation_identity(&job(Channel::Email, "provider-email-2"));

    assert_ne!(first, second);
}

#[test]
fn resend_provider_retry_keeps_the_original_uncertain_conversation() {
    let db = crate::state::Db::open_in_memory().expect("receiver state");
    let original = job(Channel::Email, "provider-email-retry");
    let mut retry = job(Channel::Email, "provider-email-retry");
    retry.prompt = "retry must not replace the accepted message".to_owned();
    let original_identity = conversation_identity(&original);
    let retry_identity = conversation_identity(&retry);
    assert_ne!(original_identity, retry_identity);

    let first = db
        .accept_receiver_job(&original, &original_identity)
        .expect("accept original Email");
    let second = db
        .accept_receiver_job(&retry, &retry_identity)
        .expect("deduplicate Email retry");

    assert!(first.was_inserted());
    assert!(!second.was_inserted());
    assert_eq!(second.job_id(), first.job_id());
    assert_eq!(second.conversation_id(), first.conversation_id());
    assert_eq!(
        db.receiver_job(first.job_id())
            .expect("load original Email")
            .expect("original durable Email")
            .inbound(),
        &original
    );
}

fn job(channel: Channel, provider_id: &str) -> InboundJob {
    let user_id = crate::users::UserId::parse("member").expect("portable user ID");
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: user_id.clone(),
            name: "Member".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: vec![crate::users::EmailIdentity {
                value: "member@example.test".to_owned(),
                inbound_allowed: true,
            }],
            response_email: None,
        }],
    };
    let request = match channel {
        Channel::Sms => crate::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        Channel::Email => crate::actor::RequestIdentity::Email {
            from: "member@example.test",
        },
    };
    let actor = crate::actor::resolve_actor(&user_id, request, &users).expect("receiver actor");
    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
            .expect("workspace ID"),
        actor,
        channel,
        authenticated_sender: match channel {
            Channel::Sms => "+12125550100",
            Channel::Email => "member@example.test",
        }
        .to_owned(),
        prompt: "receiver prompt".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 1_786_000_000_000,
        provider_id: Some(provider_id.to_owned()),
        thread_participants: Vec::new(),
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: (channel == Channel::Email).then(|| EmailReplyContext {
            provider_email_id: "resend-email-1".to_owned(),
            subject: "A subject is not lineage".to_owned(),
            message_id: Some("individual-message-id".to_owned()),
        }),
    }
}
