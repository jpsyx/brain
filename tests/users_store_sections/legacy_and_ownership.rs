
#[test]
fn inactive_legacy_helper_proposes_the_first_user_without_guessing_other_people() {
    let proposal = propose_legacy_user_migration(
        "Alex Smith",
        None,
        " Alex@Example.COM ",
        &["+12125550100".to_owned()],
        &[
            "alex@example.com".to_owned(),
            "relative@example.com".to_owned(),
        ],
    )
    .unwrap();

    assert_eq!(proposal.user.id.as_str(), "alex-smith");
    assert_eq!(proposal.user.name, "Alex Smith");
    assert_eq!(
        proposal.user.response_email.as_deref(),
        Some("alex@example.com")
    );
    assert_eq!(proposal.user.emails.len(), 1);
    assert!(proposal.user.emails[0].inbound_allowed);
    assert_eq!(proposal.unresolved_phones, ["+12125550100"]);
    assert_eq!(proposal.unresolved_emails, ["relative@example.com"]);

    let overridden =
        propose_legacy_user_migration("Alex Smith", Some("alex"), "", &[], &[]).unwrap();
    assert_eq!(overridden.user.id.as_str(), "alex");
}

#[test]
fn legacy_response_email_without_an_allowlist_match_stays_unresolved() {
    let proposal = propose_legacy_user_migration(
        "Alex Smith",
        None,
        "response@example.com",
        &[],
        &["relative@example.com".to_owned()],
    )
    .unwrap();

    assert!(proposal.user.response_email.is_none());
    assert!(proposal.user.emails.is_empty());
    assert_eq!(
        proposal.unresolved_emails,
        ["relative@example.com", "response@example.com"]
    );
}

#[cfg(unix)]
#[test]
fn grouped_removal_preserves_owner_only_users_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = CliFixture::new();
    let users_path = UsersStore::path(&workspace(&fixture.root));
    std::fs::set_permissions(&users_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let removed = fixture.run(&[
        "user",
        "remove",
        "pablo",
        "--reassign-to",
        "wife",
        "-b",
        "family",
    ]);

    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let mode = std::fs::metadata(users_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
