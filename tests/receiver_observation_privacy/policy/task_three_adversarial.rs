use super::discover_task_three_privacy_tests;
use super::task_three_assertions::private_whole_value_assertion_violations;

#[test]
fn open_block_assignment_is_private_at_the_inner_assertion() {
    let mutation = "let alias; if condition { alias = sender; assert_eq!(alias, expected); }";

    assert!(
        private_whole_value_assertion_violations(mutation) > 0,
        "open-block private assignment was accepted"
    );
}

#[test]
fn later_match_arm_binding_and_assignment_are_private_at_inner_assertions() {
    for mutation in [
        concat!(
            "match sender { _ if condition => (), alias => ",
            "assert_eq!(alias, expected), }"
        ),
        concat!(
            "let alias; match condition { true => {}, false => { alias = sender; ",
            "assert_eq!(alias, expected); } }"
        ),
    ] {
        assert!(
            private_whole_value_assertion_violations(mutation) > 0,
            "later match-arm private alias was accepted"
        );
    }
}

#[test]
fn deceptive_method_names_on_private_receivers_do_not_prove_content_free_values() {
    for mutation in [
        "assert_eq!(sender.count(), expected);",
        "assert_eq!(sender.private_count(), expected);",
        "assert_eq!(sender.private_len(), expected);",
        "assert_eq!(sender.private_length(), expected);",
        "assert_eq!(sender.uses_private(), expected);",
    ] {
        assert!(
            private_whole_value_assertion_violations(mutation) > 0,
            "deceptive private-receiver method was accepted"
        );
    }
}

#[test]
fn content_free_sounding_identifiers_do_not_bypass_value_flow() {
    for mutation in [
        "fn fixture(sender_count: String) { assert_eq!(sender_count, expected); }",
        "fn fixture(payload_len: String) { assert_eq!(payload_len, expected); }",
    ] {
        assert!(
            private_whole_value_assertion_violations(mutation) > 0,
            "content-free-looking private identifier was accepted"
        );
    }
}

#[test]
fn nested_app_filename_is_discovered_for_private_assertion_audit() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    let relative = "src/tui/app_brain/tests/future/app_delivery.rs";
    let path = temporary.path().join(relative);
    std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
    std::fs::write(&path, "#[test] fn fixture() {}\n").expect("fixture source");

    let discovered = discover_task_three_privacy_tests(temporary.path());

    assert!(
        discovered.iter().any(|path| path.ends_with(relative)),
        "nested App filename was not audited"
    );
}
