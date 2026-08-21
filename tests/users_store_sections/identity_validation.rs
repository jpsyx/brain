#[test]
fn phone_and_email_resolve_to_one_portable_user() {
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();

    assert_eq!(
        users.resolve_phone("(212) 555-0100").unwrap().id.as_str(),
        "pablo"
    );
    assert_eq!(
        users
            .resolve_email(" Wife@Example.COM ")
            .unwrap()
            .id
            .as_str(),
        "wife"
    );
    assert!(users.resolve_phone("+12125550101").is_none());
    assert!(users.resolve_email("wife+brain@example.com").is_none());
}

#[test]
fn identities_are_normalized_without_provider_specific_rewriting() {
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();
    let pablo = users.user(&UserId::parse("pablo").unwrap()).unwrap();
    let wife = users.user(&UserId::parse("wife").unwrap()).unwrap();

    assert_eq!(pablo.phones[0].value, "+12125550100");
    assert_eq!(pablo.phones[1].value, "+12125550101");
    assert_eq!(pablo.emails[0].value, "pablo+brain@example.com");
    assert_eq!(wife.emails[0].value, "wife@example.com");
    assert_eq!(wife.emails[1].value, "wife+brain@example.com");
}

#[test]
fn one_enabled_sender_cannot_identify_two_users() {
    let error = Users::parse(DUPLICATE_PHONE_FIXTURE.as_bytes()).unwrap_err();

    assert!(matches!(error, UsersError::DuplicateInboundPhone { .. }));
}

#[test]
fn invalid_user_ids_contacts_and_response_addresses_are_typed_errors() {
    for invalid in ["", "Pablo", "pablo_s", "-pablo", "pablo-"] {
        assert!(UserId::parse(invalid).is_err(), "{invalid:?}");
    }
    let ambiguous_phone = FIXTURE.replace("(212) 555-0100", "555-0100");
    assert!(matches!(
        Users::parse(ambiguous_phone.as_bytes()),
        Err(UsersError::InvalidPhone { .. })
    ));
    let foreign_without_prefix = FIXTURE.replace("(212) 555-0100", "442071838750");
    assert!(matches!(
        Users::parse(foreign_without_prefix.as_bytes()),
        Err(UsersError::InvalidPhone { .. })
    ));
    let missing_email = FIXTURE.replace(
        "\"response_email\": \"pablo+brain@example.com\"",
        "\"response_email\": \"other@example.com\"",
    );
    assert!(matches!(
        Users::parse(missing_email.as_bytes()),
        Err(UsersError::ResponseEmailNotOnUser { .. })
    ));
}
