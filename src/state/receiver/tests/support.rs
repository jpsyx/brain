pub(super) fn receiver_workspace_id() -> crate::workspace::WorkspaceId {
    crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("valid workspace ID")
}

pub(super) fn receiver_user_id() -> crate::users::UserId {
    crate::users::UserId::parse("test-user").expect("valid portable user ID")
}

fn receiver_actor(channel: crate::server::receiver::Channel) -> crate::actor::ActorContext {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: receiver_user_id(),
            name: "Test user".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: vec![crate::users::EmailIdentity {
                value: "sender@example.test".to_owned(),
                inbound_allowed: true,
            }],
            response_email: None,
        }],
    };
    let request = match channel {
        crate::server::receiver::Channel::Sms => crate::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        crate::server::receiver::Channel::Email => crate::actor::RequestIdentity::Email {
            from: "sender@example.test",
        },
    };
    crate::actor::resolve_actor(&receiver_user_id(), request, &users).expect("receiver actor")
}

pub(super) fn receiver_job_for(
    workspace_id: crate::workspace::WorkspaceId,
    channel: crate::server::receiver::Channel,
    provider_id: Option<&str>,
    received_at_unix_ms: u64,
) -> crate::server::receiver::InboundJob {
    let sender = match channel {
        crate::server::receiver::Channel::Sms => "+12125550100",
        crate::server::receiver::Channel::Email => "sender@example.test",
    };
    crate::server::receiver::InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor: receiver_actor(channel),
        channel,
        authenticated_sender: sender.to_owned(),
        prompt: "Remember the durable receiver job".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms,
        provider_id: provider_id.map(str::to_owned),
        thread_participants: vec![sender.to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

pub(super) fn receiver_job(
    provider_id: Option<&str>,
    received_at_unix_ms: u64,
) -> crate::server::receiver::InboundJob {
    receiver_job_for(
        receiver_workspace_id(),
        crate::server::receiver::Channel::Sms,
        provider_id,
        received_at_unix_ms,
    )
}
