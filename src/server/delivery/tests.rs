use super::*;

fn users() -> crate::users::Users {
    crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("member").unwrap(),
            name: "Member".to_owned(),
            phones: Vec::new(),
            emails: vec![crate::users::EmailIdentity {
                value: "member@example.test".to_owned(),
                inbound_allowed: true,
            }],
            response_email: Some("member@example.test".to_owned()),
        }],
    }
}

fn actor() -> crate::actor::ActorContext {
    crate::actor::resolve_actor(
        &crate::users::UserId::parse("member").unwrap(),
        crate::actor::RequestIdentity::Local,
        &users(),
    )
    .unwrap()
}

#[test]
fn provider_delivery_runs_off_the_tui_thread() {
    let started = std::time::Instant::now();
    dispatch_background("test delivery", || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        Ok(())
    })
    .unwrap();

    assert!(
        started.elapsed() < std::time::Duration::from_millis(250),
        "dispatch waited for the provider request"
    );
}

#[test]
fn thread_delivery_intersects_participants_and_allowlist() {
    let recipients = allowed_thread_recipients(
        &[
            "Me@Example.com".to_owned(),
            "other@example.com".to_owned(),
            "new@example.com".to_owned(),
        ],
        &["me@example.com".to_owned(), "other@example.com".to_owned()],
        "me@example.com",
    );
    assert!(
        recipients == ["other@example.com"],
        "thread recipient intersection changed"
    );
}

#[test]
fn trusted_recipients_apply_the_same_address_rule_as_the_thread_intersection() {
    let recipients = trusted_response_recipients(
        Some("Member <Member@Example.test>"),
        &["thread@example.test".to_owned()],
    );

    assert!(
        recipients == ["member@example.test", "thread@example.test"],
        "one address rule must decide the reply, not two"
    );
}

#[test]
fn a_configured_from_address_with_a_display_name_is_still_never_echoed_back() {
    let recipients = allowed_thread_recipients(
        &[
            "other@example.com".to_owned(),
            "brain@example.com".to_owned(),
        ],
        &[
            "other@example.com".to_owned(),
            "brain@example.com".to_owned(),
        ],
        "Brain <Brain@Example.com>",
    );

    assert!(
        recipients == ["other@example.com"],
        "the receiving address must be excluded however it is configured"
    );
}

#[test]
fn response_recipients_are_derived_from_the_immutable_actor() {
    let recipients = actor_thread_recipients(
        &[
            "member@example.test".to_owned(),
            "another@example.test".to_owned(),
        ],
        &users(),
        &actor(),
        "brain@example.test",
    );
    assert!(
        recipients == ["member@example.test"],
        "immutable actor recipients changed"
    );
}

#[test]
fn processing_and_final_email_use_acceptance_time_recipients_subject_and_lineage() {
    let reply = crate::server::receiver::EmailReplyContext {
        provider_email_id: "provider-email".to_owned(),
        subject: "Quarterly planning".to_owned(),
        message_id: Some("<message@example.test>".to_owned()),
    };

    let accepted_recipients = vec![
        "member@example.test".to_owned(),
        "thread@example.test".to_owned(),
    ];
    for message in ["Still working", "Final answer"] {
        let payload = email_payload(
            "brain@example.test",
            &accepted_recipients,
            &reply_subject(Some(&reply)),
            message,
            &format!("<p>{message}</p>"),
            reply.message_id.as_deref(),
        );
        assert!(
            payload["to"] == serde_json::json!(accepted_recipients),
            "accepted email recipients changed"
        );
        assert!(
            payload["subject"] == "Re: Quarterly planning",
            "accepted email subject changed"
        );
        assert!(
            payload["headers"]["In-Reply-To"] == "<message@example.test>",
            "accepted email reply lineage changed"
        );
        assert!(
            payload["headers"]["References"] == "<message@example.test>",
            "accepted email reference lineage changed"
        );
    }
}

mod executor;
mod provider_attempt;
