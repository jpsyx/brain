use super::{MappingChoice, apply_user_mapping, interactive_fields, mapping_choice};
use crate::cli::ReceiverSetupChannels;
use crate::users::{EmailIdentity, USERS_SCHEMA_VERSION, User, UserId, Users};

fn existing_users() -> Users {
    Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: UserId::parse("pablo").unwrap(),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: vec![EmailIdentity {
                value: "pablo@example.test".to_owned(),
                inbound_allowed: false,
            }],
            response_email: None,
        }],
    }
}

#[test]
fn existing_user_mapping_updates_the_exact_address_allowed_state() {
    let users = apply_user_mapping(
        existing_users(),
        "pablo",
        None,
        None,
        Some(("Pablo@Example.TEST".to_owned(), true)),
        None,
    )
    .unwrap();

    let pablo = users.user(&UserId::parse("pablo").unwrap()).unwrap();
    assert_eq!(pablo.emails.len(), 1);
    assert_eq!(pablo.emails[0].value, "pablo@example.test");
    assert!(pablo.emails[0].inbound_allowed);
}

#[test]
fn new_user_mapping_requires_a_name_and_preserves_disallowed_phone_state() {
    assert!(
        apply_user_mapping(
            Users::empty(),
            "alex-smith",
            None,
            Some(("646-555-0102".to_owned(), false)),
            None,
            None,
        )
        .is_err()
    );

    let users = apply_user_mapping(
        Users::empty(),
        "alex-smith",
        Some("Alex Smith"),
        Some(("646-555-0102".to_owned(), false)),
        None,
        None,
    )
    .unwrap();
    let alex = users.user(&UserId::parse("alex-smith").unwrap()).unwrap();
    assert_eq!(alex.name, "Alex Smith");
    assert_eq!(alex.phones[0].value, "+16465550102");
    assert!(!alex.phones[0].inbound_allowed);
}

#[test]
fn interactive_mapping_selects_existing_or_create_and_prompts_only_for_selected_channels() {
    let users = existing_users();
    assert_eq!(
        mapping_choice(&users, &UserId::parse("pablo").unwrap()),
        MappingChoice::Existing
    );
    assert_eq!(
        mapping_choice(&users, &UserId::parse("alex").unwrap()),
        MappingChoice::Create
    );
    assert_eq!(
        interactive_fields(ReceiverSetupChannels::Sms),
        ["phone", "phone-allowed"]
    );
    assert_eq!(
        interactive_fields(ReceiverSetupChannels::Email),
        ["email", "email-allowed"]
    );
    assert_eq!(
        interactive_fields(ReceiverSetupChannels::Both),
        ["phone", "phone-allowed", "email", "email-allowed"]
    );
}
