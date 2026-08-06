#[test]
fn rejects_missing_fields() {
    assert!(validate("", "k", "a").is_err());
    assert!(validate("b", "", "a").is_err());
    assert!(validate("b", "k", "").is_err());
    assert!(validate("b", "k", "a").is_ok());
}

#[test]
fn parse_yes_no_reads_affirmatives_only() {
    assert!(parse_yes_no("y"));
    assert!(parse_yes_no("Yes"));
    assert!(parse_yes_no("  YES  "));
    assert!(!parse_yes_no("n"));
    assert!(!parse_yes_no("no"));
    assert!(!parse_yes_no("")); // default: no bucket yet → show the walkthrough
    assert!(!parse_yes_no("maybe"));
}

#[test]
fn walkthrough_covers_the_critical_bucket_settings() {
    let w = bucket_walkthrough();
    assert!(w.contains("Private"), "must say the bucket is Private");
    assert!(
        w.contains("Default Encryption") && w.contains("Enable"),
        "must tell them to Enable Default Encryption"
    );
    assert!(
        w.contains("Object Lock") && w.contains("Disable"),
        "must tell them to Disable Object Lock"
    );
    assert!(
        w.contains("Application Key"),
        "must cover creating an application key"
    );
    assert!(
        w.contains("keyID") && w.contains("applicationKey"),
        "must name both credential values to copy"
    );
}

#[test]
fn intro_says_setup_enables_cloud_sync() {
    let intro = setup_intro(Theme::dark(false));
    assert!(intro.contains("This will enable cloud sync"), "{intro}");
    assert!(intro.contains("brain sync setup"), "{intro}");
    assert!(
        intro.contains("verify the remote workspace identity"),
        "{intro}"
    );
}
