use super::{env_set_confirmation, masked_echo};

#[test]
fn masked_echo_uses_one_star_per_character() {
    assert_eq!(masked_echo("abc"), "***");
    assert_eq!(masked_echo("éx"), "**");
    assert_eq!(masked_echo(""), "");
}

#[test]
fn an_enum_env_prompt_lists_the_values_it_accepts() {
    // Human-friendly fallback: never make someone read `--help` to learn
    // what an enum variable takes.
    assert_eq!(
        super::env_value_prompt("default_agent_frontend"),
        "Set default_agent_frontend (claude | codex | opencode) = "
    );
    assert_eq!(super::env_value_prompt("claude_cmd"), "Set claude_cmd = ");
}

#[test]
fn a_machine_global_env_confirmation_says_it_landed_for_the_whole_machine() {
    let confirmation = env_set_confirmation(
        "brain_receiver_public_url",
        "https://brain.example.test",
        crate::theme::Theme::dark(false),
    );

    assert!(
        confirmation.contains("https://brain.example.test"),
        "{confirmation}"
    );
    assert!(confirmation.contains("machine-global"), "{confirmation}");
    // A workspace-scoped variable says nothing of the kind.
    assert!(
        !env_set_confirmation("claude_cmd", "claude", crate::theme::Theme::dark(false))
            .contains("machine-global")
    );
}

#[test]
fn sensitive_env_confirmation_never_contains_the_assigned_value() {
    let secret = "whsec_private-value";
    let confirmation = env_set_confirmation(
        "resend_webhook_signing_secret",
        secret,
        crate::theme::Theme::dark(false),
    );

    assert!(!confirmation.contains(secret));
    assert!(confirmation.contains("saved"));
}
