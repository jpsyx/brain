use brain::actor::{ActorContext, ActorError, Channel, RequestIdentity, resolve_actor};
use brain::users::{EmailIdentity, PhoneIdentity, USERS_SCHEMA_VERSION, User, UserId, Users};

fn user(id: &str, name: &str, phones: &[(&str, bool)], emails: &[(&str, bool)]) -> User {
    User {
        id: UserId::parse(id).expect("valid fixture user id"),
        name: name.to_owned(),
        phones: phones
            .iter()
            .map(|(value, inbound_allowed)| PhoneIdentity {
                value: (*value).to_owned(),
                inbound_allowed: *inbound_allowed,
            })
            .collect(),
        emails: emails
            .iter()
            .map(|(value, inbound_allowed)| EmailIdentity {
                value: (*value).to_owned(),
                inbound_allowed: *inbound_allowed,
            })
            .collect(),
        response_email: None,
    }
}

fn family_users() -> Users {
    Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![
            user(
                "pablo",
                "Pablo",
                &[("+12125550100", true)],
                &[("pablo@example.test", true)],
            ),
            user(
                "wife",
                "Partner",
                &[("+12125550101", true), ("+12125550102", false)],
                &[
                    ("partner@example.test", true),
                    ("disabled@example.test", false),
                ],
            ),
        ],
    }
}

fn local(id: &str) -> UserId {
    UserId::parse(id).expect("valid local fixture user id")
}

#[test]
fn inbound_sender_overrides_the_machine_local_user() {
    let actor = resolve_actor(
        &local("pablo"),
        RequestIdentity::Sms {
            from: "+12125550101",
        },
        &family_users(),
    )
    .unwrap();
    assert_eq!(actor.user_id().as_str(), "wife");
    assert_eq!(actor.channel(), Channel::Sms);
}

#[test]
fn inbound_email_overrides_the_machine_local_user() {
    let actor = resolve_actor(
        &local("pablo"),
        RequestIdentity::Email {
            from: "Partner@Example.Test",
        },
        &family_users(),
    )
    .unwrap();
    assert_eq!(actor.user_id().as_str(), "wife");
    assert_eq!(actor.channel(), Channel::Email);
}

#[test]
fn terminal_request_uses_local_user() {
    let actor = resolve_actor(&local("pablo"), RequestIdentity::Local, &family_users()).unwrap();
    assert_eq!(actor.user_id().as_str(), "pablo");
    assert_eq!(actor.channel(), Channel::Interactive);
}

#[test]
fn unknown_inbound_sender_is_rejected() {
    assert!(matches!(
        resolve_actor(
            &local("pablo"),
            RequestIdentity::Sms {
                from: "+12125559999"
            },
            &family_users()
        ),
        Err(ActorError::UnknownOrDisallowedSender)
    ));
}

#[test]
fn disabled_inbound_senders_are_rejected() {
    for request in [
        RequestIdentity::Sms {
            from: "+12125550102",
        },
        RequestIdentity::Email {
            from: "disabled@example.test",
        },
    ] {
        assert!(matches!(
            resolve_actor(&local("pablo"), request, &family_users()),
            Err(ActorError::UnknownOrDisallowedSender)
        ));
    }
}

#[test]
fn follow_up_retains_the_initiating_actor_when_machine_default_differs() {
    let initiating = resolve_actor(
        &local("pablo"),
        RequestIdentity::Sms {
            from: "+12125550101",
        },
        &family_users(),
    )
    .unwrap();

    let follow_up = ActorContext::follow_up(&initiating);

    assert_eq!(follow_up.user_id().as_str(), "wife");
    assert_eq!(follow_up.channel(), Channel::Sms);
}

#[test]
fn legacy_workspace_without_portable_users_resolves_its_local_actor() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("legacy-brain");
    std::fs::create_dir_all(&root).unwrap();
    let workspace = brain::workspace::WorkspaceContext::new(
        temp.path(),
        brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        brain::workspace::WorkspaceName::parse("legacy").unwrap(),
        &root,
        "pablo",
        temp.path(),
    )
    .unwrap();

    let actor = brain::actor::local_actor(&workspace).expect("legacy local actor compatibility");

    assert_eq!(actor.user_id().as_str(), "pablo");
    assert_eq!(actor.display_name(), "pablo");
    assert_eq!(actor.channel(), Channel::Interactive);
    assert!(!root.join(".config/users.json").exists());
}
