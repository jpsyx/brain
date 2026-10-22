use super::{get_global, registry_backed_command, set};

#[test]
fn recipient_domain_is_validated_and_stored_once_for_the_machine() {
    let (_home, command) = registry_backed_command();
    set(&command, "resend_recipient_domain", " *.EXAMPLE.TEST ").unwrap();
    assert_eq!(
        get_global(&command.registry_store, "resend_recipient_domain").as_deref(),
        Some("*.example.test")
    );
    for invalid in [
        "*example.test",
        "https://example.test",
        "*.bad..test",
        "*.example.test@evil.test",
        "true",
    ] {
        assert!(
            set(&command, "resend_recipient_domain", invalid).is_err(),
            "{invalid}"
        );
        assert_eq!(
            get_global(&command.registry_store, "resend_recipient_domain").as_deref(),
            Some("*.example.test")
        );
    }
    set(&command, "resend_recipient_domain", "").unwrap();
    assert_eq!(
        get_global(&command.registry_store, "resend_recipient_domain").as_deref(),
        Some("")
    );
}
