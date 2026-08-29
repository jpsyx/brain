use super::AuthenticatedInbound;

pub(super) struct CompletionProof {
    pub(super) accepted_sender_is_canonical: bool,
    pub(super) envelope_sender_is_canonical: bool,
    pub(super) transcript_advanced: bool,
    pub(super) outbox_is_ready: bool,
    pub(super) cleanup_count: i64,
    pub(super) job_is_answer_ready: bool,
}

pub(super) fn complete_authenticated(
    authenticated: AuthenticatedInbound,
    canonical_sender: &str,
) -> CompletionProof {
    use crate::agent::SessionStore as _;
    use crate::state::{
        EmailLineage, ReceiverCompletionRequest, ReceiverConversationIdentity, ReceiverJobState,
        ReceiverNonterminalObservationPhase,
    };

    let workspace_id = crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("workspace ID");
    let user_id = crate::users::UserId::parse("test-user").expect("user ID");
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: user_id.clone(),
            name: "Test user".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: authenticated.sender.clone(),
                inbound_allowed: true,
            }],
            emails: vec![crate::users::EmailIdentity {
                value: authenticated.sender.clone(),
                inbound_allowed: true,
            }],
            response_email: None,
        }],
    };
    let request_identity = match authenticated.channel {
        super::Channel::Sms => crate::actor::RequestIdentity::Sms {
            from: &authenticated.sender,
        },
        super::Channel::Email => crate::actor::RequestIdentity::Email {
            from: &authenticated.sender,
        },
    };
    let actor = crate::actor::resolve_actor(&user_id, request_identity, &users)
        .expect("authenticated test actor");
    let channel = authenticated.channel;
    let prompt = authenticated.prompt.clone();
    let response_email = (channel == super::Channel::Email).then(|| authenticated.sender.clone());
    let job = crate::server::receiver::InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor: actor.clone(),
        channel,
        authenticated_sender: authenticated.sender,
        response_sender: authenticated.receiving_address,
        prompt: authenticated.prompt,
        attachments: authenticated.attachments,
        received_at_unix_ms: 100,
        provider_id: authenticated.provider_id,
        thread_participants: authenticated.participants,
        response_email,
        allowed_response_recipients: Vec::new(),
        email_reply: authenticated.email_reply,
    };
    let identity = match channel {
        super::Channel::Sms => ReceiverConversationIdentity::sms(workspace_id, user_id),
        super::Channel::Email => {
            ReceiverConversationIdentity::email(workspace_id, user_id, EmailLineage::Uncertain)
        }
    };
    let db = crate::state::Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept authenticated inbound job");
    let accepted_sender_is_canonical = db
        .receiver_job(accepted.job_id())
        .expect("load accepted job")
        .is_some_and(|stored| stored.inbound().response_sender == canonical_sender);
    let scope =
        crate::agent::SessionScope::new(crate::agent::AgentKind::Claude, workspace_id, actor);
    let pending =
        crate::agent::AgentSession::new("pending-http-completion").expect("pending session");
    let registration = db
        .register_receiver_session(
            accepted.conversation_id(),
            &pending,
            "http-completion-instance",
            42,
            &scope,
        )
        .expect("register completion session");
    db.claim_next_receiver_run("owner", 1_000, 2_000)
        .expect("claim authenticated job")
        .expect("authenticated job claim");
    assert!(
        db.prepare_receiver_job_launch(accepted.job_id(), "owner", 1_100)
            .expect("prepare authenticated job")
    );
    let token = db
        .receiver_job(accepted.job_id())
        .expect("load claimed job")
        .expect("claimed job")
        .token();
    assert!(
        db.commit_receiver_job_launch(
            accepted.job_id(),
            "owner",
            &crate::state::ReceiverLaunchObservation {
                token,
                instance: "http-completion-instance".to_owned(),
                session_id: pending.as_str().to_owned(),
                observed_at_unix_ms: 1_200,
                authorized_at_unix_ms: 1_200,
            },
        )
        .expect("commit authenticated launch")
    );
    for (phase, revision, at) in [
        (ReceiverNonterminalObservationPhase::Accepted, 1, 1_300),
        (ReceiverNonterminalObservationPhase::Progressing, 2, 1_400),
    ] {
        assert!(
            db.apply_receiver_observation(
                accepted.job_id(),
                "owner",
                &crate::state::ReceiverObservation {
                    token,
                    instance: "http-completion-instance".to_owned(),
                    session_id: pending.as_str().to_owned(),
                    phase,
                    revision,
                    observed_at_unix_ms: at,
                    authorized_at_unix_ms: at,
                },
            )
            .expect("commit authenticated observation")
        );
    }
    assert!(
        db.mark_completed(&pending, &scope)
            .expect("complete native session")
    );
    let answer = "exact authenticated answer";
    db.complete_receiver_job_with_binding(&ReceiverCompletionRequest {
        job_id: accepted.job_id(),
        token,
        owner: "owner",
        registration: &registration,
        completed_session: &pending,
        answer,
        observed_at_unix_ms: 1_500,
        authorized_at_unix_ms: 1_500,
    })
    .expect("complete authenticated job")
    .expect("exact completion authority");
    let transcript_advanced = db
        .receiver_conversation(accepted.conversation_id())
        .expect("load authenticated conversation")
        .is_some_and(|conversation| {
            crate::state::receiver_transcript_has_exact_turn(
                conversation.transcript_markdown(),
                &prompt,
                answer,
            )
        });
    let cleanup_count = i64::from(
        db.receiver_answer_cleanup(accepted.job_id())
            .expect("load answer cleanup")
            .is_some(),
    );
    let job_is_answer_ready = db
        .receiver_job(accepted.job_id())
        .expect("load completed authenticated job")
        .is_some_and(|stored| stored.state() == ReceiverJobState::AnswerReady);
    let delivery = db
        .claim_next_receiver_delivery("proof-owner", 1_600, 31_600)
        .expect("claim authenticated answer outbox");
    let envelope_sender_is_canonical = delivery
        .as_ref()
        .and_then(|claim| {
            claim
                .envelope()
                .sms()
                .map(crate::state::ReceiverSmsEnvelope::sender)
                .or_else(|| {
                    claim
                        .envelope()
                        .email()
                        .map(crate::state::ReceiverEmailEnvelope::sender)
                })
        })
        .is_some_and(|sender| sender == canonical_sender);
    CompletionProof {
        accepted_sender_is_canonical,
        envelope_sender_is_canonical,
        transcript_advanced,
        outbox_is_ready: delivery.is_some(),
        cleanup_count,
        job_is_answer_ready,
    }
}
